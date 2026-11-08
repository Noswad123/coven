//! Frozen reference data only. No Python execution or test-only public API.
use super::*;
use crate::values::{self, Number, Profile};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const CORPUS: &str = include_str!("../../tests/retirement/fixtures/records.json");
const PROVENANCE: &str = include_str!("../../tests/retirement/fixtures/records_provenance.json");
const METHODS: [(&str, usize); 4] = [
    ("test_task_schema_rejects_bad_shapes_and_known_fields", 12),
    ("test_agent_and_lead_schemas_reject_bad_shapes", 18),
    (
        "test_metadata_contract_rejects_unsupported_versions_runtime_and_labels",
        21,
    ),
    (
        "test_valid_legacy_records_and_unknown_fields_are_not_normalized",
        4,
    ),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    schema_version: u32,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    method: String,
    operation: String,
    input_json: String,
    expected: Expected,
    input_unchanged: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expected {
    ok: bool,
    output_json: Option<String>,
    same_object: Option<bool>,
    error: Option<String>,
    message: Option<String>,
}

// Track every container buffer, object key, string and integer backing buffer.
// Root stack slots can move into owned wrappers; their heap allocations must not.
fn allocations(value: &Value, result: &mut Vec<(&'static str, usize, usize)>) {
    match value {
        Value::Object(object) => {
            let entries = object.entries();
            result.push(("object", entries.as_ptr() as usize, entries.len()));
            for (key, value) in entries {
                result.push(("key", key.as_ptr() as usize, key.len()));
                allocations(value, result);
            }
        }
        Value::Array(array) => {
            result.push(("array", array.as_ptr() as usize, array.len()));
            for value in array {
                allocations(value, result);
            }
        }
        Value::String(string) => result.push(("string", string.as_ptr() as usize, string.len())),
        Value::Number(Number::Integer(integer)) => result.push((
            "integer",
            integer.as_str().as_ptr() as usize,
            integer.as_str().len(),
        )),
        _ => {}
    }
}

fn validate_preserving(operation: &str, input: Value, expected: &Expected) {
    let before = input.compact().unwrap();
    let storage = input.storage().unwrap();
    let mut pointers = Vec::new();
    allocations(&input, &mut pointers);
    let check = |value: &Value| {
        assert!(expected.ok);
        assert_eq!(expected.same_object, Some(true));
        assert_eq!(value.compact().unwrap(), before);
        assert_eq!(value.storage().unwrap(), storage);
        let output = values::parse(
            expected.output_json.as_ref().unwrap().as_bytes(),
            Profile::LegacySnapshot,
        )
        .unwrap();
        assert_eq!(value.compact().unwrap(), output.compact().unwrap());
        let mut after = Vec::new();
        allocations(value, &mut after);
        assert_eq!(
            after, pointers,
            "validation must retain all nested allocations"
        );
    };
    let result = match operation {
        "team" => TeamRecord::validate(input).map(|record| check(record.value())),
        "tasks" => TaskRecords::validate(input).map(|record| {
            check(record.value());
            check(&record.into_value());
        }),
        "agents" | "lead" => {
            // The actual private validators borrow immutably and return unit,
            // not a replacement. Do not invent a returned-reference identity.
            let validator: fn(&Value) -> Result<()> =
                if operation == "agents" { agents } else { lead };
            let root = &input as *const Value;
            let result = validator(&input);
            assert!(std::ptr::eq(root, &input));
            assert_eq!(input.compact().unwrap(), before);
            let mut after = Vec::new();
            allocations(&input, &mut after);
            assert_eq!(after, pointers);
            if result.is_ok() {
                check(&input);
            }
            result
        }
        _ => panic!("unknown operation: {operation}"),
    };
    assert_eq!(result.is_ok(), expected.ok);
    if !expected.ok {
        assert_eq!(expected.error.as_deref(), Some("ValueError"));
        assert!(!expected.message.as_ref().unwrap().is_empty());
        assert!(expected.output_json.is_none());
        assert!(expected.same_object.is_none());
    }
}

fn replay(method_index: usize) {
    let corpus: Corpus = serde_json::from_str(CORPUS).unwrap();
    let (method, count) = METHODS[method_index];
    let cases: Vec<_> = corpus
        .cases
        .iter()
        .filter(|case| case.method == method)
        .collect();
    assert_eq!(cases.len(), count);
    for case in cases {
        assert!(case.input_unchanged, "{}", case.id);
        let parsed = values::parse(case.input_json.as_bytes(), Profile::LegacySnapshot);
        if case.id == "r051" {
            assert!(case.input_json.contains("\\ud800"));
            assert!(!case.expected.ok);
            assert_eq!(case.expected.error.as_deref(), Some("ValueError"));
            // Python rejects this at record validation; it cannot inhabit a
            // Rust String, so the real input boundary rejects it earlier.
            assert_eq!(parsed.unwrap_err().0, "unsupported_or_invalid_json_scalar");
            continue;
        }
        let input = parsed.unwrap_or_else(|error| panic!("{}: {error:?}", case.id));
        if case.id == "r036" {
            assert!(matches!(
                input.as_object().unwrap().get("version"),
                Some(Value::Bool(true))
            ));
            assert_eq!(
                TeamRecord::validate(input.clone()).err().unwrap().0,
                "unsupported_workspace_version"
            );
        }
        validate_preserving(&case.operation, input, &case.expected);
    }
}

#[test]
fn task_schema_rejects_bad_shapes_and_known_fields() {
    replay(0);
}

#[test]
fn agent_and_lead_schemas_reject_bad_shapes() {
    replay(1);
}

#[test]
fn metadata_contract_rejects_unsupported_versions_runtime_and_labels() {
    replay(2);
}

#[test]
fn valid_legacy_records_and_unknown_fields_are_not_normalized() {
    replay(3);
}

#[test]
fn ordered_finite_extensions_retain_numbers_bytes_and_allocations() {
    // Additional native invariants, not extra oracle calls or mapped originals.
    // Literal input bytes are the preservation target, never native-derived output.
    let extension = r#"{"z":123456789012345678901234567890,"a":-0.0,"m":1.25,"b":true,"n":null,"s":"  untouched  ","nested":[{},[],"x"]}"#;
    for (operation, raw) in [
        (
            "team",
            format!(r#"{{"name":"Legacy","agents":["Jamal","_"],"extension":{extension}}}"#),
        ),
        (
            "tasks",
            format!(
                r#"[{{"id":"T001","title":"","status":"custom wait","extension":{extension}}}]"#
            ),
        ),
        (
            "agents",
            format!(
                r#"[{{"id":"retired","role":"","status":"custom status","extension":{extension}}}]"#
            ),
        ),
        (
            "lead",
            format!(r#"{{"agent":"_","extension":{extension}}}"#),
        ),
    ] {
        let input = values::parse(raw.as_bytes(), Profile::LegacySnapshot).unwrap();
        assert_eq!(input.compact().unwrap(), raw.as_bytes());
        validate_preserving(
            operation,
            input,
            &Expected {
                ok: true,
                output_json: Some(raw),
                same_object: Some(true),
                error: None,
                message: None,
            },
        );
    }
}

#[test]
fn frozen_inventory_and_provenance_are_complete() {
    let corpus: Corpus = serde_json::from_str(CORPUS).unwrap();
    assert_eq!(corpus.schema_version, 1);
    assert_eq!(corpus.cases.len(), 55);
    let provenance: serde_json::Value = serde_json::from_str(PROVENANCE).unwrap();
    let hash = |raw: &[u8]| format!("{:x}", Sha256::digest(raw));
    assert_eq!(
        hash(CORPUS.as_bytes()),
        "a65e5d492eabfef677824cbad2f64bda2fe1d3be26c69d298035ec4c6ec78da0"
    );
    assert_eq!(provenance["corpus"]["sha256"], hash(CORPUS.as_bytes()));
    assert_eq!(
        provenance["capture_tool_sha256"],
        "f0f5ea1f725885ea7752e3cb660e7f827a6ffa9e907d19559ecb79289127fa4f"
    );
    assert_eq!(
        provenance["sources_sha256"]["tests/test_records.py"],
        "b2a45dff593f9f1b60cb36fefa14ccfd1ee4cddcbc447e6348cab04ef2739d65"
    );
    assert_eq!(
        provenance["sources_sha256"]["runner/records.py"],
        "eb0b96a440cbc6b2acde77b488dacfb6358145f5ca75801b4d2ecc41c124e4ca"
    );
    assert_eq!(
        provenance["oracle_head"],
        "e389a08669204d4e51a49ffd13cbe01b90b5d8ba"
    );
    assert!(
        provenance["python_version"]
            .as_str()
            .unwrap()
            .starts_with("3.14.8 ")
    );
    assert_eq!(provenance["python_isolated"], true);
    assert_eq!(provenance["python_no_bytecode"], true);
    assert_eq!(provenance["reference_files_match_commit"], true);
    assert_eq!(provenance["calls"], 55);
    assert_eq!(provenance["methods"].as_object().unwrap().len(), 4);
    for (index, case) in corpus.cases.iter().enumerate() {
        assert_eq!(case.id, format!("r{:03}", index + 1));
        assert!(METHODS.iter().any(|(method, _)| *method == case.method));
    }
    for (method, count) in METHODS {
        let identity = format!("python::tests/test_records.py::RecordTests::{method}");
        let entry = &provenance["methods"][identity];
        assert_eq!(entry["calls"], count);
        let ids: Vec<_> = corpus
            .cases
            .iter()
            .filter(|case| case.method == method)
            .map(|case| case.id.as_str())
            .collect();
        assert_eq!(entry["case_ids"], serde_json::json!(ids));
    }
}
