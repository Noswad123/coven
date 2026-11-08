use super::{
    Request,
    engine::{Ops, check},
    model::Summary,
};
use crate::observation_socket::{Cancel, Failure, WireResult};
use crate::observation_time::Clock;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

const SAFE_JS_MAX: f64 = 9_007_199_254_740_991.0;

#[derive(Default)]
pub(super) struct LiveClock {
    previous: Option<(Instant, f64)>,
}

impl LiveClock {
    pub fn check(&mut self, clock: &dyn Clock) -> WireResult<(Instant, f64)> {
        let mono = clock.monotonic();
        let wall = clock.wall_ms();
        if !wall.is_finite() || !(0.0..=SAFE_JS_MAX).contains(&wall) {
            return Err(Failure::new("live_clock_unavailable"));
        }
        if self.previous.is_some_and(|(m, w)| mono < m || wall < w) {
            return Err(Failure::new("live_clock_regressed"));
        }
        self.previous = Some((mono, wall));
        Ok((mono, wall))
    }
}

pub(super) struct Horizon {
    source: Value,
    activity: String,
    wall: f64,
    mono: Instant,
    sample: f64,
}

fn retain(
    horizons: &mut BTreeMap<String, Horizon>,
    id: &str,
    source: Value,
    activity: &str,
    wall: f64,
    mono: Instant,
    sample: f64,
) -> bool {
    if let Some(previous) = horizons.get_mut(id) {
        if source == previous.source {
            previous.wall = previous.wall.min(wall);
            previous.mono = previous.mono.min(mono);
            if activity != previous.activity {
                return false;
            }
        } else {
            if ["contract", "runtime_id", "instance_id", "session_id"]
                .iter()
                .any(|k| source[k] != previous.source[k])
                || source["sequence"].as_u64() <= previous.source["sequence"].as_u64()
                || source["sampled_at_ms"].as_u64() <= previous.source["sampled_at_ms"].as_u64()
            {
                return false;
            }
            *previous = Horizon {
                source,
                activity: activity.into(),
                wall,
                mono,
                sample,
            };
        }
    } else {
        horizons.insert(
            id.into(),
            Horizon {
                source,
                activity: activity.into(),
                wall,
                mono,
                sample,
            },
        );
    }
    activity == "busy"
}

// Validate the frozen projection and invocation-local horizons in one scoped pass.
#[allow(clippy::too_many_arguments)]
pub(super) fn evidence(
    request: &Request,
    ops: &dyn Ops,
    clock: &dyn Clock,
    cancel: &dyn Cancel,
    live_clock: &mut LiveClock,
    revision: &Value,
    group: &Value,
    deadline: Instant,
    summary: &mut Summary,
    horizons: &mut BTreeMap<String, Horizon>,
) -> WireResult<Vec<Option<String>>> {
    let (mono, wall) = live_clock.check(clock)?;
    check(clock, deadline, cancel)?;
    let timeout = deadline
        .saturating_duration_since(clock.monotonic())
        .as_secs_f64()
        .min(1.0);
    if timeout <= 0.0 {
        return Err(Failure::new("timeout"));
    }
    let report = ops.status(request, timeout).map_err(|failure| {
        if failure.interrupted {
            failure
        } else {
            Failure::new("live_status_unavailable")
        }
    })?;
    check(clock, deadline, cancel)?;
    live_clock.check(clock)?;
    let invalid = || Failure::new("live_status_invalid");
    let identity =
        json!({"group_id": group["group_id"], "key": group["key"], "name": group["name"]});
    if !report.is_object()
        || report["schema_version"].as_u64() != Some(1)
        || report["contract"] != "coven.group.status.v1"
        || report["assessment"] != "observed"
        || &report["revision"] != revision
        || report["group"] != identity
        || report["authorization"] != "none"
        || report["dispatchable"] != Value::Bool(false)
        || report["native_lifecycle"] != "unestablished"
        || report["job_completion"] != "unestablished"
    {
        return Err(invalid());
    }
    let members = group["members"].as_array().ok_or_else(invalid)?;
    let rows = report["participants"].as_array().ok_or_else(invalid)?;
    if rows.len() != members.len() {
        return Err(invalid());
    }
    let mut by_id = BTreeMap::new();
    for row in rows {
        let id = row["participant_id"].as_str().ok_or_else(invalid)?;
        if !row.is_object() || by_id.insert(id, row).is_some() {
            return Err(invalid());
        }
    }
    let ids: BTreeSet<_> = members
        .iter()
        .filter_map(|m| m["participant_id"].as_str())
        .collect();
    if ids.len() != members.len() || by_id.keys().copied().collect::<BTreeSet<_>>() != ids {
        return Err(invalid());
    }
    for member in members {
        let row = by_id[member["participant_id"].as_str().ok_or_else(invalid)?];
        let attachment = &member["live_attachment"];
        if row["key"] != member["key"]
            || row["role"] != member["role"]
            || row["live_attachment"] != *attachment
            || row["association_basis"]
                != if attachment.is_null() {
                    "unattached"
                } else {
                    "explicit_declaration"
                }
        {
            return Err(invalid());
        }
    }
    let mut leases = Vec::with_capacity(members.len());
    for member in members {
        check(clock, deadline, cancel)?;
        let id = member["participant_id"].as_str().ok_or_else(invalid)?;
        let row = by_id[id];
        let attachment = &member["live_attachment"];
        let mut retained = false;
        if !attachment.is_null() && row["assessment"] == "observed" {
            let mut envelope = row.clone();
            envelope["adapter"] = json!("djinn_live");
            envelope["schema_version"] = json!(1);
            envelope["adapter_contract"] = json!("coven.djinn.live-observation.v1");
            envelope["selection"] = json!({"runtime_id": attachment["runtime_id"],
                "instance_id": attachment["instance_id"], "session_id": attachment["session_id"]});
            // Invalid individual evidence is unknown; never use cached positive activity.
            if let Ok(source) = ops.accepted(&envelope, attachment) {
                if let (Some(remaining), Some(sample), Some(activity)) = (
                    row["expires_in_ms"].as_u64(),
                    source["sampled_at_ms"].as_u64(),
                    row["activity"].as_str(),
                ) {
                    if (1..=3000).contains(&remaining)
                        && sample <= SAFE_JS_MAX as u64
                        && matches!(activity, "busy" | "unknown")
                    {
                        let sample = sample as f64;
                        let ceiling = (sample + 3000.0)
                            .min((wall + remaining as f64).floor())
                            .min(SAFE_JS_MAX);
                        retained = retain(
                            horizons,
                            id,
                            source,
                            activity,
                            ceiling,
                            mono + Duration::from_millis(remaining),
                            sample,
                        );
                    }
                }
            }
        }
        check(clock, deadline, cancel)?;
        if !retained {
            summary.diagnostic("live_source_unknown");
        }
        leases.push(if retained { Some(id.to_owned()) } else { None });
    }
    Ok(leases)
}

// Explicit fences prevent a callback from accidentally renewing a frame's lease.
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    params: &mut Value,
    role: &str,
    lease: Option<&mut Horizon>,
    clock: &dyn Clock,
    live_clock: &mut LiveClock,
    deadline: Instant,
    cancel: &dyn Cancel,
    summary: &mut Summary,
) -> WireResult<()> {
    let (mono, wall) = live_clock.check(clock)?;
    check(clock, deadline, cancel)?;
    let Some(lease) = lease else {
        return Ok(());
    };
    let remaining_mono = if mono <= lease.mono {
        lease.mono.duration_since(mono).as_secs_f64() * 1000.0
    } else {
        -mono.duration_since(lease.mono).as_secs_f64() * 1000.0
    };
    // Persist every tightening in the participant's current record across cycles.
    lease.wall = lease.wall.min((wall + remaining_mono).floor());
    let remaining = (lease.wall - wall).min(remaining_mono);
    if wall < lease.sample || remaining <= 0.0 {
        params["text"] = json!(role);
        params["ttl_ms"] = json!(6000);
        params
            .as_object_mut()
            .ok_or_else(|| Failure::new("schema_error"))?
            .remove("valid_until_unix_ms");
        summary.diagnostic("live_source_expired");
    } else if params["text"] != role {
        params["valid_until_unix_ms"] = json!(lease.wall as u64);
    }
    Ok(())
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
