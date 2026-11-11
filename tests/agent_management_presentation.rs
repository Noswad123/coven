#![cfg(unix)]
//! Private fake Kit plus receiver child: no live apps or process-global test signals.
use coven::{
    agent_management::{self, InventoryRequest, ReportRequest, ServeRequest},
    agent_management_presentation::{self, PresentationRequest},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
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
        let root = tempfile::tempdir().unwrap();
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
        let accepted = agent_management::report_payload(&ReportRequest {
            socket:socket.clone(), provider:provider.into(), event:Some(event.into()),
        }, json!({"session_id":"PRIVATE-NATIVE-SID","hook_event_name":event,"source":"startup","model":"PRIVATE-MODEL","permission_mode":"default","transcript_path":"PRIVATE-TRANSCRIPT","cwd":"/PRIVATE/CWD","prompt":"PRIVATE-PROMPT","token":"PRIVATE-TOKEN"}).to_string().as_bytes(), true).unwrap();
        let accepted: Value = serde_json::from_str(&accepted.stdout).unwrap();
        assert_eq!(accepted["type"], "accepted");
        assert_eq!(accepted["delivery"], "committed");
        thread::sleep(Duration::from_millis(900));
        let mut stream = UnixStream::connect(&socket).unwrap();
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
        BufReader::new(stream).read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["id"], id);
        assert_eq!(reply["type"], "result");
        let inventory = &reply["result"];
        assert_eq!(inventory["records"][0]["record_id"], accepted["record_id"]);
        assert_eq!(inventory["records"][0]["reported_state"], state);
        let interrupted_at = (mode == "ctrl_c").then(|| {
            assert_eq!(
                unsafe { libc::kill(receiver.0.id() as i32, libc::SIGINT) },
                0
            );
            Instant::now()
        });
        receiver.finish();
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
    let root = tempfile::tempdir().unwrap();
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
