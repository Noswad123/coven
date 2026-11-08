//! Read-only legacy presentation. Admission covers observation, never caches authority.
use super::{CommandResult, TIMEOUT, enabled, namespace, root, transport::Backend};
use crate::{
    Error, Result,
    error::require,
    runtime_command::NativeRunner,
    transport_gate::TransportGuard,
    values::{self, Value},
    workspace::{self, WorkspaceState},
    workspace_commands::python,
};
use std::path::Path;

mod standup;
#[cfg(test)]
mod tests;

pub(crate) fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_object()?.get(key)
}
pub(crate) fn text<'a>(value: &'a Value, key: &str, default: &'a str) -> &'a str {
    field(value, key).and_then(Value::as_str).unwrap_or(default)
}
pub(crate) fn label(team: &Value, agent: &str, markdown: bool) -> String {
    let name = field(team, "display_names")
        .and_then(|v| field(v, agent))
        .and_then(Value::as_str)
        .unwrap_or(agent);
    let mut visible = String::new();
    for ch in name.chars() {
        if crate::unicode::printable(ch) {
            visible.push(ch);
        } else {
            visible.push_str(&format!("\\u{:04x}", ch as u32));
        }
    }
    if !markdown {
        return if name == agent {
            agent.into()
        } else {
            format!("{visible} ({agent})")
        };
    }
    let mut escaped = String::new();
    for ch in visible.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '.' | '!'
            | '|' | '~' | '-' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn identifier(agent: &str) -> Result<()> {
    require(
        !agent.is_empty()
            && agent
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "invalid_record_identifier",
    )
}

fn observe(
    root: &Path,
    state: &WorkspaceState,
    gate: &TransportGuard,
    agent: &str,
    backend: impl FnOnce(&str) -> Result<Option<String>>,
) -> Result<(String, Option<String>)> {
    identifier(agent)?;
    if enabled(root, gate)? {
        return Ok(("unverified (inspect action transport records)".into(), None));
    }
    let pane = backend(&namespace(state)?)?;
    Ok((
        if pane.is_some() { "running" } else { "stopped" }.into(),
        pane,
    ))
}

pub(crate) fn status_for(workspace: &Path, agent: &str) -> Result<(String, Option<String>)> {
    let root = root(workspace)?;
    let gate = TransportGuard::shared(&root, TIMEOUT)?;
    let state = workspace::load(gate.root())?;
    observe(gate.root(), &state, &gate, agent, |namespace| {
        Backend::new(&state.multiplexer, &NativeRunner)?.pane(namespace, agent)
    })
}

pub fn agent_status(workspace: &Path, agents: &[String]) -> Result<CommandResult> {
    let root = root(workspace)?;
    let gate = TransportGuard::shared(&root, TIMEOUT)?;
    let state = workspace::load(gate.root())?;
    let roster = state.team.agent_ids()?;
    let selected: Vec<&str> = if agents.is_empty() {
        roster.clone()
    } else {
        agents.iter().map(String::as_str).collect()
    };
    // Validate the entire batch before constructing any backend or observing a pane.
    for agent in &selected {
        require(roster.contains(agent), "unknown_agent")?;
    }
    let states = state
        .agents
        .as_array()
        .ok_or(Error("invalid_workspace_record_list"))?;
    let mut out = python::Output::default();
    for (index, agent) in selected.iter().enumerate() {
        let (runtime, pane) = observe(gate.root(), &state, &gate, agent, |namespace| {
            Backend::new(&state.multiplexer, &NativeRunner)?.pane(namespace, agent)
        })?;
        let saved = states
            .iter()
            .rev()
            .find(|item| text(item, "id", "") == *agent);
        if index > 0 {
            out.push("\n")?;
        }
        out.push(&format!(
            "{}: {}={runtime}{} state=",
            label(state.team.value(), agent, false),
            state.multiplexer,
            pane.map(|p| format!(" pane={p}")).unwrap_or_default()
        ))?;
        out.value(
            saved
                .and_then(|v| field(v, "status"))
                .unwrap_or(&Value::String("unknown".into())),
        )?;
    }
    Ok(CommandResult {
        body: out.finish(),
        exit_code: 0,
    })
}

pub fn standup(workspace: &Path) -> Result<CommandResult> {
    let root = root(workspace)?;
    let gate = TransportGuard::shared(&root, TIMEOUT)?;
    let state = workspace::load(gate.root())?;
    let body = standup::render(gate.root(), &state, |agent| {
        observe(gate.root(), &state, &gate, agent, |namespace| {
            Backend::new(&state.multiplexer, &NativeRunner)?.pane(namespace, agent)
        })
    })?;
    require(
        body.len() <= values::MAX_OUTPUT,
        "unsupported_history_output_profile",
    )?;
    Ok(CommandResult { body, exit_code: 0 })
}
