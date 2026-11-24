//! Best-effort local inventory hints. Replay and silence confer no authority.
//! The upstream 512-entry history can silently overflow; there is no wire sequence.
//! A subscription acknowledgement negotiates capability, not inventory completeness.
use crate::observation_socket::{Cancel, Endpoint, EventSocket, Failure, WireResult};
use crate::values::{Object, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Hint {
    Inventory,
}

pub(super) trait LocalEventSource: Send + Sync {
    fn open(
        &self,
        path: &str,
        deadline: Instant,
        cancel: &dyn Cancel,
    ) -> WireResult<Box<dyn LocalEventStream>>;
}

pub(super) trait LocalEventStream: Send {
    fn endpoint(&self) -> &Endpoint;
    fn next(&mut self, until: Instant, cancel: &dyn Cancel) -> WireResult<Option<Hint>>;
}

pub(super) struct NativeEvents;

impl LocalEventSource for NativeEvents {
    fn open(
        &self,
        path: &str,
        deadline: Instant,
        cancel: &dyn Cancel,
    ) -> WireResult<Box<dyn LocalEventStream>> {
        let deadline = deadline.min(Instant::now() + Duration::from_secs(5));
        let (mut socket, id) = EventSocket::subscribe(path, deadline, cancel)?;
        let payload = socket
            .next_frame(deadline, cancel)?
            .ok_or_else(|| Failure::new("timeout"))?;
        let root = decode(&payload)?;
        let root = object(&root)?;
        if text(root, "id")? != id || root.get("result").is_some() == root.get("error").is_some() {
            return Err(schema());
        }
        if let Some(error) = root.get("error") {
            let error = object(error)?;
            text(error, "code")?;
            text(error, "message")?;
            return Err(Failure::new("subscription_rejected"));
        }
        if text(child(root, "result")?, "type")? != "subscription_started" {
            return Err(schema());
        }
        socket.fence()?;
        crate::observation_socket::check(deadline, cancel)?;
        Ok(Box::new(NativeStream {
            socket,
            panes: HashMap::new(),
        }))
    }
}

struct NativeStream {
    socket: EventSocket,
    panes: HashMap<String, [u8; 32]>,
}

impl LocalEventStream for NativeStream {
    fn endpoint(&self) -> &Endpoint {
        self.socket.endpoint()
    }

    fn next(&mut self, until: Instant, cancel: &dyn Cancel) -> WireResult<Option<Hint>> {
        const MAX_TURN_BYTES: usize = 512 * 1024;
        // Cooperative wall limit: a single bounded decode is not interruptible.
        let until = until.min(Instant::now() + Duration::from_millis(20));
        let mut bytes = 0;
        for _ in 0..32 {
            // Reserve a full frame (including LF) before consuming it; leave any
            // buffered frame untouched when the remaining turn budget is smaller.
            if bytes + EventSocket::MAX_FRAME_BYTES > MAX_TURN_BYTES {
                break;
            }
            let Some(frame) = self.socket.next_frame(until, cancel)? else {
                return Ok(None);
            };
            bytes += frame.len() + 1;
            let value = decode(&frame)?;
            let hint = project(&value, &mut self.panes)?;
            if cancel.interrupted() {
                return Err(Failure::new("interrupted"));
            }
            self.socket.fence()?;
            if hint {
                return Ok(Some(Hint::Inventory));
            }
        }
        Ok(None)
    }
}

fn schema() -> Failure {
    Failure::new("schema_error")
}
fn object(value: &Value) -> WireResult<&Object> {
    value.as_object().ok_or_else(schema)
}
fn child<'a>(value: &'a Object, key: &str) -> WireResult<&'a Object> {
    object(value.get(key).ok_or_else(schema)?)
}
fn text<'a>(value: &'a Object, key: &str) -> WireResult<&'a str> {
    value.get(key).and_then(Value::as_str).ok_or_else(schema)
}
fn identity<'a>(value: &'a Object, key: &str) -> WireResult<&'a str> {
    let id = text(value, key)?;
    if id.is_empty() || id.len() > 256 || id.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(schema());
    }
    Ok(id)
}

fn decode(frame: &[u8]) -> WireResult<Value> {
    let value =
        crate::observation_wire::decode_native(frame).map_err(|e| Failure::new(e.code()))?;
    // The event channel has no enlarged path or terminal-output string exception.
    fn strings(value: &Value) -> WireResult<()> {
        match value {
            Value::String(s) if s.chars().count() > 4096 => return Err(schema()),
            Value::Object(o) => {
                for (_, v) in o.entries() {
                    strings(v)?;
                }
            }
            Value::Array(a) => {
                for v in a {
                    strings(v)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    strings(&value)?;
    Ok(value)
}

fn pane_fingerprint(pane: &Object) -> WireResult<(String, [u8; 32])> {
    let mut hash = Sha256::new();
    for key in ["pane_id", "terminal_id", "workspace_id", "tab_id"] {
        let value = identity(pane, key)?;
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    // Match the inventory's display projection, not every input/volatile pane field.
    let optional = |key| -> WireResult<Option<String>> {
        match pane.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => {
                let clean: String = s.chars().filter(|c| !c.is_control()).collect();
                let mut clean = clean.trim().to_owned();
                let mut end = clean.len().min(512);
                while !clean.is_char_boundary(end) {
                    end -= 1;
                }
                clean.truncate(end);
                Ok((!clean.is_empty()).then_some(clean))
            }
            _ => Err(schema()),
        }
    };
    let title = |key| -> Option<String> {
        match pane.get(key) {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if s.len() <= super::model::TITLE_LIMIT => {
                let clean: String = s.chars().filter(|c| !c.is_control()).collect();
                let clean = clean.trim();
                (!clean.is_empty()).then(|| clean.to_owned())
            }
            Some(Value::String(_)) => Some("Title unavailable (too long)".into()),
            _ => Some("Title unavailable (invalid)".into()),
        }
    };
    let provider = optional("agent")?;
    let project = optional("cwd")?;
    let title = title("name")
        .or_else(|| title("terminal_title"))
        .or_else(|| title("title"))
        .or_else(|| provider.clone())
        .unwrap_or_else(|| "Agent".into());
    let name = pane
        .get("label")
        .and_then(Value::as_str)
        .filter(|s| crate::agent_management_presentation::safe_display_text(s, 160));
    for value in [
        title.as_str(),
        name.unwrap_or(""),
        provider.as_deref().unwrap_or(""),
        project.as_deref().unwrap_or(""),
    ] {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    Ok((identity(pane, "pane_id")?.into(), hash.finalize().into()))
}

fn project(value: &Value, panes: &mut HashMap<String, [u8; 32]>) -> WireResult<bool> {
    let root = object(value)?;
    let kind = text(root, "event")?;
    let data = child(root, "data")?;
    if text(data, "type")? != kind {
        return Err(schema());
    }
    match kind {
        "pane_created" | "pane_updated" | "pane_moved" => {
            let pane = child(data, "pane")?;
            let (id, fingerprint) = pane_fingerprint(pane)?;
            if kind == "pane_moved" {
                for key in [
                    "previous_pane_id",
                    "previous_workspace_id",
                    "previous_tab_id",
                ] {
                    identity(data, key)?;
                }
                panes.remove(text(data, "previous_pane_id")?);
            }
            let same = panes.get(&id) == Some(&fingerprint);
            if !panes.contains_key(&id) && panes.len() >= 1024 {
                return Err(Failure::new("event_dedup_limit"));
            }
            panes.insert(id, fingerprint);
            Ok(kind != "pane_updated" || !same)
        }
        "pane_closed" | "pane_exited" => {
            let id = identity(data, "pane_id")?;
            identity(data, "workspace_id")?;
            panes.remove(id);
            Ok(true)
        }
        "tab_created" => {
            let tab = child(data, "tab")?;
            identity(tab, "tab_id")?;
            identity(tab, "workspace_id")?;
            Ok(true)
        }
        "tab_closed" | "tab_moved" => {
            identity(data, "tab_id")?;
            identity(data, "workspace_id")?;
            Ok(true)
        }
        "workspace_created" => {
            identity(child(data, "workspace")?, "workspace_id")?;
            Ok(true)
        }
        "workspace_closed" | "workspace_moved" => {
            identity(data, "workspace_id")?;
            Ok(true)
        }
        "layout_updated" => {
            let layout = child(data, "layout")?;
            identity(layout, "workspace_id")?;
            identity(layout, "tab_id")?;
            identity(layout, "focused_pane_id")?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

#[cfg(all(test, unix))]
#[path = "local_events/tests.rs"]
mod tests;
