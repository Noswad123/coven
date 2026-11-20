//! Bounded, single-attempt native reads. No discovery, transcript cache or writes.
use crate::observation_socket::{NoCancel, ReadBudget, check};
use crate::session_prompts::{
    endpoint::Endpoint,
    fresh_id,
    protocol::{identifier, uuid},
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

const CONTRACT: &str = "djinn.conversation-read.v1";
const LIMIT: usize = 65536;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Message {
    id: String,
    role: String,
    text: String,
    truncated: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Conversation {
    #[serde(rename = "type")]
    kind: String,
    contract: String,
    session_id: String,
    messages: Vec<Message>,
    has_older: bool,
    truncated: bool,
    omitted_parts: bool,
    projection_sha256: String,
}

// Authority parser rejects duplicate keys and nonfinite values. This dedicated
// profile permits 8 KiB text leaves, unlike the ordinary 4K observation profile.
pub(crate) fn decode(payload: &[u8]) -> Result<Value> {
    fn walk(value: &crate::values::Value, depth: usize) -> Result<()> {
        use crate::values::Value as V;
        if depth > 32 || (depth == 32 && matches!(value, V::Object(_) | V::Array(_))) {
            return Err(Error("schema_error"));
        }
        match value {
            V::Object(o) => {
                if o.entries().len() > 128 {
                    return Err(Error("schema_error"));
                }
                for (k, v) in o.entries() {
                    if k.len() > 128 {
                        return Err(Error("schema_error"));
                    }
                    walk(v, depth + 1)?;
                }
            }
            V::Array(a) => {
                if a.len() > 128 {
                    return Err(Error("schema_error"));
                }
                for v in a {
                    walk(v, depth + 1)?;
                }
            }
            V::String(s) if s.len() > 8192 => return Err(Error("schema_error")),
            _ => {}
        }
        Ok(())
    }
    if payload.len() + 1 > LIMIT {
        return Err(Error("response_limit"));
    }
    let value = crate::values::parse(payload, crate::values::Profile::Authority)
        .map_err(|_| Error("schema_error"))?;
    walk(&value, 0)?;
    crate::observation_wire::to_json(&value).map_err(|_| Error("schema_error"))
}

fn exchange(
    endpoint: &Endpoint,
    method: &str,
    params: Value,
    runtime: &str,
    deadline: Instant,
    budget: &mut ReadBudget,
) -> Result<Value> {
    let mut operation = || -> crate::observation_socket::WireResult<Value> {
        check(deadline, &NoCancel)?;
        endpoint.fence()?;
        let id = fresh_id()?;
        let mut request =
            serde_json::to_vec(&json!({"version":1,"id":id,"method":method,"params":params}))
                .map_err(|_| crate::observation_socket::Failure::new("schema_error"))?;
        request.push(b'\n');
        let socket = crate::platform::observation_socket::Socket::connect(
            &endpoint.socket,
            deadline,
            &NoCancel,
        )?;
        endpoint.fence()?;
        socket.send(&request, deadline, &NoCancel)?;
        let payload = socket.receive(deadline, budget, &NoCancel)?;
        endpoint.fence()?;
        check(deadline, &NoCancel)?;
        let root = decode(&payload)
            .map_err(|_| crate::observation_socket::Failure::new("schema_error"))?;
        let invalid = || crate::observation_socket::Failure::new("schema_error");
        if root.as_object().is_none_or(|o| o.len() != 4)
            || root["version"].as_u64() != Some(1)
            || root["id"] != id
            || !root["runtime_id"].as_str().is_some_and(uuid)
            || root.get("result").is_some() == root.get("error").is_some()
        {
            return Err(invalid());
        }
        if root["runtime_id"] != runtime {
            return Err(crate::observation_socket::Failure::new("runtime_mismatch"));
        }
        if let Some(error) = root.get("error") {
            if error["delivery"] != "rejected" {
                return Err(invalid());
            }
            let code = match error["code"].as_str() {
                Some("runtime_mismatch") => "runtime_mismatch",
                Some("session_unavailable") => "session_unavailable",
                Some("conversation_unsupported") => "conversation_unsupported",
                Some("request_unavailable") => "request_unavailable",
                _ => return Err(invalid()),
            };
            return Err(crate::observation_socket::Failure::new(code));
        }
        Ok(root["result"].clone())
    };
    operation().map_err(|e| {
        Error(match e.code.as_str() {
            "runtime_mismatch" => "runtime_mismatch",
            "session_unavailable" => "session_unavailable",
            "conversation_unsupported" => "conversation_unsupported",
            "request_unavailable" => "request_unavailable",
            "timeout" => "timeout",
            "response_limit" => "response_limit",
            _ => "native_unavailable",
        })
    })
}

pub(crate) fn projection(result: Value, session: &str) -> Result<Value> {
    let value: Conversation =
        serde_json::from_value(result).map_err(|_| Error("invalid_conversation"))?;
    let mut ids = HashSet::new();
    if value.kind != "conversation"
        || value.contract != CONTRACT
        || value.session_id != session
        || value.messages.len() > 20
        || value.messages.iter().map(|m| m.text.len()).sum::<usize>() > 8192
        || value.messages.iter().any(|m| {
            !identifier(&m.id, "msg_")
                || !ids.insert(&m.id)
                || !matches!(m.role.as_str(), "user" | "assistant")
        })
        || value.projection_sha256.len() != 64
        || !value
            .projection_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error("invalid_conversation"));
    }
    // Tuple arrays match JSON.stringify, including literal Unicode, slash and
    // U+2028/U+2029. No object-key ordering or platform newline normalization.
    let tuples: Vec<_> = value
        .messages
        .iter()
        .map(|m| (&m.id, &m.role, &m.text, m.truncated))
        .collect();
    let canonical = serde_json::to_vec(&tuples).map_err(|_| Error("invalid_conversation"))?;
    if format!("{:x}", Sha256::digest(&canonical)) != value.projection_sha256 {
        return Err(Error("invalid_conversation"));
    }
    // Native session/message identifiers and raw errors never reach the browser.
    Ok(
        json!({"source":CONTRACT,"messages":value.messages.iter().map(|m|
        json!({"role":m.role,"text":m.text,"truncated":m.truncated})).collect::<Vec<_>>(),
        "has_older":value.has_older,"truncated":value.truncated,"omitted_parts":value.omitted_parts,
        "projection_sha256":value.projection_sha256}),
    )
}

pub(crate) fn read(socket: &str, runtime: &str, session: &str) -> Result<Value> {
    if !uuid(runtime) || !identifier(session, "ses_") {
        return Err(Error("invalid_conversation"));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let endpoint = Endpoint::select(socket).map_err(|_| Error("native_unavailable"))?;
    // Both exchanges share a single byte and time budget; each frame is also
    // capped by decode. No retries and no auto-following a restarted runtime.
    let mut budget = ReadBudget::new(LIMIT);
    let capabilities = exchange(
        &endpoint,
        "capabilities",
        json!({}),
        runtime,
        deadline,
        &mut budget,
    )?;
    let expected = json!({"contract":CONTRACT,"max_messages":20,"max_text_bytes":8192,"max_response_bytes":65536});
    if capabilities["type"] != "capabilities"
        || capabilities["runtime_id"] != runtime
        || capabilities["contract"] != "djinn.interactive-prompt.v1"
        || capabilities["version"].as_u64() != Some(1)
        || capabilities["max_prompt_bytes"].as_u64() != Some(65536)
        || capabilities["conversation_read"] != expected
    {
        return Err(Error("conversation_unsupported"));
    }
    let result = exchange(
        &endpoint,
        "conversation",
        json!({"expected_runtime_id":runtime,"session_id":session}),
        runtime,
        deadline,
        &mut budget,
    )?;
    let value = projection(result, session)?;
    check(deadline, &NoCancel).map_err(|_| Error("timeout"))?;
    Ok(value)
}

#[cfg(test)]
mod tests;
