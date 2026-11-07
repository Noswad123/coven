use coven::{archive, handoff};
use serde_json::{Value, json};
use std::fs;

fn value() -> Value {
    json!({"schema_version":1,"contract":"coven.worker-handoff.v1",
        "task":{"id":"task","owner":"worker","revision":format!("tasks-sha256:{}","0".repeat(64))},
        "claimed_status":"completed","summary":"A claim, not accepted work","confidence":"medium",
        "findings":[],"files_inspected":[],"files_changed":[],"checks":[],"blockers":[],"risks":[],
        "follow_ups":[],"references":[],"source_claim":null})
}

#[test]
#[cfg(unix)]
fn typed_claims_do_not_become_verification() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("worker.json");
    fs::write(&input, serde_json::to_vec(&value()).unwrap()).unwrap();
    let report = serde_json::to_value(archive::validate_file(&input).unwrap()).unwrap();
    assert_eq!(report["assessment"], "valid_unverified");
    assert_eq!(report["handoff"]["claimed_status"], "completed");
    assert_eq!(report["authorization"], "none");
    assert_eq!(report["dispatchable"], false);
    assert_eq!(report["verification"]["task_completion"], "unestablished");
}

#[test]
fn nullable_fields_are_required_and_versions_are_literal_integers() {
    let mut input = value();
    input.as_object_mut().unwrap().remove("source_claim");
    assert!(handoff::decode(&serde_json::to_vec(&input).unwrap()).is_err());
    for version in [json!(true), json!(1.0), json!("1"), json!(2), Value::Null] {
        let mut input = value();
        input["schema_version"] = version;
        assert!(handoff::decode(&serde_json::to_vec(&input).unwrap()).is_err());
    }
}

#[test]
fn no_normalization_of_persisted_identifiers_or_revisions() {
    assert!(handoff::Identifier::parse("Exact_ID").is_ok());
    assert!(handoff::Identifier::parse(" Exact_ID").is_err());
    assert!(
        handoff::TaskRevision::parse(&format!(
            "tasks-ledger:v1:{}:{}:{}:{}",
            "a".repeat(32),
            "b".repeat(32),
            "9".repeat(100),
            "c".repeat(64)
        ))
        .is_ok()
    );
    assert!(
        handoff::TaskRevision::parse(&format!(
            "tasks-ledger:v1:{}:{}:01:{}",
            "a".repeat(32),
            "b".repeat(32),
            "c".repeat(64)
        ))
        .is_err()
    );
}

#[test]
fn strict_decoder_rejects_duplicate_keys_invalid_unicode_and_overflow() {
    for raw in [
        b"{\"a\":1,\"a\":2}".as_slice(),
        b"{\"x\":1e999}",
        b"{\"x\":\"\\ud800\"}",
        b"\xff",
        b"[] x",
    ] {
        assert!(handoff::decode(raw).is_err());
    }
}

#[test]
fn utf8_byte_limits_not_character_counts() {
    for (text, accepted) in [
        ("\u{1f98a}".repeat(4096), true),
        ("\u{1f98a}".repeat(4097), false),
    ] {
        let mut input = value();
        input["summary"] = json!(text);
        assert_eq!(
            handoff::decode(&serde_json::to_vec(&input).unwrap()).is_ok(),
            accepted
        );
    }
}

#[test]
#[cfg(unix)]
fn existing_workspace_with_no_archive_is_not_modified() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("coven.json"),
        "not parsed by historical reads",
    )
    .unwrap();
    let report = serde_json::to_value(archive::list(temp.path(), None).unwrap()).unwrap();
    assert_eq!(report["observed_collection_count"], 0);
    assert!(!temp.path().join("output").exists());
    assert!(!temp.path().join("state").exists());
}

#[test]
fn human_output_escapes_bidi_non_bmp_and_controls() {
    let output = archive::output(&json!({"claim":"\u{202e}\u{1f98a}\u{7f}"}), false).unwrap();
    assert!(output.is_ascii());
    assert!(output.contains("\\u202e"));
    assert!(output.contains("\\ud83e\\udd8a"));
    assert!(output.contains("\\u007f"));
}

#[cfg(unix)]
#[test]
fn explicit_leaf_and_managed_ancestor_symlinks_are_rejected() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let raw = temp.path().join("raw.json");
    fs::write(&raw, serde_json::to_vec(&value()).unwrap()).unwrap();
    symlink(&raw, temp.path().join("alias.json")).unwrap();
    assert!(archive::validate_file(&temp.path().join("alias.json")).is_err());
    fs::write(temp.path().join("coven.json"), "marker").unwrap();
    let external = tempfile::tempdir().unwrap();
    symlink(external.path(), temp.path().join("output")).unwrap();
    assert!(archive::list(temp.path(), None).is_err());
}
