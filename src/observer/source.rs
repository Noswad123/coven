//! Protocol-18 projection and best-effort snapshot brackets. No lifecycle authority.
use super::model::{Agent, Inventory, Output, digest, truncate};
use crate::{
    Error, Result,
    observation_socket::{self as socket, NoCancel, ReadBudget, ReadMethod},
    values::Value as N,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashSet, time::Instant};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Request {
    Inventory {},
    Output { terminal_id: String },
}

fn field<'a>(v: &'a N, key: &str) -> Result<&'a N> {
    v.as_object()
        .and_then(|o| o.get(key))
        .ok_or(Error("schema_error"))
}
fn string<'a>(v: &'a N, key: &str) -> Result<&'a str> {
    field(v, key)?.as_str().ok_or(Error("schema_error"))
}
fn optional(v: &N, key: &str) -> Result<Option<String>> {
    match v.as_object().and_then(|o| o.get(key)) {
        None | Some(N::Null) => Ok(None),
        Some(N::String(s)) => {
            let mut text: String = s.chars().filter(|c| !c.is_control()).collect();
            text = text.trim().to_owned();
            let mut end = text.len().min(512);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            Ok((!text.is_empty()).then_some(text))
        }
        _ => Err(Error("schema_error")),
    }
}
fn locator(v: &N, key: &str) -> Result<String> {
    let s = string(v, key)?;
    if s.is_empty() || s.len() > 256 || s.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(Error("schema_error"));
    }
    Ok(s.into())
}
fn result(root: &N, id: &str, kind: &str) -> Result<N> {
    if string(root, "id")? != id {
        return Err(Error("schema_error"));
    }
    let object = root.as_object().ok_or(Error("schema_error"))?;
    if object.get("error").is_some() {
        return Err(Error(if object.get("result").is_some() {
            "schema_error"
        } else {
            "upstream_rejected"
        }));
    }
    let result = field(root, "result")?;
    if string(result, "type")? != kind {
        return Err(Error("schema_error"));
    }
    Ok(result.clone())
}
fn read(
    path: &str,
    method: ReadMethod,
    params: &Value,
    deadline: Instant,
    budget: &mut ReadBudget,
) -> Result<socket::Reply> {
    socket::read(path, method, params, deadline, budget, &NoCancel)
        .map_err(|e| Error(safe_code(&e.code)))
}
pub(super) fn safe_code(code: &str) -> &'static str {
    match code {
        "timeout" => "timeout",
        "response_limit" => "response_limit",
        "unsupported_protocol" => "unsupported_protocol",
        "endpoint_replaced" | "terminal_replaced" => "terminal_replaced",
        "schema_error" | "invalid_json" | "duplicate_key" | "invalid_utf8" | "nesting_limit" => {
            "schema_error"
        }
        "endpoint_busy" => "endpoint_busy",
        "stale_reference" => "stale_reference",
        "process_discovery_unavailable" => "process_discovery_unavailable",
        "pane_mapping_changed" | "server_changed" | "process_observation_changed" => {
            "terminal_replaced"
        }
        _ => "upstream_unavailable",
    }
}

fn snapshot(path: &str, deadline: Instant, budget: &mut ReadBudget) -> Result<Inventory> {
    let reply = read(path, ReadMethod::Snapshot, &json!({}), deadline, budget)?;
    project_snapshot(&reply).map(|(inventory, _)| inventory)
}

fn project_snapshot(reply: &socket::Reply) -> Result<(Inventory, N)> {
    let root = crate::observation_wire::decode_native(&reply.payload)
        .map_err(|_| Error("schema_error"))?;
    let result = result(&root, &reply.request_id, "session_snapshot")?;
    let snapshot = field(&result, "snapshot")?;
    if crate::observation_wire::to_json(field(snapshot, "protocol")?)? != json!(18) {
        return Err(Error("unsupported_protocol"));
    }
    let version = string(snapshot, "version")?;
    let generation = digest(&[
        &reply.endpoint.resolved,
        &reply.endpoint.device.to_string(),
        &reply.endpoint.inode.to_string(),
        version,
        "18",
    ]);
    let panes = field(snapshot, "panes")?
        .as_array()
        .ok_or(Error("schema_error"))?;
    let raw = field(snapshot, "agents")?
        .as_array()
        .ok_or(Error("schema_error"))?;
    if raw.len() > 128 || panes.len() > 1024 {
        return Err(Error("response_limit"));
    }
    let (mut all_terminals, mut all_panes) = (HashSet::new(), HashSet::new());
    for pane in panes {
        if !all_terminals.insert(locator(pane, "terminal_id")?)
            || !all_panes.insert(locator(pane, "pane_id")?)
        {
            return Err(Error("schema_error"));
        }
        locator(pane, "workspace_id")?;
        locator(pane, "tab_id")?;
    }
    let mut agents = Vec::new();
    let (mut terminals, mut pane_ids) = (HashSet::new(), HashSet::new());
    for a in raw {
        let terminal_id = locator(a, "terminal_id")?;
        let pane_id = locator(a, "pane_id")?;
        if !terminals.insert(terminal_id.clone()) || !pane_ids.insert(pane_id.clone()) {
            return Err(Error("schema_error"));
        }
        let workspace_id = locator(a, "workspace_id")?;
        let tab_id = locator(a, "tab_id")?;
        let matching: Vec<_> = panes
            .iter()
            .filter(|p| {
                ["terminal_id", "pane_id", "workspace_id", "tab_id"]
                    .iter()
                    .all(|k| string(p, k).ok() == string(a, k).ok())
            })
            .collect();
        if matching.len() != 1 {
            return Err(Error("schema_error"));
        }
        let pane = matching[0];
        let status = string(a, "agent_status")?;
        let skipped = match a
            .as_object()
            .and_then(|o| o.get("screen_detection_skipped"))
        {
            None | Some(N::Bool(false)) => false,
            Some(N::Bool(true)) => true,
            _ => return Err(Error("schema_error")),
        };
        let activity = if !skipped && matches!(status, "working" | "idle" | "blocked") {
            status
        } else {
            "unknown"
        };
        let provider = optional(a, "agent")?;
        let title = optional(a, "name")?
            .or(optional(a, "terminal_title")?)
            .or(optional(a, "title")?)
            .or(optional(pane, "name")?)
            .or(optional(pane, "terminal_title")?)
            .or(optional(pane, "title")?)
            .or_else(|| provider.clone())
            .unwrap_or_else(|| "Agent".into());
        agents.push(Agent {
            terminal_id,
            pane_id,
            workspace_id,
            tab_id,
            title,
            provider_candidates: provider
                .iter()
                .filter(|s| super::model::provider_token(s))
                .cloned()
                .collect(),
            provider,
            project: optional(a, "cwd")?.or(optional(pane, "cwd")?),
            activity: activity.into(),
            evidence: "kitsune_snapshot".into(),
        });
    }
    let unclassified_panes = panes.len().saturating_sub(agents.len());
    Ok((
        Inventory {
            generation,
            agents,
            discovery_status: "unsupported".into(),
            unclassified_panes,
        },
        snapshot.clone(),
    ))
}

fn inventory(path: &str, deadline: Instant, budget: &mut ReadBudget) -> Result<Inventory> {
    let acquisition = crate::agent_discovery::acquire(
        crate::agent_discovery::AcquisitionRequest {
            socket: path,
            pane: None,
            expected_terminal: None,
            scan_id: "",
            allow_unsupported: true,
            pane_limit: 1024,
        },
        deadline,
        &NoCancel,
        budget,
        &mut json!({"endpoint":{}}),
    )
    .map_err(|failure| {
        Error(match failure.code.as_str() {
            "endpoint_replaced"
            | "pane_mapping_changed"
            | "server_changed"
            | "process_observation_changed" => "terminal_replaced",
            "unsupported_protocol" => "unsupported_protocol",
            "timeout" => "timeout",
            "response_limit" => "response_limit",
            "schema_error" => "schema_error",
            _ => "process_discovery_unavailable",
        })
    })?;
    let (mut inventory, snapshot) = project_snapshot(&acquisition.snapshot)?;
    if acquisition.supported {
        inventory.discovery_status = "observed".into();
        let panes = field(&snapshot, "panes")?
            .as_array()
            .ok_or(Error("schema_error"))?;
        for target in acquisition.targets {
            let candidates: Vec<String> =
                serde_json::from_value(target["provider_candidates"].clone())
                    .map_err(|_| Error("schema_error"))?;
            if candidates.is_empty() {
                continue;
            }
            let pane = panes
                .iter()
                .find(|pane| {
                    ["terminal_id", "pane_id", "workspace_id", "tab_id"]
                        .iter()
                        .all(|key| string(pane, key).ok() == target[*key].as_str())
                })
                .ok_or(Error("terminal_replaced"))?;
            let terminal_id = locator(pane, "terminal_id")?;
            if inventory
                .agents
                .iter()
                .any(|agent| agent.terminal_id == terminal_id)
            {
                continue;
            }
            let provider = if candidates.len() == 1 {
                candidates.first().cloned()
            } else {
                None
            };
            let title = optional(pane, "name")?
                .or(optional(pane, "terminal_title")?)
                .or(optional(pane, "title")?)
                .or_else(|| provider.clone())
                .unwrap_or_else(|| "Agent".into());
            inventory.agents.push(Agent {
                terminal_id,
                pane_id: locator(pane, "pane_id")?,
                workspace_id: locator(pane, "workspace_id")?,
                tab_id: locator(pane, "tab_id")?,
                title,
                provider,
                project: optional(pane, "cwd")?,
                activity: "unknown".into(),
                evidence: "coven_process_discovery".into(),
                provider_candidates: candidates,
            });
        }
        inventory.unclassified_panes = panes.len().saturating_sub(inventory.agents.len());
    }
    let inventory = super::parse_inventory(
        serde_json::to_value(inventory).map_err(|_| Error("schema_error"))?,
    )?;
    socket::check(deadline, &NoCancel).map_err(|e| Error(safe_code(&e.code)))?;
    Ok(inventory)
}

pub(super) fn execute(path: &str, request: &Request, deadline: Instant) -> Result<Value> {
    socket::validate_path(path)?;
    if matches!(request, Request::Output { terminal_id } if terminal_id.is_empty()
            || terminal_id.len() > 256
            || terminal_id
                .chars()
                .any(|c| c.is_control() || c.is_whitespace()))
    {
        return Err(Error("invalid_reference"));
    }
    let mut budget = ReadBudget::new(4 * 1024 * 1024);
    let Request::Output { terminal_id } = request else {
        return serde_json::to_value(inventory(path, deadline, &mut budget)?)
            .map_err(|_| Error("schema_error"));
    };
    let before = snapshot(path, deadline, &mut budget)?;
    let expected = before
        .agents
        .iter()
        .find(|a| a.terminal_id == *terminal_id)
        .ok_or(Error("terminal_replaced"))?;
    let reply = read(
        path,
        ReadMethod::PaneRead,
        &json!({"pane_id":expected.pane_id,"source":"recent_unwrapped","format":"text","lines":256,"strip_ansi":true}),
        deadline,
        &mut budget,
    )?;
    let root = crate::observation_wire::decode_terminal_output(&reply.payload)
        .map_err(|_| Error("schema_error"))?;
    let result = result(&root, &reply.request_id, "pane_read")?;
    let read = field(&result, "read")?;
    if string(read, "pane_id")? != expected.pane_id
        || string(read, "workspace_id")? != expected.workspace_id
        || string(read, "tab_id")? != expected.tab_id
        || string(read, "source")? != "recent_unwrapped"
        || string(read, "format")? != "text"
    {
        return Err(Error("terminal_replaced"));
    }
    let mut text = string(read, "text")?.to_owned();
    let upstream_truncated = match field(read, "truncated")? {
        N::Bool(b) => *b,
        _ => return Err(Error("schema_error")),
    };
    let truncated = truncate(&mut text) || upstream_truncated;
    let after = snapshot(path, deadline, &mut budget)?;
    let reply_generation = digest(&[
        &reply.endpoint.resolved,
        &reply.endpoint.device.to_string(),
        &reply.endpoint.inode.to_string(),
        // snapshot generation also includes the source version; compare socket identity separately below.
        "output",
    ]);
    let selected = socket::select(path).map_err(|e| Error(safe_code(&e.code)))?;
    let selected_generation = digest(&[
        &selected.resolved,
        &selected.device.to_string(),
        &selected.inode.to_string(),
        "output",
    ]);
    if before.generation != after.generation
        || reply_generation != selected_generation
        || !after.agents.iter().any(|a| expected.same_terminal(a))
    {
        return Err(Error("terminal_replaced"));
    }
    socket::check(deadline, &NoCancel).map_err(|e| Error(safe_code(&e.code)))?;
    serde_json::to_value(Output {
        generation: before.generation,
        terminal_id: terminal_id.clone(),
        text,
        truncated,
    })
    .map_err(|_| Error("schema_error"))
}
