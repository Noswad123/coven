//! Historical Python inputs/outputs, never native-generated expectations.
use super::*;
use serde_json::Value as Json;

const IDS: &[&[&str]] = &[
    &["standalone-v1", "standalone-v2"],
    &[
        "fresh-two-node-downstream-first",
        "proposed-managed-task-cannot-use-standalone",
        "wrong-digest-rejected",
        "approve-unrelated-revision-change",
        "downstream-does-not-dispatch-or-wait",
        "independent-upstream-reservation",
        "source-free-revocation-preserves-reservations",
        "revoked-review-rejects-prepare",
        "prepared-blocks-reapproval",
        "prepared-blocks-successor-even-removed-node",
        "successor-preserves-family-and-head",
        "removed-node-sticky-managed-cannot-use-standalone",
        "old-head-rejected",
        "source-free-review-still-requires-current-head",
        "successor-needs-fresh-review",
        "approve-successor",
        "removed-node-remains-unavailable",
        "selected-int-float-drift-rejected",
        "missing-owner-rejected",
        "authority-change-rejected",
        "rebound-enrollment-invalidates-old-graph",
        "successor-freezes-current-ledger-and-binding",
        "approve-rebound-successor",
        "graph-bound-attempt-chain-after-rebind",
        "ledger-source-free-revocation",
    ],
    &[
        "v1-enable",
        "v1-enroll",
        "v1-reenroll",
        "v1-rebind",
        "v1-rebind_denied",
        "v1-edited",
        "v1-missing_enrollment",
        "v1-missing_task",
        "v1-owner",
        "v1-roster",
        "v1-source_changed",
        "v1-chain",
        "v2-enroll",
        "v2-reenroll",
        "v2-rebind",
        "v2-rebind_denied",
        "v2-edited",
        "v2-missing_enrollment",
        "v2-missing_task",
        "v2-owner",
        "v2-roster",
        "v2-source_changed",
        "v2-chain",
    ],
];

pub(in crate::execution::mutation) fn vectors(group: usize) -> Value {
    let raw = include_bytes!("../../../../tests/retirement/fixtures/source_mutation_effects.json");
    let provenance = include_bytes!(
        "../../../../tests/retirement/fixtures/source_mutation_effects_provenance.json"
    );
    assert_eq!(
        canonical::digest(raw),
        "7b574a0087903c171a8d18db5ff5e987d9c0b18066078bd8359ddfb0ae20025c"
    );
    assert_eq!(
        canonical::digest(provenance),
        "1d6f5fc9392f489c74f7204f5de0c01a46331c9643aaf73d4ddd6b8b010528bf"
    );
    // serde sees only wrappers and strings, never controls with huge integers.
    let corpus: Json = serde_json::from_slice(raw).unwrap();
    let proof: Json = serde_json::from_slice(provenance).unwrap();
    assert_eq!(corpus["schema_version"], 1);
    assert_eq!(proof["schema_version"], 1);
    assert_eq!(
        proof["historical_commit"],
        "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
    );
    assert_eq!(proof["corpus_sha256"], canonical::digest(raw));
    assert_eq!(
        proof["capture_tool_sha256"],
        "e9f71383db0bebd1edc98bf3f378e3eff5dc44264bb24c61148a1eb39cc2fc9b"
    );
    let pins = proof["source_sha256"].as_object().unwrap();
    assert_eq!(pins.len(), 46);
    assert_eq!(
        pins.keys()
            .filter(|p| p.starts_with("runner/") && p.ends_with(".py"))
            .count(),
        25
    );
    for path in [
        "src/execution.rs",
        "src/execution/mutation.rs",
        "src/execution/mutation/tests.rs",
        "src/execution/mutation/oracle.py",
        "src/execution/mutation/graphs.rs",
        "src/execution/mutation/graphs/tests.rs",
        "src/execution/mutation/graphs/oracle.py",
        "src/execution/mutation/effects.rs",
        "src/execution/mutation/effects/tests.rs",
        "src/execution/mutation/effects/oracle.py",
    ] {
        assert_eq!(pins[path].as_str().unwrap().len(), 64);
    }
    let captures = corpus["captures"].as_array().unwrap();
    assert_eq!(captures.len(), 3);
    assert_eq!(proof["counts"], serde_json::json!([2, 25, 23]));
    let mut selected = Vec::new();
    for (index, capture) in captures.iter().enumerate() {
        let ids = IDS[index];
        let rows = capture["rows"].as_array().unwrap();
        assert_eq!(rows.len(), ids.len());
        assert_eq!(capture["count"], ids.len());
        assert_eq!(capture["exit_status"], 0);
        assert_eq!(capture["stderr"], "");
        assert_eq!(capture["repeated_identically"], true);
        let stdout = capture["stdout"].as_str().unwrap();
        assert_eq!(
            canonical::digest(stdout.as_bytes()),
            capture["stdout_sha256"]
        );
        assert_eq!(
            stdout.len(),
            capture["stdout_bytes"].as_u64().unwrap() as usize
        );
        let mut reconstructed = Vec::new();
        for (row_index, row) in rows.iter().enumerate() {
            assert_eq!(row["id"], ids[row_index]);
            assert_eq!(proof["case_ids"][index][row_index], ids[row_index]);
            let raw = row["raw"].as_str().unwrap();
            let request = row["request"].as_str().unwrap();
            assert_eq!(canonical::digest(raw.as_bytes()), row["sha256"]);
            assert_eq!(canonical::digest(request.as_bytes()), row["request_sha256"]);
            reconstructed.push(raw);
            if index == group {
                let vector = values::parse(raw.as_bytes(), Profile::Authority).unwrap();
                let request = values::parse(request.as_bytes(), Profile::Authority).unwrap();
                for (key, value) in request.as_object().unwrap().entries() {
                    assert_eq!(
                        get(&vector, key).unwrap().canonical().unwrap(),
                        value.canonical().unwrap()
                    );
                }
                selected.push(vector);
            }
        }
        assert_eq!(format!("[{}]\n", reconstructed.join(", ")), stdout);
    }
    assert_eq!(selected.len(), IDS[group].len());
    Value::Array(selected)
}
