//! Legacy lifecycle. Admission covers native effects, history and foreground
//! attach. The generated five-second prompt senders outlive this admission and
//! are intentionally not fenced (the legacy launch contract).
//! Launch drafts retain at most 8 MiB of script and agent-ID bytes per batch,
//! including when an agent command selects only part of the validated roster.
use super::{CommandResult, TIMEOUT, legacy, namespace, object, root, transport};
use crate::{
    Error, Result,
    error::require,
    runtime_command::NativeRunner,
    runtime_foreground,
    storage::WorkspaceGuard,
    task_delivery,
    transport_gate::TransportGuard,
    values::{self, Profile, Value},
    workspace::{self, WorkspaceState},
    workspace_assets,
};
use std::path::Path;

mod create;
pub(super) mod staging;
#[cfg(test)]
pub(super) mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeAction {
    Plan,
    Up,
    Restart,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentAction {
    Start,
    Stop,
    Restart,
}

pub fn lifecycle(workspace: &Path, action: RuntimeAction) -> Result<CommandResult> {
    let ttys = runtime_foreground::terminal_ttys();
    let in_tmux = std::env::var_os("TMUX").is_some_and(|v| !v.is_empty());
    lifecycle_using(
        &root(workspace)?,
        action,
        |_, multiplexer| transport::Backend::new(multiplexer, &NativeRunner),
        &runtime_foreground::NativeRunner,
        ttys,
        in_tmux,
    )
}

// Backend construction is injectable as well as command execution, so tests
// never select installed executables or contact a real server.
pub(super) fn lifecycle_using<'a>(
    root: &Path,
    action: RuntimeAction,
    backend: impl FnOnce(&Path, &str) -> Result<transport::Backend<'a>>,
    foreground: &dyn runtime_foreground::Runner,
    ttys: (bool, bool),
    in_tmux: bool,
) -> Result<CommandResult> {
    lifecycle_admitting(root, action, backend, foreground, ttys, in_tmux, |root| {
        TransportGuard::shared(root, TIMEOUT)
    })
}

fn lifecycle_admitting<'a>(
    root: &Path,
    action: RuntimeAction,
    backend: impl FnOnce(&Path, &str) -> Result<transport::Backend<'a>>,
    foreground: &dyn runtime_foreground::Runner,
    ttys: (bool, bool),
    in_tmux: bool,
    admit: impl FnOnce(&Path) -> Result<TransportGuard>,
) -> Result<CommandResult> {
    lifecycle_preview(root, action)?;
    let gate = admit(root)?;
    let root = gate.root();
    legacy(root, &gate)?;
    lifecycle_preview(root, action)?;
    let guard = WorkspaceGuard::acquire(root, TIMEOUT)?;
    legacy(root, &gate)?;
    let state = workspace::load(root)?;
    let ns = namespace(&state)?;
    let commands = if action == RuntimeAction::Plan {
        Vec::new()
    } else {
        launch_commands(root, &state)?
    };
    let monitors = if action == RuntimeAction::Plan {
        Vec::new()
    } else {
        monitors()?
    };
    let action_name = match action {
        RuntimeAction::Plan => "plan",
        RuntimeAction::Up => "up",
        RuntimeAction::Restart => "restart",
    };
    task_delivery::append_event(
        root,
        &format!("{}.{action_name}.requested", state.multiplexer),
        &object(vec![])?,
        &guard,
    )?;
    if action == RuntimeAction::Plan {
        return plan(root, &state, &ns);
    }
    let backend = backend(root, &state.multiplexer)?;
    drop(guard);
    let id = if state.multiplexer == "tmux" {
        let exists = backend.capture(&["has-session", "-t", &ns])?.code == 0;
        if exists && action == RuntimeAction::Restart {
            checked(&backend, &["kill-session", "-t", &ns])?;
        }
        if !exists || action == RuntimeAction::Restart {
            create::tmux(root, &ns, &commands, &monitors, &backend)?;
        }
        ns.clone()
    } else {
        let existing = backend.workspace(&ns)?;
        let existing = existing
            .as_ref()
            .map(|v| required_handle(v, "workspace_id"))
            .transpose()?;
        if action == RuntimeAction::Restart {
            if let Some(id) = &existing {
                checked(&backend, &["workspace", "close", id])?;
            }
        }
        match existing {
            Some(id) if action == RuntimeAction::Up => id,
            _ => create::herdr(root, &ns, &commands, &monitors, &backend)?,
        }
    };
    if !ttys.0 || !ttys.1 {
        return Ok(CommandResult {
            body: if state.multiplexer == "tmux" {
                format!("[INFO] Coven tmux session ready: {ns}")
            } else {
                format!("[INFO] Coven Herdr workspace ready: {ns} ({id})")
            },
            exit_code: 0,
        });
    }
    let code = if state.multiplexer == "tmux" {
        backend.foreground(
            &[
                if in_tmux {
                    "switch-client"
                } else {
                    "attach-session"
                },
                "-t",
                &ns,
            ],
            foreground,
        )?
    } else {
        checked(&backend, &["workspace", "focus", &id])?;
        0
    };
    Ok(CommandResult {
        body: String::new(),
        // Python passes subprocess.returncode to SystemExit, including negative
        // signal codes; the CLI process status retains its low eight bits.
        exit_code: code.rem_euclid(256) as u8,
    })
}

// Preview is read-only and never grants mutation authority. In particular, bad
// inputs must not initialize either lock file on a previously untouched workspace.
pub(super) fn preview_workspace(root: &Path) -> Result<WorkspaceState> {
    crate::actions::validate_workspace_control(root)?;
    require(
        workspace::optional_bytes(
            root,
            Path::new(super::CONTROL),
            values::MAX_INPUT,
            "unsupported_action_input_profile",
        )?
        .is_none(),
        "legacy_runtime_disabled",
    )?;
    workspace::load(root)
}

fn lifecycle_preview(root: &Path, action: RuntimeAction) -> Result<()> {
    let state = preview_workspace(root)?;
    namespace(&state)?;
    // Plan's missing-manifest diagnostic intentionally remains after its event.
    if action != RuntimeAction::Plan {
        launch_commands(root, &state)?;
        monitors()?;
    }
    Ok(())
}

fn plan(root: &Path, state: &WorkspaceState, ns: &str) -> Result<CommandResult> {
    let root_text = crate::read_fs::path_text(root)?;
    let mut body = if state.multiplexer == "tmux" {
        let manifest = crate::read_fs::managed(root, Path::new("tmux/coven.toml"), false)?;
        require(manifest.is_file(), "missing_tmux_manifest")?;
        format!(
            "Coven tmux plan: {ns}\nWorkspace: {root_text}\nManifest: {}\nWindows:\n- orchestrator: dashboard, events, messages, shell",
            crate::read_fs::path_text(&manifest)?
        )
    } else {
        format!(
            "Coven Herdr plan: {ns}\nWorkspace: {root_text}\nTabs:\n- orchestrator: dashboard, events, messages, shell panes"
        )
    };
    for agent in state.team.agent_ids()? {
        body.push_str(&format!("\n- agent-{agent}: {agent}"));
    }
    Ok(CommandResult { body, exit_code: 0 })
}

fn launch_commands(root: &Path, state: &WorkspaceState) -> Result<Vec<(String, String)>> {
    let team = state.team.value();
    let name = transport::field(team, "name").ok_or(Error("invalid_team_record"))?;
    let command = transport::field(team, "opencode_command")
        .filter(|v| !v.is_empty())
        .unwrap_or("oc");
    let mut draft = staging::Staged::new(staging::MAX_STAGING);
    for agent in state.team.agent_ids()? {
        for relative in [
            format!("prompts/{agent}.md"),
            format!("prompts/{agent}.start.txt"),
        ] {
            crate::read_fs::managed(root, Path::new(&relative), false)?;
        }
        let script = workspace_assets::agent_launch_command(
            name,
            &crate::read_fs::path_text(root)?,
            agent,
            command,
            &state.multiplexer,
        )?;
        let sizes = [agent.len(), script.len()];
        draft.push((agent.to_owned(), script), &sizes)?;
    }
    Ok(draft.finish())
}

fn monitors() -> Result<Vec<(&'static str, String)>> {
    let cli = match std::env::var("COVEN_COMMAND") {
        Ok(value) if !value.is_empty() => workspace_assets::shell_quote(&value),
        Ok(_) | Err(std::env::VarError::NotPresent) => "coven".into(),
        Err(_) => return Err(Error("unsupported_refresh_command_environment_profile")),
    };
    Ok(vec![
        (
            "events",
            format!("while true; do clear; {cli} events; sleep 2; done"),
        ),
        (
            "messages",
            format!("while true; do clear; {cli} messages; sleep 2; done"),
        ),
        (
            "shell",
            "printf 'coven workspace: '; pwd; exec /bin/zsh".into(),
        ),
    ])
}

fn checked(backend: &transport::Backend<'_>, args: &[&str]) -> Result<()> {
    require(
        backend.capture(args)?.code == 0,
        "runtime_backend_command_failed",
    )
}
fn payload(backend: &transport::Backend<'_>, args: &[&str]) -> Result<Value> {
    let result = backend.capture(args)?;
    require(result.code == 0, "runtime_backend_command_failed")?;
    let text = if result.stdout.is_empty() {
        "{}"
    } else {
        &result.stdout
    };
    let value = values::parse(text.as_bytes(), Profile::LegacySnapshot)?;
    let response = value
        .as_object()
        .ok_or(Error("invalid_runtime_backend_payload"))?
        .get("result");
    match response {
        Some(value) if value.as_object().is_some() => Ok(value.clone()),
        _ => object(vec![]),
    }
}
fn nested_handle(value: &Value, parent: &str, key: &str) -> Result<Option<String>> {
    match value.as_object().and_then(|v| v.get(parent)) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            require(
                value.as_object().is_some(),
                "invalid_runtime_backend_payload",
            )?;
            transport::handle(value, key)
        }
    }
}
fn required_handle(value: &Value, key: &str) -> Result<String> {
    transport::handle(value, key)?.ok_or(Error("missing_runtime_handle"))
}

pub fn agent_change(
    workspace: &Path,
    agents: &[String],
    action: AgentAction,
) -> Result<CommandResult> {
    agent_change_using(&root(workspace)?, agents, action, |_, multiplexer| {
        transport::Backend::new(multiplexer, &NativeRunner)
    })
}

pub(super) fn agent_change_using<'a>(
    root: &Path,
    agents: &[String],
    action: AgentAction,
    backend: impl FnOnce(&Path, &str) -> Result<transport::Backend<'a>>,
) -> Result<CommandResult> {
    agent_change_admitting(root, agents, action, backend, |root| {
        TransportGuard::shared(root, TIMEOUT)
    })
}

fn agent_change_admitting<'a>(
    root: &Path,
    agents: &[String],
    action: AgentAction,
    backend: impl FnOnce(&Path, &str) -> Result<transport::Backend<'a>>,
    admit: impl FnOnce(&Path) -> Result<TransportGuard>,
) -> Result<CommandResult> {
    agent_preview(root, agents, action)?;
    let gate = admit(root)?;
    let root = gate.root();
    legacy(root, &gate)?;
    agent_preview(root, agents, action)?;
    let guard = WorkspaceGuard::acquire(root, TIMEOUT)?;
    legacy(root, &gate)?;
    let state = workspace::load(root)?;
    require(!agents.is_empty(), "missing_agent")?;
    let roster = state.team.agent_ids()?;
    for agent in agents {
        require(roster.contains(&agent.as_str()), "unknown_agent")?;
    }
    let ns = namespace(&state)?;
    let commands = if action != AgentAction::Stop {
        launch_commands(root, &state)?
    } else {
        Vec::new()
    };
    let backend = backend(root, &state.multiplexer)?;
    drop(guard);
    let mut lines = Vec::new();
    let mut changed = 0;
    for agent in agents {
        if action != AgentAction::Start {
            let stopped = if backend.pane(&ns, agent)?.is_none() {
                lines.push(format!("[INFO] {agent} is not running"));
                false
            } else if state.multiplexer == "tmux" {
                checked(
                    &backend,
                    &["kill-window", "-t", &format!("{ns}:agent-{agent}")],
                )?;
                true
            } else if let Some(tab) = backend.tab(&ns, agent)? {
                checked(&backend, &["tab", "close", &tab])?;
                true
            } else {
                lines.push(format!("[INFO] {agent} is not running"));
                false
            };
            if stopped {
                if let Err(error) = agent_event(root, &gate, agent, &state.multiplexer, false) {
                    return Ok(progress_error(&format!("Stopped {agent}"), error));
                }
                if action == AgentAction::Stop {
                    changed += 1;
                }
            }
        }
        if action != AgentAction::Stop {
            if let Some(pane) = backend.pane(&ns, agent)? {
                lines.push(format!("[INFO] {agent} already running at {pane}"));
                continue;
            }
            let command = &commands
                .iter()
                .find(|(name, _)| name == agent)
                .ok_or(Error("unknown_agent"))?
                .1;
            create::agent(root, &ns, agent, command, &state.multiplexer, &backend)?;
            changed += 1;
            if let Err(error) = agent_event(root, &gate, agent, &state.multiplexer, true) {
                return Ok(progress_error(&format!("Started {agent}"), error));
            }
        }
    }
    let dashboard = (|| {
        let guard = WorkspaceGuard::acquire(root, TIMEOUT)?;
        legacy(root, &gate)?;
        task_delivery::refresh_dashboard(root, &guard)
    })();
    if let Err(error) = dashboard {
        return Ok(progress_error(
            &format!("Changed {changed} agent window(s)"),
            error,
        ));
    }
    let verb = match action {
        AgentAction::Start => "Started",
        AgentAction::Stop => "Stopped",
        AgentAction::Restart => "Restarted",
    };
    lines.push(format!("[INFO] {verb} {changed} agent window(s)"));
    Ok(CommandResult {
        body: lines.join("\n"),
        exit_code: 0,
    })
}

fn agent_preview(root: &Path, agents: &[String], action: AgentAction) -> Result<()> {
    let state = preview_workspace(root)?;
    require(!agents.is_empty(), "missing_agent")?;
    let roster = state.team.agent_ids()?;
    for agent in agents {
        require(roster.contains(&agent.as_str()), "unknown_agent")?;
    }
    namespace(&state)?;
    if action != AgentAction::Stop {
        launch_commands(root, &state)?;
    }
    Ok(())
}

fn agent_event(
    root: &Path,
    gate: &TransportGuard,
    agent: &str,
    multiplexer: &str,
    started: bool,
) -> Result<()> {
    let guard = WorkspaceGuard::acquire(root, TIMEOUT)?;
    legacy(root, gate)?;
    let verb = if started { "started" } else { "stopped" };
    let mut fields = vec![("agent", agent)];
    if multiplexer == "herdr" {
        fields.push(("multiplexer", "herdr"));
    }
    task_delivery::append_event(
        root,
        &format!(
            "agent.{verb}.{}",
            if multiplexer == "tmux" {
                "window"
            } else {
                "tab"
            }
        ),
        &object(fields)?,
        &guard,
    )
}

pub(super) fn progress_error(progress: &str, error: Error) -> CommandResult {
    CommandResult {
        body: format!(
            "[ERROR] {progress}; history/dashboard update failed and may be partial ({}); no retry performed",
            error.code()
        ),
        exit_code: 1,
    }
}
