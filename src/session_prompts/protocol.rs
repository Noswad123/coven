use super::*;
use crate::values::{Number, Value};

#[derive(Clone, Copy)]
pub(super) enum Method {
    Capabilities,
    Submit,
}
impl Method {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Capabilities => "capabilities",
            Self::Submit => "submit",
        }
    }
}

pub(super) fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}

pub(super) fn identifier(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|tail| {
        !tail.is_empty()
            && tail.len() <= 128
            && tail
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    })
}

fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_object()?.get(key)
}
fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    field(value, key)?.as_str()
}
fn integer(value: &Value, key: &str, expected: usize) -> bool {
    matches!(field(value,key),Some(Value::Number(Number::Integer(n))) if n.as_str() == expected.to_string())
}

pub(super) fn decode(payload: &[u8], id: &str) -> WireResult<Value> {
    let root =
        crate::observation_wire::decode_native(payload).map_err(|e| Failure::new(e.code()))?;
    if !integer(&root, "version", 1)
        || text(&root, "id") != Some(id)
        || !text(&root, "runtime_id").is_some_and(uuid)
        || field(&root, "result").is_some() == field(&root, "error").is_some()
    {
        return Err(Failure::new("schema_error"));
    }
    Ok(root)
}

pub(super) fn capability(root: &Value) -> WireResult<String> {
    let error = || Failure::new("native_prompt_unsupported");
    let result = field(root, "result").ok_or_else(error)?;
    if text(result, "type") != Some("capabilities")
        || text(result, "contract") != Some(NATIVE_CONTRACT)
        || !integer(result, "version", 1)
        || !integer(result, "max_prompt_bytes", LIMIT)
        || text(result, "runtime_id") != text(root, "runtime_id")
    {
        return Err(error());
    }
    Ok(text(root, "runtime_id").ok_or_else(error)?.into())
}

pub(super) fn receipt(
    root: &Value,
    request: &Request,
    digest: &str,
    size: usize,
) -> WireResult<String> {
    let uncertain = |code| Failure {
        code: String::from(code),
        delivery: Delivery::Uncertain,
        interrupted: false,
    };
    if text(root, "runtime_id") != Some(&request.expected_runtime) {
        return Err(uncertain("invalid_native_runtime_receipt"));
    }
    if let Some(error) = field(root, "error") {
        let code = text(error, "code").unwrap_or("");
        let delivery = match code {
            "runtime_mismatch"
            | "request_capacity"
            | "session_unavailable"
            | "request_unavailable" => "rejected",
            "request_already_seen" | "admission_uncertain" => "uncertain",
            _ => return Err(uncertain("invalid_native_error")),
        };
        if text(error, "delivery") != Some(delivery) {
            return Err(uncertain("invalid_native_error"));
        }
        return Err(Failure {
            code: format!("native_{code}"),
            delivery: if delivery == "rejected" {
                Delivery::Rejected
            } else {
                Delivery::Uncertain
            },
            interrupted: false,
        });
    }
    let result = field(root, "result").ok_or_else(|| uncertain("invalid_native_receipt"))?;
    if text(result, "type") != Some("prompt_accepted")
        || text(result, "contract") != Some(NATIVE_CONTRACT)
        || text(result, "session_id") != Some(&request.session)
        || text(result, "prompt_sha256") != Some(digest)
        || !integer(result, "prompt_bytes", size)
        || !text(result, "message_id").is_some_and(|s| identifier(s, "msg_"))
    {
        return Err(uncertain("invalid_native_receipt"));
    }
    Ok(text(result, "message_id")
        .ok_or_else(|| uncertain("invalid_native_receipt"))?
        .into())
}
