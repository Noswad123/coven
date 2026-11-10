#![cfg(unix)]
//! Private socket fixtures: no live Kitsune, providers, or execution admission.
use coven::{
    group_presenter::{self, Request},
    groups,
    values::{self, Profile},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

struct Server {
    path: String,
    calls: Arc<Mutex<Vec<Value>>>,
    providers: Arc<Mutex<Value>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    _root: tempfile::TempDir,
}
impl Server {
    fn new(mode: &'static str, workspace: std::path::PathBuf) -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.path().join("view.sock");
        let listener = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        // An already-live provider is not owned or selected by this publisher.
        let providers = Arc::new(Mutex::new(json!({
            "build":{"lease":"build_lease","epoch":"build_epoch","revision":7,
                "items":[{"id":"build:job","label":"Build"}],
                "intents":[{"epoch":"build_epoch","revision":7,"item_id":"build:job"}]},
            "coven":{"lease":"coven_lease","epoch":"coven_epoch","revision":4,
                "items":[],"intents":[]}
        })));
        let worker_providers = providers.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_calls = calls.clone();
        let worker_stop = stop.clone();
        let socket_path = path.clone();
        let worker = thread::spawn(move || {
            let mut _replacement = None;
            let mut participant = Value::Null;
            let mut revision = 0;
            let mut selected = false;
            while !worker_stop.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                worker_calls.lock().unwrap().push(request.clone());
                let method = request["method"].as_str().unwrap();
                let source = if mode == "build_success" {
                    "build"
                } else {
                    "coven"
                };
                if method != "session.snapshot" {
                    assert_eq!(request["params"]["source"], source);
                    if method != "integration.view.open" {
                        assert_eq!(request["params"]["lease"], "lease");
                        assert_eq!(request["params"]["epoch"], "epoch");
                    }
                }
                if mode == "busy" && method == "integration.view.open" {
                    let reply = json!({"id":request["id"],"error":{"code":"sourceBusy","message":"PRIVATE PEER TEXT"}});
                    stream.write_all(format!("{reply}\n").as_bytes()).unwrap();
                    continue;
                }
                if mode == "duplicate" && method == "session.snapshot" {
                    stream
                        .write_all(
                            format!(
                                "{{\"id\":{},\"id\":{},\"result\":{{}}}}\n",
                                request["id"], request["id"]
                            )
                            .as_bytes(),
                        )
                        .unwrap();
                    continue;
                }
                let result = match method {
                    "session.snapshot" => {
                        let mut snapshot = json!({"version":"fixture","protocol":18,
                            "selected_integration_source":"build",
                            "integration_views":{"version":1,"sources":[
                                {"id":"build","epoch":"build_epoch"},
                                {"id":"coven","epoch":null}]}});
                        match mode {
                            "disabled" => snapshot["integration_views"]["sources"] = json!([]),
                            "mismatch" => {
                                snapshot["integration_views"]["sources"][1]["id"] = json!("other")
                            }
                            "unsupported" => snapshot["integration_views"]["version"] = json!(2),
                            "absent" => {
                                snapshot
                                    .as_object_mut()
                                    .unwrap()
                                    .remove("integration_views");
                            }
                            "legacy_descriptor" => {
                                snapshot
                                    .as_object_mut()
                                    .unwrap()
                                    .remove("integration_views");
                                snapshot["external_view"] =
                                    json!({"version":1,"source":"coven","epoch":null});
                            }
                            "duplicate_ids" => {
                                snapshot["integration_views"]["sources"][0]["id"] = json!("coven")
                            }
                            "other_epoch_type" => {
                                snapshot["integration_views"]["sources"][0]["epoch"] = json!(7)
                            }
                            "other_epoch_token" => {
                                snapshot["integration_views"]["sources"][0]["epoch"] =
                                    json!("bad token")
                            }
                            "other_id" => {
                                snapshot["integration_views"]["sources"][0]["id"] =
                                    json!("bad source")
                            }
                            "other_id_empty" => {
                                snapshot["integration_views"]["sources"][0]["id"] = json!("")
                            }
                            "other_id_long" => {
                                snapshot["integration_views"]["sources"][0]["id"] =
                                    json!("x".repeat(65))
                            }
                            "other_id_unicode" => {
                                snapshot["integration_views"]["sources"][0]["id"] = json!("\u{e9}")
                            }
                            "other_missing_epoch" => {
                                snapshot["integration_views"]["sources"][0]
                                    .as_object_mut()
                                    .unwrap()
                                    .remove("epoch");
                            }
                            "malformed_after_target" => snapshot["integration_views"]["sources"]
                                .as_array_mut()
                                .unwrap()
                                .push(json!({"id":"third","epoch":false})),
                            "oversized" | "eight_sources" => {
                                for n in 2..if mode == "oversized" { 9 } else { 8 } {
                                    snapshot["integration_views"]["sources"]
                                        .as_array_mut()
                                        .unwrap()
                                        .push(json!({"id":format!("provider_{n}"),"epoch":null}));
                                }
                            }
                            "sources_type" => {
                                snapshot["integration_views"]["sources"] = Value::Null
                            }
                            "protocol" => snapshot["protocol"] = json!(17),
                            _ => (),
                        }
                        json!({"type":"session_snapshot","snapshot":snapshot})
                    }
                    "integration.view.open" => {
                        assert_eq!(request["params"]["version"], 1);
                        worker_providers.lock().unwrap()[source] =
                            json!({"lease":"lease","epoch":"epoch","revision":0,"items":[]});
                        if mode == "lost_open" {
                            continue;
                        }
                        json!({"type":"view_opened","lease":"lease","epoch":"epoch","revision":0,"ttl_ms":5000})
                    }
                    "integration.view.replace" => {
                        let next = request["params"]["revision"].as_u64().unwrap();
                        assert_eq!(next, revision + 1);
                        revision = next;
                        let mut providers = worker_providers.lock().unwrap();
                        providers[source]["revision"] = json!(revision);
                        providers[source]["items"] = request["params"]["items"].clone();
                        participant = request["params"]["items"][1]["id"].clone();
                        if mode == "lost_replace" {
                            continue;
                        }
                        json!({"type":"view_replaced","epoch":"epoch","revision":revision})
                    }
                    "integration.view.poll" => {
                        if mode == "poll_timeout" {
                            thread::sleep(Duration::from_millis(1100));
                            continue;
                        }
                        if mode == "endpoint_changed" {
                            fs::remove_file(&socket_path).unwrap();
                            _replacement = Some(UnixListener::bind(&socket_path).unwrap());
                            fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
                                .unwrap();
                        }
                        if mode == "group_changed" {
                            groups::remove_member(&workspace, "team", "reviewer", None).unwrap();
                        }
                        if mode == "lost_poll" {
                            continue;
                        }
                        if mode == "malformed" {
                            json!({"type":"unexpected"})
                        } else {
                            let intents = if selected {
                                vec![]
                            } else {
                                selected = true;
                                vec![
                                    json!({"epoch":"build_epoch","revision":revision,"item_id":participant}),
                                    json!({"epoch":"epoch","revision":revision-1,"item_id":participant}),
                                    json!({"epoch":"epoch","revision":revision,"item_id":"p:unknown"}),
                                    json!({"epoch":"epoch","revision":revision,"item_id":participant}),
                                ]
                            };
                            let mut response = json!({"type":"view_intents","epoch":if mode == "wrong_epoch" { "build_epoch" } else { "epoch" },"revision":revision,"ttl_ms":5000,"intents":intents});
                            if mode == "response_limit" {
                                response["padding"] = json!("x".repeat(256 * 1024));
                            }
                            if mode == "wrong_revision" {
                                response["revision"] = json!(revision + 1);
                            }
                            response
                        }
                    }
                    "integration.view.close" => {
                        worker_providers.lock().unwrap()[source] = Value::Null;
                        json!({"type":"view_closed"})
                    }
                    other => panic!("unexpected effect: {other}"),
                };
                let id = if mode == "uncorrelated" && method == "integration.view.open" {
                    json!("wrong")
                } else {
                    request["id"].clone()
                };
                let mut bytes = serde_json::to_vec(&json!({"id":id,"result":result})).unwrap();
                bytes.push(b'\n');
                let _ = stream.write_all(&bytes);
                if mode == "interrupted" && method == "integration.view.replace" {
                    drop(stream);
                    thread::sleep(Duration::from_millis(50));
                    // Exercise the scoped SIGINT handler while the presenter is
                    // between calls, not an intentionally uncertain in-flight send.
                    unsafe {
                        libc::kill(libc::getpid(), libc::SIGINT);
                    }
                }
            }
        });
        Self {
            path: path.to_str().unwrap().into(),
            calls,
            providers,
            stop,
            worker: Some(worker),
            _root: root,
        }
    }
    fn calls(&self) -> Vec<Value> {
        self.calls.lock().unwrap().clone()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("state")).unwrap();
    fs::write(
        root.path().join("coven.json"),
        "invalid config must not be loaded",
    )
    .unwrap();
    groups::create_group(root.path(), "team", "Review Team", None).unwrap();
    let binding = values::parse(br#"{"presentation":{"adapter":"kitsune","endpoint":{"path":"/not/used","device":0,"inode":0},"terminal_id":"not-used","pane_id_at_binding":"not-used"},"evidence":{"provenance":"kitsune.session.snapshot","observed_at":"2024-W01T12Z","agent_kind_hint":null,"native_reference":null,"native_binding":"unestablished"}}"#,Profile::Authority).unwrap();
    groups::add_member(
        root.path(),
        groups::AddMember {
            group: "team",
            member: "reviewer",
            role: "Reviewer",
            binding: &binding,
            rebind: false,
            expected_revision: None,
        },
    )
    .unwrap();
    root
}

#[test]
fn group_presenter_private_socket_workflows() {
    // One sequential test keeps the process-wide interrupt guard exclusive.
    let cwd = std::env::current_dir().unwrap();
    for mode in [
        "success",
        "build_success",
        "eight_sources",
        "disabled",
        "mismatch",
        "unsupported",
        "absent",
        "legacy_descriptor",
        "duplicate_ids",
        "other_epoch_type",
        "other_epoch_token",
        "other_id",
        "other_id_empty",
        "other_id_long",
        "other_id_unicode",
        "other_missing_epoch",
        "malformed_after_target",
        "oversized",
        "sources_type",
        "protocol",
        "lost_open",
        "lost_replace",
        "lost_poll",
        "malformed",
        "uncorrelated",
        "wrong_epoch",
        "wrong_revision",
        "response_limit",
        "group_changed",
        "busy",
        "duplicate",
        "endpoint_changed",
        "interrupted",
        "poll_timeout",
    ] {
        let root = workspace();
        let before = fs::read(root.path().join("state/groups.json")).unwrap();
        let server = Server::new(mode, root.path().into());
        let untouched = if mode == "build_success" {
            "coven"
        } else {
            "build"
        };
        let other_before = server.providers.lock().unwrap()[untouched].clone();
        let request = Request {
            workspace: root.path().into(),
            group: "team".into(),
            socket: server.path.clone(),
            source: if mode == "build_success" {
                "build"
            } else {
                "coven"
            }
            .into(),
            duration: 1,
        };
        let output = group_presenter::arguments(&request, true).unwrap();
        let report: Value = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(report["contract"], "coven.group-presentation.v1");
        assert_eq!(report["authorization"], "none");
        assert_eq!(report["delivery"], "ephemeral_ui_only");
        let calls = server.calls();
        let methods: Vec<_> = calls
            .iter()
            .map(|v| v["method"].as_str().unwrap())
            .collect();
        if ["success", "build_success", "eight_sources"].contains(&mode) {
            assert_eq!(output.exit_code, 0, "{report}");
            assert_eq!(report["cleanup"], "closed");
            assert_eq!(report["frames"], 2);
            assert_eq!(report["selections"], 1);
            assert_eq!(report["ignored_intents"], 3);
            assert_eq!(methods.last(), Some(&"integration.view.close"));
            let frames: Vec<_> = calls
                .iter()
                .filter(|v| v["method"] == "integration.view.replace")
                .collect();
            assert_eq!(frames[0]["params"]["items"][0]["label"], "Review Team");
            assert_eq!(frames[0]["params"]["items"][1]["label"], "reviewer");
            assert!(
                frames[1]["params"]["items"][1]["detail"]
                    .as_str()
                    .unwrap()
                    .starts_with("Selected (read-only): Reviewer")
            );
            assert!(!serde_json::to_string(&frames).unwrap().contains("not-used"));
        } else if [
            "disabled",
            "mismatch",
            "unsupported",
            "absent",
            "legacy_descriptor",
            "duplicate_ids",
            "other_epoch_type",
            "other_epoch_token",
            "other_id",
            "other_id_empty",
            "other_id_long",
            "other_id_unicode",
            "other_missing_epoch",
            "malformed_after_target",
            "oversized",
            "sources_type",
            "protocol",
            "duplicate",
        ]
        .contains(&mode)
        {
            assert_eq!(output.exit_code, 2);
            assert_eq!(methods, vec!["session.snapshot"]);
            let diagnostic = match mode {
                "disabled" | "mismatch" => "integration_view_source_disabled",
                "unsupported" | "absent" | "legacy_descriptor" => "integration_view_unsupported",
                "protocol" => "unsupported_protocol",
                "duplicate" => "duplicate_key",
                _ => "integration_view_schema_error",
            };
            assert_eq!(
                report["diagnostics"],
                json!([diagnostic]),
                "{mode}: {report}"
            );
            assert_eq!(report["cleanup"], "not_opened");
        } else if mode == "interrupted" {
            assert_eq!(output.exit_code, 130, "{report}");
            assert_eq!(report["outcome"], "interrupted");
            assert_eq!(report["cleanup"], "closed");
            assert_eq!(
                methods,
                vec![
                    "session.snapshot",
                    "integration.view.open",
                    "integration.view.replace",
                    "integration.view.close"
                ]
            );
        } else if mode == "busy" {
            assert_eq!(report["outcome"], "failed");
            assert_eq!(report["diagnostics"], json!(["view_rejected"]));
            assert!(!output.stdout.contains("PRIVATE PEER TEXT"));
            assert_eq!(methods, vec!["session.snapshot", "integration.view.open"]);
        } else if mode == "group_changed" {
            assert_eq!(report["outcome"], "failed");
            assert_eq!(report["diagnostics"], json!(["group_revision_changed"]));
            assert_eq!(report["frames"], 1);
            assert_eq!(report["selections"], 0);
            assert_eq!(report["cleanup"], "closed");
        } else {
            assert_eq!(report["outcome"], "uncertain", "{mode}: {report}");
            if mode == "poll_timeout" {
                assert_eq!(report["diagnostics"], json!(["timeout"]));
                assert!(report["duration_ms"].as_u64().unwrap() < 1300, "{report}");
            }
            if ["wrong_epoch", "wrong_revision"].contains(&mode) {
                assert_eq!(
                    report["diagnostics"],
                    json!(["integration_view_schema_error"])
                );
                assert_eq!(report["cleanup"], "ttl_only_uncertain");
                assert_eq!(report["selections"], 0);
            }
            if mode == "response_limit" {
                assert_eq!(report["diagnostics"], json!(["response_limit"]));
            }
            assert!(!methods.contains(&"integration.view.close"));
            assert_eq!(
                methods
                    .iter()
                    .filter(|m| **m == "integration.view.open")
                    .count(),
                1
            );
            assert!(
                methods
                    .iter()
                    .filter(|m| **m == "integration.view.replace")
                    .count()
                    <= 1
            );
            assert!(
                methods
                    .iter()
                    .filter(|m| **m == "integration.view.poll")
                    .count()
                    <= 1
            );
        }
        assert_eq!(
            server.providers.lock().unwrap()[untouched],
            other_before,
            "{mode}"
        );
        for call in calls.iter().skip(1) {
            assert_eq!(call["params"]["source"], request.source);
        }
        assert_eq!(std::env::current_dir().unwrap(), cwd);
        if mode != "group_changed" {
            assert_eq!(
                fs::read(root.path().join("state/groups.json")).unwrap(),
                before
            );
        }
        assert_eq!(
            fs::read_to_string(root.path().join("coven.json")).unwrap(),
            "invalid config must not be loaded"
        );
    }
}

#[test]
fn group_presenter_cli_private_socket_selection() {
    // The isolated module-validation harness has no binary; the parent crate's
    // integration test builds and executes the real CLI after registration.
    let Some(binary) = option_env!("CARGO_BIN_EXE_coven") else {
        return;
    };
    let root = workspace();
    let before = fs::read(root.path().join("state/groups.json")).unwrap();
    let server = Server::new("success", root.path().into());
    let result = std::process::Command::new(binary)
        .args(["group", "present"])
        .arg(root.path())
        .args([
            "team",
            "--socket",
            &server.path,
            "--source",
            "coven",
            "--duration",
            "1",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["frames"], 2);
    assert_eq!(report["selections"], 1);
    assert_eq!(report["cleanup"], "closed");
    assert_eq!(
        server.calls().last().unwrap()["method"],
        "integration.view.close"
    );
    assert_eq!(
        fs::read(root.path().join("state/groups.json")).unwrap(),
        before
    );
}

#[test]
fn group_presenter_rejects_operands_before_io() {
    for (group, socket, source, duration) in [
        ("team", "relative.sock", "coven.test", 1),
        ("team", "/not/a/socket", "bad source", 1),
        ("../team", "/not/a/socket", "coven.test", 1),
        ("team", "/not/a/socket", "coven.test", 0),
        ("team", "/not/a/socket", "coven.test", 3601),
    ] {
        let request = Request {
            workspace: "/not/a/workspace".into(),
            group: group.into(),
            socket: socket.into(),
            source: source.into(),
            duration,
        };
        assert!(group_presenter::arguments(&request, true).is_err());
    }
}
