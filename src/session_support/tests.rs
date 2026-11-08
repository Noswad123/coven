use super::*;
use crate::{kitsune_observation as kitsune, observation_socket};
use serde_json::json;
use std::fs;

#[test]
fn absolute_is_lexical_and_does_not_expand_tilde_or_require_existence() {
    let path = Path::new("~/missing-session-support/../child");
    assert_eq!(absolute(path).unwrap(), std::path::absolute(path).unwrap());
    assert!(absolute(path).unwrap().ends_with(path));
    assert_eq!(
        absolute(Path::new("")).unwrap(),
        std::env::current_dir().unwrap()
    );
}

#[test]
fn presentation_is_ascii_exact_and_has_no_terminal_newline() {
    let report = json!({"text":"\u{7f}\u{e9}\u{1f600}\n", "integer":u64::MAX});
    for pretty in [false, true] {
        let result = output(&report, pretty).unwrap();
        assert!(result.stdout.is_ascii());
        assert!(!result.stdout.ends_with('\n'));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result.stdout).unwrap(),
            report
        );
        assert!(result.warnings.is_empty());
        assert_eq!(result.exit_code, 0);
    }
}

#[test]
fn presentation_checks_serialized_and_ascii_expanded_boundaries() {
    let report = json!("x".repeat(crate::values::MAX_OUTPUT - 2));
    assert_eq!(
        output(&report, false).unwrap().stdout.len(),
        crate::values::MAX_OUTPUT
    );
    let report = json!("x".repeat(crate::values::MAX_OUTPUT - 1));
    assert_eq!(
        output(&report, false).err().unwrap().code(),
        "unsupported_json_output_profile"
    );
    let report = json!("\u{7f}".repeat(crate::values::MAX_OUTPUT / 6 + 1));
    assert_eq!(
        output(&report, true).err().unwrap().code(),
        "unsupported_json_output_profile"
    );
}

fn reply(snapshot: &str) -> observation_socket::Reply {
    observation_socket::Reply {
        payload: format!(
            r#"{{"id":"test","result":{{"type":"session_snapshot","snapshot":{snapshot}}}}}"#
        )
        .into_bytes(),
        request_id: "test".into(),
        endpoint: observation_socket::Endpoint {
            selected: "/unused".into(),
            resolved: "/unused".into(),
            device: 1,
            inode: 2,
        },
    }
}

#[test]
fn command_capability_is_exact_and_extensions_are_not_serde_narrowed() {
    for version in ["1", "1.0", "1e0", "true", "2", "1.00000000000000001"] {
        let r = reply(&format!(
            r#"{{"version":"test","protocol":18,"panes":[],"command_submission":{{"version":{version}}},"extension":18446744073709551616}}"#
        ));
        assert_eq!(
            kitsune::session_snapshot(&r).unwrap(),
            json!({"version":"test","protocol":18,"panes":[]})
        );
        if version == "1" {
            assert!(kitsune::command_snapshot(&r).is_ok());
        } else {
            assert_eq!(
                kitsune::command_snapshot(&r).unwrap_err().code,
                "command_submission_unsupported"
            );
        }
    }
    let missing = reply(r#"{"version":"test","protocol":18,"panes":[]}"#);
    assert_eq!(
        kitsune::command_snapshot(&missing).unwrap_err().code,
        "command_submission_unsupported"
    );
    let invalid = reply(
        r#"{"version":"test","protocol":18,"panes":[{}],"command_submission":{"version":1}}"#,
    );
    assert_eq!(
        kitsune::command_snapshot(&invalid).unwrap_err().code,
        "schema_error"
    );
    assert!(kitsune::session_opaque("opaque:p"));
    assert!(!kitsune::session_opaque("invalid pane"));
}

#[test]
fn process_wrapper_preserves_schema_validation_and_recognition() {
    let mut r = reply("{}");
    r.payload = serde_json::to_vec(&json!({"id":"test","result":{
        "type":"pane_process_info","process_info":{"pane_id":"pane",
        "foreground_processes":[{"pid":7,"name":"unused","argv":["/bin/djinn","--help"]}]}
    }}))
    .unwrap();
    assert_eq!(
        kitsune::session_processes(&r, "pane").unwrap(),
        vec![json!({
            "pid":7,"executable_family":"djinn","rule":"exact_posix_argv0_basename"
        })]
    );
    assert_eq!(
        kitsune::session_processes(&r, "other").unwrap_err().code,
        "schema_error"
    );
}

#[cfg(unix)]
#[test]
fn admission_revalidates_root_and_actual_controls_without_transport_effects() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(
        root.join("coven.json"),
        br#"{"name":"Session","agents":["alpha"],"multiplexer":"herdr"}"#,
    )
    .unwrap();
    let gate = admit(root).unwrap();
    validate_admission(root, &gate).unwrap();
    assert_eq!(
        TransportGuard::exclusive(root, Duration::ZERO)
            .err()
            .unwrap()
            .code(),
        "runtime_transport_gate_busy"
    );
    let other = tempfile::tempdir().unwrap();
    assert_eq!(
        validate_admission(other.path(), &gate).unwrap_err().code(),
        "invalid_transport_gate_owner"
    );
    fs::create_dir_all(root.join("state/actions")).unwrap();
    assert_eq!(
        validate_admission(root, &gate).unwrap_err().code(),
        "action_orphan_bundles"
    );
    fs::remove_dir(root.join("state/actions")).unwrap();
    fs::write(root.join("state/runtime-launch-control.json"), b"{}").unwrap();
    assert!(validate_admission(root, &gate).is_err());
    let location = platform::workspace_location(root).unwrap();
    fs::write(root.join("state/runtime-launch-control.json"), serde_json::to_vec(&json!({
        "version":1,"action":"runtime_launch","workspace_id":"a".repeat(32),
        "epoch_id":"b".repeat(32),"location":{"root":location.root,"device":location.device,"inode":location.inode},
        "head":null,"claims":{}
    })).unwrap()).unwrap();
    assert_eq!(
        validate_admission(root, &gate).unwrap_err().code(),
        "legacy_runtime_disabled"
    );
    drop(gate);
    assert_eq!(admit(root).err().unwrap().code(), "legacy_runtime_disabled");
}
