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
pub const ATTENTION_VIEW: &str = "coven.attention.v1";
pub const VIEW: &str = "coven.normal-observations.v1";
const ENVELOPE: &str = "djinn.native-events.v1";
const EVIDENCE_TTL: Duration = Duration::from_secs(30);
const RETENTION_TTL: Duration = Duration::from_secs(600);
const MAX_RETAINED: usize = 512;

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
    navigation_generation: u64,
    sequences: BTreeMap<String, (u64, Instant)>,
    tombstones: BTreeMap<(String, String), Instant>,
    observations: BTreeMap<(String, String), Observation>,
    retired: u64,
    occurrence: u64,
}

impl NativeStore {
    pub(super) fn navigation_fact(&self, id: &str, now: Instant) -> Option<u64> {
        self.observations
            .values()
            .find(|o| o.id == id && o.active(now))
            .map(|_| self.navigation_generation)
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
            self.observations.remove(key);
            self.retired = self.retired.saturating_add(1);
        }
    }

    fn retire(&mut self, now: Instant) {
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
        if !self.observations.contains_key(&key) && self.observations.len() == MAX_RECORDS {
            let retired = self
                .observations
                .iter()
                .filter(|(_, o)| !o.active(now))
                .min_by_key(|(_, o)| o.first_seen)
                .map(|(key, _)| key.clone());
            if let Some(retired) = retired {
                self.observations.remove(&retired);
                self.retired = self.retired.saturating_add(1);
            } else if event == "session.deleted" {
                // The terminal safety fact is retained even if all display slots
                // contain live activity. There is no extra hidden active slot.
                self.retired = self.retired.saturating_add(1);
                return Ok(json!({"type":"accepted","delivery":"volatile"}));
            } else {
                return Err(Error("agent_capacity"));
            }
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
            });
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
        let mut observations = Vec::new();
        for record in legacy["records"].as_array().into_iter().flatten() {
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
                    "djinn_native_event"
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
            observations.push(native);
        }
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
        let limit = if observations.len() > MAX_RECORDS || self.retired > 0 {
            MAX_RECORDS - 1
        } else {
            MAX_RECORDS
        };
        let omitted = observations.len().saturating_sub(limit);
        observations.truncate(limit);
        json!({"contract":VIEW,"server_epoch":legacy["server_epoch"],"authorization":"none","observations":observations,
            "omitted_observations":omitted,"retired_native_observations":self.retired})
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
        ("djinn", "djinn_native_event", None)
            | (
                "opencode",
                "provider_native_event",
                Some("opencode-root.v1")
            )
            | ("pi", "provider_native_event", Some("pi-context.v1"))
            | (
                "claude" | "codex" | "copilot" | "pi" | "opencode",
                "v1_receipt",
                None
            )
    )
}

fn validate_observation(row: &Value) -> Result<()> {
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
                o.len() == 13 + if native { 3 } else { 0 } + usize::from(profile.is_some())
                    && o.keys().all(|key| match key.as_str() {
                        "observation_id" | "provider" | "origin" | "capability"
                        | "reported_state" | "age_ms" | "freshness" | "reference_trust"
                        | "root_eligibility" | "native_identity" | "job_completion" | "binding"
                        | "attribution" => true,
                        "fresh_pending_count"
                        | "unacknowledged_pending_count"
                        | "attention_revision" => native,
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
                == if origin == "djinn_native_event" || profile == Some("opencode-root.v1") {
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
        "root_eligibility":if origin == "djinn_native_event" {"producer_checked"} else {"unestablished"},
        "native_identity":"unestablished","job_completion":"unestablished","binding":"none","attribution":"Coven"})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::time::Duration;

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
