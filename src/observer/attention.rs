//! Bounded principal-scoped references, never a cache of question/answer bodies.
use super::conversations::DecisionTarget;
use super::*;
use crate::decision_native::{self as native, Action, Meta};
use crate::observation_socket::{Delivery, Failure};
use crate::shared_native::Connection;
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct State {
    principals: HashMap<String, Principal>,
    #[cfg(test)]
    pub(super) collected_list_pause: Option<Arc<(Mutex<(bool, bool)>, Condvar)>>,
}
#[derive(Default)]
struct Principal {
    revision: u64,
    refs: Vec<Reference>,
}
#[derive(Clone)]
struct Reference {
    id: String,
    alert: String,
    conversation: String,
    agent: String,
    binding: String,
    target: DecisionTarget,
    meta: Meta,
    can_respond: bool,
}

impl Observer {
    pub fn attention(&self, login: &str) -> Value {
        self.collect_metadata(login).1
    }

    pub(super) fn attention_revision(&self, login: &str) -> u64 {
        self.attention
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .principals
            .get(login)
            .map_or(0, |p| p.revision)
    }

    pub(super) fn collect_metadata(&self, login: &str) -> (Value, Value, metadata::Fence) {
        let missing = || json!({"contract":"coven.conversations.v1","conversations":[],"sources":[],"state":"unavailable"});
        let empty = || {
            json!({"contract":"coven.attention.v1","state":"unavailable","items":[],"streams":[],
            "omitted_count":null,"unavailable_count":1,"unlinked_count":0})
        };
        if !self.conversations.admitted(login) {
            return (missing(), empty(), metadata::Fence::read(self, login));
        }
        let revision = {
            let mut state = self.attention.lock().unwrap_or_else(|p| p.into_inner());
            if !state.principals.contains_key(login) && state.principals.len() >= 64 {
                // Do not reacquire the attention mutex while holding it.
                drop(state);
                return (missing(), empty(), metadata::Fence::read(self, login));
            }
            let p = state.principals.entry(login.into()).or_default();
            p.revision = p.revision.wrapping_add(1);
            p.revision
        };
        // Refresh leases from share_status only. Never share_begin/renew/bootstrap.
        let (list, fence) = self.conversation_list_with_fence(login, revision);
        #[cfg(test)]
        {
            let pause = self.attention.lock().unwrap().collected_list_pause.take();
            if let Some(pause) = pause {
                let mut flags = pause.0.lock().unwrap();
                flags.0 = true;
                pause.1.notify_all();
                let _ = pause
                    .1
                    .wait_timeout_while(flags, Duration::from_secs(3), |f| !f.1)
                    .unwrap();
            }
        }
        let mut unavailable = list["sources"].as_array().map_or(0, |a| {
            a.iter().filter(|s| s["status"] != "observed").count()
        }) + self.conversations.attention_unavailable(login);
        let mut omitted = list["omitted_conversations"].as_u64().unwrap_or(0);
        let mut unknown = unavailable > 0 || omitted > 0;
        let mut unlinked = 0;
        let mut refs = Vec::new();
        let mut items = Vec::new();
        let mut streams = Vec::new();
        let mut stream_targets = Vec::new();
        let deadline = crate::shared_native::deadline();
        for row in list["conversations"].as_array().into_iter().flatten() {
            let Some(conversation) = row["id"].as_str() else {
                unknown = true;
                continue;
            };
            let target = match self.conversations.decision_target(login, conversation) {
                Ok(t) => t,
                Err(_) => {
                    unavailable += 1;
                    unknown = true;
                    continue;
                }
            };
            let mut c = Connection::new(
                &target.selected.endpoint,
                &target.selected.runtime,
                deadline,
            );
            let summary = match native::summary(&mut c, &target.selected.grant) {
                Ok(s) => s,
                Err(_) => {
                    unavailable += 1;
                    unknown = true;
                    continue;
                }
            };
            unknown |= !summary.complete || summary.omitted_count.is_none();
            omitted = omitted.saturating_add(summary.omitted_count.unwrap_or(0));
            if !self.same_target(login, conversation, &target) {
                unavailable += 1;
                unknown = true;
                continue;
            }
            let alert_ids: Vec<_> = summary
                .requests
                .iter()
                .map(|meta| {
                    digest(&[
                        login,
                        &target.source_id,
                        &target.selected.runtime,
                        &target.selected.grant.session_id,
                        meta.kind.name(),
                        &meta.occurrence_id,
                    ])
                })
                .collect();
            if streams.len() < 64 {
                streams.push(json!({"id":digest(&[login,&target.source_id,&target.selected.runtime,&target.selected.grant.session_id]),
                    "complete":summary.complete && summary.omitted_count == Some(0),"alert_ids":alert_ids}));
                stream_targets.push((conversation.to_owned(), target.clone()));
            }
            let card = &row["card"];
            for (meta, alert) in summary.requests.into_iter().zip(alert_ids) {
                if row["association_status"] != "verified" {
                    unlinked += 1;
                    continue;
                }
                let (Some(agent), Some(binding), Some(endpoint)) = (
                    card["fleet_agent_id"].as_str(),
                    card["binding_id"].as_str(),
                    card["fleet_endpoint_id"].as_str(),
                ) else {
                    unlinked += 1;
                    continue;
                };
                if !self.bound(login, agent, conversation, binding)
                    || !self.same_target(login, conversation, &target)
                {
                    unavailable += 1;
                    unknown = true;
                    continue;
                }
                if refs.len() >= 32 {
                    omitted = omitted.saturating_add(1);
                    continue;
                }
                let id = digest(&[
                    &target.epoch,
                    login,
                    &target.source_id,
                    &format!("{:?}", target.selected.endpoint),
                    &target.selected.runtime,
                    &target.selected.grant.grant_id,
                    &target.selected.grant.session_id,
                    meta.kind.name(),
                    &meta.request_id,
                    &meta.occurrence_id,
                    meta.details_sha256.as_deref().unwrap_or(""),
                    conversation,
                    agent,
                    binding,
                ]);
                let can_respond = summary.can_respond && target.writable && meta.details_available;
                items.push(json!({"id":id,"alert_id":alert,"conversation_id":conversation,"fleet_agent_id":agent,
                    "fleet_endpoint_id":endpoint,"binding_id":binding,"team":target.team,"kind":meta.kind,
                    "can_respond":can_respond,"details_available":meta.details_available}));
                refs.push(Reference {
                    id,
                    alert,
                    conversation: conversation.into(),
                    agent: agent.into(),
                    binding: binding.into(),
                    target: target.clone(),
                    meta,
                    can_respond,
                });
            }
        }
        let streams: Vec<_> = streams
            .into_iter()
            .zip(stream_targets)
            .filter_map(|(stream, (conversation, target))| {
                if self.same_target(login, &conversation, &target) {
                    Some(stream)
                } else {
                    unavailable += 1;
                    unknown = true;
                    None
                }
            })
            .collect();
        let (items, refs): (Vec<_>, Vec<_>) = items
            .into_iter()
            .zip(refs)
            .filter(|(_, r)| {
                let current = self.same_target(login, &r.conversation, &r.target)
                    && self.bound(login, &r.agent, &r.conversation, &r.binding);
                if !current {
                    unavailable += 1;
                    unknown = true;
                }
                current
            })
            .unzip();
        let mut state = self.attention.lock().unwrap_or_else(|p| p.into_inner());
        let Some(p) = state.principals.get_mut(login) else {
            return (missing(), empty(), fence);
        };
        // A losing refresh must not revive an occurrence or return uncommitted IDs.
        if p.revision != revision {
            return (missing(), empty(), fence);
        }
        p.refs = refs;
        let status = if unavailable > 0 && items.is_empty() && streams.is_empty() {
            "unavailable"
        } else if unknown || omitted > 0 || unlinked > 0 {
            "partial"
        } else {
            "observed"
        };
        (
            list,
            json!({"contract":"coven.attention.v1","state":status,"items":items,"streams":streams,
            "omitted_count":if unknown { Value::Null } else { json!(omitted) },"unavailable_count":unavailable,"unlinked_count":unlinked}),
            fence,
        )
    }

    fn same_target(&self, login: &str, conversation: &str, target: &DecisionTarget) -> bool {
        target.selected.expires > Instant::now()
            && self
                .conversations
                .decision_target(login, conversation)
                .is_ok_and(|now| {
                    now.epoch == target.epoch
                        && now.source_id == target.source_id
                        && now.team == target.team
                        && now.workspace == target.workspace
                        && now.writable == target.writable
                        && now.selected.endpoint == target.selected.endpoint
                        && now.selected.runtime == target.selected.runtime
                        && now.selected.grant.grant_id == target.selected.grant.grant_id
                        && now.selected.grant.session_id == target.selected.grant.session_id
                })
    }

    fn decision_ref(&self, login: &str, id: &str) -> Result<Reference> {
        if !valid_id(id) {
            return Err(Error("invalid_reference"));
        }
        let r = self
            .attention
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .principals
            .get(login)
            .and_then(|p| p.refs.iter().find(|r| r.id == id))
            .cloned()
            .ok_or(Error("invalid_reference"))?;
        if !self.same_target(login, &r.conversation, &r.target)
            || !self.bound(login, &r.agent, &r.conversation, &r.binding)
        {
            return Err(Error("invalid_reference"));
        }
        Ok(r)
    }

    pub fn decision_response_allowed(&self, login: &str, id: &str) -> bool {
        self.decision_ref(login, id)
            .is_ok_and(|r| r.can_respond && r.target.writable && r.meta.details_available)
    }

    fn verify_decision_binding(&self, login: &str, r: &Reference) -> Result<()> {
        self.decision_ref(login, &r.id)?;
        self.verify_card_binding(login, &r.agent, &r.conversation, &r.binding)?;
        self.decision_ref(login, &r.id)?;
        Ok(())
    }

    pub fn decision_details(&self, login: &str, id: &str) -> Result<Value> {
        let r = self.decision_ref(login, id)?;
        if !r.meta.details_available {
            return Err(Error("decision_unavailable"));
        }
        self.verify_decision_binding(login, &r)?;
        let mut c = Connection::decision_workflow(
            &r.target.selected.endpoint,
            &r.target.selected.runtime,
            crate::shared_native::deadline(),
        );
        let summary = native::summary(&mut c, &r.target.selected.grant)
            .map_err(|_| Error("decision_unavailable"))?;
        if !summary.requests.contains(&r.meta) {
            return Err(Error("decision_stale"));
        }
        let detail = native::details(&mut c, &r.target.selected.grant, &r.meta)
            .map_err(|_| Error("decision_unavailable"))?;
        self.verify_decision_binding(login, &r)?;
        let current = native::summary(&mut c, &r.target.selected.grant)
            .map_err(|_| Error("decision_unavailable"))?;
        if !current.requests.contains(&r.meta) {
            return Err(Error("decision_stale"));
        }
        self.decision_ref(login, id)?;
        Ok(
            json!({"contract":"coven.decision.v1","id":id,"alert_id":r.alert,"kind":r.meta.kind,
            "can_respond":r.can_respond && r.target.writable && current.can_respond,"decision":detail.sanitized}),
        )
    }

    pub fn respond_decision(
        &self,
        login: &str,
        id: &str,
        attempt: &str,
        value: &Value,
    ) -> Result<Value> {
        if !crate::session_prompts::protocol::uuid(attempt) {
            return Err(Error("invalid_decision"));
        }
        let action = Action::parse(value).map_err(|_| Error("invalid_decision"))?;
        let report = |outcome: &str, delivery: &str, code: &str| {
            json!({"contract":"coven.decision-response.v1",
            "id":id,"attempt_id":attempt,"outcome":outcome,"delivery":delivery,"action":action.name(),"diagnostics":[code]})
        };
        if self.conversations.known_attempt(attempt) {
            return Ok(report("uncertain", "uncertain", "request_already_seen"));
        }
        let r = match self.decision_ref(login, id) {
            Ok(r) => r,
            Err(_) => return Ok(report("unavailable", "unsent", "decision_stale")),
        };
        if !r.can_respond || !r.target.writable || !r.meta.details_available {
            return Ok(report("unavailable", "unsent", "decision_unavailable"));
        }
        let _flight = match self.conversations.claim(
            attempt,
            &r.target.selected.runtime,
            &r.target.selected.grant.session_id,
        ) {
            Ok(f) => f,
            Err(e) if e.code() == "request_already_seen" => {
                return Ok(report("uncertain", "uncertain", e.code()));
            }
            Err(e) => return Ok(report("unavailable", "unsent", e.code())),
        };
        // The owned !Send guard lives on this blocking thread through the native CAS.
        // Runtime-launch authority is refused before even card verification connects.
        let Some(workspace) = r.target.workspace.as_deref() else {
            return Ok(report("unavailable", "unsent", "decision_unavailable"));
        };
        let workspace = std::path::Path::new(workspace);
        let gate = match crate::session_support::admit(workspace) {
            Ok(gate) => gate,
            Err(e) => {
                return Ok(report(
                    "unavailable",
                    "unsent",
                    safe_admission_code(e.code()),
                ));
            }
        };
        if let Err(e) = crate::session_support::validate_admission(workspace, &gate) {
            return Ok(report(
                "unavailable",
                "unsent",
                safe_admission_code(e.code()),
            ));
        }
        let result = (|| {
            self.verify_decision_binding(login, &r)
                .map_err(|_| Failure::new("decision_stale"))?;
            let mut c = Connection::decision_workflow(
                &r.target.selected.endpoint,
                &r.target.selected.runtime,
                crate::shared_native::deadline(),
            );
            let current = native::summary(&mut c, &r.target.selected.grant)?;
            if !current.can_respond {
                return Err(Failure::new("sharing_read_only"));
            }
            if !current.requests.contains(&r.meta) {
                return Err(Failure::new("decision_stale"));
            }
            let detail = native::details(&mut c, &r.target.selected.grant, &r.meta)?;
            action.validate(&detail)?;
            self.verify_decision_binding(login, &r)
                .map_err(|_| Failure::new("decision_stale"))?;
            native::decide(
                &mut c,
                &r.target.selected.grant,
                &r.meta,
                attempt,
                &action,
                || {
                    crate::session_support::validate_admission(workspace, &gate)
                        .map_err(|_| Failure::new("decision_unavailable"))?;
                    if !self.decision_response_allowed(login, id) {
                        return Err(Failure::new("decision_stale"));
                    }
                    Ok(())
                },
            )
        })();
        Ok(match result {
            Ok(()) => json!({"contract":"coven.decision-response.v1","id":id,"attempt_id":attempt,
                "outcome":"applied","delivery":"applied","action":action.name(),"diagnostics":[]}),
            Err(e) if e.delivery == Delivery::Uncertain => {
                report("uncertain", "uncertain", "decision_uncertain")
            }
            Err(e) if e.delivery == Delivery::Rejected => {
                report("rejected", "rejected", safe_decision_code(&e.code))
            }
            Err(e) => report("unavailable", "unsent", safe_decision_code(&e.code)),
        })
    }
}

fn safe_admission_code(code: &str) -> &str {
    match code {
        "legacy_runtime_disabled"
        | "action_orphan_bundles"
        | "runtime_transport_gate_busy"
        | "invalid_runtime_workspace"
        | "invalid_transport_gate_owner" => code,
        _ => "decision_unavailable",
    }
}

fn safe_decision_code(code: &str) -> &str {
    match code {
        "decision_unsupported"
        | "decision_unavailable"
        | "decision_stale"
        | "invalid_decision"
        | "sharing_read_only"
        | "grant_unavailable"
        | "native_runtime_mismatch"
        | "native_request_capacity" => code,
        _ => "decision_unavailable",
    }
}
