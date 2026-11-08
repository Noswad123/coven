//! Source-independent cancellation under the shared persistent workspace lock.
use super::*;
use crate::{
    storage::{self, Delivery, WorkspaceGuard},
    task_mutation::TaskOutput,
};
use std::{path::PathBuf, time::Duration};

mod capacity;
mod commands;
mod delivery;
mod effects;
mod graphs;
mod lifecycle;
mod upgrade;
pub use commands::{
    PreparationInput, WriteOptions, decide_arguments, enable_arguments, enroll_arguments,
    prepare_arguments, propose_arguments, upgrade_arguments,
};
#[cfg(test)]
mod tests;

trait Io {
    fn read(&self, root: &Path) -> Result<Option<(Value, Validated)>> {
        read_typed(root)
    }
    fn lock(&self, root: &Path) -> Result<WorkspaceGuard> {
        WorkspaceGuard::execution_maintenance(root, Duration::from_secs(5))
    }
    fn binding(&self, value: &Value, schema: usize) -> Result<()> {
        let mut work = Work::default();
        if schema == 2 {
            v2::binding(value, &mut work)?;
        } else {
            super::binding(value, &mut work)?;
        }
        Ok(())
    }
    fn metadata(&self) -> Result<(String, String)> {
        Ok((
            crate::platform::random_uuid_hex()?,
            crate::task_time::now()?,
        ))
    }
    fn admit(&self, bytes: &[u8]) -> Result<Validated> {
        validated(&values::parse(bytes, Profile::Authority)?)
    }
    fn replace(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        storage::atomic_replace_bytes(guard, Path::new("state/execution/control.json"), bytes)
    }
    fn output(
        &self,
        view: &Validated,
        result: &Value,
        replayed: bool,
        json: bool,
    ) -> Result<delivery::Prepared> {
        delivery::Prepared::new(view, result, replayed, json)
    }
    fn confirm(&self, root: &Path, view: &Validated, guard: &WorkspaceGuard) -> Result<()> {
        super::confirmation::confirm(root, view, guard)
    }
}
struct SystemIo;
impl Io for SystemIo {}

struct Request<'a> {
    attempt: &'a str,
    reason: &'a str,
    expected: &'a str,
    key: &'a str,
}
impl Request<'_> {
    fn validate(&self) -> Result<()> {
        require(hex(self.attempt, 32), "invalid_execution_attempt_id")?;
        require(
            self.reason.len() <= 16384 && !records::trimmed(self.reason).is_empty(),
            "invalid_execution_bounded_text",
        )?;
        require(
            self.expected.len() <= values::MAX_STRING,
            "unsupported_json_string_profile",
        )
    }
    fn validate_key(&self) -> Result<()> {
        require(
            !self.key.is_empty()
                && self.key.len() <= 128
                && self.key.as_bytes()[0].is_ascii_alphanumeric()
                && self
                    .key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':')),
            "invalid_execution_request_or_criterion_id",
        )
    }
    fn binding(&self, schema: usize, io: &impl Io) -> Result<Value> {
        let value = object(vec![
            ("schema_version", integer(schema)?),
            ("operation", string("cancel")),
            (
                "parameters",
                object(vec![
                    ("attempt_id", string(self.attempt)),
                    ("reason", string(self.reason)),
                ])?,
            ),
            ("expected_execution_revision", string(self.expected)),
            ("expected_tasks_revision", Value::Null),
        ])?;
        io.binding(&value, schema)?;
        Ok(value)
    }
}

/// Required attempt selector, with an optional literal workspace selected by arity.
pub fn cancel_arguments(
    items: &[PathBuf],
    reason: &str,
    expected: &str,
    request_id: &str,
    json: bool,
) -> Result<TaskOutput> {
    let (workspace, selected) = match items {
        [selected] => (Path::new("."), selected),
        [workspace, selected] => (workspace.as_path(), selected),
        _ => return Err(Error("invalid_execution_selector_arguments")),
    };
    crate::platform::ensure_supported()?;
    let root = crate::platform::workspace_argument(workspace)?.canonicalize()?;
    require(root.is_dir(), "invalid_execution_workspace")?;
    root.to_str().ok_or(Error("non_unicode_path_unsupported"))?;
    cancel(
        &root, selected, reason, expected, request_id, json, &SystemIo,
    )
}

fn cancel(
    root: &Path,
    selected: &Path,
    reason: &str,
    expected: &str,
    key: &str,
    json: bool,
    io: &impl Io,
) -> Result<TaskOutput> {
    // The initial full authority read intentionally precedes semantic admission.
    let initial = io.read(root)?;
    let request = Request {
        attempt: selected
            .to_str()
            .ok_or(Error("non_unicode_path_unsupported"))?,
        reason,
        expected,
        key,
    };
    request.validate()?;
    request.binding(
        if expected.starts_with("execution-control-v2:") {
            2
        } else {
            1
        },
        io,
    )?;
    request.validate_key()?;
    require(initial.is_some(), "execution_disabled")?;
    drop(initial);
    let guard = io.lock(root)?;
    let (control, mut view) = io.read(root)?.ok_or(Error("execution_disabled"))?;
    if let Some((_, entry)) = view.replayed.receipts.iter().find(|(id, _)| id == key) {
        let retained = get(entry, "binding")?;
        let schema = if exact_integer(get(retained, "schema_version")?, 2) {
            2
        } else {
            1
        };
        let incoming = request
            .binding(schema, io)
            .map_err(|_| Error("execution_request_key_binding_conflict"))?;
        require(
            Work::default().equal(retained, &incoming)?,
            "execution_request_key_binding_conflict",
        )?;
        let outputs = io.output(&view, get(entry, "result")?, true, json)?;
        return Ok(outputs.confirm(io.confirm(root, &view, &guard)));
    }
    let binding = request.binding(view.schema, io)?;
    require(
        expected == view.replayed.revision,
        "execution_revision_conflict",
    )?;
    require(
        view.replayed
            .state
            .attempts
            .iter()
            .any(|a| a.id == request.attempt && a.active()),
        "execution_attempt_not_prepared",
    )?;
    let (id, now) = io.metadata()?;
    let effect = object(vec![
        ("cancel_id", string(id)),
        ("attempt_id", string(request.attempt)),
        ("reason", string(reason)),
        ("cancelled_at", string(now)),
    ])?;
    let generation = view.replayed.receipts.len() + 1;
    let result = view.replayed.state.apply(
        &control,
        &binding,
        "cancel",
        &effect,
        generation,
        &mut Work::default(),
    )?;
    let mut candidate = control.clone();
    let mut history = get(&candidate, "history")?
        .as_array()
        .ok_or(Error("invalid_execution_history"))?
        .to_vec();
    history.push(object(vec![
        ("generation", integer(generation)?),
        ("request_id", string(key)),
        ("binding", binding),
        ("effect", effect),
        ("result", result.clone()),
    ])?);
    candidate.set_field("history", Value::Array(history))?;
    candidate.set_field("generation", integer(generation)?)?;
    let mut bytes = candidate.canonical()?;
    bytes.push(b'\n');
    require(
        bytes.len() <= values::MAX_INPUT,
        "unsupported_json_size_profile",
    )?;
    let proposed = io.admit(&bytes)?;
    let outputs = io.output(&proposed, &result, false, json)?;
    match io.replace(&guard, &bytes)? {
        Delivery::DurabilityUncertain => {
            Ok(outputs.uncertain(Error("execution_control_durability_uncertain")))
        }
        Delivery::Committed => Ok(outputs.confirm(io.confirm(root, &proposed, &guard))),
    }
}
