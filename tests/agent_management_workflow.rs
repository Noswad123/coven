#![cfg(unix)]
//! Real Coven receiver and private RPC; no provider executables or native hooks.
use coven::agent_management::{self, InventoryRequest, ReportRequest, ServeRequest};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

fn wait_ready(path: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(Instant::now() < deadline, "receiver did not bind");
        thread::sleep(Duration::from_millis(5));
    }
}

fn private_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    root
}

#[test]
fn cli_hooks_inventory_restart_and_interrupt_cleanup() {
    let root = private_root();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    fs::write(workspace.join("coven.json"), b"marker").unwrap();
    let socket = root.path().join("receiver.sock");
    let binary = env!("CARGO_BIN_EXE_coven");
    let spawn = || {
        Command::new(binary)
            .args(["agent", "serve"])
            .arg(&workspace)
            .arg("--socket")
            .arg(&socket)
            .args(["--duration", "60"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let mut server = spawn();
    wait_ready(&socket);
    assert_eq!(fs::metadata(&socket).unwrap().mode() & 0o777, 0o600);
    // Binding precedes store initialization; the RPC itself waits in the backlog.
    for (event, state) in [
        ("SessionStart", "unknown"),
        ("UserPromptSubmit", "working"),
        ("PreToolUse", "working"),
        ("PostToolUse", "working"),
        ("Stop", "idle"),
        ("SessionEnd", "ended"),
    ] {
        let mut reporter = Command::new(binary)
            .args(["agent", "report", "--socket"])
            .arg(&socket)
            .args(["--provider", "claude", "--event", event])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let payload = json!({"session_id":"private-session-id","hook_event_name":event,"agent_id":null,"prompt":"SECRET_PROMPT","transcript_path":"SECRET_TRANSCRIPT","cwd":"SECRET_CWD"});
        reporter
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        let report = reporter.wait_with_output().unwrap();
        assert!(
            report.status.success(),
            "{}",
            String::from_utf8_lossy(&report.stderr)
        );
        let stdout = String::from_utf8(report.stdout).unwrap();
        assert!(stdout.contains("Accepted"));
        assert!(!stdout.contains("private-session-id") && !stdout.contains("SECRET"));
        let inventory = Command::new(binary)
            .args(["agent", "inventory", "--socket"])
            .arg(&socket)
            .arg("--json")
            .output()
            .unwrap();
        assert!(inventory.status.success());
        let view: Value = serde_json::from_slice(&inventory.stdout).unwrap();
        assert_eq!(view["records"][0]["reported_state"], state);
        assert_eq!(view["authorization"], "none");
        assert!(
            !String::from_utf8(inventory.stdout)
                .unwrap()
                .contains("private-session-id")
        );
    }
    let saved = fs::read(workspace.join("state/agent-management.json")).unwrap();
    assert!(!String::from_utf8_lossy(&saved).contains("SECRET"));
    for payload in [
        br#"{"session_id":"private-session-id","hook_event_name":"Stop","agent_id":"sub"}"#
            .as_slice(),
        br#"{"session_id":"private-session-id","hook_event_name":"SubagentStop"}"#,
    ] {
        agent_management::report_payload(
            &ReportRequest {
                socket: socket.to_str().unwrap().into(),
                provider: "claude".into(),
                event: None,
            },
            payload,
            true,
        )
        .unwrap();
        assert_eq!(
            saved,
            fs::read(workspace.join("state/agent-management.json")).unwrap()
        );
    }
    unsafe {
        libc::kill(server.id() as i32, libc::SIGINT);
    }
    assert!(server.wait().unwrap().success());
    assert!(!socket.exists());
    let mut restarted = spawn();
    wait_ready(&socket);
    let view = agent_management::inventory_arguments(
        &InventoryRequest {
            socket: socket.to_str().unwrap().into(),
        },
        true,
    )
    .unwrap();
    let view: Value = serde_json::from_str(&view.stdout).unwrap();
    assert_eq!(view["records"][0]["reported_state"], "unknown");
    assert!(view["records"][0]["age_ms"].is_null());
    unsafe {
        libc::kill(restarted.id() as i32, libc::SIGINT);
    }
    assert!(restarted.wait().unwrap().success());
    assert_eq!(
        saved,
        fs::read(workspace.join("state/agent-management.json")).unwrap()
    );
    assert_eq!(fs::read(workspace.join("coven.json")).unwrap(), b"marker");
}

#[test]
fn reporter_discards_secrets_before_wire_and_rejects_peer_extensions() {
    let root = private_root();
    let path = root.path().join("receiver.sock");
    let listener = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert!(!line.contains("SECRET"));
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["contract"], "coven.agent-management.v1");
        assert_eq!(request["method"], "report");
        assert_eq!(request["params"].as_object().unwrap().len(), 3);
        let reply = json!({"id":request["id"],"contract":"coven.agent-management.v1","type":"result","result":{"type":"accepted","record_id":"a".repeat(64),"delivery":"committed","extra":"PEER_SECRET"}});
        writeln!(stream, "{reply}").unwrap();
    });
    let request = ReportRequest {
        socket: path.to_str().unwrap().into(),
        provider: "claude".into(),
        event: None,
    };
    let result = agent_management::report_payload(&request, br#"{"session_id":"session","hook_event_name":"Stop","prompt":"SECRET","cwd":"SECRET","transcript_path":"SECRET"}"#, true);
    assert!(result.is_err());
    assert!(!result.err().unwrap().to_string().contains("PEER_SECRET"));
    worker.join().unwrap();
}

#[test]
fn projection_slow_clients_duration_and_owned_inode_cleanup() {
    let root = private_root();
    fs::write(root.path().join("coven.json"), b"marker").unwrap();
    let failed_socket = root.path().join("failed-startup.sock");
    assert!(
        agent_management::serve_arguments(
            &ServeRequest {
                workspace: root.path().join("missing-workspace"),
                socket: failed_socket.to_str().unwrap().into(),
                duration: 1,
            },
            true
        )
        .is_err()
    );
    assert!(!failed_socket.exists());
    // The failed startup must have restored SIGINT ownership in this process.
    let socket = root.path().join("receiver.sock");
    let shared = Arc::new(Mutex::new(Value::Null));
    let projection = shared.clone();
    let request = ServeRequest {
        workspace: root.path().to_owned(),
        socket: socket.to_str().unwrap().into(),
        duration: 1,
    };
    let restart = request.clone();
    let worker = thread::spawn(move || {
        agent_management::serve_with_projection(&request, true, Some(projection))
    });
    wait_ready(&socket);
    let mut slow = UnixStream::connect(&socket).unwrap();
    slow.write_all(b"{").unwrap();
    thread::sleep(Duration::from_millis(225));
    let request = ReportRequest {
        socket: socket.to_str().unwrap().into(),
        provider: "claude".into(),
        event: None,
    };
    agent_management::report_payload(
        &request,
        br#"{"session_id":"s","hook_event_name":"Stop"}"#,
        true,
    )
    .unwrap();
    let first: Value = shared.lock().unwrap().clone();
    assert_eq!(first["records"][0]["reported_state"], "idle");
    thread::sleep(Duration::from_millis(30));
    assert!(
        shared.lock().unwrap()["records"][0]["age_ms"]
            .as_u64()
            .unwrap()
            >= first["records"][0]["age_ms"].as_u64().unwrap()
    );
    let inventory = InventoryRequest {
        socket: socket.to_str().unwrap().into(),
    };
    for (event, state) in [
        ("SessionStart", "unknown"),
        ("UserPromptSubmit", "working"),
        ("PreToolUse", "working"),
        ("PostToolUse", "working"),
        ("Stop", "idle"),
        ("SessionEnd", "ended"),
    ] {
        let payload = json!({"session_id":"s","hook_event_name":event,"prompt":"SECRET"});
        agent_management::report_payload(&request, payload.to_string().as_bytes(), true).unwrap();
        let view = agent_management::inventory_arguments(&inventory, true).unwrap();
        let view: Value = serde_json::from_str(&view.stdout).unwrap();
        assert_eq!(view["records"][0]["reported_state"], state);
        assert_eq!(view["records"][0]["job_completion"], "unestablished");
    }
    worker.join().unwrap().unwrap();
    assert!(!socket.exists());
    let before = fs::read(root.path().join("state/agent-management.json")).unwrap();
    assert!(!String::from_utf8_lossy(&before).contains("SECRET"));
    let worker = thread::spawn(move || agent_management::serve_arguments(&restart, true));
    wait_ready(&socket);
    let view = agent_management::inventory_arguments(&inventory, true).unwrap();
    let view: Value = serde_json::from_str(&view.stdout).unwrap();
    assert_eq!(view["records"][0]["reported_state"], "unknown");
    assert!(view["records"][0]["age_ms"].is_null());
    worker.join().unwrap().unwrap();
    assert!(!socket.exists());
    assert_eq!(
        before,
        fs::read(root.path().join("state/agent-management.json")).unwrap()
    );
}

#[test]
fn malformed_peer_acknowledgements_are_uncertain_and_never_retried() {
    let root = private_root();
    let path = root.path().join("receiver.sock");
    let cases = [
        (
            "report",
            json!({"type":"error","wrong":"PEER_SECRET"}),
            "agent_delivery_uncertain",
        ),
        (
            "report",
            json!({"type":"error","error":"PEER_SECRET"}),
            "agent_delivery_uncertain",
        ),
        (
            "report",
            json!({"type":"error","error":null}),
            "agent_delivery_uncertain",
        ),
        (
            "report",
            json!({"type":"error","error":{}}),
            "agent_delivery_uncertain",
        ),
        (
            "report",
            json!({"type":"error","error":{"code":false}}),
            "agent_delivery_uncertain",
        ),
        (
            "report",
            json!({"type":"error","error":{"code":""}}),
            "agent_delivery_uncertain",
        ),
        (
            "report",
            json!({"type":"error","error":{"code":"PEER_SECRET\n"}}),
            "agent_delivery_uncertain",
        ),
        (
            "report",
            json!({"type":"error","error":{"code":"x".repeat(65)}}),
            "agent_delivery_uncertain",
        ),
        (
            "report",
            json!({"type":"error","error":{"code":"safe","message":"PEER_SECRET"}}),
            "agent_delivery_uncertain",
        ),
        (
            "report",
            json!({"type":"error","error":{"code":"safe"},"result":{}}),
            "agent_delivery_uncertain",
        ),
        (
            "report",
            json!({"type":"error","error":{"code":"safe.code_1-2"}}),
            "agent_report_rejected",
        ),
        (
            "report",
            json!({"type":"result","result":null}),
            "agent_delivery_uncertain",
        ),
        (
            "report",
            json!({"type":"result","result":{"type":"accepted","record_id":false,"delivery":"committed"}}),
            "agent_delivery_uncertain",
        ),
        (
            "report",
            json!({"type":"result","result":{"type":"accepted","record_id":"a".repeat(64),"delivery":null}}),
            "agent_delivery_uncertain",
        ),
        (
            "inventory",
            json!({"type":"result","result":{"contract":"coven.agent-management.v1","server_epoch":false,"records":[],"authorization":"none"}}),
            "agent_delivery_uncertain",
        ),
        (
            "report",
            json!({"contract":"wrong","type":"error","error":{"code":"safe"}}),
            "agent_delivery_uncertain",
        ),
        (
            "report",
            json!({"id":"wrong","type":"error","error":{"code":"safe"}}),
            "agent_delivery_uncertain",
        ),
    ];
    for (method, mut reply, expected) in cases {
        let listener = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            reply
                .as_object_mut()
                .unwrap()
                .entry("id")
                .or_insert(request["id"].clone());
            reply
                .as_object_mut()
                .unwrap()
                .entry("contract")
                .or_insert(json!("coven.agent-management.v1"));
            writeln!(stream, "{reply}").unwrap();
            listener.set_nonblocking(true).unwrap();
            thread::sleep(Duration::from_millis(10));
            assert_eq!(
                listener.accept().err().unwrap().kind(),
                std::io::ErrorKind::WouldBlock
            );
        });
        let result = if method == "report" {
            agent_management::report_payload(
                &ReportRequest {
                    socket: path.to_str().unwrap().into(),
                    provider: "claude".into(),
                    event: None,
                },
                br#"{"session_id":"s","hook_event_name":"Stop"}"#,
                true,
            )
        } else {
            agent_management::inventory_arguments(
                &InventoryRequest {
                    socket: path.to_str().unwrap().into(),
                },
                true,
            )
        };
        assert_eq!(result.err().unwrap().code(), expected);
        worker.join().unwrap();
        fs::remove_file(&path).unwrap();
    }
}

#[test]
fn sigint_during_startup_lock_wait_cleans_socket_and_allows_fresh_serve() {
    let root = private_root();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    fs::write(workspace.join("coven.json"), b"marker").unwrap();
    let socket = root.path().join("receiver.sock");
    let guard = coven::storage::WorkspaceGuard::acquire(&workspace, Duration::ZERO).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_coven"))
        .args(["agent", "serve"])
        .arg(&workspace)
        .arg("--socket")
        .arg(&socket)
        .args(["--duration", "60"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_ready(&socket);
    unsafe {
        libc::kill(child.id() as i32, libc::SIGINT);
    }
    // A startup refusal is allowed, but SIGINT must not bypass owned-socket cleanup.
    assert!(child.wait().unwrap().code().is_some());
    assert!(!socket.exists());
    assert!(!workspace.join("state/agent-management.json").exists());
    drop(guard);
    let mut fresh = Command::new(env!("CARGO_BIN_EXE_coven"))
        .args(["agent", "serve"])
        .arg(&workspace)
        .arg("--socket")
        .arg(&socket)
        .args(["--duration", "1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_ready(&socket);
    agent_management::inventory_arguments(
        &InventoryRequest {
            socket: socket.to_str().unwrap().into(),
        },
        true,
    )
    .unwrap();
    assert!(fresh.wait().unwrap().success());
    assert!(!socket.exists());
}

#[test]
fn second_workspace_receiver_is_refused_without_disturbing_first() {
    let root = private_root();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    fs::write(workspace.join("coven.json"), b"marker").unwrap();
    let socket_a = root.path().join("a.sock");
    let socket_b = root.path().join("b.sock");
    let mut first = Command::new(env!("CARGO_BIN_EXE_coven"))
        .args(["agent", "serve"])
        .arg(&workspace)
        .arg("--socket")
        .arg(&socket_a)
        .args(["--duration", "60"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_ready(&socket_a);
    let report_a = ReportRequest {
        socket: socket_a.to_str().unwrap().into(),
        provider: "claude".into(),
        event: None,
    };
    agent_management::report_payload(
        &report_a,
        br#"{"session_id":"same","hook_event_name":"PreToolUse"}"#,
        true,
    )
    .unwrap();
    let before = fs::read(workspace.join("state/agent-management.json")).unwrap();
    let lease = workspace.join("state/.agent-management.receiver.lock");
    let inode = fs::metadata(&lease).unwrap().ino();
    let second = Command::new(env!("CARGO_BIN_EXE_coven"))
        .args(["agent", "serve"])
        .arg(&workspace)
        .arg("--socket")
        .arg(&socket_b)
        .args(["--duration", "1"])
        .output()
        .unwrap();
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("agent_receiver_busy"));
    assert!(!socket_b.exists());
    let report_b = ReportRequest {
        socket: socket_b.to_str().unwrap().into(),
        ..report_a.clone()
    };
    assert!(
        agent_management::report_payload(
            &report_b,
            br#"{"session_id":"same","hook_event_name":"Stop"}"#,
            true
        )
        .is_err()
    );
    assert_eq!(
        before,
        fs::read(workspace.join("state/agent-management.json")).unwrap()
    );
    assert_eq!(fs::metadata(&lease).unwrap().ino(), inode);
    let view = agent_management::inventory_arguments(
        &InventoryRequest {
            socket: report_a.socket.clone(),
        },
        true,
    )
    .unwrap();
    let view: Value = serde_json::from_str(&view.stdout).unwrap();
    assert_eq!(view["records"][0]["reported_state"], "working");
    agent_management::report_payload(
        &report_a,
        br#"{"session_id":"same","hook_event_name":"Stop"}"#,
        true,
    )
    .unwrap();
    unsafe {
        libc::kill(first.id() as i32, libc::SIGINT);
    }
    assert!(first.wait().unwrap().success());
    assert!(!socket_a.exists());
    assert_eq!(fs::metadata(&lease).unwrap().ino(), inode);
}
