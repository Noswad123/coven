#![cfg(unix)]
//! Offline CLI contracts only: private sockets, no provider or native execution.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

fn command(socket: &str) -> Command {
    let mut command = base(socket);
    command.args(["--duration", "0.25", "--interval", "0.1"]);
    command
}

fn base(socket: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_coven"));
    command.args(["agent", "discover", "--socket", socket, "--watch"]);
    command
}

#[test]
fn jsonl_samples_then_exactly_one_summary_and_final_lf() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("absent.sock");
    let out = command(socket.to_str().unwrap())
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    assert_eq!(out.stdout.last(), Some(&b'\n'));
    assert!(!out.stdout.ends_with(b"\n\n"));
    let text = String::from_utf8(out.stdout).unwrap();
    let records: Vec<Value> = text
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert!(records.len() >= 2);
    for sample in &records[..records.len() - 1] {
        assert_eq!(sample["contract"], "coven.agent-discovery.v1");
        assert_eq!(sample["assessment"], "unavailable");
        assert_eq!(sample["targets"], serde_json::json!([]));
    }
    let summary = records.last().unwrap();
    assert_eq!(summary["contract"], "coven.agent-discovery-watch.v1");
    assert_eq!(summary["outcome"], "completed");
    assert_eq!(
        summary["samples"].as_u64().unwrap(),
        (records.len() - 1) as u64
    );
    assert_eq!(summary["unavailable"], summary["samples"]);
}

#[test]
fn optional_missing_ui_does_not_change_operational_exit_status() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("absent.sock");
    let out = command(socket.to_str().unwrap())
        .args(["--present", "--source", "coven-discovery", "--json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let summary: Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!(summary["outcome"], "completed");
    assert_eq!(summary["presentation"]["outcome"], "failed");
}

#[test]
fn invalid_presentation_timing_never_connects() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("kit.sock");
    let listener = UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    listener.set_nonblocking(true).unwrap();
    for option in [["--interval", "2.1"], ["--timeout", "3.1"]] {
        let out = base(path.to_str().unwrap())
            .args([
                "--duration",
                "0.25",
                "--present",
                "--source",
                "coven-discovery",
            ])
            .args(option)
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(listener.accept().is_err());
    }
}

#[test]
fn sigint_during_blocked_collection_is_bounded_and_130() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("kit.sock");
    let listener = UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut child = base(path.to_str().unwrap())
        .args([
            "--duration",
            "60",
            "--interval",
            "0.1",
            "--timeout",
            "3",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let admitted = Instant::now() + Duration::from_secs(3);
    let (stream, _) = loop {
        if let Ok(pair) = listener.accept() {
            break pair;
        }
        if Instant::now() >= admitted {
            let _ = child.kill();
            panic!("collector did not connect");
        }
        thread::sleep(Duration::from_millis(5));
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut line = String::new();
    BufReader::new(stream.try_clone().unwrap())
        .read_line(&mut line)
        .unwrap();
    let request: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(request["method"], "session.snapshot");
    let interrupted = Instant::now();
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGINT) }, 0);
    while child.try_wait().unwrap().is_none() {
        if interrupted.elapsed() > Duration::from_secs(2) {
            let _ = child.kill();
            panic!("unbounded interruption");
        }
        thread::sleep(Duration::from_millis(5));
    }
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(130));
    let text = String::from_utf8(out.stdout).unwrap();
    let summary: Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!(summary["outcome"], "interrupted");
    assert!(listener.accept().is_err());
}

#[test]
fn human_stream_contains_no_control_sequences() {
    let root = tempfile::tempdir().unwrap();
    let out = command(root.path().join("absent.sock").to_str().unwrap())
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("Discovery sample 1: unavailable; 0 observed targets"));
    assert!(!text.chars().any(|c| c.is_control() && c != '\n'));
}

struct Kit {
    _root: tempfile::TempDir,
    path: String,
    calls: Arc<Mutex<Vec<Value>>>,
    build: Arc<Mutex<Value>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

fn build_view() -> Value {
    json!({"lease":"independent-build-lease","epoch":"build-epoch","revision":9,
        "items":[{"id":"build:job","label":"Build"}],
        "intents":[{"epoch":"build-epoch","revision":9,"item_id":"build:job"}]})
}

#[derive(Clone, Copy, Debug)]
enum Profile {
    Linux,
    Mac,
}

impl Profile {
    fn descriptor(self) -> Value {
        match self {
            Self::Linux => {
                json!({"version":1,"platform":"linux","scope":"pane_foreground_cohort","selection":"unclassified"})
            }
            Self::Mac => {
                json!({"version":2,"platform":"macos","scope":"pane_foreground_cohort","selection":"unclassified","argument_policy":"darwin_procargs2_lp64_v1"})
            }
        }
    }
}

// Keep the original Linux test names and count each macOS scenario explicitly.
macro_rules! profiles {
    ($linux:ident, $mac:ident, $profile:ident, $body:block) => {
        #[test]
        fn $linux() {
            let $profile = Profile::Linux;
            $body
        }
        #[test]
        fn $mac() {
            let $profile = Profile::Mac;
            $body
        }
    };
}

impl Kit {
    fn new(mode: &'static str, profile: Profile) -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("kit.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let build = Arc::new(Mutex::new(build_view()));
        let stop = Arc::new(AtomicBool::new(false));
        let recorded = calls.clone();
        let stopped = stop.clone();
        let build_state = build.clone();
        let worker = thread::spawn(move || {
            let mut revision = 0;
            let mut items = json!([]);
            let mut pending = Vec::<Value>::new();
            let mut collections = 0;
            let mut polls = 0;
            while !stopped.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
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
                recorded.lock().unwrap().push(request.clone());
                let method = request["method"].as_str().unwrap();
                if method == "integration.view.poll" {
                    polls += 1;
                }
                let params = &request["params"];
                if method.starts_with("integration.view.") {
                    assert_eq!(params["source"], "coven-discovery");
                    if method != "integration.view.open" {
                        assert_eq!(params["lease"], "discovery-lease");
                        assert_eq!(params["epoch"], "discovery-epoch");
                    }
                }
                if mode == method
                    || (mode == "uncertain_pre_poll"
                        && method == "integration.view.poll"
                        && polls == 2)
                {
                    continue;
                }
                if (mode == "busy" && method == "integration.view.open")
                    || (mode == "lost_lease" && method == "integration.view.poll")
                {
                    let reply = json!({"id":request["id"],"error":{"code":"sourceBusy","message":"PRIVATE"}});
                    let _ = writeln!(stream, "{reply}");
                    continue;
                }
                let result = match method {
                    "session.snapshot" => {
                        let mut snapshot = json!({"version":"fixture","protocol":18,
                            "selected_integration_source":"build",
                            "process_observation":{"version":1,"platform":"linux","scope":"pane_foreground_cohort","selection":"unclassified"},
                            "integration_views":{"version":1,"sources":[{"id":"build","epoch":"build-epoch"},{"id":"coven-discovery","epoch":null}]},
                            "panes":[{"pane_id":"p","terminal_id":"t","workspace_id":"w","tab_id":"tab", "title":"PRIVATE", "cwd":"PRIVATE", "agent_session":"PRIVATE"}]});
                        snapshot["process_observation"] = profile.descriptor();
                        if mode == "legacy_mac" || mode == "legacy_mac_marker" {
                            snapshot["process_observation"] = Profile::Mac.descriptor();
                            snapshot["process_observation"]["version"] = json!(1);
                            if mode == "legacy_mac" {
                                snapshot["process_observation"]
                                    .as_object_mut()
                                    .unwrap()
                                    .remove("argument_policy");
                            }
                        }
                        match mode {
                            "windows" => {
                                snapshot["process_observation"]["platform"] = json!("windows")
                            }
                            "selected" => {
                                snapshot["process_observation"]["scope"] = json!("selected_agent")
                            }
                            "bad_version" => {
                                snapshot["process_observation"]["version"] = json!(true)
                            }
                            "missing_policy" => {
                                snapshot["process_observation"]
                                    .as_object_mut()
                                    .unwrap()
                                    .remove("argument_policy");
                            }
                            "bad_policy" => {
                                snapshot["process_observation"]["argument_policy"] = json!("legacy")
                            }
                            "mapping_drift" if collections % 2 == 1 => {
                                snapshot["panes"][0]["terminal_id"] = json!("changed")
                            }
                            "descriptor_drift" if collections % 2 == 1 => {
                                snapshot["process_observation"] = match profile {
                                    Profile::Linux => Profile::Mac,
                                    Profile::Mac => Profile::Linux,
                                }
                                .descriptor()
                            }
                            _ => {}
                        }
                        if mode == "incompatible" {
                            snapshot["process_observation"]["selection"] = json!("legacy_ranked");
                        }
                        if mode == "unsupported_ui" {
                            snapshot["integration_views"] = Value::Null;
                        }
                        json!({"type":"session_snapshot","snapshot":snapshot})
                    }
                    "pane.process_info" => {
                        collections += 1;
                        assert_eq!(params["pane_id"], "p");
                        if mode == "outage" && collections > 2 {
                            continue;
                        }
                        if mode == "peer_error" {
                            let reply = json!({"id":request["id"],"error":{"code":"privatePeer","message":"PRIVATE /Volumes/other-machine/SECRET"}});
                            let _ = writeln!(stream, "{reply}");
                            continue;
                        }
                        let mut processes = vec![
                            json!({"pid":10,"name":"PRIVATE","argv":["/private/claude","PRIVATE"],"cwd":"PRIVATE"}),
                            json!({"pid":11,"name":"PRIVATE","argv":["/private/codex"]}),
                            json!({"pid":12,"name":"PRIVATE","argv":["/private/gemini"]}),
                        ];
                        if mode == "cap" {
                            processes = (1..=200)
                                .map(|pid| json!({"pid":pid,"name":"PRIVATE","argv":["claude"]}))
                                .collect();
                        }
                        if matches!(profile, Profile::Mac) {
                            for (index, process) in processes.iter_mut().enumerate() {
                                process["pid"] = json!(u32::MAX - index as u32);
                                process["cwd"] = json!("/Volumes/other-machine/SECRET");
                                if let Some(args) = process["argv"].as_array_mut() {
                                    args[0] = json!(format!(
                                        "/Volumes/other-machine/SECRET/{}",
                                        args[0].as_str().unwrap().rsplit('/').next().unwrap()
                                    ));
                                }
                            }
                        }
                        if mode == "name_only" {
                            processes = vec![
                                json!({"pid":u32::MAX,"name":"claude","argv0":null,"argv":null}),
                            ];
                        }
                        if mode == "empty_argv0" {
                            processes = vec![
                                json!({"pid":u32::MAX,"name":"claude","argv0":"codex","argv":["","node"]}),
                            ];
                        }
                        if mode == "candidate_drift" && collections > 1 {
                            processes[0]["pid"] = json!(999999);
                        }
                        let mut info = json!({"pane_id":"p","foreground_processes":processes});
                        info["environment"] = json!({"EnvironmentalTailSentinel":"PRIVATE"});
                        info["foreground_generation"] = json!("PRIVATE");
                        info["native_session_id"] = json!("PRIVATE");
                        info["cohort_complete"] = json!(false);
                        info["context"] = json!({"String":"/Volumes/other-machine/SECRET"});
                        info["participant_id"] = json!("PRIVATE");
                        let result = json!({"type":"pane_process_info","process_info":info});
                        if mode == "native_unknown" {
                            // Exercise the native number codec, not serde_json's number profile.
                            let wire = json!({"id":request["id"],"result":result}).to_string();
                            let wire = wire.replacen("\"process_info\":{", "\"process_info\":{\"EnvironmentalTailSentinel\":9999999999999999999999999999999999999999,", 1);
                            let _ = writeln!(stream, "{wire}");
                            continue;
                        }
                        result
                    }
                    "integration.view.open" => {
                        json!({"type":"view_opened","lease":"discovery-lease","epoch":"discovery-epoch","revision":0,"ttl_ms":5000})
                    }
                    "integration.view.replace" => {
                        revision = params["revision"].as_u64().unwrap();
                        items = params["items"].clone();
                        pending.clear(); // Kit replace clears its real intent queue.
                        assert!(items.as_array().unwrap().len() <= 128);
                        assert!(serde_json::to_vec(params).unwrap().len() <= 96 * 1024);
                        json!({"type":"view_replaced","epoch":"discovery-epoch","revision":revision})
                    }
                    "integration.view.poll" => {
                        let intents = std::mem::take(&mut pending);
                        json!({"type":"view_intents","epoch":"discovery-epoch","revision":revision,"ttl_ms":5000,"intents":intents})
                    }
                    "integration.view.close" => json!({"type":"view_closed"}),
                    _ => panic!("unexpected mutation or execution method: {method}"),
                };
                let reply = json!({"id":request["id"],"result":result});
                let _ = writeln!(stream, "{reply}");
                if method == "integration.view.poll" && !items[1]["id"].is_null() {
                    // Queue clicks between poll N and collection N+1, not in its reply.
                    pending = vec![
                        json!({"epoch":"old","revision":revision,"item_id":items[1]["id"]}),
                        json!({"epoch":"discovery-epoch","revision":revision - 1,"item_id":items[1]["id"]}),
                        json!({"epoch":"discovery-epoch","revision":revision,"item_id":"missing"}),
                        json!({"epoch":"discovery-epoch","revision":revision,"item_id":items[1]["id"]}),
                    ];
                }
                assert_eq!(*build_state.lock().unwrap(), build_view());
            }
        });
        Self {
            _root: root,
            path: path.to_str().unwrap().into(),
            calls,
            build,
            stop,
            worker: Some(worker),
        }
    }

    fn watch(&self) -> Vec<Value> {
        let out = base(&self.path)
            .args([
                "--duration",
                "0.45",
                "--interval",
                "0.1",
                "--present",
                "--source",
                "coven-discovery",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty());
        assert_eq!(out.stdout.last(), Some(&b'\n'));
        assert!(!out.stdout.ends_with(b"\n\n"));
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(!text.contains("PRIVATE"));
        assert!(!text.contains("agent_session"));
        for secret in [
            "SECRET",
            "EnvironmentalTailSentinel",
            "foreground_generation",
            "native_session_id",
            "cohort_complete",
            "participant_id",
        ] {
            assert!(!text.contains(secret), "leaked {secret}");
        }
        let calls = self.calls.lock().unwrap();
        assert!(!serde_json::to_string(&*calls).unwrap().contains("SECRET"));
        for frame in calls
            .iter()
            .filter(|r| r["method"] == "integration.view.replace")
        {
            for row in frame["params"]["items"].as_array().unwrap() {
                assert_eq!(row.as_object().unwrap().len(), 3);
                assert!(row["label"].is_string() && row["detail"].is_string());
                let id = row["id"].as_str().unwrap();
                if id != "summary" {
                    let hash = id.strip_prefix("candidate:").unwrap();
                    assert_eq!(hash.len(), 64);
                    assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
                }
            }
        }
        drop(calls);
        assert_eq!(*self.build.lock().unwrap(), build_view());
        text.lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
}

impl Drop for Kit {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

profiles!(
    mixed_families_selection_and_build_lease_are_independent,
    mac_mixed_families_selection_and_build_lease_are_independent,
    profile,
    {
        let kit = Kit::new("success", profile);
        let records = kit.watch();
        let first = &records[0]["targets"][0];
        assert_eq!(first["recognition"], "multiple_provider_families");
        assert_eq!(first["matches"].as_array().unwrap().len(), 3);
        assert_eq!(
            first["provider_candidates"],
            json!(["claude", "codex", "gemini"])
        );
        for key in ["native_identity", "native_lifecycle", "job_completion"] {
            assert_eq!(first[key], "unestablished");
        }
        let summary = records.last().unwrap();
        assert_eq!(summary["presentation"]["outcome"], "completed");
        assert!(summary["presentation"]["selections"].as_u64().unwrap() >= 1);
        assert!(summary["presentation"]["ignored_intents"].as_u64().unwrap() >= 1);
        let calls = kit.calls.lock().unwrap();
        let frames: Vec<_> = calls
            .iter()
            .filter(|r| r["method"] == "integration.view.replace")
            .collect();
        assert_eq!(frames[0]["params"]["items"].as_array().unwrap().len(), 4);
        let first_frame = calls
            .iter()
            .position(|r| r["method"] == "integration.view.replace")
            .unwrap();
        assert_eq!(calls[first_frame + 1]["method"], "integration.view.poll");
        // The second poll drains revision 1's queued click before replace clears it.
        let second_frame = calls
            .iter()
            .enumerate()
            .skip(first_frame + 1)
            .find(|(_, r)| r["method"] == "integration.view.replace")
            .unwrap()
            .0;
        assert_eq!(calls[second_frame - 1]["method"], "integration.view.poll");
        assert!(
            frames[1]["params"]["items"][1]["detail"]
                .as_str()
                .unwrap()
                .contains("Selected (read-only)")
        );
    }
);

profiles!(
    outages_after_positives_clear_samples_and_entire_frame,
    mac_outages_after_positives_clear_samples_and_entire_frame,
    profile,
    {
        let kit = Kit::new("outage", profile);
        let records = kit.watch();
        assert_eq!(records[0]["assessment"], "observed");
        assert_eq!(records[2]["assessment"], "unavailable");
        assert_eq!(records[2]["targets"], json!([]));
        let summary = records.last().unwrap();
        assert!(summary["unavailable"].as_u64().unwrap() > 0);
        let calls = kit.calls.lock().unwrap();
        let frame = calls
            .iter()
            .rev()
            .find(|r| r["method"] == "integration.view.replace")
            .unwrap();
        assert_eq!(frame["params"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            frame["params"]["items"][0]["label"],
            "Agent discovery: unavailable"
        );
        let failed_read = calls
            .iter()
            .enumerate()
            .filter(|(_, r)| r["method"] == "pane.process_info")
            .nth(2)
            .unwrap()
            .0;
        let first_ui = calls
            .iter()
            .skip(failed_read + 1)
            .find(|r| {
                r["method"]
                    .as_str()
                    .unwrap()
                    .starts_with("integration.view.")
            })
            .unwrap();
        assert_eq!(first_ui["method"], "integration.view.replace");
        assert_eq!(first_ui["params"]["items"].as_array().unwrap().len(), 1);
    }
);

profiles!(
    ui_cap_discloses_omitted_matches_without_truncating_observations,
    mac_ui_cap_discloses_omitted_matches_without_truncating_observations,
    profile,
    {
        let kit = Kit::new("cap", profile);
        let records = kit.watch();
        assert_eq!(
            records[0]["targets"][0]["matches"]
                .as_array()
                .unwrap()
                .len(),
            200
        );
        assert_eq!(
            records.last().unwrap()["presentation"]["omitted_matches"],
            73
        );
        let calls = kit.calls.lock().unwrap();
        let frame = calls
            .iter()
            .find(|r| r["method"] == "integration.view.replace")
            .unwrap();
        assert_eq!(frame["params"]["items"].as_array().unwrap().len(), 128);
        assert!(
            frame["params"]["items"][0]["detail"]
                .as_str()
                .unwrap()
                .contains("73 omitted matches")
        );
    }
);

profiles!(
    incompatible_collector_can_present_status_without_process_capability,
    mac_incompatible_collector_can_present_status_without_process_capability,
    profile,
    {
        let kit = Kit::new("incompatible", profile);
        let records = kit.watch();
        assert_eq!(records[0]["assessment"], "incompatible");
        assert!(records.last().unwrap()["incompatible"].as_u64().unwrap() > 0);
        let calls = kit.calls.lock().unwrap();
        assert!(!calls.iter().any(|r| r["method"] == "pane.process_info"));
        let frame = calls
            .iter()
            .find(|r| r["method"] == "integration.view.replace")
            .unwrap();
        assert_eq!(
            frame["params"]["items"][0]["label"],
            "Agent discovery: incompatible"
        );
    }
);

profiles!(
    ui_busy_unsupported_and_uncertain_never_interrupt_core_or_reopen,
    mac_ui_busy_unsupported_and_uncertain_never_interrupt_core_or_reopen,
    profile,
    {
        for mode in [
            "busy",
            "unsupported_ui",
            "integration.view.open",
            "integration.view.replace",
            "integration.view.poll",
        ] {
            let kit = Kit::new(mode, profile);
            let records = kit.watch();
            assert!(
                records[..records.len() - 1]
                    .iter()
                    .all(|r| r["assessment"] == "observed")
            );
            let summary = records.last().unwrap();
            assert!(summary["samples"].as_u64().unwrap() >= 3);
            assert_eq!(summary["outcome"], "completed");
            assert_eq!(
                summary["presentation"]["outcome"],
                if mode.starts_with("integration.") {
                    "uncertain"
                } else {
                    "failed"
                }
            );
            let calls = kit.calls.lock().unwrap();
            assert!(
                calls
                    .iter()
                    .filter(|r| r["method"] == "integration.view.open")
                    .count()
                    <= 1
            );
            assert!(
                !calls
                    .iter()
                    .any(|r| r["method"] == "integration.view.close")
            );
            if mode.starts_with("integration.") {
                assert_eq!(calls.iter().filter(|r| r["method"] == mode).count(), 1);
                assert_eq!(
                    calls
                        .iter()
                        .rev()
                        .find(|r| r["method"].as_str().unwrap().starts_with("integration."))
                        .unwrap()["method"],
                    mode
                );
            }
        }
    }
);

profiles!(
    uncertain_close_is_reported_once_without_changing_core_status,
    mac_uncertain_close_is_reported_once_without_changing_core_status,
    profile,
    {
        let kit = Kit::new("integration.view.close", profile);
        let records = kit.watch();
        let summary = records.last().unwrap();
        assert_eq!(summary["outcome"], "completed");
        assert_eq!(summary["presentation"]["outcome"], "uncertain");
        assert_eq!(summary["presentation"]["cleanup"], "ttl_only_uncertain");
        let calls = kit.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|r| r["method"] == "integration.view.close")
                .count(),
            1
        );
        assert_eq!(calls.last().unwrap()["method"], "integration.view.close");
    }
);

profiles!(
    uncertain_pre_poll_never_replaces_closes_or_reopens_and_core_continues,
    mac_uncertain_pre_poll_never_replaces_closes_or_reopens_and_core_continues,
    profile,
    {
        let kit = Kit::new("uncertain_pre_poll", profile);
        let records = kit.watch();
        let summary = records.last().unwrap();
        assert_eq!(summary["outcome"], "completed");
        assert_eq!(summary["presentation"]["outcome"], "uncertain");
        assert!(summary["samples"].as_u64().unwrap() >= 3);
        let calls = kit.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|r| r["method"] == "integration.view.open")
                .count(),
            1
        );
        assert_eq!(
            calls
                .iter()
                .filter(|r| r["method"] == "integration.view.replace")
                .count(),
            1
        );
        assert_eq!(
            calls
                .iter()
                .filter(|r| r["method"] == "integration.view.poll")
                .count(),
            2
        );
        assert!(
            !calls
                .iter()
                .any(|r| r["method"] == "integration.view.close")
        );
        assert_eq!(
            calls
                .iter()
                .rev()
                .find(|r| r["method"].as_str().unwrap().starts_with("integration."))
                .unwrap()["method"],
            "integration.view.poll"
        );
    }
);

profiles!(
    lost_ui_lease_stops_ui_but_discovery_continues,
    mac_lost_ui_lease_stops_ui_but_discovery_continues,
    profile,
    {
        let kit = Kit::new("lost_lease", profile);
        let records = kit.watch();
        assert!(
            records[..records.len() - 1]
                .iter()
                .all(|r| r["assessment"] == "observed")
        );
        let summary = records.last().unwrap();
        assert_eq!(summary["presentation"]["outcome"], "failed");
        assert_eq!(summary["outcome"], "completed");
        assert!(summary["samples"].as_u64().unwrap() >= 3);
        let calls = kit.calls.lock().unwrap();
        for method in [
            "integration.view.open",
            "integration.view.replace",
            "integration.view.poll",
            "integration.view.close",
        ] {
            assert_eq!(calls.iter().filter(|r| r["method"] == method).count(), 1);
        }
    }
);

profiles!(
    response_budget_outage_still_publishes_only_unavailable_status,
    mac_response_budget_outage_still_publishes_only_unavailable_status,
    profile,
    {
        let kit = Kit::new("cap", profile);
        let out = base(&kit.path)
            .args([
                "--duration",
                "0.25",
                "--interval",
                "0.1",
                "--max-response-bytes",
                "1024",
                "--present",
                "--source",
                "coven-discovery",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8(out.stdout).unwrap();
        let records: Vec<Value> = text
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        for sample in &records[..records.len() - 1] {
            assert_eq!(sample["assessment"], "unavailable");
            assert_eq!(sample["targets"], json!([]));
            assert_eq!(sample["diagnostics"], json!(["response_limit"]));
        }
        let calls = kit.calls.lock().unwrap();
        for frame in calls
            .iter()
            .filter(|r| r["method"] == "integration.view.replace")
        {
            assert_eq!(frame["params"]["items"].as_array().unwrap().len(), 1);
        }
    }
);

#[test]
fn descriptor_versions_and_policy_are_an_explicit_closed_tuple() {
    for profile in [Profile::Linux, Profile::Mac] {
        for mode in [
            "legacy_mac",
            "legacy_mac_marker",
            "windows",
            "selected",
            "bad_version",
            "bad_policy",
            "missing_policy",
        ] {
            if matches!(profile, Profile::Linux) && matches!(mode, "bad_policy" | "missing_policy")
            {
                continue; // Linux v1 has no argument-policy requirement.
            }
            let kit = Kit::new(mode, profile);
            let records = kit.watch();
            for sample in &records[..records.len() - 1] {
                assert_eq!(sample["assessment"], "incompatible", "{profile:?}/{mode}");
                assert_eq!(sample["targets"], json!([]));
                assert_eq!(
                    sample["diagnostics"],
                    json!(["process_observation_unsupported"])
                );
            }
            assert!(
                !kit.calls
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|r| r["method"] == "pane.process_info")
            );
        }
    }
}

profiles!(
    inventory_and_descriptor_drift_never_publish_candidates,
    mac_inventory_and_descriptor_drift_never_publish_candidates,
    profile,
    {
        for mode in ["mapping_drift", "descriptor_drift"] {
            let kit = Kit::new(mode, profile);
            let records = kit.watch();
            for sample in &records[..records.len() - 1] {
                assert_ne!(sample["assessment"], "observed");
                assert_eq!(sample["targets"], json!([]));
            }
            for frame in kit
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r["method"] == "integration.view.replace")
            {
                assert_eq!(frame["params"]["items"].as_array().unwrap().len(), 1);
            }
        }
    }
);

profiles!(
    candidate_drift_drops_queued_old_revision_selection,
    mac_candidate_drift_drops_queued_old_revision_selection,
    profile,
    {
        let kit = Kit::new("candidate_drift", profile);
        let records = kit.watch();
        let calls = kit.calls.lock().unwrap();
        let frames: Vec<_> = calls
            .iter()
            .filter(|r| r["method"] == "integration.view.replace")
            .collect();
        assert_ne!(
            frames[0]["params"]["items"][1]["id"],
            frames[1]["params"]["items"][1]["id"]
        );
        assert!(
            !frames[1]["params"]["items"]
                .to_string()
                .contains("Selected (read-only)")
        );
        assert!(
            records.last().unwrap()["presentation"]["ignored_intents"]
                .as_u64()
                .unwrap()
                >= 4
        );
    }
);

profiles!(
    private_peer_errors_are_redacted_status_only,
    mac_private_peer_errors_are_redacted_status_only,
    profile,
    {
        let kit = Kit::new("peer_error", profile);
        let records = kit.watch();
        for sample in &records[..records.len() - 1] {
            assert_eq!(sample["assessment"], "unavailable");
            assert_eq!(sample["diagnostics"], json!(["server_error"]));
            assert_eq!(sample["targets"], json!([]));
        }
    }
);

#[test]
fn mac_name_only_is_weak_but_nonempty_argv_masks_poison_fallbacks() {
    for (mode, recognized) in [("name_only", true), ("empty_argv0", false)] {
        let kit = Kit::new(mode, Profile::Mac);
        let records = kit.watch();
        let target = &records[0]["targets"][0];
        assert_eq!(records[0]["assessment"], "observed");
        assert_eq!(
            target["recognition"],
            if recognized {
                "provider_family"
            } else {
                "unknown"
            }
        );
        assert_eq!(
            target["matches"].as_array().unwrap().len(),
            usize::from(recognized)
        );
        assert_eq!(target["native_identity"], "unestablished");
    }
}

profiles!(
    native_unknown_fields_do_not_assert_completeness_or_escape,
    mac_native_unknown_fields_do_not_assert_completeness_or_escape,
    profile,
    {
        let kit = Kit::new("native_unknown", profile);
        let records = kit.watch();
        for sample in &records[..records.len() - 1] {
            assert_eq!(sample["assessment"], "observed");
            assert_eq!(sample["process_observation"], profile.descriptor());
            assert_eq!(sample["native_catalog_complete"], false);
            assert_eq!(sample["authorization"], "none");
            assert_eq!(sample["dispatchable"], false);
            assert_eq!(sample["native_identity"], "unestablished");
        }
        assert_ne!(
            records[0]["targets"][0]["observation_id"],
            records[1]["targets"][0]["observation_id"]
        );
    }
);
