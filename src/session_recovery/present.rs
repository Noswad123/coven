//! Recovery-only legacy reference hints, never native identity or running status.
//! Keep this optional compatibility read separate from general PTY/A1 schemas.
use super::{HashSet, Kind, MAX_BYTES, Reference, reference_shape};
use crate::observation_socket::{self as wire, Cancel, Endpoint, Failure, Reply, WireResult};
use crate::platform::observation_socket::fence;
use crate::values::Value as Native;
use serde_json::json;
use std::time::{Duration, Instant};

fn field<'a>(value: &'a Native, key: &str) -> WireResult<&'a Native> {
    value
        .as_object()
        .and_then(|object| object.get(key))
        .ok_or_else(|| Failure::new("invalid_recovery_reference_snapshot"))
}

pub(super) fn parse(reply: &Reply) -> WireResult<HashSet<Reference>> {
    if reply.payload.len() > MAX_BYTES {
        return Err(Failure::new("response_limit"));
    }
    // Validates correlation, result/error exclusivity, protocol 18, command
    // submission v1, <=1024 panes, unique locator IDs and bounded native JSON.
    crate::kitsune_observation::command_snapshot(reply)?;
    let raw = crate::observation_wire::decode_native(&reply.payload)
        .map_err(|error| Failure::new(error.code()))?;
    let panes = field(field(field(&raw, "result")?, "snapshot")?, "panes")?
        .as_array()
        .ok_or_else(|| Failure::new("invalid_recovery_reference_snapshot"))?;
    let mut references = HashSet::new();
    for pane in panes {
        let Some(hint) = pane
            .as_object()
            .and_then(|object| object.get("agent_session"))
        else {
            continue;
        };
        // Only absence means no hint. Null, missing members or malformed values
        // must not silently erase a potentially relevant conservative veto.
        let string = |key| {
            field(hint, key)?
                .as_str()
                .ok_or_else(|| Failure::new("invalid_recovery_reference_snapshot"))
        };
        let reference = Reference {
            source: string("source")?.into(),
            runtime: string("agent")?.into(),
            kind: match string("kind")? {
                "id" => Kind::Id,
                "path" => Kind::Path,
                _ => return Err(Failure::new("invalid_recovery_reference_snapshot")),
            },
            value: string("value")?.into(),
        };
        // A different source/provider is a valid nonmatching hint, not a reason
        // to relabel it as kitsune:<provider> or infer an equivalent session.
        reference_shape(&reference)
            .map_err(|_| Failure::new("invalid_recovery_reference_snapshot"))?;
        references.insert(reference);
    }
    Ok(references)
}

pub(super) fn read(
    socket: &str,
    cancel: &dyn Cancel,
) -> WireResult<(Endpoint, HashSet<Reference>)> {
    let deadline = Instant::now() + Duration::from_secs(3);
    wire::check(deadline, cancel)?;
    let endpoint = wire::select(socket)?;
    fence(&endpoint)?;
    let reply = wire::read(
        &endpoint.selected,
        wire::ReadMethod::Snapshot,
        &json!({}),
        deadline,
        &mut wire::ReadBudget::new(MAX_BYTES),
        cancel,
    )?;
    if reply.endpoint != endpoint {
        return Err(Failure::new("endpoint_replaced"));
    }
    fence(&endpoint)?;
    let references = parse(&reply)?;
    wire::check(deadline, cancel)?;
    fence(&endpoint)?;
    Ok((endpoint, references))
}

#[cfg(test)]
#[path = "present_tests.rs"]
mod tests;
