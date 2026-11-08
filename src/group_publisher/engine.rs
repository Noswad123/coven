use super::{
    Request,
    live::{Horizon, LiveClock},
    model::{Cycle, Phase, Reservation, Summary},
};
use crate::observation_socket::{Cancel, Delivery, Failure, ReadBudget, WireResult};
use crate::observation_time::Clock;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

pub(super) const BUDGET: usize = 1_048_576;

/// All filesystem/network effects are behind this boundary; no workspace lock is
/// held by the engine. Only reserve may take the workspace mutation lock.
pub(super) trait Ops {
    fn group(&self, request: &Request) -> WireResult<(Value, Value)>;
    fn snapshot(
        &self,
        request: &Request,
        deadline: Instant,
        budget: &mut ReadBudget,
        process_only: bool,
        cancel: &dyn Cancel,
    ) -> WireResult<Value>;
    // Keep both identity pins and the shared read/deadline/cancel fences explicit.
    #[allow(clippy::too_many_arguments)]
    fn process(
        &self,
        request: &Request,
        pane: &str,
        terminal: &str,
        endpoint: &Value,
        deadline: Instant,
        budget: &mut ReadBudget,
        cancel: &dyn Cancel,
    ) -> WireResult<Value>;
    fn stable(&self, binding: &Value) -> WireResult<Value>;
    fn status(&self, request: &Request, timeout: f64) -> WireResult<Value>;
    fn accepted(&self, report: &Value, attachment: &Value) -> WireResult<Value>;
    fn reserve(
        &self,
        request: &Request,
        revision: &Value,
        endpoint: &Value,
        epoch: &str,
        terminals: &[String],
        first: bool,
    ) -> WireResult<Value>;
    fn annotate(
        &self,
        request: &Request,
        endpoint: &Value,
        params: &mut Value,
        deadline: Instant,
        cancel: &dyn Cancel,
        prepare: &mut dyn FnMut(&mut Value) -> WireResult<()>,
    ) -> WireResult<()>;
    fn sleep(&self, duration: Duration, cancel: &dyn Cancel);
}

pub(super) fn check(clock: &dyn Clock, deadline: Instant, cancel: &dyn Cancel) -> WireResult<()> {
    if cancel.interrupted() {
        return Err(Failure::new("interrupted"));
    }
    if clock.monotonic() >= deadline {
        return Err(Failure::new("timeout"));
    }
    Ok(())
}

fn cancelled(cancel: &dyn Cancel) -> WireResult<()> {
    if cancel.interrupted() {
        Err(Failure::new("interrupted"))
    } else {
        Ok(())
    }
}

pub(super) fn endpoint(group: &Value) -> WireResult<Value> {
    let members = group["members"]
        .as_array()
        .ok_or_else(|| Failure::new("group_state_error"))?;
    if !(1..=16).contains(&members.len()) {
        return Err(Failure::new("group_size"));
    }
    let endpoint = &members[0]["binding"]["presentation"]["endpoint"];
    if !endpoint.is_object() {
        return Err(Failure::new("group_state_error"));
    }
    let mut terminals = BTreeSet::new();
    for member in members {
        let presentation = &member["binding"]["presentation"];
        if &presentation["endpoint"] != endpoint {
            return Err(Failure::new("group_endpoint_mismatch"));
        }
        let terminal = presentation["terminal_id"]
            .as_str()
            .ok_or_else(|| Failure::new("group_state_error"))?;
        if !terminals.insert(terminal) {
            return Err(Failure::new("duplicate_terminal"));
        }
    }
    Ok(endpoint.clone())
}

fn uncertain(code: &str) -> Failure {
    Failure {
        code: code.into(),
        delivery: Delivery::Uncertain,
        interrupted: false,
    }
}

pub(super) fn run(
    request: &Request,
    ops: &dyn Ops,
    clock: &dyn Clock,
    cancel: &dyn Cancel,
) -> Value {
    let mut summary = Summary::new(request.live);
    let mut cycles = Vec::new();
    let mut live_clock = LiveClock::default();
    let mut horizons: BTreeMap<String, Horizon> = BTreeMap::new();
    let result = (|| -> WireResult<()> {
        cancelled(cancel)?;
        let (revision, frozen_group) = ops.group(request)?;
        cancelled(cancel)?;
        let frozen_endpoint = endpoint(&frozen_group)?;
        if request.live {
            live_clock.check(clock)?;
        }
        let end = clock.monotonic() + Duration::from_secs(request.duration);
        let mut epoch: Option<String> = None;
        let mut first = true;
        while clock.monotonic() < end {
            let started = clock.monotonic();
            let deadline = end.min(started + Duration::from_secs(3));
            check(clock, deadline, cancel)?;
            if request.live {
                live_clock.check(clock)?;
            }
            let (current_revision, group) = ops.group(request)?;
            check(clock, deadline, cancel)?;
            // The token covers all groups, not just the selected group.
            if current_revision != revision || group != frozen_group {
                return Err(Failure::new("group_content_changed"));
            }
            if endpoint(&group)? != frozen_endpoint {
                return Err(Failure::new("group_endpoint_mismatch"));
            }
            let members = group["members"]
                .as_array()
                .ok_or_else(|| Failure::new("group_state_error"))?;
            let all_process = members
                .iter()
                .all(|m| m["binding"]["evidence"]["provenance"] == "kitsune.pane.process_info");
            let mut budget = ReadBudget::new(BUDGET);
            let fresh = ops.snapshot(request, deadline, &mut budget, all_process, cancel)?;
            check(clock, deadline, cancel)?;
            if fresh["endpoint"] != frozen_endpoint {
                return Err(Failure::new("endpoint_replaced"));
            }
            let current_epoch = fresh["epoch"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| Failure::new("annotations_unsupported"))?;
            if request.live && fresh["valid_until_unix_ms"] != Value::Bool(true) {
                return Err(Failure::new("annotation_deadline_unsupported"));
            }
            if !fresh["sources"]
                .as_array()
                .is_some_and(|s| s.iter().any(|s| s == &request.source))
            {
                return Err(Failure::new("source_disabled"));
            }
            if epoch.as_deref().is_some_and(|e| e != current_epoch) {
                return Err(Failure::new("annotation_epoch_changed"));
            }
            epoch = Some(current_epoch.to_owned());
            let mut plan = Vec::with_capacity(members.len());
            let mut terminals = Vec::with_capacity(members.len());
            for member in members {
                check(clock, deadline, cancel)?;
                let binding = &member["binding"];
                let evidence = &binding["evidence"];
                let terminal = binding["presentation"]["terminal_id"]
                    .as_str()
                    .ok_or_else(|| Failure::new("group_state_error"))?;
                let process = evidence["provenance"] == "kitsune.pane.process_info";
                let inventory = fresh[if process { "panes" } else { "agents" }]
                    .as_array()
                    .ok_or_else(|| Failure::new("schema_error"))?;
                let mut found = inventory.iter().filter(|v| v["terminal_id"] == terminal);
                let pane = found.next().ok_or_else(|| {
                    Failure::new(if process {
                        "bound_terminal_missing"
                    } else {
                        "bound_agent_missing"
                    })
                })?;
                if found.next().is_some() {
                    return Err(Failure::new("schema_error"));
                }
                let pane_id = pane["pane_id"]
                    .as_str()
                    .ok_or_else(|| Failure::new("schema_error"))?;
                if process {
                    let observed = ops.process(
                        request,
                        pane_id,
                        terminal,
                        &frozen_endpoint,
                        deadline,
                        &mut budget,
                        cancel,
                    )?;
                    check(clock, deadline, cancel)?;
                    if ops.stable(&observed)? != ops.stable(binding)? {
                        return Err(Failure::new("rebind_required"));
                    }
                } else if pane["agent"] != evidence["agent_kind_hint"]
                    || pane["agent_session"] != evidence["native_reference"]
                {
                    return Err(Failure::new("rebind_required"));
                }
                let name = group["name"]
                    .as_str()
                    .ok_or_else(|| Failure::new("group_state_error"))?;
                let role = member["role"]
                    .as_str()
                    .ok_or_else(|| Failure::new("group_state_error"))?;
                plan.push(json!({"pane_id": pane_id, "expected_terminal_id": terminal,
                    "annotation_epoch": current_epoch, "source": request.source,
                    "text": format!("{name} \u{b7} {role}"), "ttl_ms": 6000}));
                terminals.push(terminal.to_owned());
            }
            let leases = if request.live {
                super::live::evidence(
                    request,
                    ops,
                    clock,
                    cancel,
                    &mut live_clock,
                    &revision,
                    &group,
                    deadline,
                    &mut summary,
                    &mut horizons,
                )?
            } else {
                vec![None; plan.len()]
            };
            let roles: Vec<String> = plan
                .iter()
                .map(|p| p["text"].as_str().expect("constructed text").to_owned())
                .collect();
            if request.live {
                for (index, params) in plan.iter_mut().enumerate() {
                    if let Some(id) = &leases[index] {
                        params["text"] =
                            json!(format!("{} \u{b7} legacy runner busy", roles[index]));
                        params["ttl_ms"] = json!(3000);
                        super::live::prepare(
                            params,
                            &roles[index],
                            horizons.get_mut(id),
                            clock,
                            &mut live_clock,
                            deadline,
                            cancel,
                            &mut summary,
                        )?;
                    }
                }
            }
            check(clock, deadline, cancel)?;
            cycles.push(Cycle {
                reservation: Reservation::OutcomeUnknown,
                targets: vec![Phase::Unsent; plan.len()],
            });
            let cycle = cycles.last_mut().expect("recorded attempt");
            // From this point even an error may have burned every target's counter.
            cancelled(cancel)?;
            let reservation = ops.reserve(
                request,
                &revision,
                &frozen_endpoint,
                current_epoch,
                &terminals,
                first,
            );
            cancelled(cancel)?;
            let reserved = reservation.map_err(|failure| {
                if failure.interrupted {
                    failure
                } else {
                    uncertain("reservation_failed")
                }
            })?;
            if reserved["delivery"]["outcome"] != "durably_committed" {
                cycle.reservation = Reservation::DurabilityUncertain;
                return Err(uncertain("reservation_durability_uncertain"));
            }
            let numbers = reserved["reservations"]
                .as_object()
                .ok_or_else(|| uncertain("invalid_reservation"))?;
            if reserved["revision"] != revision
                || numbers.len() != terminals.len()
                || terminals.iter().any(|t| {
                    numbers
                        .get(t)
                        .and_then(Value::as_u64)
                        .is_none_or(|n| n == 0)
                })
            {
                return Err(uncertain("invalid_reservation"));
            }
            cancelled(cancel)?;
            cycle.reservation = Reservation::Durable;
            first = false;
            for (index, params) in plan.iter_mut().enumerate() {
                check(clock, deadline, cancel)?;
                params["revision"] = numbers[&terminals[index]].clone();
                let mut prepare = |params: &mut Value| -> WireResult<()> {
                    check(clock, deadline, cancel)?;
                    if request.live {
                        super::live::prepare(
                            params,
                            &roles[index],
                            leases[index].as_ref().and_then(|id| horizons.get_mut(id)),
                            clock,
                            &mut live_clock,
                            deadline,
                            cancel,
                            &mut summary,
                        )?;
                    }
                    check(clock, deadline, cancel)
                };
                prepare(params)?;
                cancelled(cancel)?;
                cycle.targets[index] = Phase::InFlight;
                let sent = ops.annotate(
                    request,
                    &frozen_endpoint,
                    params,
                    deadline,
                    cancel,
                    &mut prepare,
                );
                match sent {
                    Ok(()) => {
                        // An interrupt in the return-to-record gap cannot assert an ack.
                        cancelled(cancel)?;
                        cycle.targets[index] = Phase::Acknowledged;
                        cancelled(cancel)?;
                    }
                    Err(failure) => {
                        cycle.targets[index] = match failure.delivery {
                            Delivery::Unsent => Phase::Unsent,
                            Delivery::Rejected => Phase::Rejected,
                            Delivery::Uncertain => Phase::Uncertain,
                        };
                        if cancel.interrupted() && !failure.interrupted {
                            return Err(Failure::new("interrupted"));
                        }
                        return Err(failure);
                    }
                }
                if request.live {
                    live_clock.check(clock)?;
                }
            }
            cancelled(cancel)?;
            let now = clock.monotonic();
            if request.live {
                live_clock.check(clock)?;
            }
            let elapsed = now
                .checked_duration_since(started)
                .ok_or_else(|| Failure::new("live_clock_regressed"))?;
            let next = started + Duration::from_secs((elapsed.as_secs() / 2 + 1) * 2);
            if now < end {
                ops.sleep(end.min(next).saturating_duration_since(now), cancel);
                cancelled(cancel)?;
                if request.live {
                    live_clock.check(clock)?;
                }
            }
        }
        cancelled(cancel)
    })();
    // Storage/status can fail concurrently with a signal, before their return
    // checks run. Cancellation still wins the outcome without rewriting phases.
    let result = if cancel.interrupted() {
        Err(Failure::new("interrupted"))
    } else {
        result
    };
    let mut report = summary.finish(result.as_ref().err(), &cycles);
    if cancel.interrupted() {
        report["outcome"] = json!("interrupted");
        let codes = report["diagnostics"].as_array_mut().expect("summary array");
        if !codes.iter().any(|code| code == "interrupted") {
            codes.push(json!("interrupted"));
        }
    }
    report
}
