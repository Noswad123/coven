//! Independent Python-reference transcripts, never expectations produced by Rust.
//! See launch_provenance.json and the unaccepted launch review fragment for scope.
use super::tests::{evaluate, json, native};
use super::*;
use serde_json::{Value as Json, json};
use std::sync::OnceLock;

pub(super) fn corpus() -> &'static [Json] {
    static CORPUS: OnceLock<Vec<Json>> = OnceLock::new();
    CORPUS.get_or_init(|| {
        serde_json::from_str(include_str!("../../tests/retirement/fixtures/launch.json")).unwrap()
    })
}

fn check(record: &Json) {
    let mut actual = evaluate(&record["case"]);
    let mut expected = record["expected"].clone();
    if record["exact_diagnostics"] == false {
        for value in [&mut actual, &mut expected] {
            for diagnostic in value["report"]["diagnostics"].as_array_mut().unwrap() {
                diagnostic.as_object_mut().unwrap().remove("message");
            }
        }
    }
    assert_eq!(actual, expected, "{}", record["id"]);
}

fn method(name: &str, count: usize) {
    let records: Vec<_> = corpus().iter().filter(|r| r["method"] == name).collect();
    assert_eq!(
        records.len(),
        count,
        "original method corpus must be complete"
    );
    for record in records {
        check(record);
    }
}

#[test]
fn every_frozen_unit_input_has_reference_parity() {
    let records: Vec<_> = corpus()
        .iter()
        .filter(|r| r.get("family").is_some())
        .collect();
    assert_eq!(records.len(), 232);
    for record in records {
        check(record);
    }
}

#[test]
fn happy_transport_transcripts_without_python_kwargs_equivalence() {
    // This intentionally does NOT retire Python's kwargs/env-identity assertions.
    method("happy_transport_literal_payload_and_exact_call_contract", 2);
}

#[test]
fn collisions_and_query_errors_have_no_startup_effects() {
    method("collisions_and_query_errors_have_no_startup_effects", 4);
}

#[test]
fn every_required_command_failure_stops_without_cleanup() {
    method("every_required_command_failure_stops_without_cleanup", 10);
}

#[test]
fn untrusted_json_and_handles_fail_cleanly() {
    method("untrusted_json_and_handles_fail_cleanly", 25);
}

#[test]
fn sleep_failure_does_not_send_starter_or_clean_up() {
    method("sleep_failure_does_not_send_starter_or_clean_up", 2);
}

#[test]
fn ordered_roster_and_delay_for_each_agent() {
    method("ordered_roster_and_delay_for_each_agent", 2);
}

#[test]
fn stop_exact_target_absence_and_no_legacy_adoption() {
    method("stop_exact_target_absence_and_no_legacy_adoption", 4);
}

#[test]
fn stop_ambiguity_untrusted_identity_and_failure_never_retry() {
    method(
        "stop_ambiguity_untrusted_identity_and_failure_never_retry",
        5,
    );
}

#[test]
fn tmux_query_failure_is_not_evidence_of_absence() {
    method("tmux_query_failure_is_not_evidence_of_absence", 8);
    for record in corpus()
        .iter()
        .filter(|r| r["method"] == "tmux_query_failure_is_not_evidence_of_absence")
    {
        let report = evaluate(&record["case"])["report"].clone();
        if report["status"] == "transport_failed_or_partial" {
            assert!(
                report["diagnostics"][0]["message"]
                    .as_str()
                    .unwrap()
                    .contains("presence query failed")
            );
        }
    }
}

#[test]
fn timed_out_stop_records_attempt_but_not_success() {
    method("timed_out_stop_records_attempt_but_not_success", 1);
}

#[test]
fn orchestrator_role_uses_a_distinct_agent_window() {
    method("orchestrator_role_uses_a_distinct_agent_window", 1);
    let record = corpus()
        .iter()
        .find(|r| r["method"] == "orchestrator_role_uses_a_distinct_agent_window")
        .unwrap();
    let actual = evaluate(&record["case"]);
    assert_eq!(actual["report"]["status"], "transport_completed");
    assert!(actual["calls"].as_array().unwrap().iter().any(|call| {
        call["argv"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg == "agent-orchestrator")
    }));
}

#[test]
fn invalid_plan_stop_still_has_complete_report_schema() {
    method("invalid_plan_stop_still_has_complete_report_schema", 2);
    for record in corpus()
        .iter()
        .filter(|r| r["method"] == "invalid_plan_stop_still_has_complete_report_schema")
    {
        let actual = evaluate(&record["case"]);
        // Full exact field set and values, not merely presence/non-null checks.
        assert_eq!(
            actual["report"],
            json!({
                "operation":"runtime_stop", "status":"transport_failed_or_partial",
                "completed_steps":0, "handles":{}, "outcome_unknown":false,
                "stop_command_attempted":false, "stop_command_succeeded":false,
                "diagnostics":[{"step":null,"error":"ValueError",
                    "message":"plan differs from deterministic runtime_launch v1 plan"}]
            })
        );
        // No native runtime_stop report validator is exposed by actions yet.
        // The review keeps that original _report assertion explicitly unresolved.
        assert_eq!(actual["calls"], json!([]));
    }
}

#[test]
fn strict_recomputed_schema() {
    let original = corpus()
        .iter()
        .find(|r| {
            r["method"] == "happy_transport_literal_payload_and_exact_call_contract"
                && r["case"]["inputs"]["multiplexer"] == "tmux"
        })
        .unwrap()["expected"]["plan"]
        .clone();
    for mutation in 0..9 {
        let mut p = original.clone();
        match mutation {
            0 => p["version"] = json!(true),
            1 => p["version"] = json!(2),
            2 => p["operation"] = json!("task_dispatch"),
            3 => p["cwd"] = json!("/elsewhere"),
            4 => p["hidden_program"] = json!("evil"),
            5 => p["steps"].as_array_mut().unwrap().reverse(),
            6 => p["steps"][1]["argv"]
                .as_array_mut()
                .unwrap()
                .push(json!("evil")),
            7 => p["steps"][3]["seconds"] = json!(5.0),
            8 => p["steps"][1]["outputs"] = json!({"evil":["command"]}),
            _ => unreachable!(),
        }
        let p = native(&p);
        assert!(validate_plan(&p).is_err(), "mutation {mutation}");
        struct Never;
        impl Runner for Never {
            fn run(&self, _: &crate::runtime_command::Request) -> crate::runtime_command::Output {
                panic!("invalid plan must not run commands");
            }
        }
        let report = super::tests::run_transcript(
            &p,
            &Never,
            &mut |_| panic!("invalid plan must not sleep"),
            false,
        );
        assert_eq!(
            json(&report)["status"],
            "transport_failed_or_partial",
            "mutation {mutation}"
        );
    }
}
