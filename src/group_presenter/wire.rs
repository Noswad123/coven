//! Validate the complete native JSON profile, then project only consumed fields.
use super::*;
use crate::values::Value as Native;

fn schema() -> Failure {
    Failure::new("integration_view_schema_error")
}
fn field<'a>(value: &'a Native, key: &str) -> WireResult<&'a Native> {
    value
        .as_object()
        .and_then(|o| o.get(key))
        .ok_or_else(schema)
}
fn project(value: &Native, keys: &[&str]) -> WireResult<Value> {
    let mut out = json!({});
    for key in keys {
        out[*key] = crate::observation_wire::to_json(field(value, key)?).map_err(|_| schema())?;
    }
    Ok(out)
}
fn result(bytes: &[u8], id: &str, kind: &str) -> WireResult<Native> {
    let root = crate::observation_wire::decode_native(bytes).map_err(|e| Failure::new(e.code()))?;
    if field(&root, "id")?.as_str() != Some(id) {
        return Err(schema());
    }
    let object = root.as_object().ok_or_else(schema)?;
    if object.get("result").is_some() == object.get("error").is_some() {
        return Err(schema());
    }
    if let Some(error) = object.get("error") {
        let code = field(error, "code")?.as_str().ok_or_else(schema)?;
        field(error, "message")?.as_str().ok_or_else(schema)?;
        if code.is_empty()
            || code.len() > 64
            || !code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        {
            return Err(schema());
        }
        // Do not expose peer-supplied messages or codes in user output.
        return Err(Failure {
            code: "view_rejected".into(),
            delivery: Delivery::Rejected,
            interrupted: false,
        });
    }
    let result = field(&root, "result")?;
    if field(result, "type")?.as_str() != Some(kind) {
        return Err(schema());
    }
    Ok(result.clone())
}
fn token(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        !s.is_empty() && s.len() <= 128 && s.chars().all(|c| !c.is_control() && !c.is_whitespace())
    })
}
pub(super) fn snapshot(reply: &socket::Reply, source: &str) -> WireResult<()> {
    let result = result(&reply.payload, &reply.request_id, "session_snapshot")?;
    let snapshot = field(&result, "snapshot")?;
    field(snapshot, "version")?.as_str().ok_or_else(schema)?;
    let protocol = project(snapshot, &["protocol"])?;
    if protocol["protocol"].as_u64() != Some(18) {
        return Err(Failure::new("unsupported_protocol"));
    }
    let descriptor = snapshot
        .as_object()
        .and_then(|o| o.get("integration_views"))
        .filter(|v| !matches!(v, Native::Null))
        .ok_or_else(|| Failure::new("integration_view_unsupported"))?;
    if project(descriptor, &["version"])?["version"].as_u64() != Some(1) {
        return Err(Failure::new("integration_view_unsupported"));
    }
    let sources = field(descriptor, "sources")?
        .as_array()
        .ok_or_else(schema)?;
    if sources.len() > 8 {
        return Err(schema());
    }
    let mut ids = Vec::new();
    for entry in sources {
        let id = field(entry, "id")?.as_str().ok_or_else(schema)?;
        let epoch = project(entry, &["epoch"])?;
        let epoch = &epoch["epoch"];
        if crate::groups::source(id).is_err()
            || ids.contains(&id)
            || (!epoch.is_null() && !token(epoch))
        {
            return Err(schema());
        }
        ids.push(id);
    }
    // Validate every provider before selecting, including those after our entry.
    if !ids.contains(&source) {
        return Err(Failure::new("integration_view_source_disabled"));
    }
    Ok(())
}
pub(super) fn opened(bytes: &[u8], id: &str) -> WireResult<Value> {
    let value = project(
        &result(bytes, id, "view_opened")?,
        &["lease", "epoch", "revision", "ttl_ms"],
    )?;
    if !token(&value["lease"])
        || !token(&value["epoch"])
        || value["revision"].as_u64() != Some(0)
        || value["ttl_ms"].as_u64() != Some(5000)
    {
        return Err(schema());
    }
    Ok(value)
}
pub(super) fn replaced(bytes: &[u8], id: &str, epoch: &str, revision: u64) -> WireResult<Value> {
    let value = project(&result(bytes, id, "view_replaced")?, &["epoch", "revision"])?;
    if value["epoch"].as_str() != Some(epoch) || value["revision"].as_u64() != Some(revision) {
        return Err(schema());
    }
    Ok(value)
}
pub(super) fn polled(bytes: &[u8], id: &str, epoch: &str, revision: u64) -> WireResult<Value> {
    let result = result(bytes, id, "view_intents")?;
    let mut value = project(&result, &["epoch", "revision", "ttl_ms"])?;
    if value["epoch"].as_str() != Some(epoch)
        || value["revision"].as_u64() != Some(revision)
        || value["ttl_ms"].as_u64() != Some(5000)
    {
        return Err(schema());
    }
    let intents = field(&result, "intents")?.as_array().ok_or_else(schema)?;
    if intents.len() > 32 {
        return Err(schema());
    }
    let mut projected = Vec::new();
    for intent in intents {
        let intent = project(intent, &["epoch", "revision", "item_id"])?;
        if !token(&intent["epoch"])
            || intent["revision"].as_u64().is_none()
            || !token(&intent["item_id"])
        {
            return Err(schema());
        }
        projected.push(intent);
    }
    value["intents"] = json!(projected);
    Ok(value)
}
pub(super) fn closed(bytes: &[u8], id: &str) -> WireResult<Value> {
    result(bytes, id, "view_closed")?;
    Ok(json!({"type":"view_closed"}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_view_methods_are_not_allowed() {
        let endpoint = socket::Endpoint {
            selected: "/not/a/socket".into(),
            resolved: "/not/a/socket".into(),
            device: 0,
            inode: 0,
        };
        for method in ["view.open", "view.replace", "view.poll", "view.close"] {
            let error = socket::view_call(
                &endpoint,
                method,
                &json!({}),
                Instant::now() + Duration::from_secs(1),
                &socket::NoCancel,
                &|_, _| panic!("legacy method reached response validation"),
            )
            .unwrap_err();
            assert_eq!(error.code, "invalid_view_method");
            assert_eq!(error.delivery, Delivery::Unsent);
        }
    }
}
