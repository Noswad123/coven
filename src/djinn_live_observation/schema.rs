use super::{NATIVE_CONTRACT, Request};
use crate::{Error, Result};
use serde_json::Value;
use std::collections::HashSet;

pub(crate) fn require(ok: bool) -> Result<()> {
    crate::error::require(ok, "schema_error")
}
pub(crate) fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}
pub(crate) fn integer(value: u64) -> Result<Value> {
    Ok(Value::from(value))
}
pub(crate) fn object(entries: Vec<(&str, Value)>) -> Result<Value> {
    Ok(Value::Object(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
    ))
}
pub(crate) fn field<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .as_object()
        .and_then(|o| o.get(key))
        .ok_or(Error("schema_error"))
}
pub(crate) fn string(value: &Value) -> Result<&str> {
    value.as_str().ok_or(Error("schema_error"))
}
pub(crate) fn fields(value: &Value, names: &str) -> Result<()> {
    let entries = value.as_object().ok_or(Error("schema_error"))?;
    require(
        entries.len() == names.split_whitespace().count()
            && names
                .split_whitespace()
                .all(|key| entries.iter().any(|(k, _)| k == key)),
    )
}
pub(crate) fn project(value: &Value, names: &str) -> Result<Value> {
    object(
        names
            .split_whitespace()
            .map(|k| Ok((k, field(value, k)?.clone())))
            .collect::<Result<_>>()?,
    )
}
pub(crate) fn uint(value: &Value, positive: bool) -> Result<u64> {
    let n = value.as_u64().ok_or(Error("schema_error"))?;
    require(n >= u64::from(positive) && n <= 9_007_199_254_740_991)?;
    Ok(n)
}
pub(crate) fn uuid(value: &str) -> Result<()> {
    let bytes = value.as_bytes();
    require(
        bytes.len() == 36
            && bytes.iter().enumerate().all(|(i, b)| {
                if [8, 13, 18, 23].contains(&i) {
                    *b == b'-'
                } else {
                    b.is_ascii_digit() || (b'a'..=b'f').contains(b)
                }
            })
            && bytes[14] == b'4'
            && matches!(bytes[19], b'8' | b'9' | b'a' | b'b'),
    )
}
pub(crate) fn session(value: &str) -> Result<()> {
    require(
        !value.is_empty()
            && value.len() <= 128
            && value.bytes().all(|b| (b'!'..=b'~').contains(&b)),
    )
}
pub(crate) fn path(value: &str) -> Result<()> {
    require(
        value.starts_with('/')
            && !value.ends_with('/')
            && value.len() <= 4096
            && !value.split('/').any(|c| c == "..")
            && !value
                .chars()
                .any(|c| c <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&c)),
    )
}
pub(crate) fn capabilities(value: &Value) -> Result<()> {
    fields(
        value,
        "activity idle catalog_complete decision_state retry_state durable_lifecycle job_completion",
    )?;
    for (key, v) in value.as_object().ok_or(Error("schema_error"))? {
        require(matches!(v, Value::Bool(b) if *b == (key == "activity")))?;
    }
    Ok(())
}

pub(super) fn decode(payload: &[u8], request: &Request) -> Result<(String, Value)> {
    let value = crate::observation_wire::decode(payload).map_err(|error| {
        if error.code() == "unsupported_json_integer_projection" {
            Error("schema_error")
        } else {
            error
        }
    })?;
    let version = field(&value, "schema_version")?;
    require(version.as_i64().is_some() || version.as_u64().is_some())?;
    let contract = string(field(&value, "contract")?)?;
    if version.as_u64() != Some(1) || contract != NATIVE_CONTRACT {
        return Err(Error("unsupported_contract"));
    }
    fields(
        &value,
        "schema_version contract runtime_id sequence sampled_at_ms valid_for_ms source activity_scope capabilities authorization dispatchable native_lifecycle job_completion instances",
    )?;
    uuid(string(field(&value, "runtime_id")?)?)?;
    uint(field(&value, "sequence")?, true)?;
    uint(field(&value, "sampled_at_ms")?, false)?;
    require(uint(field(&value, "valid_for_ms")?, false)? == 3000)?;
    for (key, expected) in [
        ("source", "legacy_session_run_state"),
        ("activity_scope", "legacy_runner_ownership"),
        ("authorization", "none"),
        ("native_lifecycle", "unestablished"),
        ("job_completion", "unestablished"),
    ] {
        require(string(field(&value, key)?)? == expected)?;
    }
    require(matches!(field(&value, "dispatchable")?, Value::Bool(false)))?;
    capabilities(field(&value, "capabilities")?)?;
    let instances = field(&value, "instances")?
        .as_array()
        .ok_or(Error("schema_error"))?;
    require(instances.len() <= 16)?;
    let mut seen = HashSet::new();
    let mut count = 0;
    let mut matched = false;
    let mut target = None;
    for instance in instances {
        fields(instance, "instance_id sessions")?;
        let id = string(field(instance, "instance_id")?)?;
        uuid(id)?;
        require(seen.insert(id))?;
        matched |= id == request.instance;
        let sessions = field(instance, "sessions")?
            .as_array()
            .ok_or(Error("schema_error"))?;
        count += sessions.len();
        require(count <= 128)?;
        let mut seen_sessions = HashSet::new();
        for entry in sessions {
            fields(entry, "session_id activity")?;
            let id_session = string(field(entry, "session_id")?)?;
            session(id_session)?;
            require(seen_sessions.insert(id_session))?;
            let activity = string(field(entry, "activity")?)?;
            require(matches!(activity, "busy" | "unknown"))?;
            if id == request.instance && id_session == request.session {
                target = Some(activity.to_owned());
            }
        }
    }
    if string(field(&value, "runtime_id")?)? != request.runtime {
        return Err(Error("runtime_mismatch"));
    }
    if !matched {
        return Err(Error("instance_not_observed"));
    }
    let activity = target.ok_or(Error("session_not_observed"))?;
    let mut source = project(
        &value,
        "contract runtime_id sequence sampled_at_ms valid_for_ms source activity_scope capabilities",
    )?;
    source["instance_id"] = text(&request.instance);
    source["session_id"] = text(&request.session);
    Ok((activity, source))
}
