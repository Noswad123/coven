use crate::canonical::digest;
use serde_json::Value as Json;

const CASES: [&str; 5] = ["extensions", "empty", "whitespace", "fresh", "ledger"];

// Only outer metadata is decoded. Full source stdout remains opaque bytes.
pub(super) fn expected(id: &str, raw: &[u8], ids: Option<(&str, &str)>) -> Vec<u8> {
    let bytes = include_bytes!("../../../../tests/retirement/fixtures/execution_source.json");
    let provenance =
        include_bytes!("../../../../tests/retirement/fixtures/execution_source_provenance.json");
    assert_eq!(
        digest(bytes),
        "e88cbccbfde9d7ed833fb18b74c3d48aa707185ff8f6e83a5eeb8e978a4cba43"
    );
    assert_eq!(
        digest(provenance),
        "992b3f6dc52c0d20013de05e7a832ae5bc05fd12fb11f3b62ac635f5f18e0b11"
    );
    let data: Json = serde_json::from_slice(bytes).unwrap();
    let proof: Json = serde_json::from_slice(provenance).unwrap();
    assert_eq!(data["schema_version"], 1);
    assert_eq!(
        proof["historical_commit"],
        "ca3b19d87c955d6415231b1c4ebbbc591195a105"
    );
    assert_eq!(proof["corpus_sha256"], digest(bytes));
    let pins = proof["source_sha256"].as_object().unwrap();
    assert_eq!(pins.len(), 37);
    assert_eq!(
        pins.keys()
            .filter(|p| p.starts_with("runner/") && p.ends_with(".py"))
            .count(),
        25
    );
    for path in [
        "src/execution.rs",
        "src/execution/source.rs",
        "src/execution/source/tests.rs",
        "src/execution/source/oracle.py",
    ] {
        assert_eq!(pins[path].as_str().unwrap().len(), 64);
    }
    let cases = data["cases"].as_array().unwrap();
    assert_eq!(cases.len(), CASES.len());
    for i in 0..CASES.len() {
        let case = &cases[i];
        assert_eq!(case["id"], CASES[i]);
        assert_eq!(data["case_ids"][i], CASES[i]);
        assert_eq!(proof["case_ids"][i], CASES[i]);
        let stdout = case["stdout"].as_str().unwrap().as_bytes();
        assert_eq!(digest(stdout), case["stdout_sha256"]);
        assert_eq!(stdout.len() as u64, case["stdout_bytes"].as_u64().unwrap());
        assert_eq!(
            digest(case["tasks_raw"].as_str().unwrap().as_bytes()),
            case["tasks_raw_sha256"]
        );
        assert_eq!(case["exit_status"], 0);
        assert_eq!(case["stderr"], "");
        if i != 4 {
            assert!(case["slots"].as_array().unwrap().is_empty());
        }
    }
    let ledger = &cases[4];
    let template = ledger["template"].as_str().unwrap().as_bytes();
    let slots = ledger["slots"].as_array().unwrap();
    let inventory = [
        (25, "epoch_id", "\"epoch_id\":\"", "\""),
        (126, "workspace_id", "\"workspace_id\":\"", "\""),
        (189, "workspace_id", "\"revision\":\"tasks-ledger:v1:", ":"),
        (222, "epoch_id", ":", ":"),
    ];
    assert_eq!(slots.len(), inventory.len());
    let mut restored = template.to_vec();
    let mut bound = template.to_vec();
    for i in 0..inventory.len() {
        let (offset, binding, before, after) = inventory[i];
        assert_eq!(slots[i]["offset"], offset);
        assert_eq!(slots[i]["binding"], binding);
        assert_eq!(slots[i]["before"], before);
        assert_eq!(slots[i]["after"], after);
        assert_eq!(&template[offset - before.len()..offset], before.as_bytes());
        assert_eq!(
            &template[offset + 32..offset + 32 + after.len()],
            after.as_bytes()
        );
        assert_eq!(
            &template[offset..offset + 32],
            b"00000000000000000000000000000000"
        );
        let captured = ledger["captured_ids"][binding].as_str().unwrap();
        let selected = match ids {
            Some((workspace, epoch)) => {
                if binding == "workspace_id" {
                    workspace
                } else {
                    epoch
                }
            }
            None => captured,
        };
        for value in [captured, selected] {
            assert_eq!(value.len(), 32);
            assert!(
                value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            );
        }
        restored[offset..offset + 32].copy_from_slice(captured.as_bytes());
        bound[offset..offset + 32].copy_from_slice(selected.as_bytes());
    }
    assert_eq!(restored, ledger["stdout"].as_str().unwrap().as_bytes());
    assert_eq!(digest(&restored), ledger["stdout_sha256"]);
    assert_eq!(ledger["roundtrip_equal"], true);
    let index = CASES.iter().position(|case| *case == id).unwrap();
    assert_eq!(cases[index]["tasks_raw"].as_str().unwrap().as_bytes(), raw);
    assert_eq!(ids.is_some(), index == 4);
    if index == 4 {
        bound
    } else {
        cases[index]["stdout"].as_str().unwrap().as_bytes().to_vec()
    }
}
