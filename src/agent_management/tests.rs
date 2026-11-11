use super::*;
use std::{
    fs,
    os::unix::{
        fs::{MetadataExt, PermissionsExt, symlink},
        net::UnixListener,
    },
};

fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("coven.json"), b"marker").unwrap();
    root
}

fn params(sid: &str, event: &str) -> Value {
    json!({"provider":"claude","event":event,"session_id":sid})
}

#[test]
fn provider_ids_restart_and_shared_capacity() {
    use sha2::{Digest, Sha256};
    let root = workspace();
    let mut store = store::Store::open(root.path()).unwrap();
    let claude = store.report(&params("same", "PreToolUse")).unwrap()["record_id"].clone();
    assert_eq!(claude, format!("{:x}", Sha256::digest(b"claude\0same")));
    let codex_params = json!({"provider":"codex","event":"SessionStart","session_id":"same"});
    let codex = store.report(&codex_params).unwrap()["record_id"].clone();
    assert_eq!(codex, format!("{:x}", Sha256::digest(b"codex\0same")));
    assert_ne!(claude, codex);
    let copilot_params = json!({"provider":"copilot","event":"SessionStart","session_id":"same"});
    let copilot = store.report(&copilot_params).unwrap()["record_id"].clone();
    assert_eq!(copilot, format!("{:x}", Sha256::digest(b"copilot\0same")));
    assert_ne!(copilot, claude);
    assert_ne!(copilot, codex);
    let projection = store.projection();
    let fresh = projection["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["provider"] == "copilot")
        .unwrap();
    assert_eq!(fresh["reported_state"], "unknown");
    assert!(fresh["age_ms"].is_u64());
    drop(store);
    let mut store = store::Store::open(root.path()).unwrap();
    for record in store.projection()["records"].as_array().unwrap() {
        assert_eq!(record["reported_state"], "unknown");
        assert!(record["age_ms"].is_null());
    }
    assert_eq!(
        store.report(&params("same", "Stop")).unwrap()["record_id"],
        claude
    );
    assert_eq!(store.report(&codex_params).unwrap()["record_id"], codex);
    assert_eq!(store.report(&copilot_params).unwrap()["record_id"], copilot);
    for n in 3..MAX_RECORDS {
        store.report(&json!({"provider":(["claude", "codex", "copilot"][n % 3]),"event":"SessionStart","session_id":format!("s-{n}")})).unwrap();
    }
    let before = fs::read(root.path().join("state/agent-management.json")).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&before).unwrap()["schema_version"],
        1
    );
    for provider in ["claude", "codex", "copilot"] {
        assert_eq!(
            store
                .report(
                    &json!({"provider":provider,"event":"SessionStart","session_id":"overflow"})
                )
                .unwrap_err()
                .code(),
            "agent_capacity"
        );
    }
    assert_eq!(
        before,
        fs::read(root.path().join("state/agent-management.json")).unwrap()
    );
}

#[test]
fn copilot_strict_pascalcase_payloads_never_infer_or_connect() {
    let probe = crate::platform::observation_socket::SocketConstructionGuard::new(true);
    let mut request = ReportRequest {
        socket: "not-a-socket".into(),
        provider: "copilot".into(),
        event: Some("SessionStart".into()),
    };
    for payload in [
        br#"{"session_id":"s"}"#.as_slice(),
        br#"{"sessionId":"s","hook_event_name":"SessionStart"}"#,
        br#"{"sessionId":"s"}"#,
        br#"{"session_id":"s","hook_event_name":"sessionStart"}"#,
        br#"{"session_id":"s","hook_event_name":"SubagentStop","agent_id":"sub"}"#,
        br#"{"session_id":"s","hook_event_name":"Stop"}"#,
        br#"{"session_id":"s","hook_event_name":"PreToolUse"}"#,
        br#"{"session_id":null,"hook_event_name":"SessionStart"}"#,
        br#"{"session_id":"","hook_event_name":"SessionStart"}"#,
        br#"{"session_id":"s","hook_event_name":null}"#,
        br#"{"session_id":"s","session_id":"other","hook_event_name":"SessionStart"}"#,
    ] {
        assert!(report_payload(&request, payload, true).is_err());
    }
    request.event = None;
    assert!(report_payload(&request, br#"{"session_id":"s"}"#, true).is_err());
    assert_eq!(probe.count(), 0);
}

#[test]
fn copilot_capture_strips_native_fields_before_rpc_store_and_stdout() {
    use std::io::{BufRead, BufReader, Write};
    let root = workspace();
    let socket_root = tempfile::tempdir().unwrap();
    fs::set_permissions(socket_root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let path = socket_root.path().join("receiver.sock");
    let listener = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let workspace_path = root.path().to_owned();
    let peer = std::thread::spawn(move || {
        let mut store = store::Store::open(&workspace_path).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let mut frame = String::new();
        BufReader::new(&mut stream).read_line(&mut frame).unwrap();
        let request: Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(
            request["params"],
            json!({"provider":"copilot","event":"SessionStart","session_id":"reference-only"})
        );
        assert!(!frame.contains("SECRET"));
        let result = store.report(&request["params"]).unwrap();
        let reply = json!({"id":request["id"],"contract":CONTRACT,"type":"result","result":result});
        writeln!(stream, "{reply}").unwrap();
        let projection = store.projection();
        assert_eq!(projection["records"][0]["reported_state"], "unknown");
        assert!(projection["records"][0]["age_ms"].is_u64());
        assert!(!projection.to_string().contains("reference-only"));
    });
    let request = ReportRequest {
        socket: path.to_str().unwrap().into(),
        provider: "copilot".into(),
        event: Some("SessionStart".into()),
    };
    // Documentation-only evidence: github/docs at 7c3387d3b513ed43ea26c9b2e4c0e1b12234c010,
    // content/copilot/reference/hooks-reference.md, PascalCase SessionStart payload.
    let payload = json!({"session_id":"reference-only","hook_event_name":"SessionStart",
        "cwd":"SECRET-CWD","transcript_path":"SECRET-TRANSCRIPT","prompt":"SECRET-PROMPT",
        "initial_prompt":"SECRET-INITIAL","native_identity":"SECRET-NATIVE",
        "agent_id":{"arbitrary":"SECRET-AGENT"},"sessionId":"SECRET-ALIAS",
        "timestamp":"SECRET-TIME","source":"SECRET-SOURCE"});
    let output = report_payload(&request, payload.to_string().as_bytes(), true).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&output.stdout).unwrap()["type"],
        "accepted"
    );
    assert!(!output.stdout.contains("SECRET"));
    assert!(!output.stdout.contains("reference-only"));
    peer.join().unwrap();
    let saved = fs::read(root.path().join("state/agent-management.json")).unwrap();
    assert!(!String::from_utf8(saved.clone()).unwrap().contains("SECRET"));
    let saved: Value = serde_json::from_slice(&saved).unwrap();
    assert_eq!(saved["schema_version"], 1);
    let record = saved["records"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(record.as_object().unwrap().len(), 5);
    assert_eq!(record["session_id"], "reference-only");
    let restarted = store::Store::open(root.path()).unwrap().projection();
    assert_eq!(restarted["records"][0]["reported_state"], "unknown");
    assert!(restarted["records"][0]["age_ms"].is_null());
}

#[test]
fn copilot_store_refuses_invalid_pairs_and_extra_rpc_or_persisted_fields() {
    let root = workspace();
    let mut store = store::Store::open(root.path()).unwrap();
    let valid = json!({"provider":"copilot","event":"SessionStart","session_id":"s"});
    store.report(&valid).unwrap();
    let path = root.path().join("state/agent-management.json");
    let before = fs::read(&path).unwrap();
    for (field, value) in [
        ("event", json!("sessionStart")),
        ("event", json!("SubagentStop")),
        ("event", json!("Stop")),
        ("event", Value::Null),
        ("session_id", Value::Null),
        ("cwd", json!("SECRET")),
        ("reported_state", json!("working")),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        assert!(store.report(&bad).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    drop(store);
    let saved: Value = serde_json::from_slice(&before).unwrap();
    for (field, value) in [
        ("last_event", json!("Stop")),
        ("last_event", json!("sessionStart")),
        ("last_event", json!("SubagentStop")),
        ("native_identity", json!("SECRET")),
    ] {
        let mut bad = saved.clone();
        bad["records"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap()[field] = value;
        fs::write(&path, bad.to_string()).unwrap();
        assert!(store::Store::open(root.path()).is_err());
    }
    fs::write(&path, &before).unwrap();
    assert!(store::Store::open(root.path()).is_ok());
}

#[test]
fn codex_invalid_payloads_never_connect() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("receiver.sock");
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let mut request = ReportRequest {
        socket: path.to_str().unwrap().into(),
        provider: "codex".into(),
        event: None,
    };
    for payload in [
        json!({"thread_id":"s","hook_event_name":"SessionStart"}),
        json!({"thread-id":"s","hook_event_name":"SessionStart"}),
        json!({"session_id":"s"}),
        json!({"session_id":"s","hook_event_name":"Stop"}),
        json!({"session_id":"s","hook_event_name":"SubagentStop","agent_id":"sub"}),
        json!({"session_id":"s","hook_event_name":"UserPromptSubmit"}),
    ] {
        assert!(report_payload(&request, payload.to_string().as_bytes(), true).is_err());
    }
    request.event = Some("Stop".into());
    assert_eq!(
        report_payload(
            &request,
            br#"{"session_id":"s","hook_event_name":"SessionStart"}"#,
            true
        )
        .err()
        .unwrap()
        .code(),
        "agent_event_mismatch"
    );
    assert_eq!(
        listener.accept().err().unwrap().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn store_load_rejects_unknown_provider_and_provider_event_pairs() {
    let root = workspace();
    let mut store = store::Store::open(root.path()).unwrap();
    store
        .report(&json!({"provider":"codex","event":"SessionStart","session_id":"s"}))
        .unwrap();
    drop(store);
    let path = root.path().join("state/agent-management.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for (field, value) in [
        ("provider", "unknown"),
        ("last_event", "Stop"),
        ("last_event", "Other"),
    ] {
        let mut bad = original.clone();
        bad["records"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap()[field] = json!(value);
        fs::write(&path, bad.to_string()).unwrap();
        assert!(store::Store::open(root.path()).is_err());
    }
}

#[test]
fn events_persistence_restart_capacity_and_private_storage() {
    let root = workspace();
    let mut store = store::Store::open(root.path()).unwrap();
    for (name, state) in [
        ("SessionStart", "unknown"),
        ("UserPromptSubmit", "working"),
        ("PreToolUse", "working"),
        ("PostToolUse", "working"),
        ("Stop", "idle"),
        ("SessionEnd", "ended"),
    ] {
        let accepted = store.report(&params("native-session", name)).unwrap();
        assert_eq!(accepted["record_id"].as_str().unwrap().len(), 64);
        let projection = store.projection();
        assert_eq!(projection["records"][0]["reported_state"], state);
        assert!(projection["records"][0]["age_ms"].is_u64());
        assert!(!projection.to_string().contains("native-session"));
    }
    let epoch = store.epoch.clone();
    drop(store);
    let mut store = store::Store::open(root.path()).unwrap();
    assert_ne!(epoch, store.epoch);
    assert_eq!(
        store.projection()["records"][0]["reported_state"],
        "unknown"
    );
    assert!(store.projection()["records"][0]["age_ms"].is_null());
    for n in 1..MAX_RECORDS {
        store
            .report(&params(&format!("session-{n}"), "Stop"))
            .unwrap();
    }
    let before = fs::read(root.path().join("state/agent-management.json")).unwrap();
    assert_eq!(
        store
            .report(&params("overflow", "Stop"))
            .unwrap_err()
            .code(),
        "agent_capacity"
    );
    assert_eq!(
        before,
        fs::read(root.path().join("state/agent-management.json")).unwrap()
    );
    assert_eq!(
        fs::metadata(root.path().join("state/agent-management.json"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(fs::read(root.path().join("coven.json")).unwrap(), b"marker");
}

#[test]
fn malformed_hooks_and_subagents_never_connect() {
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.path().join("receiver.sock");
    let _listener = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let probe = crate::platform::observation_socket::SocketConstructionGuard::new(true);
    let request = ReportRequest {
        socket: path.to_str().unwrap().into(),
        provider: "claude".into(),
        event: None,
    };
    for payload in [
        br#"{"session_id":"x","session_id":"y","hook_event_name":"Stop"}"#.as_slice(),
        br#"{"session_id":"","hook_event_name":"Stop"}"#,
        br#"{"session_id":"   ","hook_event_name":"Stop"}"#,
        br#"{"session_id":"-option","hook_event_name":"Stop"}"#,
        br#"{"session_id":"  -option","hook_event_name":"Stop"}"#,
        br#"{"session_id":"bad\nvalue","hook_event_name":"Stop"}"#,
        br#"{"session_id":"x","hook_event_name":"Other"}"#,
        br#"{"session_id":"x","hook_event_name":"Stop","agent_id":3}"#,
        &[0xff],
    ] {
        assert!(report_payload(&request, payload, true).is_err());
    }
    assert!(report_payload(&request, &vec![b' '; MAX_PAYLOAD + 1], true).is_err());
    let deep = format!("{}0{}", "[".repeat(33), "]".repeat(33));
    assert!(report_payload(&request, deep.as_bytes(), true).is_err());
    let mut mismatch = request.clone();
    mismatch.event = Some("Stop".into());
    assert_eq!(
        report_payload(
            &mismatch,
            br#"{"session_id":"x","hook_event_name":"SessionStart"}"#,
            true
        )
        .err()
        .unwrap()
        .code(),
        "agent_event_mismatch"
    );
    for payload in [
        br#"{"session_id":"x","hook_event_name":"Stop","agent_id":"sub"}"#.as_slice(),
        br#"{"session_id":"x","hook_event_name":"SubagentStop"}"#,
    ] {
        assert!(
            report_payload(&request, payload, true)
                .unwrap()
                .stdout
                .contains("ignored")
        );
    }
    assert_eq!(probe.count(), 0);
}

#[test]
fn receiver_lease_is_exclusive_persistent_and_not_the_workspace_guard() {
    let root = workspace();
    let mut receiver = store::Store::open(root.path()).unwrap();
    receiver
        .report(&params("same-session", "PreToolUse"))
        .unwrap();
    assert_eq!(
        store::Store::open(root.path()).err().unwrap().code(),
        "agent_receiver_busy"
    );
    assert_eq!(
        receiver.projection()["records"][0]["reported_state"],
        "working"
    );
    // Other Coven writers can still acquire the ordinary cooperative mutation guard.
    drop(crate::storage::WorkspaceGuard::existing(root.path(), std::time::Duration::ZERO).unwrap());
    let lease = root.path().join("state/.agent-management.receiver.lock");
    let inode = fs::metadata(&lease).unwrap().ino();
    let guard =
        crate::storage::WorkspaceGuard::existing(root.path(), std::time::Duration::ZERO).unwrap();
    assert_eq!(
        crate::storage::atomic_replace_bytes(
            &guard,
            std::path::Path::new("state/.agent-management.receiver.lock"),
            b"must-not-replace-active-lease",
        )
        .err()
        .unwrap()
        .code(),
        "unsafe_replacement_lock"
    );
    drop(guard);
    assert_eq!(fs::metadata(&lease).unwrap().ino(), inode);
    receiver
        .report(&params("same-session", "PreToolUse"))
        .unwrap();
    fs::write(&lease, b"lease-sentinel").unwrap();
    drop(receiver);
    assert_eq!(fs::metadata(&lease).unwrap().ino(), inode);
    let mut receiver = store::Store::open(root.path()).unwrap();
    receiver.report(&params("same-session", "Stop")).unwrap();
    assert_eq!(
        receiver.projection()["records"][0]["reported_state"],
        "idle"
    );
    assert_eq!(fs::metadata(&lease).unwrap().ino(), inode);
    assert_eq!(fs::metadata(&lease).unwrap().mode() & 0o7777, 0o600);
    assert_eq!(fs::read(&lease).unwrap(), b"lease-sentinel");
    fs::rename(&lease, root.path().join("original-lease")).unwrap();
    fs::write(&lease, b"foreign").unwrap();
    assert!(
        receiver
            .report(&params("same-session", "PreToolUse"))
            .is_err()
    );
    drop(receiver);
    assert_eq!(fs::read(&lease).unwrap(), b"foreign");
}

#[test]
fn persisted_reference_schema_is_explicit_and_unknown_versions_are_refused() {
    let root = workspace();
    let mut receiver = store::Store::open(root.path()).unwrap();
    receiver
        .report(&params("schema-session", "SessionStart"))
        .unwrap();
    drop(receiver);
    let path = root.path().join("state/agent-management.json");
    let original = fs::read(&path).unwrap();
    let value: Value = serde_json::from_slice(&original).unwrap();
    assert_eq!(value["schema_version"], 1);
    for version in [json!(0), json!(2), Value::Null, json!("1")] {
        let mut invalid = value.clone();
        invalid["schema_version"] = version;
        fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(store::Store::open(root.path()).is_err());
    }
    fs::write(&path, &original).unwrap();
    let receiver = store::Store::open(root.path()).unwrap();
    assert_eq!(
        receiver.projection()["records"][0]["reported_state"],
        "unknown"
    );
}

#[test]
fn receiver_lease_rejects_symlinks_hardlinks_and_nonprivate_leaves() {
    let root = workspace();
    drop(store::Store::open(root.path()).unwrap());
    let lease = root.path().join("state/.agent-management.receiver.lock");
    let original = root.path().join("original-lease");
    fs::rename(&lease, &original).unwrap();
    symlink(&original, &lease).unwrap();
    assert!(store::Store::open(root.path()).is_err());
    fs::remove_file(&lease).unwrap();
    fs::hard_link(&original, &lease).unwrap();
    assert!(store::Store::open(root.path()).is_err());
    fs::remove_file(&lease).unwrap();
    fs::rename(&original, &lease).unwrap();
    fs::set_permissions(&lease, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store::Store::open(root.path()).is_err());
    assert_eq!(fs::metadata(&lease).unwrap().mode() & 0o777, 0o644);
    assert!(!root.path().join("state/agent-management.json").exists());
}

#[test]
fn store_symlinks_hardlinks_modes_and_replaced_state_are_fenced() {
    let root = workspace();
    let mut store = store::Store::open(root.path()).unwrap();
    let target = root.path().join("state/agent-management.json");
    let foreign = root.path().join("foreign");
    fs::write(&foreign, b"secret").unwrap();
    symlink(&foreign, &target).unwrap();
    assert!(store.report(&params("x", "Stop")).is_err());
    fs::remove_file(&target).unwrap();
    fs::hard_link(&foreign, &target).unwrap();
    assert!(store.report(&params("x", "Stop")).is_err());
    fs::remove_file(&target).unwrap();
    store.report(&params("x", "Stop")).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.report(&params("x", "Stop")).is_err());
    fs::rename(root.path().join("state"), root.path().join("old-state")).unwrap();
    fs::create_dir(root.path().join("state")).unwrap();
    assert_eq!(
        store.report(&params("x", "Stop")).unwrap_err().code(),
        "agent_store_replaced"
    );
    assert_eq!(fs::read(&foreign).unwrap(), b"secret");
    assert!(!root.path().join("state/.workspace.lock").exists());
}

#[test]
fn hardlinked_or_replaced_workspace_lock_is_refused() {
    let root = workspace();
    fs::create_dir(root.path().join("state")).unwrap();
    let foreign = root.path().join("foreign");
    fs::write(&foreign, b"unchanged").unwrap();
    let lock = root.path().join("state/.workspace.lock");
    fs::hard_link(&foreign, &lock).unwrap();
    assert!(store::Store::open(root.path()).is_err());
    assert_eq!(fs::read(&foreign).unwrap(), b"unchanged");
    fs::remove_file(&lock).unwrap();
    let mut store = store::Store::open(root.path()).unwrap();
    fs::rename(&lock, root.path().join("original-lock")).unwrap();
    fs::write(&lock, b"foreign-lock").unwrap();
    assert!(store.report(&params("s", "Stop")).is_err());
    assert_eq!(fs::read(&lock).unwrap(), b"foreign-lock");
    assert!(!root.path().join("state/agent-management.json").exists());
}

#[test]
fn unsafe_endpoints_duration_and_workspace_refused() {
    let root = workspace();
    let socket = root.path().join("receiver.sock");
    let request = ServeRequest {
        workspace: root.path().to_owned(),
        socket: socket.to_str().unwrap().into(),
        duration: 0,
    };
    assert_eq!(
        serve_arguments(&request, true).err().unwrap().code(),
        "agent_invalid_duration"
    );
    assert!(!socket.exists());
    let mut request = request;
    request.duration = 3601;
    assert!(serve_arguments(&request, true).is_err());
    request.duration = 1;
    fs::write(&socket, b"foreign").unwrap();
    assert!(serve_arguments(&request, true).is_err());
    assert_eq!(fs::read(&socket).unwrap(), b"foreign");
    fs::remove_file(&socket).unwrap();
    let stale = UnixListener::bind(&socket).unwrap();
    drop(stale);
    assert!(serve_arguments(&request, true).is_err());
    fs::remove_file(&socket).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o777)).unwrap();
    assert!(serve_arguments(&request, true).is_err());
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_file(root.path().join("coven.json")).unwrap();
    assert!(serve_arguments(&request, true).is_err());
    assert!(!socket.exists());
    assert!(!root.path().join("state").exists());
}
