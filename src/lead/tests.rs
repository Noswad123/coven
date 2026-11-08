use super::*;
use std::{cell::RefCell, fs, process::Command};

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("coven.json"),
        br#"{"name":"Lead Team","agents":["a","b"],"display_names":{"a":"A\n\u200b"}}"#,
    )
    .unwrap();
    fs::create_dir(root.path().join("state")).unwrap();
    fs::write(
        root.path().join("goal.md"),
        "# Goal\r\n\r\nDeliver safely\r\n",
    )
    .unwrap();
    root
}
#[derive(Default)]
struct TestIo {
    calls: RefCell<Vec<String>>,
    fail_secondary: bool,
    uncertain: bool,
    fail_sync: bool,
    fail_replace: bool,
    sent: bool,
    activation_race: bool,
}
impl Io for TestIo {
    fn now(&self) -> Result<String> {
        Ok("2026-11-05T12:00:00+00:00".into())
    }
    fn replace(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        self.calls.borrow_mut().push("replace".into());
        if self.fail_replace {
            return Err(Error("injected_replace"));
        }
        let result = storage::atomic_replace_bytes(guard, Path::new(LEAD), bytes)?;
        Ok(if self.uncertain {
            Delivery::DurabilityUncertain
        } else {
            result
        })
    }
    fn sync_dir(&self, path: &Path) -> std::io::Result<()> {
        self.calls.borrow_mut().push("sync".into());
        if self.fail_sync {
            Err(std::io::Error::other("injected sync"))
        } else {
            platform::sync_directory(path)
        }
    }
    fn event(
        &self,
        root: &Path,
        kind: &str,
        payload: &Value,
        guard: &WorkspaceGuard,
    ) -> Result<()> {
        self.calls.borrow_mut().push(kind.into());
        if self.fail_secondary {
            return Err(Error("injected_event"));
        }
        let mut record = object(vec![
            ("event_id", string("11111111-1111-1111-1111-111111111111")),
            ("ts", string(&self.now()?)),
            ("type", string(kind)),
        ])?;
        for (k, v) in payload.as_object().unwrap().entries() {
            record.set_field(k, v.clone())?;
        }
        logs::append_encoded(root, LogKind::Events, &logs::encode_record(&record)?, guard)
    }
    fn dashboard(&self, root: &Path, guard: &WorkspaceGuard) -> Result<()> {
        self.calls.borrow_mut().push("dashboard".into());
        if self.fail_secondary {
            return Err(Error("injected_dashboard"));
        }
        task_delivery::refresh_dashboard(root, guard)
    }
    fn notice(&self, root: &Path, _: &str, _: &str) -> Result<bool> {
        self.calls.borrow_mut().push("notice".into());
        let guard =
            WorkspaceGuard::acquire(root, Duration::ZERO).expect("notice must run after unlocking");
        drop(guard);
        if self.activation_race {
            approve(root);
            return runtime::send_notice(root, "a", "hello");
        }
        Ok(self.sent)
    }
}
fn approve(root: &Path) {
    let location = platform::workspace_location(root).unwrap();
    fs::write(root.join("state/runtime-launch-control.json"), serde_json::to_vec(&serde_json::json!({
        "version":1,"action":"runtime_launch","workspace_id":"a".repeat(32),"epoch_id":"b".repeat(32),
        "location":{"root":location.root,"device":location.device,"inode":location.inode},"head":null,"claims":{}
    })).unwrap()).unwrap();
}
#[test]
fn approved_metadata_and_historical_show_need_no_backend_even_without_path() {
    const CHILD: &str = "COVEN_LEAD_APPROVED_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "lead::tests::approved_metadata_and_historical_show_need_no_backend_even_without_path", "--nocapture"])
            .env(CHILD, "1").env("PATH", "").env("COVEN_MULTIPLEXER", "").output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let root = fixture();
    assert_eq!(
        runtime::presentation::agent_status(root.path(), &["a".into(), "not-rostered".into()])
            .unwrap_err()
            .code(),
        "unknown_agent"
    );
    approve(root.path());
    let io = TestIo::default();
    let result = command_with(root.path(), Operation::Set("a".into()), Some("hello"), &io).unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.warnings.is_empty());
    assert!(!io.calls.borrow().iter().any(|c| c == "notice"));
    assert!(
        result
            .body
            .starts_with("[INFO] Lead metadata recorded; legacy pane delivery is disabled")
    );
    fs::write(root.path().join(LEAD), br#"{"agent":"historical"}"#).unwrap();
    let shown = command(root.path(), Operation::Show, None).unwrap();
    assert_eq!(
        shown.body,
        "Coven lead: historical herdr=unverified (inspect action transport records)\nSet: unknown time\nUntil: goal achieved or critical blocker"
    );
    fs::write(root.path().join(LEAD), br#"{"agent":"a"}"#).unwrap();
    assert!(
        command(root.path(), Operation::Show, None)
            .unwrap()
            .body
            .starts_with("Coven lead: A\\u000a\\u200b (a)")
    );
    assert!(
        !runtime::presentation::standup(root.path())
            .unwrap()
            .body
            .is_empty()
    );
    assert!(
        !runtime::presentation::agent_status(root.path(), &[])
            .unwrap()
            .body
            .is_empty()
    );
    assert_eq!(
        command(root.path(), Operation::Clear, None)
            .unwrap()
            .exit_code,
        0
    );
    fs::write(root.path().join("state/tasks.json"), "{}").unwrap();
    assert!(command(root.path(), Operation::Show, None).is_err());
}
#[test]
fn activation_between_commit_and_notice_warns_without_rollback_or_backend() {
    let root = fixture();
    let io = TestIo {
        activation_race: true,
        ..Default::default()
    };
    let result = command_with(root.path(), Operation::Set("a".into()), Some("hello"), &io).unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        result.warnings,
        ["[WARN] Lead committed; delivery unavailable: legacy_runtime_disabled"]
    );
    assert!(root.path().join(LEAD).exists());
    assert_eq!(logs::read(root.path(), LogKind::Messages).unwrap().len(), 1);
}
#[test]
fn python_set_and_clear_exact_output_storage_history_dashboard_and_prompt() {
    for message in [None, Some(""), Some("custom message")] {
        let root = fixture();
        let initial =
            br#"{"agent":"historical","extension":{"x":null},"nullable":null,"until":"old"}"#;
        fs::write(root.path().join(LEAD), initial).unwrap();
        let io = TestIo::default();
        let result = command_with(root.path(), Operation::Set("a".into()), message, &io).unwrap();
        assert!(result.warnings.is_empty());
        let paths = [
            LEAD,
            "logs/events.jsonl",
            "logs/messages.jsonl",
            "dashboard.md",
        ];
        let actual: Vec<_> = paths
            .iter()
            .map(|p| fs::read(root.path().join(p)).unwrap())
            .collect();
        for p in paths {
            fs::remove_file(root.path().join(p)).unwrap();
        }
        fs::write(root.path().join(LEAD), initial).unwrap();
        let oracle = Command::new("python3").args(["-B", "-c", concat!(
            "import argparse,contextlib,io,json,sys,uuid;from pathlib import Path;from runner import coven\n",
            "w=Path(sys.argv[1]);coven.now=lambda:'2026-11-05T12:00:00+00:00';coven.uuid.uuid4=lambda:uuid.UUID('11111111-1111-1111-1111-111111111111')\n",
            "coven.notify_agents=lambda *a,**kw:(0,['a'])\n",
            "out=io.StringIO()\nwith contextlib.redirect_stdout(out): coven.command_lead(argparse.Namespace(items=[str(w),'set','a'],message=json.loads(sys.argv[2]),clear=False))\n",
            "print(json.dumps({'body':out.getvalue(),'files':[(w/p).read_text() for p in ['state/lead.json','logs/events.jsonl','logs/messages.jsonl','dashboard.md']]}))"
        )]).arg(root.path()).arg(serde_json::to_string(&message).unwrap()).env("COVEN_MULTIPLEXER", "")
            .current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
        assert!(
            oracle.status.success(),
            "{}",
            String::from_utf8_lossy(&oracle.stderr)
        );
        let expected: serde_json::Value = serde_json::from_slice(&oracle.stdout).unwrap();
        assert_eq!(
            format!("{}\n", result.body),
            expected["body"].as_str().unwrap()
        );
        for (actual, expected) in actual.iter().zip(expected["files"].as_array().unwrap()) {
            assert_eq!(
                std::str::from_utf8(actual).unwrap(),
                expected.as_str().unwrap()
            );
        }
        let cleared = command_with(root.path(), Operation::Clear, None, &io).unwrap();
        assert_eq!(cleared.body, "[INFO] Cleared coven lead");
        assert!(!root.path().join(LEAD).exists());
        let events = logs::read(root.path(), LogKind::Events).unwrap();
        let previous = runtime::presentation::field(events.last().unwrap(), "previous").unwrap();
        assert!(runtime::presentation::field(previous, "extension").is_some());
    }
}
#[test]
fn deterministic_failures_do_not_create_workspace_lock_or_change_existing_fields() {
    let root = fixture();
    for (operation, message) in [
        (Operation::Set("missing".into()), None),
        (Operation::Set("a".into()), Some("bad\0message")),
    ] {
        assert!(command_with(root.path(), operation, message, &TestIo::default()).is_err());
        assert!(!root.path().join("state/.workspace.lock").exists());
    }
    fs::write(root.path().join("goal.md"), [0xff]).unwrap();
    assert!(
        command_with(
            root.path(),
            Operation::Set("a".into()),
            None,
            &TestIo::default()
        )
        .is_err()
    );
    assert!(!root.path().join("state/.workspace.lock").exists());
    fs::write(root.path().join("goal.md"), "x".repeat(values::MAX_STRING)).unwrap();
    assert_eq!(
        command_with(
            root.path(),
            Operation::Set("a".into()),
            None,
            &TestIo::default()
        )
        .unwrap_err()
        .code(),
        "unsupported_json_string_profile"
    );
    assert!(!root.path().join("state/.workspace.lock").exists());
}
#[test]
fn uncertainty_skips_all_secondary_and_notice_effects_after_known_replace_or_unlink() {
    for clear in [false, true] {
        let root = fixture();
        fs::write(root.path().join(LEAD), br#"{"agent":"old","extra":null}"#).unwrap();
        let io = TestIo {
            uncertain: true,
            fail_sync: true,
            ..Default::default()
        };
        let result = command_with(
            root.path(),
            if clear {
                Operation::Clear
            } else {
                Operation::Set("a".into())
            },
            Some("message"),
            &io,
        )
        .unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.body.is_empty());
        assert_eq!(
            io.calls.borrow().as_slice(),
            if clear { vec!["sync"] } else { vec!["replace"] }
        );
        assert_eq!(root.path().join(LEAD).exists(), !clear);
    }
}
#[test]
fn secondary_failures_continue_in_order_committed_exit_zero_and_notice_unlocked() {
    let root = fixture();
    let io = TestIo {
        fail_secondary: true,
        sent: true,
        ..Default::default()
    };
    let result = command_with(
        root.path(),
        Operation::Set("a".into()),
        Some("message"),
        &io,
    )
    .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        io.calls.borrow().as_slice(),
        [
            "replace",
            "lead.set",
            "message.sent",
            "dashboard",
            "notice",
            "lead.message.sent"
        ]
    );
    assert_eq!(result.warnings.len(), 4);
    assert_eq!(logs::read(root.path(), LogKind::Messages).unwrap().len(), 1);
    let io = TestIo {
        fail_secondary: true,
        ..Default::default()
    };
    let result = command_with(root.path(), Operation::Clear, None, &io).unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.warnings.len(), 2);
    assert_eq!(
        io.calls.borrow().as_slice(),
        ["sync", "lead.cleared", "dashboard"]
    );
}
#[test]
fn successful_notice_records_only_one_message_and_notification_event() {
    let root = fixture();
    let io = TestIo {
        sent: true,
        ..Default::default()
    };
    let result = command_with(root.path(), Operation::Set("a".into()), Some("hello"), &io).unwrap();
    assert_eq!(
        result.body,
        "[INFO] Set a as coven lead; notified 1 agent(s)"
    );
    assert_eq!(logs::read(root.path(), LogKind::Messages).unwrap().len(), 1);
    let kinds: Vec<_> = logs::read(root.path(), LogKind::Events)
        .unwrap()
        .iter()
        .map(|v| runtime::presentation::text(v, "type", "").to_owned())
        .collect();
    assert_eq!(kinds, ["lead.set", "message.sent", "lead.message.sent"]);
}
#[test]
fn missing_clear_is_idempotent_and_pre_replace_failure_preserves_bytes() {
    let root = fixture();
    let io = TestIo::default();
    assert_eq!(
        command_with(root.path(), Operation::Clear, None, &io)
            .unwrap()
            .exit_code,
        0
    );
    assert!(!io.calls.borrow().iter().any(|c| c == "sync"));
    let initial = br#"{"agent":"old","extra":null}"#;
    fs::write(root.path().join(LEAD), initial).unwrap();
    let io = TestIo {
        fail_replace: true,
        ..Default::default()
    };
    assert!(command_with(root.path(), Operation::Set("a".into()), Some("hello"), &io).is_err());
    assert_eq!(fs::read(root.path().join(LEAD)).unwrap(), initial);
    assert_eq!(io.calls.borrow().as_slice(), ["replace"]);
}

#[test]
fn python_clear_exact_unlink_output_previous_payload_and_dashboard() {
    let root = fixture();
    let initial = br#"{"agent":"historical","set_at":"","extension":[null,{"preserve":true}]}"#;
    fs::write(root.path().join(LEAD), initial).unwrap();
    let result = command_with(root.path(), Operation::Clear, None, &TestIo::default()).unwrap();
    let paths = ["logs/events.jsonl", "dashboard.md"];
    let actual: Vec<_> = paths
        .iter()
        .map(|p| fs::read(root.path().join(p)).unwrap())
        .collect();
    for p in paths {
        fs::remove_file(root.path().join(p)).unwrap();
    }
    fs::write(root.path().join(LEAD), initial).unwrap();
    let oracle = Command::new("python3").args(["-B", "-c", concat!(
        "import argparse,contextlib,io,json,sys,uuid;from pathlib import Path;from runner import coven\n",
        "w=Path(sys.argv[1]);coven.now=lambda:'2026-11-05T12:00:00+00:00';coven.uuid.uuid4=lambda:uuid.UUID('11111111-1111-1111-1111-111111111111')\n",
        "out=io.StringIO()\nwith contextlib.redirect_stdout(out): coven.command_lead(argparse.Namespace(items=[str(w),'clear'],message=None,clear=False))\n",
        "print(json.dumps({'body':out.getvalue(),'files':[(w/p).read_text() for p in ['logs/events.jsonl','dashboard.md']],'exists':(w/'state/lead.json').exists()}))"
    )]).arg(root.path()).env("COVEN_MULTIPLEXER", "").current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
    assert!(
        oracle.status.success(),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    let expected: serde_json::Value = serde_json::from_slice(&oracle.stdout).unwrap();
    assert_eq!(
        format!("{}\n", result.body),
        expected["body"].as_str().unwrap()
    );
    assert_eq!(expected["exists"], false);
    for (actual, expected) in actual.iter().zip(expected["files"].as_array().unwrap()) {
        assert_eq!(
            std::str::from_utf8(actual).unwrap(),
            expected.as_str().unwrap()
        );
    }
}

#[test]
#[cfg(unix)]
fn replacement_preserves_mode_and_utf8_unknown_extension_values() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let root = fixture();
    let path = root.path().join(LEAD);
    let initial = "{\"agent\":\"old\",\"extra\":{\"nullable\":null,\"text\":\"\\u4e16\\u754c\"}}";
    fs::write(&path, initial).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let before = fs::metadata(&path).unwrap();
    let result = command_with(
        root.path(),
        Operation::Set("a".into()),
        Some("hello"),
        &TestIo::default(),
    )
    .unwrap();
    assert_eq!(result.exit_code, 0);
    let after = fs::metadata(&path).unwrap();
    assert_eq!(after.mode() & 0o7777, 0o640);
    assert_ne!(after.ino(), before.ino());
    let bytes = fs::read(&path).unwrap();
    assert!(
        std::str::from_utf8(&bytes)
            .unwrap()
            .contains("\u{4e16}\u{754c}")
    );
    let lead = values::parse(&bytes, values::Profile::LegacySnapshot).unwrap();
    let extra = runtime::presentation::field(&lead, "extra").unwrap();
    assert!(matches!(
        runtime::presentation::field(extra, "nullable"),
        Some(Value::Null)
    ));
    assert!(runtime::presentation::field(&lead, "never-present").is_none());
}

fn rejected_candidate_preserves_original_and_can_still_clear(original: Vec<u8>, code: &str) {
    let root = fixture();
    let path = root.path().join(LEAD);
    fs::write(&path, &original).unwrap();
    workspace::load(root.path()).expect("fixture must be readable before Set");
    let io = TestIo::default();
    let error =
        command_with(root.path(), Operation::Set("a".into()), Some("hello"), &io).unwrap_err();
    assert_eq!(error.code(), code);
    assert!(
        io.calls.borrow().is_empty(),
        "no replace, secondary, or notice"
    );
    assert!(!root.path().join("state/.workspace.lock").exists());
    assert!(!root.path().join("state/.runtime-transport.lock").exists());
    assert!(!root.path().join("logs").exists());
    assert!(!root.path().join("dashboard.md").exists());
    assert_eq!(fs::read(&path).unwrap(), original);
    workspace::load(root.path()).expect("rejected Set must leave the lead readable");
    let cleared = command_with(root.path(), Operation::Clear, None, &io).unwrap();
    assert_eq!(cleared.exit_code, 0);
    assert!(!path.exists());
    assert!(matches!(
        workspace::load(root.path()).unwrap().lead,
        Value::Null
    ));
    assert!(
        !io.calls
            .borrow()
            .iter()
            .any(|call| call == "replace" || call == "notice")
    );
}

#[test]
fn set_rejects_metadata_crossing_native_node_limit_before_lock_or_effects() {
    // 262143 nodes: root + agent + extension + four arrays + 262136 nulls.
    let extension = Value::Array(
        [65536, 65536, 65536, 65528]
            .into_iter()
            .map(|count| Value::Array(vec![Value::Null; count]))
            .collect(),
    );
    let lead = object(vec![("agent", string("old")), ("extension", extension)]).unwrap();
    let original = format!(
        " \n{}\t\n",
        String::from_utf8(lead.compact().unwrap()).unwrap()
    )
    .into_bytes();
    assert!(original.len() < values::MAX_INPUT);
    values::parse(&original, values::Profile::LegacySnapshot).unwrap();
    rejected_candidate_preserves_original_and_can_still_clear(
        original,
        "unsupported_json_node_profile",
    );
}

#[test]
fn set_rejects_pretty_storage_exceeding_native_byte_limit_before_lock_or_effects() {
    let mut extension = Value::Array(vec![Value::Array(vec![Value::Null; 50000]); 2]);
    for _ in 0..40 {
        extension = Value::Array(vec![extension]);
    }
    let lead = object(vec![("agent", string("old")), ("extension", extension)]).unwrap();
    let original = format!(
        "\t{}\n",
        String::from_utf8(lead.compact().unwrap()).unwrap()
    )
    .into_bytes();
    assert!(original.len() < values::MAX_INPUT);
    assert!(lead.storage().unwrap().len() > values::MAX_INPUT);
    values::parse(&original, values::Profile::LegacySnapshot).unwrap();
    rejected_candidate_preserves_original_and_can_still_clear(
        original,
        "unsupported_json_size_profile",
    );
}

#[test]
fn set_validates_actual_timestamp_under_lock_before_replace() {
    struct InvalidTime;
    impl Io for InvalidTime {
        fn now(&self) -> Result<String> {
            Ok("x".repeat(values::MAX_STRING + 1))
        }
        fn replace(&self, _: &WorkspaceGuard, _: &[u8]) -> Result<Delivery> {
            panic!("invalid final candidate must not reach replacement")
        }
    }
    let root = fixture();
    let original = br#" {"agent":"old","extension":null} "#;
    fs::write(root.path().join(LEAD), original).unwrap();
    let error = command_with(
        root.path(),
        Operation::Set("a".into()),
        Some("hello"),
        &InvalidTime,
    )
    .unwrap_err();
    assert_eq!(error.code(), "unsupported_json_string_profile");
    assert!(root.path().join("state/.workspace.lock").exists());
    assert_eq!(fs::read(root.path().join(LEAD)).unwrap(), original);
    assert!(!root.path().join("logs").exists());
    assert!(!root.path().join("dashboard.md").exists());
    workspace::load(root.path()).unwrap();
}
