//! Closed native capture boundaries and volatile, caller-unverified evidence.
//!
//! `coven.provider-events.v1` envelopes select exactly OpenCode's
//! `opencode-root.v1` (producer-qualified root lifecycle/status/decisions), or
//! Pi's `pi-context.v1` (UI/RPC context metadata and busy/idle only). Selecting
//! a profile is a producer claim, never receiver authentication. Normalized
//! report RPC params omit `contract` and retain `provider` and `profile`.
//! Replay/observation/tombstone bounds are shared across all profiles.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

pub const RPC: &str = "coven.native-events.v1";
pub const PROVIDER_RPC: &str = "coven.provider-events.v1";
pub const CONTROL_RPC: &str = "coven.observation-control.v1";
pub const ATTACHMENT_RPC: &str = "coven.client-attachments.v1";
pub const ATTENTION_VIEW: &str = "coven.attention.v1";
pub const VIEW: &str = "coven.normal-observations.v1";
const ENVELOPE: &str = "djinn.native-events.v1";
const EVIDENCE_TTL: Duration = Duration::from_secs(30);
const RETENTION_TTL: Duration = Duration::from_secs(600);
const MAX_RETAINED: usize = 512;
const ATTACHMENT_TTL: Duration = Duration::from_secs(5);

pub(crate) fn uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}

pub fn attachment_params(value: &Value, envelope: bool) -> Result<Value> {
    let object = value.as_object().ok_or(Error("agent_invalid_schema"))?;
    if envelope {
        require(
            value["contract"] == "djinn.client-attachment.v1",
            "agent_contract_mismatch",
        )?;
    }
    require(
        hex(text(value, "expected_epoch")?, 32) && uuid(text(value, "client_id")?),
        "agent_invalid_schema",
    )?;
    require(
        value["revision"]
            .as_u64()
            .is_some_and(|n| (1..=9_007_199_254_740_991).contains(&n)),
        "agent_invalid_schema",
    )?;
    let attach = match text(value, "operation")? {
        "attach" => true,
        "detach" => false,
        _ => return Err(Error("agent_invalid_schema")),
    };
    let mut p = json!({"expected_epoch":value["expected_epoch"],"client_id":value["client_id"],"revision":value["revision"],"operation":value["operation"]});
    if attach {
        require(
            uuid(text(value, "instance_id")?) && value["scope"] == "local_full_tui_root",
            "agent_invalid_schema",
        )?;
        identifier(text(value, "session_id")?)?;
        crate::agent_navigation::validate_target(&value["target"])?;
        require(
            uuid(text(&value["target"], "runtime_generation")?),
            "agent_invalid_schema",
        )?;
        for k in ["instance_id", "session_id", "scope", "target"] {
            p[k] = value[k].clone();
        }
    }
    require(
        object.len() == 4 + usize::from(envelope) + if attach { 4 } else { 0 }
            && object
                .keys()
                .all(|k| k == "contract" && envelope || p.get(k).is_some()),
        "agent_invalid_schema",
    )?;
    Ok(p)
}

struct ClientLease {
    revision: u64,
    seen: Instant,
    selection: Option<Selection>,
}
struct Selection {
    key: (String, String),
    target: Value,
    incarnation: u64,
}

fn identifier(value: &str) -> Result<&str> {
    require(
        !value.trim().is_empty() && value.len() <= 512 && !value.chars().any(char::is_control),
        "agent_invalid_session",
    )?;
    Ok(value)
}

/// Validate a capture envelope and reconstruct only its normalized RPC fields.
pub fn params_for_envelope(value: &Value) -> Result<Value> {
    match text(value, "contract")? {
        ENVELOPE => params(value, true),
        PROVIDER_RPC => provider_params(value, true),
        _ => Err(Error("agent_contract_mismatch")),
    }
}

pub(super) fn provider_params(value: &Value, envelope: bool) -> Result<Value> {
    let provider = text(value, "provider")?;
    let profile = text(value, "profile")?;
    require(
        matches!(
            (provider, profile),
            ("opencode", "opencode-root.v1") | ("pi", "pi-context.v1")
        ),
        "agent_unsupported_profile",
    )?;
    if envelope {
        require(
            text(value, "contract")? == PROVIDER_RPC,
            "agent_contract_mismatch",
        )?;
    }
    let event = text(value, "event")?;
    require(
        if provider == "pi" {
            matches!(event, "session.updated" | "session.status")
                && (event != "session.status" || matches!(text(value, "status")?, "busy" | "idle"))
        } else {
            matches!(
                event,
                "session.created"
                    | "session.updated"
                    | "session.deleted"
                    | "session.status"
                    | "permission.asked"
                    | "permission.replied"
                    | "question.asked"
                    | "question.replied"
                    | "question.rejected"
            )
        },
        "agent_invalid_event",
    )?;
    let mut base = value.clone();
    let object = base.as_object_mut().ok_or(Error("agent_invalid_schema"))?;
    object.remove("provider");
    object.remove("profile");
    if envelope {
        object.remove("contract");
    }
    let mut result = params(&base, false)?;
    result["provider"] = json!(provider);
    result["profile"] = json!(profile);
    Ok(result)
}

pub(super) fn params(value: &Value, envelope: bool) -> Result<Value> {
    let object = value.as_object().ok_or(Error("agent_invalid_schema"))?;
    if envelope {
        require(
            text(value, "contract")? == ENVELOPE,
            "agent_contract_mismatch",
        )?;
    }
    let instance = text(value, "instance_id")?;
    require(
        instance.len() == 36
            && instance.bytes().enumerate().all(|(i, b)| {
                if matches!(i, 8 | 13 | 18 | 23) {
                    b == b'-'
                } else {
                    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
                }
            }),
        "agent_invalid_instance",
    )?;
    let sid = identifier(text(value, "session_id")?)?;
    let sequence = value
        .get("sequence")
        .and_then(Value::as_u64)
        .ok_or(Error("agent_invalid_sequence"))?;
    require(
        (1..=9_007_199_254_740_991).contains(&sequence),
        "agent_invalid_sequence",
    )?;
    let event = text(value, "event")?;
    let mut result =
        json!({"instance_id":instance,"session_id":sid,"sequence":sequence,"event":event});
    let extra = match event {
        "session.created" | "session.updated" | "session.deleted" => 0,
        "session.status" => {
            let status = text(value, "status")?;
            require(
                matches!(status, "busy" | "retry" | "idle"),
                "agent_invalid_event",
            )?;
            result["status"] = json!(status);
            1
        }
        "permission.asked"
        | "permission.replied"
        | "permission.cancelled"
        | "question.asked"
        | "question.replied"
        | "question.rejected"
        | "question.cancelled" => {
            result["request_id"] = json!(identifier(text(value, "request_id")?)?);
            1
        }
        _ => return Err(Error("agent_invalid_event")),
    };
    require(
        object.len() == 4 + usize::from(envelope) + extra,
        "agent_invalid_schema",
    )?;
    Ok(result)
}

struct Observation {
    id: String,
    provider: String,
    profile: Option<String>,
    status: Option<(&'static str, Instant)>,
    pending: BTreeMap<(String, String), Pending>,
    ended: Option<Instant>,
    first_seen: Instant,
    attachment_only: bool,
}

struct Pending {
    at: Instant,
    occurrence_seq: u64,
    acknowledged: bool,
}

fn fresh(at: Instant, now: Instant) -> bool {
    now.saturating_duration_since(at) < EVIDENCE_TTL
}

impl Observation {
    fn attention(&self, epoch: &str, now: Instant) -> Value {
        let mut hash = Sha256::new();
        hash.update(b"coven.attention.v1\0");
        hash.update(epoch.as_bytes());
        hash.update(self.id.as_bytes());
        let mut count = 0;
        let mut unacknowledged = 0;
        for pending in self.pending.values().filter(|p| fresh(p.at, now)) {
            count += 1;
            unacknowledged += usize::from(!pending.acknowledged);
            hash.update(pending.occurrence_seq.to_be_bytes());
            hash.update([u8::from(pending.acknowledged)]);
        }
        json!({"fresh_pending_count":count,"unacknowledged_pending_count":unacknowledged,"attention_revision":format!("{:x}", hash.finalize())})
    }

    fn fact(&self, now: Instant) -> Option<(&'static str, Instant)> {
        if let Some(at) = self.ended {
            return Some(("ended", at));
        }
        if let Some(at) = self
            .pending
            .values()
            .map(|p| p.at)
            .filter(|at| fresh(*at, now))
            .min()
        {
            return Some(("blocked", at));
        }
        if let Some((state, at)) = self.status {
            if fresh(at, now) {
                return Some((state, at));
            }
        }
        self.status
            .map(|(_, at)| at)
            .into_iter()
            .chain(self.pending.values().map(|p| p.at))
            .max()
            .map(|at| ("unknown", at))
    }

    fn active(&self, now: Instant) -> bool {
        self.ended.is_none() && self.fact(now).is_some_and(|(_, at)| fresh(at, now))
    }
}

#[derive(Default)]
pub(super) struct NativeStore {
    pub(super) facts: super::facts::FactsStore,
    clients: BTreeMap<String, ClientLease>,
    attachment_incarnation: u64,
    navigation_generation: u64,
    sequences: BTreeMap<String, (u64, Instant)>,
    tombstones: BTreeMap<(String, String), Instant>,
    observations: BTreeMap<(String, String), Observation>,
    retired: u64,
    retired_djinn: u64,
    occurrence: u64,
}

impl NativeStore {
    fn revoke_selections(&mut self, key: &(String, String)) {
        for c in self.clients.values_mut() {
            if c.selection.as_ref().is_some_and(|s| &s.key == key) {
                c.selection = None;
            }
        }
    }

    fn remove_observation(&mut self, key: &(String, String)) {
        self.revoke_selections(key);
        if let Some(observation) = self.observations.remove(key) {
            self.retired = self.retired.saturating_add(1);
            if observation.provider == "djinn" {
                self.retired_djinn = self.retired_djinn.saturating_add(1);
            }
        }
    }

    fn reclaim_inactive(&mut self, now: Instant) -> bool {
        let retired = self
            .observations
            .iter()
            .filter(|(_, o)| !o.active(now))
            // Conflicting live targets are still leased, even without a focusable association.
            .filter(|(key, _)| {
                !self.clients.values().any(|c| {
                    now.saturating_duration_since(c.seen) < ATTACHMENT_TTL
                        && c.selection.as_ref().is_some_and(|s| &s.key == *key)
                })
            })
            .min_by_key(|(_, o)| o.first_seen)
            .map(|(key, _)| key.clone());
        let Some(retired) = retired else { return false };
        self.remove_observation(&retired);
        true
    }

    fn rotate_selection(&mut self, key: &(String, String)) {
        self.attachment_incarnation = self.attachment_incarnation.saturating_add(1);
        for c in self.clients.values_mut() {
            if let Some(s) = c.selection.as_mut().filter(|s| &s.key == key) {
                s.incarnation = self.attachment_incarnation;
            }
        }
    }
    pub(super) fn producer_association(&self, id: &str, now: Instant) -> Option<(String, Value)> {
        let (key, _) = self
            .observations
            .iter()
            .find(|(_, o)| o.id == id && o.ended.is_none())?;
        let mut selected = self
            .clients
            .iter()
            .filter(|(_, c)| now.saturating_duration_since(c.seen) < ATTACHMENT_TTL)
            .filter_map(|(client, c)| {
                c.selection
                    .as_ref()
                    .filter(|s| &s.key == key)
                    .map(|s| (client, s))
            });
        let (client, first) = selected.next()?;
        let mut hash = Sha256::new();
        hash.update(b"coven.producer-association.v1\0");
        hash.update(client.as_bytes());
        hash.update(first.incarnation.to_be_bytes());
        for (client, s) in selected {
            if s.target != first.target {
                return None;
            }
            hash.update(client.as_bytes());
            hash.update(s.incarnation.to_be_bytes());
        }
        Some((
            format!("{:x}", hash.finalize())[..32].to_owned(),
            first.target.clone(),
        ))
    }

    pub(super) fn report_attachment(
        &mut self,
        value: &Value,
        epoch: &str,
        now: Instant,
    ) -> Result<Value> {
        let p = attachment_params(value, false)?;
        require(p["expected_epoch"] == epoch, "agent_stale_epoch")?;
        self.retire(now);
        let client = text(&p, "client_id")?.to_owned();
        let revision = p["revision"].as_u64().unwrap();
        require(
            self.clients
                .get(&client)
                .is_none_or(|c| revision > c.revision),
            "agent_stale_revision",
        )?;
        require(
            self.clients.contains_key(&client) || self.clients.len() < MAX_RETAINED,
            "agent_capacity",
        )?;
        // Consume every admitted higher revision before qualification. A rejected new
        // selection must not leave an older selection focusable or replayable.
        let old = self.clients.remove(&client).and_then(|c| c.selection);
        let unchanged = old.as_ref().is_some_and(|s| {
            p["operation"] == "attach"
                && p["instance_id"] == s.key.0
                && p["session_id"] == s.key.1
                && p["target"] == s.target
        });
        if let Some(s) = old.as_ref().filter(|_| !unchanged) {
            self.rotate_selection(&s.key);
        }
        self.clients.insert(
            client.clone(),
            ClientLease {
                revision,
                seen: now,
                selection: None,
            },
        );
        if p["operation"] == "attach" {
            require(
                self.clients
                    .values()
                    .filter(|c| c.selection.is_some())
                    .count()
                    < 128,
                "agent_capacity",
            )?;
            let key = (
                text(&p, "instance_id")?.to_owned(),
                text(&p, "session_id")?.to_owned(),
            );
            require(
                !self.tombstones.contains_key(&key)
                    && self
                        .observations
                        .get(&key)
                        .is_none_or(|o| o.ended.is_none()),
                "agent_session_ended",
            )?;
            if !self.observations.contains_key(&key) {
                require(
                    self.observations.len() < MAX_RECORDS || self.reclaim_inactive(now),
                    "agent_capacity",
                )?;
                self.observations.insert(
                    key.clone(),
                    Observation {
                        id: format!(
                            "{:x}",
                            Sha256::digest(
                                [
                                    b"native\0".as_slice(),
                                    key.0.as_bytes(),
                                    b"\0",
                                    key.1.as_bytes()
                                ]
                                .concat()
                            )
                        ),
                        provider: "djinn".into(),
                        profile: None,
                        status: None,
                        pending: BTreeMap::new(),
                        ended: None,
                        first_seen: now,
                        attachment_only: true,
                    },
                );
            }
            let incarnation =
                if let Some(old) = old.filter(|s| s.key == key && s.target == p["target"]) {
                    old.incarnation
                } else {
                    self.attachment_incarnation = self
                        .attachment_incarnation
                        .checked_add(1)
                        .ok_or(Error("agent_capacity"))?;
                    self.attachment_incarnation
                };
            self.clients.get_mut(&client).unwrap().selection = Some(Selection {
                key,
                target: p["target"].clone(),
                incarnation,
            });
            if !unchanged {
                let key = self.clients[&client]
                    .selection
                    .as_ref()
                    .unwrap()
                    .key
                    .clone();
                self.rotate_selection(&key);
            }
        }
        Ok(json!({"type":"accepted","delivery":"volatile"}))
    }
    pub(super) fn navigation_fact(&self, id: &str, now: Instant) -> Option<u64> {
        self.observations
            .values()
            .find(|o| o.id == id && o.active(now))
            .map(|_| self.navigation_generation)
    }
    // Membership is independent of focusability: conflicting selections still host a row.
    pub(super) fn current_deadline(&self, id: &str, now: Instant) -> Option<Instant> {
        let (key, o) = self.observations.iter().find(|(_, o)| o.id == id)?;
        if o.ended.is_some() {
            return None;
        }
        let selection = self
            .clients
            .values()
            .filter_map(|c| {
                (now.saturating_duration_since(c.seen) < ATTACHMENT_TTL
                    && c.selection.as_ref().is_some_and(|s| &s.key == key))
                .then_some(c.seen + ATTACHMENT_TTL)
            })
            .max();
        // Any still-fresh activity can sustain membership, not just the oldest
        // pending fact currently chosen for the public blocked-state display.
        let activity = o
            .status
            .into_iter()
            .map(|(_, at)| at)
            .chain(o.pending.values().map(|p| p.at))
            .filter(|at| fresh(*at, now))
            .map(|at| at + EVIDENCE_TTL)
            .max();
        selection.into_iter().chain(activity).max()
    }
    pub(super) fn naming_deadline(
        &self,
        id: &str,
        now: Instant,
        producer: bool,
    ) -> Option<Instant> {
        let (key, observation) = self.observations.iter().find(|(_, o)| o.id == id)?;
        if producer {
            self.producer_association(id, now)?;
            self.clients
                .values()
                .filter(|c| {
                    now.saturating_duration_since(c.seen) < ATTACHMENT_TTL
                        && c.selection.as_ref().is_some_and(|s| &s.key == key)
                })
                .map(|c| c.seen + ATTACHMENT_TTL)
                .min()
        } else {
            self.navigation_fact(id, now)?;
            observation.fact(now).map(|(_, at)| at + EVIDENCE_TTL)
        }
    }
    pub(super) fn control(
        &mut self,
        method: &str,
        params: &Value,
        legacy: Value,
        now: Instant,
    ) -> Result<Value> {
        let object = params.as_object().ok_or(Error("agent_invalid_schema"))?;
        let epoch = text(&legacy, "server_epoch")?;
        if matches!(method, "observations" | "attention.list") {
            require(object.is_empty(), "agent_invalid_schema")?;
            if method == "observations" {
                return Ok(self.projection(legacy, now));
            }
            let mut observations: Vec<_> = self
                .observations
                .values()
                .filter_map(|o| {
                    let mut attention = o.attention(epoch, now);
                    if attention["fresh_pending_count"] == 0 {
                        return None;
                    }
                    attention["observation_id"] = json!(o.id);
                    Some(attention)
                })
                .collect();
            observations.sort_by_key(|o| {
                (
                    o["unacknowledged_pending_count"].as_u64().unwrap_or(0) == 0,
                    o["observation_id"].as_str().unwrap_or_default().to_owned(),
                )
            });
            return Ok(
                json!({"contract":ATTENTION_VIEW,"server_epoch":epoch,"authorization":"none","observations":observations}),
            );
        }
        require(method == "attention.acknowledge", "agent_unknown_method")?;
        require(object.len() == 4, "agent_invalid_schema")?;
        let id = text(params, "observation_id")?;
        require(
            hex(id, 64)
                && hex(text(params, "expected_epoch")?, 32)
                && hex(text(params, "expected_revision")?, 64),
            "agent_invalid_schema",
        )?;
        let apply = params["apply"]
            .as_bool()
            .ok_or(Error("agent_invalid_schema"))?;
        require(
            text(params, "expected_epoch")? == epoch,
            "agent_stale_epoch",
        )?;
        let observation = self
            .observations
            .values_mut()
            .find(|o| o.id == id)
            .ok_or(Error("agent_unknown_observation"))?;
        let current = observation.attention(epoch, now);
        require(
            current["fresh_pending_count"].as_u64().unwrap_or(0) > 0,
            "agent_no_fresh_attention",
        )?;
        require(
            current["attention_revision"] == params["expected_revision"],
            "agent_stale_revision",
        )?;
        if apply {
            for pending in observation
                .pending
                .values_mut()
                .filter(|p| fresh(p.at, now))
            {
                pending.acknowledged = true;
            }
        }
        let mut attention = observation.attention(epoch, now);
        attention["observation_id"] = json!(observation.id);
        Ok(
            json!({"contract":ATTENTION_VIEW,"server_epoch":epoch,"authorization":"none","observations":[attention],"applied":apply}),
        )
    }

    fn invalidate(&mut self, instance: &str) {
        for client in self.clients.values_mut() {
            if client
                .selection
                .as_ref()
                .is_some_and(|s| s.key.0 == instance)
            {
                client.selection = None;
            }
        }
        self.navigation_generation = self.navigation_generation.saturating_add(1);
        for ((owner, _), observation) in &mut self.observations {
            if owner == instance {
                observation.status = None;
                observation.pending.clear();
            }
        }
    }

    fn forget_tombstone(&mut self, key: &(String, String)) {
        self.tombstones.remove(key);
        if self
            .observations
            .get(key)
            .is_some_and(|o| o.ended.is_some())
        {
            self.remove_observation(key);
        }
    }

    fn retire(&mut self, now: Instant) {
        let expired_keys: Vec<_> = self
            .clients
            .values()
            .filter(|c| now.saturating_duration_since(c.seen) >= ATTACHMENT_TTL)
            .filter_map(|c| c.selection.as_ref().map(|s| s.key.clone()))
            .collect();
        self.clients
            .retain(|_, c| now.saturating_duration_since(c.seen) < RETENTION_TTL);
        for c in self.clients.values_mut() {
            if now.saturating_duration_since(c.seen) >= ATTACHMENT_TTL {
                c.selection = None;
            }
        }
        for key in expired_keys {
            self.rotate_selection(&key);
        }
        let expired: Vec<_> = self
            .tombstones
            .iter()
            .filter(|(_, at)| now.saturating_duration_since(**at) >= RETENTION_TTL)
            .map(|(key, _)| key.clone())
            .collect();
        for key in expired {
            self.forget_tombstone(&key);
        }
        let expired: Vec<_> = self
            .sequences
            .iter()
            .filter(|(_, (_, at))| now.saturating_duration_since(*at) >= RETENTION_TTL)
            .map(|(instance, _)| instance.clone())
            .collect();
        for instance in expired {
            self.sequences.remove(&instance);
            self.invalidate(&instance);
        }
    }

    pub fn report(&mut self, value: &Value, now: Instant) -> Result<Value> {
        let p = params(value, false)?;
        self.report_validated(p, now)
    }

    pub fn report_provider(&mut self, value: &Value, now: Instant) -> Result<Value> {
        let p = provider_params(value, false)?;
        self.report_validated(p, now)
    }

    fn report_validated(&mut self, p: Value, now: Instant) -> Result<Value> {
        self.retire(now);
        // UUIDs cannot contain NUL, so these namespace components cannot alias.
        let owner = if let Some(provider) = p["provider"].as_str() {
            format!(
                "{provider}\0{}\0{}",
                text(&p, "profile")?,
                text(&p, "instance_id")?
            )
        } else {
            text(&p, "instance_id")?.to_owned()
        };
        let instance = owner.as_str();
        let key = (instance.to_owned(), text(&p, "session_id")?.to_owned());
        let sequence = p["sequence"].as_u64().unwrap();
        require(
            self.sequences
                .get(instance)
                .is_none_or(|(old, _)| sequence > *old),
            "agent_stale_sequence",
        )?;
        if self
            .sequences
            .get(instance)
            .is_some_and(|(old, _)| sequence != old + 1)
        {
            // Producer drops are permitted. No retained derived fact survives a gap.
            self.invalidate(instance);
        }
        if !self.sequences.contains_key(instance) && self.sequences.len() == MAX_RETAINED {
            let oldest = self
                .sequences
                .iter()
                .min_by_key(|(_, (_, at))| *at)
                .unwrap()
                .0
                .clone();
            self.sequences.remove(&oldest);
            self.invalidate(&oldest);
        }
        // Valid higher sequences are consumed even when capacity/tombstones refuse
        // the event. Neither rejection renews an evidence or tombstone clock.
        self.sequences.insert(instance.to_owned(), (sequence, now));
        let event = text(&p, "event")?;
        if event == "session.deleted" {
            // End is irreversible for every current lease, even after bounded tombstones leave history.
            self.revoke_selections(&key);
        }
        if self.tombstones.contains_key(&key) {
            require(event == "session.deleted", "agent_session_ended")?;
            return Ok(json!({"type":"accepted","delivery":"volatile"}));
        }
        if event == "session.deleted" {
            if self.tombstones.len() == MAX_RETAINED {
                let oldest = self
                    .tombstones
                    .iter()
                    .min_by_key(|(_, at)| *at)
                    .unwrap()
                    .0
                    .clone();
                self.forget_tombstone(&oldest);
            }
            self.tombstones.insert(key.clone(), now);
        }
        if !self.observations.contains_key(&key)
            && self.observations.len() == MAX_RECORDS
            && !self.reclaim_inactive(now)
        {
            if event != "session.deleted" {
                return Err(Error("agent_capacity"));
            }
            // The terminal safety fact is retained even if all display slots
            // contain live activity. There is no extra hidden active slot.
            self.retired = self.retired.saturating_add(1);
            if p["provider"].as_str().unwrap_or("djinn") == "djinn" {
                self.retired_djinn = self.retired_djinn.saturating_add(1);
            }
            return Ok(json!({"type":"accepted","delivery":"volatile"}));
        }
        let kind = event.split('.').next().unwrap();
        let request = p
            .get("request_id")
            .and_then(Value::as_str)
            .map(|id| (kind.to_owned(), id.to_owned()));
        let observation = self
            .observations
            .entry(key.clone())
            .or_insert_with(|| Observation {
                id: format!(
                    "{:x}",
                    Sha256::digest(
                        [
                            if p.get("provider").is_some() {
                                b"provider-events\0".as_slice()
                            } else {
                                b"native\0".as_slice()
                            },
                            key.0.as_bytes(),
                            b"\0",
                            key.1.as_bytes()
                        ]
                        .concat()
                    )
                ),
                provider: p["provider"].as_str().unwrap_or("djinn").to_owned(),
                profile: p["profile"].as_str().map(str::to_owned),
                status: None,
                pending: BTreeMap::new(),
                ended: None,
                first_seen: now,
                attachment_only: false,
            });
        observation.attachment_only = false;
        match event {
            "session.deleted" => {
                observation.ended = Some(now);
                observation.status = None;
                observation.pending.clear();
            }
            "session.created" | "session.updated" => {}
            "session.status" => {
                let state = if p["status"] == "idle" {
                    "idle"
                } else {
                    "working"
                };
                observation.status = Some((state, now));
            }
            _ if event.ends_with(".asked") => {
                observation
                    .pending
                    .retain(|_, pending| fresh(pending.at, now));
                let request = request.unwrap();
                require(
                    observation.pending.len() < 128 || observation.pending.contains_key(&request),
                    "agent_capacity",
                )?;
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    observation.pending.entry(request)
                {
                    self.occurrence = self
                        .occurrence
                        .checked_add(1)
                        .ok_or(Error("agent_capacity"))?;
                    entry.insert(Pending {
                        at: now,
                        occurrence_seq: self.occurrence,
                        acknowledged: false,
                    });
                }
            }
            _ => {
                if let Some(request) = request {
                    observation.pending.remove(&request);
                }
            }
        }
        Ok(json!({"type":"accepted","delivery":"volatile"}))
    }

    pub fn projection(&self, legacy: Value, now: Instant) -> Value {
        self.project(legacy, now, None)
    }

    pub(super) fn publisher_projection(
        &self,
        legacy: Value,
        now: Instant,
        manual: &BTreeMap<String, Instant>,
    ) -> Value {
        let deadlines: BTreeMap<_, _> = self
            .observations
            .values()
            .filter(|o| o.provider == "djinn" && o.ended.is_none())
            .filter_map(|o| {
                self.current_deadline(&o.id, now)
                    .into_iter()
                    .chain(manual.get(&o.id).copied())
                    .max()
                    .map(|end| (o.id.clone(), end))
            })
            .collect();
        let mut view = self.project(legacy, now, Some(&deadlines));
        view["_publisher_current"] = json!(
            view["observations"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|row| {
                    let id = row["observation_id"].as_str()?;
                    let end = deadlines.get(id)?;
                    Some(json!({"id":format!("observation:{id}"),
                    "until":crate::agent_management_presentation::naming_tick(*end)}))
                })
                .collect::<Vec<_>>()
        );
        view
    }

    fn project(
        &self,
        legacy: Value,
        now: Instant,
        current: Option<&BTreeMap<String, Instant>>,
    ) -> Value {
        let mut observations = Vec::new();
        for record in legacy["records"].as_array().into_iter().flatten() {
            if current.is_some() && record["provider"] == "djinn" {
                continue;
            }
            let age = record["age_ms"].as_u64();
            let capability = if record["provider"] == "claude" {
                "activity"
            } else {
                "reference_only"
            };
            observations.push(row(
                record["record_id"].clone(),
                record["provider"].clone(),
                "v1_receipt",
                capability,
                record["reported_state"].as_str().unwrap_or("unknown"),
                age,
            ));
        }
        for observation in self.observations.values() {
            if observation.provider == "djinn"
                && current.is_some_and(|ids| !ids.contains_key(&observation.id))
            {
                continue;
            }
            let fact = observation.fact(now);
            let age = fact.map(|(_, at)| {
                now.saturating_duration_since(at)
                    .as_millis()
                    .min(u64::MAX as u128) as u64
            });
            let mut native = row(
                json!(observation.id),
                json!(observation.provider),
                if observation.profile.is_some() {
                    "provider_native_event"
                } else {
                    if observation.attachment_only {
                        "djinn_client_attachment"
                    } else {
                        "djinn_native_event"
                    }
                },
                "activity",
                fact.map_or("unknown", |(state, _)| state),
                age,
            );
            if age.is_none() {
                native["freshness"] = json!("unknown");
            }
            if let Some(profile) = &observation.profile {
                native["profile"] = json!(profile);
                native["root_eligibility"] = json!(if profile == "opencode-root.v1" {
                    "producer_checked"
                } else {
                    "ui_rpc_context"
                });
            }
            let attention =
                observation.attention(legacy["server_epoch"].as_str().unwrap_or_default(), now);
            for (key, value) in attention.as_object().unwrap() {
                native[key] = value.clone();
            }
            let association = self.producer_association(&observation.id, now);
            native["hosting_state"] = json!(if association.is_some() {
                "local_full_tui_root"
            } else {
                "none"
            });
            native["association_revision"] =
                association.map(|(r, _)| json!(r)).unwrap_or(Value::Null);
            observations.push(native);
        }
        observations.extend(self.facts.rows(now));
        observations.sort_by_key(|o| {
            (
                if o["unacknowledged_pending_count"].as_u64().unwrap_or(0) > 0 {
                    0
                } else if o["fresh_pending_count"].as_u64().unwrap_or(0) > 0 {
                    1
                } else {
                    2
                },
                o["freshness"] != "fresh",
                o["origin"] == "v1_receipt",
                o["observation_id"].as_str().unwrap_or_default().to_owned(),
            )
        });
        let retired = if current.is_some() {
            self.retired.saturating_sub(self.retired_djinn)
        } else {
            self.retired
        };
        let limit = if observations.len() > MAX_RECORDS || retired > 0 {
            MAX_RECORDS - 1
        } else {
            MAX_RECORDS
        };
        let omitted = observations.len().saturating_sub(limit);
        observations.truncate(limit);
        json!({"contract":VIEW,"server_epoch":legacy["server_epoch"],"authorization":"none","observations":observations,
            "omitted_observations":omitted,"retired_native_observations":retired})
    }
}

fn hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Validate private control replies before they can reach a public wrapper.
pub(super) fn validate_control_result(method: &str, value: &Value) -> Result<()> {
    require(
        value["authorization"] == "none" && hex(text(value, "server_epoch")?, 32),
        "agent_invalid_schema",
    )?;
    let rows = value["observations"]
        .as_array()
        .ok_or(Error("agent_invalid_schema"))?;
    require(rows.len() <= MAX_RECORDS, "agent_invalid_schema")?;
    let normal = method == "observations";
    require(
        value.as_object().is_some_and(|o| {
            o.len()
                == if normal {
                    6
                } else if method == "attention.acknowledge" {
                    5
                } else {
                    4
                }
        }),
        "agent_invalid_schema",
    )?;
    require(
        value["contract"] == if normal { VIEW } else { ATTENTION_VIEW },
        "agent_contract_mismatch",
    )?;
    if normal {
        require(
            value["omitted_observations"].as_u64().is_some()
                && value["retired_native_observations"].as_u64().is_some(),
            "agent_invalid_schema",
        )?;
    } else if method == "attention.acknowledge" {
        require(
            value["applied"].is_boolean() && rows.len() == 1,
            "agent_invalid_schema",
        )?;
    }
    let mut ids = std::collections::BTreeSet::new();
    for row in rows {
        let id = text(row, "observation_id")?;
        require(hex(id, 64) && ids.insert(id), "agent_invalid_schema")?;
        if normal {
            validate_observation(row)?;
        } else {
            require(
                row.as_object().is_some_and(|o| o.len() == 4),
                "agent_invalid_schema",
            )?;
            validate_attention(row)?;
            require(
                row["fresh_pending_count"].as_u64().unwrap_or(0) > 0,
                "agent_invalid_schema",
            )?;
        }
    }
    Ok(())
}

fn validate_attention(row: &Value) -> Result<()> {
    let count = row["fresh_pending_count"]
        .as_u64()
        .ok_or(Error("agent_invalid_schema"))?;
    let unacked = row["unacknowledged_pending_count"]
        .as_u64()
        .ok_or(Error("agent_invalid_schema"))?;
    require(
        count <= 128 && unacked <= count && hex(text(row, "attention_revision")?, 64),
        "agent_invalid_schema",
    )
}

fn valid_provenance(provider: &str, origin: &str, profile: Option<&str>) -> bool {
    matches!(
        (provider, origin, profile),
        (
            "djinn",
            "djinn_native_event" | "djinn_client_attachment",
            None
        ) | (
            "opencode",
            "provider_native_event",
            Some("opencode-root.v1")
        ) | ("pi", "provider_native_event", Some("pi-context.v1"))
            | (
                "claude" | "codex" | "copilot" | "pi" | "opencode",
                "v1_receipt",
                None
            )
    )
}

fn validate_observation(row: &Value) -> Result<()> {
    if row["origin"] == "provider_native_fact" {
        return super::facts::validate_row(row);
    }
    let provider = text(row, "provider")?;
    let origin = text(row, "origin")?;
    let profile = row
        .get("profile")
        .map(|_| text(row, "profile"))
        .transpose()?;
    let native = origin != "v1_receipt";
    require(
        valid_provenance(provider, origin, profile)
            && row.as_object().is_some_and(|o| {
                o.len() == 13 + if native { 5 } else { 0 } + usize::from(profile.is_some())
                    && o.keys().all(|key| match key.as_str() {
                        "observation_id" | "provider" | "origin" | "capability"
                        | "reported_state" | "age_ms" | "freshness" | "reference_trust"
                        | "root_eligibility" | "native_identity" | "job_completion" | "binding"
                        | "attribution" => true,
                        "fresh_pending_count"
                        | "unacknowledged_pending_count"
                        | "attention_revision" => native,
                        "hosting_state" | "association_revision" => native,
                        "profile" => profile.is_some(),
                        _ => false,
                    })
            })
            && row["reference_trust"] == "caller_supplied_unverified"
            && row["native_identity"] == "unestablished"
            && row["job_completion"] == "unestablished"
            && row["binding"] == "none"
            && row["attribution"] == "Coven"
            && row["root_eligibility"]
                == if matches!(origin, "djinn_native_event" | "djinn_client_attachment")
                    || profile == Some("opencode-root.v1")
                {
                    "producer_checked"
                } else if profile == Some("pi-context.v1") {
                    "ui_rpc_context"
                } else {
                    "unestablished"
                }
            && row["capability"]
                == if native || provider == "claude" {
                    "activity"
                } else {
                    "reference_only"
                }
            && matches!(
                text(row, "freshness")?,
                "fresh" | "expired" | "historical" | "unknown"
            )
            && matches!(
                text(row, "reported_state")?,
                "unknown" | "working" | "idle" | "blocked" | "ended"
            )
            && (row["freshness"] == "fresh" || row["reported_state"] == "unknown")
            && (native || provider == "claude" || row["reported_state"] == "unknown")
            && (profile != Some("pi-context.v1")
                || !matches!(text(row, "reported_state")?, "blocked" | "ended"))
            && row
                .get("age_ms")
                .is_some_and(|age| age.is_null() || age.as_u64().is_some()),
        "agent_invalid_schema",
    )?;
    if native {
        validate_attention(row)?;
        require(
            matches!(text(row, "hosting_state")?, "none" | "local_full_tui_root")
                && row.get("association_revision").is_some_and(|r| {
                    if row["hosting_state"] == "none" {
                        r.is_null()
                    } else {
                        r.as_str().is_some_and(|s| hex(s, 32))
                    }
                }),
            "agent_invalid_schema",
        )?;
    }
    Ok(())
}

fn row(
    id: Value,
    provider: Value,
    origin: &str,
    capability: &str,
    state: &str,
    age: Option<u64>,
) -> Value {
    let freshness = match age {
        None => "historical",
        Some(age) if age >= 30_000 => "expired",
        _ => "fresh",
    };
    json!({"observation_id":id,"provider":provider,"origin":origin,"capability":capability,
        "reported_state":if freshness == "fresh" && capability == "activity" {state} else {"unknown"},
        "age_ms":age,"freshness":freshness,"reference_trust":"caller_supplied_unverified",
        "root_eligibility":if matches!(origin, "djinn_native_event" | "djinn_client_attachment") {"producer_checked"} else {"unestablished"},
        "native_identity":"unestablished","job_completion":"unestablished","binding":"none","attribution":"Coven"})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::time::Duration;

    fn attach(client: u64, revision: u64) -> Value {
        json!({"expected_epoch":"a".repeat(32),"client_id":format!("12345678-1234-1234-1234-{client:012x}"),
            "revision":revision,"operation":"attach","instance_id":"12345678-1234-1234-1234-123456789abc",
            "session_id":"PRIVATE-SID","scope":"local_full_tui_root","target":{"socket":"/not/contacted",
            "device":1,"inode":2,"terminal_id":"private-term","runtime_generation":"12345678-1234-1234-1234-123456789abc"}})
    }
    fn detach(client: u64, revision: u64) -> Value {
        json!({"expected_epoch":"a".repeat(32),"client_id":format!("12345678-1234-1234-1234-{client:012x}"),
            "revision":revision,"operation":"detach"})
    }
    fn attachment(store: &mut NativeStore, p: Value, now: Instant) -> Result<Value> {
        store.report_attachment(&p, &"a".repeat(32), now)
    }

    #[test]
    fn current_view_restart_keeps_distinct_roots_and_history_without_clock_mutation() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        store.report(&status(1, "busy"), now).unwrap();
        let old = view(&store, now)["observations"][0]["observation_id"].clone();
        let later = now + Duration::from_secs(30);
        let mut restarted = attach(2, 1);
        restarted["instance_id"] = json!("12345678-1234-1234-1234-123456789abd");
        attachment(&mut store, restarted.clone(), later).unwrap();
        let legacy = json!({"server_epoch":"a".repeat(32),"records":[]});
        let associations = BTreeMap::new();
        let history = store.projection(legacy.clone(), later);
        let sequences = store.sequences.clone();
        let tombstones = store.tombstones.clone();
        let current = store.publisher_projection(legacy.clone(), later, &associations);
        assert_eq!(current["observations"].as_array().unwrap().len(), 1);
        assert_ne!(current["observations"][0]["observation_id"], old);
        assert_eq!(current["observations"][0]["reported_state"], "unknown");
        let new_id = current["observations"][0]["observation_id"]
            .as_str()
            .unwrap();
        assert!(store.navigation_fact(new_id, later).is_none());
        assert_eq!(store.projection(legacy.clone(), later), history);
        assert_eq!(store.sequences, sequences);
        assert_eq!(store.tombstones, tombstones);

        attachment(&mut store, attach(1, 1), later).unwrap();
        let both = store.publisher_projection(legacy.clone(), later, &associations);
        assert_eq!(both["observations"].as_array().unwrap().len(), 2);
        attachment(&mut store, detach(1, 2), later).unwrap();
        let remaining = store.publisher_projection(legacy.clone(), later, &associations);
        assert_eq!(remaining["observations"], current["observations"]);
        let mut conflict = restarted;
        conflict["client_id"] = attach(3, 1)["client_id"].clone();
        conflict["target"]["terminal_id"] = json!("conflicting-root");
        attachment(&mut store, conflict, later).unwrap();
        assert!(store.producer_association(new_id, later).is_none());
        assert_eq!(
            store.current_deadline(new_id, later),
            Some(later + ATTACHMENT_TTL)
        );
        assert_eq!(
            store.publisher_projection(legacy.clone(), later, &associations)["observations"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(store.publisher_projection(legacy, later + ATTACHMENT_TTL, &associations)["observations"].as_array().unwrap().is_empty());
    }

    #[test]
    fn current_selection_uses_last_live_client_deadline_and_explicit_end_wins() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        attachment(&mut store, attach(1, 1), now).unwrap();
        let id = view(&store, now)["observations"][0]["observation_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut conflict = attach(2, 1);
        conflict["target"]["terminal_id"] = json!("other-root");
        attachment(&mut store, conflict, now + Duration::from_secs(1)).unwrap();
        assert!(
            store
                .producer_association(&id, now + Duration::from_secs(1))
                .is_none()
        );
        let end = now + Duration::from_secs(6);
        assert_eq!(
            store.current_deadline(&id, end - Duration::from_nanos(1)),
            Some(end)
        );
        assert_eq!(store.current_deadline(&id, end), None);
        // Exercise the native ended guard independently of report-time lease revocation.
        store.observations.values_mut().next().unwrap().ended = Some(now);
        assert_eq!(
            store.current_deadline(&id, now + Duration::from_secs(1)),
            None
        );
        assert!(
            store.publisher_projection(
                json!({"server_epoch":"epoch","records":[]}),
                now + Duration::from_secs(1),
                &BTreeMap::new()
            )["observations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn current_activity_uses_latest_real_evidence_without_mutating_attention() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        store
            .report(&decision(1, "permission.asked", "request"), now)
            .unwrap();
        store
            .report(&status(2, "idle"), now + Duration::from_secs(1))
            .unwrap();
        let before = view(&store, now + Duration::from_secs(1));
        let id = before["observations"][0]["observation_id"]
            .as_str()
            .unwrap();
        let occurrence = store.occurrence;
        let current = store.publisher_projection(
            json!({"server_epoch":"epoch","records":[]}),
            now + Duration::from_secs(1),
            &BTreeMap::new(),
        );
        assert_eq!(current["observations"], before["observations"]);
        assert_eq!(view(&store, now + Duration::from_secs(1)), before);
        assert_eq!(store.occurrence, occurrence);
        assert_eq!(
            store.current_deadline(id, now + Duration::from_secs(29)),
            Some(now + Duration::from_secs(31))
        );
    }

    #[test]
    fn current_activity_deadlines_are_exact_and_lifecycle_never_renews_them() {
        for state in ["busy", "idle", "blocked"] {
            let now = Instant::now();
            let mut store = NativeStore::default();
            let p = if state == "blocked" {
                let mut p = event(1, "permission.asked");
                p["request_id"] = json!("request");
                p
            } else {
                status(1, state)
            };
            store.report(&p, now).unwrap();
            let id = view(&store, now)["observations"][0]["observation_id"]
                .as_str()
                .unwrap()
                .to_owned();
            let end = now + EVIDENCE_TTL;
            assert_eq!(
                store.current_deadline(&id, end - Duration::from_nanos(1)),
                Some(end)
            );
            assert_eq!(store.current_deadline(&id, end), None);
            store.report(&event(2, "session.updated"), end).unwrap();
            assert_eq!(store.current_deadline(&id, end), None);
            store.report(&event(3, "session.deleted"), end).unwrap();
            assert_eq!(store.current_deadline(&id, end), None);
        }
        let now = Instant::now();
        let mut store = NativeStore::default();
        store.report(&event(1, "session.created"), now).unwrap();
        let id = view(&store, now)["observations"][0]["observation_id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(store.current_deadline(&id, now), None);
    }

    #[test]
    fn publisher_djinn_retirements_never_create_a_ghost_notice_or_reserve_a_current_slot() {
        let now = Instant::now();
        let legacy = json!({"server_epoch":"epoch","records":[]});
        let mut store = NativeStore::default();
        for sequence in 1..=129 {
            let mut p = event(sequence, "session.updated");
            p["session_id"] = json!(format!("history-{sequence}"));
            store.report(&p, now).unwrap();
        }
        assert_eq!(store.retired, 1);
        assert_eq!(store.retired_djinn, 1);
        let current = store.publisher_projection(legacy.clone(), now, &BTreeMap::new());
        assert!(current["observations"].as_array().unwrap().is_empty());
        assert_eq!(current["retired_native_observations"], 0);
        assert_eq!(current["omitted_observations"], 0);
        assert_eq!(
            store.projection(legacy.clone(), now)["retired_native_observations"],
            1
        );

        let sessions: Vec<_> = store
            .observations
            .keys()
            .map(|(_, sid)| sid.clone())
            .collect();
        for (n, sid) in sessions.into_iter().enumerate() {
            let mut p = status(130 + n as u64, "busy");
            p["session_id"] = json!(sid);
            store.report(&p, now).unwrap();
        }
        let current = store.publisher_projection(legacy.clone(), now, &BTreeMap::new());
        assert_eq!(current["observations"].as_array().unwrap().len(), 128);
        assert_eq!(current["omitted_observations"], 0);
        assert_eq!(current["retired_native_observations"], 0);
        let public = store.projection(legacy, now);
        assert_eq!(public["retired_native_observations"], 1);
        assert_eq!(public["omitted_observations"], 1);
    }

    #[test]
    fn publisher_preserves_non_djinn_retirement_notices() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        for sequence in 1..=129 {
            let mut p = event(sequence, "session.updated");
            p["session_id"] = json!(format!("history-{sequence}"));
            p["provider"] = json!("opencode");
            p["profile"] = json!("opencode-root.v1");
            store.report_provider(&p, now).unwrap();
        }
        assert_eq!(store.retired_djinn, 0);
        let legacy = json!({"server_epoch":"epoch","records":[]});
        let public = store.projection(legacy.clone(), now);
        let mut current = store.publisher_projection(legacy, now, &BTreeMap::new());
        current
            .as_object_mut()
            .unwrap()
            .remove("_publisher_current");
        assert_eq!(current, public);
        assert_eq!(current["retired_native_observations"], 1);
    }

    #[test]
    fn publisher_filters_history_before_cap_without_counting_it_as_evidence_loss() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        attachment(&mut store, attach(1, 1), now).unwrap();
        for sequence in 1..128 {
            let mut p = event(sequence, "session.updated");
            p["session_id"] = json!(format!("history-{sequence}"));
            store.report(&p, now).unwrap();
        }
        let records: Vec<_> = (0..127).map(|n| json!({"record_id":format!("{n:064x}"),
            "provider":"djinn","reported_state":"unknown","age_ms":null})).chain([
                json!({"record_id":"f".repeat(64),"provider":"codex","reported_state":"unknown","age_ms":null})
            ]).collect();
        let legacy = json!({"server_epoch":"a".repeat(32),"records":records});
        let public = store.projection(legacy.clone(), now);
        assert_eq!(public["omitted_observations"], 129);
        let current = store.publisher_projection(legacy.clone(), now, &BTreeMap::new());
        assert_eq!(current["observations"].as_array().unwrap().len(), 2);
        assert_eq!(current["omitted_observations"], 0);
        assert_eq!(current["retired_native_observations"], 0);
        assert_eq!(current["observations"][1]["provider"], "codex");
        assert_eq!(store.projection(legacy, now), public);
    }

    #[test]
    fn attachment_deletion_revokes_all_matching_selections_but_retains_watermarks() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        attachment(&mut store, attach(1, 1), now).unwrap();
        let mut conflict = attach(2, 1);
        conflict["target"]["terminal_id"] = json!("different");
        attachment(&mut store, conflict, now).unwrap();
        let mut other = attach(3, 1);
        other["session_id"] = json!("other");
        attachment(&mut store, other, now).unwrap();
        store
            .report(&event(1, "session.deleted"), now + Duration::from_micros(1))
            .unwrap();
        for client in 1..=3 {
            let lease = &store.clients[text(&attach(client, 1), "client_id").unwrap()];
            assert_eq!(lease.selection.is_some(), client == 3);
            assert_eq!(lease.revision, 1);
            assert_eq!(lease.seen, now);
        }
        assert_eq!(
            attachment(&mut store, attach(1, 1), now + Duration::from_micros(2))
                .unwrap_err()
                .code(),
            "agent_stale_revision"
        );
        assert_eq!(
            attachment(&mut store, attach(1, 2), now + Duration::from_micros(3))
                .unwrap_err()
                .code(),
            "agent_session_ended"
        );
        assert_eq!(
            store.clients[text(&attach(1, 1), "client_id").unwrap()].revision,
            2
        );
    }

    #[test]
    fn attachment_deletion_forgetting_ended_row_clears_selections_and_counts_retirement_once() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        for client in 1..=2 {
            attachment(&mut store, attach(client, 1), now).unwrap();
        }
        let key = (
            text(&attach(1, 1), "instance_id").unwrap().to_owned(),
            "PRIVATE-SID".to_owned(),
        );
        // Exercise removal independently of report-time deletion revocation.
        store.observations.get_mut(&key).unwrap().ended = Some(now);
        store.tombstones.insert(key.clone(), now);
        store.forget_tombstone(&key);
        assert!(!store.observations.contains_key(&key));
        assert!(!store.tombstones.contains_key(&key));
        assert_eq!(store.retired, 1);
        assert!(
            store
                .clients
                .values()
                .all(|c| c.selection.is_none() && c.revision == 1 && c.seen == now)
        );
        store.forget_tombstone(&key);
        assert_eq!(store.retired, 1);
    }

    #[test]
    fn attachment_schema_is_closed_for_both_operations_and_bounded_before_transport() {
        for mut p in [attach(1, 1), detach(1, 1)] {
            p["contract"] = json!("djinn.client-attachment.v1");
            let valid = attachment_params(&p, true).unwrap();
            assert!(valid.get("contract").is_none());
            for k in [
                "client_id",
                "expected_epoch",
                "revision",
                "operation",
                "contract",
            ] {
                let mut bad = p.clone();
                bad.as_object_mut().unwrap().remove(k);
                assert!(attachment_params(&bad, true).is_err());
            }
            for field in ["prompt", "sessionID", "scoop", "parent_id"] {
                let mut bad = p.clone();
                bad[field] = json!("PRIVATE");
                assert!(attachment_params(&bad, true).is_err());
            }
            for revision in [
                json!(0),
                json!(-1),
                json!(1.5),
                json!(9_007_199_254_740_992u64),
            ] {
                let mut bad = p.clone();
                bad["revision"] = revision;
                assert!(attachment_params(&bad, true).is_err());
            }
        }
        let mut p = detach(1, 2);
        p["scope"] = json!("local_full_tui_root");
        assert!(attachment_params(&p, false).is_err());
        let mut p = attach(1, 1);
        p["session_id"] = json!("x".repeat(513));
        assert!(attachment_params(&p, false).is_err());
        p = attach(1, 1);
        p["target"]["runtime_generation"] = json!("generation");
        assert!(attachment_params(&p, false).is_err());
        assert!(super::super::attachment_report_payload(None, &vec![b' '; 8193], true).is_err());
    }

    #[test]
    fn quiet_attachment_joins_exact_instance_without_native_clock_or_sequence_fabrication() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        let mut other = status(1, "busy");
        other["instance_id"] = json!("12345678-1234-1234-1234-123456789abd");
        store.report(&other, now).unwrap();
        attachment(&mut store, attach(1, 1), now).unwrap();
        assert_eq!(store.sequences.len(), 1);
        let normal = control(&mut store, "observations", json!({}), now).unwrap();
        validate_control_result("observations", &normal).unwrap();
        let quiet = normal["observations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["origin"] == "djinn_client_attachment")
            .unwrap();
        assert_eq!(quiet["reported_state"], "unknown");
        assert_eq!(quiet["age_ms"], Value::Null);
        assert_eq!(quiet["freshness"], "unknown");
        assert_eq!(quiet["hosting_state"], "local_full_tui_root");
        let id = quiet["observation_id"].as_str().unwrap().to_owned();
        let binding = store.producer_association(&id, now).unwrap();
        assert!(!normal.to_string().contains("PRIVATE"));
        assert!(!normal.to_string().contains("private-term"));
        for field in ["age_ms", "association_revision", "hosting_state"] {
            let mut bad = normal.clone();
            let row = bad["observations"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|r| r["observation_id"] == id)
                .unwrap();
            row.as_object_mut().unwrap().remove(field);
            row["private"] = json!("SECRET");
            assert!(validate_control_result("observations", &bad).is_err());
        }
        store.report(&status(1, "idle"), now).unwrap();
        for (rev, secs) in [
            (2, 4),
            (3, 8),
            (4, 12),
            (5, 16),
            (6, 20),
            (7, 24),
            (8, 28),
            (9, 32),
        ] {
            attachment(&mut store, attach(1, rev), now + Duration::from_secs(secs)).unwrap();
            assert_eq!(
                store
                    .producer_association(&id, now + Duration::from_secs(secs))
                    .unwrap(),
                binding
            );
        }
        let normal = control(
            &mut store,
            "observations",
            json!({}),
            now + Duration::from_secs(32),
        )
        .unwrap();
        let joined = normal["observations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["observation_id"] == id)
            .unwrap();
        assert_eq!(joined["origin"], "djinn_native_event");
        assert_eq!(joined["reported_state"], "unknown");
        assert_eq!(joined["age_ms"], 32000);
        assert_eq!(store.sequences.len(), 2);
        assert_eq!(normal["observations"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn attachment_selection_cas_ambiguity_expiry_and_watermarks_never_revive_old_binding() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        attachment(&mut store, attach(1, 1), now).unwrap();
        let id = view(&store, now)["observations"][0]["observation_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let first = store.producer_association(&id, now).unwrap().0;
        attachment(&mut store, attach(2, 1), now).unwrap();
        let shared = store.producer_association(&id, now).unwrap().0;
        assert_ne!(shared, first);
        attachment(&mut store, attach(2, 2), now).unwrap();
        assert_eq!(store.producer_association(&id, now).unwrap().0, shared);
        let mut different = attach(2, 3);
        different["target"]["terminal_id"] = json!("B");
        attachment(&mut store, different, now).unwrap();
        assert!(store.producer_association(&id, now).is_none());
        assert!(attachment(&mut store, detach(2, 2), now).is_err());
        assert!(store.producer_association(&id, now).is_none());
        attachment(&mut store, detach(2, 4), now).unwrap();
        let after = store.producer_association(&id, now).unwrap().0;
        assert_ne!(after, first);
        assert_ne!(after, shared);
        assert!(attachment(&mut store, attach(2, 3), now).is_err());
        let expired = now + ATTACHMENT_TTL;
        assert!(store.producer_association(&id, expired).is_none());
        assert!(attachment(&mut store, attach(1, 1), expired).is_err());
        assert!(store.producer_association(&id, expired).is_none());
        attachment(&mut store, attach(1, 2), expired).unwrap();
        assert_ne!(store.producer_association(&id, expired).unwrap().0, after);
        let mut b = attach(1, 3);
        b["session_id"] = json!("B");
        attachment(&mut store, b, expired).unwrap();
        assert!(store.producer_association(&id, expired).is_none());
        attachment(&mut store, detach(1, 4), expired).unwrap();
        assert!(store.clients.values().all(|c| c.selection.is_none()));
    }

    #[test]
    fn attachment_gaps_end_epochs_retirement_and_capacity_fail_closed() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        store.report(&status(1, "busy"), now).unwrap();
        attachment(&mut store, attach(1, 1), now).unwrap();
        let id = view(&store, now)["observations"][0]["observation_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let initial = store.producer_association(&id, now).unwrap().0;
        store.report(&event(3, "session.updated"), now).unwrap();
        assert!(store.producer_association(&id, now).is_none());
        assert!(attachment(&mut store, attach(1, 1), now).is_err());
        attachment(&mut store, attach(1, 2), now).unwrap();
        assert_ne!(initial, store.producer_association(&id, now).unwrap().0);
        store.report(&event(4, "session.deleted"), now).unwrap();
        assert!(attachment(&mut store, attach(1, 3), now).is_err());
        assert!(store.producer_association(&id, now).is_none());
        attachment(&mut store, detach(1, 4), now).unwrap();
        let mut stale = attach(1, 5);
        stale["expected_epoch"] = json!("b".repeat(32));
        assert!(attachment(&mut NativeStore::default(), stale, now).is_err());
        let mut store = NativeStore::default();
        for client in 1..=128 {
            attachment(&mut store, attach(client, 1), now).unwrap();
        }
        assert!(attachment(&mut store, attach(129, 1), now).is_err());
        attachment(&mut store, attach(1, 2), now).unwrap();
        for client in 1..=128 {
            attachment(&mut store, detach(client, 3), now).unwrap();
        }
        for client in 129..=512 {
            attachment(&mut store, detach(client, 3), now).unwrap();
        }
        assert_eq!(store.clients.len(), 512);
        assert!(attachment(&mut store, detach(513, 1), now).is_err());
        attachment(&mut store, detach(513, 1), now + RETENTION_TTL).unwrap();
        assert_eq!(store.clients.len(), 1);
        let mut full = NativeStore::default();
        for n in 0..128 {
            let mut p = status(n + 1, "busy");
            p["session_id"] = json!(format!("s{n}"));
            full.report(&p, now).unwrap();
        }
        assert!(attachment(&mut full, attach(1, 1), now).is_err());
        assert_eq!(full.observations.len(), 128);
    }

    #[test]
    fn quiet_attachment_capacity_reclaims_detached_and_expired_rows_not_watermarks() {
        for delay in [
            Duration::ZERO,
            ATTACHMENT_TTL,
            RETENTION_TTL - Duration::from_millis(1),
            RETENTION_TTL,
        ] {
            let now = Instant::now();
            let mut store = NativeStore::default();
            for client in 1..=128 {
                let mut p = attach(client, 1);
                p["session_id"] = json!(format!("quiet-{client}"));
                attachment(&mut store, p, now).unwrap();
            }
            assert_eq!(store.observations.len(), 128);
            if delay.is_zero() {
                for client in 1..=128 {
                    attachment(&mut store, detach(client, 2), now).unwrap();
                }
            } else {
                let before_retirement = now + delay - Duration::from_millis(1);
                let mut stale = attach(1, 1);
                stale["session_id"] = json!("quiet-1");
                assert_eq!(
                    attachment(&mut store, stale, before_retirement)
                        .unwrap_err()
                        .code(),
                    "agent_stale_revision"
                );
                // Stale delivery neither advances nor renews retained watermarks.
                assert_eq!(
                    store.clients.values().filter(|c| c.seen == now).count(),
                    128
                );
            }
            let mut next = attach(129, 1);
            next["session_id"] = json!("quiet-129");
            attachment(&mut store, next, now + delay).unwrap();
            assert_eq!(store.observations.len(), 128);
            assert_eq!(store.retired, 1);
            assert!(!store.observations.contains_key(&(
                text(&attach(1, 1), "instance_id").unwrap().into(),
                "quiet-1".into()
            )));
            assert_eq!(
                store.clients.len(),
                if delay == RETENTION_TTL { 1 } else { 129 }
            );
            let mut old = attach(1, 1);
            old["session_id"] = json!("quiet-1");
            if delay == RETENTION_TTL {
                attachment(&mut store, old, now + delay).unwrap();
                assert_eq!(store.observations.len(), 128);
                assert_eq!(store.retired, 2);
            } else {
                assert_eq!(
                    attachment(&mut store, old, now + delay).unwrap_err().code(),
                    "agent_stale_revision"
                );
            }
        }
    }

    #[test]
    fn reclamation_preserves_fresh_attention_and_all_live_leases_including_ambiguous() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        for n in 1..=126 {
            let mut p = decision(n, "permission.asked", "pending");
            p["session_id"] = json!(format!("fresh-{n}"));
            store.report(&p, now).unwrap();
        }
        attachment(&mut store, attach(1, 1), now).unwrap();
        let mut conflict = attach(2, 1);
        conflict["target"]["terminal_id"] = json!("different");
        attachment(&mut store, conflict, now).unwrap();
        let mut unleased = attach(3, 1);
        unleased["session_id"] = json!("unleased");
        attachment(&mut store, unleased, now).unwrap();
        attachment(&mut store, detach(3, 2), now).unwrap();
        let mut next = attach(4, 1);
        next["session_id"] = json!("new");
        attachment(&mut store, next, now).unwrap();
        assert_eq!(store.observations.len(), 128);
        assert_eq!(store.retired, 1);
        assert!(
            store
                .clients
                .values()
                .take(2)
                .all(|c| c.selection.is_some())
        );
        assert_eq!(
            store
                .observations
                .values()
                .filter(|o| o.active(now))
                .count(),
            126
        );
        let mut refused = attach(5, 1);
        refused["session_id"] = json!("refused");
        assert_eq!(
            attachment(&mut store, refused, now).unwrap_err().code(),
            "agent_capacity"
        );
        let mut event = status(127, "busy");
        event["session_id"] = json!("refused-native");
        assert_eq!(
            store.report(&event, now).unwrap_err().code(),
            "agent_capacity"
        );
        assert_eq!(store.retired, 1);
        assert_eq!(store.observations.len(), 128);
        assert_eq!(
            store
                .clients
                .values()
                .filter(|c| c.selection.is_some())
                .count(),
            3
        );
    }

    #[test]
    fn rejected_higher_selection_releases_old_binding_and_consumes_revision() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        attachment(&mut store, attach(1, 1), now).unwrap();
        let id = view(&store, now)["observations"][0]["observation_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut ended = event(1, "session.deleted");
        ended["session_id"] = json!("ended");
        store.report(&ended, now).unwrap();
        let mut replacement = attach(1, 2);
        replacement["session_id"] = json!("ended");
        assert_eq!(
            attachment(&mut store, replacement, now).unwrap_err().code(),
            "agent_session_ended"
        );
        assert!(store.producer_association(&id, now).is_none());
        assert!(store.clients.values().all(|c| c.selection.is_none()));
        assert_eq!(
            attachment(&mut store, attach(1, 2), now)
                .unwrap_err()
                .code(),
            "agent_stale_revision"
        );
        assert!(store.producer_association(&id, now).is_none());
    }

    fn provider(mut value: Value, name: &str) -> Value {
        value["provider"] = json!(name);
        value["profile"] = json!(if name == "pi" {
            "pi-context.v1"
        } else {
            "opencode-root.v1"
        });
        value
    }

    fn control(
        store: &mut NativeStore,
        method: &str,
        params: Value,
        now: Instant,
    ) -> Result<Value> {
        store.control(
            method,
            &params,
            json!({"server_epoch":"a".repeat(32),"records":[]}),
            now,
        )
    }

    fn ack_params(row: &Value, apply: bool) -> Value {
        json!({"observation_id":row["observation_id"],"expected_epoch":"a".repeat(32),"expected_revision":row["attention_revision"],"apply":apply})
    }

    #[test]
    fn provider_profiles_are_closed_and_have_no_legacy_fallback() {
        for name in ["opencode", "pi"] {
            for event_name in [
                "session.created",
                "session.updated",
                "session.deleted",
                "session.status",
                "permission.asked",
                "permission.replied",
                "question.asked",
                "question.replied",
                "question.rejected",
                "permission.cancelled",
                "question.cancelled",
                "shutdown",
            ] {
                let mut p = provider(event(1, event_name), name);
                p["contract"] = json!(PROVIDER_RPC);
                if event_name == "session.status" {
                    p["status"] = json!("busy");
                }
                if event_name.starts_with("permission.") || event_name.starts_with("question.") {
                    p["request_id"] = json!("private-request");
                }
                let accepted = !event_name.ends_with("cancelled")
                    && event_name != "shutdown"
                    && (name == "opencode"
                        || matches!(event_name, "session.updated" | "session.status"));
                assert_eq!(
                    params_for_envelope(&p).is_ok(),
                    accepted,
                    "{name}/{event_name}"
                );
                if !accepted {
                    continue;
                }
                for field in ["prompt", "path", "parent_id", "root_qualified", "payload"] {
                    let mut bad = p.clone();
                    bad[field] = json!("private");
                    assert!(params_for_envelope(&bad).is_err());
                }
                let normalized = params_for_envelope(&p).unwrap();
                assert_eq!(normalized["provider"], name);
                assert!(normalized.get("contract").is_none());
                p["profile"] = json!("future.v1");
                assert!(params_for_envelope(&p).is_err());
            }
        }
        let mut bad = provider(status(1, "retry"), "pi");
        assert!(provider_params(&bad, false).is_err());
        bad["provider"] = json!("opencode");
        assert!(provider_params(&bad, false).is_err());
    }

    #[test]
    fn identical_native_references_are_isolated_by_provider_and_profile() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        store.report(&status(1, "busy"), now).unwrap();
        for name in ["pi", "opencode"] {
            store
                .report_provider(&provider(status(1, "busy"), name), now)
                .unwrap();
        }
        store
            .report_provider(&provider(event(2, "session.deleted"), "opencode"), now)
            .unwrap();
        store
            .report_provider(&provider(status(2, "idle"), "pi"), now)
            .unwrap();
        store.report(&status(2, "retry"), now).unwrap();
        assert!(
            store
                .report_provider(&provider(status(3, "busy"), "opencode"), now)
                .is_err()
        );
        let rows = view(&store, now)["observations"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(
            rows.iter()
                .map(|r| r["observation_id"].as_str().unwrap())
                .collect::<BTreeSet<_>>()
                .len(),
            3
        );
        let pi = rows.iter().find(|r| r["provider"] == "pi").unwrap();
        assert_eq!(pi["root_eligibility"], "ui_rpc_context");
        assert_eq!(pi["native_identity"], "unestablished");
        assert_eq!(pi["reported_state"], "idle");
        let old_hash = format!(
            "{:x}",
            Sha256::digest(b"native\x0012345678-1234-1234-1234-123456789abc\x00PRIVATE-SID")
        );
        assert_eq!(
            rows.iter().find(|r| r["provider"] == "djinn").unwrap()["observation_id"],
            old_hash
        );
        let normal = control(&mut store, "observations", json!({}), now).unwrap();
        validate_control_result("observations", &normal).unwrap();
        assert!(!normal.to_string().contains("PRIVATE"));
    }

    #[test]
    fn acknowledgement_is_occurrence_scoped_previewed_and_never_resolves_decisions() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        for (seq, kind) in [(1, "permission.asked"), (2, "question.asked")] {
            store
                .report(&decision(seq, kind, "private-request"), now)
                .unwrap();
        }
        let list = control(&mut store, "attention.list", json!({}), now).unwrap();
        validate_control_result("attention.list", &list).unwrap();
        let row = &list["observations"][0];
        assert_eq!(row["fresh_pending_count"], 2);
        let params = ack_params(row, false);
        let preview = control(&mut store, "attention.acknowledge", params.clone(), now).unwrap();
        assert_eq!(preview["observations"][0], *row);
        assert_eq!(
            control(&mut store, "attention.list", json!({}), now).unwrap(),
            list
        );
        let applied = control(
            &mut store,
            "attention.acknowledge",
            ack_params(row, true),
            now,
        )
        .unwrap();
        validate_control_result("attention.acknowledge", &applied).unwrap();
        assert_eq!(
            applied["observations"][0]["unacknowledged_pending_count"],
            0
        );
        assert_ne!(
            applied["observations"][0]["attention_revision"],
            row["attention_revision"]
        );
        assert_eq!(state(&store, now), "blocked");
        store
            .report(
                &decision(3, "permission.asked", "private-request"),
                now + Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(
            control(&mut store, "attention.list", json!({}), now).unwrap()["observations"],
            applied["observations"]
        );
        store.report(&status(4, "idle"), now).unwrap();
        assert_eq!(state(&store, now), "blocked");
        store
            .report(&decision(5, "permission.replied", "private-request"), now)
            .unwrap();
        store
            .report(&decision(6, "permission.asked", "private-request"), now)
            .unwrap();
        let next = control(&mut store, "attention.list", json!({}), now).unwrap();
        assert_eq!(next["observations"][0]["unacknowledged_pending_count"], 1);
        assert!(
            control(
                &mut store,
                "attention.acknowledge",
                ack_params(&applied["observations"][0], true),
                now
            )
            .is_err()
        );
        let mut stale_epoch = ack_params(&next["observations"][0], true);
        stale_epoch["expected_epoch"] = json!("b".repeat(32));
        assert!(control(&mut store, "attention.acknowledge", stale_epoch, now).is_err());
        assert!(
            control(
                &mut store,
                "attention.acknowledge",
                ack_params(&next["observations"][0], true),
                now + EVIDENCE_TTL
            )
            .is_err()
        );
        store
            .report(
                &decision(7, "permission.asked", "private-request"),
                now + EVIDENCE_TTL,
            )
            .unwrap();
        let reask = control(&mut store, "attention.list", json!({}), now + EVIDENCE_TTL).unwrap();
        assert_eq!(reask["observations"][0]["fresh_pending_count"], 1);
        assert_ne!(
            reask["observations"][0]["attention_revision"],
            next["observations"][0]["attention_revision"]
        );
        store
            .report(&event(9, "session.updated"), now + EVIDENCE_TTL)
            .unwrap();
        assert!(
            control(
                &mut store,
                "attention.acknowledge",
                ack_params(&reask["observations"][0], true),
                now + EVIDENCE_TTL
            )
            .is_err()
        );
        assert_eq!(
            control(&mut store, "attention.list", json!({}), now + EVIDENCE_TTL).unwrap()["observations"],
            json!([])
        );
        assert!(!list.to_string().contains("private-request"));
    }

    #[test]
    fn attention_priority_precedes_display_truncation_and_list_is_complete() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        for n in 1..=128 {
            let mut p = decision(n, "permission.asked", "private");
            p["session_id"] = json!(format!("s{n}"));
            store.report(&p, now).unwrap();
        }
        let list = control(&mut store, "attention.list", json!({}), now).unwrap();
        assert_eq!(list["observations"].as_array().unwrap().len(), 128);
        let acked = &list["observations"][0];
        control(
            &mut store,
            "attention.acknowledge",
            ack_params(acked, true),
            now,
        )
        .unwrap();
        let legacy = json!({"server_epoch":"a".repeat(32),"records":[{"record_id":"f".repeat(64),"provider":"claude","reported_state":"working","age_ms":0}]});
        let projection = store.projection(legacy, now);
        let rows = projection["observations"].as_array().unwrap();
        assert_eq!(rows.len(), 127);
        assert!(rows.iter().all(|r| r["unacknowledged_pending_count"] == 1));
        assert!(
            rows.iter()
                .all(|r| r["observation_id"] != acked["observation_id"])
        );
        assert_eq!(
            control(&mut store, "attention.list", json!({}), now).unwrap()["observations"]
                .as_array()
                .unwrap()
                .len(),
            128
        );
    }

    #[test]
    fn deleted_unknown_and_restarted_attention_cannot_be_acknowledged() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        store
            .report_provider(
                &provider(decision(1, "question.asked", "private"), "opencode"),
                now,
            )
            .unwrap();
        let list = control(&mut store, "attention.list", json!({}), now).unwrap();
        let params = ack_params(&list["observations"][0], true);
        store
            .report_provider(&provider(event(2, "session.deleted"), "opencode"), now)
            .unwrap();
        assert!(control(&mut store, "attention.acknowledge", params.clone(), now).is_err());
        assert!(
            store
                .report_provider(
                    &provider(decision(3, "question.asked", "private"), "opencode"),
                    now
                )
                .is_err()
        );
        assert!(
            control(
                &mut NativeStore::default(),
                "attention.acknowledge",
                params,
                now
            )
            .is_err()
        );
        let normal = control(&mut store, "observations", json!({}), now).unwrap();
        validate_control_result("observations", &normal).unwrap();
        for field in ["prompt", "request_id", "session_id", "cwd"] {
            let mut invalid = normal.clone();
            invalid["observations"][0][field] = json!("private");
            assert!(validate_control_result("observations", &invalid).is_err());
        }
        let mut invalid = list.clone();
        invalid["observations"][0]["request_id"] = json!("private");
        assert!(validate_control_result("attention.list", &invalid).is_err());
    }

    #[test]
    fn control_observation_rows_reject_age_substitution_for_every_origin() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        store.report(&status(1, "busy"), now).unwrap();
        for name in ["pi", "opencode"] {
            store
                .report_provider(&provider(status(1, "busy"), name), now)
                .unwrap();
        }
        let legacy = json!({"server_epoch":"a".repeat(32),"records":[{
            "record_id":"b".repeat(64),"provider":"claude","reported_state":"working","age_ms":0
        }]});
        let normal = store.projection(legacy, now);
        validate_control_result("observations", &normal).unwrap();
        for index in 0..4 {
            for field in ["prompt", "session_id", "request_id", "unknown"] {
                let mut invalid = normal.clone();
                let row = invalid["observations"][index].as_object_mut().unwrap();
                let len = row.len();
                assert!(row.remove("age_ms").is_some());
                row.insert(field.into(), json!("PRIVATE"));
                assert_eq!(row.len(), len);
                assert!(validate_control_result("observations", &invalid).is_err());
            }
            let mut valid = normal.clone();
            valid["observations"][index]["age_ms"] = Value::Null;
            validate_control_result("observations", &valid).unwrap();
            valid["observations"][index]
                .as_object_mut()
                .unwrap()
                .remove("age_ms");
            assert!(validate_control_result("observations", &valid).is_err());
        }
    }

    fn event(sequence: u64, event: &str) -> Value {
        json!({"instance_id":"12345678-1234-1234-1234-123456789abc","session_id":"PRIVATE-SID","sequence":sequence,"event":event})
    }
    fn view(store: &NativeStore, now: Instant) -> Value {
        store.projection(json!({"server_epoch":"epoch","records":[]}), now)
    }

    fn status(sequence: u64, state: &str) -> Value {
        let mut p = event(sequence, "session.status");
        p["status"] = json!(state);
        p
    }

    fn decision(sequence: u64, name: &str, id: &str) -> Value {
        let mut p = event(sequence, name);
        p["request_id"] = json!(id);
        p
    }

    fn state(store: &NativeStore, now: Instant) -> String {
        view(store, now)["observations"][0]["reported_state"]
            .as_str()
            .unwrap()
            .into()
    }

    #[test]
    fn metadata_repeated_creation_and_unmatched_replies_do_not_renew_status() {
        for name in [
            "session.updated",
            "session.created",
            "permission.replied",
            "permission.cancelled",
            "question.replied",
            "question.rejected",
            "question.cancelled",
        ] {
            let now = Instant::now();
            let mut store = NativeStore::default();
            store.report(&status(1, "busy"), now).unwrap();
            for (sequence, seconds) in [(2, 29), (3, 40)] {
                let mut p = event(sequence, name);
                if name.starts_with("permission.") || name.starts_with("question.") {
                    p["request_id"] = json!("unmatched");
                }
                store
                    .report(&p, now + Duration::from_secs(seconds))
                    .unwrap();
            }
            for seconds in [30, 40, 60] {
                let row = &view(&store, now + Duration::from_secs(seconds))["observations"][0];
                assert_eq!(row["reported_state"], "unknown", "{name}");
                assert_eq!(row["freshness"], "expired");
                assert_eq!(row["age_ms"], seconds * 1000);
                assert_eq!(row["provider"], "djinn");
            }
        }
    }

    #[test]
    fn pending_clocks_are_independent_from_status_metadata_and_other_requests() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        store
            .report(&decision(1, "permission.asked", "lost"), now)
            .unwrap();
        store
            .report(&status(2, "busy"), now + Duration::from_secs(29))
            .unwrap();
        assert_eq!(state(&store, now + Duration::from_secs(29)), "blocked");
        assert_eq!(state(&store, now + Duration::from_secs(30)), "working");
        store
            .report(&status(3, "busy"), now + Duration::from_secs(40))
            .unwrap();
        assert_eq!(state(&store, now + Duration::from_secs(40)), "working");
        assert_eq!(state(&store, now + Duration::from_secs(70)), "unknown");

        let mut store = NativeStore::default();
        store
            .report(&decision(1, "question.asked", "old"), now)
            .unwrap();
        store
            .report(
                &decision(2, "question.asked", "old"),
                now + Duration::from_secs(29),
            )
            .unwrap();
        store
            .report(&event(3, "session.created"), now + Duration::from_secs(29))
            .unwrap();
        store
            .report(
                &decision(4, "permission.replied", "unmatched"),
                now + Duration::from_secs(29),
            )
            .unwrap();
        assert_eq!(state(&store, now + Duration::from_secs(30)), "unknown");
        store
            .report(
                &decision(5, "permission.asked", "new"),
                now + Duration::from_secs(40),
            )
            .unwrap();
        assert_eq!(state(&store, now + Duration::from_secs(40)), "blocked");
        assert_eq!(store.observations.values().next().unwrap().pending.len(), 1);
    }

    #[test]
    fn gap_invalidates_whole_instance_but_not_other_instances_or_tombstones() {
        for incoming in [
            "session.updated",
            "session.created",
            "session.status",
            "permission.asked",
            "permission.replied",
            "question.cancelled",
        ] {
            let now = Instant::now();
            let mut store = NativeStore::default();
            store.report(&status(1, "busy"), now).unwrap();
            let mut other = decision(2, "question.asked", "lost");
            other["session_id"] = json!("second");
            store.report(&other, now).unwrap();
            let mut independent = status(1, "busy");
            independent["instance_id"] = json!("12345678-1234-1234-1234-123456789abd");
            store.report(&independent, now).unwrap();
            let mut ended = event(3, "session.deleted");
            ended["session_id"] = json!("ended");
            store.report(&ended, now).unwrap();
            let mut gap = event(5, incoming);
            if incoming == "session.status" {
                gap["status"] = json!("idle");
            }
            if incoming.starts_with("permission.") || incoming.starts_with("question.") {
                gap["request_id"] = json!("new");
            }
            store.report(&gap, now + Duration::from_secs(1)).unwrap();
            let first = store
                .observations
                .get(&(
                    text(&gap, "instance_id").unwrap().into(),
                    "PRIVATE-SID".into(),
                ))
                .unwrap();
            assert_eq!(
                first.fact(now + Duration::from_secs(1)).map(|(s, _)| s),
                match incoming {
                    "session.status" => Some("idle"),
                    "permission.asked" => Some("blocked"),
                    _ => None,
                }
            );
            let second = store
                .observations
                .get(&(text(&gap, "instance_id").unwrap().into(), "second".into()))
                .unwrap();
            assert!(second.fact(now).is_none());
            assert!(second.pending.is_empty());
            assert!(
                store
                    .observations
                    .get(&(
                        text(&independent, "instance_id").unwrap().into(),
                        "PRIVATE-SID".into()
                    ))
                    .unwrap()
                    .status
                    .is_some()
            );
            assert!(store.report(&ended, now + Duration::from_secs(2)).is_err());
            ended["sequence"] = json!(6);
            ended["event"] = json!("session.created");
            assert_eq!(
                store
                    .report(&ended, now + Duration::from_secs(2))
                    .unwrap_err()
                    .code(),
                "agent_session_ended"
            );
        }
    }

    #[test]
    fn terminal_facts_cannot_be_revived_or_renewed() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        store.report(&event(1, "session.deleted"), now).unwrap();
        for (index, name) in [
            "session.created",
            "session.updated",
            "session.status",
            "permission.asked",
            "permission.replied",
            "permission.cancelled",
            "question.asked",
            "question.replied",
            "question.rejected",
            "question.cancelled",
            "session.deleted",
        ]
        .into_iter()
        .enumerate()
        {
            let mut p = event(index as u64 + 2, name);
            if name == "session.status" {
                p["status"] = json!("busy");
            }
            if name.starts_with("permission.") || name.starts_with("question.") {
                p["request_id"] = json!("r");
            }
            let result = store.report(&p, now + Duration::from_secs(29));
            assert_eq!(result.is_ok(), name == "session.deleted");
            assert_eq!(state(&store, now + Duration::from_secs(29)), "ended");
            assert_eq!(state(&store, now + Duration::from_secs(30)), "unknown");
            assert_eq!(*store.tombstones.values().next().unwrap(), now);
        }
    }

    #[test]
    fn cancellation_removes_only_matching_decision_and_never_infers_status() {
        for cancellation in ["permission.cancelled", "question.cancelled"] {
            for status_age in [None, Some(0), Some(31)] {
                let now = Instant::now();
                let mut store = NativeStore::default();
                let mut sequence = 1;
                if let Some(age) = status_age {
                    store
                        .report(&status(sequence, "idle"), now - Duration::from_secs(age))
                        .unwrap();
                    sequence += 1;
                }
                for name in ["permission.asked", "question.asked"] {
                    store
                        .report(&decision(sequence, name, "same"), now)
                        .unwrap();
                    sequence += 1;
                }
                store
                    .report(&decision(sequence, cancellation, "same"), now)
                    .unwrap();
                sequence += 1;
                assert_eq!(state(&store, now), "blocked");
                assert_eq!(store.observations.values().next().unwrap().pending.len(), 1);
                let other = if cancellation == "permission.cancelled" {
                    "question.cancelled"
                } else {
                    "permission.cancelled"
                };
                store
                    .report(&decision(sequence, other, "same"), now)
                    .unwrap();
                assert_eq!(
                    state(&store, now),
                    if status_age == Some(0) {
                        "idle"
                    } else {
                        "unknown"
                    }
                );
            }
        }
    }

    #[test]
    fn lifecycle_retirement_handles_more_than_128_sessions_and_instances() {
        for separate_instances in [false, true] {
            for ended in [false, true] {
                let now = Instant::now();
                let mut store = NativeStore::default();
                for n in 0..700 {
                    let mut p = if ended {
                        event(n + 1, "session.deleted")
                    } else {
                        status(n + 1, "busy")
                    };
                    p["session_id"] = json!(format!("session-{n}"));
                    if separate_instances {
                        p["instance_id"] = json!(format!("12345678-1234-1234-1234-{n:012x}"));
                        p["sequence"] = json!(1);
                    }
                    let at = if ended {
                        now
                    } else {
                        now + Duration::from_secs(n * 31)
                    };
                    store.report(&p, at).unwrap();
                    assert!(store.observations.len() <= MAX_RECORDS);
                    assert!(store.sequences.len() <= MAX_RETAINED);
                    assert!(store.tombstones.len() <= MAX_RETAINED);
                }
                assert!(store.retired > 0);
            }
        }
    }

    #[test]
    fn active_capacity_is_real_but_expires_and_retention_is_explicitly_bounded() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        for n in 1..=128 {
            let mut p = status(n, "busy");
            p["session_id"] = json!(format!("s{n}"));
            store.report(&p, now).unwrap();
        }
        let mut p = status(129, "busy");
        p["session_id"] = json!("overflow");
        assert_eq!(store.report(&p, now).unwrap_err().code(), "agent_capacity");
        p["sequence"] = json!(130);
        store.report(&p, now + EVIDENCE_TTL).unwrap();
        assert_eq!(store.observations.len(), 128);

        let mut store = NativeStore::default();
        store.report(&event(1, "session.deleted"), now).unwrap();
        assert!(
            store
                .report(&status(2, "busy"), now + Duration::from_secs(599))
                .is_err()
        );
        store
            .report(&status(1, "busy"), now + Duration::from_secs(600))
            .unwrap_err(); // high-water still retained from rejected seq 2
        store
            .report(&status(3, "busy"), now + Duration::from_secs(600))
            .unwrap();
        assert_eq!(state(&store, now + Duration::from_secs(600)), "working");
        let mut store = NativeStore::default();
        store.report(&status(1, "busy"), now).unwrap();
        store
            .report(&status(1, "idle"), now + RETENTION_TTL)
            .unwrap();
        assert_eq!(state(&store, now + RETENTION_TTL), "idle");
    }

    #[test]
    fn count_pressure_forgets_old_replay_guards_without_leaving_derived_facts() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        store.report(&status(1, "busy"), now).unwrap();
        for n in 1..=MAX_RETAINED {
            let mut p = event(1, "session.deleted");
            p["instance_id"] = json!(format!("22345678-1234-1234-1234-{n:012x}"));
            store
                .report(&p, now + Duration::from_millis(n as u64))
                .unwrap();
        }
        let original = (
            "12345678-1234-1234-1234-123456789abc".into(),
            "PRIVATE-SID".into(),
        );
        assert!(
            store
                .observations
                .get(&original)
                .is_none_or(|o| o.status.is_none())
        );
        assert_eq!(store.sequences.len(), MAX_RETAINED);
        // After high-water eviction, sequence 1 may be admitted again. Only the
        // new independent status is established, never the forgotten old state.
        store
            .report(&status(1, "idle"), now + Duration::from_secs(1))
            .unwrap();
        assert_eq!(
            store.observations.get(&original).unwrap().status.unwrap().0,
            "idle"
        );

        let mut store = NativeStore::default();
        store.report(&event(1, "session.deleted"), now).unwrap();
        for n in 1..=MAX_RETAINED {
            let mut p = event(n as u64 + 1, "session.deleted");
            p["session_id"] = json!(format!("other-{n}"));
            store
                .report(&p, now + Duration::from_millis(n as u64))
                .unwrap();
        }
        assert!(!store.tombstones.contains_key(&original));
        let p = status(MAX_RETAINED as u64 + 2, "busy");
        store.report(&p, now + Duration::from_secs(1)).unwrap();
        assert_eq!(
            store.observations.get(&original).unwrap().status.unwrap().0,
            "working"
        );
    }
    #[test]
    fn decisions_sequence_expiry_restart_and_closed_projection() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        let mut busy = event(1, "session.status");
        busy["status"] = json!("busy");
        assert_eq!(
            store.report(&busy, now).unwrap(),
            json!({"type":"accepted","delivery":"volatile"})
        );
        assert_eq!(
            view(&store, now)["observations"][0]["reported_state"],
            "working"
        );
        for (sequence, kind, id) in [
            (2, "permission.asked", "a"),
            (3, "question.asked", "a"),
            (4, "permission.replied", "a"),
        ] {
            let mut p = event(sequence, kind);
            p["request_id"] = json!(id);
            store.report(&p, now).unwrap();
        }
        let mut idle = event(5, "session.status");
        idle["status"] = json!("idle");
        store.report(&idle, now).unwrap();
        assert_eq!(
            view(&store, now)["observations"][0]["reported_state"],
            "blocked"
        );
        for sequence in [1, 4, 5] {
            idle["sequence"] = json!(sequence);
            assert!(store.report(&idle, now + Duration::from_secs(29)).is_err());
        }
        let expired = view(&store, now + Duration::from_secs(30));
        assert_eq!(
            view(&store, now + Duration::from_secs(29))["observations"][0]["freshness"],
            "fresh"
        );
        assert_eq!(expired["observations"][0]["freshness"], "expired");
        assert_eq!(expired["observations"][0]["reported_state"], "unknown");
        let mut reply = event(6, "question.rejected");
        reply["request_id"] = json!("a");
        store.report(&reply, now).unwrap();
        assert_eq!(
            view(&store, now)["observations"][0]["reported_state"],
            "idle"
        );
        store.report(&event(7, "session.deleted"), now).unwrap();
        assert_eq!(
            view(&store, now)["observations"][0]["reported_state"],
            "ended"
        );
        assert!(!view(&store, now).to_string().contains("PRIVATE-SID"));
        assert_eq!(
            view(&NativeStore::default(), now)["observations"],
            json!([])
        );
    }
    #[test]
    fn envelope_is_closed_and_reconstructed() {
        for name in [
            "session.created",
            "session.updated",
            "session.deleted",
            "session.status",
            "permission.asked",
            "permission.replied",
            "permission.cancelled",
            "question.asked",
            "question.replied",
            "question.rejected",
            "question.cancelled",
        ] {
            let mut p = event(1, name);
            p["contract"] = json!(ENVELOPE);
            if name == "session.status" {
                p["status"] = json!("retry");
            }
            if name.starts_with("permission.") || name.starts_with("question.") {
                p["request_id"] = json!("request");
            }
            let result = params(&p, true).unwrap();
            assert!(result.get("contract").is_none());
            for field in ["prompt", "path", "parent_id", "payload", "contract2"] {
                let mut bad = p.clone();
                bad[field] = json!("PRIVATE");
                assert!(params(&bad, true).is_err());
            }
        }
        let mut p = event(1, "session.created");
        for invalid in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!(9_007_199_254_740_992u64),
            json!("1"),
        ] {
            p["sequence"] = invalid;
            assert!(params(&p, false).is_err());
        }
        p["sequence"] = json!(1);
        p["instance_id"] = json!("12345678-1234-1234-1234-123456789abC");
        assert!(params(&p, false).is_err());
    }
    #[test]
    fn pending_capacity_and_per_instance_sequence_are_bounded() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        store.report(&event(1, "session.created"), now).unwrap();
        let mut second = event(2, "session.created");
        second["session_id"] = json!("second");
        store.report(&second, now).unwrap();
        assert!(store.report(&event(2, "session.created"), now).is_err());
        for n in 3..131 {
            let mut p = event(n, "question.asked");
            p["request_id"] = json!(format!("r{n}"));
            store.report(&p, now).unwrap();
        }
        let mut p = event(131, "question.asked");
        p["request_id"] = json!("overflow");
        assert!(store.report(&p, now).is_err());
    }

    #[test]
    fn instance_and_session_tuples_remain_separate() {
        let now = Instant::now();
        let mut store = NativeStore::default();
        store.report(&event(1, "session.created"), now).unwrap();
        let mut second = event(1, "session.deleted");
        second["instance_id"] = json!("12345678-1234-1234-1234-123456789abd");
        store.report(&second, now).unwrap();
        second["sequence"] = json!(2);
        second["session_id"] = json!("another-session");
        store.report(&second, now).unwrap();
        let projection = view(&store, now);
        let rows = projection["observations"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter()
                .map(|r| r["observation_id"].as_str().unwrap())
                .collect::<BTreeSet<_>>()
                .len(),
            3
        );
    }

    #[test]
    fn malformed_capture_never_reaches_private_client() {
        let mut payload = event(1, "session.created");
        payload["contract"] = json!(ENVELOPE);
        for field in ["prompt", "cwd", "parent_id", "tool", "payload"] {
            let mut invalid = payload.clone();
            invalid[field] = json!("PRIVATE");
            assert_eq!(
                native_report_payload("not-a-socket", invalid.to_string().as_bytes(), true)
                    .err()
                    .unwrap()
                    .code(),
                "agent_invalid_schema"
            );
        }
        let duplicate = br#"{"contract":"djinn.native-events.v1","instance_id":"12345678-1234-1234-1234-123456789abc","session_id":"one","session_id":"two","sequence":1,"event":"session.created"}"#;
        assert!(native_report_payload("not-a-socket", duplicate, true).is_err());
        assert_eq!(
            native_report_payload("not-a-socket", &vec![b' '; 8193], true)
                .err()
                .unwrap()
                .code(),
            "agent_payload_limit"
        );
    }
    #[test]
    fn legacy_receipts_are_not_native_or_fresh_after_restart() {
        let store = NativeStore::default();
        for provider in ["claude", "codex", "copilot", "pi", "opencode"] {
            for age in [Value::Null, json!(0), json!(30_000)] {
                let legacy = json!({"server_epoch":"epoch","records":[{"record_id":"a".repeat(64),"provider":provider,"reported_state":"working","age_ms":age}]});
                let projection = store.projection(legacy, Instant::now());
                let row = &projection["observations"][0];
                assert_eq!(row["origin"], "v1_receipt");
                assert_eq!(
                    row["reported_state"],
                    if provider == "claude" && age == json!(0) {
                        "working"
                    } else {
                        "unknown"
                    }
                );
                assert_eq!(row["reference_trust"], "caller_supplied_unverified");
            }
        }
    }
}
