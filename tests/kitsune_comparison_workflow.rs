//! Independent paired-producer tests. All endpoints and source files are disposable.
#![cfg(unix)]

#[path = "support/kitsune_comparison_fixture.rs"]
mod fixture;

use coven::kitsune_comparison::{Request, inspect};
use fixture::{Peer, Plan, SECRET, process};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{FileTypeExt, PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn request(peer: &Peer) -> Request {
    Request {
        socket: peer.socket.to_str().unwrap().into(),
        timeout: 3.0,
        max_response_bytes: 8_388_608,
    }
}

fn trust(report: &Value) {
    assert_eq!(report["authorization"], "none");
    assert_eq!(report["dispatchable"], false);
    for key in [
        "native_identity",
        "native_lifecycle",
        "readiness",
        "worker_acceptance",
        "job_completion",
    ] {
        assert_eq!(report[key], "unestablished", "{key}: {report}");
    }
    assert_eq!(report["replacement_equivalence"], "not_established");
    assert_eq!(report["runtime_fact_freshness"], "unknown");
    assert_eq!(report["timestamp_basis"], "local_receipt");
    assert!(
        !report.to_string().contains(SECRET),
        "private wire fields escaped: {report}"
    );
    let limitations = report["limitations"].to_string();
    assert!(limitations.contains("not atomic"));
    assert!(limitations.contains("not peer authentication"));
    assert!(limitations.contains("Zero server-side effects are not promised"));
}

fn observed(report: &Value, count: usize) {
    trust(report);
    assert_eq!(report["assessment"], "observed", "{report}");
    assert_eq!(report["diagnostics"], json!([]));
    assert_eq!(report["target_count"], count);
    assert_eq!(report["rows"].as_array().unwrap().len(), count);
    let total: u64 = report["counts"]
        .as_object()
        .unwrap()
        .values()
        .map(|v| v.as_u64().unwrap())
        .sum();
    assert_eq!(total, count as u64);
}

fn failed(report: &Value, diagnostic: &str) {
    trust(report);
    assert_ne!(report["assessment"], "observed", "{report}");
    assert_eq!(report["diagnostics"], json!([diagnostic]), "{report}");
    assert_eq!(
        report["rows"],
        json!([]),
        "partial positives survived: {report}"
    );
    assert!(report["counts"].is_null());
    assert!(report["target_count"].is_null());
}

fn run(plan: Plan) -> (Value, Vec<Value>) {
    let mut peer = Peer::start(plan);
    let report = inspect(&request(&peer)).unwrap();
    peer.finish();
    (report, peer.calls())
}

fn paired_calls(calls: &[Value], panes: usize) {
    assert_eq!(calls.len(), panes + 2);
    assert_eq!(calls[0]["method"], "session.snapshot");
    assert_eq!(calls.last().unwrap()["method"], "session.snapshot");
    let mut queried: Vec<_> = calls[1..=panes]
        .iter()
        .map(|c| {
            assert_eq!(c["method"], "pane.process_info");
            c["params"]["pane_id"].as_str().unwrap().to_owned()
        })
        .collect();
    queried.sort();
    let mut expected: Vec<_> = (0..panes).map(|i| format!("pane-{i}")).collect();
    expected.sort();
    assert_eq!(queried, expected);
}

#[test]
fn paired_producer_projects_only_safe_facts_and_never_promotes_done() {
    let (report, calls) = run(Plan::new(2));
    observed(&report, 2);
    paired_calls(&calls, 2);
    assert_eq!(report["counts"]["both_djinn"], 2);
    for (i, row) in report["rows"].as_array().unwrap().iter().enumerate() {
        assert_eq!(
            row["locators"],
            json!({"pane_id":format!("pane-{i}"),
            "terminal_id":format!("term-{i}"),"workspace_id":"ws-0","tab_id":"tab-0"})
        );
        assert_eq!(
            row["kitsune"]["before"],
            json!({"listed":true,"reported_family":"djinn",
            "status":"done","native_reference_present":true})
        );
        assert_eq!(row["kitsune"]["after"], row["kitsune"]["before"]);
        assert_eq!(
            row["coven"],
            json!({"recognition":"djinn_executable_family",
            "matches":[{"pid":101,"executable_family":"djinn","rule":"exact_posix_argv0_basename"}]})
        );
        assert_eq!(row["family_comparison"], "both_djinn");
        for forbidden in [
            "activity",
            "attention",
            "interactive_ready",
            "job_completion",
            "native_session",
        ] {
            assert!(row.get(forbidden).is_none());
            assert!(row["coven"].get(forbidden).is_none());
        }
    }
}

#[test]
fn serializer_omitted_optionals_and_empty_process_inventory_compare_with_stable_defaults() {
    // Kitsune's collector lists a terminal only when it has a name or effective family.
    for (name, family, category) in [
        (Some(SECRET), None, "neither"),
        (None, Some("djinn"), "builtin_only"),
        (None, None, "neither"),
    ] {
        let listed = name.is_some() || family.is_some();
        for omitted_processes in [true, false] {
            let mut plan = Plan::new(1);
            for sample in [&mut plan.before, &mut plan.after] {
                for inventory in ["panes", "agents"] {
                    let row = sample[inventory][0].as_object_mut().unwrap();
                    // Option::None, empty metadata, and false skip_serializing_if fields.
                    for key in [
                        "agent",
                        "agent_session",
                        "name",
                        "launch_pending",
                        "interactive_ready",
                        "screen_detection_skipped",
                        "cwd",
                        "foreground_cwd",
                        "label",
                        "title",
                        "terminal_title",
                        "terminal_title_stripped",
                        "display_agent",
                        "state_labels",
                        "tokens",
                        "scroll",
                    ] {
                        row.remove(key);
                    }
                    row.insert("agent_status".into(), json!("unknown"));
                    if let Some(family) = family {
                        row.insert("agent".into(), json!(family));
                    }
                    if inventory == "agents" {
                        if let Some(name) = name {
                            row.insert("name".into(), json!(name));
                        }
                        // Unlike the false flags, AgentInfo serializes its default sequence.
                        row.insert("state_change_seq".into(), json!(0));
                    }
                }
                if !listed {
                    sample["agents"] = json!([]);
                }
            }
            let mut info = json!({"type":"pane_process_info","process_info":{"pane_id":"pane-0"}});
            if !omitted_processes {
                info["process_info"]["foreground_processes"] = json!([]);
            }
            plan.processes.insert("pane-0".into(), info);
            let (report, calls) = run(plan);
            observed(&report, 1);
            paired_calls(&calls, 1);
            assert_eq!(
                report["rows"][0]["kitsune"],
                json!({
                    "before":{"listed":listed,"reported_family":family,"status":"unknown","native_reference_present":false},
                    "after":{"listed":listed,"reported_family":family,"status":"unknown","native_reference_present":false}
                })
            );
            assert_eq!(
                report["rows"][0]["coven"],
                json!({"recognition":"unknown","matches":[]})
            );
            assert_eq!(report["rows"][0]["family_comparison"], category);
            assert_eq!(report["counts"][category], 1);
            assert_eq!(report["counts"]["legacy_changed"], 0);
        }
    }
}

#[test]
fn all_seven_family_categories_are_distinct_and_other_families_are_not_regressions() {
    for (family, listed, argv, category) in [
        (Some("djinn"), true, json!(["djinn"]), "both_djinn"),
        (None, false, json!(["djinn-ui", "--help"]), "generic_only"),
        (Some("djinn"), true, json!(["bash"]), "builtin_only"),
        (None, false, json!(["bash"]), "neither"),
        (
            Some("claude"),
            true,
            json!(["claude"]),
            "other_builtin_family",
        ),
        (
            Some("unrecognized-provider"),
            true,
            json!(["djinn"]),
            "conflicting_family_hints",
        ),
    ] {
        let mut plan = Plan::new(1);
        for snapshot in [&mut plan.before, &mut plan.after] {
            snapshot["panes"][0]["agent"] = json!(family);
            snapshot["panes"][0]
                .as_object_mut()
                .unwrap()
                .remove("agent_session");
            if listed {
                snapshot["agents"][0]["agent"] = json!(family);
                snapshot["agents"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("agent_session");
            } else {
                snapshot["agents"] = json!([]);
            }
        }
        plan.processes
            .insert("pane-0".into(), process("pane-0", argv));
        let (report, calls) = run(plan);
        observed(&report, 1);
        paired_calls(&calls, 1);
        assert_eq!(report["rows"][0]["family_comparison"], category);
        assert_eq!(report["counts"][category], 1);
        assert_eq!(report["counts"].as_object().unwrap().len(), 7);
        assert!(report["rows"][0]["coven"].get("status").is_none());
    }
    let mut plan = Plan::new(1);
    plan.after["agents"][0]["state_change_seq"] = json!(4);
    let (report, _) = run(plan);
    observed(&report, 1);
    assert_eq!(report["rows"][0]["family_comparison"], "legacy_changed");
}

#[test]
fn argv_zero_only_exact_posix_basename_recognition_includes_help_not_activity() {
    for (argv, recognized) in [
        (json!(["/opt/djinn", "--help", SECRET]), true),
        (json!(["/opt/djinn-ui", "maintenance"]), true),
        (json!(["djinn"]), true),
        (json!(["/opt/not-djinn", "djinn"]), false),
        (json!(["/opt/djinn/", "djinn"]), false),
        (json!(["DJINN"]), false),
        (json!(["node", "/opt/djinn"]), false),
        (json!(["bash", "-c", "djinn"]), false),
        (json!(["djinn-extra"]), false),
        (json!(["C:\\bin\\djinn"]), false),
        (json!([]), false),
        (Value::Null, false),
    ] {
        let mut plan = Plan::new(1);
        let mut info = process("pane-0", argv.clone());
        info["process_info"]["foreground_processes"][0]["name"] = json!("djinn");
        info["process_info"]["foreground_processes"][0]["argv0"] = json!("/opt/djinn");
        info["process_info"]["foreground_processes"][0]["cmdline"] = json!("djinn --help");
        plan.processes.insert("pane-0".into(), info);
        let (report, _) = run(plan);
        observed(&report, 1);
        let evidence = &report["rows"][0]["coven"];
        assert_eq!(
            evidence["recognition"],
            if recognized {
                "djinn_executable_family"
            } else {
                "unknown"
            },
            "argv={argv}"
        );
        assert_eq!(
            evidence["matches"].as_array().unwrap().len(),
            usize::from(recognized)
        );
        assert!(evidence.get("activity").is_none());
    }
}

#[test]
fn sixty_four_panes_are_sampled_once_and_sixty_five_fail_before_process_queries() {
    let (report, calls) = run(Plan::new(64));
    observed(&report, 64);
    paired_calls(&calls, 64);
    let (report, calls) = run(Plan::new(65));
    failed(&report, "pane_limit");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["method"], "session.snapshot");
}

#[test]
fn empty_inventory_still_requires_two_snapshots_and_is_not_replacement_proof() {
    let (report, calls) = run(Plan::new(0));
    observed(&report, 0);
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|c| c["method"] == "session.snapshot"));
}

#[test]
fn inventory_order_alone_does_not_change_mapping_or_legacy_evidence() {
    let mut plan = Plan::new(3);
    plan.after["panes"].as_array_mut().unwrap().reverse();
    plan.after["agents"].as_array_mut().unwrap().reverse();
    let (report, calls) = run(plan);
    observed(&report, 3);
    paired_calls(&calls, 3);
    assert_eq!(report["counts"]["both_djinn"], 3);
}

#[test]
fn whole_cohort_mapping_drift_clears_all_positives() {
    for (key, value) in [
        ("terminal_id", "replaced-terminal"),
        ("workspace_id", "ws-moved"),
        ("tab_id", "tab-moved"),
        ("pane_id", "pane-replaced"),
    ] {
        let mut plan = Plan::new(2);
        for inventory in ["panes", "agents"] {
            plan.after[inventory][1][key] = json!(value);
        }
        let (report, calls) = run(plan);
        failed(&report, "pane_mapping_changed");
        paired_calls(&calls, 2);
    }
    for added in [true, false] {
        let mut plan = Plan::new(2);
        if added {
            let extra = fixture::snapshot(3);
            plan.after = extra;
        } else {
            plan.after["panes"].as_array_mut().unwrap().pop();
            plan.after["agents"].as_array_mut().unwrap().pop();
        }
        let (report, _) = run(plan);
        failed(&report, "pane_mapping_changed");
    }
}

#[test]
fn all_tracked_legacy_changes_are_detected_including_hidden_native_reference_value() {
    for (key, value) in [
        ("agent", json!("claude")),
        ("agent_status", json!("working")),
        ("revision", json!(2)),
        ("focused", json!(false)),
        (
            "agent_session",
            json!({"source":"hook","agent":"djinn","kind":"path",
            "value":format!("/{SECRET}/changed-only-value")}),
        ),
    ] {
        let mut plan = Plan::new(2);
        for inventory in ["panes", "agents"] {
            plan.after[inventory][0][key] = value.clone();
        }
        let (report, _) = run(plan);
        observed(&report, 2);
        assert_eq!(
            report["rows"][0]["family_comparison"], "legacy_changed",
            "{key}: {report}"
        );
        assert_eq!(report["rows"][1]["family_comparison"], "both_djinn");
        assert_eq!(report["counts"]["legacy_changed"], 1);
    }
    for (key, value) in [
        ("name", json!("changed-private-name")),
        ("launch_pending", json!(true)),
        ("interactive_ready", json!(true)),
        ("screen_detection_skipped", json!(true)),
        ("state_change_seq", json!(4)),
    ] {
        let mut plan = Plan::new(1);
        plan.after["agents"][0][key] = value;
        let (report, _) = run(plan);
        observed(&report, 1);
        assert_eq!(
            report["rows"][0]["family_comparison"], "legacy_changed",
            "{key}: {report}"
        );
    }
    let mut plan = Plan::new(1);
    plan.after["agents"] = json!([]);
    let (report, _) = run(plan);
    observed(&report, 1);
    assert_eq!(report["rows"][0]["family_comparison"], "legacy_changed");
}

#[test]
fn native_reference_presence_and_each_hidden_component_are_tracked_without_disclosure() {
    for (key, value) in [
        ("source", "changed-source"),
        ("agent", "changed-native-family"),
        ("kind", "id"),
        ("value", "changed-private-native-value"),
    ] {
        let mut plan = Plan::new(1);
        for inventory in ["panes", "agents"] {
            plan.after[inventory][0]["agent_session"][key] = json!(value);
        }
        let (report, _) = run(plan);
        observed(&report, 1);
        assert_eq!(report["rows"][0]["family_comparison"], "legacy_changed");
        if key != "kind" {
            assert!(!report.to_string().contains(value));
        }
        assert_eq!(
            report["rows"][0]["kitsune"]["before"],
            report["rows"][0]["kitsune"]["after"]
        );
    }
    for added in [true, false] {
        let mut plan = Plan::new(1);
        let sample = if added {
            &mut plan.before
        } else {
            &mut plan.after
        };
        for inventory in ["panes", "agents"] {
            sample[inventory][0]
                .as_object_mut()
                .unwrap()
                .remove("agent_session");
        }
        let (report, _) = run(plan);
        observed(&report, 1);
        assert_eq!(report["rows"][0]["family_comparison"], "legacy_changed");
        assert_ne!(
            report["rows"][0]["kitsune"]["before"]["native_reference_present"],
            report["rows"][0]["kitsune"]["after"]["native_reference_present"]
        );
    }
}

#[test]
fn second_process_failure_never_publishes_first_positive_row() {
    for error in [true, false] {
        let mut plan = Plan::new(3);
        if error {
            plan.error_at = Some(3);
        } else {
            plan.wrong_id_at = Some(3);
        }
        let (report, calls) = run(plan);
        failed(
            &report,
            if error {
                "server_error"
            } else {
                "schema_error"
            },
        );
        assert_eq!(calls.len(), 3);
    }
}

#[test]
fn every_process_payload_is_validated_not_only_matching_entries() {
    for (key, value) in [
        ("pid", json!(0)),
        ("pid", json!(4_294_967_296_u64)),
        ("pid", json!(true)),
        ("name", Value::Null),
        ("argv", json!(["djinn", 3])),
        ("argv", json!("djinn")),
        ("argv", json!(["bash", "bad\u{0}argument"])),
    ] {
        let mut plan = Plan::new(2);
        plan.processes.get_mut("pane-1").unwrap()["process_info"]["foreground_processes"][0][key] =
            value;
        let (report, calls) = run(plan);
        failed(&report, "schema_error");
        assert_eq!(calls.len(), 3, "{key}");
    }
    for key in [
        "pane_id",
        "shell_pid",
        "foreground_process_group_id",
        "foreground_processes",
    ] {
        let mut plan = Plan::new(2);
        plan.processes.get_mut("pane-1").unwrap()["process_info"][key] = json!("invalid");
        let (report, _) = run(plan);
        failed(&report, "schema_error");
    }
    let mut plan = Plan::new(1);
    let rows = plan.processes.get_mut("pane-0").unwrap()["process_info"]["foreground_processes"]
        .as_array_mut()
        .unwrap();
    rows.push(rows[0].clone());
    let (report, _) = run(plan);
    failed(&report, "schema_error");
}

#[test]
fn ignored_raw_hundred_digit_integer_is_accepted_without_lossy_projection() {
    let mut plan = Plan::new(1);
    let mut info = process("pane-0", json!(["djinn"]));
    info["process_info"]["ignored"] = json!("RAW_INTEGER");
    info["process_info"]["foreground_processes"][0]["ignored"] = json!("RAW_INTEGER");
    let raw = format!("{{\"id\":REQUEST_ID,\"result\":{info}}}")
        .replace("\"RAW_INTEGER\"", &"9".repeat(100));
    plan.raw_at = Some((2, raw));
    let (report, calls) = run(plan);
    observed(&report, 1);
    paired_calls(&calls, 1);
    assert_eq!(report["counts"]["both_djinn"], 1);
}

#[test]
fn ignored_integer_wire_profile_accepts_400_and_4300_digits_but_refuses_4301_in_each_reply() {
    let (baseline, _) = run(Plan::new(1));
    observed(&baseline, 1);
    for digits in [400, 4300, 4301] {
        let integer = "9".repeat(digits);
        for at in 1..=3 {
            let mut plan = Plan::new(1);
            let mut result = match at {
                1 => json!({"type":"session_snapshot","snapshot":plan.before}),
                2 => plan.processes["pane-0"].clone(),
                _ => json!({"type":"session_snapshot","snapshot":plan.after}),
            };
            if at == 2 {
                result["process_info"]["ignored"] = json!("RAW_INTEGER");
                result["process_info"]["foreground_processes"][0]["ignored"] = json!("RAW_INTEGER");
            } else {
                result["snapshot"]["ignored"] = json!("RAW_INTEGER");
                for inventory in ["panes", "agents"] {
                    result["snapshot"][inventory][0]["ignored"] = json!("RAW_INTEGER");
                }
            }
            let raw = format!("{{\"id\":REQUEST_ID,\"result\":{result}}}")
                .replace("\"RAW_INTEGER\"", &integer);
            plan.raw_at = Some((at, raw));
            let (report, calls) = run(plan);
            if digits <= 4300 {
                observed(&report, 1);
                paired_calls(&calls, 1);
                for key in ["rows", "counts", "target_count"] {
                    assert_eq!(
                        report[key], baseline[key],
                        "digits={digits}, reply={at}, key={key}"
                    );
                }
            } else {
                failed(&report, "schema_error");
                assert_eq!(calls.len(), at);
            }
            assert!(
                !report.to_string().contains(&integer),
                "digits={digits}, reply={at}"
            );
        }
    }
}

#[test]
fn malformed_wire_extensions_are_not_silently_ignored_or_echoed() {
    for (raw, diagnostic) in [
        (
            format!("{{\"id\":REQUEST_ID,\"id\":REQUEST_ID,\"private\":\"{SECRET}\"}}"),
            "duplicate_key",
        ),
        (
            format!(
                "{{\"id\":REQUEST_ID,\"result\":null,\"ignored\":NaN,\"private\":\"{SECRET}\"}}"
            ),
            "nonfinite_json",
        ),
        (
            format!("{{\"id\":REQUEST_ID,\"result\":null,\"private\":\"{SECRET}\""),
            "invalid_json",
        ),
        ("{\"id\":REQUEST_ID,\"result\":null}".into(), "schema_error"),
        (
            format!(
                "{{\"id\":REQUEST_ID,\"result\":{{\"type\":\"pane_process_info\",\"process_info\":{{\"pane_id\":\"pane-0\"}}}},\"error\":{{\"code\":\"bad\",\"message\":\"{SECRET}\"}}}}"
            ),
            "schema_error",
        ),
    ] {
        let mut plan = Plan::new(2);
        plan.raw_at = Some((3, raw));
        let (report, calls) = run(plan);
        failed(&report, diagnostic);
        assert_eq!(calls.len(), 3);
    }
}

#[test]
fn wrong_correlation_protocol_and_server_error_are_private_at_both_brackets() {
    for at in [1, 3] {
        for server_error in [true, false] {
            let mut plan = Plan::new(1);
            if server_error {
                plan.error_at = Some(at);
            } else {
                plan.wrong_id_at = Some(at);
            }
            let (report, calls) = run(plan);
            failed(
                &report,
                if server_error {
                    "server_error"
                } else {
                    "schema_error"
                },
            );
            assert_eq!(calls.len(), at);
        }
        let mut plan = Plan::new(1);
        if at == 1 {
            plan.before["protocol"] = json!(17);
        } else {
            plan.after["protocol"] = json!(19);
        }
        let (report, calls) = run(plan);
        failed(&report, "unsupported_protocol");
        assert_eq!(report["assessment"], "incompatible");
        assert_eq!(calls.len(), at);
    }
    let mut plan = Plan::new(1);
    plan.after["version"] = json!("different-build");
    let (report, _) = run(plan);
    failed(&report, "version_changed");
}

#[test]
fn strict_snapshots_reject_missing_invalid_duplicate_and_inconsistent_fields() {
    for inventory in ["panes", "agents", "workspaces", "tabs", "layouts"] {
        let mut plan = Plan::new(1);
        plan.before.as_object_mut().unwrap().remove(inventory);
        let (report, calls) = run(plan);
        failed(&report, "schema_error");
        assert_eq!(calls.len(), 1);
    }
    for key in ["pane_id", "terminal_id", "workspace_id", "tab_id"] {
        for bad in [
            json!(""),
            json!("has whitespace"),
            json!("bad\ncontrol"),
            json!(false),
        ] {
            let mut plan = Plan::new(1);
            for inventory in ["panes", "agents"] {
                plan.before[inventory][0][key] = bad.clone();
            }
            let (report, _) = run(plan);
            failed(&report, "schema_error");
        }
    }
    for inventory in ["panes", "agents"] {
        let mut plan = Plan::new(1);
        let row = plan.before[inventory][0].clone();
        plan.before[inventory].as_array_mut().unwrap().push(row);
        let (report, _) = run(plan);
        failed(&report, "schema_error");
    }
    for (key, bad) in [
        ("revision", json!(true)),
        ("focused", json!(1)),
        ("agent_status", json!("completed")),
        ("agent_session", json!({"kind":"path"})),
    ] {
        let mut plan = Plan::new(1);
        plan.before["panes"][0][key] = bad;
        let (report, _) = run(plan);
        failed(&report, "schema_error");
    }
    let mut plan = Plan::new(1);
    plan.before["agents"][0]["terminal_id"] = json!("different");
    let (report, _) = run(plan);
    failed(&report, "schema_error");
}

#[test]
fn endpoint_replacement_aborts_before_any_new_endpoint_query() {
    let mut plan = Plan::new(2);
    plan.swap_at = Some(2);
    let (report, calls) = run(plan);
    failed(&report, "endpoint_replaced");
    assert_eq!(calls.len(), 2);
}

#[test]
fn parent_path_alias_resolves_to_same_pinned_endpoint() {
    let mut peer = Peer::start(Plan::new(1));
    let aliases = tempfile::tempdir().unwrap();
    symlink(peer.root.path(), aliases.path().join("alias")).unwrap();
    let mut req = request(&peer);
    req.socket = aliases.path().join("alias/s").to_str().unwrap().into();
    let report = inspect(&req).unwrap();
    observed(&report, 1);
    assert_eq!(report["endpoint"]["selected"], req.socket);
    assert_eq!(
        report["endpoint"]["resolved"],
        peer.socket.to_str().unwrap()
    );
    paired_calls(&peer.calls(), 1);
    peer.finish();
}

#[test]
fn leaf_alias_is_not_followed_as_an_authenticated_endpoint() {
    let mut peer = Peer::start(Plan::new(1));
    let alias = peer.root.path().join("leaf-alias");
    symlink(&peer.socket, &alias).unwrap();
    let mut req = request(&peer);
    req.socket = alias.to_str().unwrap().into();
    let report = inspect(&req).unwrap();
    failed(&report, "unsafe_endpoint");
    assert!(peer.calls().is_empty());
    peer.finish();
}

#[test]
fn process_matches_have_deterministic_order_and_do_not_expose_other_fields() {
    let mut plan = Plan::new(1);
    plan.processes.get_mut("pane-0").unwrap()["process_info"]["foreground_processes"] = json!([
        {"pid":300,"name":SECRET,"argv":["/private/djinn-ui","--help",SECRET]},
        {"pid":200,"name":SECRET,"argv":["bash","djinn",SECRET]},
        {"pid":100,"name":SECRET,"argv":["/private/djinn","--help",SECRET]}
    ]);
    let (report, calls) = run(plan);
    observed(&report, 1);
    paired_calls(&calls, 1);
    assert_eq!(
        report["rows"][0]["coven"]["matches"],
        json!([
            {"pid":100,"executable_family":"djinn","rule":"exact_posix_argv0_basename"},
            {"pid":300,"executable_family":"djinn-ui","rule":"exact_posix_argv0_basename"}
        ])
    );
}

#[test]
fn one_response_budget_is_shared_across_the_entire_collection() {
    let plan = Plan::new(2);
    // Each snapshot fits; two snapshots alone exceed this collection budget.
    let single =
        json!({"id":"x".repeat(256),"result":{"type":"session_snapshot","snapshot":plan.before}})
            .to_string()
            .len();
    let mut peer = Peer::start(plan);
    let mut req = request(&peer);
    req.max_response_bytes = single + 1024;
    let report = inspect(&req).unwrap();
    failed(&report, "response_limit");
    assert!(peer.calls().len() > 1);
    peer.finish();
}

#[test]
fn deadline_is_not_reset_for_each_process_request() {
    let mut plan = Plan::new(2);
    plan.delay = Duration::from_millis(120);
    let mut peer = Peer::start(plan);
    let mut req = request(&peer);
    req.timeout = 0.3;
    let start = Instant::now();
    let report = inspect(&req).unwrap();
    failed(&report, "timeout");
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(peer.calls().len() < 4);
    peer.finish();
}

#[test]
fn invalid_profiles_fail_before_connect_and_unsafe_socket_is_not_read() {
    let mut peer = Peer::start(Plan::new(1));
    for timeout in [f64::NAN, f64::INFINITY, -1.0, 0.0, 30.001] {
        let mut req = request(&peer);
        req.timeout = timeout;
        assert_eq!(inspect(&req).unwrap_err().code(), "invalid_timeout");
    }
    for bytes in [0, 1023, 8_388_609, usize::MAX] {
        let mut req = request(&peer);
        req.max_response_bytes = bytes;
        assert_eq!(inspect(&req).unwrap_err().code(), "invalid_response_budget");
    }
    for path in [
        "",
        "relative.sock",
        "bad\npath",
        "bad\0path",
        "/tmp/../s",
        "/tmp/s/",
    ] {
        let mut req = request(&peer);
        req.socket = path.into();
        assert!(inspect(&req).is_err());
    }
    assert!(peer.calls().is_empty());
    fs::set_permissions(&peer.socket, fs::Permissions::from_mode(0o666)).unwrap();
    let report = inspect(&request(&peer)).unwrap();
    failed(&report, "unsafe_endpoint");
    assert!(peer.calls().is_empty());
    peer.finish();
}

// Captures actual disposable source bytes and entry types, not a client-clock
// freshness or atomicity assertion. Socket I/O cannot change fixture file data.
fn tree(root: &Path) -> BTreeMap<String, (u32, Vec<u8>)> {
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<String, (u32, Vec<u8>)>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).unwrap();
            let kind = meta.file_type();
            let bytes = if kind.is_file() {
                fs::read(&path).unwrap()
            } else if kind.is_symlink() {
                fs::read_link(&path)
                    .unwrap()
                    .as_os_str()
                    .as_encoded_bytes()
                    .to_vec()
            } else if kind.is_socket() {
                b"socket".to_vec()
            } else {
                b"directory".to_vec()
            };
            out.insert(
                path.strip_prefix(root).unwrap().to_str().unwrap().into(),
                (meta.permissions().mode(), bytes),
            );
            if kind.is_dir() {
                visit(root, &path, out);
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

fn cli_command(peer: &Peer, extra: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_coven"));
    command.args([
        "observe",
        "kitsune",
        "--socket",
        peer.socket.to_str().unwrap(),
    ]);
    command
        .args(extra)
        .current_dir(peer.root.path())
        .env("HOME", peer.root.path())
        .env("PATH", "/nonexistent-fixture-bin")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn cli(peer: &Peer, extra: &[&str]) -> Output {
    let mut command = cli_command(peer, extra);
    let mut child = command.spawn().unwrap();
    let end = Instant::now() + Duration::from_secs(8);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() > end {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!(
                "CLI watchdog expired: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.wait_with_output().unwrap()
}

#[test]
#[cfg(target_os = "linux")]
fn actual_cli_sigint_during_later_process_query_discards_earlier_positive() {
    struct ChildGuard(Option<std::process::Child>);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if let Some(child) = &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    let mut plan = Plan::new(3);
    plan.hold_at = Some(3);
    let mut peer = Peer::start(plan);
    let before = tree(peer.root.path());
    let mut guard = ChildGuard(Some(
        cli_command(&peer, &["--compare-agents", "--json", "--timeout", "10"])
            .spawn()
            .unwrap(),
    ));
    let child = guard.0.as_mut().unwrap();
    let handshake_deadline = Instant::now() + Duration::from_secs(5);
    while peer.calls().len() < 3 {
        assert!(
            child.try_wait().unwrap().is_none(),
            "CLI exited before later-query handshake"
        );
        assert!(
            Instant::now() < handshake_deadline,
            "later-query handshake timed out"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let calls = peer.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0]["method"], "session.snapshot");
    assert_eq!(calls[1]["method"], "pane.process_info");
    assert_eq!(calls[1]["params"], json!({"pane_id":"pane-0"}));
    assert_eq!(calls[2]["method"], "pane.process_info");
    assert_eq!(calls[2]["params"], json!({"pane_id":"pane-1"}));
    // Reaching this request requires the CLI guard and the earlier positive read.
    // A strictly positive child PID cannot signal the test process or its group.
    let pid = i32::try_from(child.id()).unwrap();
    assert!(pid > 0);
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(unsafe { libc::kill(pid, libc::SIGINT) }, 0);
    let exit_deadline = Instant::now() + Duration::from_secs(3);
    while child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < exit_deadline,
            "interrupted CLI did not exit promptly"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let output = guard.0.take().unwrap().wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(130),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report = serde_json::from_slice(&output.stdout).unwrap();
    failed(&report, "interrupted");
    peer.finish();
    assert_eq!(
        peer.calls(),
        calls,
        "interruption issued further queries or fallback"
    );
    assert_eq!(tree(peer.root.path()), before);
}

#[test]
fn actual_cli_default_pretty_and_json_ignore_invalid_workspace_and_do_not_write_source() {
    for json_output in [false, true] {
        let mut peer = Peer::start(Plan::new(1));
        fs::write(
            peer.root.path().join("coven.json"),
            b"not valid JSON and not a workspace",
        )
        .unwrap();
        fs::create_dir(peer.root.path().join("source")).unwrap();
        fs::write(
            peer.root.path().join("source/session.json"),
            format!("{{\"secret\":\"{SECRET}\"}}"),
        )
        .unwrap();
        let before = tree(peer.root.path());
        let output = cli(
            &peer,
            if json_output {
                &["--compare-agents", "--json"]
            } else {
                &["--compare-agents"]
            },
        );
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        observed(&report, 1);
        assert!(output.stdout.ends_with(b"\n"));
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text.contains("\n  \""), !json_output, "{text}");
        paired_calls(&peer.calls(), 1);
        assert_eq!(
            tree(peer.root.path()),
            before,
            "CLI mutated source or created workspace/lock files"
        );
        peer.finish();
    }
}

#[test]
fn actual_cli_preserves_abbreviation_and_repeated_option_semantics() {
    for args in [
        vec!["--compare-agent", "--js"],
        vec!["--compare-agents", "--compare-agents", "--json", "--json"],
        vec![
            "--compare-agents",
            "--timeout",
            "0",
            "--timeout",
            "3",
            "--json",
        ],
    ] {
        let mut peer = Peer::start(Plan::new(1));
        let output = cli(&peer, &args);
        assert_eq!(
            output.status.code(),
            Some(0),
            "args={args:?}, stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = serde_json::from_slice(&output.stdout).unwrap();
        observed(&report, 1);
        paired_calls(&peer.calls(), 1);
        peer.finish();
    }
}

#[test]
fn actual_cli_rejects_conflicting_modes_in_both_orders_without_connect() {
    for args in [
        vec!["--compare-agents", "--process-pane", "pane-0"],
        vec!["--process-pane", "pane-0", "--compare-agents"],
        vec!["--compare-agent", "--process-p", "pane-0"],
        vec!["--process-p=pane-0", "--compare-agent"],
        vec!["--compare-agents=true"],
    ] {
        let mut peer = Peer::start(Plan::new(1));
        let before = tree(peer.root.path());
        let output = cli(&peer, &args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "args={args:?}, stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(peer.calls().is_empty(), "args={args:?}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains(SECRET));
        assert_eq!(tree(peer.root.path()), before);
        peer.finish();
    }
}

#[test]
fn actual_cli_operational_and_protocol_failures_have_distinct_exit_codes() {
    for incompatible in [false, true] {
        let mut plan = Plan::new(1);
        if incompatible {
            plan.before["protocol"] = json!(17);
        } else {
            plan.error_at = Some(2);
        }
        let mut peer = Peer::start(plan);
        let output = cli(&peer, &["--compare-agents", "--json"]);
        assert_eq!(output.status.code(), Some(if incompatible { 2 } else { 1 }));
        let report = serde_json::from_slice(&output.stdout).unwrap();
        failed(
            &report,
            if incompatible {
                "unsupported_protocol"
            } else {
                "server_error"
            },
        );
        assert!(!String::from_utf8_lossy(&output.stderr).contains(SECRET));
        peer.finish();
    }
}
