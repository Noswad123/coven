//! Pinned historical inputs and complete outputs; authority is never serde-narrowed.
use super::*;
use serde_json::Value as Json;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

const SECTIONS: [(&str, usize); 8] = [
    ("pure-selector", 10),
    ("pure-display", 13),
    ("pure-source", 7),
    ("pure-attachment", 8),
    ("schema", 12),
    ("timestamp", 102),
    ("authority", 8),
    ("mutation", 18),
];

fn digest(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}

fn keys(value: &Json, expected: &[&str]) {
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        expected
    );
}

fn cases() -> &'static [Json] {
    static CASES: OnceLock<Vec<Json>> = OnceLock::new();
    CASES.get_or_init(|| {
        let raw = include_bytes!("../../../tests/retirement/fixtures/source_groups.json");
        let proof =
            include_bytes!("../../../tests/retirement/fixtures/source_groups_provenance.json");
        assert_eq!(
            digest(raw),
            "c8a4d7958ee332c457727d7de742e1e3a69c4f2c4c7a579f50968d8ec2ea4098"
        );
        assert_eq!(
            digest(proof),
            "a87f9623e99c7e36f06e0249ca63460355be5947f80537b3c1576941ffe3e95e"
        );
        let data: Json = serde_json::from_slice(raw).unwrap();
        let proof: Json = serde_json::from_slice(proof).unwrap();
        keys(&data, &["cases", "schema_version"]);
        assert_eq!(data["schema_version"], 1);
        keys(
            &proof,
            &[
                "capture_tool_sha256",
                "case_count",
                "corpus_sha256",
                "counts",
                "environment",
                "historical_commit",
                "input_strategy",
                "interpreter",
                "repeat_capture_equal",
                "result_strategy",
                "schema_version",
                "source_sha256",
            ],
        );
        assert_eq!(proof["schema_version"], 1);
        assert_eq!(
            proof["historical_commit"],
            "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
        );
        assert_eq!(proof["corpus_sha256"], digest(raw));
        assert_eq!(proof["case_count"], 178);
        assert_eq!(proof["repeat_capture_equal"], true);
        assert_eq!(
            proof["counts"],
            serde_json::json!({"pure":38,"schema":12,"timestamp":102,"authority":8,"mutation":18})
        );
        let pins = proof["source_sha256"].as_object().unwrap();
        assert_eq!(pins.len(), 63);
        assert_eq!(pins.keys().filter(|p| p.starts_with("runner/")).count(), 25);
        for path in [
            "src/groups/oracle.py",
            "src/groups/tests.rs",
            "src/groups.rs",
            "src/groups/schema.rs",
            "src/groups/store.rs",
            "src/groups/timestamp.rs",
            "src/storage.rs",
            "src/values/encode.rs",
            "src/values/parser.rs",
        ] {
            assert_eq!(pins[path].as_str().unwrap().len(), 64);
        }
        let rows = data["cases"].as_array().unwrap();
        assert_eq!(rows.len(), 178);
        let mut at = 0;
        for (section, count) in SECTIONS {
            for index in 0..count {
                let row = &rows[at];
                keys(
                    row,
                    &[
                        "id",
                        "request_raw",
                        "request_sha256",
                        "result_raw",
                        "result_sha256",
                    ],
                );
                assert_eq!(row["id"], format!("{section}-{index:03}"));
                for name in ["request", "result"] {
                    let raw = row[format!("{name}_raw")].as_str().unwrap();
                    assert_eq!(row[format!("{name}_sha256")], digest(raw.as_bytes()));
                    parse(raw);
                }
                let request = parse(row["request_raw"].as_str().unwrap());
                let result = parse(row["result_raw"].as_str().unwrap());
                if section == "mutation" {
                    assert_eq!(array(get(&request, "ids").unwrap()).unwrap().len(), 8);
                    assert!(matches!(
                        get(&result, "accepted").unwrap(),
                        Value::Bool(true)
                    ));
                    get(&result, "result").unwrap();
                    text(get(&result, "raw").unwrap()).unwrap();
                } else {
                    assert_eq!(array(get(&request, "values").unwrap()).unwrap().len(), 1);
                    assert_eq!(array(&result).unwrap().len(), 1);
                }
                at += 1;
            }
        }
        assert_eq!(at, rows.len());
        rows.clone()
    })
}

pub(super) fn case(id: &str, request: Json) -> Value {
    let row = cases().iter().find(|r| r["id"] == id).unwrap();
    let expected_request = parse(row["request_raw"].as_str().unwrap());
    let actual_request =
        values::parse(&serde_json::to_vec(&request).unwrap(), Profile::Authority).unwrap();
    assert!(
        equal(&actual_request, &expected_request).unwrap(),
        "full input {id}"
    );
    parse(row["result_raw"].as_str().unwrap())
}

pub(super) fn batch(section: &str, request: Json) -> Value {
    let count = SECTIONS
        .iter()
        .find(|(name, _)| *name == section)
        .unwrap()
        .1;
    assert_ne!(section, "mutation");
    let inputs = request["values"].as_array().unwrap();
    assert_eq!(inputs.len(), count, "complete input inventory {section}");
    let mut results = Vec::with_capacity(count);
    for (index, input) in inputs.iter().enumerate() {
        let mut single = request.clone();
        single["values"] = serde_json::json!([input]);
        let result = case(&format!("{section}-{index:03}"), single);
        let output = array(&result).unwrap();
        assert_eq!(output.len(), 1);
        results.push(output[0].clone());
    }
    Value::Array(results)
}

pub(super) fn comparison_setup(root: &Path) {
    // Approved setup-only UUID change: default Io still publishes and confirms real bytes.
    let io = Fixed::new();
    mutate(root, "g", None, Operation::Create("Group"), &io).unwrap();
    let b = binding();
    mutate(
        root,
        "g",
        None,
        Operation::Add(AddMember {
            group: "g",
            member: "m",
            role: "Role",
            binding: &b,
            rebind: false,
            expected_revision: None,
        }),
        &io,
    )
    .unwrap();
    assert_eq!(io.0.load(Ordering::SeqCst), 4);
}

pub(super) fn validation_result(actual: Result<Value>, expected: &Value) {
    let actual = match actual {
        Ok(state) => obj(&[
            ("accepted", Value::Bool(true)),
            ("revision", string(revision(Some(&state)).unwrap())),
        ])
        .unwrap(),
        Err(_) => obj(&[("accepted", Value::Bool(false))]).unwrap(),
    };
    assert!(
        equal(&actual, expected).unwrap(),
        "complete validation result"
    );
}
