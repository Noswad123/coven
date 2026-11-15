//! Checked navigation to temporary user assertions, never proof of native hosting.
use crate::{Error, Result, error::require, task_mutation::TaskOutput};
use serde_json::{Value, json};

pub(crate) fn hex(s: &str, len: usize) -> Result<()> {
    require(
        s.len() == len
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "agent_invalid_schema",
    )
}
fn opaque(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control))
}
pub(crate) fn validate_target(t: &Value) -> Result<()> {
    require(
        t.as_object().is_some_and(|o| o.len() == 5)
            && opaque(&t["socket"])
            && opaque(&t["terminal_id"])
            && opaque(&t["runtime_generation"])
            && t["device"].is_u64()
            && t["inode"].is_u64(),
        "agent_invalid_schema",
    )?;
    crate::observation_socket::validate_path(t["socket"].as_str().unwrap())
}
pub(crate) fn validate_control_result(v: &Value) -> Result<()> {
    require(
        v.as_object().is_some_and(|o| o.len() == 9)
            && [
                "contract",
                "server_epoch",
                "observation_id",
                "association_revision",
                "provenance",
                "native_hosting",
                "valid",
                "applied",
                "target",
            ]
            .iter()
            .all(|k| v.get(*k).is_some())
            && v["contract"] == "coven.agent-navigation.v1"
            && v["provenance"] == "user_asserted"
            && v["native_hosting"] == "unestablished"
            && v["valid"].is_boolean()
            && v["applied"].is_boolean(),
        "agent_invalid_schema",
    )?;
    hex(
        v["server_epoch"]
            .as_str()
            .ok_or(Error("agent_invalid_schema"))?,
        32,
    )?;
    hex(
        v["observation_id"]
            .as_str()
            .ok_or(Error("agent_invalid_schema"))?,
        64,
    )?;
    if !v["association_revision"].is_null() {
        hex(
            v["association_revision"]
                .as_str()
                .ok_or(Error("agent_invalid_schema"))?,
            32,
        )?;
    }
    if v["target"].is_null() {
        require(v["valid"] == false, "agent_invalid_schema")
    } else {
        validate_target(&v["target"])
    }
}

pub struct Selection<'a> {
    pub socket: &'a str,
    pub observation: &'a str,
    pub expected_epoch: &'a str,
    pub expected_revision: Option<&'a str>,
}
impl Selection<'_> {
    fn params(&self) -> Result<Value> {
        hex(self.observation, 64)?;
        hex(self.expected_epoch, 32)?;
        if let Some(rev) = self.expected_revision {
            hex(rev, 32)?;
        }
        Ok(
            json!({"observation_id":self.observation,"expected_epoch":self.expected_epoch,"expected_revision":self.expected_revision}),
        )
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::observation_socket::{self as wire, Endpoint, NoCancel, ReadBudget};
    use std::time::{Duration, Instant};

    fn call(s: &Selection<'_>, method: &str, p: Value) -> Result<Value> {
        let result = crate::agent_management::unix::call_contract(
            s.socket,
            crate::agent_management::native::CONTROL_RPC,
            method,
            p.clone(),
        )?;
        require(
            result["server_epoch"] == s.expected_epoch
                && result["observation_id"] == s.observation
                && result["applied"] == p.get("apply").cloned().unwrap_or(json!(false))
                && (method != "association.assert" || result["target"] == p["target"])
                && (p["apply"] == true || result["association_revision"] == p["expected_revision"]),
            "agent_delivery_uncertain",
        )?;
        Ok(result)
    }
    fn fence(e: &Endpoint) -> Result<()> {
        crate::platform::observation_socket::fence(e).map_err(|_| Error("agent_endpoint_replaced"))
    }
    // Closed internal caller: no public arbitrary RPC or mutable validated plan.
    fn kit(e: &Endpoint, method: &str, params: Value) -> Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(1);
        fence(e)?;
        let id = crate::platform::random_uuid_hex()?;
        let mut bytes = serde_json::to_vec(&json!({"id":id,"method":method,"params":params}))?;
        bytes.push(b'\n');
        let socket = crate::platform::observation_socket::Socket::connect(e, deadline, &NoCancel)
            .map_err(|_| Error("agent_connect_failed"))?;
        fence(e)?;
        socket
            .send(&bytes, deadline, &NoCancel)
            .map_err(|_| Error("agent_delivery_uncertain"))?;
        let reply = socket
            .receive(deadline, &mut ReadBudget::new(256 * 1024), &NoCancel)
            .map_err(|_| Error("agent_delivery_uncertain"))?;
        fence(e).map_err(|_| Error("agent_delivery_uncertain"))?;
        let v: Value = (|| -> Result<Value> {
            crate::strict_json::check_depth(&reply, 32)?;
            let native = crate::values::parse(&reply, crate::values::Profile::Authority)?;
            Ok(serde_json::from_slice(&native.compact()?)?)
        })()
        .map_err(|_| Error("agent_delivery_uncertain"))?;
        require(
            v.as_object().is_some_and(|o| o.len() == 2) && v["id"] == id,
            "agent_delivery_uncertain",
        )?;
        if let Some(error) = v.get("error") {
            require(
                error.as_object().is_some_and(|o| o.len() == 2)
                    && error["code"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty() && s.len() <= 128)
                    && error["message"].as_str().is_some_and(|s| s.len() <= 4096),
                "agent_delivery_uncertain",
            )?;
            // Peer codes/messages are private data, not public diagnostics.
            return Err(Error("agent_navigation_rejected"));
        }
        v.get("result")
            .cloned()
            .ok_or(Error("agent_delivery_uncertain"))
    }
    pub(super) fn inspect(e: &Endpoint, terminal: &str) -> Result<Value> {
        let snapshot = kit(e, "session.snapshot", json!({}))?;
        require(
            snapshot["snapshot"]["terminal_navigation"]["version"] == 1,
            "agent_navigation_unsupported",
        )?;
        let v = kit(e, "terminal.inspect", json!({"terminal_id":terminal}))?;
        require(
            v.as_object().is_some_and(|o| o.len() == 7)
                && v["type"] == "terminal_inspected"
                && v["terminal_id"] == terminal
                && v["availability"] == "live"
                && opaque(&v["runtime_generation"])
                && ["workspace_id", "tab_id", "pane_id"]
                    .iter()
                    .all(|k| opaque(&v[*k])),
            "agent_target_not_live",
        )?;
        Ok(v)
    }
    pub(super) fn associate(
        s: &Selection<'_>,
        socket: &str,
        terminal: &str,
        apply: bool,
    ) -> Result<Value> {
        let mut p = s.params()?;
        require(opaque(&json!(terminal)), "agent_invalid_schema")?;
        let e = wire::select(socket).map_err(|_| Error("agent_unsafe_socket"))?;
        let inspected = inspect(&e, terminal)?;
        p["target"] = json!({"socket":e.resolved,"device":e.device,"inode":e.inode,
            "terminal_id":terminal,"runtime_generation":inspected["runtime_generation"]});
        p["provenance"] = json!("user_asserted");
        p["apply"] = json!(apply);
        fence(&e)?;
        call(s, "association.assert", p)
    }
    pub(super) fn dissociate(s: &Selection<'_>, apply: bool) -> Result<Value> {
        let mut p = s.params()?;
        require(s.expected_revision.is_some(), "agent_revision_required")?;
        p["provenance"] = json!("user_asserted");
        p["apply"] = json!(apply);
        call(s, "association.remove", p)
    }
    pub(super) fn focus(s: &Selection<'_>, execute: bool) -> Result<Value> {
        require(s.expected_revision.is_some(), "agent_revision_required")?;
        let p = s.params()?;
        let prepared = call(s, "focus.prepare", p.clone())?;
        require(
            prepared["association_revision"] == p["expected_revision"] && prepared["valid"] == true,
            "agent_stale_revision",
        )?;
        if !execute {
            return Ok(prepared);
        }
        let t = &prepared["target"];
        let e = wire::select(t["socket"].as_str().unwrap())
            .map_err(|_| Error("agent_unsafe_socket"))?;
        require(
            t["device"] == e.device && t["inode"] == e.inode,
            "agent_endpoint_replaced",
        )?;
        let terminal = t["terminal_id"].as_str().unwrap();
        let inspected = inspect(&e, terminal)?;
        require(
            inspected["runtime_generation"] == t["runtime_generation"],
            "agent_runtime_changed",
        )?;
        // Last receiver CAS/freshness check immediately before the one checked focus frame.
        require(
            call(s, "focus.prepare", p)? == prepared,
            "agent_stale_revision",
        )?;
        let focused = kit(
            &e,
            "terminal.focus",
            json!({"terminal_id":terminal,
            "expected_runtime_generation":t["runtime_generation"]}),
        )?;
        require(
            focused.as_object().is_some_and(|o| o.len() == 6)
                && focused["type"] == "terminal_focused"
                && focused["terminal_id"] == terminal
                && focused["runtime_generation"] == t["runtime_generation"]
                && ["workspace_id", "tab_id", "pane_id"]
                    .iter()
                    .all(|k| opaque(&focused[*k])),
            "agent_delivery_uncertain",
        )?;
        Ok(
            json!({"outcome":"focus_accepted","provenance":"user_asserted","native_hosting":"unestablished",
            "terminal_id":terminal,"runtime_generation":focused["runtime_generation"],
            "workspace_id":focused["workspace_id"],"tab_id":focused["tab_id"],"pane_id":focused["pane_id"]}),
        )
    }
}

pub fn associate_arguments(
    s: &Selection<'_>,
    kitsune_socket: &str,
    terminal: &str,
    apply: bool,
    json: bool,
) -> Result<TaskOutput> {
    #[cfg(unix)]
    {
        crate::session_support::output(&unix::associate(s, kitsune_socket, terminal, apply)?, !json)
    }
    #[cfg(not(unix))]
    {
        let _ = (s, kitsune_socket, terminal, apply, json);
        Err(Error("agent_unsupported_platform"))
    }
}
pub fn dissociate_arguments(s: &Selection<'_>, apply: bool, json: bool) -> Result<TaskOutput> {
    #[cfg(unix)]
    {
        crate::session_support::output(&unix::dissociate(s, apply)?, !json)
    }
    #[cfg(not(unix))]
    {
        let _ = (s, apply, json);
        Err(Error("agent_unsupported_platform"))
    }
}
pub fn focus_arguments(s: &Selection<'_>, execute: bool, json: bool) -> Result<TaskOutput> {
    #[cfg(unix)]
    {
        crate::session_support::output(&unix::focus(s, execute)?, !json)
    }
    #[cfg(not(unix))]
    {
        let _ = (s, execute, json);
        Err(Error("agent_unsupported_platform"))
    }
}

#[cfg(all(test, unix))]
#[path = "agent_navigation/tests.rs"]
mod tests;
