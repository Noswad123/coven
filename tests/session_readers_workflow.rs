#![cfg(unix)]
#[path = "support/session_readers.rs"]
mod support;
use coven::{session_inspection as inspection, session_watch};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::Mutex;
use support::*;

// SIGINT ownership is process-wide, so reader entry points are serialized here.
static READERS: Mutex<()> = Mutex::new(());
fn failure(report: &Value, code: &str) {
    assert_eq!(report["diagnostics"], json!([code]));
    assert_eq!(report["targets"], json!([]));
    assert!(report["target_count"].is_null());
    assert_eq!(report["declarations"]["assessment"], "unavailable");
    assert!(report["declarations"]["revision"].is_null());
}

#[test]
fn native_n_plus_two_privacy_defaults_and_no_workspace_preflight() {
    let _serial = READERS.lock().unwrap();
    let workspace = workspace();
    let peer = Peer::normal(2);
    let request = request(workspace.path(), &peer);
    let before = fs::read_dir(workspace.path().join("state"))
        .unwrap()
        .count();
    let report = inspection::inspect(&request).unwrap();
    assert_eq!(report["assessment"], "observed");
    assert_eq!(report["target_count"], 2);
    assert_eq!(report["targets"][0]["matches"][0]["pid"], 3);
    assert_eq!(report["declarations"]["assessment"], "uninitialized");
    assert_eq!(
        peer.calls()
            .iter()
            .map(|r| r["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "session.snapshot",
            "pane.process_info",
            "pane.process_info",
            "session.snapshot"
        ]
    );
    assert!(!report.to_string().contains("SECRET"));
    assert_eq!(report["native_catalog_complete"], false);
    assert_eq!(report["authorization"], "none");
    let human = inspection::arguments(&request, false, false).unwrap();
    assert!(human.stdout.contains("p0  term0  djinn_executable_family"));
    assert!(!human.stdout.ends_with('\n'));
    let pretty = inspection::arguments(&request, false, true).unwrap();
    assert!(pretty.stdout.starts_with("{\n"));
    assert_eq!(
        fs::read_dir(workspace.path().join("state"))
            .unwrap()
            .count(),
        before
    );
    assert!(!workspace.path().join("state/groups.json").exists());
}

#[test]
fn bounds_terminal_pin_empty_unknown_and_full_failure_clearing() {
    let _serial = READERS.lock().unwrap();
    let workspace = workspace();
    let peer = Peer::normal(65);
    let mut req = request(workspace.path(), &peer);
    req.group = Some("team".into());
    failure(&inspection::inspect(&req).unwrap(), "pane_limit");
    assert_eq!(peer.calls().len(), 1);
    req.pane = Some("p64".into());
    req.expected_terminal = Some("term64".into());
    assert_eq!(
        inspection::inspect(&req).unwrap()["view"]["observed_target_count"],
        1
    );
    req.expected_terminal = Some("wrong".into());
    failure(&inspection::inspect(&req).unwrap(), "terminal_mismatch");
    req.expected_terminal = None;
    req.pane = Some("absent".into());
    failure(&inspection::inspect(&req).unwrap(), "pane_not_found");
    let empty = Peer::normal(0);
    let report = inspection::inspect(&request(workspace.path(), &empty)).unwrap();
    assert_eq!(report["target_count"], 0);
    assert_eq!(empty.calls().len(), 2);
    let unknown = Peer::new(|r, _| {
        reply(
            r,
            if r["method"] == "session.snapshot" {
                json!({"type":"session_snapshot","snapshot":snapshot(1)})
            } else {
                json!({"type":"pane_process_info","process_info":{"pane_id":r["params"]["pane_id"],"foreground_processes":[{"pid":1,"name":"djinn","argv":["node","/private/djinn"]}]}})
            },
        )
    });
    let report = inspection::inspect(&request(workspace.path(), &unknown)).unwrap();
    assert_eq!(report["assessment"], "observed");
    assert_eq!(report["targets"][0]["recognition"], "unknown");
    req = request(workspace.path(), &peer);
    req.max_response_bytes = 1024;
    failure(&inspection::inspect(&req).unwrap(), "response_limit");
}

#[test]
fn mapping_order_context_and_unrelated_inspect_changes() {
    let _serial = READERS.lock().unwrap();
    for mode in [
        "order",
        "selected",
        "unrelated",
        "version",
        "protocol",
        "malformed",
    ] {
        for inspect in [false, true] {
            let workspace = workspace();
            let peer = Peer::new(move |r, index| {
                if r["method"] == "pane.process_info" {
                    if mode == "malformed" {
                        return reply(
                            r,
                            json!({"type":"pane_process_info","process_info":{"pane_id":r["params"]["pane_id"],"foreground_processes":[{"pid":true,"name":"PRIVATE"}]}}),
                        );
                    }
                    return reply(r, processes(&r["params"]["pane_id"]));
                }
                let mut s = snapshot(2);
                if index > 0 {
                    match mode {
                        "order" => s["panes"].as_array_mut().unwrap().reverse(),
                        "selected" => s["panes"][0]["terminal_id"] = json!("changed"),
                        "unrelated" => s["panes"][1]["tab_id"] = json!("changed"),
                        "version" => s["version"] = json!("changed"),
                        "protocol" => s["protocol"] = json!(19),
                        _ => {}
                    }
                }
                reply(r, json!({"type":"session_snapshot","snapshot":s}))
            });
            let mut req = request(workspace.path(), &peer);
            if inspect {
                req.pane = Some("p0".into());
            }
            let report = inspection::inspect(&req).unwrap();
            match mode {
                "order" => assert_eq!(report["assessment"], "observed"),
                "unrelated" if inspect => assert_eq!(report["assessment"], "observed"),
                "version" => failure(&report, "server_context_changed"),
                "protocol" => {
                    failure(&report, "unsupported_protocol");
                    assert_eq!(inspection::exit_code(&report), 2);
                }
                "malformed" => failure(&report, "schema_error"),
                _ => failure(&report, "pane_mapping_changed"),
            }
        }
    }
}

#[test]
fn declaration_joins_revision_schema_counter_drift_and_huge_endpoint_identity() {
    let _serial = READERS.lock().unwrap();
    for mode in [
        "normal",
        "content",
        "schema",
        "counter",
        "huge",
        "terminal",
        "elsewhere",
        "outside",
        "process_changed",
        "legacy",
    ] {
        let workspace = workspace();
        let path = workspace.path().join("state/groups.json");
        let changed = path.clone();
        let peer = Peer::new(move |r, _| {
            if r["method"] == "pane.process_info" {
                if matches!(mode, "content" | "schema" | "counter") {
                    let mut state: Value =
                        serde_json::from_slice(&fs::read(&changed).unwrap()).unwrap();
                    match mode {
                        "content" => state["content_revision"] = json!(2),
                        "schema" => state["schema_version"] = json!(2),
                        "counter" => {
                            state["publication_counters"] = json!([{"endpoint":state["groups"][0]["members"][0]["binding"]["presentation"]["endpoint"],"source":"test","epoch":"epoch","watermarks":{"term0":1}}])
                        }
                        _ => {}
                    }
                    fs::write(&changed, state.to_string()).unwrap();
                }
                reply(r, processes(&r["params"]["pane_id"]))
            } else {
                reply(r, json!({"type":"session_snapshot","snapshot":snapshot(2)}))
            }
        });
        let mut state = authority(peer.endpoint());
        if mode == "process_changed" {
            state["groups"][0]["members"][0]["binding"]["evidence"]["process_evidence"]["matches"]
                [0]["pid"] = json!(4);
        }
        if matches!(mode, "legacy" | "schema") {
            let evidence = &mut state["groups"][0]["members"][0]["binding"]["evidence"];
            evidence.as_object_mut().unwrap().remove("process_evidence");
            evidence["provenance"] = json!("kitsune.session.snapshot");
            evidence["native_reference"] = json!({"source":"declared","agent":"djinn","kind":"id","value":"historical-native-reference"});
            evidence["native_binding"] = json!("observed_reference");
        }
        if mode == "schema" {
            state["schema_version"] = json!(1);
        }
        if mode == "terminal" {
            state["groups"][0]["members"][0]["binding"]["presentation"]["terminal_id"] =
                json!("lost");
        }
        if mode == "elsewhere" {
            state["groups"][0]["members"][0]["binding"]["presentation"]["endpoint"]["path"] =
                json!("/elsewhere/s");
        }
        let mut encoded = state.to_string();
        if mode == "huge" {
            encoded = encoded.replace(
                &format!("\"inode\":{}", peer.endpoint()["inode"]),
                &format!("\"inode\":{}", "9".repeat(100)),
            );
        }
        fs::write(&path, encoded).unwrap();
        let before = fs::read(&path).unwrap();
        let mut req = request(workspace.path(), &peer);
        if mode == "outside" {
            req.pane = Some("p1".into());
        }
        let report = inspection::inspect(&req).unwrap();
        match mode {
            "content" | "schema" => failure(&report, "groups_changed"),
            "process_changed" => assert_eq!(
                report["targets"][0]["associations"][0]["binding_check"],
                "process_evidence_changed"
            ),
            "legacy" => {
                let association = &report["targets"][0]["associations"][0];
                assert_eq!(
                    association["binding_check"],
                    "legacy_evidence_not_revalidated"
                );
                assert_eq!(
                    association["declared_native_reference"]["value"],
                    "historical-native-reference"
                );
                assert_eq!(association["native_identity"], "unestablished");
            }
            "huge" => assert_eq!(
                report["declarations"]["unmatched"][0]["binding_check"],
                "endpoint_identity_changed"
            ),
            "terminal" => assert_eq!(
                report["declarations"]["unmatched"][0]["binding_check"],
                "terminal_not_observed"
            ),
            "elsewhere" => assert_eq!(report["declarations"]["other_endpoint_count"], 1),
            "outside" => assert_eq!(report["declarations"]["outside_inspection_count"], 1),
            _ => {
                assert_eq!(report["assessment"], "observed", "{mode}: {report}");
                assert_eq!(
                    report["targets"][0]["associations"][0]["binding_check"],
                    "matching_process_evidence"
                );
            }
        }
        if !matches!(mode, "content" | "schema" | "counter") {
            assert_eq!(fs::read(&path).unwrap(), before);
        }
    }
}

#[test]
fn authority_and_endpoint_errors_are_private_and_validation_precedes_io() {
    let _serial = READERS.lock().unwrap();
    let workspace = workspace();
    let peer = Peer::normal(1);
    let mut req = request(workspace.path(), &peer);
    fs::write(
        workspace.path().join("state/groups.json"),
        b"PRIVATE corrupt authority",
    )
    .unwrap();
    failure(&inspection::inspect(&req).unwrap(), "groups_unavailable");
    assert!(peer.calls().is_empty());
    fs::remove_file(workspace.path().join("state/groups.json")).unwrap();
    for mode in 0..5 {
        let mut invalid = req.clone();
        match mode {
            0 => invalid.timeout = f64::NAN,
            1 => invalid.max_response_bytes = 1,
            2 => invalid.runtime = "other".into(),
            3 => invalid.member = Some("member".into()),
            _ => invalid.expected_terminal = Some("term0".into()),
        }
        assert!(inspection::inspect(&invalid).is_err());
    }
    assert!(peer.calls().is_empty());
    let alias = workspace.path().join("alias");
    symlink(std::path::Path::new(&peer.path).parent().unwrap(), &alias).unwrap();
    req.socket = alias.join("s").to_str().unwrap().into();
    let report = inspection::inspect(&req).unwrap();
    assert_eq!(report["endpoint"]["selected"], req.socket);
    assert_eq!(report["endpoint"]["resolved"], peer.path);
    fs::set_permissions(&peer.path, fs::Permissions::from_mode(0o644)).unwrap();
    failure(&inspection::inspect(&req).unwrap(), "unsafe_endpoint");
}

#[test]
fn bounded_watch_streams_summary_and_does_not_retry_broken_output() {
    let _serial = READERS.lock().unwrap();
    let workspace = workspace();
    let peer = Peer::normal(1);
    let req = request(workspace.path(), &peer);
    let mut bytes = Vec::new();
    let out = session_watch::arguments(&req, 0.1, 0.1, true, &mut bytes).unwrap();
    assert_eq!(out.exit_code, 0);
    assert!(out.stdout.is_empty());
    let lines = String::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["contract"], "coven.session-watch.v1");
    assert_eq!(lines[1]["outcome"], "completed");
    struct Broken(usize);
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            self.0 += 1;
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            panic!("flush after broken write")
        }
    }
    let mut broken = Broken(0);
    let out = session_watch::arguments(&req, 1., 0.1, true, &mut broken).unwrap();
    assert_eq!(out.exit_code, 1);
    assert_eq!(broken.0, 1);
}

#[test]
fn wire_schema_integer_limits_correlation_and_private_peer_errors() {
    let _serial = READERS.lock().unwrap();
    for mode in [
        "huge_ignored",
        "integer_limit",
        "duplicate",
        "wrong_id",
        "destination",
        "peer_error",
    ] {
        let workspace = workspace();
        let peer = Peer::new(move |r, _| {
            if mode == "peer_error" {
                return format!(
                    "{}\n",
                    json!({"id":r["id"],"error":{"code":"PRIVATE","message":"PRIVATE diagnostic argv"}})
                );
            }
            let result = if r["method"] == "session.snapshot" {
                json!({"type":"session_snapshot","snapshot":snapshot(1)})
            } else if mode == "destination" {
                processes(&json!("another-pane"))
            } else {
                processes(&r["params"]["pane_id"])
            };
            let mut encoded = reply(r, result);
            if r["method"] == "session.snapshot" {
                match mode {
                    "huge_ignored" | "integer_limit" => {
                        let length = if mode == "huge_ignored" { 4300 } else { 4301 };
                        encoded = encoded.replacen(
                            "\"version\":",
                            &format!("\"ignored\":{},\"version\":", "9".repeat(length)),
                            1,
                        );
                    }
                    "duplicate" => {
                        encoded = encoded.replacen(
                            "\"protocol\":18",
                            "\"protocol\":18,\"protocol\":18",
                            1,
                        )
                    }
                    "wrong_id" => {
                        encoded = encoded.replace(r["id"].as_str().unwrap(), "not-correlated")
                    }
                    _ => {}
                }
            }
            encoded
        });
        let report = inspection::inspect(&request(workspace.path(), &peer)).unwrap();
        if mode == "huge_ignored" {
            assert_eq!(report["assessment"], "observed");
            assert!(!report.to_string().contains(&"9".repeat(100)));
        } else {
            assert_ne!(report["assessment"], "observed", "{mode}");
            assert_eq!(report["targets"], json!([]));
            assert!(report["target_count"].is_null());
            assert!(!report.to_string().contains("PRIVATE"));
        }
    }
}

#[test]
fn endpoint_replacement_and_second_authority_failure_discard_positive_rows() {
    let _serial = READERS.lock().unwrap();
    for mode in ["endpoint", "authority"] {
        let workspace = workspace();
        let authority = workspace.path().join("state/groups.json");
        let endpoint_path = std::sync::Arc::new(Mutex::new(String::new()));
        let changed_path = endpoint_path.clone();
        let mut replacements = Vec::new();
        let peer = Peer::new(move |r, index| {
            if r["method"] == "pane.process_info" {
                if mode == "endpoint" {
                    let path = changed_path.lock().unwrap().clone();
                    fs::remove_file(&path).unwrap();
                    replacements.push(std::os::unix::net::UnixListener::bind(&path).unwrap());
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
                }
                return reply(r, processes(&r["params"]["pane_id"]));
            }
            if mode == "authority" && index > 0 {
                fs::write(&authority, b"PRIVATE corrupt second authority").unwrap();
            }
            reply(r, json!({"type":"session_snapshot","snapshot":snapshot(1)}))
        });
        *endpoint_path.lock().unwrap() = peer.path.clone();
        let report = inspection::inspect(&request(workspace.path(), &peer)).unwrap();
        failure(
            &report,
            if mode == "endpoint" {
                "endpoint_replaced"
            } else {
                "groups_unavailable"
            },
        );
        assert!(!report.to_string().contains("PRIVATE"));
    }
}

#[test]
fn declared_live_attachment_is_boolean_only_and_never_opened() {
    let _serial = READERS.lock().unwrap();
    let workspace = workspace();
    let peer = Peer::normal(1);
    let mut state = authority(peer.endpoint());
    state["schema_version"] = json!(3);
    state["groups"][0]["members"][0]["live_attachment"] = json!({"adapter":"djinn_live","source_contract":"djinn.live-observation.v1",
        "artifact_path":"/never/open/00000000-0000-4000-8000-000000000004.json","runtime_id":"00000000-0000-4000-8000-000000000004",
        "instance_id":"00000000-0000-4000-8000-000000000005","session_id":"PRIVATE-native-selector","attached_at":"2026-10-26T00:00:00+00:00"});
    let path = workspace.path().join("state/groups.json");
    fs::write(&path, state.to_string()).unwrap();
    let before = fs::read(&path).unwrap();
    let report = inspection::inspect(&request(workspace.path(), &peer)).unwrap();
    assert_eq!(report["assessment"], "observed", "{report}");
    assert_eq!(
        report["targets"][0]["associations"][0]["live_attachment_declared"],
        true
    );
    assert_eq!(report["targets"][0]["activity"], "unknown");
    assert!(!report.to_string().contains("/never/open"));
    assert!(!report.to_string().contains("PRIVATE"));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn cli_empty_workspace_operand_uses_child_current_directory() {
    let workspace = workspace();
    let peer = Peer::normal(1);
    fs::write(
        workspace.path().join("state/groups.json"),
        authority(peer.endpoint()).to_string(),
    )
    .unwrap();
    for operation in ["list", "inspect", "watch"] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_coven"));
        command
            .current_dir(workspace.path())
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .args(["session", operation, "", "--socket", &peer.path, "--json"]);
        if operation == "inspect" {
            command.args(["--pane", "p0", "--expected-terminal", "term0"]);
        }
        if operation == "watch" {
            command.args(["--duration", "0.1", "--interval", "0.1"]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{operation}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let records: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let report = if operation == "watch" {
            assert_eq!(records.len(), 2);
            assert_eq!(records[1]["outcome"], "completed");
            &records[0]["observation"]
        } else {
            assert_eq!(records.len(), 1);
            &records[0]
        };
        assert_eq!(report["workspace"], workspace.path().to_str().unwrap());
        assert_eq!(report["assessment"], "observed");
        assert_eq!(
            report["targets"][0]["associations"][0]["binding_check"],
            "matching_process_evidence"
        );
    }
    assert_eq!(peer.calls().len(), 9);
}

#[test]
fn watch_caps_slow_collection_to_remaining_watch_budget() {
    let _serial = READERS.lock().unwrap();
    let workspace = workspace();
    let peer = Peer::new(|r, _| {
        std::thread::sleep(std::time::Duration::from_millis(180));
        reply(r, json!({"type":"session_snapshot","snapshot":snapshot(1)}))
    });
    let mut bytes = Vec::new();
    let output = session_watch::arguments(
        &request(workspace.path(), &peer),
        0.1,
        0.1,
        true,
        &mut bytes,
    )
    .unwrap();
    assert_eq!(output.exit_code, 0);
    let records: Vec<Value> = String::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["observation"]["diagnostics"], json!(["timeout"]));
    assert_eq!(records[0]["observation"]["targets"], json!([]));
    assert_eq!(records[1]["poll_attempts"], 1);
    assert_eq!(records[1]["outcome"], "completed");
}
