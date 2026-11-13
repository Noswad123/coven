//! Owned fake Kit sockets only: no live Kit, native apps, or user state.
#![cfg(unix)]
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    process::{Command, Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct Server {
    dir: tempfile::TempDir,
    stop: Arc<AtomicBool>,
    methods: Arc<Mutex<Vec<String>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn new(handler: impl Fn(usize, &Value) -> Vec<u8> + Send + 'static) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(dir.path().join("kit.sock")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            dir.path().join("kit.sock"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let methods = Arc::new(Mutex::new(Vec::new()));
        let stopped = stop.clone();
        let recorded = methods.clone();
        let thread = thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(e) => panic!("accept: {e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut line = String::new();
                if BufReader::new(&stream).read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                let method = request["method"].as_str().unwrap().to_owned();
                assert!(matches!(
                    method.as_str(),
                    "session.snapshot" | "pane.process_info"
                ));
                let index = {
                    let mut methods = recorded.lock().unwrap();
                    let i = methods.len();
                    methods.push(method);
                    i
                };
                let response = handler(index, &request);
                let _ = stream.write_all(&response);
            }
        });
        Self {
            dir,
            stop,
            methods,
            thread: Some(thread),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_coven"));
        command
            .args(["agent", "discover", "--socket"])
            .arg(self.dir.path().join("kit.sock"))
            .arg("--json")
            .env("HOME", self.dir.path())
            .env("PATH", "/nonexistent")
            .current_dir(self.dir.path());
        command
    }

    fn run(&self, args: &[&str]) -> (Output, Value) {
        let output = self.command().args(args).output().unwrap();
        let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "{e}: {} / {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output, report)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn snapshot(count: usize) -> Value {
    json!({"type":"session_snapshot","snapshot":{"version":"owned-fake-kit","protocol":18,
        "process_observation":{"version":1,"platform":"linux","scope":"pane_foreground_cohort","selection":"unclassified"},
        "panes":(0..count).map(|i| json!({"pane_id":format!("p{i}"),"terminal_id":format!("t{i}"),"workspace_id":"w","tab_id":"tab",
            "title":"SECRET_TITLE","agent_session":{"value":"SECRET_NATIVE_ID"},"agent":"SECRET_HINT"})).collect::<Vec<_>>(),
        "agents":[false,{"malformed":"SECRET_AGENT"}]}})
}

fn mac_snapshot(count: usize) -> Value {
    let mut s = snapshot(count);
    s["snapshot"]["process_observation"] = json!({"version":2,"platform":"macos",
        "scope":"pane_foreground_cohort","selection":"unclassified",
        "argument_policy":"darwin_procargs2_lp64_v1"});
    s
}

fn process(request: &Value, rows: Value) -> Value {
    json!({"type":"pane_process_info","process_info":{"pane_id":request["params"]["pane_id"],"foreground_processes":rows}})
}

fn frame(request: &Value, result: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&json!({"id":request["id"],"result":result})).unwrap();
    bytes.push(b'\n');
    bytes
}

fn failed(output: &Output, report: &Value, exit: i32, code: &str) {
    assert_eq!(output.status.code(), Some(exit), "{report}");
    assert_eq!(report["targets"], json!([]));
    assert!(report["target_count"].is_null());
    assert!(report["recognized_target_count"].is_null());
    assert_eq!(report["diagnostics"], json!([code]));
    assert_eq!(report["acquisition_completeness"], "unknown");
}

#[test]
fn both_peer_platforms_retain_all_22_families_without_raw_evidence_or_user_state() {
    const FAMILIES: [&str; 22] = [
        "pi",
        "claude",
        "codex",
        "gemini",
        "cursor",
        "devin",
        "agy",
        "cline",
        "omp",
        "mastracode",
        "opencode",
        "copilot",
        "kimi",
        "kiro",
        "droid",
        "amp",
        "grok",
        "hermes",
        "kilo",
        "qodercli",
        "maki",
        "djinn",
    ];
    for mac in [false, true] {
        let server = Server::new(move |_, r| {
            if r["method"] == "session.snapshot" {
                let mut s = if mac { mac_snapshot(1) } else { snapshot(1) };
                s["snapshot"]["panes"][0]["agentHint"] = json!({"provider":"SECRET_ENV_SENTINEL"});
                return frame(r, s);
            }
            let mut rows: Vec<_> = FAMILIES.iter().enumerate().map(|(i, family)| {
                json!({"pid":4_000_000_000u32 + i as u32,"name":"SECRET_NAME",
                    "argv0":"SECRET_ARGV0","argv":[format!("/nonexistent/SECRET_PATH/{family}"),"","SECRET_ENV_SENTINEL"],
                    "cwd":"/SECRET_CWD","cmdline":"SECRET_CMDLINE","env":{"SECRET_ENV_SENTINEL":"SECRET"}})
            }).collect();
            // Unsupported 32-bit argument acquisition is name-only, not absence.
            rows.push(json!({"pid":4_000_000_100u32,"name":"claude","argv0":null,"argv":null}));
            // A nonempty structured vector masks both raw argv0 and name.
            rows.push(json!({"pid":4_000_000_101u32,"name":"claude","argv0":"codex","argv":["","claude",""]}));
            rows.push(json!({"pid":4_000_000_102u32,"name":"claude","argv":["C:\\bin\\claude"]}));
            frame(r, process(r, json!(rows)))
        });
        let (out, report) = server.run(&[]);
        assert_eq!(out.status.code(), Some(0), "{report}");
        assert_eq!(
            report["process_observation"],
            if mac { mac_snapshot(0) } else { snapshot(0) }["snapshot"]["process_observation"]
        );
        let target = &report["targets"][0];
        let matches = target["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 23);
        for (i, family) in FAMILIES.iter().enumerate() {
            assert_eq!(matches[i]["provider"], *family);
            assert_eq!(matches[i]["pid"], 4_000_000_000u32 + i as u32);
        }
        assert_eq!(matches[22]["provider"], "claude");
        assert_eq!(target["provider_candidates"].as_array().unwrap().len(), 22);
        assert_eq!(target["activity"], "unknown");
        assert_eq!(report["acquisition_completeness"], "unknown");
        assert_eq!(report["native_catalog_complete"], false);
        assert!(!String::from_utf8_lossy(&out.stdout).contains("SECRET"));
        let entries: Vec<_> = std::fs::read_dir(server.dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, [std::ffi::OsString::from("kit.sock")]);
    }
}

#[test]
fn mac_admission_rejects_legacy_missing_wrong_or_malformed_marker_before_reads() {
    for change in [
        "legacy",
        "legacy_marker",
        "missing",
        "wrong",
        "typed",
        "long",
        "ranked",
        "scope",
        "version",
        "windows",
    ] {
        let server = Server::new(move |_, r| {
            let mut s = mac_snapshot(1);
            let d = &mut s["snapshot"]["process_observation"];
            match change {
                "legacy" | "legacy_marker" => d["version"] = json!(1),
                "wrong" => d["argument_policy"] = json!("darwin_procargs2_lp64_v0"),
                "typed" => d["argument_policy"] = json!(true),
                "long" => d["argument_policy"] = json!("x".repeat(65)),
                "ranked" => d["selection"] = json!("ranked"),
                "scope" => d["scope"] = json!("agent_aware"),
                "version" => d["version"] = json!(2.0),
                "windows" => d["platform"] = json!("windows"),
                _ => {}
            }
            if matches!(change, "legacy" | "missing") {
                d.as_object_mut().unwrap().remove("argument_policy");
            }
            frame(r, s)
        });
        let (out, report) = server.run(&[]);
        failed(&out, &report, 2, "process_observation_unsupported");
        assert_eq!(server.methods.lock().unwrap().len(), 1);
    }
}

#[test]
fn supported_platform_drift_and_unsupported_marker_clear_whole_sample() {
    for change in [
        "linux_to_mac",
        "mac_to_linux",
        "marker",
        "raw_version",
        "raw_tuple",
        "partial",
    ] {
        let server = Server::new(move |index, r| {
            let mut s = if r["method"] == "session.snapshot" {
                if change == "linux_to_mac" {
                    snapshot(2)
                } else {
                    mac_snapshot(2)
                }
            } else {
                process(r, json!([{"pid":4_000_000_000u32,"name":"claude"}]))
            };
            if index == 3 {
                match change {
                    "linux_to_mac" => s = mac_snapshot(2),
                    "mac_to_linux" => s = snapshot(2),
                    "marker" => {
                        s["snapshot"]["process_observation"]["argument_policy"] =
                            json!("unsupported")
                    }
                    "raw_version" => s["snapshot"]["process_observation"]["version"] = json!("2"),
                    "raw_tuple" => s["snapshot"]["process_observation"]["scope"] = json!(null),
                    _ => {}
                }
            }
            if index == 2 && change == "partial" {
                s["process_info"]["foreground_processes"][0]["argv"] = json!(false);
            }
            frame(r, s)
        });
        let (out, report) = server.run(&[]);
        let (exit, code) = match change {
            "linux_to_mac" | "mac_to_linux" => (1, "process_observation_changed"),
            "partial" => (1, "schema_error"),
            _ => (2, "process_observation_unsupported"),
        };
        failed(&out, &report, exit, code);
        assert_eq!(
            report["process_observation"]["platform"],
            if change == "linux_to_mac" {
                "linux"
            } else {
                "macos"
            }
        );
    }
}

#[test]
fn mac_upstream_missing_cohort_is_observed_but_completeness_is_unknown() {
    let server = Server::new(|_, r| {
        frame(
            r,
            if r["method"] == "session.snapshot" {
                mac_snapshot(1)
            } else {
                json!({"type":"pane_process_info","process_info":{"pane_id":r["params"]["pane_id"]}})
            },
        )
    });
    let (out, report) = server.run(&[]);
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert_eq!(report["targets"][0]["recognition"], "unknown");
    assert_eq!(report["acquisition_completeness"], "unknown");
}

#[test]
fn mac_uses_the_same_generic_bounds_and_exact_protocol() {
    for case in [
        "panes64",
        "panes65",
        "processes256",
        "processes257",
        "args128",
        "args129",
        "bytes4096",
        "bytes4097",
        "text65536",
        "text65537",
        "protocol17",
        "protocol19",
        "budget",
    ] {
        let server = Server::new(move |_, r| {
            let mut result = if r["method"] == "session.snapshot" {
                let mut s = mac_snapshot(match case {
                    "panes64" => 64,
                    "panes65" => 65,
                    _ => 1,
                });
                if case == "protocol17" {
                    s["snapshot"]["protocol"] = json!(17);
                }
                if case == "protocol19" {
                    s["snapshot"]["protocol"] = json!(19);
                }
                s
            } else {
                let mut row = json!({"pid":4_000_000_000u32,"name":""});
                match case {
                    "args128" => row["argv"] = json!(vec![""; 128]),
                    "args129" => row["argv"] = json!(vec![""; 129]),
                    "bytes4096" => row["argv"] = json!(["x".repeat(4096)]),
                    "bytes4097" => row["argv"] = json!(["x".repeat(4097)]),
                    "text65536" | "text65537" => {
                        row["argv"] = json!(vec!["x".repeat(4096); 16]);
                        if case == "text65537" {
                            row["name"] = json!("x");
                        }
                    }
                    _ => {}
                }
                let count = match case {
                    "processes256" => 256,
                    "processes257" => 257,
                    _ => 1,
                };
                let rows: Vec<_> = (0..count)
                    .map(|i| {
                        let mut row = row.clone();
                        row["pid"] = json!(4_000_000_000u32 + i);
                        row
                    })
                    .collect();
                process(r, json!(rows))
            };
            if case == "budget" {
                result["padding"] = json!("x".repeat(600));
            }
            frame(r, result)
        });
        let (out, report) = server.run(if case == "budget" {
            &["--max-response-bytes", "1600"]
        } else {
            &[]
        });
        match case {
            "panes65" => failed(&out, &report, 1, "pane_limit"),
            "processes257" => failed(&out, &report, 1, "process_limit"),
            "args129" | "bytes4097" | "text65537" => failed(&out, &report, 1, "schema_error"),
            "protocol17" | "protocol19" => failed(&out, &report, 2, "unsupported_protocol"),
            "budget" => failed(&out, &report, 1, "response_limit"),
            _ => assert_eq!(out.status.code(), Some(0), "{case}: {report}"),
        }
    }
}

#[test]
fn all_candidates_are_kept_and_sensitive_fields_are_not_projected() {
    let server = Server::new(|_, r| {
        frame(
            r,
            if r["method"] == "session.snapshot" {
                snapshot(2)
            } else {
                process(
                    r,
                    json!([
                        {"pid":1,"name":"SECRET_NAME","argv0":"SECRET_ARGV0","argv":["/SECRET_PATH/claude","SECRET_PROMPT"],"cmdline":"SECRET_CMDLINE","cwd":"/SECRET_CWD","tty":"SECRET_TTY"},
                        {"pid":2,"name":"node","argv":["node","/private/codex.js","--token=SECRET_TOKEN"]},
                        {"pid":3,"name":"claude","argv":["claude"]},
                        {"pid":4,"name":"codex","argv0":"claude","argv":["unrecognized","codex"]}
                    ]),
                )
            },
        )
    });
    let (output, report) = server.run(&[]);
    assert_eq!(output.status.code(), Some(0), "{report}");
    assert_eq!(report["contract"], "coven.agent-discovery.v1");
    assert_eq!(report["target_count"], 2);
    assert_eq!(report["recognized_target_count"], 2);
    assert_eq!(
        report["targets"][0]["provider_candidates"],
        json!(["claude", "codex"])
    );
    assert_eq!(report["targets"][0]["matches"].as_array().unwrap().len(), 3);
    assert_eq!(
        report["targets"][0]["recognition"],
        "multiple_provider_families"
    );
    assert_eq!(report["authorization"], "none");
    assert_eq!(report["dispatchable"], false);
    assert_eq!(report["native_catalog_complete"], false);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("SECRET"));
    let (_, next) = server.run(&[]);
    assert_ne!(report["scan_id"], next["scan_id"]);
    assert_ne!(
        report["targets"][0]["observation_id"],
        next["targets"][0]["observation_id"]
    );
    assert_eq!(
        server.methods.lock().unwrap().as_slice(),
        [
            "session.snapshot",
            "pane.process_info",
            "pane.process_info",
            "session.snapshot",
            "session.snapshot",
            "pane.process_info",
            "pane.process_info",
            "session.snapshot"
        ]
    );
}

#[test]
fn capability_is_not_inferred_from_host_or_agent_hints() {
    for change in ["missing", "windows", "selection", "version"] {
        let server = Server::new(move |_, r| {
            let mut s = snapshot(1);
            match change {
                "missing" => {
                    s["snapshot"]
                        .as_object_mut()
                        .unwrap()
                        .remove("process_observation");
                }
                "windows" => s["snapshot"]["process_observation"]["platform"] = json!("windows"),
                "selection" => {
                    s["snapshot"]["process_observation"]["selection"] = json!("selected_agent")
                }
                _ => s["snapshot"]["process_observation"]["version"] = json!(2),
            }
            frame(r, s)
        });
        let (out, report) = server.run(&[]);
        failed(&out, &report, 2, "process_observation_unsupported");
        assert_eq!(report["assessment"], "incompatible");
        assert_eq!(server.methods.lock().unwrap().len(), 1);
    }
}

#[test]
fn whole_sample_is_discarded_on_mapping_server_or_descriptor_drift() {
    for change in [
        "mapping",
        "pane",
        "workspace",
        "tab",
        "version",
        "protocol",
        "descriptor",
        "process",
    ] {
        let server = Server::new(move |index, r| {
            let mut result = if r["method"] == "session.snapshot" {
                snapshot(2)
            } else {
                process(r, json!([{"pid":1,"name":"claude"}]))
            };
            if index == 3 {
                match change {
                    "mapping" => result["snapshot"]["panes"][1]["terminal_id"] = json!("changed"),
                    "pane" => result["snapshot"]["panes"][1]["pane_id"] = json!("changed"),
                    "workspace" => {
                        result["snapshot"]["panes"][1]["workspace_id"] = json!("changed")
                    }
                    "tab" => result["snapshot"]["panes"][1]["tab_id"] = json!("changed"),
                    "version" => result["snapshot"]["version"] = json!("different"),
                    "protocol" => result["snapshot"]["protocol"] = json!(19),
                    "descriptor" => {
                        result["snapshot"]["process_observation"]["selection"] = json!("ranked")
                    }
                    _ => {}
                }
            }
            if change == "process" && index == 2 {
                result["process_info"]["pane_id"] = json!("wrong");
            }
            frame(r, result)
        });
        let (out, report) = server.run(&[]);
        let (exit, code) = match change {
            "mapping" | "pane" | "workspace" | "tab" => (1, "pane_mapping_changed"),
            "version" => (1, "server_changed"),
            "protocol" => (2, "unsupported_protocol"),
            "descriptor" => (2, "process_observation_unsupported"),
            _ => (1, "schema_error"),
        };
        failed(&out, &report, exit, code);
    }
}

#[test]
fn only_the_supported_protocol_is_admitted_before_process_reads() {
    for version in [17, 19] {
        let server = Server::new(move |_, r| {
            let mut s = snapshot(1);
            s["snapshot"]["protocol"] = json!(version);
            frame(r, s)
        });
        let (out, report) = server.run(&[]);
        failed(&out, &report, 2, "unsupported_protocol");
        assert_eq!(server.methods.lock().unwrap().len(), 1);
    }
}

#[test]
fn native_nullable_process_fields_are_valid_but_malformed_top_level_tty_is_not() {
    for malformed in [false, true] {
        let server = Server::new(move |_, r| {
            if r["method"] == "session.snapshot" {
                return frame(r, snapshot(1));
            }
            let mut p = process(
                r,
                json!([{"pid":1,"name":"claude","argv0":null,
                "argv":null,"cmdline":null,"cwd":null,"tty":{"opaque":"SECRET"}}]),
            );
            p["process_info"]["tty"] = if malformed { json!(false) } else { Value::Null };
            frame(r, p)
        });
        let (out, report) = server.run(&[]);
        if malformed {
            failed(&out, &report, 1, "schema_error");
        } else {
            assert_eq!(out.status.code(), Some(0));
            assert_eq!(report["targets"][0]["matches"][0]["provider"], "claude");
            assert!(!String::from_utf8_lossy(&out.stdout).contains("SECRET"));
        }
    }
}

#[test]
fn human_discovery_lists_provider_hints_without_process_secrets() {
    let server = Server::new(|_, r| {
        frame(
            r,
            if r["method"] == "session.snapshot" {
                snapshot(1)
            } else {
                process(
                    r,
                    json!([
                        {"pid":1,"name":"SECRET_NAME","argv":["/SECRET_PATH/claude","SECRET_PROMPT"]},
                        {"pid":2,"name":"codex"}
                    ]),
                )
            },
        )
    });
    let out = Command::new(env!("CARGO_BIN_EXE_coven"))
        .args(["agent", "discover", "--socket"])
        .arg(server.dir.path().join("kit.sock"))
        .env("HOME", server.dir.path())
        .env("PATH", "/nonexistent")
        .current_dir(server.dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    for expected in ["p0", "t0", "claude pid 1", "codex pid 2", "unknown"] {
        assert!(text.contains(expected), "missing {expected}");
    }
    assert!(!text.contains("SECRET"));
}

#[test]
fn selected_pane_is_bracketed_and_expected_terminal_is_checked_before_process_read() {
    let server = Server::new(|_, r| {
        frame(
            r,
            if r["method"] == "session.snapshot" {
                snapshot(2)
            } else {
                process(r, json!([]))
            },
        )
    });
    let (out, report) = server.run(&["--pane", "p1", "--expected-terminal", "t1"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(report["target_count"], 1);
    assert_eq!(report["targets"][0]["pane_id"], "p1");
    let (out, report) = server.run(&["--pane", "p1", "--expected-terminal", "wrong"]);
    failed(&out, &report, 1, "pane_mapping_changed");
    assert_eq!(server.methods.lock().unwrap().len(), 4);
}

#[test]
fn only_successful_bracketed_empty_inventory_is_zero() {
    let server = Server::new(|_, r| frame(r, snapshot(0)));
    let (out, report) = server.run(&[]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(report["target_count"], 0);
    assert_eq!(server.methods.lock().unwrap().len(), 2);
    let path = server.dir.path().join("missing.sock");
    let output = Command::new(env!("CARGO_BIN_EXE_coven"))
        .args(["agent", "discover", "--socket"])
        .arg(path)
        .arg("--json")
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(report["target_count"].is_null());
}

#[test]
fn malformed_and_uncorrelated_responses_are_not_observations() {
    for mode in ["malformed", "wrong_id", "duplicate", "nonfinite", "error"] {
        let server = Server::new(move |_, r| match mode {
            "malformed" => b"{\n".to_vec(),
            "wrong_id" => frame(&json!({"id":"wrong"}), snapshot(0)),
            "duplicate" => b"{\"id\":1,\"id\":2}\n".to_vec(),
            "nonfinite" => b"{\"ignored\":NaN}\n".to_vec(),
            _ => format!(
                "{{\"id\":{},\"error\":{{\"code\":\"SECRET\",\"message\":\"SECRET\"}}}}\n",
                r["id"]
            )
            .into_bytes(),
        });
        let (out, report) = server.run(&[]);
        failed(
            &out,
            &report,
            1,
            match mode {
                "malformed" => "invalid_json",
                "duplicate" => "duplicate_key",
                "nonfinite" => "nonfinite_json",
                "error" => "server_error",
                _ => "schema_error",
            },
        );
        assert!(!String::from_utf8_lossy(&out.stdout).contains("SECRET"));
    }
}

#[test]
fn aggregate_budget_and_shared_deadline_include_all_reads() {
    let server = Server::new(|_, r| {
        let mut result = if r["method"] == "session.snapshot" {
            snapshot(1)
        } else {
            process(r, json!([]))
        };
        result["padding"] = json!("x".repeat(600));
        frame(r, result)
    });
    let (out, report) = server.run(&["--max-response-bytes", "1600"]);
    failed(&out, &report, 1, "response_limit");
    let server = Server::new(|_, r| {
        thread::sleep(Duration::from_millis(70));
        frame(
            r,
            if r["method"] == "session.snapshot" {
                snapshot(1)
            } else {
                process(r, json!([]))
            },
        )
    });
    let started = Instant::now();
    let (out, report) = server.run(&["--timeout", "0.12"]);
    failed(&out, &report, 1, "timeout");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn cohort_and_process_caps_are_inclusive_and_no_partial_success_escapes() {
    for count in [64, 65] {
        let server = Server::new(move |_, r| {
            frame(
                r,
                if r["method"] == "session.snapshot" {
                    snapshot(count)
                } else {
                    process(r, json!([]))
                },
            )
        });
        let (out, report) = server.run(&[]);
        if count == 64 {
            assert_eq!(out.status.code(), Some(0), "{report}");
            assert_eq!(report["target_count"], 64);
            assert_eq!(server.methods.lock().unwrap().len(), 66);
        } else {
            failed(&out, &report, 1, "pane_limit");
        }
    }
    for count in [256, 257] {
        let server = Server::new(move |_, r| {
            frame(
                r,
                if r["method"] == "session.snapshot" {
                    snapshot(1)
                } else {
                    process(
                        r,
                        json!(
                            (1..=count)
                                .map(|pid| json!({"pid":pid,"name":"claude"}))
                                .collect::<Vec<_>>()
                        ),
                    )
                },
            )
        });
        let (out, report) = server.run(&[]);
        if count == 256 {
            assert_eq!(out.status.code(), Some(0), "{report}");
            assert_eq!(
                report["targets"][0]["matches"].as_array().unwrap().len(),
                256
            );
        } else {
            failed(&out, &report, 1, "process_limit");
        }
    }
}

#[test]
fn sigint_returns_a_redacted_interrupted_report_and_exit_130() {
    let server = Server::new(|_, r| {
        thread::sleep(Duration::from_millis(300));
        frame(r, snapshot(0))
    });
    let child = server
        .command()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(2);
    while server.methods.lock().unwrap().is_empty() {
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGINT) }, 0);
    let output = child.wait_with_output().unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    failed(&output, &report, 130, "interrupted");
}

#[test]
fn endpoint_replacement_during_read_discards_the_whole_sample() {
    let path = Arc::new(Mutex::new(None::<std::path::PathBuf>));
    let replace = path.clone();
    let server = Server::new(move |index, r| {
        if index == 2 {
            let path = replace.lock().unwrap().clone().unwrap();
            std::fs::rename(&path, path.with_extension("old")).unwrap();
            let _replacement = UnixListener::bind(&path).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        frame(
            r,
            if r["method"] == "session.snapshot" {
                snapshot(2)
            } else {
                process(r, json!([{"pid":1,"name":"claude"}]))
            },
        )
    });
    *path.lock().unwrap() = Some(server.dir.path().join("kit.sock"));
    let (out, report) = server.run(&[]);
    failed(&out, &report, 1, "endpoint_replaced");
}

#[test]
fn consumed_process_schema_errors_clear_previously_collected_targets() {
    for change in [
        "duplicate_pid",
        "argv_type",
        "arg_bytes",
        "arg_count",
        "text_budget",
        "cmdline",
    ] {
        let server = Server::new(move |index, r| {
            let result = if r["method"] == "session.snapshot" {
                snapshot(2)
            } else {
                let mut rows = json!([{"pid":1,"name":"claude","argv":["claude","legal secret"]}]);
                if index == 2 {
                    match change {
                        "duplicate_pid" => {
                            let duplicate = rows[0].clone();
                            rows.as_array_mut().unwrap().push(duplicate);
                        }
                        "argv_type" => rows[0]["argv"] = json!("SECRET"),
                        "arg_bytes" => rows[0]["argv"] = json!(["é".repeat(2049)]),
                        "arg_count" => rows[0]["argv"] = json!(vec!["secret"; 129]),
                        "text_budget" => rows[0]["argv"] = json!(vec!["x".repeat(4096); 17]),
                        _ => rows[0]["cmdline"] = json!(false),
                    }
                }
                process(r, rows)
            };
            frame(r, result)
        });
        let (out, report) = server.run(&[]);
        failed(&out, &report, 1, "schema_error");
        assert!(!String::from_utf8_lossy(&out.stdout).contains("SECRET"));
    }
}

#[test]
fn unsafe_socket_is_refused_without_any_request() {
    use std::os::unix::fs::PermissionsExt;
    let server = Server::new(|_, _| panic!("unsafe endpoint must not be contacted"));
    std::fs::set_permissions(
        server.dir.path().join("kit.sock"),
        std::fs::Permissions::from_mode(0o666),
    )
    .unwrap();
    let (out, report) = server.run(&[]);
    failed(&out, &report, 1, "unsafe_endpoint");
    assert!(server.methods.lock().unwrap().is_empty());
}
