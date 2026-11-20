//! Pinned automatic-sharing transport. No retry, cancellation or process signal handler.
use crate::observation_socket::{Delivery, Failure, NoCancel, ReadBudget, WireResult, check};
use crate::session_prompts::{
    endpoint::Endpoint,
    fresh_id,
    protocol::{identifier, uuid},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub(crate) const CONTRACT: &str = "djinn.web-sharing.v1";
pub(crate) const ASSOCIATION_CONTRACT: &str = "djinn.card-association.v1";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CardTarget {
    pub socket: String,
    pub device: u64,
    pub inode: u64,
    pub terminal_id: String,
    pub runtime_generation: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Association {
    pub instance_id: String,
    pub session_id: String,
    pub receiver_epoch: String,
    pub scope: String,
    pub lease_ms: u64,
    pub target: CardTarget,
}

fn association_result(value: Value, grant: &Grant) -> WireResult<Option<Association>> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Reply {
        r#type: String,
        contract: String,
        grant_id: String,
        session_id: String,
        association: Option<Association>,
    }
    if value.get("association").is_none() {
        return Err(invalid());
    }
    let reply: Reply = serde_json::from_value(value).map_err(|_| invalid())?;
    if reply.r#type != "share_association"
        || reply.contract != ASSOCIATION_CONTRACT
        || reply.grant_id != grant.grant_id
        || reply.session_id != grant.session_id
    {
        return Err(invalid());
    }
    if let Some(a) = &reply.association {
        let t = &a.target;
        if !uuid(&a.instance_id)
            || a.receiver_epoch.len() != 32
            || !a
                .receiver_epoch
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || a.session_id != grant.session_id
            || !identifier(&a.session_id, "ses_")
            || a.scope != "local_full_tui_root"
            || !(1..=5000).contains(&a.lease_ms)
            || !uuid(&t.runtime_generation)
            || t.terminal_id.is_empty()
            || t.terminal_id.len() > 512
            || t.terminal_id.chars().any(char::is_control)
            || crate::observation_socket::validate_path(&t.socket).is_err()
            || t.socket.len() > 4096
            || t.socket.chars().any(char::is_control)
            || t.socket.split('/').any(|p| matches!(p, "." | ".."))
            || t.socket.contains("//")
            || t.device > 9_007_199_254_740_991
            || t.inode > 9_007_199_254_740_991
        {
            return Err(invalid());
        }
    }
    Ok(reply.association)
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Grant {
    pub grant_id: String,
    pub session_id: String,
    pub can_submit: bool,
    pub lease_ms: u64,
}

pub(crate) struct Connection<'a> {
    endpoint: &'a Endpoint,
    runtime: &'a str,
    deadline: Instant,
    budget: ReadBudget,
    card_association: bool,
    decision_control: bool,
}

fn invalid() -> Failure {
    Failure::new("invalid_native_frame")
}

impl<'a> Connection<'a> {
    pub(crate) fn new(endpoint: &'a Endpoint, runtime: &'a str, deadline: Instant) -> Self {
        Self {
            endpoint,
            runtime,
            deadline,
            budget: ReadBudget::new(65536),
            card_association: false,
            decision_control: false,
        }
    }

    pub(crate) fn decision_workflow(
        endpoint: &'a Endpoint,
        runtime: &'a str,
        deadline: Instant,
    ) -> Self {
        Self {
            budget: ReadBudget::new(256 * 1024),
            ..Self::new(endpoint, runtime, deadline)
        }
    }

    pub(crate) fn call(&mut self, method: &str, params: Value, id: &str) -> WireResult<Value> {
        self.call_checked(method, params, id, || Ok(()))
    }

    pub(crate) fn call_checked(
        &mut self,
        method: &str,
        params: Value,
        id: &str,
        before_send: impl FnOnce() -> WireResult<()>,
    ) -> WireResult<Value> {
        let mut possible_send = false;
        let result = (|| {
            check(self.deadline, &NoCancel)?;
            self.endpoint.fence()?;
            let mut frame =
                serde_json::to_vec(&json!({"version":1,"id":id,"method":method,"params":params}))
                    .map_err(|_| invalid())?;
            // Worst case: 65536 UTF-8 bytes encoded as JSON control escapes.
            if frame.len() > 400 * 1024 {
                return Err(Failure::new("request_limit"));
            }
            if matches!(
                method,
                "shared_attention" | "shared_decision" | "shared_decide"
            ) && frame.len() + 1 > 65536
            {
                return Err(Failure::new("request_limit"));
            }
            frame.push(b'\n');
            let socket = crate::platform::observation_socket::Socket::connect(
                &self.endpoint.socket,
                self.deadline,
                &NoCancel,
            )?;
            self.endpoint.fence()?;
            check(self.deadline, &NoCancel)?;
            before_send()?;
            possible_send = true;
            socket.send(&frame, self.deadline, &NoCancel)?;
            // A workflow may span several large replies, but no single frame grows.
            let limit = self.budget.remaining().min(65536);
            let mut frame_budget = ReadBudget::new(limit);
            let received = socket.receive(self.deadline, &mut frame_budget, &NoCancel);
            self.budget.charge(limit - frame_budget.remaining())?;
            let payload = received?;
            self.endpoint.fence()?;
            check(self.deadline, &NoCancel)?;
            let root = crate::session_conversations::decode(&payload).map_err(|_| invalid())?;
            if root.as_object().is_none_or(|o| o.len() != 4)
                || root["version"].as_u64() != Some(1)
                || root["id"] != id
                || root["runtime_id"] != self.runtime
                || !uuid(self.runtime)
                || root.get("result").is_some() == root.get("error").is_some()
            {
                return Err(invalid());
            }
            if let Some(error) = root.get("error") {
                if error.as_object().is_none_or(|o| o.len() != 2) {
                    return Err(invalid());
                }
                let (code, delivery) = match error["code"].as_str() {
                    Some("runtime_mismatch") => ("native_runtime_mismatch", Delivery::Rejected),
                    Some("request_capacity") => ("native_request_capacity", Delivery::Rejected),
                    Some("session_unavailable") => {
                        ("native_session_unavailable", Delivery::Rejected)
                    }
                    Some("request_unavailable") => {
                        ("native_request_unavailable", Delivery::Rejected)
                    }
                    Some("grant_unavailable") => ("grant_unavailable", Delivery::Rejected),
                    Some("sharing_read_only") => ("sharing_read_only", Delivery::Rejected),
                    Some("decision_unsupported") => ("decision_unsupported", Delivery::Rejected),
                    Some("decision_unavailable") => ("decision_unavailable", Delivery::Rejected),
                    Some("decision_stale") => ("decision_stale", Delivery::Rejected),
                    Some("invalid_decision") => ("invalid_decision", Delivery::Rejected),
                    Some("decision_uncertain") => ("decision_uncertain", Delivery::Uncertain),
                    Some("conversation_unsupported") => {
                        ("conversation_unsupported", Delivery::Rejected)
                    }
                    Some("request_already_seen") => {
                        ("native_request_already_seen", Delivery::Uncertain)
                    }
                    Some("admission_uncertain") => {
                        ("native_admission_uncertain", Delivery::Uncertain)
                    }
                    _ => return Err(invalid()),
                };
                let expected = if delivery == Delivery::Rejected {
                    "rejected"
                } else {
                    "uncertain"
                };
                if error["delivery"] != expected {
                    return Err(invalid());
                }
                return Ok(Err(Failure {
                    code: code.into(),
                    delivery,
                    interrupted: false,
                }));
            }
            Ok(Ok(root["result"].clone()))
        })();
        match result {
            Ok(reply) => reply,
            Err(mut e) => {
                if possible_send && matches!(method, "shared_submit" | "shared_decide") {
                    e.delivery = Delivery::Uncertain;
                }
                Err(e)
            }
        }
    }

    pub(crate) fn capabilities(&mut self) -> WireResult<()> {
        self.card_association = false;
        self.decision_control = false;
        let value = self.call("capabilities", json!({}), &fresh_id()?)?;
        if value["type"] != "capabilities"
            || value["runtime_id"] != self.runtime
            || value["contract"] != "djinn.interactive-prompt.v1"
            || value["version"].as_u64() != Some(1)
            || value["max_prompt_bytes"].as_u64() != Some(65536)
            || value["web_sharing"] != json!({"contract":CONTRACT,"version":1})
        {
            return Err(Failure::new("sharing_unsupported"));
        }
        self.card_association = value.get("card_association")
            == Some(&json!({"contract":ASSOCIATION_CONTRACT,"version":1}));
        self.decision_control =
            value.get("decision_control") == Some(&crate::decision_native::capability());
        Ok(())
    }

    pub(crate) fn require_decisions(&mut self, grant: &Grant) -> WireResult<()> {
        self.capabilities()?;
        if !self.decision_control {
            return Err(Failure::new("decision_unsupported"));
        }
        // can_submit is model-dependent follow-up authority, NOT decision authority.
        self.require_grant(grant, false)
    }

    pub(crate) fn association(&mut self, grant: &Grant) -> WireResult<Option<Association>> {
        self.capabilities()?;
        if !self.card_association {
            return Ok(None);
        }
        self.require_grant(grant, false)?;
        let value = self.call("share_association", self.params(grant), &fresh_id()?)?;
        let association = association_result(value, grant)?;
        self.require_grant(grant, false)?;
        Ok(association)
    }

    pub(crate) fn status(&mut self) -> WireResult<Option<Grant>> {
        let value = self.call(
            "share_status",
            json!({"expected_runtime_id":self.runtime}),
            &fresh_id()?,
        )?;
        if value.as_object().is_none_or(|o| o.len() != 3)
            || value["type"] != "share_status"
            || value["contract"] != CONTRACT
            || value.get("grant").is_none()
        {
            return Err(invalid());
        }
        if value["grant"].is_null() {
            return Ok(None);
        }
        let grant: Grant = serde_json::from_value(value["grant"].clone()).map_err(|_| invalid())?;
        if !uuid(&grant.grant_id)
            || !identifier(&grant.session_id, "ses_")
            || !(1..=5000).contains(&grant.lease_ms)
        {
            return Err(invalid());
        }
        Ok(Some(grant))
    }

    pub(crate) fn require_grant(&mut self, grant: &Grant, write: bool) -> WireResult<()> {
        let current = self
            .status()?
            .ok_or_else(|| Failure::new("grant_unavailable"))?;
        if current.grant_id != grant.grant_id || current.session_id != grant.session_id {
            return Err(Failure::new("grant_unavailable"));
        }
        if write && !current.can_submit {
            return Err(Failure::new("sharing_read_only"));
        }
        Ok(())
    }

    pub(crate) fn conversation(&mut self, grant: &Grant) -> WireResult<Value> {
        self.capabilities()?;
        self.require_grant(grant, false)?;
        let value = self.call("shared_conversation", self.params(grant), &fresh_id()?)?;
        if value.as_object().is_none_or(|o| o.len() != 4)
            || value["type"] != "shared_conversation"
            || value["contract"] != CONTRACT
            || value["grant_id"] != grant.grant_id
        {
            return Err(invalid());
        }
        crate::session_conversations::projection(value["conversation"].clone(), &grant.session_id)
            .map_err(|_| invalid())
    }

    pub(crate) fn params(&self, grant: &Grant) -> Value {
        json!({"expected_runtime_id":self.runtime,"session_id":grant.session_id,"grant_id":grant.grant_id})
    }
}

pub(crate) fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(4)
}

pub(crate) fn receipt(value: &Value, grant: &Grant, digest: &str, bytes: usize) -> WireResult<()> {
    if value.as_object().is_none_or(|o| o.len() != 7)
        || value["type"] != "shared_prompt_accepted"
        || value["contract"] != CONTRACT
        || value["grant_id"] != grant.grant_id
        || value["session_id"] != grant.session_id
        || value["prompt_sha256"] != digest
        || value["prompt_bytes"].as_u64() != Some(bytes as u64)
        || !value["message_id"]
            .as_str()
            .is_some_and(|s| identifier(s, "msg_"))
    {
        return Err(Failure {
            code: "invalid_native_receipt".into(),
            delivery: Delivery::Uncertain,
            interrupted: false,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_association_is_closed_bounded_and_correlated() {
        let id = "11111111-2222-4333-8444-555555555555";
        let grant = Grant {
            grant_id: id.into(),
            session_id: "ses_private".into(),
            can_submit: false,
            lease_ms: 5000,
        };
        let valid = json!({"type":"share_association","contract":ASSOCIATION_CONTRACT,
            "grant_id":id,"session_id":"ses_private","association":{
                "instance_id":id,"session_id":"ses_private","receiver_epoch":"a".repeat(32),
                "scope":"local_full_tui_root","lease_ms":5000,
                "target":{"socket":"/private/kit.sock","device":0,"inode":2,
                    "terminal_id":"terminal","runtime_generation":id}}});
        assert!(association_result(valid.clone(), &grant).unwrap().is_some());
        for (pointer, wrong) in [
            ("/type", json!("other")),
            ("/contract", json!(CONTRACT)),
            ("/grant_id", json!("21111111-2222-4333-8444-555555555555")),
            ("/session_id", json!("ses_other")),
            ("/association/session_id", json!("ses_other")),
            ("/association/instance_id", json!("not-uuid")),
            ("/association/receiver_epoch", json!("not-uuid")),
            ("/association/receiver_epoch", json!(id)),
            ("/association/receiver_epoch", json!("a".repeat(31))),
            ("/association/receiver_epoch", json!("A".repeat(32))),
            ("/association/scope", json!("anything")),
            ("/association/lease_ms", json!(0)),
            ("/association/lease_ms", json!(5001)),
            ("/association/lease_ms", json!(1.0)),
            ("/association/target/socket", json!("/a/../b")),
            ("/association/target/socket", json!("/a/./b")),
            ("/association/target/device", json!(-1)),
            ("/association/target/inode", json!(-1)),
            (
                "/association/target/device",
                json!(9_007_199_254_740_992u64),
            ),
            ("/association/target/inode", json!(9_007_199_254_740_992u64)),
            ("/association/target/terminal_id", json!("")),
            ("/association/target/terminal_id", json!("a".repeat(513))),
            ("/association/target/runtime_generation", json!("not-uuid")),
        ] {
            let mut bad = valid.clone();
            *bad.pointer_mut(pointer).unwrap() = wrong;
            assert!(association_result(bad, &grant).is_err(), "{pointer}");
        }
        for pointer in ["", "/association", "/association/target"] {
            let object = valid.pointer(pointer).unwrap().as_object().unwrap();
            for key in object.keys() {
                let mut bad = valid.clone();
                bad.pointer_mut(pointer)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                assert!(association_result(bad, &grant).is_err(), "{pointer}/{key}");
            }
            let mut bad = valid.clone();
            bad.pointer_mut(pointer).unwrap()["extension"] = json!(true);
            assert!(association_result(bad, &grant).is_err());
        }
        let mut null = valid;
        null["association"] = Value::Null;
        assert!(association_result(null, &grant).unwrap().is_none());
    }

    #[test]
    fn accepted_receipt_correlates_every_field_with_closed_schema() {
        let grant = Grant {
            grant_id: "11111111-2222-4333-8444-555555555555".into(),
            session_id: "ses_fixture".into(),
            can_submit: true,
            lease_ms: 5000,
        };
        let valid = json!({"type":"shared_prompt_accepted","contract":CONTRACT,"grant_id":grant.grant_id,
            "session_id":grant.session_id,"message_id":"msg_fixture","prompt_sha256":"digest","prompt_bytes":12});
        receipt(&valid, &grant, "digest", 12).unwrap();
        for (field, wrong) in [
            ("type", json!("prompt_accepted")),
            ("contract", json!("djinn.interactive-prompt.v1")),
            ("grant_id", json!("21111111-2222-4333-8444-555555555555")),
            ("session_id", json!("ses_other")),
            ("message_id", json!("invalid")),
            ("prompt_sha256", json!("other")),
            ("prompt_bytes", json!(13)),
            ("prompt_bytes", json!(12.0)),
            ("prompt_bytes", json!(true)),
            ("extension", json!("private peer text")),
        ] {
            let mut value = valid.clone();
            value[field] = wrong;
            assert_eq!(
                receipt(&value, &grant, "digest", 12).unwrap_err().delivery,
                Delivery::Uncertain,
                "{field}"
            );
        }
        for field in valid.as_object().unwrap().keys() {
            let mut value = valid.clone();
            value.as_object_mut().unwrap().remove(field);
            assert_eq!(
                receipt(&value, &grant, "digest", 12).unwrap_err().delivery,
                Delivery::Uncertain
            );
        }
    }
}
