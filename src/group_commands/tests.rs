use super::*;

fn binding_fixture(process: bool, native: bool) -> serde_json::Value {
    let mut value = serde_json::json!({
        "presentation": {"adapter":"kitsune", "endpoint":{"path":"/fixture/api.sock",
            "device":u64::MAX,"inode":42}, "terminal_id":"term", "pane_id_at_binding":"pane"},
        "evidence": {"provenance":"kitsune.session.snapshot", "observed_at":"2026-11-05T00:00:00+00:00",
            "agent_kind_hint":"djinn", "native_reference":null, "native_binding":"unestablished"}
    });
    if native {
        value["evidence"]["native_reference"] = serde_json::json!({
            "source":"native", "agent":"djinn", "kind":"id", "value":"session-\u{e9}"
        });
        value["evidence"]["native_binding"] = serde_json::json!("observed_reference");
    }
    if process {
        value["evidence"]["provenance"] = serde_json::json!("kitsune.pane.process_info");
        value["evidence"]["process_evidence"] = serde_json::json!({
            "interpreter":"coven.kitsune.djinn-process.v1", "matches":[
                {"pid":7,"executable_family":"djinn","rule":"exact_posix_argv0_basename"},
                {"pid":9,"executable_family":"djinn-ui","rule":"exact_posix_argv0_basename"}
            ]
        });
    }
    value
}

fn raw_storage_oracle(process: bool, native: bool) {
    use sha2::{Digest, Sha256};
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("coven.json"), "not team JSON").unwrap();
    std::fs::create_dir(root.path().join("state")).unwrap();
    groups::create_group(root.path(), "g", "Group", None).unwrap();
    let path = root.path().join("state/groups.json");
    let seed = std::fs::read_to_string(&path).unwrap();
    let produced = binding_fixture(process, native);
    let binding = observation::fresh_binding(&produced).unwrap();
    let result = groups::add_member(
        root.path(),
        groups::AddMember {
            group: "g",
            member: "m",
            role: "Role",
            binding: &binding,
            rebind: false,
            expected_revision: None,
        },
    )
    .unwrap();
    let member = field(&result, "member").unwrap();
    let participant = field(member, "participant_id").unwrap().as_str().unwrap();
    let mut child = Command::new("python3")
        .arg("-B")
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/group_commands/oracle.py"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            &serde_json::to_vec(&serde_json::json!({
                "seed":seed,"produced":produced,"participant":participant,"process":process,
            }))
            .unwrap(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let raw = std::fs::read(path).unwrap();
    assert_eq!(raw, expected["raw"].as_str().unwrap().as_bytes());
    assert_eq!(
        format!("{:x}", Sha256::digest(&raw)),
        expected["sha256"].as_str().unwrap()
    );
    assert_eq!(
        field(&result, "revision").unwrap().as_str().unwrap(),
        expected["revision"].as_str().unwrap()
    );
    assert_eq!(
        binding.compact().unwrap(),
        expected["binding"].as_str().unwrap().as_bytes()
    );
}

#[test]
fn fresh_snapshot_null_reference_matches_python_raw_storage_and_hash() {
    raw_storage_oracle(false, false);
}

#[test]
fn fresh_snapshot_full_reference_matches_python_raw_storage_and_hash() {
    raw_storage_oracle(false, true);
}

#[test]
fn fresh_process_binding_matches_python_raw_storage_and_hash() {
    raw_storage_oracle(true, false);
}

#[test]
fn fresh_binding_rejects_unknown_fields_before_reconstruction() {
    for (process, native) in [(false, false), (false, true), (true, false)] {
        let fixture = binding_fixture(process, native);
        for pointer in ["", "/presentation", "/presentation/endpoint", "/evidence"] {
            let mut invalid = fixture.clone();
            invalid.pointer_mut(pointer).unwrap()["unexpected"] = serde_json::json!(true);
            assert!(observation::fresh_binding(&invalid).is_err(), "{pointer}");
        }
        for pointer in if process {
            vec![
                "/evidence/process_evidence",
                "/evidence/process_evidence/matches/0",
            ]
        } else if native {
            vec!["/evidence/native_reference"]
        } else {
            vec![]
        } {
            let mut invalid = fixture.clone();
            invalid.pointer_mut(pointer).unwrap()["unexpected"] = serde_json::json!(true);
            assert!(observation::fresh_binding(&invalid).is_err(), "{pointer}");
        }
    }
}

#[test]
fn native_render_keeps_large_integers_ascii_and_lf_ownership() {
    let raw = r#"{"device":184467440737095516160000000000000000000,"name":"caf\u00e9"}"#;
    let value = values::parse(raw.as_bytes(), values::Profile::Authority).unwrap();
    for json in [true, false] {
        let result = render(&value, json, 2).unwrap();
        assert_eq!(result.exit_code, 2);
        assert!(result.body.is_ascii());
        assert!(!result.body.ends_with('\n'));
        assert!(
            result
                .body
                .contains("184467440737095516160000000000000000000")
        );
        assert!(result.body.contains("caf\\u00e9"));
        assert_eq!(result.body.contains('\n'), !json);
    }
}

#[test]
fn mutation_exit_code_uses_nested_delivery_only() {
    for (raw, code) in [
        (
            r#"{"outcome":"committed","delivery":{"outcome":"durability_uncertain"}}"#,
            2,
        ),
        (
            r#"{"outcome":"unchanged","delivery":{"outcome":"durable"}}"#,
            0,
        ),
        (r#"{"outcome":"durability_uncertain"}"#, 0),
    ] {
        let value = values::parse(raw.as_bytes(), values::Profile::Authority).unwrap();
        assert_eq!(mutation(value, true).unwrap().exit_code, code);
    }
}
