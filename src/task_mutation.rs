//! Native add/assign coordination. Authority commits are never rolled back by
//! secondary delivery failures; a receipt is history, not a fresh execution.
use crate::{
    Error, Result,
    error::require,
    execution, ledger,
    records::{self, TaskRecords, TeamRecord},
    storage::{self, Delivery, WorkspaceGuard},
    task_admission, task_delivery, task_time,
    values::{self, Profile, Value},
    workspace,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const PROJECTION_WARNING: &str =
    "Task projection is not current; new mutations require state recover-tasks --repair-projection";

fn object(fields: Vec<(&str, Value)>) -> Result<Value> {
    Value::object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}
fn text(value: impl Into<String>) -> Value {
    Value::String(value.into())
}
fn field<'a>(value: &'a Value, name: &str) -> Result<&'a Value> {
    value
        .as_object()
        .and_then(|v| v.get(name))
        .ok_or(Error("invalid_task_result"))
}
fn string(value: &Value) -> Result<&str> {
    value.as_str().ok_or(Error("invalid_task_result"))
}

enum Change<'a> {
    Add {
        title: &'a str,
        owner: Option<&'a str>,
        notes: &'a str,
    },
    Assign {
        task_id: &'a str,
        agent: &'a str,
    },
}
impl Change<'_> {
    fn binding(&self, expected: Option<&str>) -> Result<ledger::RequestBinding> {
        let binding = match self {
            Self::Add {
                title,
                owner,
                notes,
            } => {
                // Python's raw binding normalizes only the empty owner to null.
                ledger::RequestBinding::add(title, owner.filter(|v| !v.is_empty()), notes, expected)
            }
            Self::Assign { task_id, agent } => {
                ledger::RequestBinding::assign(task_id, agent, expected)
            }
        };
        // Keep the constructor's revision-first validation and resource errors;
        // only add service context to its otherwise field-agnostic diagnostics.
        binding.map_err(|error| match error.code() {
            "invalid_handoff_token" => Error("invalid_task_revision"),
            "invalid_ledger_text" => Error(match self {
                Self::Add { title, .. } if records::trimmed(title).is_empty() => {
                    "invalid_task_title"
                }
                Self::Add { .. } => "invalid_task_owner",
                Self::Assign { task_id, .. } if records::trimmed(task_id).is_empty() => {
                    "invalid_task_id"
                }
                Self::Assign { .. } => "invalid_task_agent",
            }),
            _ => error,
        })
    }
    fn draft(&self, team: &TeamRecord, tasks: &mut Value, io: &TaskIo) -> Result<Draft> {
        match self {
            Self::Add {
                title,
                owner,
                notes,
            } => {
                let owner = owner
                    .filter(|v| !v.is_empty())
                    .map(|v| resolve_agent(team, v))
                    .transpose()?;
                require(!records::trimmed(title).is_empty(), "invalid_task_title")?;
                let base = ledger::alias(title);
                require(sluggable(&base), "invalid_task_slug")?;
                let tasks = tasks.array_mut()?;
                let mut id = base.clone();
                let mut suffix = 2;
                while tasks.iter().any(|v| {
                    v.as_object()
                        .and_then(|v| v.get("id"))
                        .and_then(Value::as_str)
                        == Some(&id)
                }) {
                    id = format!("{base}-{suffix}");
                    suffix += 1;
                }
                let task = object(vec![
                    ("id", text(id)),
                    ("title", text(*title)),
                    (
                        "status",
                        text(if owner.is_some() {
                            "assigned"
                        } else {
                            "pending"
                        }),
                    ),
                    ("owner", owner.map_or(Value::Null, text)),
                    ("notes", text(*notes)),
                    ("created_at", text(io.now()?)),
                    ("updated_at", text(io.now()?)),
                ])?;
                tasks.push(task.clone());
                Ok(Draft {
                    payload: object(vec![("task", task.clone())])?,
                    task,
                    event: "task.created",
                })
            }
            Self::Assign { task_id, agent } => {
                let owner = resolve_agent(team, agent)?;
                for task in tasks.array_mut()? {
                    if field(task, "id")?.as_str() == Some(*task_id) {
                        task.set_field("owner", text(&owner))?;
                        task.set_field("status", text("assigned"))?;
                        task.set_field("updated_at", text(io.now()?))?;
                        return Ok(Draft {
                            task: task.clone(),
                            event: "task.assigned",
                            payload: object(vec![
                                ("task_id", text(*task_id)),
                                ("agent", text(owner)),
                            ])?,
                        });
                    }
                }
                Err(Error("task_not_found"))
            }
        }
    }
}
struct Draft {
    task: Value,
    event: &'static str,
    payload: Value,
}

fn sluggable(value: &str) -> bool {
    value.bytes().any(|b| b.is_ascii_alphanumeric())
}
fn resolve_agent(team: &TeamRecord, value: &str) -> Result<String> {
    let ids = team.agent_ids()?;
    let alias = ledger::alias(value);
    if !sluggable(&alias) {
        return ids
            .into_iter()
            .find(|id| *id == value)
            .map(str::to_owned)
            .ok_or(Error("unknown_agent"));
    }
    let mut matches = ids.into_iter().filter(|id| {
        let candidate = ledger::alias(id);
        sluggable(&candidate) && candidate == alias
    });
    let id = matches.next().ok_or(Error("unknown_agent"))?;
    require(matches.next().is_none(), "ambiguous_agent")?;
    Ok(id.to_owned())
}

pub fn add_arguments(
    items: &[PathBuf],
    owner: Option<&str>,
    notes: &str,
    expected_revision: Option<&str>,
    request_id: Option<&str>,
) -> Result<Value> {
    let (root, remaining) = workspace::split_optional_workspace(items)?;
    require(!remaining.is_empty(), "task_add_requires_title")?;
    let title = remaining
        .iter()
        .map(|v| v.to_str().ok_or(Error("non_unicode_task_argument")))
        .collect::<Result<Vec<_>>>()?
        .join(" ");
    add(&root, &title, owner, notes, expected_revision, request_id)
}

pub fn assign_arguments(
    items: &[PathBuf],
    expected_revision: Option<&str>,
    key: Option<&str>,
) -> Result<Value> {
    let (root, remaining) = workspace::split_optional_workspace(items)?;
    require(remaining.len() >= 2, "task_assign_requires_task_and_agent")?;
    let task = remaining[0]
        .to_str()
        .ok_or(Error("non_unicode_task_argument"))?;
    let agent = remaining[1]
        .to_str()
        .ok_or(Error("non_unicode_task_argument"))?;
    assign(&root, task, agent, expected_revision, key)
}

pub fn add(
    workspace: &Path,
    title: &str,
    owner: Option<&str>,
    notes: &str,
    expected: Option<&str>,
    key: Option<&str>,
) -> Result<Value> {
    mutate(
        workspace,
        Change::Add {
            title,
            owner,
            notes,
        },
        expected,
        key,
        &TaskIo::default(),
    )
}
pub fn assign(
    workspace: &Path,
    task_id: &str,
    agent: &str,
    expected: Option<&str>,
    key: Option<&str>,
) -> Result<Value> {
    mutate(
        workspace,
        Change::Assign { task_id, agent },
        expected,
        key,
        &TaskIo::default(),
    )
}

fn snapshot(root: &Path) -> Result<(String, Value)> {
    let raw = ledger::optional_bytes(root, "state/tasks.json")?;
    let tasks = raw
        .as_deref()
        .map(|raw| values::parse(raw, Profile::LegacySnapshot))
        .transpose()?
        .unwrap_or(Value::Array(Vec::new()));
    Ok((
        ledger::snapshot_revision(raw.as_deref()),
        TaskRecords::validate(tasks)?.into_value(),
    ))
}
fn selected_tasks(root: &Path) -> Result<(String, Value)> {
    if let Some(view) = ledger::read(root)? {
        Ok(view.into_task_values())
    } else {
        snapshot(root)
    }
}
fn check_revision(expected: Option<&str>, before: &str) -> Result<()> {
    require(
        expected.is_none_or(|v| v == before),
        "task_revision_conflict",
    )
}
fn snapshot_revision(expected: Option<&str>) -> Result<()> {
    if let Some(expected) = expected {
        require(
            expected == "tasks-missing:v1"
                || expected
                    .strip_prefix("tasks-sha256:")
                    .is_some_and(|v| crate::handoff::hex(v, 64)),
            "invalid_snapshot_revision",
        )?;
    }
    Ok(())
}
fn encoded_tasks(tasks: &Value) -> Result<Vec<u8>> {
    TaskRecords::validate(tasks.clone())?;
    let encoded = tasks.storage()?;
    require(
        encoded.len() <= values::MAX_INPUT,
        "unsupported_task_snapshot_size",
    )?;
    TaskRecords::validate(values::parse(&encoded, Profile::LegacySnapshot)?)?;
    Ok(encoded)
}
fn event_admission(payload: &Value) -> Result<()> {
    let encoded = payload.storage()?;
    require(
        encoded.len() <= values::MAX_INPUT,
        "unsupported_task_event_size",
    )?;
    values::parse(&encoded, Profile::LegacySnapshot)?;
    Ok(())
}

fn mutate(
    root: &Path,
    change: Change<'_>,
    expected: Option<&str>,
    key: Option<&str>,
    io: &TaskIo,
) -> Result<Value> {
    // Identity is captured before discovering authority or interpreting selectors.
    let binding = key.map(|_| change.binding(expected)).transpose()?;
    crate::platform::ensure_supported()?;
    let root = root
        .canonicalize()
        .map_err(|_| Error("invalid_task_workspace"))?;
    require(root.is_dir(), "invalid_task_workspace")?;
    if io.read(&root)?.is_some() {
        require(key.is_some(), "ledger_request_id_required")?;
        let key = ledger::RequestKey::parse(key.ok_or(Error("ledger_request_id_required"))?)?;
        workspace::load(&root)?;
        io.before_lock(&root)?;
        let guard = WorkspaceGuard::acquire(&root, Duration::from_secs(5))?;
        return ledger_mutation(
            &root,
            &change,
            expected,
            &key,
            binding
                .as_ref()
                .ok_or(Error("ledger_request_binding_required"))?,
            &guard,
            io,
        );
    }
    require(key.is_none(), "snapshot_request_id_unsupported")?;
    require(!ledger::pending(&root)?, "task_import_pending")?;
    snapshot_revision(expected)?;
    let team = workspace::load(&root)?.team;
    let (before, mut draft) = selected_tasks(&root)?;
    check_revision(expected, &before)?;
    change.draft(&team, &mut draft, io)?;
    encoded_tasks(&draft)?;
    task_delivery::validate_snapshot_logs(&root)?;
    // Discard the unlocked draft. Only the state selected under this guard wins.
    workspace::load(&root)?;
    io.before_lock(&root)?;
    let guard = WorkspaceGuard::acquire(&root, Duration::from_secs(5))?;
    workspace::load(&root)?;
    if io.read(&root)?.is_some() {
        return Err(Error("ledger_request_id_required"));
    }
    require(!ledger::pending(&root)?, "task_import_pending")?;
    let team = workspace::load(&root)?.team;
    let (before, mut tasks) = selected_tasks(&root)?;
    check_revision(expected, &before)?;
    task_delivery::validate_snapshot_logs(&root)?;
    let mut draft = change.draft(&team, &mut tasks, io)?;
    TaskRecords::validate(tasks.clone())?;
    let _reservation =
        execution::guard_task_mutation(&root, string(field(&draft.task, "id")?)?, &guard)?;
    let encoded = encoded_tasks(&tasks)?;
    let after = ledger::snapshot_revision(Some(&encoded));
    let mutation_id = io.uuid()?;
    draft.payload.set_field("mutation_id", text(&mutation_id))?;
    draft.payload.set_field("before_revision", text(&before))?;
    draft.payload.set_field("after_revision", text(&after))?;
    event_admission(&draft.payload)?;
    let mut result = object(vec![
        ("outcome", text("committed")),
        ("mutation_id", text(mutation_id)),
        ("task", draft.task),
        ("before_revision", text(before)),
        ("after_revision", text(&after)),
        ("warnings", Value::Array(Vec::new())),
    ])?;
    task_admission::response(&result)?;
    task_admission::response(&object(vec![("revision", text(after)), ("tasks", tasks)])?)?;
    let mut warnings = Vec::new();
    if io.replace(&guard, &encoded)? == Delivery::DurabilityUncertain {
        result.set_field("outcome", text("durability_uncertain"))?;
        warnings.push(
            "Task snapshot replaced; durability uncertain. Inspect before retrying.".to_owned(),
        );
    } else {
        secondary(
            &mut warnings,
            "Task committed; history incomplete",
            io.event(&root, draft.event, &draft.payload, &guard),
        );
        secondary(
            &mut warnings,
            "Task committed; dashboard stale",
            io.dashboard(&root, &guard),
        );
    }
    result.set_field("warnings", warning_values(warnings))?;
    Ok(result)
}

fn ledger_mutation(
    root: &Path,
    change: &Change<'_>,
    expected: Option<&str>,
    key: &ledger::RequestKey,
    binding: &ledger::RequestBinding,
    guard: &WorkspaceGuard,
    io: &TaskIo,
) -> Result<Value> {
    guard.validate_for(root)?;
    let team = workspace::load(root)?.team;
    let view = io.read(root)?.ok_or(Error("task_authority_changed"))?;
    let receipt = view.lookup(key, binding)?.map(|v| v.to_value());
    if let Some(receipt) = &receipt {
        task_admission::response(receipt)?;
    }
    let confirmation = io.confirm(root, &view, guard)?;
    if confirmation == ledger::LedgerConfirmation::SynchronizationUncertain {
        let receipt = receipt.ok_or(Error("ledger_synchronization_uncertain"))?;
        return delivery(
            root,
            &view,
            receipt,
            true,
            Delivery::DurabilityUncertain,
            vec!["Ledger authority observed; durability uncertain".to_owned()],
            io,
        );
    }
    if let Some(receipt) = receipt {
        return delivery(
            root,
            &view,
            receipt,
            true,
            Delivery::Committed,
            Vec::new(),
            io,
        );
    }
    let health = io.health(root, &view)?;
    require(
        field(&health, "status")?.as_str() == Some("current"),
        "task_projection_not_current",
    )?;
    check_revision(expected, view.revision())?;
    let mut tasks = view.tasks().clone();
    let mut draft = change.draft(&team, &mut tasks, io)?;
    TaskRecords::validate(tasks)?;
    let committed = io.commit(root, &view, key, binding, draft.task, guard)?;
    if committed.delivery == Delivery::DurabilityUncertain {
        // Python requires a new, validated authority read and matching receipt.
        // Never substitute the cached candidate on a failed read or missing key.
        let observed = io
            .uncertain_read(root)?
            .ok_or(Error("ledger_uncertain_receipt_missing"))?;
        let receipt = observed
            .lookup(key, binding)?
            .ok_or(Error("ledger_uncertain_receipt_missing"))?
            .to_value();
        return delivery(
            root,
            &observed,
            receipt,
            false,
            Delivery::DurabilityUncertain,
            vec!["Ledger frame published; durability uncertain".to_owned()],
            io,
        );
    }
    let receipt = committed
        .view
        .lookup(key, binding)?
        .ok_or(Error("ledger_committed_receipt_missing"))?
        .to_value();
    let mut warnings = Vec::new();
    let projection = io
        .projection(root, &committed.view, guard)
        .and_then(|v| require(v == Delivery::Committed, "projection_durability_uncertain"));
    secondary(
        &mut warnings,
        "Ledger task committed; task projection stale",
        projection,
    );
    // Keep the original draft payload, not the canonical frozen task object.
    let event = (|| {
        draft
            .payload
            .set_field("mutation_id", field(&receipt, "mutation_id")?.clone())?;
        let metadata = ledger::event_fields(&committed.view)?;
        for (key, value) in metadata
            .as_object()
            .ok_or(Error("invalid_ledger_event_metadata"))?
            .entries()
        {
            draft.payload.set_field(key, value.clone())?;
        }
        io.event(root, draft.event, &draft.payload, guard)
    })();
    secondary(
        &mut warnings,
        "Ledger task committed; history incomplete",
        event,
    );
    secondary(
        &mut warnings,
        "Ledger task committed; dashboard stale",
        io.dashboard(root, guard),
    );
    delivery(
        root,
        &committed.view,
        receipt,
        false,
        Delivery::Committed,
        warnings,
        io,
    )
}

fn secondary(warnings: &mut Vec<String>, label: &str, result: Result<()>) {
    if let Err(error) = result {
        warnings.push(format!("{label}: {}", task_admission::warning_code(&error)));
    }
}
fn warning_values(warnings: Vec<String>) -> Value {
    Value::Array(warnings.into_iter().map(text).collect())
}

fn delivery(
    root: &Path,
    view: &ledger::View,
    receipt: Value,
    replayed: bool,
    outcome: Delivery,
    mut warnings: Vec<String>,
    io: &TaskIo,
) -> Result<Value> {
    let health = match io.final_health(root, view).and_then(bounded_health) {
        Ok(health) => health,
        Err(error) => {
            warnings.push(format!(
                "Task projection health unavailable: {}",
                task_admission::warning_code(&error)
            ));
            object(vec![
                ("status", text("unreadable")),
                ("expected_revision", Value::Null),
                ("observed_revision", Value::Null),
                ("error", text(task_admission::warning_code(&error))),
            ])?
        }
    };
    if field(&health, "status")?.as_str() != Some("current") {
        warnings.push(PROJECTION_WARNING.to_owned());
    }
    let result = object(vec![
        ("result", receipt),
        (
            "delivery",
            object(vec![
                (
                    "outcome",
                    text(if outcome == Delivery::Committed {
                        "committed"
                    } else {
                        "durability_uncertain"
                    }),
                ),
                ("replayed", Value::Bool(replayed)),
                ("warnings", warning_values(warnings)),
                ("current_revision", text(view.revision())),
                ("projection", health),
            ])?,
        ),
    ])?;
    // Fresh publication admitted its frozen result with an 8 KiB envelope
    // reserve. Historical oversized receipts may be refused without a new write.
    if replayed {
        task_admission::response(&result)?;
    }
    Ok(result)
}

fn bounded_health(value: Value) -> Result<Value> {
    require(
        matches!(
            field(&value, "status")?.as_str(),
            Some("current" | "missing" | "diverged" | "unreadable")
        ),
        "invalid_projection_health",
    )?;
    let entries = value
        .as_object()
        .ok_or(Error("invalid_projection_health"))?
        .entries();
    require(entries.len() <= 4, "invalid_projection_health")?;
    for (key, value) in entries {
        require(
            matches!(
                key.as_str(),
                "status" | "expected_revision" | "observed_revision" | "error"
            ),
            "invalid_projection_health",
        )?;
        require(
            matches!(value, Value::Null)
                || value.as_str().is_some_and(|v| {
                    v.len() <= 128 && v.is_ascii() && !v.bytes().any(|b| b.is_ascii_control())
                }),
            "invalid_projection_health",
        )?;
    }
    Ok(value)
}

/// Presentation-only escaping: human text never emits terminal controls or
/// non-ASCII. Escapes use JSON spelling, but printable ASCII quotes/backslashes
/// stay literal. JSON retains ordinary values through the shared ASCII encoder.
fn human(value: &str) -> String {
    let mut output = String::new();
    for character in value.chars() {
        if character.is_ascii() && !character.is_ascii_control() {
            output.push(character);
        } else {
            match character {
                '\u{08}' => output.push_str("\\b"),
                '\u{0c}' => output.push_str("\\f"),
                '\n' => output.push_str("\\n"),
                '\r' => output.push_str("\\r"),
                '\t' => output.push_str("\\t"),
                _ => {
                    let mut units = [0_u16; 2];
                    for unit in character.encode_utf16(&mut units) {
                        output.push_str("\\u");
                        for shift in [12, 8, 4, 0] {
                            output.push(
                                b"0123456789abcdef"[usize::from((*unit >> shift) & 15)] as char,
                            );
                        }
                    }
                }
            }
        }
    }
    output
}
pub struct TaskOutput {
    pub stdout: String,
    pub warnings: Vec<String>,
    pub exit_code: u8,
}
pub fn render_result(value: &Value, json: bool, action: &str) -> Result<TaskOutput> {
    let ledger = value.as_object().and_then(|v| v.get("delivery"));
    let status = ledger.unwrap_or(value);
    let outcome = string(field(status, "outcome")?)?;
    let exit_code = if outcome == "committed" { 0 } else { 2 };
    let warnings = if json {
        Vec::new()
    } else {
        field(status, "warnings")?
            .as_array()
            .ok_or(Error("invalid_task_result"))?
            .iter()
            .map(|v| Ok(format!("[WARN] {}", human(string(v)?))))
            .collect::<Result<Vec<_>>>()?
    };
    let stdout = if json {
        crate::output::ascii_json(std::str::from_utf8(&value.compact()?)?)?
    } else if let Some(delivery) = ledger {
        let frozen = field(value, "result")?;
        let verb = if outcome == "durability_uncertain" {
            "Observed; durability uncertain for"
        } else if matches!(field(delivery, "replayed")?, Value::Bool(true)) {
            "Replayed committed"
        } else {
            "Committed"
        };
        let Value::Number(values::Number::Integer(sequence)) = field(frozen, "sequence")? else {
            return Err(Error("invalid_task_result"));
        };
        format!(
            "{verb} ledger task {} (sequence {})",
            human(string(field(field(frozen, "task")?, "id")?)?),
            sequence.as_str()
        )
    } else {
        let task = field(value, "task")?;
        let id = human(string(field(task, "id")?)?);
        if outcome != "committed" {
            format!(
                "Task snapshot replaced for {id}; durability uncertain. Inspect before retrying."
            )
        } else if action == "add" {
            format!("Created {id}: {}", human(string(field(task, "title")?)?))
        } else {
            format!("Assigned {id} to {}", human(string(field(task, "owner")?)?))
        }
    };
    Ok(TaskOutput {
        stdout,
        warnings,
        exit_code,
    })
}

// Concrete domain-local seam. Test faults are not selectable by production
// callers, environment variables, providers, or subprocesses.
#[cfg(test)]
type HeldHook = Box<dyn Fn(&WorkspaceGuard)>;

#[derive(Default)]
struct TaskIo {
    #[cfg(test)]
    faults: std::collections::BTreeMap<&'static str, Fault>,
    #[cfg(test)]
    trace: std::cell::RefCell<Vec<&'static str>>,
    #[cfg(test)]
    hook: Option<fn(&Path)>,
    #[cfg(test)]
    uncertain_hook: Option<fn(&Path)>,
    #[cfg(test)]
    event_hook: Option<fn(&Path)>,
    #[cfg(test)]
    held_hook: Option<HeldHook>,
    #[cfg(test)]
    fixed_clock: bool,
}
#[cfg(test)]
#[derive(Clone, Copy)]
enum Fault {
    Error,
    Uncertain,
}
impl TaskIo {
    #[cfg(test)]
    fn held(&self, guard: &WorkspaceGuard) -> Result<()> {
        guard.validate_for(guard.root())?;
        self.step("held")?;
        if let Some(hook) = &self.held_hook {
            hook(guard);
        }
        Ok(())
    }
    fn step(&self, name: &'static str) -> Result<bool> {
        #[cfg(test)]
        {
            self.trace.borrow_mut().push(name);
            match self.faults.get(name) {
                Some(Fault::Error) => return Err(Error("injected_task_io_failure")),
                Some(Fault::Uncertain) => return Ok(true),
                None => {}
            }
        }
        let _ = name;
        Ok(false)
    }
    fn before_lock(&self, root: &Path) -> Result<()> {
        self.step("before_lock")?;
        #[cfg(test)]
        if let Some(hook) = self.hook {
            hook(root);
        }
        let _ = root;
        Ok(())
    }
    fn now(&self) -> Result<String> {
        self.step("now")?;
        #[cfg(test)]
        if self.fixed_clock {
            return Ok("2026-01-02T03:04:05+00:00".to_owned());
        }
        task_time::now()
    }
    fn uuid(&self) -> Result<String> {
        self.step("uuid")?;
        task_time::uuid_hyphenated()
    }
    fn read(&self, root: &Path) -> Result<Option<ledger::View>> {
        self.step("read")?;
        ledger::read(root)
    }
    fn uncertain_read(&self, root: &Path) -> Result<Option<ledger::View>> {
        if self.step("uncertain_read")? {
            return Ok(None);
        }
        #[cfg(test)]
        if let Some(hook) = self.uncertain_hook {
            hook(root);
        }
        ledger::read(root)
    }
    fn confirm(
        &self,
        root: &Path,
        view: &ledger::View,
        guard: &WorkspaceGuard,
    ) -> Result<ledger::LedgerConfirmation> {
        if self.step("confirm")? {
            return Ok(ledger::LedgerConfirmation::SynchronizationUncertain);
        }
        ledger::confirm_selected(root, view, guard)
    }
    fn replace(&self, guard: &WorkspaceGuard, encoded: &[u8]) -> Result<Delivery> {
        let uncertain = self.step("replace")?;
        #[cfg(test)]
        self.held(guard)?;
        let delivery =
            storage::atomic_replace_bytes(guard, Path::new("state/tasks.json"), encoded)?;
        Ok(if uncertain {
            Delivery::DurabilityUncertain
        } else {
            delivery
        })
    }
    fn commit(
        &self,
        root: &Path,
        expected: &ledger::View,
        key: &ledger::RequestKey,
        binding: &ledger::RequestBinding,
        task: Value,
        guard: &WorkspaceGuard,
    ) -> Result<ledger::LedgerCommit> {
        let uncertain = self.step("commit")?;
        #[cfg(test)]
        self.held(guard)?;
        let mut committed = ledger::commit_task(root, expected, key, binding, task, guard)?;
        if uncertain {
            committed.delivery = Delivery::DurabilityUncertain;
        }
        Ok(committed)
    }
    fn projection(
        &self,
        root: &Path,
        view: &ledger::View,
        guard: &WorkspaceGuard,
    ) -> Result<Delivery> {
        if self.step("projection")? {
            return Ok(Delivery::DurabilityUncertain);
        }
        ledger::publish_projection(root, view, guard)
    }
    fn health(&self, root: &Path, view: &ledger::View) -> Result<Value> {
        self.step("health")?;
        ledger::projection_status(root, view)
    }
    fn final_health(&self, root: &Path, view: &ledger::View) -> Result<Value> {
        self.step("final_health")?;
        ledger::projection_status(root, view)
    }
    fn event(
        &self,
        root: &Path,
        event: &str,
        payload: &Value,
        guard: &WorkspaceGuard,
    ) -> Result<()> {
        self.step("event")?;
        #[cfg(test)]
        if let Some(hook) = self.event_hook {
            hook(root);
        }
        task_delivery::append_event(root, event, payload, guard)
    }
    fn dashboard(&self, root: &Path, guard: &WorkspaceGuard) -> Result<()> {
        self.step("dashboard")?;
        task_delivery::refresh_dashboard(root, guard)
    }
}

#[cfg(test)]
mod tests;
