use super::*;
use crate::{logs::LogKind, read_fs, records, task_delivery, workspace};
use std::io::{self, BufRead, Write};

// Reuse the established Python-compatible request ordering without changing its owner.
// The original module is private; sharing its source also shares its characterization tests.
#[allow(clippy::duplicate_mod)]
#[path = "../checkpoint_guidance/timestamp.rs"]
mod timestamp;

fn contains_request(value: &Value) -> bool {
    match value {
        Value::String(s) => s.contains("request"),
        Value::Array(a) => a.iter().any(contains_request),
        Value::Object(o) => o
            .entries()
            .iter()
            .any(|(k, v)| k.contains("request") || contains_request(v)),
        _ => false,
    }
}

fn candidates(root: &Path) -> Result<Vec<String>> {
    let mut requests = task_delivery::checkpoint_log_records(root, LogKind::Messages)?;
    requests.extend(task_delivery::checkpoint_log_records(
        root,
        LogKind::Events,
    )?);
    let mut ordered = Vec::new();
    for record in requests {
        let Some(o) = record.as_object() else {
            continue;
        };
        let Some(id) = o.get("checkpoint").and_then(Value::as_str) else {
            continue;
        };
        if o.get("type").is_some_and(contains_request) {
            ordered.push((timestamp::sort_key(o.get("ts"))?, id.to_owned()));
        }
    }
    ordered.sort_by_key(|(key, _)| *key);
    let mut seen = std::collections::HashSet::new();
    let mut pending = Vec::new();
    for (_, id) in ordered {
        if seen.insert(id.clone()) && field(&view(root, &id)?, "effective_status")? != "approved" {
            pending.push(id);
        }
    }
    if !pending.is_empty() {
        return Ok(pending);
    }
    let directory = read_fs::managed(root, Path::new("checkpoints"), true)?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for (index, entry) in std::fs::read_dir(directory)?.enumerate() {
        require(index < 4096, "unsupported_checkpoint_scan_profile")?;
        let name = entry?
            .file_name()
            .into_string()
            .map_err(|_| Error("non_unicode_path_unsupported"))?;
        if let Some(id) = name.strip_suffix(".md") {
            names.push(id.to_owned());
        }
    }
    names.sort();
    for id in names {
        if field(&view(root, &id)?, "effective_status")? != "approved" {
            pending.push(id);
        }
    }
    Ok(pending)
}

pub fn review(workspace: &Path, checkpoints: &[String], agent: &str, message: &str) -> Result<u8> {
    review_io(
        workspace,
        checkpoints,
        agent,
        message,
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
    )
}

pub(crate) fn review_io(
    root: &Path,
    ids: &[String],
    agent: &str,
    message: &str,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<u8> {
    review_with(root, ids, agent, message, input, output, &mut |request| {
        decide(root, request, false)
    })
}

/// Render the inspected bytes and return exactly their revision, without a lock
/// surviving into the caller's user-input boundary.
pub(crate) fn display_io(root: &Path, id: &str, output: &mut impl Write) -> Result<String> {
    let displayed = view(root, id)?;
    let revision = field(&displayed, "revision")?.to_owned();
    writeln!(
        output,
        "\n{}\nReview checkpoint: {id}\nFile: {}\nSnapshot: {revision}\nEffective status: {}; binding={}",
        "=".repeat(80),
        root.join(relative(id)?).display(),
        field(&displayed, "effective_status")?,
        field(&displayed, "binding")?
    )?;
    if let Some(reason) = displayed
        .as_object()
        .and_then(|o| o.get("reason"))
        .and_then(Value::as_str)
    {
        writeln!(output, "Reason: {reason}")?;
    }
    let content = displayed
        .as_object()
        .and_then(|o| o.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("Checkpoint does not exist; a decision will create a default local checkpoint.");
    writeln!(output, "{}\n{content}", "=".repeat(80))?;
    Ok(revision)
}

fn read_line(input: &mut impl BufRead) -> Result<Option<String>> {
    // A bounded read even when a noninteractive caller supplies no newline.
    let mut bytes = Vec::new();
    std::io::Read::take(input, 1_048_578).read_until(b'\n', &mut bytes)?;
    require(
        bytes.len() <= 1_048_577,
        "unsupported_checkpoint_input_profile",
    )?;
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    require(
        bytes.len() <= 1_048_576,
        "unsupported_checkpoint_input_profile",
    )?;
    Ok(Some(
        String::from_utf8(bytes).map_err(|_| Error("invalid_checkpoint_utf8"))?,
    ))
}

pub(crate) fn review_with(
    root: &Path,
    ids: &[String],
    agent: &str,
    message: &str,
    input: &mut impl BufRead,
    output: &mut impl Write,
    submit: &mut impl FnMut(DecisionRequest) -> Result<CommandResult>,
) -> Result<u8> {
    workspace::load(root)?;
    let ids = if ids.is_empty() {
        candidates(root)?
    } else {
        ids.to_vec()
    };
    for id in &ids {
        view(root, id)?;
    }
    super::coordinator::validate_selector(root, agent)?;
    if ids.is_empty() {
        writeln!(output, "No pending checkpoint files found.")?;
        return Ok(0);
    }
    let mut status = 0;
    for id in ids {
        let revision = display_io(root, &id, output)?;
        loop {
            write!(output, "[a]pprove, [s]uggest changes, [n]ext, [q]uit > ")?;
            output.flush()?;
            let Some(action) = read_line(input)? else {
                writeln!(output)?;
                return Ok(status);
            };
            let (action, notes) = match records::trimmed(&action).to_lowercase().as_str() {
                "a" | "approve" => (
                    DecisionAction::Approve,
                    if message.is_empty() {
                        format!(
                            "Checkpoint `{id}` approved after reviewing the displayed local snapshot."
                        )
                    } else {
                        message.into()
                    },
                ),
                "s" | "suggest" | "changes" | "change" => {
                    writeln!(
                        output,
                        "Enter requested changes for the agents.\nFinish with a single '.' on its own line."
                    )?;
                    let mut notes = String::new();
                    loop {
                        write!(output, "> ")?;
                        output.flush()?;
                        let Some(line) = read_line(input)? else {
                            break;
                        };
                        if line == "." {
                            break;
                        }
                        require(
                            line.len().saturating_add(1)
                                <= 1_048_576usize.saturating_sub(notes.len()),
                            "unsupported_checkpoint_input_profile",
                        )?;
                        if !notes.is_empty() {
                            notes.push('\n');
                        }
                        notes.push_str(&line);
                    }
                    let notes = records::trimmed(&notes).to_owned();
                    if notes.is_empty() {
                        writeln!(output, "No change request saved")?;
                        break;
                    }
                    (DecisionAction::Suggest, notes)
                }
                "n" | "next" | "skip" => break,
                "q" | "quit" | "exit" => return Ok(status),
                _ => {
                    writeln!(output, "Choose approve, suggest changes, next, or quit.")?;
                    continue;
                }
            };
            let request = DecisionRequest {
                checkpoint: id.clone(),
                action,
                notes,
                source: "review".into(),
                agent: agent.into(),
                expected_revision: Some(revision.clone()),
            };
            match submit(request) {
                Ok(result) => {
                    status = status.max(result.exit_code);
                    writeln!(output, "{}", result.body)?;
                }
                Err(error) if error.code() == "stale_checkpoint_revision" => {
                    writeln!(
                        output,
                        "[WARN] Stale checkpoint revision. Reload and review explicitly; decision skipped."
                    )?;
                    status = status.max(1);
                }
                Err(error) => return Err(error),
            }
            break;
        }
    }
    Ok(status)
}

#[cfg(test)]
mod tests;
