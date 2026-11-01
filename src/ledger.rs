//! Bounded ledger replay, frozen receipts, selected confirmation and explicit
//! SDK baseline activation/recovery. No task mutation, projection repair or
//! delivery pipeline is enabled. Invalid authority never becomes snapshot.
mod activation;
mod confirmation;
mod publication;
use crate::{
    Error, Result, canonical,
    error::require,
    handoff, read_fs,
    records::{self, TaskRecords},
    values::{self, Number, Object, Profile, Value},
};
pub use activation::{
    ActivationDurability, ActivationReport, ArchiveDurability, ArchiveReport, ImportFailure,
    ImportProgress, abort_import, import_tasks, resume_import,
};
pub use confirmation::{LedgerConfirmation, confirm_selected};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::Path,
};

pub const MARKER: &str = "state/task-authority.json";
pub const DIRECTORY: &str = "state/task-ledger";
pub const MAX_FRAMES: usize = 4096;
pub const MAX_FRAME_BYTES: usize = 8 * 1_048_576;
pub const MAX_HISTORY_BYTES: usize = 256 * 1_048_576;
/// Retained canonical data, not an estimate of heap usage or raw history size.
pub const MAX_RECEIPT_BYTES: usize = 256 * 1_048_576;
pub const MAX_DIRECTORY_ENTRIES: usize = 16_384;
pub const MAX_PROJECTION_BYTES: usize = 256 * 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestKey(String);
impl RequestKey {
    pub fn parse(value: &str) -> Result<Self> {
        require(request_key(value), "invalid_request_key")?;
        Ok(Self(value.to_owned()))
    }
}

/// Exact request identity: raw selectors and null/token distinctions are retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestBinding(Vec<u8>);
impl RequestBinding {
    pub fn parse(value: Value) -> Result<Self> {
        binding(&value)?;
        Ok(Self(value.canonical()?))
    }
    pub fn add(
        title: &str,
        owner: Option<&str>,
        notes: &str,
        expected_revision: Option<&str>,
    ) -> Result<Self> {
        Self::new(
            "add",
            vec![
                ("title", Value::String(title.to_owned())),
                (
                    "owner",
                    owner.map_or(Value::Null, |v| Value::String(v.to_owned())),
                ),
                ("notes", Value::String(notes.to_owned())),
            ],
            expected_revision,
        )
    }
    pub fn assign(task_id: &str, agent: &str, expected_revision: Option<&str>) -> Result<Self> {
        Self::new(
            "assign",
            vec![
                ("task_id", Value::String(task_id.to_owned())),
                ("agent", Value::String(agent.to_owned())),
            ],
            expected_revision,
        )
    }
    fn new(
        operation: &str,
        args: Vec<(&str, Value)>,
        expected_revision: Option<&str>,
    ) -> Result<Self> {
        Self::parse(Value::object(vec![
            ("version".to_owned(), Value::integer("1")?),
            ("operation".to_owned(), Value::String(operation.to_owned())),
            (
                "args".to_owned(),
                Value::object(args.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())?,
            ),
            (
                "expected_revision".to_owned(),
                expected_revision.map_or(Value::Null, |v| Value::String(v.to_owned())),
            ),
        ])?)
    }
}

/// Validated historical result. `committed` is not a fresh durability acknowledgement.
/// No reread, confirmation or delivery is performed here. Python ledger known-result
/// rereads can fail hard; execution's verification-failed delivery is a separate policy.
#[derive(Debug, Clone)]
pub struct FrozenResult(Value);
impl FrozenResult {
    pub fn value(&self) -> &Value {
        &self.0
    }
    pub fn to_value(&self) -> Value {
        self.0.clone()
    }
}

struct Receipt {
    binding: RequestBinding,
    result: FrozenResult,
}

fn receipt_bytes(total: usize, key: usize, binding: usize, result: usize) -> Result<usize> {
    let total = total
        .checked_add(key)
        .and_then(|n| n.checked_add(binding))
        .and_then(|n| n.checked_add(result))
        .ok_or(Error("unsupported_ledger_receipt_bytes"))?;
    require(
        total <= MAX_RECEIPT_BYTES,
        "unsupported_ledger_receipt_bytes",
    )?;
    Ok(total)
}

fn history_bytes(total: usize, increment: usize) -> Result<usize> {
    let total = total
        .checked_add(increment)
        .ok_or(Error("unsupported_ledger_history_bytes"))?;
    require(
        total <= MAX_HISTORY_BYTES,
        "unsupported_ledger_history_bytes",
    )?;
    Ok(total)
}

fn frame_names(names: &[String]) -> Result<()> {
    require(
        !names.is_empty() && names.len() <= MAX_FRAMES,
        "unsupported_ledger_history_profile",
    )?;
    require(
        names
            .iter()
            .enumerate()
            .all(|(index, name)| *name == format!("{index:020}.frame")),
        "ledger_sequence_gap_or_filename",
    )
}

pub(crate) fn object(value: &Value, names: &[&str]) -> Result<()> {
    let value = value.as_object().ok_or(Error("invalid_ledger_fields"))?;
    require(
        value.entries().len() == names.len() && names.iter().all(|name| value.get(name).is_some()),
        "invalid_ledger_fields",
    )
}
fn get<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .as_object()
        .and_then(|object| object.get(key))
        .ok_or(Error("missing_ledger_field"))
}
fn string(value: &Value) -> Result<&str> {
    value.as_str().ok_or(Error("invalid_ledger_text"))
}
fn text(value: &Value, blank: bool) -> Result<&str> {
    let value = string(value)?;
    require(
        blank || !records::trimmed(value).is_empty(),
        "invalid_ledger_text",
    )?;
    Ok(value)
}
fn eq_text(value: &Value, expected: &str) -> bool {
    value.as_str() == Some(expected)
}
fn exact_integer(value: &Value, expected: usize) -> bool {
    matches!(value,Value::Number(Number::Integer(value)) if value.as_str()==expected.to_string())
}
fn unsigned(value: &Value) -> Result<u64> {
    if let Value::Number(Number::Integer(value)) = value {
        value
            .as_str()
            .parse()
            .map_err(|_| Error("invalid_ledger_integer"))
    } else {
        Err(Error("invalid_ledger_integer"))
    }
}
fn version(value: &Value) -> Result<()> {
    require(
        get(value, "version")?.is_integer_one(),
        "unsupported_ledger_version",
    )
}
fn null(value: &Value) -> bool {
    matches!(value, Value::Null)
}
fn request_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

pub(crate) fn optional_bytes(root: &Path, relative: &str) -> Result<Option<Vec<u8>>> {
    let path = read_fs::managed(root, Path::new(relative), false)?;
    match fs::symlink_metadata(&path) {
        Ok(_) => Ok(Some(read_fs::read_regular(&path, MAX_FRAME_BYTES)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn bytes(root: &Path, relative: &str) -> Result<Vec<u8>> {
    optional_bytes(root, relative)?.ok_or(Error("required_ledger_file_missing"))
}
pub fn snapshot_revision(raw: Option<&[u8]>) -> String {
    raw.map(|raw| format!("tasks-sha256:{}", canonical::digest(raw)))
        .unwrap_or_else(|| "tasks-missing:v1".to_owned())
}
fn revision(workspace: &str, epoch: &str, sequence: usize, tasks: &Value) -> Result<String> {
    Ok(format!(
        "tasks-ledger:v1:{workspace}:{epoch}:{sequence}:{}",
        canonical::digest(&tasks.canonical()?)
    ))
}

fn payload(raw: &[u8]) -> Result<Value> {
    require(
        raw.len() <= MAX_FRAME_BYTES,
        "unsupported_ledger_frame_size",
    )?;
    let envelope = values::parse(raw, Profile::Authority)?;
    object(&envelope, &["version", "length", "sha256", "payload"])?;
    version(&envelope)?;
    let value = get(&envelope, "payload")?;
    let encoded = value.canonical()?;
    require(
        exact_integer(get(&envelope, "length")?, encoded.len())
            && eq_text(get(&envelope, "sha256")?, &canonical::digest(&encoded)),
        "ledger_frame_integrity_mismatch",
    )?;
    let mut canonical = envelope.canonical()?;
    canonical.push(b'\n');
    require(canonical == raw, "noncanonical_ledger_frame")?;
    Ok(value.clone())
}

fn inert(name: &str) -> bool {
    let Some(name) = name.strip_prefix('.') else {
        return false;
    };
    let Some(name) = name.strip_suffix(".tmp") else {
        return false;
    };
    let Some((base, random)) = name.rsplit_once('.') else {
        return false;
    };
    !random.is_empty()
        && random
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && (matches!(base, "manifest.json" | "baseline.tasks.bytes")
            || base.strip_suffix(".frame").is_some_and(|prefix| {
                prefix.len() == 20 && prefix.bytes().all(|byte| byte.is_ascii_digit())
            }))
}
fn entries(root: &Path, relative: &str, directories: &[&str]) -> Result<Vec<String>> {
    let path = read_fs::managed(root, Path::new(relative), true)?;
    let mut names = Vec::new();
    for (index, entry) in fs::read_dir(path)?.enumerate() {
        require(
            index < MAX_DIRECTORY_ENTRIES,
            "unsupported_ledger_directory_profile",
        )?;
        let name = entry?
            .file_name()
            .into_string()
            .map_err(|_| Error("invalid_ledger_entry_name"))?;
        read_fs::managed(
            root,
            Path::new(relative).join(&name).as_path(),
            directories.contains(&name.as_str()),
        )?;
        if !inert(&name) {
            names.push(name)
        }
    }
    names.sort();
    Ok(names)
}

fn alias(value: &str) -> String {
    let mut output = String::new();
    let mut invalid = false;
    for character in records::trimmed(value).to_lowercase().chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
            output.push(character);
            invalid = false;
        } else if !invalid {
            output.push('-');
            invalid = true;
        }
    }
    output.trim_matches('-').to_owned()
}
fn binding_revision(value: &Value) -> Result<&Value> {
    object(
        value,
        &["version", "operation", "args", "expected_revision"],
    )?;
    version(value)?;
    let expected = get(value, "expected_revision")?;
    if !null(expected) {
        handoff::TaskRevision::parse(string(expected)?)?;
    }
    Ok(expected)
}
fn binding(value: &Value) -> Result<(&str, &Object)> {
    binding_revision(value)?;
    let operation = string(get(value, "operation")?)?;
    let args = get(value, "args")?;
    match operation {
        "add" => {
            object(args, &["title", "owner", "notes"])?;
            text(get(args, "title")?, false)?;
            if !null(get(args, "owner")?) {
                text(get(args, "owner")?, false)?;
            }
            text(get(args, "notes")?, true)?;
        }
        "assign" => {
            object(args, &["task_id", "agent"])?;
            text(get(args, "task_id")?, false)?;
            text(get(args, "agent")?, false)?;
        }
        _ => return Err(Error("invalid_ledger_operation")),
    }
    Ok((
        operation,
        args.as_object().ok_or(Error("invalid_ledger_binding"))?,
    ))
}
fn arg<'a>(args: &'a Object, name: &str) -> Result<&'a Value> {
    args.get(name).ok_or(Error("invalid_ledger_binding"))
}
fn task_field<'a>(task: &'a Value, name: &str) -> Option<&'a Value> {
    task.as_object()?.get(name)
}

pub struct View {
    tasks: Value,
    revision: String,
    workspace_id: String,
    epoch_id: String,
    sequence: usize,
    head_digest: String,
    manifest_digest: String,
    baseline_projection: Option<Vec<u8>>,
    marker_raw: Vec<u8>,
    receipts: HashMap<String, Receipt>,
    receipt_bytes: usize,
}
impl View {
    /// Looks only in fully validated captured history, never in current state or files.
    /// `None` means only that the key is absent from this captured history, not that
    /// it is currently absent, durable, or permitted for fresh execution. Missing
    /// authority and pending imports are handled outside `View`, not by this lookup.
    pub fn lookup(
        &self,
        key: &RequestKey,
        binding: &RequestBinding,
    ) -> Result<Option<&FrozenResult>> {
        let Some(receipt) = self.receipts.get(&key.0) else {
            return Ok(None);
        };
        require(receipt.binding == *binding, "request_key_binding_conflict")?;
        Ok(Some(&receipt.result))
    }
    pub(crate) fn into_task_values(self) -> (String, Value) {
        (self.revision, self.tasks)
    }
    fn apply(&mut self, payload: Value, digest: String) -> Result<()> {
        object(
            &payload,
            &[
                "version",
                "workspace_id",
                "epoch_id",
                "sequence",
                "previous_digest",
                "request_id",
                "binding",
                "result",
            ],
        )?;
        version(&payload)?;
        let sequence = self.sequence + 1;
        require(
            exact_integer(get(&payload, "sequence")?, sequence)
                && eq_text(get(&payload, "workspace_id")?, &self.workspace_id)
                && eq_text(get(&payload, "epoch_id")?, &self.epoch_id)
                && eq_text(get(&payload, "previous_digest")?, &self.head_digest),
            "invalid_ledger_chain",
        )?;
        let key = string(get(&payload, "request_id")?)?;
        require(
            request_key(key) && !self.receipts.contains_key(key),
            "invalid_or_duplicate_ledger_request",
        )?;
        let binding_value = get(&payload, "binding")?;
        let expected = binding_revision(binding_value)?;
        // Replay CAS stays before operation/effect validation; lookup never does CAS.
        require(
            null(expected) || eq_text(expected, &self.revision),
            "ledger_expected_revision_mismatch",
        )?;
        let (operation, args) = binding(binding_value)?;
        let result = get(&payload, "result")?;
        object(
            result,
            &[
                "outcome",
                "mutation_id",
                "task",
                "before_revision",
                "after_revision",
                "sequence",
            ],
        )?;
        require(
            handoff::hex(string(get(result, "mutation_id")?)?, 32),
            "invalid_ledger_mutation_id",
        )?;
        let task = get(result, "task")?.clone();
        TaskRecords::validate(Value::Array(vec![task.clone()]))?;
        let tasks = self.tasks.array_mut()?;
        if operation == "add" {
            let title = string(arg(args, "title")?)?;
            let base = alias(title);
            require(
                base.bytes()
                    .any(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()),
                "invalid_ledger_slug",
            )?;
            let ids = tasks
                .iter()
                .map(|task| string(task_field(task, "id").ok_or(Error("invalid_task_record"))?))
                .collect::<Result<HashSet<_>>>()?;
            let mut candidate = base.clone();
            let mut index = 2;
            while ids.contains(candidate.as_str()) {
                candidate = format!("{base}-{index}");
                index += 1;
            }
            let owner = task_field(&task, "owner").unwrap_or(&Value::Null);
            let requested_owner = arg(args, "owner")?;
            require(
                eq_text(get(&task, "id")?, &candidate)
                    && eq_text(get(&task, "title")?, title)
                    && task_field(&task, "notes").is_some_and(|notes| {
                        notes.as_str() == arg(args, "notes").ok().and_then(Value::as_str)
                    })
                    && null(owner) == null(requested_owner)
                    && (null(owner) || alias(string(owner)?) == alias(string(requested_owner)?))
                    && eq_text(
                        get(&task, "status")?,
                        if null(owner) { "pending" } else { "assigned" },
                    )
                    && task_field(&task, "created_at")
                        .and_then(Value::as_str)
                        .is_some()
                    && task_field(&task, "updated_at")
                        .and_then(Value::as_str)
                        .is_some(),
                "invalid_ledger_add_effect",
            )?;
            require(
                tasks.len() < values::MAX_ITEMS,
                "unsupported_ledger_task_count",
            )?;
            tasks.push(task);
        } else {
            let id = string(arg(args, "task_id")?)?;
            let positions = tasks
                .iter()
                .enumerate()
                .filter_map(|(index, task)| {
                    task_field(task, "id")
                        .is_some_and(|value| eq_text(value, id))
                        .then_some(index)
                })
                .collect::<Vec<_>>();
            require(positions.len() == 1, "ledger_assigned_task_not_found")?;
            let index = positions[0];
            let mut expected = tasks[index].clone();
            let owner = task_field(&task, "owner").unwrap_or(&Value::Null);
            let updated = task_field(&task, "updated_at").unwrap_or(&Value::Null);
            expected.set_field("owner", owner.clone())?;
            expected.set_field("status", Value::String("assigned".to_owned()))?;
            expected.set_field("updated_at", updated.clone())?;
            require(
                task.canonical()? == expected.canonical()?
                    && alias(string(owner)?) == alias(string(arg(args, "agent")?)?)
                    && updated.as_str().is_some(),
                "invalid_ledger_assign_effect",
            )?;
            tasks[index] = task;
        }
        let revision = revision(&self.workspace_id, &self.epoch_id, sequence, &self.tasks)?;
        require(
            eq_text(get(result, "outcome")?, "committed")
                && eq_text(get(result, "before_revision")?, &self.revision)
                && eq_text(get(result, "after_revision")?, &revision)
                && exact_integer(get(result, "sequence")?, sequence),
            "invalid_ledger_frozen_result",
        )?;
        let binding = RequestBinding(binding_value.canonical()?);
        let retained = receipt_bytes(
            self.receipt_bytes,
            key.len(),
            binding.0.len(),
            result.canonical()?.len(),
        )?;
        self.receipts.insert(
            key.to_owned(),
            Receipt {
                binding,
                result: FrozenResult(result.clone()),
            },
        );
        self.receipt_bytes = retained;
        self.sequence = sequence;
        self.revision = revision;
        self.head_digest = digest;
        self.baseline_projection = None;
        Ok(())
    }
    pub fn tasks(&self) -> &Value {
        &self.tasks
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn projection_bytes(&self) -> Result<Vec<u8>> {
        self.baseline_projection
            .clone()
            .map(Ok)
            .unwrap_or_else(|| self.tasks.storage())
    }
    pub fn fence(&self, root: &Path) -> Result<()> {
        require(
            optional_bytes(root, MARKER)?.as_deref() == Some(self.marker_raw.as_slice()),
            "task_authority_changed_during_read",
        )
    }
    pub fn report(&self, root: &Path) -> Result<Value> {
        Value::object(vec![
            (
                "authority".to_owned(),
                Value::String("task-ledger-v1".to_owned()),
            ),
            ("revision".to_owned(), Value::String(self.revision.clone())),
            ("tasks".to_owned(), self.tasks.clone()),
            (
                "workspace_id".to_owned(),
                Value::String(self.workspace_id.clone()),
            ),
            ("epoch_id".to_owned(), Value::String(self.epoch_id.clone())),
            (
                "sequence".to_owned(),
                Value::integer(&self.sequence.to_string())?,
            ),
            (
                "frame_digest".to_owned(),
                Value::String(self.head_digest.clone()),
            ),
            (
                "projection".to_owned(),
                projection_health(root, &self.projection_bytes()?)?,
            ),
        ])
    }
}

fn projection_health(root: &Path, expected: &[u8]) -> Result<Value> {
    let expected_revision = Value::String(snapshot_revision(Some(expected)));
    let (status, observed, error) = match projection_observation(root, expected) {
        Ok((status, revision)) => (status, Value::String(revision), None),
        Err(error) => ("unreadable", Value::Null, Some(error.code())),
    };
    let mut fields = vec![
        ("status".to_owned(), Value::String(status.to_owned())),
        ("expected_revision".to_owned(), expected_revision),
        ("observed_revision".to_owned(), observed),
    ];
    if let Some(error) = error {
        fields.push(("error".to_owned(), Value::String(error.to_owned())));
    }
    Value::object(fields)
}

fn projection_observation(root: &Path, expected: &[u8]) -> Result<(&'static str, String)> {
    let path = read_fs::managed(root, Path::new("state/tasks.json"), false)?;
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(("missing", snapshot_revision(None)));
        }
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    let mut file = crate::platform::open_readonly(&path)?;
    require(file.metadata()?.is_file(), "unsafe_projection_type")?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    let mut total = 0_usize;
    let mut equal = true;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let next = total
            .checked_add(count)
            .ok_or(Error("unsupported_projection_profile"))?;
        require(
            next <= MAX_PROJECTION_BYTES,
            "unsupported_projection_profile",
        )?;
        equal &= expected.get(total..next) == Some(&buffer[..count]);
        hash.update(&buffer[..count]);
        total = next;
    }
    Ok((
        if equal && total == expected.len() {
            "current"
        } else {
            "diverged"
        },
        format!("tasks-sha256:{:x}", hash.finalize()),
    ))
}

pub fn pending(root: &Path) -> Result<bool> {
    require(
        optional_bytes(root, MARKER)?.is_none(),
        "task_authority_changed_during_read",
    )?;
    let path = read_fs::managed(root, Path::new(DIRECTORY), true)?;
    Ok(path.exists())
}

pub fn read(root: &Path) -> Result<Option<View>> {
    let Some(marker_raw) = optional_bytes(root, MARKER)? else {
        return Ok(None);
    };
    let candidate = validate_epoch(root, marker_raw)?;
    candidate.view.fence(root)?;
    Ok(Some(candidate.view))
}

// An epoch can be valid without being selected authority. Only read() or the
// activation protocol may expose its view after establishing marker selection.
struct CandidateEpoch {
    view: View,
    manifest: Value,
}

fn validate_epoch(root: &Path, marker_raw: Vec<u8>) -> Result<CandidateEpoch> {
    let marker = values::parse(&marker_raw, Profile::Authority)?;
    object(
        &marker,
        &[
            "version",
            "workspace_id",
            "epoch_id",
            "manifest_digest",
            "baseline_digest",
        ],
    )?;
    version(&marker)?;
    let workspace = string(get(&marker, "workspace_id")?)?;
    let epoch = string(get(&marker, "epoch_id")?)?;
    require(
        handoff::hex(workspace, 32) && handoff::hex(epoch, 32),
        "invalid_ledger_identity",
    )?;
    let base = format!("{DIRECTORY}/{epoch}");
    let raw = bytes(root, &format!("{base}/manifest.json"))?;
    require(
        eq_text(get(&marker, "manifest_digest")?, &canonical::digest(&raw)),
        "ledger_manifest_digest_mismatch",
    )?;
    let manifest = values::parse(&raw, Profile::Authority)?;
    object(
        &manifest,
        &[
            "version",
            "workspace_id",
            "epoch_id",
            "source_revision",
            "source_present",
            "source_mode",
            "source_length",
            "source_digest",
            "baseline_digest",
        ],
    )?;
    version(&manifest)?;
    for name in ["workspace_id", "epoch_id", "baseline_digest"] {
        require(
            get(&marker, name)?.canonical()? == get(&manifest, name)?.canonical()?,
            "ledger_manifest_identity_mismatch",
        )?;
    }
    let present = match get(&manifest, "source_present")? {
        Value::Bool(value) => *value,
        _ => return Err(Error("invalid_ledger_source_metadata")),
    };
    unsigned(get(&manifest, "source_length")?)?;
    let source = if present {
        Some(bytes(root, &format!("{base}/baseline.tasks.bytes"))?)
    } else {
        None
    };
    if present {
        require(
            unsigned(get(&manifest, "source_mode")?)? <= 0o7777,
            "invalid_ledger_source_mode",
        )?;
    } else {
        require(
            null(get(&manifest, "source_mode")?),
            "invalid_absent_ledger_source_mode",
        )?;
    }
    require(
        eq_text(
            get(&manifest, "source_revision")?,
            &snapshot_revision(source.as_deref()),
        ) && exact_integer(
            get(&manifest, "source_length")?,
            source.as_ref().map_or(0, Vec::len),
        ) && eq_text(
            get(&manifest, "source_digest")?,
            &canonical::digest(source.as_deref().unwrap_or(b"")),
        ),
        "ledger_source_backup_mismatch",
    )?;
    let tasks = TaskRecords::validate(
        source
            .as_deref()
            .map(|raw| values::parse(raw, Profile::Authority))
            .transpose()?
            .unwrap_or(Value::Array(Vec::new())),
    )?
    .into_value();
    let mut expected = vec!["frames".to_owned(), "manifest.json".to_owned()];
    if present {
        expected.push("baseline.tasks.bytes".to_owned());
    }
    expected.sort();
    require(
        entries(root, &base, &["frames"])? == expected,
        "unexpected_ledger_epoch_entries",
    )?;
    let frames = entries(root, &format!("{base}/frames"), &[])?;
    frame_names(&frames)?;
    let first = bytes(root, &format!("{base}/frames/{}", frames[0]))?;
    let mut total = history_bytes(0, first.len())?;
    let digest = canonical::digest(&first);
    require(
        eq_text(get(&marker, "baseline_digest")?, &digest),
        "ledger_baseline_digest_mismatch",
    )?;
    let initial = payload(&first)?;
    object(
        &initial,
        &[
            "version",
            "workspace_id",
            "epoch_id",
            "sequence",
            "previous_digest",
            "source_revision",
            "tasks",
            "revision",
        ],
    )?;
    version(&initial)?;
    let revision = revision(workspace, epoch, 0, &tasks)?;
    require(
        exact_integer(get(&initial, "sequence")?, 0)
            && null(get(&initial, "previous_digest")?)
            && get(&initial, "tasks")?.canonical()? == tasks.canonical()?
            && eq_text(get(&initial, "revision")?, &revision)
            && get(&initial, "source_revision")?.canonical()?
                == get(&manifest, "source_revision")?.canonical()?
            && eq_text(get(&initial, "workspace_id")?, workspace)
            && eq_text(get(&initial, "epoch_id")?, epoch),
        "invalid_ledger_baseline",
    )?;
    let mut view = View {
        tasks,
        revision,
        workspace_id: workspace.to_owned(),
        epoch_id: epoch.to_owned(),
        sequence: 0,
        head_digest: digest,
        manifest_digest: canonical::digest(&raw),
        baseline_projection: source.or(Some(b"[]\n".to_vec())),
        marker_raw,
        receipts: HashMap::new(),
        receipt_bytes: 0,
    };
    for name in frames.iter().skip(1) {
        let raw = bytes(root, &format!("{base}/frames/{name}"))?;
        total = history_bytes(total, raw.len())?;
        view.apply(payload(&raw)?, canonical::digest(&raw))?;
    }
    Ok(CandidateEpoch { view, manifest })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn empty_view() -> View {
        let tasks = Value::Array(Vec::new());
        View {
            revision: revision(&"a".repeat(32), &"b".repeat(32), 0, &tasks).unwrap(),
            tasks,
            workspace_id: "a".repeat(32),
            epoch_id: "b".repeat(32),
            sequence: 0,
            head_digest: "c".repeat(64),
            manifest_digest: String::new(),
            baseline_projection: None,
            marker_raw: Vec::new(),
            receipts: HashMap::new(),
            receipt_bytes: 0,
        }
    }
    fn json(raw: &str) -> Value {
        values::parse(raw.as_bytes(), Profile::Authority).unwrap()
    }
    fn apply_task(view: &mut View, key: &str, binding: Value, task: Value) -> Result<()> {
        let sequence = view.sequence + 1;
        let mut tasks = view.tasks.clone();
        if get(&binding, "operation")?.as_str() == Some("add") {
            tasks.array_mut()?.push(task.clone());
        } else {
            tasks.array_mut()?[0] = task.clone();
        }
        let after = revision(&view.workspace_id, &view.epoch_id, sequence, &tasks)?;
        let result = Value::object(vec![
            ("outcome".into(), Value::String("committed".into())),
            ("mutation_id".into(), Value::String("d".repeat(32))),
            ("task".into(), task),
            (
                "before_revision".into(),
                Value::String(view.revision.clone()),
            ),
            ("after_revision".into(), Value::String(after)),
            ("sequence".into(), Value::integer(&sequence.to_string())?),
        ])?;
        let payload = Value::object(vec![
            ("version".into(), Value::integer("1")?),
            (
                "workspace_id".into(),
                Value::String(view.workspace_id.clone()),
            ),
            ("epoch_id".into(), Value::String(view.epoch_id.clone())),
            ("sequence".into(), Value::integer(&sequence.to_string())?),
            (
                "previous_digest".into(),
                Value::String(view.head_digest.clone()),
            ),
            ("request_id".into(), Value::String(key.into())),
            ("binding".into(), binding),
            ("result".into(), result),
        ])?;
        view.apply(payload, "e".repeat(64))
    }
    // Test-only pure routing seam, not a mutation or delivery API.
    fn route<'a>(
        view: &'a View,
        key: &RequestKey,
        binding: &RequestBinding,
        fresh: impl FnOnce() -> Result<Option<&'a FrozenResult>>,
    ) -> Result<Option<&'a FrozenResult>> {
        match view.lookup(key, binding)? {
            Some(result) => Ok(Some(result)),
            None => fresh(),
        }
    }
    #[test]
    fn retained_budget_counts_each_component_and_checks_overflow() {
        assert_eq!(
            receipt_bytes(MAX_RECEIPT_BYTES - 6, 1, 2, 3).unwrap(),
            MAX_RECEIPT_BYTES
        );
        for parts in [
            (MAX_RECEIPT_BYTES, 1, 0, 0),
            (MAX_RECEIPT_BYTES, 0, 1, 0),
            (MAX_RECEIPT_BYTES, 0, 0, 1),
            (usize::MAX, 1, 0, 0),
            (0, usize::MAX, 1, 0),
            (0, 0, usize::MAX, 1),
        ] {
            assert_eq!(
                receipt_bytes(parts.0, parts.1, parts.2, parts.3)
                    .unwrap_err()
                    .code(),
                "unsupported_ledger_receipt_bytes"
            );
        }
    }
    #[test]
    fn historical_receipts_keep_raw_binding_and_isolated_extension_values() {
        let mut view = empty_view();
        let before = view.revision.clone();
        let binding = json(&format!(
            r#"{{"version":1,"operation":"add","args":{{"title":"Task","owner":" Worker ","notes":""}},"expected_revision":"{before}"}}"#
        ));
        let typed = RequestBinding::parse(binding.clone()).unwrap();
        let key = RequestKey::parse("add:1").unwrap();
        let mut task = json(
            r#"{"id":"task","title":"Task","owner":"worker","status":"assigned","notes":"","created_at":"then","updated_at":"then","extension":[123456789012345678901234567890,-0.0,0.0]}"#,
        );
        apply_task(&mut view, "add:1", binding, task.clone()).unwrap();
        let frozen = view.lookup(&key, &typed).unwrap().unwrap().to_value();
        let expected_bytes = frozen.canonical().unwrap();
        let expected_retained = key.0.len() + typed.0.len() + expected_bytes.len();
        assert_eq!(view.receipt_bytes, expected_retained);
        task.set_field("owner", Value::String("other".into()))
            .unwrap();
        task.set_field("updated_at", Value::String("later".into()))
            .unwrap();
        let assign = json(&format!(
            r#"{{"version":1,"operation":"assign","args":{{"task_id":"task","agent":"OTHER"}},"expected_revision":"{}"}}"#,
            view.revision
        ));
        apply_task(&mut view, "assign:1", assign, task).unwrap();
        assert_eq!(view.receipts.len(), 2);
        let all_retained: usize = view
            .receipts
            .iter()
            .map(|(key, receipt)| {
                key.len()
                    + values::parse(&receipt.binding.0, Profile::Authority)
                        .unwrap()
                        .canonical()
                        .unwrap()
                        .len()
                    + receipt.result.value().canonical().unwrap().len()
            })
            .sum();
        assert_eq!(view.receipt_bytes, all_retained);
        assert!(all_retained > expected_retained);
        assert_ne!(before, view.revision);
        let mut owned = frozen;
        owned.set_field("task", Value::Null).unwrap();
        assert_eq!(
            view.lookup(&key, &typed)
                .unwrap()
                .unwrap()
                .value()
                .canonical()
                .unwrap(),
            expected_bytes
        );
        assert_eq!(
            get(&view.tasks.as_array().unwrap()[0], "owner")
                .unwrap()
                .as_str(),
            Some("other")
        );
        for changed in [
            RequestBinding::add("Task", Some("worker"), "", Some(&before)).unwrap(),
            RequestBinding::add("Task", Some(" Worker "), "", None).unwrap(),
            RequestBinding::add("Task", Some(" Worker "), "", Some(&view.revision)).unwrap(),
        ] {
            assert_eq!(
                route(&view, &key, &changed, || panic!(
                    "fresh CAS/projection/owner/execution must not run"
                ))
                .unwrap_err()
                .code(),
                "request_key_binding_conflict"
            );
        }
        assert!(
            route(&view, &key, &typed, || panic!(
                "fresh CAS/projection/owner/execution must not run"
            ))
            .unwrap()
            .is_some()
        );
        let missing = RequestKey::parse("missing").unwrap();
        let continued = std::cell::Cell::new(false);
        assert!(
            route(&view, &missing, &typed, || {
                continued.set(true);
                Ok(None)
            })
            .unwrap()
            .is_none()
        );
        assert!(continued.get());
        // A stale expected revision is valid identity, but still invalid at replay CAS.
        let stale = json(&format!(
            r#"{{"version":1,"operation":"assign","args":{{"task_id":"task","agent":"other"}},"expected_revision":"{before}"}}"#
        ));
        assert!(RequestBinding::parse(stale.clone()).is_ok());
        let current = view.tasks.as_array().unwrap()[0].clone();
        assert_eq!(
            apply_task(&mut view, "stale", stale, current)
                .unwrap_err()
                .code(),
            "ledger_expected_revision_mismatch"
        );
        assert!(!view.receipts.contains_key("stale"));
    }
    #[test]
    fn constructors_validate_absent_requests_and_canonicalize_only_object_order() {
        for key in ["", " space", "bad/key", "-first"] {
            assert!(RequestKey::parse(key).is_err());
        }
        assert!(RequestKey::parse(&"x".repeat(129)).is_err());
        assert!(RequestBinding::add(" ", None, "", None).is_err());
        assert!(RequestBinding::assign("task", "", None).is_err());
        assert!(RequestBinding::assign("task", "agent", Some("invalid")).is_err());
        let oversized = "x".repeat(values::MAX_STRING + 1);
        for result in [
            RequestBinding::add(&oversized, None, "", None),
            RequestBinding::add("Task", None, &oversized, None),
            RequestBinding::add("Task", Some(&oversized), "", None),
            RequestBinding::assign(&oversized, "agent", None),
            RequestBinding::assign("task", &oversized, None),
        ] {
            assert_eq!(
                result.unwrap_err().code(),
                "unsupported_json_string_profile"
            );
        }
        let ordered = json(
            r#"{"args":{"notes":"","owner":null,"title":"Task"},"expected_revision":null,"operation":"add","version":1}"#,
        );
        assert_eq!(
            RequestBinding::parse(ordered).unwrap(),
            RequestBinding::add("Task", None, "", None).unwrap()
        );
        assert_ne!(
            RequestBinding::assign("task", " AGENT ", None).unwrap(),
            RequestBinding::assign("task", "agent", None).unwrap()
        );
    }
    #[test]
    fn binding_constructor_rejects_raw_json_float_versions() {
        for version in ["1e0", "1.00"] {
            let value = json(&format!(
                r#"{{"version":{version},"operation":"add","args":{{"title":"Task","owner":null,"notes":""}},"expected_revision":null}}"#
            ));
            assert!(matches!(
                get(&value, "version").unwrap(),
                Value::Number(Number::Float(_))
            ));
            assert_eq!(
                RequestBinding::parse(value).unwrap_err().code(),
                "unsupported_ledger_version"
            );
        }
    }
    #[test]
    fn replay_stale_cas_precedes_invalid_operation_and_args() {
        let mut view = empty_view();
        let stale_revision = view.revision.clone();
        let binding = json(
            r#"{"version":1,"operation":"add","args":{"title":"Task","owner":null,"notes":""},"expected_revision":null}"#,
        );
        let task = json(
            r#"{"id":"task","title":"Task","status":"pending","owner":null,"notes":"","created_at":"then","updated_at":"then"}"#,
        );
        apply_task(&mut view, "initial", binding, task.clone()).unwrap();
        // Rust's pre-existing replay CAS priority differs from Python's binding-first
        // validation. Constructors validate identity only, without historical CAS.
        for (operation, args, constructor_error) in [
            (
                "invalid",
                r#"{"task_id":"task","agent":"agent"}"#,
                "invalid_ledger_operation",
            ),
            ("assign", r#"{"task_id":"task"}"#, "invalid_ledger_fields"),
        ] {
            let binding = json(&format!(
                r#"{{"version":1,"operation":"{operation}","args":{args},"expected_revision":"{stale_revision}"}}"#
            ));
            assert_eq!(
                RequestBinding::parse(binding.clone()).unwrap_err().code(),
                constructor_error
            );
            assert_eq!(
                apply_task(&mut view, "mixed-invalid", binding, task.clone())
                    .unwrap_err()
                    .code(),
                "ledger_expected_revision_mismatch"
            );
            assert_eq!(view.receipts.len(), 1);
            assert!(!view.receipts.contains_key("mixed-invalid"));
        }
    }
    #[test]
    fn missing_authority_is_not_a_captured_view_miss_and_pending_is_not_active() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("state")).unwrap();
        assert!(read(root.path()).unwrap().is_none());
        assert!(!pending(root.path()).unwrap());
        fs::create_dir(root.path().join(DIRECTORY)).unwrap();
        assert!(pending(root.path()).unwrap());
        assert!(read(root.path()).unwrap().is_none());
        fs::write(root.path().join(MARKER), b"invalid authority").unwrap();
        assert!(read(root.path()).is_err());
        assert_eq!(
            pending(root.path()).unwrap_err().code(),
            "task_authority_changed_during_read"
        );
        let view = empty_view();
        assert!(
            view.lookup(
                &RequestKey::parse("absent").unwrap(),
                &RequestBinding::add("Task", None, "", None).unwrap()
            )
            .unwrap()
            .is_none()
        );
    }
    #[test]
    fn rejected_frames_never_retain_receipts() {
        let binding = json(
            r#"{"version":1,"operation":"add","args":{"title":"Task","owner":null,"notes":""},"expected_revision":null}"#,
        );
        let task = json(
            r#"{"id":"task","title":"Task","status":"pending","owner":null,"notes":"","created_at":"then","updated_at":"then"}"#,
        );
        let mut view = empty_view();
        view.receipt_bytes = MAX_RECEIPT_BYTES;
        assert_eq!(
            apply_task(&mut view, "over", binding.clone(), task.clone())
                .unwrap_err()
                .code(),
            "unsupported_ledger_receipt_bytes"
        );
        assert!(view.receipts.is_empty());
        let mut view = empty_view();
        let mut invalid = task.clone();
        invalid
            .set_field("title", Value::String("Wrong".into()))
            .unwrap();
        assert_eq!(
            apply_task(&mut view, "bad", binding.clone(), invalid)
                .unwrap_err()
                .code(),
            "invalid_ledger_add_effect"
        );
        assert!(view.receipts.is_empty());
        assert_eq!(view.receipt_bytes, 0);
        apply_task(&mut view, "same", binding.clone(), task.clone()).unwrap();
        assert_eq!(
            apply_task(&mut view, "same", binding, task)
                .unwrap_err()
                .code(),
            "invalid_or_duplicate_ledger_request"
        );
        assert_eq!(view.receipts.len(), 1);
    }
    #[test]
    fn full_read_rejects_corrupt_tail_before_exposing_any_view() {
        let root = tempfile::tempdir().unwrap();
        let view = empty_view();
        let base = root.path().join(DIRECTORY).join(&view.epoch_id);
        fs::create_dir_all(base.join("frames")).unwrap();
        let initial = Value::object(vec![
            ("version".into(), Value::integer("1").unwrap()),
            (
                "workspace_id".into(),
                Value::String(view.workspace_id.clone()),
            ),
            ("epoch_id".into(), Value::String(view.epoch_id.clone())),
            ("sequence".into(), Value::integer("0").unwrap()),
            ("previous_digest".into(), Value::Null),
            (
                "source_revision".into(),
                Value::String(snapshot_revision(None)),
            ),
            ("tasks".into(), view.tasks.clone()),
            ("revision".into(), Value::String(view.revision.clone())),
        ])
        .unwrap();
        let encoded = initial.canonical().unwrap();
        let envelope = Value::object(vec![
            ("version".into(), Value::integer("1").unwrap()),
            (
                "length".into(),
                Value::integer(&encoded.len().to_string()).unwrap(),
            ),
            ("sha256".into(), Value::String(canonical::digest(&encoded))),
            ("payload".into(), initial),
        ])
        .unwrap();
        let mut raw = envelope.canonical().unwrap();
        raw.push(b'\n');
        let baseline_digest = canonical::digest(&raw);
        fs::write(base.join("frames/00000000000000000000.frame"), raw).unwrap();
        let manifest = json(&format!(
            r#"{{"version":1,"workspace_id":"{}","epoch_id":"{}","source_revision":"tasks-missing:v1","source_present":false,"source_mode":null,"source_length":0,"source_digest":"{}","baseline_digest":"{baseline_digest}"}}"#,
            view.workspace_id,
            view.epoch_id,
            canonical::digest(b"")
        ));
        let raw = manifest.canonical().unwrap();
        let manifest_digest = canonical::digest(&raw);
        fs::write(base.join("manifest.json"), raw).unwrap();
        let marker = json(&format!(
            r#"{{"version":1,"workspace_id":"{}","epoch_id":"{}","manifest_digest":"{manifest_digest}","baseline_digest":"{baseline_digest}"}}"#,
            view.workspace_id, view.epoch_id
        ));
        fs::write(root.path().join(MARKER), marker.canonical().unwrap()).unwrap();
        let captured = read(root.path()).unwrap().unwrap();
        let key = RequestKey::parse("absent").unwrap();
        let binding = RequestBinding::add("Task", None, "", None).unwrap();
        assert!(captured.lookup(&key, &binding).unwrap().is_none());
        fs::write(
            base.join("frames/00000000000000000001.frame"),
            b"corrupt tail",
        )
        .unwrap();
        assert!(read(root.path()).is_err());
        // Already captured history is pure even after authority becomes unreadable.
        assert!(captured.lookup(&key, &binding).unwrap().is_none());
        fs::remove_file(root.path().join(MARKER)).unwrap();
        assert_eq!(
            captured.fence(root.path()).unwrap_err().code(),
            "task_authority_changed_during_read"
        );
        assert!(read(root.path()).unwrap().is_none());
        assert!(pending(root.path()).unwrap());
    }
    #[test]
    fn captured_marker_changes_or_disappears_never_become_snapshot() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("state")).unwrap();
        let marker = root.path().join(MARKER);
        fs::write(&marker, b"original marker").unwrap();
        let view = View {
            tasks: Value::Array(Vec::new()),
            revision: String::new(),
            workspace_id: String::new(),
            epoch_id: String::new(),
            sequence: 0,
            head_digest: String::new(),
            manifest_digest: String::new(),
            baseline_projection: None,
            marker_raw: b"original marker".to_vec(),
            receipts: HashMap::new(),
            receipt_bytes: 0,
        };
        view.fence(root.path()).unwrap();
        fs::write(&marker, b"different marker").unwrap();
        assert_eq!(
            view.fence(root.path()).unwrap_err().code(),
            "task_authority_changed_during_read"
        );
        fs::remove_file(marker).unwrap();
        assert_eq!(
            view.fence(root.path()).unwrap_err().code(),
            "task_authority_changed_during_read"
        );
    }
    #[test]
    fn inert_names_are_exact_and_not_directory_specific() {
        for name in [
            ".manifest.json.X.tmp",
            ".baseline.tasks.bytes.a_B-9.tmp",
            ".00000000000000000000.frame.X.tmp",
        ] {
            assert!(inert(name));
        }
        for name in [
            ".manifest.json..tmp",
            ".manifest.json.X.more.tmp",
            ".0.frame.X.tmp",
            "manifest.json.X.tmp",
        ] {
            assert!(!inert(name));
        }
    }
    #[test]
    fn history_byte_budget_is_exact_and_checked_for_overflow() {
        assert_eq!(
            history_bytes(MAX_HISTORY_BYTES - 1, 1).unwrap(),
            MAX_HISTORY_BYTES
        );
        assert!(history_bytes(MAX_HISTORY_BYTES, 1).is_err());
        assert!(history_bytes(usize::MAX, 1).is_err());
    }
    #[test]
    fn final_frame_count_accepts_boundary_and_refuses_overflow() {
        let mut names = (0..MAX_FRAMES)
            .map(|index| format!("{index:020}.frame"))
            .collect::<Vec<_>>();
        frame_names(&names).unwrap();
        names.push(format!("{MAX_FRAMES:020}.frame"));
        assert_eq!(
            frame_names(&names).unwrap_err().code(),
            "unsupported_ledger_history_profile"
        );
        assert!(frame_names(&[]).is_err());
    }
    #[test]
    fn directory_bound_counts_inert_temps_without_promoting_them() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("inert");
        fs::create_dir(&directory).unwrap();
        let seed = directory.join(".manifest.json.seed.tmp");
        fs::write(&seed, b"inert").unwrap();
        for index in 1..MAX_DIRECTORY_ENTRIES {
            fs::hard_link(&seed, directory.join(format!(".manifest.json.{index}.tmp"))).unwrap();
        }
        assert!(entries(root.path(), "inert", &[]).unwrap().is_empty());
        fs::hard_link(seed, directory.join(".manifest.json.extra.tmp")).unwrap();
        assert_eq!(
            entries(root.path(), "inert", &[]).unwrap_err().code(),
            "unsupported_ledger_directory_profile"
        );
    }
}
