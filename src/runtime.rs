//! Legacy runtime commands and checkpoint notice transport.
//! Shared admission precedes workspace exclusion and lasts through delivery/history.
//! Native CLI children use null stdin, a 60s deadline and 1 MiB combined output;
//! unlike probes these commands may start server-owned processes. Failure is not
//! evidence that a native effect did not happen. Never retry delivery implicitly.
use crate::{
    Error, Result, actions,
    error::require,
    logs::{self, LogKind},
    platform,
    runtime_command::{NativeRunner, Runner},
    storage::WorkspaceGuard,
    task_delivery, task_time,
    transport_gate::TransportGuard,
    values::{self, Value},
    workspace::{self, WorkspaceState},
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

mod bootstrap;
mod lifecycle;
pub(crate) mod presentation;
#[cfg(test)]
mod tests;
mod transport;
pub use bootstrap::bootstrap;
pub use lifecycle::{AgentAction, RuntimeAction, agent_change, lifecycle};
pub use presentation::{agent_status, standup};

const TIMEOUT: Duration = Duration::from_secs(5);
const CONTROL: &str = "state/runtime-launch-control.json";

#[derive(Debug)]
/// Command output without a terminal newline; the CLI supplies the final newline.
pub struct CommandResult {
    pub body: String,
    pub exit_code: u8,
}

fn root(workspace: &Path) -> Result<PathBuf> {
    platform::ensure_supported()?;
    let root = platform::workspace_argument(workspace)?.canonicalize()?;
    require(root.is_dir(), "invalid_runtime_workspace")?;
    root.to_str().ok_or(Error("non_unicode_path_unsupported"))?;
    Ok(root)
}

// This observation is made only while admitted. Validation includes orphan
// bundles and relocation, even when no mutable team configuration can be read.
fn enabled(root: &Path, gate: &TransportGuard) -> Result<bool> {
    gate.validate_for(root, false)?;
    actions::validate_workspace_control(root)?;
    Ok(workspace::optional_bytes(
        root,
        Path::new(CONTROL),
        values::MAX_INPUT,
        "unsupported_action_input_profile",
    )?
    .is_some())
}

pub(crate) fn legacy(root: &Path, gate: &TransportGuard) -> Result<()> {
    require(!enabled(root, gate)?, "legacy_runtime_disabled")
}

fn targets(state: &WorkspaceState, selector: &str) -> Result<Vec<String>> {
    let agents = state.team.agent_ids()?;
    if selector == "all" {
        return Ok(agents.into_iter().map(str::to_owned).collect());
    }
    require(agents.contains(&selector), "unknown_agent")?;
    Ok(vec![selector.to_owned()])
}

fn namespace(state: &WorkspaceState) -> Result<String> {
    let name = state
        .team
        .value()
        .as_object()
        .and_then(|team| team.get("name"))
        .and_then(Value::as_str)
        .ok_or(Error("invalid_team_record"))?;
    Ok(format!("coven-{}", crate::workspace_assets::slug(name)?))
}

fn text_profile(text: &str) -> Result<()> {
    require(!text.contains('\0'), "invalid_runtime_text")?;
    require(
        text.len() <= values::MAX_STRING,
        "unsupported_json_string_profile",
    )
}

/// Exact persisted identities, never display-name/slug aliases. This snapshot is
/// not authority for a later send_notice: that function obtains its own gate.
pub(crate) fn notice_targets(root: &Path, selector: &str) -> Result<Vec<String>> {
    let gate = TransportGuard::shared(root, TIMEOUT)?;
    legacy(gate.root(), &gate)?;
    targets(&workspace::load(gate.root())?, selector)
}

pub(crate) fn send_notice(root: &Path, agent: &str, text: &str) -> Result<bool> {
    notice_with(root, agent, text, &NativeRunner)
}

fn notice_with(root: &Path, agent: &str, text: &str, runner: &dyn Runner) -> Result<bool> {
    notice_using(root, agent, text, |_, multiplexer| {
        transport::Backend::new(multiplexer, runner)
    })
}

fn notice_using<'a>(
    root: &Path,
    agent: &str,
    text: &str,
    backend: impl FnOnce(&Path, &str) -> Result<transport::Backend<'a>>,
) -> Result<bool> {
    let gate = TransportGuard::shared(root, TIMEOUT)?;
    let root = gate.root();
    legacy(root, &gate)?;
    let state = workspace::load(root)?;
    // "all" is a selector only for target enumeration, not a literal pane ID.
    require(state.team.agent_ids()?.contains(&agent), "unknown_agent")?;
    text_profile(text)?;
    let backend = backend(root, &state.multiplexer)?;
    backend.send(&namespace(&state)?, agent, text)
}

pub fn send(workspace: &Path, selector: &str, text: &str) -> Result<CommandResult> {
    send_with(&root(workspace)?, selector, text, &NativeRunner)
}

fn object(entries: Vec<(&str, &str)>) -> Result<Value> {
    Value::object(
        entries
            .into_iter()
            .map(|(key, value)| (key.into(), Value::String(value.into())))
            .collect(),
    )
}

fn send_with(
    root: &Path,
    selector: &str,
    text: &str,
    runner: &dyn Runner,
) -> Result<CommandResult> {
    send_using(root, selector, text, |_, multiplexer| {
        transport::Backend::new(multiplexer, runner)
    })
}

fn send_using<'a>(
    root: &Path,
    selector: &str,
    text: &str,
    backend: impl FnOnce(&Path, &str) -> Result<transport::Backend<'a>>,
) -> Result<CommandResult> {
    let gate = TransportGuard::shared(root, TIMEOUT)?;
    let root = gate.root();
    legacy(root, &gate)?;
    workspace::load(root)?;
    let guard = WorkspaceGuard::acquire(root, TIMEOUT)?;
    legacy(root, &gate)?;
    let state = workspace::load(root)?;
    let agents = targets(&state, selector)?;
    let namespace = namespace(&state)?;
    text_profile(text)?;
    task_delivery::validate_snapshot_logs(root)?;
    task_delivery::validate_dashboard_inputs(root)?;
    task_delivery::prepare_dashboard_bytes(root)?;
    // Prepare every deterministic record and aggregate capacity before any
    // lookup/effect. Missing panes leave their reserved history unwritten.
    let mut prepared = Vec::new();
    let (mut message_bytes, mut message_lines) =
        logs::read_log(root, LogKind::Messages.relative())?;
    let (mut event_bytes, mut event_lines) = logs::read_log(root, LogKind::Events.relative())?;
    for agent in &agents {
        let message = logs::encode_record(&object(vec![
            ("ts", &task_time::now()?),
            ("from", "user"),
            ("to", agent),
            ("body", text),
        ])?)?;
        let mut events = Vec::new();
        for (kind, payload) in [
            (
                "message.sent",
                vec![("sender", "user"), ("recipient", agent.as_str())],
            ),
            ("agent.message.sent", vec![("agent", agent.as_str())]),
        ] {
            let mut record = object(vec![
                ("event_id", &task_time::uuid_hyphenated()?),
                ("ts", &task_time::now()?),
                ("type", kind),
            ])?;
            for (key, value) in payload {
                record.set_field(key, Value::String(value.into()))?;
            }
            events.push(logs::encode_record(&record)?);
        }
        message_bytes += message.len();
        message_lines += 1;
        event_bytes += events.iter().map(Vec::len).sum::<usize>();
        event_lines += events.len();
        require(
            message_bytes <= logs::MAX_LOG && event_bytes <= logs::MAX_LOG,
            "unsupported_event_log_size_profile",
        )?;
        require(
            message_lines <= logs::MAX_LINES && event_lines <= logs::MAX_LINES,
            "unsupported_event_log_line_profile",
        )?;
        prepared.push((message, events));
    }
    let backend = backend(root, &state.multiplexer)?;
    drop(guard);
    let mut missing = Vec::new();
    for (agent, (message, events)) in agents.iter().zip(prepared) {
        if backend.send(&namespace, agent, text)? {
            // Native lookup/text/Enter must never run under workspace exclusion.
            // Delivery already happened: a history failure cannot justify retry.
            let history = (|| -> Result<()> {
                let guard = WorkspaceGuard::acquire(root, TIMEOUT)?;
                legacy(root, &gate)?;
                workspace::load(root)?;
                task_delivery::validate_snapshot_logs(root)?;
                logs::append_encoded(root, LogKind::Messages, &message, &guard)?;
                for event in events {
                    logs::append_encoded(root, LogKind::Events, &event, &guard)?;
                }
                Ok(())
            })();
            if let Err(error) = history {
                return Ok(CommandResult {
                    body: format!(
                        "[ERROR] Message delivered to {agent}; history update failed and may be partial ({}); no retry performed",
                        error.code()
                    ),
                    exit_code: 1,
                });
            }
        } else {
            missing.push(agent.as_str());
        }
    }
    let dashboard = (|| -> Result<()> {
        let guard = WorkspaceGuard::acquire(root, TIMEOUT)?;
        legacy(root, &gate)?;
        task_delivery::refresh_dashboard(root, &guard)
    })();
    if let Err(error) = dashboard {
        return Ok(CommandResult {
            body: format!(
                "[ERROR] Delivery completed for {} agent(s); dashboard update failed ({}); no retry performed",
                agents.len() - missing.len(),
                error.code()
            ),
            exit_code: 1,
        });
    }
    let mut body = String::new();
    if !missing.is_empty() {
        body.push_str(&format!(
            "[INFO] No running pane found for: {}\n",
            missing.join(", ")
        ));
    }
    body.push_str(&format!(
        "[INFO] Sent message to {} agent(s)",
        agents.len() - missing.len()
    ));
    Ok(CommandResult { body, exit_code: 0 })
}

pub fn down(workspace: &Path) -> Result<CommandResult> {
    down_with(&root(workspace)?, &NativeRunner)
}

fn down_with(root: &Path, runner: &dyn Runner) -> Result<CommandResult> {
    down_using(root, |_, multiplexer| {
        transport::Backend::new(multiplexer, runner)
    })
}

fn down_using<'a>(
    root: &Path,
    backend: impl FnOnce(&Path, &str) -> Result<transport::Backend<'a>>,
) -> Result<CommandResult> {
    let gate = TransportGuard::shared(root, TIMEOUT)?;
    let root = gate.root();
    if enabled(root, &gate)? {
        let result = actions::mutation::stop_current(root)?;
        return Ok(CommandResult {
            body: result.body,
            exit_code: result.exit_code,
        });
    }
    workspace::load(root)?;
    let guard = WorkspaceGuard::acquire(root, TIMEOUT)?;
    legacy(root, &gate)?;
    let state = workspace::load(root)?;
    let namespace = namespace(&state)?;
    task_delivery::validate_snapshot_logs(root)?;
    let backend = backend(root, &state.multiplexer)?;
    task_delivery::append_event(
        root,
        &format!("{}.down.requested", state.multiplexer),
        &object(vec![])?,
        &guard,
    )?;
    drop(guard);
    backend.down(&namespace)
}
