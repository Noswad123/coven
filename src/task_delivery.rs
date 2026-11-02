//! Secondary task publication only. Never repairs logs or task projections, and
//! never rolls back an already committed task when either publication fails.
//! JSONL: 8 MiB total/file, 65536 physical LF-terminated lines (including blanks),
//! and the finite LegacySnapshot value profile per record. Python is unbounded.
mod unicode;

use crate::{
    Error, Result, checkpoints,
    error::require,
    platform, read_fs, records,
    storage::{self, Delivery, WorkspaceGuard},
    task_time,
    values::{self, Object, Profile, Value},
    workspace::{self, WorkspaceState},
};
use std::{fs::File, io::Write, path::Path};

const MAX_LOG: usize = 8 * 1_048_576;
const MAX_LINES: usize = 65_536;
const MAX_TEXT: usize = 1_048_576;
const MAX_DASHBOARD: usize = 32 * 1_048_576;
const CHECKPOINTS: [(&str, &str); 2] = [
    ("plan-approval", "consult user before implementation"),
    (
        "pre-delivery-review",
        "consult user before declaring completion",
    ),
];

fn optional_bytes(
    root: &Path,
    relative: &str,
    limit: usize,
    size_code: &'static str,
) -> Result<Option<Vec<u8>>> {
    let path = read_fs::managed(root, Path::new(relative), false)?;
    match std::fs::symlink_metadata(&path) {
        Ok(_) => read_fs::read_regular(&path, limit)
            .map(Some)
            .map_err(|error| match error.code() {
                "handoff_size_limit" => Error(size_code),
                _ => error,
            }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn validate_log(bytes: &[u8]) -> Result<usize> {
    require(bytes.len() <= MAX_LOG, "unsupported_event_log_size_profile")?;
    require(
        bytes.is_empty() || bytes.last() == Some(&b'\n'),
        "event_log_unterminated_line",
    )?;
    let mut count = 0;
    // Split LF only: CR remains JSON whitespace, never a record separator.
    for raw in bytes.split_inclusive(|c| *c == b'\n') {
        count += 1;
        require(count <= MAX_LINES, "unsupported_event_log_line_profile")?;
        let text =
            std::str::from_utf8(&raw[..raw.len() - 1]).map_err(|_| Error("invalid_jsonl_utf8"))?;
        if records::trimmed(text).is_empty() {
            continue;
        }
        // Do not trim nonblank records: json.loads accepts JSON whitespace only.
        let record = values::parse(text.as_bytes(), Profile::LegacySnapshot).map_err(|error| {
            // Add history context only to record errors, never resource/profile
            // refusals whose specific limits remain actionable to the caller.
            match error.code() {
                "invalid_compat_json"
                | "invalid_compat_json_string"
                | "invalid_json_trailing_data"
                | "invalid_json_number"
                | "unsupported_nonfinite_json"
                | "unsupported_or_invalid_json_scalar" => Error("invalid_jsonl_record"),
                _ => error,
            }
        })?;
        require(record.as_object().is_some(), "invalid_jsonl_record")?;
    }
    Ok(count)
}

fn read_log(root: &Path, relative: &str) -> Result<(usize, usize)> {
    let bytes = optional_bytes(
        root,
        relative,
        MAX_LOG,
        "unsupported_event_log_size_profile",
    )?
    .unwrap_or_default();
    Ok((bytes.len(), validate_log(&bytes)?))
}

/// Snapshot callers run this before task commit. Ledger callers intentionally
/// defer log *content* checks to secondary publication after the task commit.
pub(crate) fn validate_snapshot_logs(root: &Path) -> Result<()> {
    read_log(root, "logs/events.jsonl")?;
    read_log(root, "logs/messages.jsonl")?;
    Ok(())
}

/// Refresh discards history ordering but validates every explicit checkpoint
/// reference, including non-request records. This never restores decisions.
pub(crate) fn validate_checkpoint_history(root: &Path) -> Result<()> {
    for log in ["logs/messages.jsonl", "logs/events.jsonl"] {
        let bytes = optional_bytes(root, log, MAX_LOG, "unsupported_event_log_size_profile")?
            .unwrap_or_default();
        // Python parses the whole file before validating any reference in it.
        validate_log(&bytes)?;
        for raw in bytes.split_inclusive(|byte| *byte == b'\n') {
            let text = std::str::from_utf8(&raw[..raw.len() - 1])?;
            if records::trimmed(text).is_empty() {
                continue;
            }
            let record = values::parse(text.as_bytes(), Profile::LegacySnapshot)?;
            if let Some(reference) = record
                .as_object()
                .and_then(|object| object.get("checkpoint"))
            {
                let id = reference
                    .as_str()
                    .ok_or(Error("invalid_checkpoint_history_reference"))?;
                checkpoints::inspect_bytes(id, None)?; // ID only, not referenced decision grammar.
                if let Some(raw) = optional_bytes(
                    root,
                    &format!("checkpoints/{id}.md"),
                    MAX_TEXT,
                    "unsupported_checkpoint_history_profile",
                )? {
                    std::str::from_utf8(&raw)?;
                }
            }
        }
    }
    Ok(())
}

trait AppendIo {
    fn open(&self, path: &Path) -> Result<File> {
        platform::open_append(path)
    }
    fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
        file.write_all(bytes)
    }
    fn sync(&self, file: &File) -> std::io::Result<()> {
        file.sync_all()
    }
}
struct SystemAppendIo;
impl AppendIo for SystemAppendIo {}

pub(crate) fn append_event(
    root: &Path,
    event_type: &str,
    payload: &Value,
    guard: &WorkspaceGuard,
) -> Result<()> {
    append_with_metadata(
        root,
        event_type,
        payload,
        guard,
        &task_time::uuid_hyphenated()?,
        &task_time::now()?,
        &SystemAppendIo,
    )
}

fn append_with_metadata(
    root: &Path,
    event_type: &str,
    payload: &Value,
    guard: &WorkspaceGuard,
    event_id: &str,
    ts: &str,
    io: &impl AppendIo,
) -> Result<()> {
    guard.validate_for(root)?;
    workspace::load(root)?; // append_event's outer full preflight
    let mut event = Value::object(vec![
        ("event_id".into(), Value::String(event_id.into())),
        ("ts".into(), Value::String(ts.into())),
        ("type".into(), Value::String(event_type.into())),
    ])?;
    for (key, value) in payload
        .as_object()
        .ok_or(Error("invalid_event_payload"))?
        .entries()
    {
        // Python's {metadata, **payload}: replacement retains the original slot.
        event.set_field(key, value.clone())?;
    }
    read_log(root, "logs/events.jsonl")?;
    let mut encoded = event.compact()?;
    encoded.push(b'\n');
    // Check the actual complete encoded record, not only the caller's value.
    require(validate_log(&encoded)? == 1, "invalid_encoded_event")?;
    workspace::load(root)?; // workspace_mutation's pre-context preflight
    guard.validate_for(root)?;
    workspace::load(root)?; // borrowed held-context preflight
    let (bytes, lines) = read_log(root, "logs/events.jsonl")?;
    require(
        encoded.len() <= MAX_LOG - bytes,
        "unsupported_event_log_size_profile",
    )?;
    require(lines < MAX_LINES, "unsupported_event_log_line_profile")?;
    // Python's append_event wrapper creates only this managed parent, after
    // admission and under the held guard, with mkdir's default umask policy.
    let logs = read_fs::managed(root, Path::new("logs"), true)?;
    std::fs::create_dir_all(&logs)?;
    read_fs::managed(root, Path::new("logs"), true)?;
    let path = read_fs::managed(root, Path::new("logs/events.jsonl"), false)?;
    // No truncate, tail repair, deduplication, or rollback. A failed write
    // or sync can leave evidence behind; future writes must validate it afresh.
    let mut file = io.open(&path)?;
    io.write(&mut file, &encoded)
        .and_then(|()| io.sync(&file))
        .map_err(|_| Error("event_append_may_be_partial"))
}

struct CapturedDashboard {
    state: WorkspaceState,
    goal: String,
    statuses: [String; 2],
}

fn goal(root: &Path) -> Result<String> {
    let bytes = optional_bytes(
        root,
        "goal.md",
        MAX_TEXT,
        "unsupported_dashboard_input_profile",
    )?
    .unwrap_or_default();
    Ok(goal_text(std::str::from_utf8(&bytes)?))
}

fn goal_text(raw: &str) -> String {
    let text = raw.replace("\r\n", "\n").replace('\r', "\n");
    let text = records::trimmed(&text);
    records::trimmed(text.strip_prefix("# Goal").unwrap_or(text)).to_owned()
}

fn capture(root: &Path) -> Result<CapturedDashboard> {
    let state = workspace::load(root)?;
    let goal = goal(root)?;
    let mut statuses = [String::new(), String::new()];
    for (index, (id, _)) in CHECKPOINTS.iter().enumerate() {
        let bytes = optional_bytes(
            root,
            &format!("checkpoints/{id}.md"),
            MAX_TEXT,
            "unsupported_dashboard_input_profile",
        )?;
        // Checkpoint revisions bind raw bytes, not universal-newline goal text.
        let view = checkpoints::inspect_bytes(id, bytes.as_deref())?;
        statuses[index] = field(object(&view)?, "effective_status")?.to_owned();
    }
    Ok(CapturedDashboard {
        state,
        goal,
        statuses,
    })
}

pub(crate) fn refresh_dashboard(root: &Path, guard: &WorkspaceGuard) -> Result<()> {
    refresh_with(root, guard, &SystemDashboardIo)
}

/// Prepare only; refresh stages all generated outputs before any file write and
/// repeats preparation under its borrowed workspace guard.
pub(crate) fn prepare_dashboard_bytes(root: &Path) -> Result<Vec<u8>> {
    Ok(render(&capture(root)?)?.into_bytes())
}

/// Init has validated all state in memory before a workspace marker exists.
/// Reuse the formatter without relaxing ordinary capture or guard admission.
pub(crate) fn prepare_initial_dashboard(state: WorkspaceState, goal: &str) -> Result<Vec<u8>> {
    Ok(render(&CapturedDashboard {
        state,
        goal: goal_text(goal),
        statuses: ["pending".to_owned(), "pending".to_owned()],
    })?
    .into_bytes())
}

trait DashboardIo {
    fn capture(&self, root: &Path, _guard: &WorkspaceGuard) -> Result<CapturedDashboard> {
        capture(root)
    }
    fn publish(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        storage::atomic_replace_bytes(guard, Path::new("dashboard.md"), bytes)
    }
}
struct SystemDashboardIo;
impl DashboardIo for SystemDashboardIo {}

fn refresh_with(root: &Path, guard: &WorkspaceGuard, io: &impl DashboardIo) -> Result<()> {
    guard.validate_for(root)?;
    workspace::load(root)?;
    goal(root)?;
    workspace::load(root)?;
    guard.validate_for(root)?;
    workspace::load(root)?;
    // Capture, render and replace all remain within the caller's borrowed guard.
    let captured = io.capture(root, guard)?;
    let rendered = render(&captured)?;
    std::str::from_utf8(rendered.as_bytes())?;
    match io.publish(guard, rendered.as_bytes())? {
        Delivery::Committed => Ok(()),
        Delivery::DurabilityUncertain => Err(Error("dashboard_synchronization_uncertain")),
    }
}

fn object(value: &Value) -> Result<&Object> {
    value.as_object().ok_or(Error("invalid_dashboard_record"))
}
fn field<'a>(value: &'a Object, name: &str) -> Result<&'a str> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or(Error("invalid_dashboard_record"))
}

struct Markdown(String);
impl Markdown {
    fn push(&mut self, text: &str) -> Result<()> {
        require(
            text.len() <= MAX_DASHBOARD - self.0.len(),
            "unsupported_dashboard_output_profile",
        )?;
        self.0.push_str(text);
        Ok(())
    }
    fn parts(&mut self, parts: &[&str]) -> Result<()> {
        for part in parts {
            self.push(part)?;
        }
        Ok(())
    }
    fn label(&mut self, text: &str) -> Result<()> {
        for character in text.chars() {
            if unicode::printable(character) {
                self.visible_character(character)?;
            } else {
                // Python :04x means *minimum* four digits, including astral Cn.
                for character in format!("\\u{:04x}", u32::from(character)).chars() {
                    self.visible_character(character)?;
                }
            }
        }
        Ok(())
    }
    fn visible_character(&mut self, character: char) -> Result<()> {
        match character {
            '&' => self.push("&amp;"),
            '<' => self.push("&lt;"),
            '>' => self.push("&gt;"),
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '.' | '!'
            | '|' | '~' | '-' => {
                self.push("\\")?;
                self.push(character.encode_utf8(&mut [0; 4]))
            }
            _ => self.push(character.encode_utf8(&mut [0; 4])),
        }
    }
}

fn render(captured: &CapturedDashboard) -> Result<String> {
    let state = &captured.state;
    let team = object(state.team.value())?;
    let mut out = Markdown(String::new());
    out.parts(&[
        "# Coven Dashboard: ",
        field(team, "name")?,
        "\n\n## Goal\n\n",
        if captured.goal.is_empty() {
            "_No goal set yet._"
        } else {
            &captured.goal
        },
        "\n\n## Coven Lead\n\n",
    ])?;
    if matches!(state.lead, Value::Null) {
        out.push("None set.")?;
    } else {
        let lead = object(&state.lead)?;
        out.parts(&[
            "`",
            field(lead, "agent")?,
            "` \u{2014} set ",
            lead.get("set_at")
                .and_then(Value::as_str)
                .unwrap_or("unknown time"),
            ". Stop condition: ",
            lead.get("until")
                .and_then(Value::as_str)
                .unwrap_or("goal achieved or critical blocker"),
            ".",
        ])?;
    }
    out.push(
        "\n\n## Agents\n\n| Agent ID | Display name | Status | Role |\n| --- | --- | --- | --- |\n",
    )?;
    let names = team.get("display_names").and_then(Value::as_object);
    let agents = state
        .agents
        .as_array()
        .ok_or(Error("invalid_dashboard_agents"))?;
    for (index, agent) in agents.iter().enumerate() {
        if index > 0 {
            out.push("\n")?;
        }
        let agent = object(agent)?;
        let id = field(agent, "id")?;
        out.parts(&["| ", id, " | "])?;
        out.label(
            names
                .and_then(|names| names.get(id))
                .and_then(Value::as_str)
                .unwrap_or(id),
        )?;
        out.parts(&[
            " | ",
            field(agent, "status")?,
            " | ",
            field(agent, "role")?,
            " |",
        ])?;
    }
    out.push("\n\n## Tasks\n\n| ID | Status | Owner | Title |\n| --- | --- | --- | --- |\n")?;
    let tasks = state
        .tasks
        .as_array()
        .ok_or(Error("invalid_dashboard_tasks"))?;
    if tasks.is_empty() {
        out.push("| - | - | - | No tasks yet |")?;
    }
    for (index, task) in tasks.iter().enumerate() {
        if index > 0 {
            out.push("\n")?;
        }
        let task = object(task)?;
        out.parts(&[
            "| `",
            field(task, "id")?,
            "` | ",
            field(task, "status")?,
            " | ",
            task.get("owner")
                .and_then(Value::as_str)
                .filter(|owner| !owner.is_empty())
                .unwrap_or("unassigned"),
            " | ",
            field(task, "title")?,
            " |",
        ])?;
    }
    out.push(
        "\n\n## Checkpoints\n\n| ID | Status | File | Purpose |\n| --- | --- | --- | --- |\n",
    )?;
    for (index, (id, purpose)) in CHECKPOINTS.iter().enumerate() {
        out.parts(&[
            "| `",
            id,
            "` | ",
            &captured.statuses[index],
            " | `checkpoints/",
            id,
            ".md` | ",
            purpose,
            " |\n",
        ])?;
    }
    out.parts(&["\n## Work output\n\n- In-progress build files: `work/`\n- Final/demo artifacts: `output/`\n\n## Monitoring\n\n- Events: `logs/events.jsonl`\n- Messages: `logs/messages.jsonl`\n- Runtime multiplexer: `", &state.multiplexer,
        "`\n- Tmux manifest (for optional tmux runtime): `tmux/coven.toml`\n"])?;
    Ok(out.0)
}

#[cfg(test)]
mod tests;
