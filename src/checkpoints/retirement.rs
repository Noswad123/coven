//! Data-only retirement corpus. The optional Python capture is never run here.
use super::*;
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};

const CORPUS: &[u8] = include_bytes!("../../tests/retirement/fixtures/checkpoints.json");
const PROVENANCE: &[u8] =
    include_bytes!("../../tests/retirement/fixtures/checkpoints_provenance.json");
const REVIEW: &[u8] = include_bytes!("../../tests/retirement/reviews/checkpoints.json");

fn fixtures() -> (Json, Vec<String>) {
    let corpus: Json = serde_json::from_slice(CORPUS).unwrap();
    let mut strings: Vec<String> = Vec::new();
    for entry in corpus["strings"].as_array().unwrap() {
        let text = if let Some(text) = entry["text"].as_str() {
            text.to_owned()
        } else {
            // Lossless generic character-offset deltas, never checkpoint logic.
            let mut chars: Vec<char> = strings[entry["base"].as_u64().unwrap() as usize]
                .chars()
                .collect();
            for edit in entry["edits"].as_array().unwrap().iter().rev() {
                chars.splice(
                    edit[0].as_u64().unwrap() as usize..edit[1].as_u64().unwrap() as usize,
                    edit[2].as_str().unwrap().chars(),
                );
            }
            chars.into_iter().collect()
        };
        strings.push(text);
    }
    (corpus, strings)
}

fn text<'a>(reference: &Json, strings: &'a [String]) -> &'a str {
    &strings[reference["string"].as_u64().unwrap() as usize]
}

fn bytes<'a>(input: &Json, strings: &'a [String]) -> Option<&'a [u8]> {
    (!input.is_null()).then(|| text(&input["bytes_utf8"], strings).as_bytes())
}

fn field<'a>(view: &'a Value, name: &str) -> &'a Value {
    view.as_object().unwrap().get(name).unwrap()
}

fn mutate(value: &mut Value) {
    match value {
        Value::Object(object) => {
            let entries = object.entries().to_vec();
            for (key, mut child) in entries {
                mutate(&mut child);
                value.set_field(&key, child).unwrap();
            }
            value
                .set_field("mutation_sentinel", Value::Bool(true))
                .unwrap();
        }
        Value::Array(array) => {
            for child in array.iter_mut() {
                mutate(child);
            }
            array.push(Value::Null);
        }
        Value::String(string) => string.push_str("mutation_sentinel"),
        _ => {}
    }
}

// The revision validator is private in a sibling module. Exercise its real
// entry point before workspace access, rather than duplicating its grammar.
fn revision_boundary(token: Option<&str>, accepted: bool) {
    use crate::checkpoint_decisions::{DecisionAction, DecisionRequest, decide};
    let root = tempfile::tempdir().unwrap();
    let request = |expected_revision| DecisionRequest {
        checkpoint: "plan".into(),
        action: DecisionAction::Approve,
        notes: String::new(),
        source: "cli".into(),
        agent: "all".into(),
        expected_revision,
    };
    let baseline = decide(root.path(), request(None), true).unwrap_err();
    assert_ne!(baseline.code(), "invalid_expected_checkpoint_revision");
    let error = decide(root.path(), request(token.map(str::to_owned)), true).unwrap_err();
    assert_eq!(
        error.code(),
        if accepted {
            baseline.code()
        } else {
            "invalid_expected_checkpoint_revision"
        }
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn frozen_calls_exact_views_outputs_and_independence() {
    let (corpus, strings) = fixtures();
    let mut direct = 0;
    for case in corpus["cases"].as_array().unwrap() {
        if case["native_route"] != "direct" {
            continue;
        }
        direct += 1;
        let id = case["id"].as_str().unwrap();
        let input = &case["input"];
        let expected = &case["expected"];
        let ok = expected["ok"].as_bool().unwrap();
        let operation = case["operation"].as_str().unwrap();
        if operation == "validate_revision" {
            revision_boundary(input["token"].as_str(), ok);
            if ok {
                assert_eq!(expected["token"], input["token"], "{id}");
            }
            continue;
        }
        let checkpoint = input["checkpoint"].as_str().unwrap();
        let raw = bytes(&input["data"], &strings);
        let untouched = raw.map(<[u8]>::to_vec);
        let result = if operation == "inspect" {
            inspect_bytes(checkpoint, raw).map(|view| (None, view))
        } else {
            assert_eq!(operation, "prepare", "{id}");
            prepare(
                checkpoint,
                raw,
                input["action"].as_str().unwrap(),
                input["notes"].as_str().unwrap(),
                input["decision_id"].as_str().unwrap(),
                input["decided_at"].as_str().unwrap(),
                input["source"].as_str().unwrap(),
                bytes(&input["initial_content"], &strings),
            )
            .map(|(output, view)| (Some(output), view))
        };
        assert_eq!(raw, untouched.as_deref(), "input changed: {id}");
        if !ok {
            let error = result.expect_err(id);
            assert!(
                error.code().starts_with("invalid_checkpoint_")
                    || error.code() == "missing_checkpoint_initial_content",
                "unexpected refusal {id}: {error:?}"
            );
            assert_eq!(expected["type"], "CheckpointError");
            assert!(
                expected["message"]
                    .as_str()
                    .unwrap()
                    .starts_with("Cannot edit checkpoint")
            );
            continue;
        }
        let (output, mut view) = result.unwrap_or_else(|error| panic!("{id}: {error:?}"));
        let canonical = view.canonical().unwrap();
        // Compare directly with frozen reference bytes. serde is only the
        // fixture envelope reader, never the expected authority serializer.
        assert_eq!(
            canonical,
            text(&expected["canonical_view"], &strings).as_bytes(),
            "{id}"
        );
        let mut independent = inspect_bytes(checkpoint, output.as_deref().or(raw)).unwrap();
        assert_eq!(independent.canonical().unwrap(), canonical, "{id}");
        if let Some(output) = &output {
            assert_eq!(
                output,
                text(&expected["bytes_utf8"], &strings).as_bytes(),
                "{id}"
            );
            assert_eq!(
                field(&view, "content").as_str().unwrap().as_bytes(),
                output,
                "{id}"
            );
            let before = inspect_bytes(checkpoint, raw).unwrap();
            let original = raw
                .or_else(|| bytes(&input["initial_content"], &strings))
                .unwrap();
            let base = inspect_bytes(checkpoint, Some(original)).unwrap();
            assert_eq!(
                field(&view, "scope_revision").canonical().unwrap(),
                field(&base, "scope_revision").canonical().unwrap(),
                "{id}"
            );
            assert_eq!(
                field(field(&view, "decision"), "reviewed_revision")
                    .canonical()
                    .unwrap(),
                field(&before, "revision").canonical().unwrap(),
                "{id}"
            );
            assert_eq!(
                field(field(&view, "decision"), "scope_revision")
                    .canonical()
                    .unwrap(),
                field(&view, "scope_revision").canonical().unwrap(),
                "{id}"
            );
            assert_eq!(
                field(&view, "effective_status").as_str(),
                input["action"].as_str(),
                "{id}"
            );
            assert_eq!(field(&view, "binding").as_str(), Some("bound"), "{id}");
            let old_history = field(&base, "history").as_array().unwrap();
            let history = field(&view, "history").as_array().unwrap();
            assert_eq!(history.len(), old_history.len() + 1, "{id}");
            for (old, current) in old_history.iter().zip(history) {
                assert_eq!(
                    old.canonical().unwrap(),
                    current.canonical().unwrap(),
                    "{id}"
                );
            }
            assert_eq!(
                field(history.last().unwrap(), "notes").as_str(),
                input["notes"].as_str(),
                "{id}"
            );
            // Check original machine-history byte spans, including separators,
            // not reconstructed blocks or merely equal parsed records.
            for (start, end) in scan(original, checkpoint).unwrap().history_spans {
                assert!(
                    contains(output, &original[start..end]),
                    "old history changed: {id}"
                );
            }
            assert_eq!(
                before.canonical().unwrap(),
                inspect_bytes(checkpoint, raw).unwrap().canonical().unwrap(),
                "{id}"
            );
        }
        mutate(&mut view);
        assert_eq!(independent.canonical().unwrap(), canonical, "alias: {id}");
        mutate(&mut independent);
        assert_eq!(
            inspect_bytes(checkpoint, output.as_deref().or(raw))
                .unwrap()
                .canonical()
                .unwrap(),
            canonical,
            "retained mutation: {id}"
        );
    }
    assert_eq!(direct, 156);
}

#[test]
fn inventory_hashes_and_typed_boundaries() {
    let (corpus, _) = fixtures();
    let provenance: Json = serde_json::from_slice(PROVENANCE).unwrap();
    let review: Json = serde_json::from_slice(REVIEW).unwrap();
    let digest = |raw: &[u8]| format!("{:x}", Sha256::digest(raw));
    assert_eq!(
        digest(CORPUS),
        "2dfbadc3e7757f8e383776127e35ca460e29b95540c29099e8fb9b3bb18c7dba"
    );
    assert_eq!(provenance["corpus_sha256"], digest(CORPUS));
    assert_eq!(
        digest(PROVENANCE),
        "8962f03dfe96b15f3ac10d4f93109b9c8d19e0e11268975292618448be493472"
    );
    assert_eq!(
        provenance["capture_tool_sha256"],
        "b44f9bb7dc8b7afa445fcc62847931b48aa0acdf5840c1b0546e4c9e06cbd06c"
    );
    assert_eq!(
        provenance["oracle_head"],
        "e389a08669204d4e51a49ffd13cbe01b90b5d8ba"
    );
    assert_eq!(provenance["reference_files_match_commit"], true);
    assert_eq!(provenance["isolated"], true);
    assert_eq!(provenance["no_bytecode"], true);
    assert!(
        provenance["python_version"]
            .as_str()
            .unwrap()
            .starts_with("3.14.8")
    );
    let methods = provenance["methods"].as_object().unwrap();
    assert_eq!(methods.len(), 16);
    assert_eq!(review["reviews"].as_object().unwrap().len(), 18);
    let mut calls = BTreeMap::new();
    let mut typed = BTreeSet::new();
    for case in corpus["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        assert!(calls.insert(id, case).is_none(), "duplicate {id}");
        if case["native_route"] != "direct" {
            assert_eq!(case["expected"]["ok"], false);
            let input = &case["input"];
            match case["native_route"].as_str().unwrap() {
                "typed_notes_boundary" => {
                    assert!(
                        input["notes"].is_null() || input["notes"]["python_type"] == "surrogate"
                    );
                    typed.insert(if input["notes"].is_null() {
                        "None"
                    } else {
                        "surrogate"
                    });
                }
                "typed_revision_boundary" => {
                    assert_eq!(input["token"], false);
                    typed.insert("False");
                }
                route => panic!("unreviewed route {route}"),
            }
        }
    }
    assert_eq!(calls.len(), 159);
    assert_eq!(typed, BTreeSet::from(["False", "None", "surrogate"]));
    let mut mapped = BTreeSet::new();
    for (method, evidence) in methods {
        let entry = &review["reviews"][method];
        assert_eq!(entry["status"], "mapped");
        assert_eq!(entry["category"], "A");
        assert_eq!(entry["assertion_breakdown"], *evidence);
        assert!(
            !evidence["original_assertions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        for id in evidence["call_ids"].as_array().unwrap() {
            let id = id.as_str().unwrap();
            assert!(mapped.insert(id));
            assert_eq!(calls[id]["method"], method.as_str());
        }
        assert_eq!(entry["fixtures"][0]["sha256"], digest(CORPUS));
        assert_eq!(entry["fixtures"][1]["sha256"], digest(PROVENANCE));
    }
    assert_eq!(mapped.len(), calls.len());
    for (method, entry) in review["reviews"].as_object().unwrap() {
        if !methods.contains_key(method) {
            assert_eq!(entry["status"], "unresolved");
            assert_eq!(entry["category"], "D");
        }
    }
    // Exact native signature: None, bool and lone UTF-16 surrogates cannot
    // inhabit &str. No invented bytes or string coercions substitute for them.
    #[allow(clippy::type_complexity)]
    let _: fn(
        &str,
        Option<&[u8]>,
        &str,
        &str,
        &str,
        &str,
        &str,
        Option<&[u8]>,
    ) -> Result<(Vec<u8>, Value)> = prepare;
    assert!(String::from_utf16(&[0xd800]).is_err());
}

#[test]
fn independent_hash_domains() {
    // Independent literal vectors, not generated by the native implementation.
    for (kind, raw, expected) in [
        (
            "snapshot",
            b"".as_slice(),
            "checkpoint-snapshot-v1:928859b8f8f129c2b2eeab608e81f060ba50cb41c98d2285eb43df0b0c92ec65",
        ),
        (
            "scope",
            b"".as_slice(),
            "checkpoint-scope-v1:561c68f1a5a88f08320eb1d214b2b930f979b8d920b4b3ccda56ebc285db585d",
        ),
        (
            "snapshot",
            b"# Plan\r\nStatus: pending\r## User decision\n- [ ] Approved\r\nTail\x0b\x0c"
                .as_slice(),
            "checkpoint-snapshot-v1:f40612f316849a77421b7b4d7e017dc873568100f5a077619815c13e0e2d8a9d",
        ),
    ] {
        assert_eq!(hash(kind, "plan", raw), expected);
        assert_ne!(hash(kind, "other", raw), expected);
    }
    let missing = inspect_bytes("plan", None).unwrap();
    assert_eq!(
        field(&missing, "revision").as_str(),
        Some("checkpoint-missing-v1:plan")
    );
    assert!(matches!(field(&missing, "scope_revision"), Value::Null));
    assert_ne!(
        field(&missing, "revision").canonical().unwrap(),
        field(&inspect_bytes("plan", Some(b"")).unwrap(), "revision")
            .canonical()
            .unwrap()
    );
}
