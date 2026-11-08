//! Mutable lead metadata, with independent best-effort history and notice delivery.
use crate::{
    Error, Result,
    error::require,
    logs::{self, LogKind},
    platform, read_fs, records, runtime,
    storage::{self, Delivery, WorkspaceGuard},
    task_delivery, task_time,
    transport_gate::TransportGuard,
    values::{self, Value},
    workspace,
};
use std::{path::Path, time::Duration};

mod prompt;
#[cfg(test)]
mod tests;
const TIMEOUT: Duration = Duration::from_secs(5);
const LEAD: &str = "state/lead.json";

#[derive(Debug)]
pub struct CommandResult {
    /// No terminal newline. Warnings belong on stderr, not in this body.
    pub body: String,
    pub warnings: Vec<String>,
    pub exit_code: u8,
}
#[derive(Debug, Clone)]
pub enum Operation {
    Show,
    Set(String),
    Clear,
}

trait Io {
    fn now(&self) -> Result<String> {
        task_time::now()
    }
    fn replace(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        storage::atomic_replace_bytes(guard, Path::new(LEAD), bytes)
    }
    fn unlink(&self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }
    fn sync_dir(&self, path: &Path) -> std::io::Result<()> {
        platform::sync_directory(path)
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
    fn message(
        &self,
        root: &Path,
        agent: &str,
        message: &str,
        guard: &WorkspaceGuard,
    ) -> Result<()> {
        workspace::load(root)?;
        let record = object(vec![
            ("ts", string(&self.now()?)),
            ("from", string("user")),
            ("to", string(agent)),
            ("body", string(message)),
        ])?;
        let bytes = logs::encode_record(&record)?;
        task_delivery::validate_snapshot_logs(root)?;
        logs::append_encoded(root, LogKind::Messages, &bytes, guard)?;
        self.event(
            root,
            "message.sent",
            &object(vec![
                ("sender", string("user")),
                ("recipient", string(agent)),
            ])?,
            guard,
        )
    }
    fn dashboard(&self, root: &Path, guard: &WorkspaceGuard) -> Result<()> {
        task_delivery::refresh_dashboard(root, guard)
    }
    fn notice(&self, root: &Path, agent: &str, message: &str) -> Result<bool> {
        runtime::send_notice(root, agent, message)
    }
}
struct SystemIo;
impl Io for SystemIo {}
fn string(text: &str) -> Value {
    Value::String(text.into())
}
fn object(entries: Vec<(&str, Value)>) -> Result<Value> {
    Value::object(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
fn enabled(root: &Path, gate: &TransportGuard) -> Result<bool> {
    gate.validate_for(root, false)?;
    crate::actions::validate_workspace_control(root)?;
    Ok(workspace::optional_bytes(
        root,
        Path::new("state/runtime-launch-control.json"),
        values::MAX_INPUT,
        "unsupported_action_input_profile",
    )?
    .is_some())
}
fn goal(root: &Path) -> Result<String> {
    let bytes = workspace::optional_bytes(
        root,
        Path::new("goal.md"),
        workspace::MAX_TEXT,
        "unsupported_workspace_text_profile",
    )?
    .unwrap_or_default();
    let text = std::str::from_utf8(&bytes)?
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let text = records::trimmed(&text);
    Ok(records::trimmed(text.strip_prefix("# Goal").unwrap_or(text)).into())
}

fn candidate(previous: &Value, agent: &str, set_at: &str) -> Result<Vec<u8>> {
    let mut lead = if matches!(previous, Value::Null) {
        object(vec![])?
    } else {
        previous.clone()
    };
    for (key, value) in [
        ("agent", agent),
        ("set_at", set_at),
        (
            "until",
            "goal achieved or critical blocker requiring the user",
        ),
        (
            "instructions",
            "Coordinate using supported task/message operations; runtime launch approval is separate.",
        ),
    ] {
        lead.set_field(key, string(value))?;
    }
    let bytes = lead.storage()?;
    // Publication must fit the reader, including pretty-print expansion and the
    // nodes added by metadata. Publish these exact bytes, not a second encoding.
    records::lead(&values::parse(&bytes, values::Profile::LegacySnapshot)?)?;
    Ok(bytes)
}

fn validate(
    state: &workspace::WorkspaceState,
    root: &Path,
    operation: &Operation,
    message: Option<&str>,
) -> Result<()> {
    if let Operation::Set(agent) = operation {
        require(
            state.team.agent_ids()?.contains(&agent.as_str()),
            "unknown_agent",
        )?;
        let goal = goal(root)?;
        if let Some(message) = message {
            require(
                message.len() <= values::MAX_STRING,
                "unsupported_json_string_profile",
            )?;
            require(!message.contains('\0'), "invalid_runtime_text")?;
        }
        if message.is_none_or(str::is_empty) {
            require(
                prompt::render(root, &goal).len() <= values::MAX_STRING,
                "unsupported_json_string_profile",
            )?;
        }
        // Empty time admits deterministic candidate shape before any workspace
        // lock. The actual timestamp and reloaded state are checked at commit.
        candidate(&state.lead, agent, "")?;
    }
    Ok(())
}
fn warning(result: &mut CommandResult, clear: bool, outcome: Result<()>) {
    if let Err(error) = outcome {
        result.warnings.push(format!(
            "[WARN] Lead {}; secondary update failed: {}",
            if clear { "cleared" } else { "committed" },
            error.code()
        ));
    }
}

pub fn command(
    workspace: &Path,
    operation: Operation,
    message: Option<&str>,
) -> Result<CommandResult> {
    command_with(workspace, operation, message, &SystemIo)
}

fn command_with(
    workspace: &Path,
    operation: Operation,
    message: Option<&str>,
    io: &impl Io,
) -> Result<CommandResult> {
    platform::ensure_supported()?;
    let root = platform::workspace_argument(workspace)?.canonicalize()?;
    root.to_str().ok_or(Error("non_unicode_path_unsupported"))?;
    let state = workspace::load(&root)?;
    validate(&state, &root, &operation, message)?;
    let mut result = CommandResult {
        body: String::new(),
        warnings: Vec::new(),
        exit_code: 0,
    };
    if matches!(operation, Operation::Show) {
        result.body = if matches!(state.lead, Value::Null) {
            "No coven lead set.".into()
        } else {
            let agent = runtime::presentation::text(&state.lead, "agent", "");
            let (runtime, pane) = runtime::presentation::status_for(&root, agent)?;
            format!(
                "Coven lead: {} {}={runtime}{}\nSet: {}\nUntil: {}",
                runtime::presentation::label(state.team.value(), agent, false),
                state.multiplexer,
                pane.map(|p| format!(" pane={p}")).unwrap_or_default(),
                runtime::presentation::text(&state.lead, "set_at", "unknown time"),
                runtime::presentation::text(
                    &state.lead,
                    "until",
                    "goal achieved or critical blocker"
                )
            )
        };
        return Ok(result);
    }
    let gate = TransportGuard::shared(&root, TIMEOUT)?;
    enabled(&root, &gate)?;
    let guard = WorkspaceGuard::acquire(&root, TIMEOUT)?;
    let state = workspace::load(&root)?;
    validate(&state, &root, &operation, message)?;
    let approved = enabled(&root, &gate)?;
    let clear = matches!(operation, Operation::Clear);
    let (delivery, agent_message) = match operation {
        Operation::Clear => {
            let path = read_fs::managed(&root, Path::new(LEAD), false)?;
            let delivery = match io.unlink(&path) {
                Ok(()) => {
                    if io
                        .sync_dir(path.parent().ok_or(Error("invalid_replacement_parent"))?)
                        .is_ok()
                    {
                        Delivery::Committed
                    } else {
                        Delivery::DurabilityUncertain
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Delivery::Committed,
                Err(error) => return Err(error.into()),
            };
            (delivery, None)
        }
        Operation::Set(agent) => {
            let goal = goal(&root)?;
            let message = message
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| prompt::render(&root, &goal));
            require(
                message.len() <= values::MAX_STRING,
                "unsupported_json_string_profile",
            )?;
            let bytes = candidate(&state.lead, &agent, &io.now()?)?;
            (io.replace(&guard, &bytes)?, Some((agent, message)))
        }
        Operation::Show => unreachable!(),
    };
    if delivery == Delivery::DurabilityUncertain {
        result.exit_code = 1;
        result.warnings.push(format!("[ERROR] Lead {} but synchronization is uncertain; secondary updates and delivery skipped",
            if clear { "cleared" } else { "replaced" }));
        return Ok(result);
    }
    if let Some((agent, message)) = &agent_message {
        warning(
            &mut result,
            false,
            io.event(
                &root,
                "lead.set",
                &object(vec![("agent", string(agent))])?,
                &guard,
            ),
        );
        warning(
            &mut result,
            false,
            io.message(&root, agent, message, &guard),
        );
    } else {
        warning(
            &mut result,
            true,
            io.event(
                &root,
                "lead.cleared",
                &object(vec![("previous", state.lead)])?,
                &guard,
            ),
        );
    }
    warning(&mut result, clear, io.dashboard(&root, &guard));
    drop(guard);
    drop(gate);
    let Some((agent, message)) = agent_message else {
        result.body = "[INFO] Cleared coven lead".into();
        return Ok(result);
    };
    let mut sent = false;
    if approved {
        result.body.push_str("[INFO] Lead metadata recorded; legacy pane delivery is disabled in approved-launch mode\n");
    } else {
        match io.notice(&root, &agent, &message) {
            Ok(value) => {
                sent = value;
                if sent {
                    let history = (|| {
                        let guard = WorkspaceGuard::acquire(&root, TIMEOUT)?;
                        workspace::load(&root)?;
                        io.event(
                            &root,
                            "lead.message.sent",
                            &object(vec![("agent", string(&agent))])?,
                            &guard,
                        )
                    })();
                    if let Err(error) = history {
                        result.warnings.push(format!(
                            "[WARN] Lead committed; delivery unavailable: {}",
                            error.code()
                        ));
                        sent = false;
                    }
                }
            }
            Err(error) => result.warnings.push(format!(
                "[WARN] Lead committed; delivery unavailable: {}",
                error.code()
            )),
        }
    }
    if !sent {
        result
            .body
            .push_str(&format!("[INFO] No running pane found for: {agent}\n"));
    }
    result.body.push_str(&format!(
        "[INFO] Set {agent} as coven lead; notified {} agent(s)",
        u8::from(sent)
    ));
    Ok(result)
}
