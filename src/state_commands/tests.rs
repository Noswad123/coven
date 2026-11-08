use super::*;
use std::fs;

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("coven.json"),
        br#"{"name":"test","agents":["operator"]}"#,
    )
    .unwrap();
    root
}
fn seeded() -> tempfile::TempDir {
    let root = fixture();
    fs::create_dir(root.path().join("state")).unwrap();
    fs::write(root.path().join("state/tasks.json"), br#" [ {"id":"t","title":"Task","status":"custom","extension":[123456789012345678901234567890,-0.0,"\u03bb"]} ]
"#).unwrap();
    root
}
fn fault(name: &'static str, fault: Fault) -> StateIo {
    StateIo {
        faults: [(name, fault)].into(),
        ..Default::default()
    }
}
fn parsed(result: &TaskOutput) -> Value {
    assert!(result.warnings.is_empty());
    assert!(result.stdout.is_ascii());
    assert!(!result.stdout.ends_with('\n'));
    values::parse(result.stdout.as_bytes(), Profile::LegacySnapshot).unwrap()
}
fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    field(value, key).unwrap()
}
fn s<'a>(value: &'a Value, key: &str) -> &'a str {
    get(value, key).as_str().unwrap()
}
fn code<T>(result: Result<T>) -> &'static str {
    match result {
        Err(error) => error.code(),
        Ok(_) => panic!("expected error"),
    }
}
fn import(root: &Path) -> TaskOutput {
    let (revision, _) = snapshot(root).unwrap();
    import_arguments(root, &revision, true).unwrap()
}
fn pending(root: &Path) -> String {
    let (revision, _) = snapshot(root).unwrap();
    let guard = WorkspaceGuard::acquire(root, Duration::ZERO).unwrap();
    let _report = ledger::import_tasks(root, &revision, &guard).unwrap();
    fs::remove_file(root.join(ledger::MARKER)).unwrap();
    revision
}

#[test]
fn inspect_pretty_compact_and_missing_are_json_without_stderr_or_final_lf() {
    let root = fixture();
    let compact = inspect_arguments(root.path(), true).unwrap();
    assert_eq!(compact.exit_code, 0);
    assert_eq!(
        compact.stdout,
        r#"{"authority":"snapshot-v1","revision":"tasks-missing:v1","tasks":[],"import_pending":false}"#
    );
    let pretty = inspect_arguments(root.path(), false).unwrap();
    let expected = parsed(&compact).storage().unwrap();
    assert_eq!(pretty.stdout.as_bytes(), &expected[..expected.len() - 1]);
    assert!(!root.path().join("state").exists());
}

#[test]
fn inspection_uses_diagnostic_not_agents_lead_or_pending_source_parsing() {
    let root = fixture();
    fs::create_dir_all(root.path().join(ledger::DIRECTORY)).unwrap();
    for relative in ["state/tasks.json", "state/agents.json", "state/lead.json"] {
        fs::write(root.path().join(relative), "bad json").unwrap();
    }
    let report = parsed(&inspect_arguments(root.path(), false).unwrap());
    assert_eq!(s(&report, "authority"), "snapshot-v1");
    assert!(matches!(get(&report, "import_pending"), Value::Bool(true)));
    assert!(report.as_object().unwrap().get("source_error").is_some());
    assert!(report.as_object().unwrap().get("revision").is_none());
    assert!(report.as_object().unwrap().get("tasks").is_none());
    fs::remove_dir(root.path().join(ledger::DIRECTORY)).unwrap();
    assert!(inspect_arguments(root.path(), true).is_err());
    assert!(!root.path().join("state/.workspace.lock").exists());
}

#[test]
fn strict_snapshot_pins_do_not_accept_ledger_or_uppercase_or_extra_bytes() {
    assert!(pin("tasks-missing:v1").is_ok());
    assert!(pin(&format!("tasks-sha256:{}", "a1".repeat(32))).is_ok());
    for value in [
        "",
        "tasks-missing:v1\n",
        "tasks-ledger:v1:abc",
        &format!("tasks-sha256:{}", "A".repeat(64)),
        &format!("tasks-sha256:{}", "a".repeat(63)),
    ] {
        assert_eq!(code(pin(value)), "invalid_snapshot_revision");
    }
}

#[test]
fn deterministic_import_and_recovery_errors_precede_lock_creation() {
    let root = fixture();
    let io = StateIo::default();
    assert_eq!(
        code(activate(root.path(), Some("bad"), false, true, &io)),
        "invalid_snapshot_revision"
    );
    assert_eq!(
        code(activate(root.path(), None, true, true, &io)),
        "resume_import_requires_revision"
    );
    assert_eq!(
        code(recover(
            root.path(),
            RecoveryMode::RepairProjection,
            Some("bad"),
            true,
            &io
        )),
        "revision_only_with_resume_import"
    );
    assert_eq!(
        code(activate(
            root.path(),
            Some(&format!("tasks-sha256:{}", "0".repeat(64))),
            false,
            true,
            &io
        )),
        "snapshot_revision_mismatch"
    );
    assert_eq!(
        code(recover(
            root.path(),
            RecoveryMode::RepairProjection,
            None,
            true,
            &io
        )),
        "no_active_ledger"
    );
    fs::write(root.path().join("coven.json"), "corrupt").unwrap();
    for mode in [RecoveryMode::ResumeImport, RecoveryMode::RepairProjection] {
        let error = code(recover(root.path(), mode, None, true, &io));
        assert_ne!(error, "resume_import_requires_revision");
        assert_ne!(error, "no_active_ledger");
    }
    assert_eq!(
        code(recover(
            root.path(),
            RecoveryMode::AbortImport,
            Some("bad"),
            true,
            &io
        )),
        "revision_only_with_resume_import"
    );
    assert!(io.trace.borrow().is_empty());
    assert!(!root.path().join("state").exists());
}

#[test]
fn import_resume_and_repair_normal_reports_keep_known_metadata_and_no_events() {
    let root = seeded();
    let before = fs::read(root.path().join("state/tasks.json")).unwrap();
    let expected = snapshot(root.path()).unwrap().0;
    let result = parsed(&import(root.path()));
    assert_eq!(s(&result, "outcome"), "committed");
    assert_eq!(s(&result, "authority"), "task-ledger-v1");
    assert_eq!(
        fs::read(root.path().join("state/tasks.json")).unwrap(),
        before
    );
    let view = ledger::read(root.path()).unwrap().unwrap();
    assert_eq!(s(&result, "revision"), view.revision());
    assert_eq!(s(&result, "workspace_id"), view.workspace_id());
    assert_eq!(s(&result, "epoch_id"), view.epoch_id());
    assert_eq!(s(&result, "frame_digest"), view.head_digest());
    assert_eq!(s(get(&result, "projection"), "status"), "current");
    fs::remove_file(root.path().join(ledger::MARKER)).unwrap();
    let resumed = recover_arguments(
        root.path(),
        RecoveryMode::ResumeImport,
        Some(&expected),
        false,
    )
    .unwrap();
    assert_eq!(resumed.exit_code, 0);
    let resumed = parsed(&resumed);
    assert_eq!(s(&resumed, "outcome"), "recovered");
    assert!(matches!(
        get(get(&resumed, "recovery"), "import_resumed"),
        Value::Bool(true)
    ));
    let repaired = parsed(
        &recover_arguments(root.path(), RecoveryMode::RepairProjection, None, true).unwrap(),
    );
    assert_eq!(s(get(&repaired, "recovery"), "status"), "current");
    assert!(matches!(
        get(get(&repaired, "recovery"), "backup_path"),
        Value::Null
    ));
    assert!(!root.path().join("logs/events.jsonl").exists());
}

#[test]
fn projection_repair_preserves_exact_invalid_bytes_and_path_even_if_render_fails() {
    for failed in ["none", "render", "report"] {
        let root = seeded();
        import(root.path());
        let raw = b"not json\x00\xff\r\n";
        fs::write(root.path().join("state/tasks.json"), raw).unwrap();
        let io = fault(failed, Fault::Error);
        let result = recover(
            root.path(),
            RecoveryMode::RepairProjection,
            None,
            false,
            &io,
        )
        .unwrap();
        assert_eq!(result.exit_code, 0);
        let result = parsed(&result);
        assert_eq!(s(&result, "outcome"), "recovered");
        let detail = get(&result, "recovery");
        assert_eq!(s(detail, "status"), "repaired");
        let backup = Path::new(s(detail, "backup_path"));
        assert!(backup.is_absolute());
        assert_eq!(fs::read(backup.join("tasks.bytes")).unwrap(), raw);
        let view = ledger::read(root.path()).unwrap().unwrap();
        assert_eq!(
            fs::read(root.path().join("state/tasks.json")).unwrap(),
            view.projection_bytes().unwrap()
        );
        assert_eq!(s(&result, "revision"), view.revision());
    }
}

#[test]
fn aborted_corrupt_source_keeps_archive_path_and_strict_cli_configuration_policy() {
    let root = seeded();
    pending(root.path());
    fs::write(root.path().join("state/tasks.json"), "corrupt source").unwrap();
    fs::write(
        root.path().join("coven.toml"),
        "[runtime]\nmultiplexer='wrong'",
    )
    .unwrap();
    assert_eq!(
        code(recover_arguments(
            root.path(),
            RecoveryMode::AbortImport,
            None,
            true
        )),
        "unsupported_multiplexer"
    );
    assert!(root.path().join(ledger::DIRECTORY).exists());
    fs::remove_file(root.path().join("coven.toml")).unwrap();
    let result =
        parsed(&recover_arguments(root.path(), RecoveryMode::AbortImport, None, false).unwrap());
    assert_eq!(s(&result, "outcome"), "aborted");
    assert_eq!(s(&result, "authority"), "snapshot-v1");
    assert!(Path::new(s(&result, "preserved_artifacts")).is_dir());
    assert!(result.as_object().unwrap().get("warnings").is_none());
    assert!(!root.path().join(ledger::DIRECTORY).exists());
}

#[test]
fn archive_uncertainty_and_late_render_failure_preserve_real_rename() {
    let root = seeded();
    pending(root.path());
    let io = StateIo {
        faults: [
            ("archive_uncertain", Fault::Uncertain),
            ("render", Fault::Error),
        ]
        .into(),
        ..Default::default()
    };
    let result = recover(root.path(), RecoveryMode::AbortImport, None, false, &io).unwrap();
    assert_eq!(result.exit_code, 2);
    let result = parsed(&result);
    assert_eq!(s(&result, "outcome"), "durability_uncertain");
    let archived = Path::new(s(&result, "preserved_artifacts"));
    assert!(archived.is_absolute());
    assert!(archived.is_dir());
    assert!(!root.path().join(ledger::DIRECTORY).exists());
}

#[test]
fn activation_uncertainty_skips_every_secondary_and_retains_actual_sdk_view() {
    for resume in [false, true] {
        for failed in ["none", "report", "render"] {
            let root = seeded();
            let expected = if resume {
                pending(root.path())
            } else {
                snapshot(root.path()).unwrap().0
            };
            let io = StateIo {
                faults: [
                    ("activation_uncertain", Fault::Uncertain),
                    (failed, Fault::Error),
                ]
                .into(),
                ..Default::default()
            };
            let result = activate(root.path(), Some(&expected), resume, false, &io).unwrap();
            assert_eq!(result.exit_code, 2);
            let result = parsed(&result);
            let view = ledger::read(root.path()).unwrap().unwrap();
            assert_eq!(s(&result, "revision"), view.revision());
            assert_eq!(s(&result, "workspace_id"), view.workspace_id());
            assert_eq!(s(&result, "outcome"), "durability_uncertain");
            for name in ["health", "final_health", "projection", "dashboard"] {
                assert!(!io.trace.borrow().contains(&name));
            }
            assert!(!root.path().join("dashboard.md").exists());
        }
    }
}

#[test]
fn final_health_report_render_and_oversize_failures_never_erase_commit() {
    for (name, failure) in [
        ("final_health", Fault::Error),
        ("final_health", Fault::BadHealth),
        ("report", Fault::Error),
        ("report", Fault::Oversize),
        ("render", Fault::Error),
    ] {
        let root = fixture();
        let io = fault(name, failure);
        let result = activate(root.path(), Some("tasks-missing:v1"), false, false, &io).unwrap();
        assert_eq!(result.exit_code, 0);
        let result = parsed(&result);
        assert_eq!(s(&result, "outcome"), "committed");
        let view = ledger::read(root.path()).unwrap().unwrap();
        assert_eq!(s(&result, "revision"), view.revision());
        assert!(!get(&result, "warnings").as_array().unwrap().is_empty());
        if name == "final_health" {
            assert_eq!(s(get(&result, "projection"), "status"), "unreadable");
        }
    }
}

#[test]
fn secondaries_are_ordered_independent_and_do_not_emit_events() {
    for (name, failure) in [
        ("health", Fault::Error),
        ("projection", Fault::Error),
        ("projection", Fault::Uncertain),
        ("dashboard", Fault::Error),
    ] {
        let root = fixture();
        let io = fault(name, failure);
        let result = activate(root.path(), Some("tasks-missing:v1"), false, true, &io).unwrap();
        assert_eq!(result.exit_code, 0);
        let result = parsed(&result);
        assert!(!get(&result, "warnings").as_array().unwrap().is_empty());
        let trace = io.trace.borrow();
        assert!(trace.contains(&"dashboard"));
        assert!(
            trace.iter().position(|s| *s == "health").unwrap()
                < trace.iter().position(|s| *s == "dashboard").unwrap()
        );
        assert!(
            trace.iter().position(|s| *s == "dashboard").unwrap()
                < trace.iter().position(|s| *s == "final_health").unwrap()
        );
        assert!(!root.path().join("logs/events.jsonl").exists());
    }
}

#[test]
fn preserved_failure_is_empty_stdout_and_bounded_escaped_diagnostic() {
    let root = fixture();
    fs::create_dir_all(root.path().join(ledger::DIRECTORY)).unwrap();
    let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap();
    let failure = ledger::resume_import(root.path(), "tasks-missing:v1", &guard).unwrap_err();
    let result = import_failure(root.path(), failure).unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stdout.is_empty());
    assert_eq!(result.warnings.len(), 1);
    assert!(
        result.warnings[0]
            .starts_with("[ERROR] pending_import_requires_one_epoch; preserved artifacts:")
    );
    assert!(!result.warnings[0].contains("active"));
    let (_, quoted_path) = result.warnings[0]
        .split_once("; preserved artifacts: ")
        .unwrap();
    assert_eq!(
        values::parse(quoted_path.as_bytes(), Profile::LegacySnapshot)
            .unwrap()
            .as_str(),
        root.path().join(ledger::DIRECTORY).to_str()
    );
    let result = preserved_failure("static_code", Path::new("/tmp/\n\u{1b}\u{03bb}/partial"));
    assert!(result.warnings[0].is_ascii());
    assert!(!result.warnings[0].contains('\n'));
    assert!(!result.warnings[0].contains('\u{1b}'));
    assert!(result.warnings[0].contains("\\u03bb"));
}

#[test]
fn no_new_artifacts_failure_stays_normal_error() {
    let root = fixture();
    let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap();
    let failure = ledger::resume_import(root.path(), "tasks-missing:v1", &guard).unwrap_err();
    assert_eq!(
        code(import_failure(root.path(), failure)),
        "no_resumable_pending_import"
    );
}

#[test]
fn pending_diagnostic_resolves_published_and_fallback_paths_in_explicit_workspace() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("workspace-\u{03bb}-\n\u{1b}");
    fs::create_dir_all(root.join(ledger::DIRECTORY)).unwrap();
    let root = root.canonicalize().unwrap();
    assert_ne!(root, std::env::current_dir().unwrap());
    let relative = Path::new("state/task-ledger/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/manifest.json");
    fs::create_dir_all(root.join(relative).parent().unwrap()).unwrap();
    fs::write(root.join(relative), "preserved bytes").unwrap();
    for published in [None, Some(relative)] {
        let result = pending_failure(&root, "pending_import_synchronization_uncertain", published);
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
        assert_eq!(result.warnings.len(), 1);
        let message = &result.warnings[0];
        assert!(message.starts_with(
            "[ERROR] pending_import_synchronization_uncertain; preserved artifacts: "
        ));
        assert!(message.is_ascii());
        assert!(!message.chars().any(|c| c.is_ascii_control()));
        let (_, quoted_path) = message.split_once("; preserved artifacts: ").unwrap();
        let path = values::parse(quoted_path.as_bytes(), Profile::LegacySnapshot).unwrap();
        let path = Path::new(path.as_str().unwrap());
        assert_eq!(
            path,
            root.join(published.unwrap_or(Path::new(ledger::DIRECTORY)))
        );
        assert!(path.is_absolute());
        assert!(path.exists());
        assert!(message.contains(quote(root.to_str().unwrap()).trim_matches('"')));
    }
}

#[test]
fn repair_coordinator_mapping_covers_all_report_outcomes_and_optional_paths() {
    use ledger::{
        RepairDurability::{Confirmed, SynchronizationUncertain},
        RepairStatus::*,
    };
    let root = fixture();
    let backup = root
        .path()
        .join("state/task-recovery/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    for path in [None, Some(backup.as_path())] {
        for (status, durability, expected_status, expected_uncertain) in [
            (Current, Confirmed, Some("current"), false),
            (Repaired, Confirmed, Some("repaired"), false),
            (AuthorityUnconfirmed, SynchronizationUncertain, None, true),
            (BackupUnconfirmed, SynchronizationUncertain, None, true),
            (Repaired, SynchronizationUncertain, Some("repaired"), true),
        ] {
            let (detail, preserved, uncertain) = repair_delivery(status, durability, path);
            assert_eq!(uncertain, expected_uncertain);
            assert_eq!(
                preserved,
                if status == BackupUnconfirmed {
                    path
                } else {
                    None
                }
            );
            match expected_status {
                None => assert!(detail.is_none()),
                Some(expected) => {
                    let value = detail.unwrap().value().unwrap();
                    assert_eq!(s(&value, "status"), expected);
                    let expected_path = if status == Repaired { path } else { None };
                    assert_eq!(
                        get(&value, "backup_path").as_str(),
                        expected_path.and_then(Path::to_str)
                    );
                }
            }
        }
    }
}

#[test]
fn ascii_escaping_round_trips_controls_quotes_non_bmp_and_del() {
    let raw = "quote\" slash\\ \n\r\t\u{7f}\u{03bb}\u{1f98a}";
    let encoded = quote(raw);
    assert!(encoded.is_ascii());
    assert_eq!(
        values::parse(encoded.as_bytes(), Profile::LegacySnapshot)
            .unwrap()
            .as_str(),
        Some(raw)
    );
}

#[test]
#[cfg(unix)]
fn non_unicode_root_refused_before_even_lock_creation() {
    use std::os::unix::ffi::OsStringExt;
    let parent = tempfile::tempdir().unwrap();
    let path = parent.path().join(std::ffi::OsString::from_vec(vec![0xff]));
    fs::create_dir(&path).unwrap();
    fs::write(
        path.join("coven.json"),
        br#"{"name":"test","agents":["operator"]}"#,
    )
    .unwrap();
    assert_eq!(
        code(import_arguments(&path, "tasks-missing:v1", true)),
        "non_unicode_path_unsupported"
    );
    assert!(!path.join("state").exists());
}

#[test]
fn before_lock_fault_has_no_effect_and_admission_is_fresh() {
    let root = fixture();
    let io = fault("before_lock", Fault::Error);
    assert_eq!(
        code(activate(
            root.path(),
            Some("tasks-missing:v1"),
            false,
            true,
            &io
        )),
        "injected_state_io_failure"
    );
    assert!(!root.path().join("state").exists());
}

fn corrupt_config(root: &Path) {
    fs::write(root.join("coven.toml"), "[runtime]\nmultiplexer='wrong'").unwrap();
}
fn change_source(root: &Path) {
    fs::write(root.join("state/tasks.json"), "[]\n").unwrap();
}

#[test]
fn full_preflight_is_repeated_under_guard_for_every_writer() {
    for mode in [
        None,
        Some(RecoveryMode::ResumeImport),
        Some(RecoveryMode::RepairProjection),
        Some(RecoveryMode::AbortImport),
    ] {
        let root = seeded();
        let expected = match mode {
            Some(RecoveryMode::ResumeImport | RecoveryMode::AbortImport) => pending(root.path()),
            Some(RecoveryMode::RepairProjection) => {
                import(root.path());
                snapshot(root.path()).unwrap().0
            }
            None => snapshot(root.path()).unwrap().0,
        };
        let marker = fs::read(root.path().join(ledger::MARKER)).ok();
        let io = StateIo {
            hooks: [("held", corrupt_config as fn(&Path))].into(),
            ..Default::default()
        };
        let result = if let Some(mode) = mode {
            recover(
                root.path(),
                mode,
                if mode == RecoveryMode::ResumeImport {
                    Some(&expected)
                } else {
                    None
                },
                true,
                &io,
            )
        } else {
            activate(root.path(), Some(&expected), false, true, &io)
        };
        assert_eq!(code(result), "unsupported_multiplexer");
        assert_eq!(fs::read(root.path().join(ledger::MARKER)).ok(), marker);
        assert!(!io.trace.borrow().contains(&"archive"));
        assert!(!root.path().join("state/task-recovery").exists());
    }
}

#[test]
fn sdk_source_cas_is_not_replaced_by_the_preflight_observation() {
    for resume in [false, true] {
        let root = seeded();
        let expected = if resume {
            pending(root.path())
        } else {
            snapshot(root.path()).unwrap().0
        };
        let io = StateIo {
            hooks: [("before_sdk", change_source as fn(&Path))].into(),
            ..Default::default()
        };
        let result = activate(root.path(), Some(&expected), resume, true, &io);
        if resume {
            let result = result.unwrap();
            assert_eq!(result.exit_code, 1);
            assert!(result.stdout.is_empty());
            assert!(
                result.warnings[0].contains("pending_import_source_changed_or_contains_mutations")
            );
        } else {
            assert_eq!(code(result), "snapshot_revision_mismatch");
            assert!(!root.path().join(ledger::DIRECTORY).exists());
        }
        assert!(!root.path().join(ledger::MARKER).exists());
    }
}

fn publish_bad_marker(root: &Path) {
    fs::create_dir_all(root.join("state")).unwrap();
    fs::write(root.join(ledger::MARKER), "corrupt newly published marker").unwrap();
}
fn remove_marker(root: &Path) {
    fs::remove_file(root.join(ledger::MARKER)).unwrap();
}
fn remove_pending(root: &Path) {
    fs::remove_dir(root.join(ledger::DIRECTORY)).unwrap();
}
fn corrupt_control(root: &Path) {
    fs::create_dir_all(root.join("state")).unwrap();
    fs::write(
        root.join("state/runtime-launch-control.json"),
        "corrupt control",
    )
    .unwrap();
}

#[test]
fn captured_inspection_fences_marker_pending_and_actual_runtime_control() {
    let root = fixture();
    let io = StateIo {
        hooks: [("inspect_captured", publish_bad_marker as fn(&Path))].into(),
        ..Default::default()
    };
    assert_eq!(
        code(inspect(root.path(), true, &io)),
        "task_authority_changed_during_read"
    );

    let root = fixture();
    fs::create_dir_all(root.path().join(ledger::DIRECTORY)).unwrap();
    let io = StateIo {
        hooks: [("inspect_captured", remove_pending as fn(&Path))].into(),
        ..Default::default()
    };
    assert_eq!(
        code(inspect(root.path(), true, &io)),
        "task_authority_changed_during_read"
    );

    let root = fixture();
    import(root.path());
    let io = StateIo {
        hooks: [("inspect_captured", remove_marker as fn(&Path))].into(),
        ..Default::default()
    };
    assert_eq!(
        code(inspect(root.path(), true, &io)),
        "task_authority_changed_during_read"
    );

    let root = fixture();
    let io = StateIo {
        hooks: [("inspect_captured", corrupt_control as fn(&Path))].into(),
        ..Default::default()
    };
    assert!(inspect(root.path(), true, &io).is_err());
    assert!(!root.path().join("state/.workspace.lock").exists());
}

fn valid_control(root: &Path) {
    fs::create_dir_all(root.join("state/actions")).unwrap();
    let actual = crate::platform::workspace_location(root).unwrap();
    let control = object(vec![
        ("version", Value::integer("1").unwrap()),
        ("action", text("runtime_launch")),
        ("workspace_id", text("b".repeat(32))),
        ("epoch_id", text("c".repeat(32))),
        (
            "location",
            object(vec![
                ("root", text(actual.root)),
                (
                    "device",
                    Value::integer(&actual.device.to_string()).unwrap(),
                ),
                ("inode", Value::integer(&actual.inode.to_string()).unwrap()),
            ])
            .unwrap(),
        ),
        ("head", Value::Null),
        ("claims", object(vec![]).unwrap()),
    ])
    .unwrap();
    fs::write(
        root.join("state/runtime-launch-control.json"),
        control.canonical().unwrap(),
    )
    .unwrap();
}

#[test]
fn valid_runtime_authority_is_inspectable_without_changing_old_tasks_policy() {
    let root = seeded();
    valid_control(root.path());
    let report = parsed(&inspect_arguments(root.path(), false).unwrap());
    assert_eq!(s(&report, "authority"), "snapshot-v1");
    assert_eq!(
        code(crate::tasks::inspect_state(root.path())),
        "unsupported_native_authority"
    );
    assert!(!root.path().join("state/.workspace.lock").exists());
    import(root.path());
    let report = parsed(&inspect_arguments(root.path(), false).unwrap());
    assert_eq!(s(&report, "authority"), "task-ledger-v1");
    assert_eq!(s(get(&report, "projection"), "status"), "current");
}

#[test]
fn repair_and_abort_do_not_add_an_execution_gate() {
    for abort in [false, true] {
        let root = seeded();
        if abort {
            pending(root.path());
        } else {
            import(root.path());
        }
        fs::create_dir_all(root.path().join("state/execution")).unwrap();
        fs::write(root.path().join("state/execution/control.json"), "corrupt").unwrap();
        let result = recover_arguments(
            root.path(),
            if abort {
                RecoveryMode::AbortImport
            } else {
                RecoveryMode::RepairProjection
            },
            None,
            true,
        )
        .unwrap();
        assert_eq!(result.exit_code, 0);
    }
}

#[test]
fn known_uncertain_repair_details_preserved_paths_and_metadata_survive_render_failure() {
    let root = seeded();
    import(root.path());
    fs::write(root.path().join("state/tasks.json"), "corrupt").unwrap();
    let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap();
    let report = ledger::repair_projection(root.path(), &guard).unwrap();
    let backup = report.backup_path().unwrap();
    assert!(backup.join("tasks.bytes").exists());
    // Exercise the coordinator's uncertain delivery shapes with an actual SDK
    // selected view and actual SDK backup, never invented authority metadata.
    for status in [
        ledger::RepairStatus::AuthorityUnconfirmed,
        ledger::RepairStatus::BackupUnconfirmed,
        ledger::RepairStatus::Repaired,
    ] {
        let (detail, preserved, uncertain) = repair_delivery(
            status,
            ledger::RepairDurability::SynchronizationUncertain,
            Some(backup),
        );
        let io = fault("render", Fault::Error);
        let result = finish(
            root.path(),
            report.view(),
            "recovered",
            detail,
            preserved,
            uncertain,
            &guard,
            false,
            &io,
        );
        assert_eq!(result.exit_code, 2);
        let result = parsed(&result);
        assert_eq!(s(&result, "revision"), report.view().revision());
        assert_eq!(s(&result, "outcome"), "durability_uncertain");
        if preserved.is_some() {
            assert_eq!(s(&result, "preserved_artifacts"), backup.to_str().unwrap());
        }
        if detail.is_some() {
            assert_eq!(
                s(get(&result, "recovery"), "backup_path"),
                backup.to_str().unwrap()
            );
        }
        assert!(
            !io.trace
                .borrow()
                .iter()
                .any(|v| matches!(*v, "health" | "final_health" | "projection" | "dashboard"))
        );
    }
    let failure = preserved_failure("projection_replacement_failed", backup);
    assert_eq!(failure.exit_code, 1);
    assert!(failure.stdout.is_empty());
    assert!(failure.warnings[0].contains(backup.to_str().unwrap()));
}

#[test]
fn unicode_workspace_and_preserved_archive_are_ascii_and_lossless() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("workspace-\u{03bb}-\u{1f98a}-\n");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("coven.json"),
        br#"{"name":"test","agents":["operator"]}"#,
    )
    .unwrap();
    pending(&root);
    let report = parsed(&recover_arguments(&root, RecoveryMode::AbortImport, None, false).unwrap());
    let path = Path::new(s(&report, "preserved_artifacts"));
    assert!(path.is_dir());
    assert_eq!(path.parent().unwrap(), root.join("state"));
}

#[test]
fn deeply_nested_full_response_is_refused_before_lock_or_import_artifacts() {
    let root = seeded();
    let nested = format!(
        "[{{\"id\":\"t\",\"title\":\"T\",\"status\":\"pending\",\"extra\":{}0{}}}]",
        "[".repeat(62),
        "]".repeat(62)
    );
    fs::write(root.path().join("state/tasks.json"), nested).unwrap();
    let expected = snapshot(root.path()).unwrap().0;
    assert!(import_arguments(root.path(), &expected, true).is_err());
    assert!(!root.path().join("state/.workspace.lock").exists());
    assert!(!root.path().join(ledger::DIRECTORY).exists());
}

// Keep the source within 8 MiB and each string within 1 MiB. Nested, bounded
// zero arrays supply pretty indentation; Unicode supplies ASCII expansion.
fn boundary_tasks(mut unicode: usize, ascii: usize) -> Value {
    let mut chunks = Vec::new();
    for _ in 0..8 {
        let count = unicode.min(values::MAX_STRING / 2);
        chunks.push(text("\u{03bb}".repeat(count)));
        unicode -= count;
    }
    assert_eq!(unicode, 0);
    let zero = Value::integer("0").unwrap();
    let mut nested = Value::Array(
        (0..3)
            .map(|_| Value::Array(vec![zero.clone(); 50_000]))
            .collect(),
    );
    for _ in 0..40 {
        nested = Value::Array(vec![nested]);
    }
    Value::Array(vec![
        object(vec![
            ("id", text("t")),
            ("title", text("Task")),
            ("status", text("pending")),
            ("payload", Value::Array(chunks)),
            ("tail", text("a".repeat(ascii))),
            ("nested", nested),
        ])
        .unwrap(),
    ])
}

#[test]
fn actual_unicode_path_pretty_ascii_boundary_and_reserve_precede_publication() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent
        .path()
        .join(format!("workspace-{}-\n", "\u{03bb}".repeat(64)));
    fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    fs::write(
        root.join("coven.json"),
        br#"{"name":"test","agents":["operator"]}"#,
    )
    .unwrap();
    fs::create_dir(root.join("state")).unwrap();

    let base = render(
        &admission_value(&root, boundary_tasks(0, 0), None).unwrap(),
        false,
    )
    .unwrap()
    .len();
    let limit = values::MAX_OUTPUT - task_admission::RESPONSE_RESERVE;
    let unicode = (limit - base) / 6;
    let ascii = (limit - base) % 6;
    {
        let mut value = admission_value(&root, boundary_tasks(unicode, ascii), None).unwrap();
        let recovery = root.join(format!("state/task-recovery/{}", "0".repeat(32)));
        let paths = get(&value, "path_budget").as_array().unwrap();
        assert_eq!(paths[1].as_str(), recovery.to_str());
        assert_eq!(
            paths[0].as_str(),
            root.join(format!("state/task-ledger-aborted-{}", "0".repeat(32)))
                .to_str()
        );
        let pretty = render(&value, false).unwrap();
        assert_eq!(pretty.len(), limit);
        assert!(pretty.contains(&render(&text(recovery.to_str().unwrap()), true).unwrap()));
        assert!(quote(recovery.to_str().unwrap()).len() > recovery.to_str().unwrap().len());
        drop(pretty);
        let source = get(&value, "tasks").compact().unwrap();
        assert!(source.len() <= values::MAX_INPUT);
        TaskRecords::validate(values::parse(&source, Profile::LegacySnapshot).unwrap()).unwrap();
        drop(source);
        // Conservatively reserve six maximum-code warnings plus both forms of
        // recovery-path metadata, even though production emits fewer warnings.
        value
            .set_field(
                "warnings",
                warning_values(&vec![
                    format!(
                        "Ledger authority active; task projection stale: {}",
                        "x".repeat(128)
                    );
                    6
                ]),
            )
            .unwrap();
        value
            .set_field(
                "recovery",
                Detail::Projection("repaired", Some(&recovery))
                    .value()
                    .unwrap(),
            )
            .unwrap();
        value
            .set_field("preserved_artifacts", text(recovery.to_str().unwrap()))
            .unwrap();
        let full = render(&value, false).unwrap();
        assert!(full.len() > limit);
        assert!(full.len() - limit <= task_admission::RESPONSE_RESERVE);
        assert!(full.len() <= values::MAX_OUTPUT);
    }
    admit(&root, boundary_tasks(unicode, ascii), None).unwrap();

    // One additional ASCII byte crosses the pretty+ASCII admission boundary,
    // despite the compact source still fitting the supported input profile.
    let source = boundary_tasks(unicode, ascii + 1).compact().unwrap();
    assert!(source.len() <= values::MAX_INPUT);
    let expected = ledger::snapshot_revision(Some(&source));
    fs::write(root.join("state/tasks.json"), &source).unwrap();
    drop(source);
    let io = StateIo::default();
    assert_eq!(
        code(activate(&root, Some(&expected), false, true, &io)),
        "unsupported_task_response_profile"
    );
    assert!(io.trace.borrow().is_empty());
    assert!(!root.join("state/.workspace.lock").exists());
    assert!(!root.join(ledger::DIRECTORY).exists());
    assert!(!root.join(ledger::MARKER).exists());
}

#[cfg(unix)]
fn python_report(root: &Path, mode: &str, pretty: bool) -> String {
    use std::{
        io::{Read, Seek, SeekFrom},
        process::{Child, Command},
        time::Instant,
    };
    struct Reap(Child);
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut stdout = tempfile::tempfile().unwrap();
    let mut stderr = tempfile::tempfile().unwrap();
    let script = r#"
import json,sys
from pathlib import Path
from runner import coven
value=coven.task_inspection(Path(sys.argv[1]))
mode=sys.argv[2]
if mode!='inspect':
 value['outcome']='committed' if mode=='import' else 'recovered'
 if mode=='resume': value['recovery']={'import_resumed':True}
 if mode=='repair': value['recovery']={'status':'current','backup_path':None}
 value['warnings']=[]
print(json.dumps(value,ensure_ascii=True,indent=2) if sys.argv[3]=='pretty' else json.dumps(value,ensure_ascii=True,separators=(',',':')))
"#;
    let mut child = Reap(
        Command::new("python3")
            .args(["-c", script])
            .arg(root)
            .arg(mode)
            .arg(if pretty { "pretty" } else { "compact" })
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("COVEN_MULTIPLEXER", "")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdout(stdout.try_clone().unwrap())
            .stderr(stderr.try_clone().unwrap())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "Python state oracle exceeded watchdog"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    stdout.seek(SeekFrom::Start(0)).unwrap();
    stderr.seek(SeekFrom::Start(0)).unwrap();
    let mut out = String::new();
    let mut err = String::new();
    stdout.take(1_048_576).read_to_string(&mut out).unwrap();
    stderr.take(8192).read_to_string(&mut err).unwrap();
    assert!(status.success(), "{err}");
    out.strip_suffix('\n').unwrap().to_owned()
}

#[test]
#[cfg(unix)]
fn exact_normal_json_and_pretty_match_bounded_python_oracle() {
    for pretty in [false, true] {
        let root = seeded();
        let native = inspect_arguments(root.path(), !pretty).unwrap();
        assert_eq!(native.stdout, python_report(root.path(), "inspect", pretty));
        let expected = snapshot(root.path()).unwrap().0;
        let native = import_arguments(root.path(), &expected, !pretty).unwrap();
        assert_eq!(native.stdout, python_report(root.path(), "import", pretty));
        let native =
            recover_arguments(root.path(), RecoveryMode::RepairProjection, None, !pretty).unwrap();
        assert_eq!(native.stdout, python_report(root.path(), "repair", pretty));
        fs::remove_file(root.path().join(ledger::MARKER)).unwrap();
        let native = recover_arguments(
            root.path(),
            RecoveryMode::ResumeImport,
            Some(&expected),
            !pretty,
        )
        .unwrap();
        assert_eq!(native.stdout, python_report(root.path(), "resume", pretty));
    }
}
