//! Consume a closed set of native fields; never project unknown extension values.
use crate::agent_processes::ProcessFact;
use crate::observation_socket::{Failure, Reply, WireResult};
use crate::values::{Number, Value as N};
use serde_json::{Value, json};
use std::collections::HashSet;

const LOCATORS: [&str; 4] = ["pane_id", "terminal_id", "workspace_id", "tab_id"];

fn require(ok: bool) -> WireResult<()> {
    if ok {
        Ok(())
    } else {
        Err(Failure::new("schema_error"))
    }
}

fn field<'a>(value: &'a N, key: &str) -> WireResult<&'a N> {
    value
        .as_object()
        .and_then(|o| o.get(key))
        .ok_or_else(|| Failure::new("schema_error"))
}

fn optional<'a>(value: &'a N, key: &str) -> Option<&'a N> {
    value.as_object().and_then(|o| o.get(key))
}

fn string(value: &N, limit: usize) -> WireResult<&str> {
    let text = value.as_str().ok_or_else(|| Failure::new("schema_error"))?;
    require(text.len() <= limit && !text.contains('\0'))?;
    Ok(text)
}

fn uint(value: &N, max: u64) -> WireResult<u64> {
    let N::Number(Number::Integer(number)) = value else {
        return Err(Failure::new("schema_error"));
    };
    let number = number
        .as_str()
        .parse::<u64>()
        .map_err(|_| Failure::new("schema_error"))?;
    require(number <= max)?;
    Ok(number)
}

fn result<'a>(root: &'a N, id: &str, kind: &str) -> WireResult<&'a N> {
    require(string(field(root, "id")?, 4096)? == id)?;
    require(optional(root, "result").is_some() != optional(root, "error").is_some())?;
    if let Some(error) = optional(root, "error") {
        string(field(error, "code")?, 64)?;
        string(field(error, "message")?, 4096)?;
        return Err(Failure::new("server_error"));
    }
    let result = field(root, "result")?;
    require(string(field(result, "type")?, 64)? == kind)?;
    Ok(result)
}

pub(super) struct Snapshot {
    pub version: String,
    pub protocol: u64,
    pub descriptor: Value,
    panes: Vec<Value>,
}

impl Snapshot {
    pub fn selected(&self, pane: Option<&str>) -> WireResult<Vec<Value>> {
        let mut selected: Vec<_> = self
            .panes
            .iter()
            .filter(|p| pane.is_none_or(|id| p["pane_id"].as_str() == Some(id)))
            .cloned()
            .collect();
        if pane.is_some() && selected.is_empty() {
            return Err(Failure::new("pane_not_found"));
        }
        if selected.len() > 64 {
            return Err(Failure::new("pane_limit"));
        }
        selected.sort_by(|a, b| a["pane_id"].as_str().cmp(&b["pane_id"].as_str()));
        Ok(selected)
    }
}

pub(super) fn snapshot(reply: &Reply) -> WireResult<Snapshot> {
    let root = crate::observation_wire::decode_native(&reply.payload)
        .map_err(|e| Failure::new(e.code()))?;
    let result = result(&root, &reply.request_id, "session_snapshot")?;
    let snapshot = field(result, "snapshot")?;
    let version = string(field(snapshot, "version")?, 4096)?.to_owned();
    require(!version.is_empty())?;
    let protocol = uint(field(snapshot, "protocol")?, u32::MAX.into())?;
    if protocol != 18 {
        return Err(Failure::new("unsupported_protocol"));
    }
    let descriptor = (|| -> WireResult<Value> {
        let d = field(snapshot, "process_observation")?;
        require(uint(field(d,"version")?, u32::MAX.into())? == 1)?;
        for (key, expected) in [("platform","linux"),("scope","pane_foreground_cohort"),("selection","unclassified")] {
            require(string(field(d,key)?, 64)? == expected)?;
        }
        Ok(json!({"version":1,"platform":"linux","scope":"pane_foreground_cohort","selection":"unclassified"}))
    })().map_err(|_| Failure::new("process_observation_unsupported"))?;
    let rows = field(snapshot, "panes")?
        .as_array()
        .ok_or_else(|| Failure::new("schema_error"))?;
    if rows.len() > 1024 {
        return Err(Failure::new("pane_limit"));
    }
    let (mut panes, mut terminals) = (HashSet::new(), HashSet::new());
    let mut projected = Vec::new();
    for row in rows {
        let mut out = json!({});
        for key in LOCATORS {
            let text = string(field(row, key)?, 16384)?;
            require(crate::kitsune_observation::session_opaque(text))?;
            out[key] = json!(text);
        }
        require(
            panes.insert(out["pane_id"].as_str().unwrap().to_owned())
                && terminals.insert(out["terminal_id"].as_str().unwrap().to_owned()),
        )?;
        projected.push(out);
    }
    Ok(Snapshot {
        version,
        protocol,
        descriptor,
        panes: projected,
    })
}

pub(super) fn processes(reply: &Reply, pane: &str) -> WireResult<Vec<ProcessFact>> {
    let root = crate::observation_wire::decode_native(&reply.payload)
        .map_err(|e| Failure::new(e.code()))?;
    let result = result(&root, &reply.request_id, "pane_process_info")?;
    let info = field(result, "process_info")?;
    require(string(field(info, "pane_id")?, 16384)? == pane)?;
    for key in ["shell_pid", "foreground_process_group_id"] {
        if let Some(value) = optional(info, key).filter(|v| !matches!(v, N::Null)) {
            require(uint(value, u32::MAX.into())? > 0)?;
        }
    }
    if let Some(value) = optional(info, "tty").filter(|v| !matches!(v, N::Null)) {
        string(value, 4096)?;
    }
    let Some(rows) = optional(info, "foreground_processes") else {
        return Ok(Vec::new());
    };
    let rows = rows
        .as_array()
        .ok_or_else(|| Failure::new("schema_error"))?;
    if rows.len() > 256 {
        return Err(Failure::new("process_limit"));
    }
    let mut pids = HashSet::new();
    let mut facts = Vec::new();
    for row in rows {
        let pid = uint(field(row, "pid")?, u32::MAX.into())? as u32;
        require(pid > 0 && pids.insert(pid))?;
        let name = string(field(row, "name")?, 4096)?.to_owned();
        let mut text_bytes = name.len();
        let argv0 = optional(row, "argv0")
            .filter(|v| !matches!(v, N::Null))
            .map(|v| string(v, 4096).map(str::to_owned))
            .transpose()?;
        text_bytes += argv0.as_ref().map_or(0, String::len);
        let argv = optional(row, "argv")
            .filter(|v| !matches!(v, N::Null))
            .map(|v| {
                let args = v.as_array().ok_or_else(|| Failure::new("schema_error"))?;
                require(args.len() <= 128)?;
                args.iter()
                    .map(|v| string(v, 4096).map(str::to_owned))
                    .collect::<WireResult<Vec<_>>>()
            })
            .transpose()?;
        text_bytes += argv
            .as_ref()
            .map_or(0, |args| args.iter().map(String::len).sum());
        for key in ["cmdline", "cwd"] {
            if let Some(value) = optional(row, key).filter(|v| !matches!(v, N::Null)) {
                text_bytes += string(value, 65536)?.len();
            }
        }
        require(text_bytes <= 65536)?;
        facts.push(ProcessFact {
            pid,
            name,
            argv0,
            argv,
        });
    }
    Ok(facts)
}
