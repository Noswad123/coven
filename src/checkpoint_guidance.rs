//! Read-only composition of the existing `next` guidance, not approval authority.
use crate::{
    Error, Result, checkpoints,
    error::require,
    logs::LogKind,
    platform, read_fs, task_delivery,
    task_mutation::TaskOutput,
    values::Value,
    workspace::{self, MAX_TEXT, optional_bytes},
};
use std::{fs, path::Path};

mod timestamp;

const MAX_OUTPUT: usize = 32 * 1_048_576;

// On finite JSON, only string contents (including object keys) can contribute
// the literal substring to Python's str(type); no repr allocation is needed.
fn contains_request(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains("request"),
        Value::Array(items) => items.iter().any(contains_request),
        Value::Object(object) => object
            .entries()
            .iter()
            .any(|(key, value)| key.contains("request") || contains_request(value)),
        _ => false,
    }
}

fn status(root: &Path, id: &str) -> Result<String> {
    checkpoints::inspect_bytes(id, None)?;
    let raw = optional_bytes(
        root,
        &Path::new("checkpoints").join(format!("{id}.md")),
        MAX_TEXT,
        "unsupported_checkpoint_input_profile",
    )?;
    let view = checkpoints::inspect_bytes(id, raw.as_deref())?;
    view.as_object()
        .and_then(|object| object.get("effective_status"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(Error("invalid_checkpoint_result"))
}

fn select(root: &Path, observed: &mut impl FnMut(&str)) -> Result<Option<String>> {
    let mut records = task_delivery::checkpoint_log_records(root, LogKind::Messages)?;
    records.extend(task_delivery::checkpoint_log_records(
        root,
        LogKind::Events,
    )?);
    let mut requests = Vec::new();
    for record in &records {
        let Some(object) = record.as_object() else {
            continue;
        };
        let Some(id) = object.get("checkpoint").and_then(Value::as_str) else {
            continue;
        };
        if !id.is_empty() && object.get("type").is_some_and(contains_request) {
            requests.push((timestamp::sort_key(object.get("ts"))?, id));
        }
    }
    requests.sort_by_key(|(key, _)| *key);
    let mut selected = None;
    for (_, id) in requests {
        observed(id);
        if status(root, id)? != "approved" {
            selected = Some(id.to_owned());
        }
    }
    if selected.is_some() {
        return Ok(selected);
    }

    let directory = read_fs::managed(root, Path::new("checkpoints"), true)?;
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut names = Vec::new();
    for (index, entry) in entries.enumerate() {
        require(index < 4096, "unsupported_checkpoint_scan_profile")?;
        names.push(entry?.file_name());
    }
    let mut names = names
        .into_iter()
        .map(|name| {
            name.into_string()
                .map_err(|_| Error("non_unicode_path_unsupported"))
        })
        .collect::<Result<Vec<_>>>()?;
    names.sort();
    for name in names {
        if let Some(id) = name.strip_suffix(".md") {
            observed(id);
            if matches!(
                status(root, id)?.as_str(),
                "approval_stale" | "approved_unbound" | "decision_invalid"
            ) {
                selected = Some(id.to_owned());
            }
        }
    }
    Ok(selected)
}

fn render(root: &str, same_cwd: bool, selected: Option<&str>) -> Result<String> {
    let mut out = String::new();
    let mut push = |text: &str| -> Result<()> {
        require(
            text.len() <= MAX_OUTPUT - out.len(),
            "unsupported_checkpoint_guidance_output_profile",
        )?;
        out.push_str(text);
        Ok(())
    };
    if let Some(id) = selected {
        for part in [
            "Next action: approve checkpoint `",
            id,
            "`\nReason: one or more agents are blocked waiting for user approval.\nReview file: ",
            root,
            if root.ends_with('/') { "" } else { "/" },
            "checkpoints/",
            id,
            ".md\nSuggested: coven approve ",
        ] {
            push(part)?;
        }
        if !same_cwd {
            push(root)?;
            push(" ")?;
        }
        push(id)?;
    } else {
        push("Next action: inspect status/dashboard\nSuggested: coven status")?;
        if !same_cwd {
            push(" ")?;
            push(root)?;
        }
    }
    push("\n")?;
    Ok(out)
}

/// Complete literal stdout, including terminal LF, before the caller emits any
/// bytes. Explicit workspace resolution never searches ancestors or takes locks.
pub fn next_arguments(workspace: &Path) -> Result<TaskOutput> {
    platform::ensure_supported()?;
    let root = platform::workspace_argument(workspace)?
        .canonicalize()
        .map_err(|_| Error("invalid_checkpoint_workspace"))?;
    require(root.is_dir(), "invalid_checkpoint_workspace")?;
    let root_text = read_fs::path_text(&root)?;
    workspace::load(&root)?;
    let selected = select(&root, &mut |_| {})?;
    let cwd = platform::workspace_argument(Path::new("."))?.canonicalize()?;
    Ok(TaskOutput {
        stdout: render(&root_text, root == cwd, selected.as_deref())?,
        warnings: Vec::new(),
        exit_code: 0,
    })
}

#[cfg(test)]
mod tests;
