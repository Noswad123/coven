// Private E2 corpus reader. Raw JSON payloads remain strings, including large integers.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(super) fn hash(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}

fn files_hash(files: &Value) -> String {
    let mut bytes = Vec::new();
    let sorted: BTreeMap<_, _> = files.as_object().unwrap().iter().collect();
    for (path, raw) in sorted {
        bytes.extend_from_slice(path.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(raw.as_str().unwrap().as_bytes());
        bytes.push(0);
    }
    hash(&bytes)
}

pub(super) fn case(id: &str) -> Value {
    let bytes = include_bytes!("../../tests/retirement/fixtures/source_workspace.json");
    let provenance =
        include_bytes!("../../tests/retirement/fixtures/source_workspace_provenance.json");
    assert_eq!(
        hash(bytes),
        "994bcdae4ce998c4b8c4d3d70940fd393f3759ce0388f41613d5c218e2e411eb"
    );
    assert_eq!(
        hash(provenance),
        "5c30f68ed9f3b09b01fbb28e07686374ce04f23ead50356660c9ad59cafe3f70"
    );
    let proof: Value = serde_json::from_slice(provenance).unwrap();
    assert_eq!(hash(bytes), proof["corpus_sha256"].as_str().unwrap());
    assert_eq!(
        proof["historical_commit"],
        "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
    );
    assert_eq!(proof["runner_modules"], 25);
    let corpus: Value = serde_json::from_slice(bytes).unwrap();
    let rows = corpus["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 25);
    let mut ids = std::collections::BTreeSet::new();
    for row in rows {
        assert!(ids.insert(row["id"].as_str().unwrap()));
        assert_eq!(files_hash(&row["inputs"]), row["input_sha256"]);
        let stdout = row["stdout"].as_str().unwrap().as_bytes();
        assert_eq!(hash(stdout), row["stdout_sha256"]);
        assert_eq!(stdout.len() as u64, row["stdout_bytes"].as_u64().unwrap());
        if let Some(files) = row.get("artifacts") {
            assert_eq!(files_hash(files), row["artifacts_sha256"]);
        }
    }
    rows.iter().find(|row| row["id"] == id).unwrap().clone()
}
