use super::*;
use std::{cell::RefCell, fs, io::Write};

fn minimal() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("coven.json"),
        br#"{"name":"test","agents":["operator"]}"#,
    )
    .unwrap();
    temp
}
fn items(values: &[&str]) -> Vec<PathBuf> {
    values.iter().map(PathBuf::from).collect()
}

struct AppendFault {
    partial: bool,
}
impl logs::AppendIo for AppendFault {
    fn write(&self, file: &mut fs::File, bytes: &[u8]) -> std::io::Result<()> {
        if self.partial {
            file.write_all(&bytes[..bytes.len() / 2])?;
            return Err(std::io::Error::other("injected partial write"));
        }
        file.write_all(bytes)
    }
    fn sync(&self, _file: &fs::File) -> std::io::Result<()> {
        Err(std::io::Error::other("injected sync failure"))
    }
}

#[derive(Default)]
struct FaultIo {
    fault: &'static str,
    phases: RefCell<Vec<String>>,
    guards: RefCell<Vec<usize>>,
}
impl FaultIo {
    fn new(fault: &'static str) -> Self {
        Self {
            fault,
            ..Self::default()
        }
    }
}
impl Io for FaultIo {
    fn now(&self) -> Result<String> {
        Ok("2026-11-01T00:00:00+00:00".into())
    }
    fn phase(&self, phase: &str, root: &Path) -> Result<()> {
        self.phases.borrow_mut().push(phase.into());
        if phase.ends_with(".held") || phase.ends_with("dashboard_held") {
            assert!(matches!(
                WorkspaceGuard::acquire(root, Duration::ZERO),
                Err(Error("workspace_lock_busy"))
            ));
        }
        if phase.ends_with(".released") {
            drop(WorkspaceGuard::acquire(root, Duration::ZERO).unwrap());
        }
        if self.fault == phase {
            return Err(Error("injected_phase_failure"));
        }
        if self.fault == "race" && phase.ends_with("before_lock") {
            fs::write(root.join("coven.toml"), "[runtime]\nopencode_command=' '").unwrap();
        }
        if self.fault == "dashboard_race" && phase == "message.dashboard_before_lock" {
            fs::write(root.join("coven.toml"), "[runtime]\nopencode_command=' '").unwrap();
        }
        if self.fault == "events_race" && phase == "message.before_lock" {
            fs::write(root.join("logs/events.jsonl"), b"bad\n").unwrap();
        }
        if phase == "status.released" {
            match self.fault {
                "literal" => {
                    fs::write(root.join("dashboard.md"), b"\x1b[31m\r\nraw\rtext\n").unwrap()
                }
                "missing" => fs::remove_file(root.join("dashboard.md")).unwrap(),
                "bad_output" => fs::write(root.join("dashboard.md"), [0xff]).unwrap(),
                _ => {}
            }
        }
        Ok(())
    }
    fn replace(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        self.guards.borrow_mut().push(guard as *const _ as usize);
        if self.fault == "replace" {
            return Err(Error("injected_replace_failure"));
        }
        let result = storage::atomic_replace_bytes(guard, Path::new("goal.md"), bytes)?;
        if self.fault == "uncertain" {
            Ok(Delivery::DurabilityUncertain)
        } else {
            Ok(result)
        }
    }
    fn append(&self, root: &Path, bytes: &[u8], guard: &WorkspaceGuard) -> Result<()> {
        guard.validate_for(root)?;
        self.guards.borrow_mut().push(guard as *const _ as usize);
        if self.fault == "partial" {
            fs::create_dir_all(root.join("logs"))?;
            let mut file = platform::open_append(&root.join("logs/messages.jsonl"))?;
            file.write_all(&bytes[..bytes.len() / 2])?;
            file.sync_all()?;
            return Err(Error("message_append_may_be_partial"));
        }
        if self.fault == "message_sync" {
            return logs::append_encoded_with(
                root,
                LogKind::Messages,
                bytes,
                guard,
                &AppendFault { partial: false },
            );
        }
        logs::append_encoded(root, LogKind::Messages, bytes, guard)
    }
    fn event(
        &self,
        root: &Path,
        kind: &str,
        payload: &Value,
        guard: &WorkspaceGuard,
    ) -> Result<()> {
        guard.validate_for(root)?;
        self.phases.borrow_mut().push(kind.into());
        self.guards.borrow_mut().push(guard as *const _ as usize);
        if matches!(self.fault, "event" | "both") {
            return Err(Error("injected_event_failure"));
        }
        if self.fault == "event_partial" {
            workspace::load(root)?;
            let mut event = Value::object(vec![
                (
                    "event_id".into(),
                    Value::String(task_time::uuid_hyphenated()?),
                ),
                ("ts".into(), Value::String(self.now()?)),
                ("type".into(), Value::String(kind.into())),
            ])?;
            for (key, value) in payload.as_object().unwrap().entries() {
                event.set_field(key, value.clone())?;
            }
            let encoded = logs::encode_record(&event)?;
            return logs::append_encoded_with(
                root,
                LogKind::Events,
                &encoded,
                guard,
                &AppendFault { partial: true },
            );
        }
        task_delivery::append_event(root, kind, payload, guard)
    }
    fn dashboard(&self, root: &Path, guard: &WorkspaceGuard) -> Result<()> {
        guard.validate_for(root)?;
        self.phases.borrow_mut().push("dashboard".into());
        self.guards.borrow_mut().push(guard as *const _ as usize);
        if matches!(self.fault, "dashboard" | "both") {
            return Err(Error("injected_dashboard_failure"));
        }
        task_delivery::refresh_dashboard(root, guard)?;
        if self.fault == "dashboard_uncertain" {
            return Err(Error("dashboard_synchronization_uncertain"));
        }
        Ok(())
    }
}

#[test]
fn goal_reads_literal_universal_newlines_and_adds_exactly_one_lf() {
    let root = minimal();
    assert_eq!(
        goal(root.path(), &[], &SystemIo).err().unwrap().code(),
        "missing_goal_file"
    );
    for (raw, expected) in [
        (b"".as_slice(), "\n"),
        (b"a\r\nb\rc\n".as_slice(), "a\nb\nc\n\n"),
        (
            "\u{1b}[31m\u{202e}\u{1f}é".as_bytes(),
            "\u{1b}[31m\u{202e}\u{1f}é\n",
        ),
    ] {
        fs::write(root.path().join("goal.md"), raw).unwrap();
        let out = goal(root.path(), &items(&[" \u{a0}", "\u{1f}"]), &SystemIo).unwrap();
        assert_eq!(out.stdout, expected);
        assert_eq!(fs::read(root.path().join("goal.md")).unwrap(), raw);
        assert!(!root.path().join("state").exists());
    }
}

#[test]
fn goal_exact_bytes_independent_secondary_warnings_and_same_guard() {
    for fault in ["", "event", "dashboard", "both"] {
        let root = minimal();
        let io = FaultIo::new(fault);
        let out = goal(
            root.path(),
            &items(&[" \u{a0}hello", "\u{1b}[31m\rworld\u{a0} "]),
            &io,
        )
        .unwrap();
        assert_eq!(out.stdout, "[INFO] Goal updated\n");
        assert_eq!(out.exit_code, 0);
        assert_eq!(
            out.warnings.len(),
            match fault {
                "" => 0,
                "both" => 2,
                _ => 1,
            }
        );
        assert_eq!(
            fs::read(root.path().join("goal.md")).unwrap(),
            "# Goal\n\nhello \u{1b}[31m\rworld\n".as_bytes()
        );
        assert!(io.guards.borrow().windows(2).all(|g| g[0] == g[1]));
        assert!(io.phases.borrow().iter().any(|p| p == "dashboard"));
    }
}

#[test]
fn goal_known_rename_uncertainty_is_exit_one_and_skips_secondaries() {
    let root = minimal();
    fs::write(root.path().join("goal.md"), "old").unwrap();
    let io = FaultIo::new("uncertain");
    let out = goal(root.path(), &items(&["new"]), &io).unwrap();
    assert_eq!(out.exit_code, 1);
    assert!(out.stdout.is_empty());
    assert!(out.warnings[0].contains("New goal observed"));
    assert_eq!(
        fs::read(root.path().join("goal.md")).unwrap(),
        b"# Goal\n\nnew\n"
    );
    assert!(!root.path().join("logs").exists());
    assert!(!root.path().join("dashboard.md").exists());
    assert_eq!(io.guards.borrow().len(), 1);
}

#[test]
fn goal_bounds_and_pre_rename_failures_leave_primary_unchanged() {
    let root = minimal();
    fs::write(root.path().join("goal.md"), "old").unwrap();
    assert!(
        goal(
            root.path(),
            &[PathBuf::from("x".repeat(MAX_TEXT - 8))],
            &SystemIo
        )
        .is_err()
    );
    assert!(!root.path().join("state").exists());
    assert!(goal(root.path(), &items(&["new"]), &FaultIo::new("replace")).is_err());
    assert_eq!(fs::read(root.path().join("goal.md")).unwrap(), b"old");
    let out = goal(
        root.path(),
        &[PathBuf::from("x".repeat(MAX_TEXT - 9))],
        &FaultIo::new("both"),
    )
    .unwrap();
    assert_eq!(out.exit_code, 0);
    assert_eq!(
        fs::metadata(root.path().join("goal.md")).unwrap().len(),
        MAX_TEXT as u64
    );
    fs::write(root.path().join("goal.md"), vec![b'x'; MAX_TEXT + 1]).unwrap();
    assert!(goal(root.path(), &[], &SystemIo).is_err());
}

#[test]
fn message_exact_record_free_parties_unstripped_body_and_lock_epochs() {
    let root = minimal();
    let io = FaultIo::default();
    let out = message(
        root.path(),
        &items(&[" outside\n", "", " \u{a0}", "\u{1b}[31m\r "]),
        &io,
    )
    .unwrap();
    assert_eq!(out.stdout, "[INFO] Message appended\n");
    assert_eq!(out.exit_code, 0);
    assert_eq!(fs::read(root.path().join("logs/messages.jsonl")).unwrap(), "{\"ts\":\"2026-11-01T00:00:00+00:00\",\"from\":\" outside\\n\",\"to\":\"\",\"body\":\" \u{a0} \\u001b[31m\\r \"}\n".as_bytes());
    let events = logs::read(root.path(), LogKind::Events).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].as_object().unwrap().get("type").unwrap().as_str(),
        Some("message.sent")
    );
    let phases = io.phases.borrow();
    assert_eq!(
        phases.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "message.before_lock",
            "message.held",
            "message.sent",
            "message.released",
            "message.dashboard_before_lock",
            "message.dashboard_held",
            "dashboard"
        ]
    );
    assert_eq!(io.guards.borrow()[0], io.guards.borrow()[1]);
}

#[test]
fn message_actual_partial_append_is_not_repaired_or_successful() {
    let root = minimal();
    let out = message(
        root.path(),
        &items(&["a", "b", "body"]),
        &FaultIo::new("partial"),
    )
    .unwrap();
    assert_eq!(out.exit_code, 1);
    assert!(out.stdout.is_empty());
    assert!(out.warnings[0].contains("may be partial"));
    let bytes = fs::read(root.path().join("logs/messages.jsonl")).unwrap();
    assert!(!bytes.is_empty());
    assert_ne!(bytes.last(), Some(&b'\n'));
    assert!(!root.path().join("logs/events.jsonl").exists());
    assert!(!root.path().join("dashboard.md").exists());
    assert!(message(root.path(), &items(&["a", "b", "retry"]), &SystemIo).is_err());
    assert_eq!(
        fs::read(root.path().join("logs/messages.jsonl")).unwrap(),
        bytes
    );
}

#[test]
fn message_late_failures_report_known_progress_without_success_or_rollback() {
    for fault in [
        "event",
        "dashboard",
        "message.released",
        "message.dashboard_held",
    ] {
        let root = minimal();
        let out = message(
            root.path(),
            &items(&["a", "b", "body"]),
            &FaultIo::new(fault),
        )
        .unwrap();
        assert_eq!(out.exit_code, 1, "{fault}");
        assert!(out.stdout.is_empty());
        assert_eq!(logs::read(root.path(), LogKind::Messages).unwrap().len(), 1);
        if fault == "event" {
            assert!(out.warnings[0].contains("Message appended; event"));
            assert!(!root.path().join("logs/events.jsonl").exists());
        } else {
            assert!(out.warnings[0].contains("Message and event appended"));
            assert_eq!(logs::read(root.path(), LogKind::Events).unwrap().len(), 1);
        }
    }
}

#[test]
fn held_admission_is_fresh_for_all_three_commands() {
    for command in ["goal", "message", "status"] {
        let root = minimal();
        let io = FaultIo::new("race");
        let result = match command {
            "goal" => goal(root.path(), &items(&["new"]), &io),
            "message" => message(root.path(), &items(&["a", "b", "body"]), &io),
            _ => status(root.path(), &io),
        };
        assert_eq!(
            result.err().unwrap().code(),
            "invalid_runtime_configuration"
        );
        assert!(!root.path().join("goal.md").exists());
        assert!(!root.path().join("logs").exists());
        assert!(!root.path().join("dashboard.md").exists());
    }
}

#[test]
fn message_rechecks_event_contents_under_guard_before_append() {
    let root = minimal();
    fs::create_dir(root.path().join("logs")).unwrap();
    assert!(
        message(
            root.path(),
            &items(&["a", "b", "body"]),
            &FaultIo::new("events_race")
        )
        .is_err()
    );
    assert!(!root.path().join("logs/messages.jsonl").exists());
}

#[test]
fn goal_and_status_do_not_parse_unrelated_logs() {
    let root = minimal();
    fs::create_dir(root.path().join("logs")).unwrap();
    for log in ["events", "messages"] {
        fs::write(root.path().join(format!("logs/{log}.jsonl")), [0xff]).unwrap();
    }
    let out = goal(root.path(), &items(&["new"]), &SystemIo).unwrap();
    assert_eq!(out.exit_code, 0);
    assert_eq!(out.warnings.len(), 1);
    assert!(root.path().join("dashboard.md").exists());
    assert!(
        status(root.path(), &SystemIo)
            .unwrap()
            .stdout
            .starts_with("# Coven Dashboard")
    );
    for log in ["events", "messages"] {
        assert_eq!(
            fs::read(root.path().join(format!("logs/{log}.jsonl"))).unwrap(),
            [0xff]
        );
    }
}

#[test]
fn status_prints_reread_literal_file_without_implicit_lf_and_missing_fallback() {
    for (fault, expected) in [
        ("literal", "\u{1b}[31m\nraw\ntext\n"),
        ("missing", "No dashboard yet\n\n"),
    ] {
        let root = minimal();
        assert_eq!(
            status(root.path(), &FaultIo::new(fault)).unwrap().stdout,
            expected
        );
    }
    let root = minimal();
    let out = status(root.path(), &FaultIo::new("bad_output")).unwrap();
    assert_eq!(out.exit_code, 1);
    assert!(out.stdout.is_empty());
    assert!(out.warnings[0].contains("Dashboard published"));
}

#[test]
fn status_publication_uncertainty_preserves_known_new_bytes_exit_one() {
    let root = minimal();
    fs::write(root.path().join("dashboard.md"), b"old").unwrap();
    let out = status(root.path(), &FaultIo::new("dashboard_uncertain")).unwrap();
    assert_eq!(out.exit_code, 1);
    assert!(out.stdout.is_empty());
    assert!(out.warnings[0].contains("dashboard_synchronization_uncertain"));
    assert!(
        fs::read(root.path().join("dashboard.md"))
            .unwrap()
            .starts_with(b"# Coven Dashboard")
    );
}

#[test]
fn message_arity_precedes_config_and_goal_config_precedes_operand_decode() {
    let root = minimal();
    fs::write(
        root.path().join("coven.toml"),
        "[runtime]\nopencode_command=' '",
    )
    .unwrap();
    assert_eq!(
        message(root.path(), &items(&["a", "b"]), &SystemIo)
            .err()
            .unwrap()
            .code(),
        "message_requires_sender_recipient_body"
    );
    assert_eq!(
        goal(root.path(), &items(&["x"]), &SystemIo)
            .err()
            .unwrap()
            .code(),
        "invalid_runtime_configuration"
    );
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let bad = PathBuf::from(std::ffi::OsString::from_vec(vec![0xff]));
        assert_eq!(
            goal(root.path(), std::slice::from_ref(&bad), &SystemIo)
                .err()
                .unwrap()
                .code(),
            "invalid_runtime_configuration"
        );
        fs::remove_file(root.path().join("coven.toml")).unwrap();
        assert_eq!(
            goal(root.path(), &[bad], &SystemIo).err().unwrap().code(),
            "non_unicode_path_unsupported"
        );
    }
}

#[test]
#[cfg(unix)]
fn goal_replacement_preserves_mode_and_new_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let root = minimal();
    goal(root.path(), &items(&["new"]), &FaultIo::new("both")).unwrap();
    let path = root.path().join("goal.md");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
        0o600
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    goal(root.path(), &items(&["changed"]), &FaultIo::new("both")).unwrap();
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o7777,
        0o640
    );
}

#[test]
fn pending_import_and_active_projection_are_not_writer_authority_gates() {
    for active in [false, true] {
        let root = minimal();
        if active {
            let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap();
            let activated = crate::ledger::import_tasks(
                root.path(),
                &crate::ledger::snapshot_revision(None),
                &guard,
            )
            .unwrap();
            assert_eq!(activated.view().tasks().as_array().unwrap().len(), 0);
            drop(guard);
            fs::write(root.path().join("state/tasks.json"), b"invalid projection").unwrap();
        } else {
            fs::create_dir_all(root.path().join("state/task-ledger")).unwrap();
            fs::write(root.path().join("state/task-ledger/orphan"), b"pending").unwrap();
        }
        assert_eq!(
            goal(root.path(), &items(&["new"]), &SystemIo)
                .unwrap()
                .exit_code,
            0
        );
        assert_eq!(
            message(root.path(), &items(&["a", "b", "body"]), &SystemIo)
                .unwrap()
                .exit_code,
            0
        );
        assert_eq!(status(root.path(), &SystemIo).unwrap().exit_code, 0);
        if active {
            assert_eq!(
                fs::read(root.path().join("state/tasks.json")).unwrap(),
                b"invalid projection"
            );
        } else {
            assert!(!root.path().join(crate::ledger::MARKER).exists());
            assert_eq!(
                fs::read(root.path().join("state/task-ledger/orphan")).unwrap(),
                b"pending"
            );
        }
    }
}

#[test]
fn valid_runtime_control_is_validated_but_never_an_execution_gate() {
    let root = minimal();
    fs::create_dir(root.path().join("state")).unwrap();
    let location = platform::workspace_location(root.path()).unwrap();
    let control = serde_json::json!({
        "version": 1, "action": "runtime_launch",
        "workspace_id": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "epoch_id": "cccccccccccccccccccccccccccccccc",
        "location": { "root": location.root, "device": location.device, "inode": location.inode },
        "head": null, "claims": {}
    });
    let bytes = serde_json::to_vec(&control).unwrap();
    let path = root.path().join("state/runtime-launch-control.json");
    fs::write(&path, &bytes).unwrap();
    assert_eq!(
        goal(root.path(), &items(&["new"]), &SystemIo)
            .unwrap()
            .exit_code,
        0
    );
    assert_eq!(
        message(root.path(), &items(&["a", "b", "body"]), &SystemIo)
            .unwrap()
            .exit_code,
        0
    );
    assert_eq!(status(root.path(), &SystemIo).unwrap().exit_code, 0);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    fs::write(&path, b"null").unwrap();
    assert!(goal(root.path(), &[], &SystemIo).is_err());
    assert!(message(root.path(), &items(&["a", "b", "body"]), &SystemIo).is_err());
    assert!(status(root.path(), &SystemIo).is_err());
}

#[test]
fn oversized_message_parties_are_refused_before_clock_or_workspace_effects() {
    struct NeverNow;
    impl Io for NeverNow {
        fn now(&self) -> Result<String> {
            panic!("oversized party must be refused before metadata creation");
        }
    }
    for index in [0, 1] {
        let root = minimal();
        let mut operands = items(&["sender", "recipient", "body"]);
        operands[index] = PathBuf::from("x".repeat(values::MAX_STRING + 1));
        assert_eq!(
            message(root.path(), &operands, &NeverNow)
                .err()
                .unwrap()
                .code(),
            "unsupported_json_string_profile"
        );
        operands.insert(0, root.path().to_owned());
        assert_eq!(
            message_arguments(&operands).err().unwrap().code(),
            "unsupported_json_string_profile"
        );
        let entries = fs::read_dir(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, ["coven.json"]);
    }
}

#[test]
fn exact_maximum_message_parties_pass_public_api_serialization_and_append() {
    let root = minimal();
    let sender = "s".repeat(values::MAX_STRING);
    let recipient = "r".repeat(values::MAX_STRING);
    let operands = [
        root.path().to_owned(),
        PathBuf::from(&sender),
        PathBuf::from(&recipient),
        PathBuf::from(" body "),
    ];
    let out = message_arguments(&operands).unwrap();
    assert_eq!(out.exit_code, 0);
    assert_eq!(out.stdout, "[INFO] Message appended\n");
    assert!(out.warnings.is_empty());
    let messages = logs::read(root.path(), LogKind::Messages).unwrap();
    assert_eq!(messages.len(), 1);
    let message = messages[0].as_object().unwrap();
    assert_eq!(message.get("from").unwrap().as_str(), Some(sender.as_str()));
    assert_eq!(
        message.get("to").unwrap().as_str(),
        Some(recipient.as_str())
    );
    assert_eq!(message.get("body").unwrap().as_str(), Some(" body "));
    let events = logs::read(root.path(), LogKind::Events).unwrap();
    assert_eq!(events.len(), 1);
    let event = events[0].as_object().unwrap();
    assert_eq!(event.get("sender").unwrap().as_str(), Some(sender.as_str()));
    assert_eq!(
        event.get("recipient").unwrap().as_str(),
        Some(recipient.as_str())
    );
    assert!(root.path().join("dashboard.md").is_file());
}

#[test]
fn message_sync_failure_retains_complete_record_but_reports_conservative_failure() {
    let root = minimal();
    let io = FaultIo::new("message_sync");
    let out = message(root.path(), &items(&["a", "b", "body"]), &io).unwrap();
    assert_eq!(out.exit_code, 1);
    assert!(out.stdout.is_empty());
    assert_eq!(
        out.warnings,
        [
            "[ERROR] Message append failed and may be partial; no rollback performed: message_append_may_be_partial"
        ]
    );
    let bytes = fs::read(root.path().join("logs/messages.jsonl")).unwrap();
    assert_eq!(
        bytes,
        b"{\"ts\":\"2026-11-01T00:00:00+00:00\",\"from\":\"a\",\"to\":\"b\",\"body\":\"body\"}\n"
    );
    assert_eq!(logs::read(root.path(), LogKind::Messages).unwrap().len(), 1);
    assert!(!root.path().join("logs/events.jsonl").exists());
    assert!(!root.path().join("dashboard.md").exists());
    assert!(
        !io.phases
            .borrow()
            .iter()
            .any(|p| p == "message.sent" || p == "dashboard")
    );
}

#[test]
fn partial_event_append_preserves_goal_success_but_fails_message_without_tail_repair() {
    for command in ["goal", "message"] {
        let root = minimal();
        let io = FaultIo::new("event_partial");
        let out = if command == "goal" {
            goal(root.path(), &items(&["new goal"]), &io).unwrap()
        } else {
            message(root.path(), &items(&["a", "b", "body"]), &io).unwrap()
        };
        let path = root.path().join("logs/events.jsonl");
        let prefix = fs::read(&path).unwrap();
        assert!(prefix.starts_with(b"{\"event_id\":"));
        assert_ne!(prefix.last(), Some(&b'\n'));
        assert_eq!(out.warnings.len(), 1);
        assert!(out.warnings[0].contains("event_append_may_be_partial"));
        if command == "goal" {
            assert_eq!(out.exit_code, 0);
            assert_eq!(out.stdout, "[INFO] Goal updated\n");
            assert!(out.warnings[0].starts_with("[WARN] Goal committed"));
            assert_eq!(
                fs::read(root.path().join("goal.md")).unwrap(),
                b"# Goal\n\nnew goal\n"
            );
            assert!(
                fs::read_to_string(root.path().join("dashboard.md"))
                    .unwrap()
                    .contains("new goal")
            );
            assert!(io.phases.borrow().iter().any(|p| p == "dashboard"));
            assert!(!root.path().join("logs/messages.jsonl").exists());
        } else {
            assert_eq!(out.exit_code, 1);
            assert!(out.stdout.is_empty());
            assert!(
                out.warnings[0]
                    .contains("Message appended; event update failed and may be partial")
            );
            assert_eq!(logs::read(root.path(), LogKind::Messages).unwrap().len(), 1);
            assert!(!root.path().join("dashboard.md").exists());
            assert!(!io.phases.borrow().iter().any(|p| p == "dashboard"));
        }
        let messages = fs::read(root.path().join("logs/messages.jsonl")).ok();
        assert_eq!(
            message(root.path(), &items(&["a", "b", "retry"]), &SystemIo)
                .err()
                .unwrap()
                .code(),
            "event_log_unterminated_line"
        );
        assert_eq!(fs::read(&path).unwrap(), prefix);
        assert_eq!(
            fs::read(root.path().join("logs/messages.jsonl")).ok(),
            messages
        );
    }
}

#[test]
#[cfg(unix)]
fn dashboard_epoch_revalidates_config_and_preserves_committed_message_and_lock_inode() {
    use std::os::unix::fs::MetadataExt;
    let root = minimal();
    let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap();
    let path = root.path().join("state/.workspace.lock");
    let before = fs::metadata(&path).unwrap();
    drop(guard);
    fs::write(root.path().join("dashboard.md"), b"unchanged dashboard\r\n").unwrap();
    let io = FaultIo::new("dashboard_race");
    let out = message(root.path(), &items(&["a", "b", "body"]), &io).unwrap();
    assert_eq!(out.exit_code, 1);
    assert!(out.stdout.is_empty());
    assert_eq!(
        out.warnings,
        [
            "[ERROR] Message and event appended; dashboard update failed; no rollback performed: invalid_runtime_configuration"
        ]
    );
    assert_eq!(logs::read(root.path(), LogKind::Messages).unwrap().len(), 1);
    assert_eq!(logs::read(root.path(), LogKind::Events).unwrap().len(), 1);
    assert_eq!(
        fs::read(root.path().join("dashboard.md")).unwrap(),
        b"unchanged dashboard\r\n"
    );
    let after = fs::metadata(&path).unwrap();
    assert_eq!((after.dev(), after.ino()), (before.dev(), before.ino()));
    assert_eq!(
        io.phases
            .borrow()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "message.before_lock",
            "message.held",
            "message.sent",
            "message.released",
            "message.dashboard_before_lock",
            "message.dashboard_held",
        ]
    );
    drop(WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap());
}
