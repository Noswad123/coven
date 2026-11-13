#![cfg(any(target_os = "linux", target_os = "macos"))]
//! Private fake Kit plus receiver child: no live apps or process-global test signals.
#[path = "support/physical_temp.rs"]
mod physical_temp;
use coven::{
    agent_management::{self, InventoryRequest, ReportRequest, ServeRequest},
    agent_management_presentation::{self, PresentationRequest},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[test]
fn receiver_child() {
    let Ok(root) = std::env::var("COVEN_AGENT_VIEW_TEST_ROOT") else {
        return;
    };
    let output = agent_management_presentation::serve_arguments(
        &ServeRequest {
            workspace: root.clone().into(),
            socket: format!("{root}/receiver.sock"),
            duration: 2,
        },
        &PresentationRequest {
            socket: format!("{root}/kit.sock"),
            source: "queueSource2".into(),
        },
        true,
    )
    .unwrap();
    assert_eq!(output.exit_code, 0);
    fs::write(format!("{root}/result.json"), output.stdout).unwrap();
}

struct Receiver(Child);

#[test]
fn normal_cli_reports_before_eof_and_joins_on_eof_or_interrupt() {
    for ending in [0, libc::SIGINT, libc::SIGTERM] {
        let root = physical_temp::tempdir();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.path().join("coven.json"), "{}").unwrap();
        let kit = Kit::start(root.path(), "success");
        let path = root.path().join("native.sock");
        let mut receiver = Receiver(
            Command::new(env!("CARGO_BIN_EXE_coven"))
                .args(["agent", "run"])
                .arg(root.path())
                .arg("--socket")
                .arg(&path)
                .arg("--kitsune-socket")
                .arg(root.path().join("kit.sock"))
                .args(["--source", "queueSource2", "--json"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let end = Instant::now() + Duration::from_secs(5);
        while !path.exists() {
            assert!(Instant::now() < end);
            thread::sleep(Duration::from_millis(5));
        }
        let mut reporter = Command::new(env!("CARGO_BIN_EXE_coven"))
            .args(["agent", "report-native", "--socket"])
            .arg(&path)
            .arg("--json")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let payload = json!({"contract":"djinn.native-events.v1","instance_id":"12345678-1234-1234-1234-123456789abc","session_id":"PRIVATE-NATIVE-ID","sequence":1,"event":"session.status","status":"busy"});
        writeln!(reporter.stdin.take().unwrap(), "{payload}").unwrap();
        let report = reporter.wait_with_output().unwrap();
        assert!(
            report.status.success(),
            "{}",
            String::from_utf8_lossy(&report.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&report.stdout).unwrap(),
            json!({"type":"accepted","delivery":"volatile"})
        );
        loop {
            let calls = kit.calls.lock().unwrap();
            if calls.iter().any(|c| {
                c["method"] == "integration.view.replace"
                    && c["params"]["items"].to_string().contains("WORKING")
            }) {
                break;
            }
            drop(calls);
            assert!(
                Instant::now() < end,
                "normal report was not presented before EOF"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(receiver.0.try_wait().unwrap().is_none());
        let calls = kit.calls.lock().unwrap().clone();
        assert!(!json!(calls).to_string().contains("PRIVATE-NATIVE-ID"));
        assert!(
            calls
                .iter()
                .any(|c| c["params"]["items"].to_string().contains("Coven"))
        );
        assert!(
            calls
                .iter()
                .any(|c| c["params"]["items"].to_string().contains("Djinn"))
        );
        assert!(!root.path().join("state/agent-management.json").exists());
        if ending == 0 {
            // Keeping the attached presenter alive must not extend evidence.
            // A duplicate is rejected and cannot renew the original receipt.
            assert!(
                agent_management::native_report_payload(
                    path.to_str().unwrap(),
                    payload.to_string().as_bytes(),
                    true
                )
                .is_err()
            );
            let expiry_end = Instant::now() + Duration::from_secs(32);
            loop {
                let calls = kit.calls.lock().unwrap();
                if calls.iter().any(|c| {
                    c["method"] == "integration.view.replace"
                        && c["params"]["items"]
                            .to_string()
                            .contains("UNKNOWN (expired)")
                }) {
                    break;
                }
                drop(calls);
                assert!(
                    Instant::now() < expiry_end,
                    "evidence was renewed by repainting"
                );
                thread::sleep(Duration::from_millis(20));
            }
            assert!(receiver.0.try_wait().unwrap().is_none());
            agent_management::report_payload(
                &ReportRequest {
                    socket: path.to_string_lossy().into_owned(),
                    provider: "claude".into(),
                    event: None,
                },
                br#"{"session_id":"historical-ref","hook_event_name":"Stop"}"#,
                true,
            )
            .unwrap();
        }
        if ending == 0 {
            drop(receiver.0.stdin.take());
        } else {
            assert_eq!(unsafe { libc::kill(receiver.0.id() as i32, ending) }, 0);
        }
        receiver.finish();
        assert!(!path.exists());
        assert!(
            kit.calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c["method"] == "integration.view.close")
        );
        if ending == 0 {
            let before = kit.calls.lock().unwrap().len();
            let mut restarted = Receiver(
                Command::new(env!("CARGO_BIN_EXE_coven"))
                    .args(["agent", "run"])
                    .arg(root.path())
                    .arg("--socket")
                    .arg(&path)
                    .arg("--kitsune-socket")
                    .arg(root.path().join("kit.sock"))
                    .args(["--source", "queueSource2", "--json"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap(),
            );
            let end = Instant::now() + Duration::from_secs(5);
            loop {
                let calls = kit.calls.lock().unwrap();
                if let Some(frame) = calls[before..]
                    .iter()
                    .find(|c| c["method"] == "integration.view.replace")
                {
                    let items = frame["params"]["items"].as_array().unwrap();
                    assert_eq!(
                        items.len(),
                        1,
                        "native observations must be lost on restart"
                    );
                    assert!(
                        items[0]["detail"]
                            .as_str()
                            .unwrap()
                            .contains("UNKNOWN (historical)")
                    );
                    break;
                }
                drop(calls);
                assert!(Instant::now() < end);
                thread::sleep(Duration::from_millis(10));
            }
            drop(restarted.0.stdin.take());
            restarted.finish();
        }
    }
}

#[test]
fn native_cli_reconstructs_closed_rpc_and_rejects_peer_extra_fields() {
    for (extra, event) in [
        (false, "session.created"),
        (true, "session.created"),
        (false, "permission.cancelled"),
        (false, "question.cancelled"),
    ] {
        let root = physical_temp::tempdir();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.path().join("peer.sock");
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
            assert_eq!(request.as_object().unwrap().len(), 4);
            assert_eq!(request["contract"], "coven.native-events.v1");
            assert_eq!(request["method"], "report");
            assert_eq!(
                request["params"].as_object().unwrap().len(),
                if event == "session.created" { 4 } else { 5 }
            );
            assert_eq!(request["params"]["event"], event);
            assert!(request["params"].get("contract").is_none());
            let mut result = json!({"type":"accepted","delivery":"volatile"});
            if extra {
                result["prompt"] = json!("PRIVATE-PEER");
            }
            writeln!(stream,"{}",json!({"id":request["id"],"contract":"coven.native-events.v1","type":"result","result":result})).unwrap();
        });
        let mut reporter = Command::new(env!("CARGO_BIN_EXE_coven"))
            .args(["agent", "report-native", "--socket"])
            .arg(&path)
            .arg("--json")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut payload = json!({"contract":"djinn.native-events.v1","instance_id":"12345678-1234-1234-1234-123456789abc","session_id":"PRIVATE-SID","sequence":1,"event":event});
        if event != "session.created" {
            payload["request_id"] = json!("PRIVATE-REQUEST");
        }
        writeln!(reporter.stdin.take().unwrap(), "{payload}").unwrap();
        let result = reporter.wait_with_output().unwrap();
        worker.join().unwrap();
        assert_eq!(result.status.success(), !extra);
        assert!(!String::from_utf8_lossy(&result.stdout).contains("PRIVATE"));
        assert!(!String::from_utf8_lossy(&result.stderr).contains("PRIVATE"));
    }
}
impl Receiver {
    fn start(root: &std::path::Path) -> Self {
        Self(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "receiver_child", "--nocapture"])
                .env("COVEN_AGENT_VIEW_TEST_ROOT", root)
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    fn finish(mut self) {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(
                Instant::now() < deadline,
                "receiver did not stop within bound"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Kit {
    calls: Arc<Mutex<Vec<Value>>>,
    providers: Arc<Mutex<Value>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Kit {
    fn start(root: &std::path::Path, mode: &'static str) -> Self {
        let path = root.join("kit.sock");
        let listener = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let providers = Arc::new(Mutex::new(json!({"build":{
            "lease":"build-lease","epoch":"build-epoch","revision":7,
            "items":[{"id":"build:job","label":"Build"}],
            "intents":[{"epoch":"build-epoch","revision":7,"item_id":"build:job"}]}})));
        let stop = Arc::new(AtomicBool::new(false));
        let seen = calls.clone();
        let state = providers.clone();
        let done = stop.clone();
        let worker = thread::spawn(move || {
            let mut revision = 0;
            let mut selected = false;
            while !done.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                seen.lock().unwrap().push(request.clone());
                let method = request["method"].as_str().unwrap();
                let params = &request["params"];
                if method != "session.snapshot" {
                    assert_eq!(params["source"], "queueSource2");
                    if method != "integration.view.open" {
                        assert_eq!(params["lease"], "queue-lease");
                        assert_eq!(params["epoch"], "queue-epoch");
                    }
                }
                if mode == "busy" && method == "integration.view.open" {
                    let reply = json!({"id":request["id"],"error":{"code":"sourceBusy","message":"PRIVATE MESSAGE"}});
                    writeln!(stream, "{reply}").unwrap();
                    continue;
                }
                let result = match method {
                    "session.snapshot" => json!({"type":"session_snapshot","snapshot":{
                        "version":"fixture","protocol":18,"selected_integration_source":"build",
                        "integration_views":{"version":if mode == "unsupported" {2} else {1},
                            "sources":[{"id":"build","epoch":"build-epoch"},{"id":"queueSource2","epoch":null}]}}}),
                    "integration.view.open" => {
                        revision = 0;
                        assert_eq!(params["version"], 1);
                        state.lock().unwrap()["queueSource2"] = json!({"items":[],"intents":[]});
                        if mode == "lost_open" {
                            continue;
                        }
                        json!({"type":"view_opened","lease":"queue-lease","epoch":"queue-epoch","revision":0,"ttl_ms":5000})
                    }
                    "integration.view.replace" => {
                        assert_eq!(params["revision"].as_u64(), Some(revision + 1));
                        revision += 1;
                        assert!(serde_json::to_vec(params).unwrap().len() <= 96 * 1024);
                        assert!(params["items"].as_array().unwrap().len() <= 128);
                        state.lock().unwrap()["queueSource2"]["items"] = params["items"].clone();
                        json!({"type":"view_replaced","epoch":"queue-epoch","revision":revision})
                    }
                    "integration.view.poll" => {
                        if mode == "lost_poll" {
                            continue;
                        }
                        if mode == "blocked_poll" {
                            while !done.load(Ordering::Acquire) {
                                thread::sleep(Duration::from_millis(5));
                            }
                            continue;
                        }
                        let item = state.lock().unwrap()["queueSource2"]["items"][0]["id"].clone();
                        let intents = if !selected && item.is_string() {
                            selected = true;
                            json!([
                                {"epoch":"build-epoch","revision":revision,"item_id":item},
                                {"epoch":"queue-epoch","revision":revision-1,"item_id":item},
                                {"epoch":"queue-epoch","revision":revision,"item_id":"agent:unknown"},
                                {"epoch":"queue-epoch","revision":revision,"item_id":item}
                            ])
                        } else {
                            json!([])
                        };
                        json!({"type":"view_intents","epoch":"queue-epoch","revision":revision,"ttl_ms":5000,"intents":intents})
                    }
                    "integration.view.close" => {
                        state
                            .lock()
                            .unwrap()
                            .as_object_mut()
                            .unwrap()
                            .remove("queueSource2");
                        json!({"type":"view_closed"})
                    }
                    other => panic!("unexpected effect: {other}"),
                };
                let reply = json!({"id":request["id"],"result":result});
                let _ = writeln!(stream, "{reply}");
            }
        });
        Self {
            calls,
            providers,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Kit {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}

#[test]
fn optional_kit_isolated_from_receiver_and_other_sources() {
    for mode in [
        "unavailable",
        "unsupported",
        "busy",
        "success",
        "codex",
        "lost_open",
        "lost_poll",
        "blocked_poll",
        "ctrl_c",
    ] {
        let root = physical_temp::tempdir();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.path().join("coven.json"), "{}").unwrap();
        let kit = (mode != "unavailable").then(|| Kit::start(root.path(), mode));
        let build_before = kit
            .as_ref()
            .map(|kit| kit.providers.lock().unwrap()["build"].clone());
        let started = Instant::now();
        let receiver = Receiver::start(root.path());
        let socket = root
            .path()
            .join("receiver.sock")
            .to_str()
            .unwrap()
            .to_owned();
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if agent_management::inventory_arguments(
                &InventoryRequest {
                    socket: socket.clone(),
                },
                true,
            )
            .is_ok()
            {
                break;
            }
            assert!(Instant::now() < deadline, "receiver not ready: {mode}");
            thread::sleep(Duration::from_millis(5));
        }
        let provider = if mode == "codex" { "codex" } else { "claude" };
        let event = if mode == "codex" {
            "SessionStart"
        } else {
            "PreToolUse"
        };
        let state = if mode == "codex" {
            "unknown"
        } else {
            "working"
        };
        let expected_id = format!(
            "{:x}",
            Sha256::digest([provider.as_bytes(), b"\0", b"PRIVATE-NATIVE-SID"].concat())
        );
        let report_started = Instant::now();
        // Send exactly once. An uncertain receipt is not evidence of non-delivery.
        let receipt = agent_management::report_payload(&ReportRequest {
            socket:socket.clone(), provider:provider.into(), event:Some(event.into()),
        }, json!({"session_id":"PRIVATE-NATIVE-SID","hook_event_name":event,"source":"startup","model":"PRIVATE-MODEL","permission_mode":"default","transcript_path":"PRIVATE-TRANSCRIPT","cwd":"/PRIVATE/CWD","prompt":"PRIVATE-PROMPT","token":"PRIVATE-TOKEN"}).to_string().as_bytes(), true);
        let report_elapsed = report_started.elapsed();
        let receipt_status = match receipt {
            Ok(accepted) => {
                let accepted: Value = serde_json::from_str(&accepted.stdout).unwrap();
                assert_eq!(accepted["type"], "accepted", "mode={mode}");
                assert_eq!(accepted["delivery"], "committed", "mode={mode}");
                assert_eq!(accepted["record_id"], expected_id, "mode={mode}");
                "acknowledged"
            }
            Err(error) => {
                assert_eq!(error.code(), "agent_delivery_uncertain", "mode={mode}");
                "agent_delivery_uncertain"
            }
        };
        eprintln!(
            "mode={mode} report={receipt_status} report_ms={} elapsed_ms={}",
            report_elapsed.as_millis(),
            started.elapsed().as_millis()
        );
        thread::sleep(Duration::from_millis(900));
        let inventory_started = Instant::now();
        let context = format!(
            "mode={mode} report={receipt_status} report_ms={} elapsed_ms={}",
            report_elapsed.as_millis(),
            started.elapsed().as_millis()
        );
        let mut stream = UnixStream::connect(&socket).expect(&context);
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let id = "1".repeat(32);
        writeln!(
            stream,
            "{}",
            json!({"id":id,"contract":agent_management::CONTRACT,
            "method":"inventory","params":{}})
        )
        .unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).expect(&context);
        let reply: Value = serde_json::from_str(&line).expect(&context);
        assert_eq!(reply["id"], id, "{context}");
        assert_eq!(reply["contract"], agent_management::CONTRACT, "{context}");
        assert_eq!(reply["type"], "result", "{context}");
        let inventory = &reply["result"];
        assert_eq!(
            inventory["contract"],
            agent_management::CONTRACT,
            "{context}"
        );
        assert_eq!(inventory["authorization"], "none", "{context}");
        assert_eq!(
            inventory["records"].as_array().unwrap().len(),
            1,
            "{context}"
        );
        let record = &inventory["records"][0];
        assert_eq!(record["record_id"], expected_id, "{context}");
        assert_eq!(record["provider"], provider, "{context}");
        assert_eq!(record["last_event"], event, "{context}");
        assert_eq!(record["reported_state"], state, "{context}");
        assert!(record["age_ms"].as_u64().is_some(), "{context}");
        assert_eq!(
            record["reference_trust"], "caller_supplied_unverified",
            "{context}"
        );
        assert_eq!(record["native_identity"], "unestablished", "{context}");
        assert_eq!(record["job_completion"], "unestablished", "{context}");
        // Require private on-disk proof as well, even when no ack was received.
        // Never print the saved value: it contains the native session reference.
        let store = root.path().join("state/agent-management.json");
        let metadata = fs::symlink_metadata(&store).expect(&context);
        assert!(metadata.is_file(), "{context}");
        assert_eq!(metadata.mode() & 0o7777, 0o600, "{context}");
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() }, "{context}");
        assert_eq!(metadata.nlink(), 1, "{context}");
        let saved_bytes = fs::read(&store).expect(&context);
        let saved: Value = serde_json::from_slice(&saved_bytes).expect(&context);
        assert_eq!(saved["schema_version"], 1, "{context}");
        assert_eq!(saved["records"].as_object().unwrap().len(), 1, "{context}");
        let persisted = &saved["records"][&expected_id];
        assert!(
            persisted["session_id"] == "PRIVATE-NATIVE-SID",
            "{context}: saved identity mismatch"
        );
        assert!(
            persisted["provider"] == provider,
            "{context}: saved provider mismatch"
        );
        assert!(
            persisted["last_event"] == event,
            "{context}: saved event mismatch"
        );
        assert!(
            persisted["server_epoch"] == inventory["server_epoch"],
            "{context}: saved epoch mismatch"
        );
        assert!(
            persisted["timestamp_ms"].as_u64().is_some_and(|ms| ms > 0),
            "{context}: missing saved timestamp"
        );
        eprintln!(
            "mode={mode} report={receipt_status} raw_inventory=original_report persisted=original_report inventory_ms={} elapsed_ms={}",
            inventory_started.elapsed().as_millis(),
            started.elapsed().as_millis()
        );
        let interrupted_at = (mode == "ctrl_c").then(|| {
            assert_eq!(
                unsafe { libc::kill(receiver.0.id() as i32, libc::SIGINT) },
                0
            );
            Instant::now()
        });
        receiver.finish();
        assert!(
            fs::read(&store).expect(&context) == saved_bytes,
            "{context}: read-only presentation changed persisted report"
        );
        if let Some(at) = interrupted_at {
            assert!(
                at.elapsed() < Duration::from_secs(1),
                "Ctrl-C shutdown exceeded bound"
            );
        }
        assert!(
            started.elapsed() < Duration::from_millis(3100),
            "shutdown exceeded bound: {mode}"
        );
        let report: Value =
            serde_json::from_slice(&fs::read(root.path().join("result.json")).unwrap()).unwrap();
        assert_eq!(report["type"], "stopped");
        assert_eq!(report["authorization"], "none");
        let ui = &report["presentation"];
        eprintln!(
            "mode={mode} completed elapsed_ms={}",
            started.elapsed().as_millis()
        );
        if let Some(kit) = &kit {
            let calls = kit.calls.lock().unwrap();
            assert_eq!(
                kit.providers.lock().unwrap()["build"],
                build_before.unwrap()
            );
            assert_eq!(
                calls
                    .iter()
                    .filter(|c| c["method"] == "session.snapshot")
                    .count(),
                1
            );
            assert!(
                calls
                    .iter()
                    .filter(|c| c["method"] == "integration.view.open")
                    .count()
                    <= 1
            );
            let serialized = serde_json::to_string(&*calls).unwrap();
            for private in [
                "PRIVATE-NATIVE-SID",
                "/PRIVATE/CWD",
                "PRIVATE-PROMPT",
                "PRIVATE-TOKEN",
                "PRIVATE-MODEL",
                "PRIVATE-TRANSCRIPT",
                "permission_mode",
            ] {
                assert!(!serialized.contains(private));
            }
            if matches!(mode, "success" | "ctrl_c" | "codex") {
                assert_eq!(ui["cleanup"], "closed");
                assert!(calls.iter().any(|c| {
                    c["method"] == "integration.view.replace"
                        && c["params"]["items"][0]["label"]
                            .as_str()
                            .is_some_and(|s| s.starts_with(provider))
                        && c["params"]["items"][0]["detail"].as_str().is_some_and(|s| {
                            s.starts_with(&format!("Last report: {}", state.to_ascii_uppercase()))
                                && s.contains("Selected (read-only)")
                        })
                }));
                assert_eq!(
                    calls
                        .iter()
                        .filter(|c| c["method"] == "integration.view.close")
                        .count(),
                    1
                );
                assert!(kit.providers.lock().unwrap().get("queueSource2").is_none());
            } else if matches!(mode, "lost_open" | "lost_poll" | "blocked_poll") {
                assert_eq!(ui["outcome"], "uncertain");
                assert!(
                    calls
                        .iter()
                        .all(|c| c["method"] != "integration.view.close")
                );
            } else {
                assert_eq!(ui["outcome"], "failed");
            }
        } else {
            assert_eq!(ui["outcome"], "failed");
        }
    }
}

#[test]
fn failed_core_load_does_not_admit_kit_discovery() {
    let root = physical_temp::tempdir();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.path().join("coven.json"), "{}").unwrap();
    fs::create_dir(root.path().join("state")).unwrap();
    let store = root.path().join("state/agent-management.json");
    fs::write(&store, "invalid persisted inventory").unwrap();
    fs::set_permissions(&store, fs::Permissions::from_mode(0o600)).unwrap();
    let kit = Kit::start(root.path(), "success");
    let started = Instant::now();
    assert!(
        agent_management_presentation::serve_arguments(
            &ServeRequest {
                workspace: root.path().into(),
                socket: root.path().join("receiver.sock").to_str().unwrap().into(),
                duration: 1
            },
            &PresentationRequest {
                socket: root.path().join("kit.sock").to_str().unwrap().into(),
                source: "queueSource2".into()
            },
            true,
        )
        .is_err()
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(kit.calls.lock().unwrap().is_empty());
}
