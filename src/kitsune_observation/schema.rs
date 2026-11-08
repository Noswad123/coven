//! Distinct strict-agent and generic-pane contracts; never fall back between them.
use super::*;
use crate::values::{Number, Value as N};
use std::collections::HashSet;

pub(super) const LOCATORS: [&str; 4] = ["pane_id", "terminal_id", "workspace_id", "tab_id"];
#[derive(Debug)]
pub(super) struct Snapshot {
    projection: Value,
    // Preserve presence and the exact native value. Only publication consumes it.
    pub annotations: Option<N>,
}
impl std::ops::Deref for Snapshot {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.projection
    }
}
pub(super) fn require(ok: bool) -> WireResult<()> {
    if ok {
        Ok(())
    } else {
        Err(Failure::new("schema_error"))
    }
}
fn field<'a>(v: &'a N, k: &str) -> WireResult<&'a N> {
    v.as_object()
        .and_then(|o| o.get(k))
        .ok_or_else(|| Failure::new("schema_error"))
}
fn optional<'a>(v: &'a N, k: &str) -> &'a N {
    v.as_object().and_then(|o| o.get(k)).unwrap_or(&N::Null)
}
fn string(v: &N) -> WireResult<&str> {
    v.as_str().ok_or_else(|| Failure::new("schema_error"))
}
fn uint(v: &N, max: u64) -> WireResult<u64> {
    let N::Number(Number::Integer(n)) = v else {
        return Err(Failure::new("schema_error"));
    };
    let n = n
        .as_str()
        .parse::<u64>()
        .map_err(|_| Failure::new("schema_error"))?;
    require(n <= max)?;
    Ok(n)
}
fn boolean(v: &N) -> WireResult<()> {
    require(matches!(v, N::Bool(_)))
}
fn array(v: &N) -> WireResult<&[N]> {
    let a = v.as_array().ok_or_else(|| Failure::new("schema_error"))?;
    require(a.len() <= 1024)?;
    Ok(a)
}
fn json(v: &N) -> WireResult<Value> {
    crate::observation_wire::to_json(v).map_err(|e| Failure::new(e.code()))
}
fn project(v: &N, keys: &[&str]) -> WireResult<Value> {
    let mut out = json!({});
    for k in keys {
        out[*k] = json(field(v, k)?)?;
    }
    Ok(out)
}
fn status(v: &N) -> WireResult<()> {
    require(matches!(
        v.as_str(),
        Some("idle" | "working" | "blocked" | "done" | "unknown")
    ))
}
pub(super) fn opaque(s: &str) -> bool {
    !s.is_empty()
        && s.chars().count() <= 4096
        && s.chars()
            .all(|c| crate::unicode::printable(c) && !c.is_whitespace())
}
fn result(payload: &[u8], id: &str, kind: &str) -> WireResult<N> {
    let root =
        crate::observation_wire::decode_native(payload).map_err(|e| Failure::new(e.code()))?;
    require(field(&root, "id")?.as_str() == Some(id))?;
    let o = root
        .as_object()
        .ok_or_else(|| Failure::new("schema_error"))?;
    require(o.get("result").is_some() != o.get("error").is_some())?;
    if let Some(e) = o.get("error") {
        string(field(e, "code")?)?;
        string(field(e, "message")?)?;
        return Err(Failure::new("server_error"));
    }
    let r = field(&root, "result")?;
    require(field(r, "type")?.as_str() == Some(kind))?;
    Ok(r.clone())
}

pub(super) fn snapshot(reply: &observation_socket::Reply, generic: bool) -> WireResult<Snapshot> {
    let result = result(&reply.payload, &reply.request_id, "session_snapshot")?;
    let s = field(&result, "snapshot")?;
    let version = string(field(s, "version")?)?;
    if uint(field(s, "protocol")?, u32::MAX.into())? != 18 {
        return Err(Failure::new("unsupported_protocol"));
    }
    let mut out = json!({"version":version,"protocol":18});
    let annotations = s.as_object().and_then(|o| o.get("annotations")).cloned();
    if !generic {
        for k in ["focused_workspace_id", "focused_tab_id", "focused_pane_id"] {
            if !matches!(optional(s, k), N::Null) {
                string(field(s, k)?)?;
            }
        }
        for (inventory, ids) in [
            ("workspaces", &["workspace_id"][..]),
            ("tabs", &["workspace_id", "tab_id"][..]),
            (
                "layouts",
                &["workspace_id", "tab_id", "focused_pane_id"][..],
            ),
        ] {
            for item in array(field(s, inventory)?)? {
                for k in ids {
                    string(field(item, k)?)?;
                }
                boolean(field(
                    item,
                    if inventory == "layouts" {
                        "zoomed"
                    } else {
                        "focused"
                    },
                )?)?;
                if inventory == "layouts" {
                    require(field(item, "area")?.as_object().is_some())?;
                    for k in ["panes", "splits"] {
                        let a = field(item, k)?
                            .as_array()
                            .ok_or_else(|| Failure::new("schema_error"))?;
                        require(a.iter().all(|v| v.as_object().is_some()))?;
                    }
                } else {
                    for k in ["number", "pane_count"] {
                        uint(field(item, k)?, u64::MAX)?;
                    }
                    string(field(item, "label")?)?;
                    status(field(item, "agent_status")?)?;
                    if inventory == "workspaces" {
                        uint(field(item, "tab_count")?, u64::MAX)?;
                        string(field(item, "active_tab_id")?)?;
                    }
                }
            }
        }
    }
    for inventory in if generic { &['p'][..] } else { &['p', 'a'][..] } {
        let name = if *inventory == 'p' { "panes" } else { "agents" };
        let (mut panes, mut terminals) = (HashSet::new(), HashSet::new());
        let mut items = Vec::new();
        for item in array(field(s, name)?)? {
            for k in LOCATORS {
                let s = string(field(item, k)?)?;
                if generic {
                    require(opaque(s))?;
                }
            }
            require(
                panes.insert(string(field(item, "pane_id")?)?)
                    && terminals.insert(string(field(item, "terminal_id")?)?),
            )?;
            let mut p = project(item, &LOCATORS)?;
            if !generic {
                boolean(field(item, "focused")?)?;
                status(field(item, "agent_status")?)?;
                uint(field(item, "revision")?, u64::MAX)?;
                for k in ["focused", "agent_status", "revision"] {
                    p[k] = json(field(item, k)?)?;
                }
                for k in ["agent", "name"] {
                    let v = optional(item, k);
                    if !matches!(v, N::Null) {
                        string(v)?;
                    }
                    p[k] = json(v)?;
                }
                let native = optional(item, "agent_session");
                p["agent_session"] = if matches!(native, N::Null) {
                    Value::Null
                } else {
                    for k in ["source", "agent", "kind", "value"] {
                        let s = string(field(native, k)?)?;
                        let cap =
                            if k == "value" && optional(native, "kind").as_str() == Some("path") {
                                8192
                            } else {
                                4096
                            };
                        require(s.chars().count() <= cap)?;
                    }
                    require(matches!(
                        field(native, "kind")?.as_str(),
                        Some("id" | "path")
                    ))?;
                    project(native, &["source", "agent", "kind", "value"])?
                };
                if name == "agents" {
                    for k in [
                        "launch_pending",
                        "interactive_ready",
                        "screen_detection_skipped",
                    ] {
                        let v = item
                            .as_object()
                            .and_then(|o| o.get(k))
                            .unwrap_or(&N::Bool(false));
                        boolean(v)?;
                        p[k] = json(v)?;
                    }
                    p["state_change_seq"] =
                        match item.as_object().and_then(|o| o.get("state_change_seq")) {
                            Some(v) => json!(uint(v, u64::MAX)?),
                            None => json!(0),
                        };
                }
            }
            items.push(p);
        }
        out[name] = Value::Array(items);
    }
    if !generic {
        for a in out["agents"].as_array().into_iter().flatten() {
            let p = out["panes"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|p| p["pane_id"] == a["pane_id"])
                .ok_or_else(|| Failure::new("schema_error"))?;
            for k in LOCATORS.into_iter().chain([
                "focused",
                "agent_status",
                "revision",
                "agent",
                "agent_session",
            ]) {
                require(p[k] == a[k])?;
            }
        }
    }
    Ok(Snapshot {
        projection: out,
        annotations,
    })
}

pub(super) fn command_snapshot(reply: &observation_socket::Reply) -> WireResult<Value> {
    let snapshot = snapshot(reply, true)?;
    let result = result(&reply.payload, &reply.request_id, "session_snapshot")?;
    let version = field(&result, "snapshot")
        .and_then(|s| field(s, "command_submission"))
        .and_then(|c| field(c, "version"))
        .and_then(|v| uint(v, u64::MAX));
    if version.ok() != Some(1) {
        return Err(Failure::new("command_submission_unsupported"));
    }
    Ok((*snapshot).clone())
}

pub(super) fn comparison_snapshot(reply: &observation_socket::Reply) -> WireResult<Value> {
    let snapshot = snapshot(reply, false)?;
    for inventory in ["panes", "agents"] {
        for row in snapshot[inventory].as_array().into_iter().flatten() {
            for key in LOCATORS {
                require(row[key].as_str().is_some_and(opaque))?;
            }
        }
    }
    Ok((*snapshot).clone())
}

pub(super) fn publication_annotations(a: &N) -> WireResult<Value> {
    let epoch = string(field(a, "epoch")?)?;
    require(!epoch.is_empty())?;
    let sources = field(a, "sources")?
        .as_array()
        .ok_or_else(|| Failure::new("schema_error"))?;
    require(sources.len() <= 8)?;
    let mut seen = HashSet::new();
    let mut projected = Vec::new();
    for source in sources {
        let source = string(source)?;
        require(
            !source.is_empty()
                && source.len() <= 64
                && source
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
                && seen.insert(source),
        )?;
        projected.push(source);
    }
    // Capability is a literal-true test, not a truthiness or integer conversion.
    Ok(json!({"epoch":epoch,"sources":projected,
        "valid_until_unix_ms":matches!(optional(a,"valid_until_unix_ms"),N::Bool(true))}))
}

pub(super) fn processes(reply: &observation_socket::Reply, pane: &str) -> WireResult<Vec<Value>> {
    let r = result(&reply.payload, &reply.request_id, "pane_process_info")?;
    let info = field(&r, "process_info")?;
    require(field(info, "pane_id")?.as_str() == Some(pane))?;
    for k in ["shell_pid", "foreground_process_group_id"] {
        if !matches!(optional(info, k), N::Null) {
            require(uint(field(info, k)?, u32::MAX.into())? > 0)?;
        }
    }
    let empty = N::Array(vec![]);
    let a = info
        .as_object()
        .and_then(|o| o.get("foreground_processes"))
        .unwrap_or(&empty);
    let (mut matches, mut pids) = (vec![], HashSet::new());
    for p in array(a)? {
        let pid = uint(field(p, "pid")?, u32::MAX.into())?;
        require(pid > 0 && pids.insert(pid))?;
        string(field(p, "name")?)?;
        if matches!(optional(p, "argv"), N::Null) {
            continue;
        }
        let argv = array(field(p, "argv")?)?;
        for arg in argv {
            require(!string(arg)?.contains('\0'))?;
        }
        let executable = argv
            .first()
            .and_then(N::as_str)
            .unwrap_or("")
            .rsplit('/')
            .next()
            .unwrap_or("");
        if matches!(executable, "djinn" | "djinn-ui") {
            matches.push(json!({"pid":pid,"executable_family":executable,"rule":"exact_posix_argv0_basename"}));
        }
    }
    Ok(matches)
}
