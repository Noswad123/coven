//! Complete bounded task-ledger read/replay. No mutation, import, repair, fsync or
//! receipt lookup activation. A selected invalid authority never becomes snapshot.
use crate::{
    Error, Result, canonical,
    error::require,
    handoff, read_fs,
    records::{self, TaskRecords},
    values::{self, Number, Object, Profile, Value},
};
use sha2::{Digest as _, Sha256};
use std::{collections::HashSet, fs, io::Read, path::Path};

pub const MARKER: &str = "state/task-authority.json";
pub const DIRECTORY: &str = "state/task-ledger";
pub const MAX_FRAMES: usize = 4096;
pub const MAX_FRAME_BYTES: usize = 8 * 1_048_576;
pub const MAX_HISTORY_BYTES: usize = 256 * 1_048_576;
pub const MAX_DIRECTORY_ENTRIES: usize = 16_384;
pub const MAX_PROJECTION_BYTES: usize = 256 * 1_048_576;

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
fn binding<'a>(value: &'a Value, current_revision: &str) -> Result<(&'a str, &'a Object)> {
    object(
        value,
        &["version", "operation", "args", "expected_revision"],
    )?;
    version(value)?;
    let expected = get(value, "expected_revision")?;
    if !null(expected) {
        handoff::TaskRevision::parse(string(expected)?)?;
        require(
            eq_text(expected, current_revision),
            "ledger_expected_revision_mismatch",
        )?;
    }
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
    baseline_projection: Option<Vec<u8>>,
    marker_raw: Vec<u8>,
    request_ids: HashSet<String>,
}
impl View {
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
            request_key(key) && !self.request_ids.contains(key),
            "invalid_or_duplicate_ledger_request",
        )?;
        let (operation, args) = binding(get(&payload, "binding")?, &self.revision)?;
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
        self.request_ids.insert(key.to_owned());
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
        baseline_projection: source.or(Some(b"[]\n".to_vec())),
        marker_raw,
        request_ids: HashSet::new(),
    };
    for name in frames.iter().skip(1) {
        let raw = bytes(root, &format!("{base}/frames/{name}"))?;
        total = history_bytes(total, raw.len())?;
        view.apply(payload(&raw)?, canonical::digest(&raw))?;
    }
    view.fence(root)?;
    Ok(Some(view))
}

#[cfg(test)]
mod tests {
    use super::*;
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
            baseline_projection: None,
            marker_raw: b"original marker".to_vec(),
            request_ids: HashSet::new(),
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
