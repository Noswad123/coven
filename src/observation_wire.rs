//! Shared bounded JSON wire profile. Validate extensions before projection.
use crate::{
    Error, Result,
    values::{self, Number, Profile, Value},
};

fn require(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error("schema_error"))
    }
}

fn wire_walk(v: &Value, key: Option<&str>) -> Result<()> {
    match v {
        Value::Object(o) => {
            require(o.entries().len() <= 4096)?;
            for (k, v) in o.entries() {
                require(k.chars().count() <= 4096)?;
                wire_walk(v, Some(k))?;
            }
        }
        Value::Array(a) => {
            require(a.len() <= 4096)?;
            for v in a {
                wire_walk(v, key)?;
            }
        }
        Value::String(s) => {
            let path = key.is_some_and(|k| {
                matches!(k, "value" | "cwd" | "foreground_cwd" | "path") || k.ends_with("_path")
            });
            require(s.chars().count() <= if path { 8192 } else { 4096 })?;
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn decode_native(payload: &[u8]) -> Result<Value> {
    let s = std::str::from_utf8(payload).map_err(|_| Error("invalid_utf8"))?;
    let (mut depth, mut quoted, mut escaped) = (0i32, false, false);
    let mut constant = false;
    // Preserve the lexical nesting limit even on malformed JSON.
    for (at, c) in s.bytes().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                quoted = false;
            }
        } else if c == b'"' {
            quoted = true;
        } else if matches!(c, b'[' | b'{') {
            depth += 1;
            if depth > 32 {
                return Err(Error("nesting_limit"));
            }
        } else if matches!(c, b']' | b'}') {
            depth -= 1;
        } else if s.as_bytes()[at..].starts_with(b"NaN")
            || s.as_bytes()[at..].starts_with(b"Infinity")
            || s.as_bytes()[at..].starts_with(b"-Infinity")
        {
            constant = true;
        }
    }
    let value = values::parse(payload, Profile::Authority).map_err(|e| {
        Error(match e.code() {
            "duplicate_json_key" => "duplicate_key",
            "unsupported_nonfinite_json" => {
                if constant {
                    "nonfinite_json"
                } else {
                    "schema_error"
                }
            }
            "invalid_json_number" if constant => "nonfinite_json",
            "unsupported_or_invalid_json_scalar"
            | "unsupported_json_node_profile"
            | "unsupported_integer_profile"
            | "unsupported_number_token_profile" => "schema_error",
            _ => "invalid_json",
        })
    })?;
    wire_walk(&value, None)?;
    Ok(value)
}

// Exact conversion for closed-schema consumers. Open protocols must project
// consumed native fields first, rather than narrowing ignored extension values.
pub(crate) fn decode(payload: &[u8]) -> Result<serde_json::Value> {
    to_json(&decode_native(payload)?)
}

pub(crate) fn to_json(v: &Value) -> Result<serde_json::Value> {
    use serde_json::Value as J;
    Ok(match v {
        Value::Null => J::Null,
        Value::Bool(b) => J::Bool(*b),
        Value::String(s) => J::String(s.clone()),
        Value::Number(Number::Integer(n)) => {
            let number = if let Ok(n) = n.as_str().parse::<u64>() {
                n.into()
            } else if let Ok(n) = n.as_str().parse::<i64>() {
                n.into()
            } else {
                return Err(Error("unsupported_json_integer_projection"));
            };
            J::Number(number)
        }
        Value::Number(Number::Float(n)) => {
            J::Number(serde_json::Number::from_f64(n.value()).ok_or(Error("schema_error"))?)
        }
        Value::Array(a) => J::Array(a.iter().map(to_json).collect::<Result<_>>()?),
        Value::Object(o) => J::Object(
            o.entries()
                .iter()
                .map(|(k, v)| Ok((k.clone(), to_json(v)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

/// Fully validate the wire, then project only acknowledgement envelope fields.
/// Correlation, expected result type and safe rejection policy remain the caller's.
pub(crate) fn decode_ack(payload: &[u8]) -> Result<serde_json::Value> {
    let root = decode_native(payload)?;
    let root = root.as_object().ok_or(Error("schema_error"))?;
    let id = root
        .get("id")
        .and_then(Value::as_str)
        .ok_or(Error("schema_error"))?;
    require(root.get("result").is_some() != root.get("error").is_some())?;
    let mut out = serde_json::json!({"id":id});
    let (branch, keys): (&str, &[&str]) = if root.get("result").is_some() {
        ("result", &["type"])
    } else {
        ("error", &["code", "message"])
    };
    let object = root
        .get(branch)
        .and_then(Value::as_object)
        .ok_or(Error("schema_error"))?;
    let mut projected = serde_json::Map::new();
    for key in keys {
        let value = object
            .get(key)
            .and_then(Value::as_str)
            .ok_or(Error("schema_error"))?;
        projected.insert((*key).into(), serde_json::Value::String(value.into()));
    }
    out[branch] = serde_json::Value::Object(projected);
    Ok(out)
}

#[cfg(test)]
#[path = "observation_wire/tests.rs"]
mod tests;
