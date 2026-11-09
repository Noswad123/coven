//! Frozen characterization, not a Coven implementation or authorization policy.
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};

const AGENTS: [&str; 16] = [
    "claude",
    "codex",
    "copilot",
    "devin",
    "droid",
    "djinn",
    "kimi",
    "mastracode",
    "pi",
    "omp",
    "hermes",
    "opencode",
    "qodercli",
    "kilo",
    "cursor",
    "grok",
];
const BYTES: &[u8] = include_bytes!("fixtures/kitsune_native_reference/reference.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    reference: Value,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    operation: String,
    input: Value,
    output: Value,
}

fn fixture() -> Fixture {
    serde_json::from_slice(BYTES).unwrap()
}

fn case<'a>(fixture: &'a Fixture, name: &str) -> &'a Case {
    fixture
        .cases
        .iter()
        .find(|case| case.name == name)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn selected(kind: &str, value: &str) -> Value {
    json!({"kind": kind, "value": value})
}

fn exact_keys(value: &Value, keys: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
    })
}

fn valid_selector(value: &Value) -> bool {
    exact_keys(value, &["kind", "value"])
        && matches!(value["kind"].as_str(), Some("id" | "path"))
        && value["value"].is_string()
}

// Schema and persisted identity only: do not reimplement reference selection policy.
fn valid_record(record: &Value) -> bool {
    if !exact_keys(record, &["name", "operation", "input", "output"]) {
        return false;
    }
    let Ok(case) = serde_json::from_value::<Case>(record.clone()) else {
        return false;
    };
    let input = &case.input;
    let output = &case.output;
    let fields: &[&str] = match case.operation.as_str() {
        "report" => &["source", "agent", "id", "path"],
        "snapshot" | "dedupe" => &["source", "agent", "kind", "value"],
        "reserved" => &["source", "agent"],
        "normalize" => &["value"],
        _ => return false,
    };
    if !exact_keys(input, fields) {
        return false;
    }
    let nullable_string = |value: &Value| value.is_null() || value.is_string();
    if case.operation == "normalize" {
        return nullable_string(&input["value"]) && nullable_string(output);
    }
    if !input["source"].is_string() || !input["agent"].is_string() {
        return false;
    }
    match case.operation.as_str() {
        "report" => {
            nullable_string(&input["id"])
                && nullable_string(&input["path"])
                && (output.is_null() || valid_selector(output))
        }
        "reserved" => output.is_boolean(),
        "snapshot" | "dedupe" => {
            if !matches!(input["kind"].as_str(), Some("id" | "path")) || !input["value"].is_string()
            {
                return false;
            }
            if case.operation == "dedupe" {
                return output.is_string();
            }
            output.is_null()
                || (exact_keys(output, &["source", "agent", "session_ref"])
                    && output["source"] == input["source"]
                    && output["agent"] == input["agent"]
                    && valid_selector(&output["session_ref"])
                    && output["session_ref"]["kind"] == input["kind"]
                    && output["session_ref"]["value"] == input["value"])
        }
        _ => false,
    }
}

#[test]
fn record_schema_rejects_malformed_mutations_without_hash_check() {
    let fixture: Value = serde_json::from_slice(BYTES).unwrap();
    let records = fixture["cases"].as_array().unwrap();
    for record in records {
        assert!(valid_record(record), "{}", record["name"]);
    }
    let mut rejected = 0;
    for operation in ["report", "snapshot", "reserved", "normalize", "dedupe"] {
        let record = records
            .iter()
            .find(|record| record["operation"] == operation && !record["output"].is_null())
            .unwrap();
        // Every object has required, exact keys, including nullable fields.
        for path in ["", "/input", "/output", "/output/session_ref"] {
            let Some(object) = record.pointer(path).and_then(Value::as_object) else {
                continue;
            };
            for key in object.keys() {
                let mut mutated = record.clone();
                mutated
                    .pointer_mut(path)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                assert!(!valid_record(&mutated), "{operation}: missing {path}/{key}");
                rejected += 1;
            }
            let mut mutated = record.clone();
            mutated
                .pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unexpected".into(), Value::Null);
            assert!(
                !valid_record(&mutated),
                "{operation}: unknown field in {path}"
            );
            rejected += 1;
        }
        let mut mutations = vec![
            ("/name".to_owned(), json!(false)),
            ("/operation".to_owned(), json!(null)),
            ("/operation".to_owned(), json!("unknown")),
            ("/input".to_owned(), json!([])),
            ("/output".to_owned(), json!([])),
        ];
        for key in record["input"].as_object().unwrap().keys() {
            mutations.push((format!("/input/{key}"), json!(false)));
            if !["id", "path"].contains(&key.as_str()) && operation != "normalize" {
                mutations.push((format!("/input/{key}"), Value::Null));
            }
        }
        if ["snapshot", "dedupe"].contains(&operation) {
            mutations.push(("/input/kind".into(), json!("ID")));
        }
        if operation == "report" || operation == "snapshot" {
            let prefix = if operation == "snapshot" {
                "/output/session_ref"
            } else {
                "/output"
            };
            mutations.extend([
                (format!("{prefix}/kind"), json!("unknown")),
                (format!("{prefix}/kind"), Value::Null),
                (format!("{prefix}/value"), json!(42)),
                (format!("{prefix}/value"), Value::Null),
            ]);
        }
        if operation == "snapshot" {
            mutations.extend([
                ("/output/source".into(), Value::Null),
                ("/output/agent".into(), json!(42)),
                ("/output/source".into(), json!("different-source")),
                ("/output/agent".into(), json!("different-agent")),
                ("/output/session_ref".into(), Value::Null),
                ("/output/session_ref/kind".into(), json!("path")),
                ("/output/session_ref/value".into(), json!("different-value")),
            ]);
        }
        for (path, value) in mutations {
            let mut mutated = record.clone();
            *mutated.pointer_mut(&path).unwrap() = value;
            assert!(
                !valid_record(&mutated),
                "{operation}: wrong type/identity at {path}"
            );
            rejected += 1;
        }
        // None is a valid outcome even for invalid reference selector data.
        if ["report", "snapshot"].contains(&operation) {
            let mut absent = record.clone();
            absent["output"] = Value::Null;
            if operation == "snapshot" {
                absent["input"]["value"] = json!("bad\nselector");
            } else {
                absent["input"]["id"] = json!("bad\nselector");
                absent["input"]["path"] = Value::Null;
            }
            assert!(valid_record(&absent));
        }
    }
    assert_eq!(rejected, 124);
}

#[test]
fn integrity_provenance_schema_and_coverage() {
    assert_eq!(
        format!("{:x}", Sha256::digest(BYTES)),
        "c18f0b9f2871088dfdd0e9aa1ad2987c350333175c7ade72bca4e55e8e186712"
    );
    let fixture = fixture();
    assert_eq!(
        fixture.reference,
        json!({
            "repository": "kitsune", "commit": "01d7cef6e98e2ccab216cd669d569c0731abfbce",
            "source": "src/agent_resume.rs", "blob": "0c3b51bd19d73aa0ede31d6775cac35395447273",
            "capture": "unchanged module copied to OUT_DIR after git blob byte verification",
            "harness": "tools/harnesses/kitsune-resume-reference", "mode": "--native-policy",
            "dependencies": {"serde": "1.0.228", "serde_json": "1.0.150", "schemars": "1.2.1"},
            "platform": "unix", "schema_version": 1
        })
    );
    assert_eq!(fixture.cases.len(), 5282);
    let mut names = HashSet::new();
    let mut counts = BTreeMap::new();
    for case in &fixture.cases {
        assert!(names.insert(&case.name), "duplicate {}", case.name);
        *counts.entry(case.operation.as_str()).or_insert(0) += 1;
        assert!(
            valid_record(&json!({
                "name": case.name, "operation": case.operation,
                "input": case.input, "output": case.output,
            })),
            "{}",
            case.name
        );
    }
    assert_eq!(
        counts,
        BTreeMap::from([
            ("report", 3216),
            ("snapshot", 1608),
            ("reserved", 420),
            ("normalize", 30),
            ("dedupe", 8)
        ])
    );
    for (prefix, count) in [("matrix/", 2268), ("variant/", 672), ("edge/", 2304)] {
        assert_eq!(
            fixture
                .cases
                .iter()
                .filter(|case| case.name.starts_with(prefix))
                .count(),
            count
        );
    }
}

#[test]
fn exact_source_provider_crossproduct_and_variants() {
    let fixture = fixture();
    let sources: Vec<_> = AGENTS
        .iter()
        .map(|agent| format!("kitsune:{agent}"))
        .chain(["kitsune:unknown".into(), "custom:claude".into()])
        .collect();
    for source in &sources {
        for agent in AGENTS.into_iter().chain(["unknown", "custom-agent"]) {
            let name = format!("matrix/{source}/{agent}");
            let both = case(&fixture, &format!("{name}/report/both"));
            assert_eq!(
                both.input,
                json!({"source": source, "agent": agent, "id": "saved-id", "path": "/saved/session"})
            );
            let id_snapshot = case(&fixture, &format!("{name}/snapshot/id"));
            let path_snapshot = case(&fixture, &format!("{name}/snapshot/path"));
            let official = AGENTS.contains(&agent) && *source == format!("kitsune:{agent}");
            assert_eq!(!both.output.is_null(), official, "{name}");
            assert_eq!(!id_snapshot.output.is_null(), official, "{name}");
            assert_eq!(
                !path_snapshot.output.is_null(),
                official && ["pi", "omp"].contains(&agent),
                "{name}"
            );
            if !official {
                for label in ["id-only", "path-only", "missing"] {
                    assert!(
                        case(&fixture, &format!("{name}/report/{label}"))
                            .output
                            .is_null()
                    );
                }
            }
            assert!(
                case(&fixture, &format!("{name}/report/missing"))
                    .output
                    .is_null()
            );
        }
    }
    for case in fixture
        .cases
        .iter()
        .filter(|case| case.name.starts_with("variant/"))
    {
        if case.operation == "reserved" {
            assert_eq!(case.output, false, "{}", case.name);
        } else {
            assert!(case.output.is_null(), "{}", case.name);
        }
    }
}

#[test]
fn report_path_preference_fallback_and_id_only_path_ignoring() {
    let fixture = fixture();
    for agent in AGENTS {
        let base = format!("matrix/kitsune:{agent}/{agent}/report");
        let paths = ["pi", "omp"].contains(&agent);
        assert_eq!(
            case(&fixture, &format!("{base}/both")).output,
            if paths {
                selected("path", "/saved/session")
            } else {
                selected("id", "saved-id")
            }
        );
        assert_eq!(
            case(&fixture, &format!("{base}/id-only")).output,
            selected("id", "saved-id")
        );
        assert_eq!(
            case(&fixture, &format!("{base}/path-only")).output,
            if paths {
                selected("path", "/saved/session")
            } else {
                Value::Null
            }
        );
        for edge in ["empty", "newline", "over-limit", "unicode-over-limit"] {
            let case = case(
                &fixture,
                &format!("edge/{agent}/id/{edge}/report-with-path"),
            );
            assert_eq!(
                case.output,
                if paths {
                    selected("path", "/saved/session")
                } else {
                    Value::Null
                }
            );
        }
        for edge in [
            "empty",
            "blank",
            "relative",
            "space-before-absolute",
            "newline",
            "nul",
            "tab",
            "del",
            "c1-control",
            "over-limit",
            "unicode-over-limit",
        ] {
            let base = format!("edge/{agent}/path/{edge}");
            assert_eq!(
                case(&fixture, &format!("{base}/report-with-id")).output,
                selected("id", "saved-id")
            );
            assert!(
                case(&fixture, &format!("{base}/report-without-id"))
                    .output
                    .is_null()
            );
        }
        // No Coven hardening: reference IDs accept blank and option-like data.
        for edge in [
            "blank",
            "unicode-blank",
            "leading-hyphen",
            "space-before-hyphen",
            "unicode-space-before-hyphen",
        ] {
            let case = case(&fixture, &format!("edge/{agent}/id/{edge}/report"));
            assert_eq!(
                case.output,
                selected("id", case.input["id"].as_str().unwrap())
            );
        }
    }
}

#[test]
fn snapshot_explicit_kind_byte_boundaries_controls_and_path_literals() {
    let fixture = fixture();
    for agent in AGENTS {
        for (kind, limit) in [("id", 512), ("path", 4096)] {
            let supported = kind == "id" || ["pi", "omp"].contains(&agent);
            for edge in [
                "below-limit",
                "at-limit",
                "unicode-at-limit",
                "over-limit",
                "unicode-over-limit",
            ] {
                let case = case(&fixture, &format!("edge/{agent}/{kind}/{edge}/snapshot"));
                let value = case.input["value"].as_str().unwrap();
                let expected_len = match edge {
                    "below-limit" => limit - 1,
                    "over-limit" | "unicode-over-limit" => limit + 1,
                    _ => limit,
                };
                assert_eq!(value.len(), expected_len);
                assert_eq!(
                    !case.output.is_null(),
                    supported && expected_len <= limit,
                    "{}",
                    case.name
                );
            }
            for edge in ["empty", "newline", "nul", "tab", "del", "c1-control"] {
                assert!(
                    case(&fixture, &format!("edge/{agent}/{kind}/{edge}/snapshot"))
                        .output
                        .is_null()
                );
            }
            for edge in [
                "path-components",
                "hyphen-component",
                "unicode",
                "unicode-format",
                "literal-metacharacters",
                "surrounding-spaces",
            ] {
                let case = case(&fixture, &format!("edge/{agent}/{kind}/{edge}/snapshot"));
                if supported {
                    assert_eq!(
                        case.output,
                        json!({"source": format!("kitsune:{agent}"), "agent": agent,
                        "session_ref": {"kind": kind, "value": case.input["value"]}})
                    );
                } else {
                    assert!(case.output.is_null());
                }
            }
        }
        for edge in [
            "relative",
            "blank",
            "space-before-absolute",
            "leading-hyphen",
        ] {
            assert!(
                case(&fixture, &format!("edge/{agent}/path/{edge}/snapshot"))
                    .output
                    .is_null()
            );
        }
    }
    // The same relative path falls back in reports but is strictly rejected in snapshots.
    for agent in ["pi", "omp"] {
        let base = format!("edge/{agent}/path/relative");
        assert!(case(&fixture, &format!("{base}/snapshot")).output.is_null());
        assert_eq!(
            case(&fixture, &format!("{base}/report-with-id")).output,
            selected("id", "saved-id")
        );
    }
}

#[test]
fn reservation_is_exactly_eight_pairs_not_all_official_pairs() {
    let fixture = fixture();
    let reserved: HashSet<_> = fixture
        .cases
        .iter()
        .filter(|case| case.operation == "reserved" && case.output == true)
        .map(|case| {
            (
                case.input["source"].as_str().unwrap().to_owned(),
                case.input["agent"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        reserved,
        [
            "claude", "codex", "copilot", "devin", "droid", "qodercli", "cursor", "grok"
        ]
        .into_iter()
        .map(|agent| (format!("kitsune:{agent}"), agent.to_owned()))
        .collect()
    );
    for agent in [
        "djinn",
        "kimi",
        "mastracode",
        "pi",
        "omp",
        "hermes",
        "opencode",
        "kilo",
    ] {
        assert_eq!(
            case(
                &fixture,
                &format!("matrix/kitsune:{agent}/{agent}/reserved")
            )
            .output,
            false
        );
    }
}

#[test]
fn startup_normalization_six_values_unicode_trim_not_case_folding() {
    let fixture = fixture();
    for value in ["startup", "resume", "clear", "compact", "new", "fork"] {
        for (label, input) in [
            ("exact", value.to_owned()),
            ("ascii-trim", format!(" \t{value}\n")),
            ("unicode-trim", format!("\u{2003}{value}\u{a0}")),
        ] {
            let case = case(&fixture, &format!("normalize/{label}/{input}"));
            assert_eq!(case.input["value"], input);
            assert_eq!(case.output, value);
        }
        assert!(
            case(
                &fixture,
                &format!("normalize/case/{}", value.to_uppercase())
            )
            .output
            .is_null()
        );
    }
    for label in [
        "missing",
        "empty",
        "blank",
        "unknown",
        "internal-space",
        "nul",
    ] {
        assert!(
            case(&fixture, &format!("normalize/{label}"))
                .output
                .is_null()
        );
    }
}

#[test]
fn reference_only_dedupe_separates_fields_without_normalizing_or_authorizing() {
    let fixture = fixture();
    let outputs: HashSet<_> = fixture
        .cases
        .iter()
        .filter(|case| case.operation == "dedupe")
        .map(|case| case.output.as_str().unwrap())
        .collect();
    assert_eq!(outputs.len(), 8);
    for (label, expected) in [
        ("base", "kitsune:pi\0pi\0Id\0/a/../b//session"),
        ("source", "custom:pi\0pi\0Id\0/a/../b//session"),
        ("agent", "kitsune:pi\0unknown\0Id\0/a/../b//session"),
        ("kind", "kitsune:pi\0pi\0Path\0/a/../b//session"),
        ("value", "kitsune:pi\0pi\0Id\0/b/session"),
        ("path-literal", "kitsune:pi\0pi\0Path\0/b/session"),
        ("blank", "custom:pi\0unknown\0Id\0 "),
        ("control", "custom:pi\0unknown\0Id\0a\0b"),
    ] {
        assert_eq!(case(&fixture, &format!("dedupe/{label}")).output, expected);
    }
}
