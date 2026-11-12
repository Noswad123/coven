#![cfg(any(target_os = "linux", target_os = "macos"))]
//! Pinned Codex projection, exercised without native agents or CLI provider choices.
#[path = "support/physical_temp.rs"]
mod physical_temp;
use coven::agent_management::{self, InventoryRequest, ReportRequest, ServeRequest};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

// openai/codex rust-v0.167.1, 0a94398fa60c4fac0e7659e4213a0a36e613dd46:
// codex-rs/hooks/schema/generated/session-start.command.input.schema.json.
fn native(source: &str, transcript: Value) -> Value {
    json!({"session_id":"PRIVATE-SID","hook_event_name":"SessionStart",
        "source":source,"cwd":"PRIVATE-CWD","model":"PRIVATE-MODEL",
        "permission_mode":"default","transcript_path":transcript})
}

#[test]
fn receiver_child() {
    let Ok(root) = std::env::var("COVEN_CODEX_CAPTURE_ROOT") else {
        return;
    };
    agent_management::serve_arguments(
        &ServeRequest {
            workspace: root.clone().into(),
            socket: format!("{root}/receiver.sock"),
            duration: 1,
        },
        true,
    )
    .unwrap();
}

#[test]
fn capture_restart_and_minimal_wire() {
    let root = physical_temp::tempdir();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.path().join("coven.json"), "{}").unwrap();
    let socket = root
        .path()
        .join("receiver.sock")
        .to_str()
        .unwrap()
        .to_owned();
    let inventory = InventoryRequest {
        socket: socket.clone(),
    };
    let request = ReportRequest {
        socket: socket.clone(),
        provider: "codex".into(),
        event: Some("SessionStart".into()),
    };
    let start = || {
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "receiver_child", "--nocapture"])
            .env("COVEN_CODEX_CAPTURE_ROOT", root.path())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if agent_management::inventory_arguments(&inventory, true).is_ok() {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        child
    };
    let mut child = start();
    let mut ids = Vec::new();
    for provider in ["claude", "codex"] {
        let report = ReportRequest {
            provider: provider.into(),
            ..request.clone()
        };
        let output = agent_management::report_payload(
            &report,
            native("startup", Value::Null).to_string().as_bytes(),
            true,
        )
        .unwrap();
        assert!(!output.stdout.contains("PRIVATE"));
        let accepted: Value = serde_json::from_str(&output.stdout).unwrap();
        ids.push(accepted["record_id"].clone());
    }
    assert_ne!(ids[0], ids[1]);
    // Extra agent_id is discarded, not classified as a Claude subagent.
    let mut payload = native("resume", json!("PRIVATE-TRANSCRIPT"));
    payload["agent_id"] = json!({"private":"PRIVATE-AGENT"});
    payload["prompt"] = json!("PRIVATE-PROMPT");
    agent_management::report_payload(&request, payload.to_string().as_bytes(), true).unwrap();
    let output = agent_management::inventory_arguments(&inventory, true).unwrap();
    assert!(!output.stdout.contains("PRIVATE"));
    let view: Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(view["records"].as_array().unwrap().len(), 2);
    for record in view["records"].as_array().unwrap() {
        assert_eq!(record["reported_state"], "unknown");
        assert!(record["age_ms"].is_u64());
    }
    assert!(child.wait().unwrap().success());
    let path = root.path().join("state/agent-management.json");
    let saved = fs::read(&path).unwrap();
    let saved_text = String::from_utf8(saved.clone()).unwrap();
    for secret in [
        "PRIVATE-CWD",
        "PRIVATE-MODEL",
        "PRIVATE-TRANSCRIPT",
        "PRIVATE-AGENT",
        "PRIVATE-PROMPT",
        "permission_mode",
        "source",
    ] {
        assert!(!saved_text.contains(secret));
    }
    let mut child = start();
    let view: Value = serde_json::from_str(
        &agent_management::inventory_arguments(&inventory, true)
            .unwrap()
            .stdout,
    )
    .unwrap();
    for record in view["records"].as_array().unwrap() {
        assert!(ids.contains(&record["record_id"]));
        assert_eq!(record["reported_state"], "unknown");
        assert!(record["age_ms"].is_null());
    }
    assert!(child.wait().unwrap().success());
    assert_eq!(saved, fs::read(&path).unwrap());

    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let peer = thread::spawn(move || {
        for _ in 0..6 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let frame: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(
                frame["params"],
                json!({"provider":"codex","event":"SessionStart","session_id":"PRIVATE-SID"})
            );
            writeln!(stream, "{}", json!({"id":frame["id"],"contract":agent_management::CONTRACT,"type":"result","result":{"type":"accepted","record_id":"a".repeat(64),"delivery":"committed"}})).unwrap();
        }
    });
    for source in ["startup", "resume", "clear", "compact", "fork"] {
        let output = agent_management::report_payload(
            &request,
            native(source, json!("PRIVATE-TRANSCRIPT"))
                .to_string()
                .as_bytes(),
            true,
        )
        .unwrap();
        assert!(!output.stdout.contains("PRIVATE"));
    }
    agent_management::report_payload(
        &request,
        br#"{"session_id":"PRIVATE-SID","hook_event_name":"SessionStart"}"#,
        true,
    )
    .unwrap();
    peer.join().unwrap();
}
