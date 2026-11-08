//! Adapter to the shared authority, observation and transport owners.
use super::{Request, engine::Ops};
use crate::{
    groups, kitsune_observation,
    observation_socket::{
        self, Cancel, Failure, InterruptGuard, PublisherLease, ReadBudget, WireResult,
    },
};
use serde_json::Value;
use std::cell::RefCell;
use std::time::{Duration, Instant};

pub(super) struct Session {
    _lease: PublisherLease,
    pub interrupt: InterruptGuard,
    frozen: RefCell<Option<(Value, Value)>>,
    reservation_endpoint: crate::values::Value,
}

impl Session {
    pub fn open(request: &Request) -> WireResult<Self> {
        let interrupt =
            InterruptGuard::install().map_err(|_| Failure::new("publisher_lease_error"))?;
        // Keep the pre-lease content freeze for the engine's first read. It must
        // not silently adopt declarations changed while acquiring the lease.
        let frozen = read_group(request);
        if interrupt.interrupted() {
            return Err(Failure::new("interrupted"));
        }
        let frozen = frozen?;
        super::engine::endpoint(&frozen.1)?;
        if interrupt.interrupted() {
            return Err(Failure::new("interrupted"));
        }
        let lease = PublisherLease::acquire(&request.workspace);
        if interrupt.interrupted() {
            return Err(Failure::new("interrupted"));
        }
        let lease = lease.map_err(|error| {
            Failure::new(
                if matches!(
                    error.code(),
                    "lock_busy" | "publisher_busy" | "workspace_lock_busy"
                ) {
                    "publisher_busy"
                } else {
                    "publisher_lease_error"
                },
            )
        })?;
        Ok(Self {
            _lease: lease,
            interrupt,
            reservation_endpoint: frozen.2,
            frozen: RefCell::new(Some((frozen.0, frozen.1))),
        })
    }
}

fn read_group(request: &Request) -> WireResult<(Value, Value, crate::values::Value)> {
    let view =
        groups::read_groups(&request.workspace).map_err(|_| Failure::new("group_state_error"))?;
    let group = view
        .group(&request.group)
        .map_err(|_| Failure::new("group_not_found"))?;
    let members = group
        .as_object()
        .and_then(|group| group.get("members"))
        .and_then(crate::values::Value::as_array)
        .ok_or_else(|| Failure::new("group_state_error"))?;
    if !(1..=16).contains(&members.len()) {
        return Err(Failure::new("group_size"));
    }
    let mut common = None;
    for member in members {
        let endpoint = member
            .as_object()
            .and_then(|member| member.get("binding"))
            .and_then(crate::values::Value::as_object)
            .and_then(|binding| binding.get("presentation"))
            .and_then(crate::values::Value::as_object)
            .and_then(|presentation| presentation.get("endpoint"))
            .ok_or_else(|| Failure::new("group_state_error"))?;
        let exact = endpoint
            .canonical()
            .map_err(|_| Failure::new("group_state_error"))?;
        if let Some((_, previous)) = &common {
            if previous != &exact {
                return Err(Failure::new("group_endpoint_mismatch"));
            }
        } else {
            common = Some((endpoint, exact));
        }
    }
    let original_endpoint = common.ok_or_else(|| Failure::new("group_size"))?.0;
    let endpoint = original_endpoint
        .as_object()
        .ok_or_else(|| Failure::new("group_state_error"))?;
    // Historical authorities allow arbitrary nonnegative endpoint integers.
    // Compare the common endpoint before narrowing: an integer outside u64
    // cannot equal an actual OS identity. This is an ordinary stale binding,
    // not malformed authority or a new restriction on the group reader.
    for key in ["device", "inode"] {
        if !matches!(endpoint.get(key), Some(crate::values::Value::Number(
            crate::values::Number::Integer(number))) if number.as_str().parse::<u64>().is_ok())
        {
            return Err(Failure::new("endpoint_replaced"));
        }
    }
    Ok((
        Value::String(view.revision().into()),
        from_authority(group)?,
        original_endpoint.clone(),
    ))
}

// Only semantically bounded projections reach this boundary: selected groups
// after the exact endpoint checks above, stable bindings from those groups or
// observations, and reservations with schema-checked u64 counters. Never reparse
// arbitrary authority JSON through serde, which can round large integers to f64.
fn from_authority(value: &crate::values::Value) -> WireResult<Value> {
    crate::observation_wire::to_json(value).map_err(|_| Failure::new("group_state_error"))
}

fn to_authority(value: &Value) -> WireResult<crate::values::Value> {
    let bytes = serde_json::to_vec(value).map_err(|_| Failure::new("group_state_error"))?;
    crate::values::parse(&bytes, crate::values::Profile::Authority)
        .map_err(|_| Failure::new("group_state_error"))
}

fn reservation_endpoint(
    endpoint: &Value,
    original: &crate::values::Value,
) -> WireResult<crate::values::Value> {
    if from_authority(original)? != *endpoint {
        return Err(Failure::new("group_state_error"));
    }
    // Python retains the initial declaration object across the lease and rereads.
    // JSON projections are sufficient for identity checks, but lose its key order.
    Ok(original.clone())
}

impl Ops for Session {
    fn group(&self, request: &Request) -> WireResult<(Value, Value)> {
        if let Some(frozen) = self.frozen.borrow_mut().take() {
            return Ok(frozen);
        }
        read_group(request).map(|(revision, group, _)| (revision, group))
    }

    fn snapshot(
        &self,
        request: &Request,
        deadline: Instant,
        budget: &mut ReadBudget,
        process_only: bool,
        cancel: &dyn Cancel,
    ) -> WireResult<Value> {
        kitsune_observation::read_publication_snapshot(
            &request.socket,
            deadline,
            budget,
            process_only,
            cancel,
        )
    }

    fn process(
        &self,
        request: &Request,
        pane: &str,
        terminal: &str,
        endpoint: &Value,
        deadline: Instant,
        budget: &mut ReadBudget,
        cancel: &dyn Cancel,
    ) -> WireResult<Value> {
        kitsune_observation::binding_from_process(
            kitsune_observation::ProcessRequest {
                socket: &request.socket,
                pane,
                deadline,
                frozen_endpoint: Some(endpoint),
                expected_terminal: Some(terminal),
            },
            budget,
            cancel,
        )
    }

    fn stable(&self, binding: &Value) -> WireResult<Value> {
        from_authority(
            &groups::stable_binding(&to_authority(binding)?)
                .map_err(|_| Failure::new("process_binding_invalid"))?,
        )
    }

    fn status(&self, request: &Request, timeout: f64) -> WireResult<Value> {
        crate::group_live::status(&request.workspace, &request.group, None, timeout)
            .map_err(|_| Failure::new("live_status_unavailable"))
    }

    fn accepted(&self, report: &Value, attachment: &Value) -> WireResult<Value> {
        crate::group_live::accepted(report, attachment)
            .map_err(|_| Failure::new("invalid_live_report"))
    }

    fn reserve(
        &self,
        request: &Request,
        revision: &Value,
        endpoint: &Value,
        epoch: &str,
        terminals: &[String],
        first: bool,
    ) -> WireResult<Value> {
        let revision = revision
            .as_str()
            .ok_or_else(|| Failure::new("group_state_error"))?;
        let endpoint = reservation_endpoint(endpoint, &self.reservation_endpoint)?;
        from_authority(
            &groups::reserve_cycle(
                &request.workspace,
                groups::ReserveRequest {
                    group: &request.group,
                    expected_revision: revision,
                    endpoint: &endpoint,
                    source: &request.source,
                    epoch,
                    terminal_ids: terminals,
                    allow_new_epoch: first,
                },
            )
            .map_err(|_| Failure::new("reservation_failed"))?,
        )
    }

    fn annotate(
        &self,
        request: &Request,
        endpoint: &Value,
        params: &mut Value,
        deadline: Instant,
        cancel: &dyn Cancel,
        prepare: &mut dyn FnMut(&mut Value) -> WireResult<()>,
    ) -> WireResult<()> {
        observation_socket::annotate(
            &request.socket,
            endpoint,
            params,
            deadline,
            super::engine::BUDGET,
            cancel,
            prepare,
        )
    }

    fn sleep(&self, duration: Duration, cancel: &dyn Cancel) {
        let end = Instant::now() + duration;
        while !cancel.interrupted() {
            let remaining = end.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            std::thread::sleep(remaining.min(Duration::from_millis(10)));
        }
    }
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
