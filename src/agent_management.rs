//! Caller-supplied provider hook receipts, never native identity or execution authority.
use crate::{Error, Result, error::require, task_mutation::TaskOutput};
use serde_json::{Value, json};
use std::{
    io::Read,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[cfg(unix)]
#[path = "agent_management/files.rs"]
mod files;
#[cfg(unix)]
#[path = "agent_management/native.rs"]
pub mod native;
#[path = "agent_management/store.rs"]
#[cfg(unix)]
mod store;
#[cfg(unix)]
#[path = "agent_management/unix.rs"]
mod unix;

#[cfg(all(test, unix))]
#[path = "agent_management/receipt_trace.rs"]
pub(crate) mod receipt_trace;

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
pub struct RunRequest {
    pub workspace: PathBuf,
    pub socket: String,
}

pub fn run_with_projection(
    request: &RunRequest,
    projection: Arc<Mutex<Value>>,
) -> Result<TaskOutput> {
    run_with_projection_and_cancel(request, projection, None)
}

/// Optional cooperative cancellation; the receiver cleans up before returning.
pub fn run_with_projection_and_cancel(
    request: &RunRequest,
    projection: Arc<Mutex<Value>>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<TaskOutput> {
    #[cfg(unix)]
    {
        unix::run(request, projection, cancel)
    }
    #[cfg(not(unix))]
    {
        let _ = (request, projection, cancel);
        Err(Error("agent_unsupported_platform"))
    }
}

pub fn native_report_arguments(socket: &str, json: bool) -> Result<TaskOutput> {
    let mut payload = Vec::new();
    std::io::stdin()
        .lock()
        .take(8193)
        .read_to_end(&mut payload)?;
    native_report_payload(socket, &payload, json)
}

pub fn native_report_payload(socket: &str, payload: &[u8], json: bool) -> Result<TaskOutput> {
    #[cfg(unix)]
    {
        let params = native::params(&decode(payload, 8192)?, true)?;
        output(
            unix::call_contract(socket, native::RPC, "report", params)?,
            json,
        )
    }
    #[cfg(not(unix))]
    {
        let _ = (socket, payload, json);
        Err(Error("agent_unsupported_platform"))
    }
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
        matches!(provider, "claude" | "codex" | "copilot" | "pi" | "opencode"),
        "agent_unsupported_provider",
    )?;
    require(
        (matches!(provider, "codex" | "copilot") && value == "SessionStart")
            || (provider == "pi" && value == "session_start")
            || (provider == "opencode" && matches!(value, "session.created" | "session.updated"))
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
        matches!(request.provider.as_str(), "claude" | "codex" | "copilot"),
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

/// Capture Coven's extension envelope, not a native provider hook payload.
pub fn extension_report_arguments(request: &ReportRequest, json: bool) -> Result<TaskOutput> {
    require(cfg!(unix), "agent_unsupported_platform")?;
    extension_event(request)?;
    let mut payload = Vec::new();
    std::io::stdin()
        .lock()
        .take((MAX_PAYLOAD + 1) as u64)
        .read_to_end(&mut payload)?;
    extension_report_payload(request, &payload, json)
}

pub fn extension_report_payload(
    request: &ReportRequest,
    payload: &[u8],
    json: bool,
) -> Result<TaskOutput> {
    require(cfg!(unix), "agent_unsupported_platform")?;
    let expected = extension_event(request)?;
    let value = decode(payload, MAX_PAYLOAD)?;
    require(
        value.as_object().is_some_and(|o| o.len() == 3),
        "agent_invalid_schema",
    )?;
    require(
        text(&value, "contract")?
            == if request.provider == "pi" {
                "coven.pi-extension.v1"
            } else {
                "coven.opencode-extension.v1"
            },
        "agent_contract_mismatch",
    )?;
    let name = event(&request.provider, text(&value, "event")?)?;
    require(expected == name, "agent_event_mismatch")?;
    let sid = session(text(&value, "session_id")?)?;
    // Provider/event identifies extension interpretation, not authenticated native
    // origin. Keep the persisted v1 record unchanged.
    let params = json!({"provider":request.provider,"event":name,"session_id":sid});
    output(call(&request.socket, "report", params)?, json)
}

fn extension_event(request: &ReportRequest) -> Result<&str> {
    require(
        matches!(request.provider.as_str(), "pi" | "opencode"),
        "agent_unsupported_provider",
    )?;
    event(
        &request.provider,
        request
            .event
            .as_deref()
            .ok_or(Error("agent_invalid_event"))?,
    )
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
