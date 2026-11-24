//! Schema obligations only; collection/process and Python mutation APIs are excluded.
use super::*;
use serde_json::{Value, json};

fn claim() -> Value {
    json!({"schema_version":1,"contract":"coven.worker-handoff.v1",
        "task":{"id":"task","owner":"operator","revision":format!("tasks-sha256:{}", "0".repeat(64))},
        "claimed_status":"completed","summary":"Worker reports a result; not verified. \u{03bb}",
        "confidence":"medium","findings":[{"priority":"high","claim":"An observed behavior","evidence":["djinn://declared/session#message"]}],
        "files_inspected":["src/example.rs"],"files_changed":[],
        "checks":[{"name":"unit","claimed_outcome":"passed","details":"just test (worker report only)","evidence":[]}],
        "blockers":[],"risks":["No independent checks"],"follow_ups":[],
        "references":[{"kind":"artifact","uri":"file:///missing/never-followed"}],"source_claim":null})
}
fn validate(value: &Value) -> Result<ValidatedHandoff> {
    decode(&serde_json::to_vec(value).unwrap())
}

#[test]
fn schema_rejects_grants_status_authority_and_unknown_fields() {
    assert!(validate(&claim()).is_ok());
    for (key, value) in [
        ("authorization", json!("merge")),
        ("verified", json!(true)),
        ("status", json!("done")),
        ("attempt_id", json!("prepared-attempt")),
        ("native_accepted", json!(true)),
    ] {
        let mut claim = claim();
        claim[key] = value;
        assert!(validate(&claim).is_err(), "{key}");
    }
}

#[test]
fn source_claim_honestly_supports_uncommitted_workers() {
    let mut value = claim();
    value["files_changed"] = json!(["src/file"]);
    value["source_claim"] = json!({"state":"uncommitted","repository_id":null,"base_sha":"a".repeat(40),"source_sha":null,"tree_sha":null});
    assert_eq!(
        serde_json::to_value(validate(&value).unwrap()).unwrap(),
        value
    );
    value["source_claim"]["state"] = json!("committed");
    value["source_claim"]["source_sha"] = json!("b".repeat(40));
    value["source_claim"]["tree_sha"] = json!("c".repeat(40));
    assert_eq!(
        serde_json::to_value(validate(&value).unwrap()).unwrap(),
        value
    );
    value["source_claim"]["tree_sha"] = json!("d".repeat(64));
    assert!(validate(&value).is_err());
}

#[test]
fn strict_json_rejects_duplicate_keys_nonfinite_unicode_and_depth() {
    let nested = format!("{}0{}", "[".repeat(17), "]".repeat(17));
    for raw in [
        br#"{"x":1,"x":2}"#.as_slice(),
        br#"{"x":NaN}"#,
        br#"{"x":1e999}"#,
        b"\xff",
        b"{",
        nested.as_bytes(),
        br#"{"x":"\ud800"}"#,
    ] {
        assert!(crate::strict_json::decode(raw).is_err(), "{raw:?}");
    }
    // These are valid JSON but not handoffs: domain missing-field failures cannot
    // stand in for the JSON parser refusals above.
    for raw in [
        br#"{"x":1}"#.as_slice(),
        br#"{"x":1.5}"#,
        br#"{"x":"\ud83d\ude00"}"#,
        b"null",
    ] {
        assert!(crate::strict_json::decode(raw).is_ok(), "{raw:?}");
    }
    let boundary = format!("{}0{}", "[".repeat(16), "]".repeat(16));
    assert!(crate::strict_json::decode(boundary.as_bytes()).is_ok());
}
