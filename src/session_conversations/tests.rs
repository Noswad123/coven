use super::*;

const RUNTIME: &str = "11111111-2222-4333-8444-555555555555";

fn result(text: &str) -> Value {
    let messages = json!([{"id":"msg_fixture","role":"assistant","text":text,"truncated":false}]);
    let tuples = json!([["msg_fixture", "assistant", text, false]]);
    json!({"type":"conversation","contract":CONTRACT,"session_id":"ses_fixture",
        "messages":messages,"has_older":true,"truncated":false,"omitted_parts":true,
        "projection_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&tuples).unwrap()))})
}

#[test]
fn projection_accepts_full_text_and_strips_native_identifiers() {
    let text = "x".repeat(8192);
    let output = projection(result(&text), "ses_fixture").unwrap();
    assert_eq!(output["messages"][0]["text"], text);
    assert!(output["messages"][0].get("id").is_none());
    assert!(output.get("session_id").is_none());
    assert!(projection(result(&"x".repeat(8193)), "ses_fixture").is_err());
    for key in ["contract", "session_id", "projection_sha256"] {
        let mut bad = result("safe");
        bad[key] = json!("wrong");
        assert!(projection(bad, "ses_fixture").is_err());
    }
    let mut bad = result("safe");
    bad["messages"][0]["role"] = json!("system");
    assert!(projection(bad, "ses_fixture").is_err());
    let mut bad = result("safe");
    bad["extra"] = json!(true);
    assert!(projection(bad, "ses_fixture").is_err());
}

#[test]
fn dedicated_decoder_rejects_duplicates_depth_and_accepts_8k_text() {
    assert!(decode(br#"{"a":1,"a":2}"#).is_err());
    assert!(decode(format!("{}0{}", "[".repeat(34), "]".repeat(34)).as_bytes()).is_err());
    assert!(decode(&serde_json::to_vec(&result(&"x".repeat(8192))).unwrap()).is_ok());
    assert!(decode(&vec![b' '; LIMIT]).is_err());
}

#[test]
fn nesting_exactly_32_containers_allowed_and_33_refused() {
    for kind in ["array", "object", "mixed"] {
        for empty in [false, true] {
            for count in [32, 33] {
                let mut payload = if empty { String::new() } else { "0".into() };
                for i in 0..count {
                    payload = if kind == "object" || (kind == "mixed" && i % 2 == 0) {
                        if empty && i == 0 {
                            "{}".into()
                        } else {
                            format!("{{\"x\":{payload}}}")
                        }
                    } else {
                        format!("[{payload}]")
                    };
                }
                assert_eq!(
                    decode(payload.as_bytes()).is_ok(),
                    count == 32,
                    "{kind} empty={empty} count={count}"
                );
            }
        }
    }
}

#[test]
fn javascript_stringify_digest_vector() {
    // Generated with Node JSON.stringify on tuple arrays, including all JSON
    // control escapes, literal Unicode/line separators, slash and backslash.
    let text = "\0\u{0008}\t\n\u{000c}\r\u{001f}\"\\/\u{00e9}\u{2028}\u{2029}\u{1f600}";
    let encoded =
        serde_json::to_string(&json!([["msg_fixture", "assistant", text, false]])).unwrap();
    assert_eq!(
        encoded,
        "[[\"msg_fixture\",\"assistant\",\"\\u0000\\b\\t\\n\\f\\r\\u001f\\\"\\\\/\u{00e9}\u{2028}\u{2029}\u{1f600}\",false]]"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(encoded.as_bytes())),
        "4b55cfc034c79e9c1f6bbc31c7443fe11a9e18f14777f16f988d520a3a9ddbb8"
    );
    assert!(projection(result(text), "ses_fixture").is_ok());
}

#[cfg(unix)]
fn native_fixture(runtime: &str, response: Value, supported: bool) -> Result<Value> {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::{fs::PermissionsExt, net::UnixListener},
    };
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.path().join("n.sock");
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let runtime = runtime.to_string();
    let server = std::thread::spawn(move || {
        for method in ["capabilities", "conversation"] {
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut socket = loop {
                if let Ok((socket, _)) = listener.accept() {
                    break socket;
                }
                if Instant::now() > deadline {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&socket).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["version"], 1);
            assert_eq!(request["method"], method);
            assert!(request["id"].as_str().is_some_and(uuid));
            assert_eq!(
                request["params"],
                if method == "capabilities" {
                    json!({})
                } else {
                    json!({"expected_runtime_id":RUNTIME,"session_id":"ses_fixture"})
                }
            );
            let branch = if method == "capabilities" {
                json!({"result":{"type":"capabilities","runtime_id":runtime,
                    "contract":"djinn.interactive-prompt.v1","version":1,"max_prompt_bytes":65536,
                    "conversation_read":if supported {json!({"contract":CONTRACT,"max_messages":20,"max_text_bytes":8192,"max_response_bytes":65536})} else {Value::Null}}})
            } else {
                response.clone()
            };
            let mut reply = json!({"version":1,"id":request["id"],"runtime_id":runtime});
            reply
                .as_object_mut()
                .unwrap()
                .extend(branch.as_object().unwrap().clone());
            let mut frame = serde_json::to_vec(&reply).unwrap();
            frame.push(b'\n');
            socket.write_all(&frame).unwrap();
            if !supported || runtime != RUNTIME {
                break;
            }
        }
    });
    let output = read(path.to_str().unwrap(), RUNTIME, "ses_fixture");
    server.join().unwrap();
    output
}

#[cfg(unix)]
#[test]
fn private_socket_capabilities_then_conversation() {
    let output =
        native_fixture(RUNTIME, json!({"result":result(&"x".repeat(8192))}), true).unwrap();
    assert_eq!(output["messages"][0]["text"].as_str().unwrap().len(), 8192);
    assert!(output["has_older"].as_bool().unwrap());
    assert_eq!(
        native_fixture(
            "00000000-2222-4333-8444-555555555555",
            json!({"result":result("ignored")}),
            true
        )
        .unwrap_err()
        .code(),
        "runtime_mismatch"
    );
    assert!(native_fixture(RUNTIME, json!({"result":result(&"x".repeat(8193))}), true).is_err());
    assert_eq!(
        native_fixture(RUNTIME, json!({}), false)
            .unwrap_err()
            .code(),
        "conversation_unsupported"
    );
    for code in [
        "runtime_mismatch",
        "session_unavailable",
        "conversation_unsupported",
        "request_unavailable",
    ] {
        let error = json!({"error":{"code":code,"delivery":"rejected","message":"DO NOT FORWARD RAW ERROR"}});
        assert_eq!(
            native_fixture(RUNTIME, error, true).unwrap_err().code(),
            code
        );
    }
    let error = json!({"error":{"code":"request_unavailable","delivery":"uncertain","message":"DO NOT FORWARD"}});
    assert_eq!(
        native_fixture(RUNTIME, error, true).unwrap_err().code(),
        "native_unavailable"
    );
}
