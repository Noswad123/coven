//! Frozen reference data only. No interpreter or reference checkout is used here.
use coven::session_views::{select_report, validate_selection};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path, process::Command};

const CORPUS: &[u8] = include_bytes!("retirement/fixtures/session_views.json");
const PROVENANCE: &[u8] = include_bytes!("retirement/fixtures/session_views_provenance.json");

fn fixtures() -> (Value, Vec<Value>) {
    let corpus: Value = serde_json::from_slice(CORPUS).unwrap();
    let mut documents: Vec<Value> = Vec::new();
    for entry in corpus["documents"].as_array().unwrap() {
        let mut value = if let Some(value) = entry.get("value") {
            value.clone()
        } else {
            documents[entry["base"].as_u64().unwrap() as usize].clone()
        };
        for edit in entry["patch"].as_array().into_iter().flatten() {
            // Generic JSON-pointer data expansion, not a selection implementation.
            let path = edit.get("set").or_else(|| edit.get("remove")).unwrap();
            let path = path.as_str().unwrap();
            if path.is_empty() {
                value = edit["value"].clone();
                continue;
            }
            let (parent, key) = path.rsplit_once('/').unwrap();
            let key = key.replace("~1", "/").replace("~0", "~");
            let target = value.pointer_mut(parent).unwrap();
            if let Some(object) = target.as_object_mut() {
                if edit.get("remove").is_some() {
                    assert!(object.remove(&key).is_some());
                } else {
                    object.insert(key, edit["value"].clone());
                }
            } else {
                let array = target.as_array_mut().unwrap();
                let index: usize = key.parse().unwrap();
                if edit.get("remove").is_some() {
                    array.remove(index);
                } else if index == array.len() {
                    array.push(edit["value"].clone());
                } else {
                    array[index] = edit["value"].clone();
                }
            }
        }
        documents.push(value);
    }
    (corpus, documents)
}

fn selector<'a>(options: &'a Value, field: &str) -> Option<&'a str> {
    match options.get(field) {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .expect("non-string selector needs typed-boundary evidence"),
        ),
    }
}

fn direct(case: &Value, documents: &[Value]) -> coven::Result<Value> {
    assert_eq!(case["native_route"], "direct");
    let options = &case["options"];
    let group = selector(options, "group_key");
    let member = selector(options, "member_key");
    let recognized = options
        .get("recognized_only")
        .map(|value| {
            value
                .as_bool()
                .expect("non-bool needs typed-boundary evidence")
        })
        .unwrap_or(false);
    match case["operation"].as_str().unwrap() {
        "validate" => validate_selection(group, member).map(|()| Value::Null),
        "select" => select_report(
            &documents[case["input"].as_u64().unwrap() as usize],
            group,
            member,
            recognized,
        ),
        other => panic!("unaccounted operation: {other}"),
    }
}

fn container_addresses(value: &Value, addresses: &mut BTreeSet<usize>) {
    match value {
        Value::Object(object) => {
            addresses.insert(value as *const Value as usize);
            for child in object.values() {
                container_addresses(child, addresses);
            }
        }
        Value::Array(array) => {
            addresses.insert(value as *const Value as usize);
            for child in array {
                container_addresses(child, addresses);
            }
        }
        _ => {}
    }
}

fn independent(left: &Value, right: &Value) {
    let (mut a, mut b) = (BTreeSet::new(), BTreeSet::new());
    container_addresses(left, &mut a);
    container_addresses(right, &mut b);
    assert!(a.is_disjoint(&b), "mutable descendants alias");
}

fn mutate_containers(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for child in object.values_mut() {
                mutate_containers(child);
            }
            object.insert("mutation_sentinel".into(), json!(true));
        }
        Value::Array(array) => {
            for child in array.iter_mut() {
                mutate_containers(child);
            }
            array.push(json!("mutation_sentinel"));
        }
        _ => {}
    }
}

fn preserved_context_and_order(original: &Value, selected: &Value) {
    for (key, value) in original.as_object().unwrap() {
        if ![
            "targets",
            "target_count",
            "recognized_target_count",
            "declarations",
        ]
        .contains(&key.as_str())
        {
            assert_eq!(&selected[key], value, "context: {key}");
        }
    }
    for (key, value) in original["declarations"].as_object().unwrap() {
        if key != "unmatched" {
            assert_eq!(
                &selected["declarations"][key], value,
                "declaration aggregate: {key}"
            );
        }
    }
    if let Some(view) = selected.get("view") {
        assert_eq!(view["observed_target_count"], original["target_count"]);
        assert_eq!(
            view["observed_recognized_target_count"],
            original["recognized_target_count"]
        );
        assert_eq!(view["declaration_count_scope"], "full_observation");
    }
    if original["assessment"] == "observed" {
        let targets = selected["targets"].as_array().unwrap();
        assert_eq!(selected["target_count"], targets.len());
        assert_eq!(
            selected["recognized_target_count"],
            targets
                .iter()
                .filter(|target| target["recognition"] == "djinn_executable_family")
                .count()
        );
        let panes: Vec<_> = targets.iter().map(|target| &target["pane_id"]).collect();
        let original_order: Vec<_> = original["targets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|target| &target["pane_id"])
            .filter(|pane| panes.contains(pane))
            .collect();
        assert_eq!(panes, original_order);
    }
}

#[test]
fn corpus_hash_and_every_original_method_case_are_accounted_for() {
    let (corpus, documents) = fixtures();
    let provenance: Value = serde_json::from_slice(PROVENANCE).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(CORPUS)),
        provenance["corpus"]["sha256"]
    );
    assert_eq!(corpus["schema_version"], 1);
    assert_eq!(documents.len(), 46);
    assert_eq!(corpus["cases"].as_array().unwrap().len(), 127);
    let ledger: Value = serde_json::from_str(include_str!("retirement/coverage.json")).unwrap();
    let original = ledger["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == provenance["original_tests"]["path"])
        .unwrap();
    let ids: BTreeSet<_> = original["classes"]
        .as_object()
        .unwrap()
        .iter()
        .flat_map(|(class, names)| {
            names.as_array().unwrap().iter().map(move |method| {
                format!(
                    "python::tests/test_session_views.py::{class}::{}",
                    method.as_str().unwrap()
                )
            })
        })
        .collect();
    assert_eq!(ids.len(), 19);
    assert_eq!(
        ids,
        provenance["methods"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    );
    let mut cases = BTreeSet::new();
    for method in provenance["methods"].as_object().unwrap().values() {
        assert!(!method["assertion_obligations"].as_str().unwrap().is_empty());
        assert!(!method["edge_cases"].as_array().unwrap().is_empty());
        for id in method["edge_cases"].as_array().unwrap() {
            assert!(cases.insert(id.as_str().unwrap()));
        }
    }
    assert_eq!(
        cases,
        corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|case| case["id"].as_str().unwrap())
            .collect()
    );
    for (index, case) in corpus["cases"].as_array().unwrap().iter().enumerate() {
        assert_eq!(case["id"], format!("v{:03}", index + 1));
        assert!(
            [
                "direct",
                "typed_selector_exclusion",
                "typed_bool_and_cli_refusal"
            ]
            .contains(&case["native_route"].as_str().unwrap())
        );
    }
}

#[test]
fn frozen_values_errors_context_order_and_deep_independence() {
    let (corpus, documents) = fixtures();
    let mut direct_cases = 0;
    let mut exclusions = 0;
    for case in corpus["cases"].as_array().unwrap() {
        if case["native_route"] != "direct" {
            // These are tested explicitly by typed_api_and_actual_cli_parser_boundaries.
            assert_eq!(case["expected"]["ok"], false);
            assert_eq!(case["expected"]["error"], "ValueError");
            exclusions += 1;
            continue;
        }
        direct_cases += 1;
        let inputs_before = documents.clone();
        let result = direct(case, &documents);
        assert_eq!(documents, inputs_before, "input changed: {}", case["id"]);
        if case["expected"]["ok"] == false {
            let code = if case["expected"]["message"] == "member_key requires group_key" {
                "member_requires_group"
            } else {
                "groups_invalid_selector"
            };
            assert_eq!(result.unwrap_err().code(), code, "{}", case["id"]);
            continue;
        }
        let mut selected = result.unwrap();
        let expected = &documents[case["expected"]["output"].as_u64().unwrap() as usize];
        assert_eq!(&selected, expected, "{}", case["id"]);
        if case["operation"] == "select" {
            let original = &documents[case["input"].as_u64().unwrap() as usize];
            let before = original.clone();
            preserved_context_and_order(original, &selected);
            independent(original, &selected);
            let second = direct(case, &documents).unwrap();
            independent(&selected, &second);
            mutate_containers(&mut selected);
            assert_eq!(original, &before, "input changed: {}", case["id"]);
            assert_eq!(&second, expected, "second result changed: {}", case["id"]);
            assert_eq!(&direct(case, &documents).unwrap(), expected);
            let encoded = serde_json::to_vec(&second).unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&encoded).unwrap(), second);
        }
    }
    assert_eq!((direct_cases, exclusions), (107, 20));
}

#[test]
fn independent_calls_do_not_retain_mutation_or_prior_success() {
    let (corpus, documents) = fixtures();
    let case = |id: &str| {
        corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"] == id)
            .unwrap()
    };
    let original = &documents[1];
    let before = original.clone();
    let mut first = direct(case("v095"), &documents).unwrap();
    let second = direct(case("v096"), &documents).unwrap();
    independent(original, &first);
    independent(original, &second);
    independent(&first, &second);
    first["targets"][0]["associations"][0]["declared_native_reference"]["value"] = json!("changed");
    first["limitations"]
        .as_array_mut()
        .unwrap()
        .push(json!("changed"));
    first["declarations"]["unmatched"]
        .as_array_mut()
        .unwrap()
        .clear();
    assert_eq!(original, &before);
    assert_eq!(second, direct(case("v097"), &documents).unwrap());
    let failed = direct(case("v098"), &documents).unwrap();
    assert_eq!(failed["targets"], json!([]));
    assert!(failed["target_count"].is_null());
}

fn cli(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_coven"));
    command
        .env_clear()
        .env("PATH", "")
        .env("HOME", root)
        .current_dir(root)
        .args(["session", "list"])
        .arg(root)
        .arg("--socket")
        .arg(root.join("absent.sock"));
    command
}

#[test]
fn typed_api_and_actual_cli_parser_boundaries() {
    // Exact function-pointer types are checked by rustc. No integer/container/
    // bytes/object is coerced to &str or bool to fabricate a native API case.
    let _: fn(Option<&str>, Option<&str>) -> coven::Result<()> = validate_selection;
    let _: fn(&Value, Option<&str>, Option<&str>, bool) -> coven::Result<Value> = select_report;
    let (corpus, _) = fixtures();
    let temp = tempfile::tempdir().unwrap();
    let mut selectors = 0;
    let mut booleans = 0;
    for case in corpus["cases"].as_array().unwrap() {
        match case["native_route"].as_str().unwrap() {
            "typed_selector_exclusion" => {
                selectors += 1;
                assert!(
                    case["options"]
                        .as_object()
                        .unwrap()
                        .iter()
                        .any(
                            |(key, value)| ["group_key", "member_key"].contains(&key.as_str())
                                && !value.is_null()
                                && !value.is_string()
                        )
                );
                assert_eq!(case["expected"]["error"], "ValueError");
            }
            "typed_bool_and_cli_refusal" => {
                booleans += 1;
                let value = &case["options"]["recognized_only"];
                assert!(!value.is_boolean());
                assert_eq!(case["expected"]["error"], "ValueError");
                // These are explicit CLI value spellings, not Python objects
                // presented as if the Rust API accepted dynamically typed values.
                let spelling = value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string());
                for flags in [
                    vec![format!("--recognized-only={spelling}")],
                    vec!["--recognized-only".into(), spelling],
                ] {
                    let output = cli(temp.path()).args(flags).output().unwrap();
                    assert_eq!(output.status.code(), Some(2), "{}", case["id"]);
                    let error = String::from_utf8(output.stderr).unwrap();
                    assert!(
                        error.contains("unexpected value") || error.contains("unexpected argument"),
                        "{error}"
                    );
                }
            }
            "direct" => {}
            other => panic!("unaccounted route: {other}"),
        }
    }
    assert_eq!((selectors, booleans), (10, 10));
    for flag in ["--group", "--member"] {
        let output = cli(temp.path()).arg(flag).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains("a value is required")
                || error.contains("one of the values isn't valid"),
            "{error}"
        );
    }
    for flags in [vec!["--help"], vec!["--recognized-only", "--help"]] {
        let output = cli(temp.path()).args(flags).output().unwrap();
        assert!(output.status.success());
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("--recognized-only")
        );
    }
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn local_filesystem_no_effects_is_not_clock_io_proof() {
    let (corpus, documents) = fixtures();
    let temp = tempfile::tempdir().unwrap();
    let sentinel = temp.path().join("sentinel");
    fs::write(&sentinel, b"preserve exact bytes").unwrap();
    let before = fs::metadata(&sentinel).unwrap().modified().unwrap();
    for id in ["v126", "v127"] {
        let case = corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"] == id)
            .unwrap();
        let result = direct(case, &documents).unwrap();
        if id == "v127" {
            assert_eq!(result["target_count"], 1);
        }
    }
    assert_eq!(fs::read(&sentinel).unwrap(), b"preserve exact bytes");
    assert_eq!(fs::metadata(&sentinel).unwrap().modified().unwrap(), before);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    // No clock/socket/process poison seam exists in this API. This local check
    // and source review do not resolve the stronger original purity obligation.
}
