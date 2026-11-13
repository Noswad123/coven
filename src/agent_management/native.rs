//! Closed Djinn capture boundary and volatile, caller-unverified evidence.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

pub const RPC: &str = "coven.native-events.v1";
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
    status: Option<(&'static str, Instant)>,
    pending: BTreeMap<(String, String), Instant>,
    ended: Option<Instant>,
    first_seen: Instant,
}

fn fresh(at: Instant, now: Instant) -> bool {
    now.saturating_duration_since(at) < EVIDENCE_TTL
}

impl Observation {
    fn fact(&self, now: Instant) -> Option<(&'static str, Instant)> {
        if let Some(at) = self.ended {
            return Some(("ended", at));
        }
        if let Some(at) = self.pending.values().filter(|at| fresh(**at, now)).min() {
            return Some(("blocked", *at));
        }
        if let Some((state, at)) = self.status {
            if fresh(at, now) {
                return Some((state, at));
            }
        }
        self.status
            .map(|(_, at)| at)
            .into_iter()
            .chain(self.pending.values().copied())
            .max()
            .map(|at| ("unknown", at))
    }

    fn active(&self, now: Instant) -> bool {
        self.ended.is_none() && self.fact(now).is_some_and(|(_, at)| fresh(at, now))
    }
}

#[derive(Default)]
pub(super) struct NativeStore {
    sequences: BTreeMap<String, (u64, Instant)>,
    tombstones: BTreeMap<(String, String), Instant>,
    observations: BTreeMap<(String, String), Observation>,
    retired: u64,
}

impl NativeStore {
    fn invalidate(&mut self, instance: &str) {
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
        self.retire(now);
        let instance = text(&p, "instance_id")?;
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
                            b"native\0".as_slice(),
                            key.0.as_bytes(),
                            b"\0",
                            key.1.as_bytes()
                        ]
                        .concat()
                    )
                ),
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
                observation.pending.retain(|_, at| fresh(*at, now));
                let request = request.unwrap();
                require(
                    observation.pending.len() < 128 || observation.pending.contains_key(&request),
                    "agent_capacity",
                )?;
                observation.pending.entry(request).or_insert(now);
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
                json!("djinn"),
                "djinn_native_event",
                "activity",
                fact.map_or("unknown", |(state, _)| state),
                age,
            );
            if age.is_none() {
                native["freshness"] = json!("unknown");
            }
            observations.push(native);
        }
        observations.sort_by_key(|o| {
            (
                o["freshness"] != "fresh",
                o["origin"] != "djinn_native_event",
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
