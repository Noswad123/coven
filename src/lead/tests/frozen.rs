use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

const CORPUS: &[u8] = include_bytes!("../../../tests/retirement/fixtures/source_lead_init.json");
const PROOF: &[u8] =
    include_bytes!("../../../tests/retirement/fixtures/source_lead_init_provenance.json");
const IDS: [&str; 4] = ["set-default", "set-empty", "set-custom", "clear"];
const PATHS: [&str; 4] = [
    "state/lead.json",
    "logs/events.jsonl",
    "logs/messages.jsonl",
    "dashboard.md",
];

fn digest(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}

fn keys(value: &Value, expected: &[&str]) {
    let mut actual: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut expected = expected.to_vec();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

pub(super) fn expected(index: usize, root: &Path, message: Option<&str>) -> (String, Vec<Vec<u8>>) {
    assert_eq!(
        digest(CORPUS),
        "77deb8d7d7b6d40154d4618a45faad21defd621c06cf7400531d8cd30c9f3eab"
    );
    assert_eq!(
        digest(PROOF),
        "456db89e702df84b24f0ba98014fff88663b72e6b3acdfd088614be2d55a54d5"
    );
    let corpus: Value = serde_json::from_slice(CORPUS).unwrap();
    let proof: Value = serde_json::from_slice(PROOF).unwrap();
    keys(
        &proof,
        &[
            "schema_version",
            "historical_commit",
            "source_sha256",
            "capture_tool_sha256",
            "corpus_sha256",
            "interpreter",
            "environment",
            "case_count",
            "lead_capture_count",
            "helper_capture_count",
            "repeat_capture_equal",
            "inline_program_sha256",
            "retired_helper",
            "disposition",
            "historical_probe",
            "binding",
            "counts",
        ],
    );
    assert_eq!(proof["case_count"], 4);
    assert_eq!(proof["lead_capture_count"], 8);
    assert_eq!(proof["helper_capture_count"], 2);
    assert_eq!(proof["repeat_capture_equal"], true);
    assert_eq!(
        proof["source_sha256"]
            .as_object()
            .unwrap()
            .keys()
            .filter(|p| p.starts_with("runner/"))
            .count(),
        25
    );
    assert_eq!(proof["corpus_sha256"], digest(CORPUS));
    assert_eq!(
        proof["capture_tool_sha256"],
        "2a2d1803ab940bde7acc463c86c40abcdf7d1897222139bd00d078227af200ef"
    );
    assert_eq!(
        proof["historical_commit"],
        "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
    );
    assert_eq!(
        proof["retired_helper"]["sha256"],
        proof["source_sha256"]["src/init/mixed_overlap.py"]
    );
    keys(&corpus, &["schema_version", "case_ids", "cases", "marker"]);
    assert_eq!(corpus["schema_version"], 1);
    assert_eq!(corpus["case_ids"], serde_json::json!(IDS));
    keys(
        &corpus["marker"],
        &["mode", "raw_utf8", "sha256", "stdout_utf8", "stdout_sha256"],
    );
    for (raw, hash) in [("raw_utf8", "sha256"), ("stdout_utf8", "stdout_sha256")] {
        assert_eq!(
            digest(corpus["marker"][raw].as_str().unwrap().as_bytes()),
            corpus["marker"][hash]
        );
    }
    let rows = corpus["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    for (i, row) in rows.iter().enumerate() {
        keys(
            row,
            &[
                "id",
                "message",
                "inputs",
                "input_utf8",
                "input_sha256",
                "captured_workspace",
                "stdout_utf8",
                "stdout_sha256",
                "body",
                "files",
                "file_sha256",
            ],
        );
        assert_eq!(row["id"], IDS[i]);
        keys(
            &row["inputs"],
            &["coven.json", "goal.md", "state/lead.json"],
        );
        let input = row["input_utf8"].as_str().unwrap();
        assert_eq!(digest(input.as_bytes()), row["input_sha256"]);
        let input: Value = serde_json::from_str(input).unwrap();
        keys(&input, &["files", "message", "operation"]);
        assert_eq!(input["files"], row["inputs"]);
        assert_eq!(input["message"], row["message"]);
        assert_eq!(input["operation"], if i == 3 { "clear" } else { "set" });
        let stdout = row["stdout_utf8"].as_str().unwrap();
        assert_eq!(digest(stdout.as_bytes()), row["stdout_sha256"]);
        let output: Value = serde_json::from_str(stdout).unwrap();
        keys(
            &output,
            if i == 3 {
                &["body", "files", "exists"]
            } else {
                &["body", "files"]
            },
        );
        assert_eq!(output["body"], row["body"]);
        let paths: &[&str] = if i == 3 {
            &[PATHS[1], PATHS[3]]
        } else {
            &PATHS
        };
        keys(&row["files"], paths);
        keys(&row["file_sha256"], paths);
        assert_eq!(output["files"].as_array().unwrap().len(), paths.len());
        for (j, path) in paths.iter().enumerate() {
            let raw = row["files"][path].as_str().unwrap();
            assert_eq!(digest(raw.as_bytes()), row["file_sha256"][path]);
            assert_eq!(output["files"][j], raw);
        }
        if i == 3 {
            assert_eq!(output["exists"], false);
        }
    }
    let row = &rows[index];
    assert_eq!(row["message"], serde_json::json!(message));
    for (path, raw) in row["inputs"].as_object().unwrap() {
        assert_eq!(
            fs::read(root.join(path)).unwrap(),
            raw.as_str().unwrap().as_bytes()
        );
    }
    let paths: &[&str] = if index == 3 {
        &[PATHS[1], PATHS[3]]
    } else {
        &PATHS
    };
    let files = paths
        .iter()
        .map(|path| {
            let raw = row["files"][path].as_str().unwrap();
            if index < 2 && *path == "logs/messages.jsonl" {
                // Bind just the expected prompt's input root, before the operation.
                let old = format!(
                    "Workspace: {}\\nGoal:",
                    row["captured_workspace"].as_str().unwrap()
                );
                assert_eq!(raw.matches(&old).count(), 1);
                let canonical_root = root.canonicalize().unwrap();
                let escaped = serde_json::to_string(canonical_root.to_str().unwrap()).unwrap();
                let new = format!("Workspace: {}\\nGoal:", &escaped[1..escaped.len() - 1]);
                raw.replacen(&old, &new, 1).into_bytes()
            } else {
                assert!(!raw.contains(row["captured_workspace"].as_str().unwrap()));
                raw.as_bytes().to_vec()
            }
        })
        .collect();
    (row["body"].as_str().unwrap().to_owned(), files)
}
