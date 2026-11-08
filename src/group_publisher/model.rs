use crate::observation_socket::{Delivery, Failure};
use serde_json::{Value, json};
use std::path::PathBuf;

pub struct Request {
    pub workspace: PathBuf,
    pub group: String,
    pub socket: String,
    pub source: String,
    pub duration: u64,
    pub live: bool,
}

pub(super) fn validate(request: &Request) -> crate::Result<()> {
    crate::observation_socket::validate_path(&request.socket)?;
    let group = request.group.as_bytes();
    crate::error::require(
        (1..=64).contains(&group.len())
            && (group[0].is_ascii_lowercase() || group[0].is_ascii_digit())
            && group
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-')),
        "invalid_group_selector",
    )?;
    crate::error::require(
        (1..=64).contains(&request.source.len())
            && request
                .source
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-')),
        "invalid_source",
    )?;
    crate::error::require((1..=3600).contains(&request.duration), "invalid_duration")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Unsent,
    InFlight,
    Acknowledged,
    Rejected,
    Uncertain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reservation {
    OutcomeUnknown,
    DurabilityUncertain,
    Durable,
}

/// Retain only phases, never per-cycle params, bindings, or live-source histories.
pub(super) struct Cycle {
    pub reservation: Reservation,
    pub targets: Vec<Phase>,
}

pub(super) struct Summary {
    value: Value,
}

impl Summary {
    pub fn new(live: bool) -> Self {
        let mut value = json!({
            "outcome": "completed", "diagnostics": [],
            "authorization": "annotation_publication_only",
            "native_execution_authorization": "none", "dispatchable": false,
            "ttl_ms": 6000,
            "limitations": [
                "Labels expire by TTL; stopping does not cancel admitted writes.",
                "Filesystem fsync is not wall-clock bounded."
            ]
        });
        if live {
            value["publication_mode"] = json!("live");
            value["ttl_ms"] = Value::Null;
            value["static_ttl_ms"] = json!(6000);
            value["live_max_age_ms"] = json!(3000);
            value["limitations"].as_array_mut().expect("summary array").extend([
                "Busy is only pinned legacy runner ownership, not idle, health, progress or completion.",
                "Attachments are declarations, not verified terminal/UI-session equivalence or distinct workers.",
                "Expiry assumes compatible source/server Unix clocks, no disruptive wall changes and normal monotonic progression; unknown skew, rollback, suspend and display lag are not bounded.",
                "A composed live frame expires as a whole; later refresh may restore role-only text. Clear/expiry never reveals a lower-priority source."
            ].map(|text| json!(text)));
        }
        Self { value }
    }

    pub fn diagnostic(&mut self, code: &str) {
        let codes = self.value["diagnostics"]
            .as_array_mut()
            .expect("summary array");
        if !codes.iter().any(|v| v == code) {
            codes.push(json!(code));
        }
    }

    pub fn finish(mut self, failure: Option<&Failure>, cycles: &[Cycle]) -> Value {
        if let Some(failure) = failure {
            self.value["outcome"] = json!(if failure.interrupted {
                "interrupted"
            } else if failure.delivery == Delivery::Uncertain {
                "uncertain"
            } else {
                "failed"
            });
            self.diagnostic(&failure.code);
        }
        let mut counts = [0u64; 5];
        let mut durable = 0u64;
        let mut reserved = 0u64;
        let mut completed = 0u64;
        for cycle in cycles {
            for phase in &cycle.targets {
                counts[match phase {
                    Phase::Unsent => 0,
                    Phase::InFlight => 1,
                    Phase::Acknowledged => 2,
                    Phase::Rejected => 3,
                    Phase::Uncertain => 4,
                }] += 1;
            }
            if cycle.reservation == Reservation::Durable {
                durable += 1;
                reserved += cycle.targets.len() as u64;
                completed += u64::from(cycle.targets.iter().all(|p| *p == Phase::Acknowledged));
            }
        }
        let attempted: u64 = counts.iter().sum();
        for (key, count) in [
            ("cycles", durable),
            ("completed_cycles", completed),
            ("reservation_attempts", cycles.len() as u64),
            ("reservation_attempted_targets", attempted),
            ("reserved_targets", reserved),
            ("reservation_uncertain", attempted - reserved),
            ("acknowledged", counts[2]),
            ("rejected", counts[3]),
            ("unsent", counts[0]),
            ("uncertain", counts[1] + counts[4]),
        ] {
            self.value[key] = json!(count);
        }
        self.value
    }
}
