//! Caller-supplied provider hook receipts, never native identity or execution authority.
use crate::{Error, Result, error::require, task_mutation::TaskOutput};
use serde_json::{Value, json};
use std::{
    io::Read,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[path = "agent_management/store.rs"]
mod store;
#[cfg(unix)]
#[path = "agent_management/unix.rs"]
mod unix;

pub const CONTRACT: &str = "coven.agent-management.v1";
const MAX_PAYLOAD: usize = 1_048_576;
const MAX_RECORDS: usize = 128;

#[derive(Clone, Debug)]
pub struct ServeRequest {
    pub workspace: PathBuf,
    pub socket: String,
    pub duration: u64,
}

#[derive(Clone, Debug)]
pub struct ReportRequest {
    pub socket: String,
    pub provider: String,
    pub event: Option<String>,
}

#[derive(Clone, Debug)]
pub struct InventoryRequest {
    pub socket: String,
}

fn decode(bytes: &[u8], limit: usize) -> Result<Value> {
    require(bytes.len() <= limit, "agent_payload_limit")?;
    crate::strict_json::check_depth(bytes, 32)?;
    let native = crate::values::parse(bytes, crate::values::Profile::Authority)?;
    Ok(serde_json::from_slice(&native.compact()?)?)
}

pub(crate) fn event<'a>(provider: &str, value: &'a str) -> Result<&'a str> {
    require(
        matches!(provider, "claude" | "codex"),
        "agent_unsupported_provider",
    )?;
    require(
        (provider == "codex" && value == "SessionStart")
            || (provider == "claude"
                && matches!(
                    value,
                    "SessionStart"
                        | "UserPromptSubmit"
                        | "PreToolUse"
                        | "PostToolUse"
                        | "Stop"
                        | "SessionEnd"
                )),
        "agent_invalid_event",
    )?;
    Ok(value)
}

fn session(value: &str) -> Result<&str> {
    require(
        !value.trim().is_empty()
            && value.len() <= 512
            && !value.trim_start().starts_with('-')
            && !value.chars().any(char::is_control),
        "agent_invalid_session",
    )?;
    Ok(value)
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(Error("agent_invalid_schema"))
}

fn output(value: Value, json_mode: bool) -> Result<TaskOutput> {
    let stdout = if !json_mode {
        if let Some(id) = value.get("record_id").and_then(Value::as_str) {
            format!("Accepted {id}")
        } else {
            return crate::session_support::output(&value, true);
        }
    } else {
        return crate::session_support::output(&value, false);
    };
    Ok(TaskOutput {
        stdout,
        warnings: Vec::new(),
        exit_code: 0,
    })
}

pub fn serve_arguments(request: &ServeRequest, json: bool) -> Result<TaskOutput> {
    serve_with_projection(request, json, None)
}

pub fn serve_with_projection(
    request: &ServeRequest,
    json: bool,
    projection: Option<Arc<Mutex<Value>>>,
) -> Result<TaskOutput> {
    require(
        (1..=3600).contains(&request.duration),
        "agent_invalid_duration",
    )?;
    #[cfg(unix)]
    {
        unix::serve(request, json, projection)
    }
    #[cfg(not(unix))]
    {
        let _ = (request, json, projection);
        Err(Error("agent_unsupported_platform"))
    }
}

pub fn report_arguments(request: &ReportRequest, json: bool) -> Result<TaskOutput> {
    require(cfg!(unix), "agent_unsupported_platform")?;
    let mut payload = Vec::new();
    std::io::stdin()
        .lock()
        .take((MAX_PAYLOAD + 1) as u64)
        .read_to_end(&mut payload)?;
    report_payload(request, &payload, json)
}

pub fn report_payload(request: &ReportRequest, payload: &[u8], json: bool) -> Result<TaskOutput> {
    require(cfg!(unix), "agent_unsupported_platform")?;
    require(
        matches!(request.provider.as_str(), "claude" | "codex"),
        "agent_unsupported_provider",
    )?;
    let value = decode(payload, MAX_PAYLOAD)?;
    let sid = session(text(&value, "session_id")?)?;
    let name = text(&value, "hook_event_name")?;
    require(
        request
            .event
            .as_deref()
            .is_none_or(|expected| expected == name),
        "agent_event_mismatch",
    )?;
    // SubagentStop is not a main-session event and must not revive that session.
    if request.provider != "claude" || name != "SubagentStop" {
        event(&request.provider, name)?;
    }
    if request.provider == "claude" {
        if let Some(agent) = value.get("agent_id") {
            require(agent.is_null() || agent.is_string(), "agent_invalid_schema")?;
        }
        if name == "SubagentStop"
            || value
                .get("agent_id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty())
        {
            return output(json!({"type":"ignored", "reason":"subagent"}), json);
        }
    }
    // Construct a new object before transport. Unknown fields never cross this boundary.
    let params = json!({"provider":request.provider, "event":name, "session_id":sid});
    output(call(&request.socket, "report", params)?, json)
}

pub fn inventory_arguments(request: &InventoryRequest, json: bool) -> Result<TaskOutput> {
    output(call(&request.socket, "inventory", json!({}))?, json)
}

fn call(socket: &str, method: &str, params: Value) -> Result<Value> {
    #[cfg(unix)]
    {
        unix::call(socket, method, params)
    }
    #[cfg(not(unix))]
    {
        let _ = (socket, method, params);
        Err(Error("agent_unsupported_platform"))
    }
}

#[cfg(all(test, unix))]
#[path = "agent_management/tests.rs"]
mod tests;
