use super::*;
use sha2::{Digest, Sha256};

pub(super) fn approved(id: &str) -> String {
    assert!(matches!(id, "yes" | "test"));
    let corpus = include_bytes!("../../../tests/retirement/fixtures/source_actions.json");
    let proof_raw =
        include_bytes!("../../../tests/retirement/fixtures/source_actions_provenance.json");
    assert_eq!(
        format!("{:x}", Sha256::digest(corpus)),
        "def8a98f85c36dda24c82e2797ec5ccdcc006d440b94534b84cde7e69f5ff882"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(proof_raw)),
        "fad747f54c59dc304d4c36766c6f495115e11d03f1a11d834e302aebf92e5dbc"
    );
    let proof = parse(include_str!(
        "../../../tests/retirement/fixtures/source_actions_provenance.json"
    ));
    let get = |v: &Value, key: &str| v.as_object().unwrap().get(key).unwrap().clone();
    let hash = |raw: &[u8]| format!("{:x}", Sha256::digest(raw));
    assert_eq!(
        get(&proof, "historical_commit").as_str().unwrap(),
        "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
    );
    assert_eq!(get(&proof, "corpus_sha256").as_str().unwrap(), hash(corpus));
    assert_eq!(
        get(&proof, "capture_tool_sha256").as_str().unwrap(),
        "e1077b1155d799c4d6d3107c0a78e78db4ad13748895ce24c34fa135c852bc37"
    );
    let pins = get(&proof, "source_sha256");
    assert_eq!(pins.as_object().unwrap().entries().len(), 47);
    assert_eq!(
        get(&pins, "src/checkpoint_guidance/tests.rs")
            .as_str()
            .unwrap(),
        "258b429f79724f300ee5a25227701dbcf81f7da0bac5ef10d1a95d71dc9b6351"
    );
    let data = values::parse(corpus, Profile::Authority).unwrap();
    assert_eq!(
        get(&data, "case_ids").canonical().unwrap(),
        get(&proof, "case_ids").canonical().unwrap()
    );
    let cases = get(&data, "cases");
    assert_eq!(cases.as_array().unwrap().len(), 11);
    let wanted = format!("guidance_{id}");
    let entry = cases
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| get(entry, "id").as_str() == Some(wanted.as_str()))
        .unwrap();
    let stdout = get(entry, "stdout").as_str().unwrap().to_owned();
    assert_eq!(
        get(entry, "stdout_sha256").as_str().unwrap(),
        hash(stdout.as_bytes())
    );
    assert_eq!(
        get(entry, "stdout_bytes").canonical().unwrap(),
        stdout.len().to_string().as_bytes()
    );
    assert_eq!(get(entry, "input_sha256").as_str().unwrap(), hash(b""));
    assert_eq!(get(entry, "exit_status").canonical().unwrap(), b"0");
    assert_eq!(get(entry, "stderr").as_str().unwrap(), "");
    assert!(stdout.contains(&format!("\"checkpoint\":\"{id}\"")));
    assert_eq!(stdout.matches("\"decided_at\":\"test\"").count(), 2);
    assert_eq!(
        stdout
            .matches("\"decision_id\":\"11111111111111111111111111111111\"")
            .count(),
        2
    );
    assert_eq!(stdout.matches("\"source\":\"cli\"").count(), 2);
    stdout
}
