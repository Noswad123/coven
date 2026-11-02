//! Read-only history observations. Literal output intentionally preserves controls.
//! No lock, durability confirmation, or execution authority is acquired here.
#[path = "python.rs"]
mod python;

use crate::{
    Error, Result,
    error::require,
    logs::{self, LogKind},
    platform,
    task_mutation::TaskOutput,
    values::{self, Value},
    workspace,
};
use python::{Output, truthy};
use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub enum HistoryKind {
    Events,
    Messages,
}

impl HistoryKind {
    fn log(self) -> LogKind {
        match self {
            Self::Events => LogKind::Events,
            Self::Messages => LogKind::Messages,
        }
    }

    fn empty(self) -> &'static str {
        match self {
            Self::Events => "No events yet\n",
            Self::Messages => "No messages yet\n",
        }
    }
}

/// Capture and admit the complete literal output before the caller prints it.
pub fn history_arguments(workspace: &Path, kind: HistoryKind, raw: bool) -> Result<TaskOutput> {
    platform::ensure_supported()?;
    let root = platform::workspace_argument(workspace)?
        .canonicalize()
        .map_err(|_| Error("invalid_checkpoint_workspace"))?;
    require(root.is_dir(), "invalid_checkpoint_workspace")?;
    require(root.to_str().is_some(), "non_unicode_path_unsupported")?;
    workspace::load(&root)?;
    let stdout = if raw {
        raw_history(&root, kind)?
    } else {
        pretty(&logs::read(&root, kind.log())?, kind)?
    };
    Ok(TaskOutput {
        stdout,
        warnings: Vec::new(),
        exit_code: 0,
    })
}

fn raw_history(root: &Path, kind: HistoryKind) -> Result<String> {
    let bytes = workspace::optional_bytes(
        root,
        Path::new(kind.log().relative()),
        values::MAX_OUTPUT,
        "unsupported_history_output_profile",
    )?;
    match bytes {
        None => Ok(format!("{}\n", kind.empty())),
        // Python's text reader uses universal newlines, not splitlines().
        Some(bytes) => Ok(std::str::from_utf8(&bytes)?
            .replace("\r\n", "\n")
            .replace('\r', "\n")),
    }
}

fn pretty(items: &[Value], kind: HistoryKind) -> Result<String> {
    let mut output = Output::default();
    if items.is_empty() {
        output.push(kind.empty())?;
    }
    for item in items {
        let object = item.as_object().ok_or(Error("invalid_jsonl_record"))?;
        let field = |name| object.get(name);
        output.push("- ")?;
        match field("ts") {
            Some(value) => output.value(value)?,
            None => output.push("unknown-time")?,
        }
        output.push(" [")?;
        match field("type") {
            Some(value) => output.value(value)?,
            None => output.push(match kind {
                HistoryKind::Events => "event",
                HistoryKind::Messages => "message",
            })?,
        }
        if matches!(kind, HistoryKind::Events) {
            if let Some(agent) = field("agent").filter(|value| truthy(value)) {
                output.push(" agent=")?;
                output.value(agent)?;
            }
        }
        if let Some(checkpoint) = field("checkpoint").filter(|value| truthy(value)) {
            output.push(" checkpoint=")?;
            output.value(checkpoint)?;
        }
        let task = field("task_id").filter(|value| truthy(value)).or_else(|| {
            if matches!(kind, HistoryKind::Events) {
                field("task")
                    .and_then(Value::as_object)
                    .and_then(|object| object.get("id"))
                    .filter(|value| truthy(value))
            } else {
                None
            }
        });
        if let Some(task) = task {
            output.push(" task=")?;
            output.value(task)?;
        }
        output.push("]")?;
        if matches!(kind, HistoryKind::Messages) {
            output.push(" ")?;
            match field("from") {
                Some(value) => output.value(value)?,
                None => output.push("?")?,
            }
            output.push(" -> ")?;
            match field("to") {
                Some(value) => output.value(value)?,
                None => output.push("?")?,
            }
            output.push(": ")?;
            if let Some(body) = ["message", "request", "body"]
                .iter()
                .find_map(|name| field(name).filter(|value| truthy(value)))
            {
                output.value(body)?;
            }
        }
        output.push("\n")?;
    }
    Ok(output.finish())
}

#[cfg(test)]
#[path = "history/tests.rs"]
mod tests;
