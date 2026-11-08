//! Local workspace writers. Output is complete literal text, not terminal-sanitized.
mod history;
pub(crate) mod python;
pub use history::{HistoryKind, history_arguments};

use crate::{
    Error, Result,
    error::require,
    logs::{self, LogKind},
    platform, records,
    storage::{self, Delivery, WorkspaceGuard},
    task_delivery,
    task_mutation::TaskOutput,
    task_time,
    values::{self, Value},
    workspace,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_TEXT: usize = 1_048_576;
const MAX_OUTPUT: usize = 32 * MAX_TEXT;
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

#[doc(hidden)]
pub fn split_optional_workspace(items: &[PathBuf]) -> Result<(PathBuf, &[PathBuf])> {
    workspace::split_optional_workspace(items)
}

// Domain-local fault seam; never selected by environment or public callers.
trait Io {
    fn phase(&self, _name: &str, _root: &Path) -> Result<()> {
        Ok(())
    }
    fn now(&self) -> Result<String> {
        task_time::now()
    }
    fn replace(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        storage::atomic_replace_bytes(guard, Path::new("goal.md"), bytes)
    }
    fn event(
        &self,
        root: &Path,
        kind: &str,
        payload: &Value,
        guard: &WorkspaceGuard,
    ) -> Result<()> {
        task_delivery::append_event(root, kind, payload, guard)
    }
    fn append(&self, root: &Path, bytes: &[u8], guard: &WorkspaceGuard) -> Result<()> {
        logs::append_encoded(root, LogKind::Messages, bytes, guard)
    }
    fn dashboard(&self, root: &Path, guard: &WorkspaceGuard) -> Result<()> {
        task_delivery::refresh_dashboard(root, guard)
    }
}
struct SystemIo;
impl Io for SystemIo {}

fn output(stdout: String) -> TaskOutput {
    TaskOutput {
        stdout,
        warnings: Vec::new(),
        exit_code: 0,
    }
}
fn failure(progress: &str, error: Error) -> TaskOutput {
    TaskOutput {
        stdout: String::new(),
        warnings: vec![format!("[ERROR] {progress}: {}", error.code())],
        exit_code: 1,
    }
}
fn operand(item: &Path) -> Result<&str> {
    item.to_str().ok_or(Error("non_unicode_path_unsupported"))
}
fn joined(items: &[PathBuf], limit: usize) -> Result<String> {
    let mut text = String::new();
    for (index, item) in items.iter().enumerate() {
        let item = operand(item)?;
        require(
            item.len().saturating_add(usize::from(index > 0)) <= limit.saturating_sub(text.len()),
            "unsupported_workspace_text_profile",
        )?;
        if index > 0 {
            text.push(' ');
        }
        text.push_str(item);
    }
    Ok(text)
}
fn literal(root: &Path, relative: &str, limit: usize, missing: Option<&str>) -> Result<String> {
    let raw = workspace::optional_bytes(
        root,
        Path::new(relative),
        limit,
        "unsupported_workspace_text_profile",
    )?;
    let text = match raw {
        Some(raw) => std::str::from_utf8(&raw)?
            .replace("\r\n", "\n")
            .replace('\r', "\n"),
        None => missing.ok_or(Error("missing_goal_file"))?.to_owned(),
    };
    require(
        text.len() <= MAX_OUTPUT,
        "unsupported_workspace_output_profile",
    )?;
    Ok(text)
}

pub fn goal_arguments(items: &[PathBuf]) -> Result<TaskOutput> {
    let (root, items) = workspace::split_optional_workspace(items)?;
    goal(&root, items, &SystemIo)
}
fn goal(root: &Path, items: &[PathBuf], io: &impl Io) -> Result<TaskOutput> {
    workspace::load(root)?;
    let joined = joined(items, MAX_OUTPUT)?;
    let text = records::trimmed(&joined);
    if text.is_empty() {
        let mut text = literal(root, "goal.md", MAX_TEXT, None)?;
        require(
            text.len() < MAX_OUTPUT,
            "unsupported_workspace_output_profile",
        )?;
        text.push('\n');
        return Ok(output(text));
    }
    require(
        text.len() <= MAX_TEXT - 9,
        "unsupported_workspace_text_profile",
    )?;
    let bytes = format!("# Goal\n\n{text}\n").into_bytes();
    let payload = Value::object(vec![("goal".into(), Value::String(text.into()))])?;
    workspace::load(root)?;
    io.phase("goal.before_lock", root)?;
    let guard = WorkspaceGuard::acquire(root, LOCK_TIMEOUT)?;
    io.phase("goal.held", root)?;
    workspace::load(root)?;
    match io.replace(&guard, &bytes)? {
        Delivery::DurabilityUncertain => {
            return Ok(failure(
                "New goal observed; synchronization uncertain; no rollback or secondary updates performed",
                Error("goal_synchronization_uncertain"),
            ));
        }
        Delivery::Committed => {}
    }
    let mut result = output("[INFO] Goal updated\n".into());
    for update in [
        io.event(root, "goal.updated", &payload, &guard),
        io.dashboard(root, &guard),
    ] {
        if let Err(error) = update {
            result.warnings.push(format!(
                "[WARN] Goal committed; secondary update failed: {}",
                error.code()
            ));
        }
    }
    Ok(result)
}

pub fn message_arguments(items: &[PathBuf]) -> Result<TaskOutput> {
    let (root, items) = workspace::split_optional_workspace(items)?;
    message(&root, items, &SystemIo)
}
fn message(root: &Path, items: &[PathBuf], io: &impl Io) -> Result<TaskOutput> {
    require(items.len() >= 3, "message_requires_sender_recipient_body")?;
    workspace::load(root)?;
    let sender = operand(&items[0])?;
    let recipient = operand(&items[1])?;
    require(
        sender.len() <= values::MAX_STRING && recipient.len() <= values::MAX_STRING,
        "unsupported_json_string_profile",
    )?;
    let body = joined(&items[2..], MAX_OUTPUT)?;
    let record = Value::object(vec![
        ("ts".into(), Value::String(io.now()?)),
        ("from".into(), Value::String(sender.into())),
        ("to".into(), Value::String(recipient.into())),
        ("body".into(), Value::String(body)),
    ])?;
    let encoded = logs::encode_record(&record)?;
    let payload = Value::object(vec![
        ("sender".into(), Value::String(sender.into())),
        ("recipient".into(), Value::String(recipient.into())),
    ])?;
    logs::read_log(root, LogKind::Events.relative())?;
    logs::read_log(root, LogKind::Messages.relative())?;
    workspace::load(root)?;
    io.phase("message.before_lock", root)?;
    let guard = WorkspaceGuard::acquire(root, LOCK_TIMEOUT)?;
    io.phase("message.held", root)?;
    workspace::load(root)?;
    logs::read_log(root, LogKind::Events.relative())?;
    if let Err(error) = io.append(root, &encoded, &guard) {
        return Ok(failure(
            "Message append failed and may be partial; no rollback performed",
            error,
        ));
    }
    if let Err(error) = io.event(root, "message.sent", &payload, &guard) {
        return Ok(failure(
            "Message appended; event update failed and may be partial; no rollback performed",
            error,
        ));
    }
    drop(guard);
    // Dashboard is deliberately a second mutation, not part of message delivery.
    let refreshed = (|| {
        io.phase("message.released", root)?;
        workspace::load(root)?;
        task_delivery::validate_dashboard_inputs(root)?;
        workspace::load(root)?;
        io.phase("message.dashboard_before_lock", root)?;
        let guard = WorkspaceGuard::acquire(root, LOCK_TIMEOUT)?;
        io.phase("message.dashboard_held", root)?;
        workspace::load(root)?;
        io.dashboard(root, &guard)
    })();
    if let Err(error) = refreshed {
        return Ok(failure(
            "Message and event appended; dashboard update failed; no rollback performed",
            error,
        ));
    }
    Ok(output("[INFO] Message appended\n".into()))
}

pub fn status_arguments(workspace: &Path) -> Result<TaskOutput> {
    platform::ensure_supported()?;
    let root = platform::workspace_argument(workspace)?
        .canonicalize()
        .map_err(|_| Error("invalid_checkpoint_workspace"))?;
    require(root.is_dir(), "invalid_checkpoint_workspace")?;
    require(root.to_str().is_some(), "non_unicode_path_unsupported")?;
    status(&root, &SystemIo)
}
fn status(root: &Path, io: &impl Io) -> Result<TaskOutput> {
    workspace::load(root)?;
    task_delivery::validate_dashboard_inputs(root)?;
    workspace::load(root)?;
    io.phase("status.before_lock", root)?;
    let guard = WorkspaceGuard::acquire(root, LOCK_TIMEOUT)?;
    io.phase("status.held", root)?;
    workspace::load(root)?;
    if let Err(error) = io.dashboard(root, &guard) {
        return Ok(failure(
            if error.code() == "dashboard_synchronization_uncertain" {
                "New dashboard observed; synchronization uncertain; no rollback performed"
            } else {
                "Dashboard publication failed; replacement may already be visible; no rollback performed"
            },
            error,
        ));
    }
    drop(guard);
    let printed = (|| {
        io.phase("status.released", root)?;
        literal(
            root,
            "dashboard.md",
            MAX_OUTPUT,
            Some("No dashboard yet\n\n"),
        )
    })();
    match printed {
        Ok(text) => Ok(output(text)),
        Err(error) => Ok(failure(
            "Dashboard published; reading output failed; no rollback performed",
            error,
        )),
    }
}

#[cfg(test)]
mod tests;
