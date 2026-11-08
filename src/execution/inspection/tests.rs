use super::*;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

fn enabled() -> Value {
    super::super::tests::enabled()
}

fn fixture(marker: bool, lock: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("state/execution")).unwrap();
    fs::write(
        dir.path().join("state/execution/control.json"),
        enabled().compact().unwrap(),
    )
    .unwrap();
    if marker {
        fs::write(
            dir.path().join("coven.json"),
            br#"{"name":"test","agents":["worker"]}"#,
        )
        .unwrap();
    }
    if lock {
        fs::write(
            dir.path().join("state/.workspace.lock"),
            b"retained lock metadata",
        )
        .unwrap();
    }
    dir
}

fn field<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    path.iter()
        .fold(value, |value, key| get(value, key).unwrap())
}

fn status(value: &Value) -> &str {
    field(value, &["execution", "status"]).as_str().unwrap()
}

fn parsed(output: TaskOutput) -> Value {
    assert_eq!(output.exit_code, 0);
    assert!(output.warnings.is_empty());
    assert!(output.stdout.is_ascii());
    assert!(!output.stdout.ends_with('\n'));
    values::parse(output.stdout.as_bytes(), Profile::Authority).unwrap()
}

struct Script {
    reads: RefCell<VecDeque<Result<Option<Value>>>>,
    read_count: Cell<usize>,
    locks: Cell<usize>,
    sources: Cell<usize>,
    lock_error: Option<Error>,
    source_error: Option<Error>,
}

impl Script {
    fn new(reads: Vec<Result<Option<Value>>>) -> Self {
        Self {
            reads: RefCell::new(reads.into()),
            read_count: Cell::new(0),
            locks: Cell::new(0),
            sources: Cell::new(0),
            lock_error: None,
            source_error: None,
        }
    }
}

impl Io for Script {
    fn read(&self, _root: &Path) -> Result<Option<(Value, Validated)>> {
        self.read_count.set(self.read_count.get() + 1);
        assert!(self.read_count.get() <= 4);
        self.reads
            .borrow_mut()
            .pop_front()
            .expect("unexpected replay read")?
            .map(|control| Ok((control.clone(), validated(&control)?)))
            .transpose()
    }
    fn lock(&self, root: &Path) -> Result<Option<WorkspaceGuard>> {
        self.locks.set(self.locks.get() + 1);
        if let Some(error) = &self.lock_error {
            return Err(error.clone());
        }
        existing_lock(root, Duration::ZERO)
    }
    fn source(
        &self,
        root: &Path,
        snapshot: &Value,
        guard: &WorkspaceGuard,
        work: &mut Work,
    ) -> Result<Value> {
        self.sources.set(self.sources.get() + 1);
        guard.validate_for(root)?;
        if let Some(error) = &self.source_error {
            return Err(error.clone());
        }
        // Successful coordination always uses the real borrowed-guard capture.
        super::super::source::capture(root, snapshot, guard, work)
    }
}

#[test]
fn nonactive_states_are_minimal_and_never_consult_lock_or_source() {
    let root = tempfile::tempdir().unwrap();
    for (read, expected) in [
        (Ok(None), "disabled"),
        (Err(Error("pending_execution_authority")), "pending"),
        (Ok(Some(Value::Null)), "unavailable"),
        (Err(Error("unsafe_archive_path")), "unavailable"),
    ] {
        let io = Script::new(vec![read]);
        let result = inspect(root.path(), &io, &mut Work::default()).unwrap();
        assert_eq!(status(&result), expected);
        assert_eq!(result.as_object().unwrap().entries().len(), 4);
        assert_eq!(io.read_count.get(), 1);
        assert_eq!(io.locks.get(), 0);
        assert_eq!(io.sources.get(), 0);
    }
}

#[test]
fn second_read_is_hard_and_precedes_lock() {
    let root = tempfile::tempdir().unwrap();
    let io = Script::new(vec![
        Ok(Some(enabled())),
        Err(Error("input_or_storage_unavailable")),
    ]);
    assert_eq!(
        inspect(root.path(), &io, &mut Work::default())
            .unwrap_err()
            .code(),
        "input_or_storage_unavailable"
    );
    assert_eq!(io.read_count.get(), 2);
    assert_eq!(io.locks.get(), 0);
}

#[test]
fn second_read_disabled_transition_is_minimal() {
    let root = tempfile::tempdir().unwrap();
    let io = Script::new(vec![Ok(Some(enabled())), Ok(None)]);
    assert_eq!(
        status(&inspect(root.path(), &io, &mut Work::default()).unwrap()),
        "disabled"
    );
    assert_eq!(io.locks.get(), 0);
}

#[test]
fn missing_lock_keeps_second_view_without_creating_anything() {
    let root = fixture(false, false);
    let mut second = enabled();
    second
        .set_field(
            "extensions",
            object(vec![("second", Value::Bool(true))]).unwrap(),
        )
        .unwrap();
    let expected = validated(&second).unwrap().replayed.revision;
    let io = Script::new(vec![Ok(Some(enabled())), Ok(Some(second))]);
    let report = inspect(root.path(), &io, &mut Work::default()).unwrap();
    assert_eq!(
        field(&report, &["execution", "revision"]).as_str(),
        Some(expected.as_str())
    );
    assert_eq!(
        field(&report, &["source_assessment", "error"]).as_str(),
        Some(NO_LOCK)
    );
    assert!(matches!(
        field(&report, &["source_assessment", "coordinated"]),
        Value::Bool(false)
    ));
    assert_eq!(io.read_count.get(), 2);
    assert_eq!(io.sources.get(), 0);
    assert!(!root.path().join("state/.workspace.lock").exists());
    assert!(!root.path().join("coven.json").exists());
}

#[test]
fn locked_reread_disabled_transition_never_captures_source() {
    let root = fixture(true, true);
    let io = Script::new(vec![Ok(Some(enabled())), Ok(Some(enabled())), Ok(None)]);
    assert_eq!(
        status(&inspect(root.path(), &io, &mut Work::default()).unwrap()),
        "disabled"
    );
    assert_eq!(io.read_count.get(), 3);
    assert_eq!(io.sources.get(), 0);
}

#[test]
fn failed_locked_reread_reclassifies_nonactive_once() {
    let root = fixture(true, true);
    for (fallback, expected) in [
        (Ok(None), "disabled"),
        (Err(Error("pending_execution_authority")), "pending"),
        (Ok(Some(Value::Null)), "unavailable"),
    ] {
        let io = Script::new(vec![
            Ok(Some(enabled())),
            Ok(Some(enabled())),
            Err(Error("input_or_storage_unavailable")),
            fallback,
        ]);
        assert_eq!(
            status(&inspect(root.path(), &io, &mut Work::default()).unwrap()),
            expected
        );
        assert_eq!(io.read_count.get(), 4);
        assert_eq!(io.sources.get(), 0);
    }
}

#[test]
fn failed_locked_reread_active_fallback_keeps_prior_view_not_fallback_revision() {
    let root = fixture(true, true);
    let original = enabled();
    let revision = validated(&original).unwrap().replayed.revision;
    let mut fresh = enabled();
    fresh
        .set_field(
            "extensions",
            object(vec![("fresh", Value::Bool(true))]).unwrap(),
        )
        .unwrap();
    assert_ne!(validated(&fresh).unwrap().replayed.revision, revision);
    let io = Script::new(vec![
        Ok(Some(enabled())),
        Ok(Some(original)),
        Err(Error("invalid_execution_fields")),
        Ok(Some(fresh)),
    ]);
    let report = inspect(root.path(), &io, &mut Work::default()).unwrap();
    assert_eq!(
        field(&report, &["execution", "revision"]).as_str(),
        Some(revision.as_str())
    );
    assert_eq!(
        field(&report, &["source_assessment", "error"]).as_str(),
        Some("Coordinated source assessment unavailable: invalid_execution_fields")
    );
    assert!(matches!(
        field(&report, &["source_assessment", "revision"]),
        Value::Null
    ));
    assert_eq!(io.read_count.get(), 4);
}

#[test]
fn source_diagnostics_keep_locked_view_and_do_not_trigger_fallback() {
    let root = fixture(true, true);
    for code in [
        "unsupported_multiplexer",
        "unsupported_nonfinite_json",
        "unsupported_or_invalid_json_scalar",
        "pending_task_import",
        "invalid_execution_fields",
    ] {
        let mut locked = enabled();
        locked
            .set_field(
                "extensions",
                object(vec![("locked", Value::Bool(true))]).unwrap(),
            )
            .unwrap();
        let revision = validated(&locked).unwrap().replayed.revision;
        let mut io = Script::new(vec![
            Ok(Some(enabled())),
            Ok(Some(enabled())),
            Ok(Some(locked)),
        ]);
        io.source_error = Some(Error(code));
        let report = inspect(root.path(), &io, &mut Work::default()).unwrap();
        assert_eq!(
            field(&report, &["execution", "revision"]).as_str(),
            Some(revision.as_str())
        );
        assert_eq!(
            field(&report, &["source_assessment", "error"]).as_str(),
            Some(format!("Current source could not be assessed: {code}").as_str())
        );
        assert!(matches!(
            field(&report, &["source_assessment", "coordinated"]),
            Value::Bool(false)
        ));
        assert_eq!(io.read_count.get(), 3);
        assert_eq!(io.sources.get(), 1);
    }
}

#[test]
fn successful_borrowed_capture_is_the_only_coordinated_result() {
    let root = fixture(true, true);
    let io = Script::new(vec![
        Ok(Some(enabled())),
        Ok(Some(enabled())),
        Ok(Some(enabled())),
    ]);
    let mut work = Work::default();
    let report = inspect(root.path(), &io, &mut work).unwrap();
    assert!(matches!(
        field(&report, &["source_assessment", "coordinated"]),
        Value::Bool(true)
    ));
    assert!(matches!(
        field(&report, &["source_assessment", "error"]),
        Value::Null
    ));
    assert_eq!(
        field(&report, &["source_assessment", "revision"]).as_str(),
        Some("tasks-missing:v1")
    );
    assert_eq!(io.sources.get(), 1);
    assert!(work.0 > 0);
}

#[test]
fn fatal_source_limits_do_not_trigger_fallback_or_erase_history() {
    let root = fixture(true, true);
    for code in [
        "unsupported_execution_work_profile",
        "unsupported_json_depth_profile",
        "graph_size_limit",
        "execution_source_size_limit",
        "handoff_size_limit",
    ] {
        let mut io = Script::new(vec![
            Ok(Some(enabled())),
            Ok(Some(enabled())),
            Ok(Some(enabled())),
        ]);
        io.source_error = Some(Error(code));
        assert_eq!(
            inspect(root.path(), &io, &mut Work::default())
                .unwrap_err()
                .code(),
            code
        );
        assert_eq!(io.read_count.get(), 3);
    }
    let io = Script::new(vec![
        Ok(Some(enabled())),
        Ok(Some(enabled())),
        Ok(Some(enabled())),
    ]);
    assert_eq!(
        inspect(root.path(), &io, &mut Work(MAX_WORK))
            .unwrap_err()
            .code(),
        "unsupported_execution_work_profile"
    );
    assert_eq!(io.read_count.get(), 3);
}

#[test]
fn fatal_initial_and_fallback_profiles_never_become_success_envelopes() {
    let root = tempfile::tempdir().unwrap();
    let io = Script::new(vec![Err(Error("handoff_size_limit"))]);
    assert_eq!(
        inspect(root.path(), &io, &mut Work::default())
            .unwrap_err()
            .code(),
        "handoff_size_limit"
    );
    let mut io = Script::new(vec![
        Ok(Some(enabled())),
        Ok(Some(enabled())),
        Err(Error("unsupported_json_node_profile")),
    ]);
    io.lock_error = Some(Error("workspace_lock_busy"));
    assert_eq!(
        inspect(root.path(), &io, &mut Work::default())
            .unwrap_err()
            .code(),
        "unsupported_json_node_profile"
    );
    assert_eq!(io.read_count.get(), 3);
}

#[test]
fn all_known_native_profiles_are_classified_explicitly_not_by_unsupported_prefix() {
    for code in [
        "handoff_size_limit",
        "execution_cancellation_capacity_exceeded",
        "execution_intent_size_limit",
        "execution_source_size_limit",
        "graph_size_limit",
        "graph_edge_limit",
        "unsupported_json_size_profile",
        "unsupported_json_depth_profile",
        "unsupported_json_node_profile",
        "unsupported_json_array_profile",
        "unsupported_json_object_profile",
        "unsupported_json_string_profile",
        "unsupported_json_output_profile",
        "unsupported_integer_profile",
        "unsupported_number_token_profile",
        "unsupported_execution_history_profile",
        "unsupported_execution_timestamp_profile",
        "unsupported_execution_work_profile",
        "unsupported_action_input_profile",
        "unsupported_action_encoding_profile",
        "unsupported_action_work_profile",
        "unsupported_checkpoint_input_profile",
        "unsupported_checkpoint_scan_profile",
        "unsupported_checkpoint_toml_scalar_profile",
        "unsupported_checkpoint_workspace_input_profile",
        "unsupported_ledger_directory_profile",
        "unsupported_ledger_frame_size",
        "unsupported_ledger_history_bytes",
        "unsupported_ledger_history_profile",
        "unsupported_ledger_receipt_bytes",
        "unsupported_ledger_task_count",
        "unsupported_projection_profile",
        "unsupported_recovery_size",
        "unsupported_replacement_size",
        "unsupported_task_response_profile",
        "unsupported_platform",
        "unsupported_named_user_expansion",
        "non_unicode_path_unsupported",
        "float_encoding_failed",
        "report_encoding_failed",
    ] {
        assert!(profile_failure(Error(code)), "{code}");
    }
    for code in [
        "unsupported_multiplexer",
        "unsupported_nonfinite_json",
        "unsupported_or_invalid_json_scalar",
        "unsupported_workspace_version",
        "unsupported_ledger_version",
        "unsupported_execution_version",
        "unsupported_execution_operation",
        "unsupported_execution_source_identity",
        "unsupported_graph_source_identity",
        "unsupported_graph_version",
        "invalid_utf8",
        "workspace_lock_busy",
        "input_or_storage_unavailable",
    ] {
        assert!(!profile_failure(Error(code)), "{code}");
    }
}

#[test]
fn actual_files_distinguish_disabled_pending_and_present_invalid_control() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        status(&parsed(inspect_arguments(root.path(), false).unwrap())),
        "disabled"
    );
    assert!(!root.path().join("state").exists());
    fs::create_dir_all(root.path().join("state/execution")).unwrap();
    assert_eq!(
        status(&parsed(inspect_arguments(root.path(), true).unwrap())),
        "pending"
    );
    for raw in [
        b"null".as_slice(),
        b"{}",
        b"",
        b"[",
        b"{\"schema_version\":99}",
    ] {
        fs::write(root.path().join("state/execution/control.json"), raw).unwrap();
        assert_eq!(
            status(&parsed(inspect_arguments(root.path(), true).unwrap())),
            "unavailable"
        );
    }
    fs::write(
        root.path().join("state/execution/control.json"),
        vec![b' '; values::MAX_INPUT + 1],
    )
    .unwrap();
    assert_eq!(
        inspect_arguments(root.path(), true).err().unwrap().code(),
        "handoff_size_limit"
    );
    assert!(!root.path().join("state/.workspace.lock").exists());
}

#[test]
fn actual_marker_free_lock_and_corrupt_source_are_bounded_diagnostics() {
    let root = fixture(false, true);
    let report = parsed(inspect_arguments(root.path(), true).unwrap());
    assert_eq!(status(&report), "active");
    assert_eq!(
        field(&report, &["source_assessment", "error"]).as_str(),
        Some("Coordinated source assessment unavailable: invalid_handoff_workspace")
    );
    fs::write(root.path().join("coven.json"), b"{}").unwrap();
    let report = parsed(inspect_arguments(root.path(), true).unwrap());
    assert_eq!(status(&report), "active");
    assert!(
        field(&report, &["source_assessment", "error"])
            .as_str()
            .unwrap()
            .starts_with("Current source could not be assessed: ")
    );
}

#[test]
fn real_busy_lock_uses_short_private_timeout_without_touching_lock() {
    struct Short;
    impl Io for Short {
        fn lock(&self, root: &Path) -> Result<Option<WorkspaceGuard>> {
            existing_lock(root, Duration::from_millis(1))
        }
    }
    let root = fixture(true, true);
    let _held = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
    let report = inspect(root.path(), &Short, &mut Work::default()).unwrap();
    assert_eq!(status(&report), "active");
    assert_eq!(
        field(&report, &["source_assessment", "error"]).as_str(),
        Some("Coordinated source assessment unavailable: workspace_lock_busy")
    );
    assert_eq!(
        fs::read(root.path().join("state/.workspace.lock")).unwrap(),
        b"retained lock metadata"
    );
}

#[test]
fn selectors_and_filter_preserve_preflight_order() {
    let missing = Path::new("/definitely-missing-coven-inspection-workspace");
    let args = [missing.to_owned(), PathBuf::from("bad")];
    assert_eq!(
        attempt_inspect_arguments(&args, true).err().unwrap().code(),
        "invalid_execution_attempt_id"
    );
    assert_eq!(
        graph_export_arguments(&args, true).err().unwrap().code(),
        "invalid_execution_graph_revision"
    );
    assert_eq!(
        attempt_list_arguments(missing, Some("bad id"), true)
            .err()
            .unwrap()
            .code(),
        "input_or_storage_unavailable"
    );
    let root = tempfile::tempdir().unwrap();
    let args = [root.path().to_owned(), PathBuf::from("f".repeat(32))];
    assert_eq!(
        status(&parsed(attempt_inspect_arguments(&args, true).unwrap())),
        "disabled"
    );
    assert_eq!(
        attempt_list_arguments(root.path(), Some("bad id"), true)
            .err()
            .unwrap()
            .code(),
        "invalid_execution_identifier"
    );
    assert_eq!(
        graph_export_arguments(&args, true).err().unwrap().code(),
        "execution_upgrade_required"
    );
    // A single operand is always the selector, even when it names a workspace.
    fs::write(root.path().join("coven.json"), b"{}").unwrap();
    assert_eq!(
        attempt_inspect_arguments(&[root.path().to_owned()], true)
            .err()
            .unwrap()
            .code(),
        "invalid_execution_attempt_id"
    );
}

#[test]
fn graph_inspect_arity_inactive_v1_and_empty_head_precedence() {
    let root = fixture(true, false);
    let items = [
        root.path().to_owned(),
        PathBuf::from("bad"),
        PathBuf::from("extra"),
    ];
    assert_eq!(
        graph_inspect_arguments(&items, true).err().unwrap().code(),
        "invalid_execution_graph_inspect_arguments"
    );
    assert_eq!(
        graph_inspect_arguments(&items[..2], true)
            .err()
            .unwrap()
            .code(),
        "execution_upgrade_required"
    );
    fs::write(root.path().join("state/execution/control.json"), b"null").unwrap();
    assert_eq!(
        status(&parsed(graph_inspect_arguments(&items[..2], true).unwrap())),
        "unavailable"
    );
    let control = super::super::tests::upgraded(enabled());
    fs::write(
        root.path().join("state/execution/control.json"),
        control.compact().unwrap(),
    )
    .unwrap();
    for items in [
        vec![root.path().to_owned()],
        vec![root.path().to_owned(), PathBuf::new()],
    ] {
        let report = parsed(graph_inspect_arguments(&items, true).unwrap());
        assert!(matches!(get(&report, "graph").unwrap(), Value::Null));
        assert_eq!(report.as_object().unwrap().entries().len(), 4);
    }
}

#[test]
fn pretty_compact_ascii_and_final_admission_share_work() {
    let value = object(vec![("unicode", string("\u{2603}\u{1f98a}"))]).unwrap();
    let mut work = Work::default();
    let pretty = output(&value, false, &mut work).unwrap();
    assert_eq!(
        pretty.stdout,
        "{\n  \"unicode\": \"\\u2603\\ud83e\\udd8a\"\n}"
    );
    assert_eq!(work.0, pretty.stdout.len());
    let compact = output(&value, true, &mut work).unwrap();
    assert_eq!(work.0, pretty.stdout.len() + compact.stdout.len());
    assert_eq!(
        output(&value, true, &mut Work(MAX_WORK))
            .err()
            .unwrap()
            .code(),
        "unsupported_execution_work_profile"
    );
    let large = Value::Array(vec![string("\u{7f}".repeat(values::MAX_STRING)); 6]);
    assert_eq!(
        output(&large, true, &mut Work::default())
            .err()
            .unwrap()
            .code(),
        "unsupported_json_output_profile"
    );
}

#[cfg(unix)]
#[test]
fn inspection_preserves_bytes_modes_inodes_and_mtimes_and_rejects_unsafe_lock() {
    use std::os::unix::fs::{MetadataExt, symlink};
    fn inventory(root: &Path) -> Vec<(PathBuf, u64, u32, i64, i64, Vec<u8>)> {
        let mut paths = vec![root.to_owned()];
        let mut result = Vec::new();
        while let Some(path) = paths.pop() {
            let meta = fs::symlink_metadata(&path).unwrap();
            if meta.is_dir() {
                paths.extend(fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
            }
            let bytes = if meta.is_file() {
                fs::read(&path).unwrap()
            } else {
                Vec::new()
            };
            result.push((
                path,
                meta.ino(),
                meta.mode(),
                meta.mtime(),
                meta.mtime_nsec(),
                bytes,
            ));
        }
        result.sort();
        result
    }
    let root = fixture(true, true);
    let before = inventory(root.path());
    for json in [true, false] {
        parsed(inspect_arguments(root.path(), json).unwrap());
    }
    assert_eq!(inventory(root.path()), before);
    fs::remove_file(root.path().join("state/.workspace.lock")).unwrap();
    symlink(
        root.path().join("coven.json"),
        root.path().join("state/.workspace.lock"),
    )
    .unwrap();
    let report = parsed(inspect_arguments(root.path(), true).unwrap());
    assert_eq!(
        field(&report, &["source_assessment", "error"]).as_str(),
        Some("Coordinated source assessment unavailable: unsafe_archive_path")
    );
    assert!(root.path().join("state/.workspace.lock").is_symlink());
}

#[cfg(unix)]
#[test]
fn non_unicode_root_is_rejected_before_execution_read() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(OsString::from_vec(vec![0xff]));
    fs::create_dir(&path).unwrap();
    assert_eq!(
        inspect_arguments(&path, true).err().unwrap().code(),
        "non_unicode_path_unsupported"
    );
    assert!(!path.join("state").exists());
}

#[test]
fn attempt_wrappers_filter_only_attempts_and_select_cancelled_or_prepared() {
    let root = fixture(false, false);
    let mut control = enabled();
    let first = "ffffffffffffffffffffffffffffffff";
    let second = "11111111111111111111111111111111";
    super::super::tests::enroll(&mut control);
    super::super::tests::prepare(&mut control, first, Value::Null, 1);
    super::super::tests::cancel(&mut control, first, "dddddddddddddddddddddddddddddddd");
    super::super::tests::prepare(&mut control, second, string(first), 2);
    fs::write(
        root.path().join("state/execution/control.json"),
        control.compact().unwrap(),
    )
    .unwrap();
    for filter in [None, Some("task"), Some("other")] {
        let report = parsed(attempt_list_arguments(root.path(), filter, true).unwrap());
        assert!(report.as_object().unwrap().get("enrollments").is_none());
        assert_eq!(
            get(&report, "attempts").unwrap().as_array().unwrap().len(),
            if filter == Some("other") { 0 } else { 2 }
        );
        assert_eq!(status(&report), "active");
    }
    for (id, expected) in [(first, "cancelled_before_submission"), (second, "prepared")] {
        let report = parsed(
            attempt_inspect_arguments(&[root.path().to_owned(), PathBuf::from(id)], false).unwrap(),
        );
        assert_eq!(
            field(&report, &["attempt", "state"]).as_str(),
            Some(expected)
        );
        assert!(report.as_object().unwrap().get("attempts").is_none());
        assert!(report.as_object().unwrap().get("enrollments").is_none());
    }
    assert_eq!(
        attempt_inspect_arguments(
            &[root.path().to_owned(), PathBuf::from("0".repeat(32))],
            true
        )
        .err()
        .unwrap()
        .code(),
        "unknown_execution_attempt_id"
    );
    // Even an invalid filter cannot bypass full replay/profile admission.
    fs::write(
        root.path().join("state/execution/control.json"),
        vec![b' '; values::MAX_INPUT + 1],
    )
    .unwrap();
    assert_eq!(
        attempt_list_arguments(root.path(), Some("invalid id"), true)
            .err()
            .unwrap()
            .code(),
        "handoff_size_limit"
    );
}

#[test]
fn task_filter_is_borrowed_identifier_text_not_a_retained_json_string() {
    let root = tempfile::tempdir().unwrap();
    let large = "a".repeat(values::MAX_STRING + 1);
    for filter in [large.as_str(), "_", "--", "Case_ID", "-1e2"] {
        let report = parsed(attempt_list_arguments(root.path(), Some(filter), true).unwrap());
        assert_eq!(status(&report), "disabled");
        assert!(report.as_object().unwrap().get("attempts").is_none());
    }
    for filter in ["", "a/b", "a b", "\u{732b}"] {
        assert_eq!(
            attempt_list_arguments(root.path(), Some(filter), true)
                .err()
                .unwrap()
                .code(),
            "invalid_execution_identifier",
        );
    }
    assert!(fs::read_dir(root.path()).unwrap().next().is_none());
}

#[test]
fn graph_export_uses_only_complete_frozen_authority_despite_corrupt_current_files() {
    // Reuse the projection agent's frozen, fully validated v2 graph corpus.
    let vectors = values::parse(
        include_bytes!("projection/vectors.json"),
        Profile::Authority,
    )
    .unwrap();
    let control = get(vectors.as_array().unwrap().last().unwrap(), "control").unwrap();
    let retained = validated(control).unwrap();
    let metadata = retained.graph.as_ref().unwrap();
    let revision = text(get(metadata, "current_graph_revision").unwrap()).unwrap();
    let graph = get(get(metadata, "graphs").unwrap(), revision).unwrap();
    let root = fixture(false, false);
    fs::write(
        root.path().join("state/execution/control.json"),
        control.compact().unwrap(),
    )
    .unwrap();
    fs::write(root.path().join("coven.json"), b"corrupt config").unwrap();
    fs::write(root.path().join("state/tasks.json"), b"not JSON").unwrap();
    fs::write(root.path().join("state/task-authority.json"), b"not JSON").unwrap();
    fs::create_dir(root.path().join("state/.workspace.lock")).unwrap();
    fs::create_dir(root.path().join("checkpoints")).unwrap();
    fs::write(root.path().join("checkpoints/plan-approval.md"), [0xff]).unwrap();
    let args = [root.path().to_owned(), PathBuf::from(revision)];
    for json in [false, true] {
        let report = parsed(graph_export_arguments(&args, json).unwrap());
        assert_eq!(report.as_object().unwrap().entries().len(), 5);
        assert_eq!(
            get(&report, "manifest").unwrap().canonical().unwrap(),
            get(graph, "manifest").unwrap().canonical().unwrap()
        );
        assert_eq!(
            get(&report, "graph_digest").unwrap().as_str(),
            get(graph, "digest").unwrap().as_str()
        );
        assert_eq!(
            get(&report, "execution_revision").unwrap().as_str(),
            Some(retained.replayed.revision.as_str())
        );
    }
    for selector in [Some(revision), Some(""), None] {
        let mut items = vec![root.path().to_owned()];
        if let Some(selector) = selector {
            items.push(PathBuf::from(selector));
        }
        let report = parsed(graph_inspect_arguments(&items, true).unwrap());
        assert_eq!(
            field(&report, &["graph", "graph_revision"]).as_str(),
            Some(revision)
        );
        assert_eq!(report.as_object().unwrap().entries().len(), 4);
    }
    fs::remove_file(root.path().join("coven.json")).unwrap();
    parsed(graph_export_arguments(&args, true).unwrap());
    assert_eq!(
        graph_export_arguments(
            &[root.path().to_owned(), PathBuf::from("0".repeat(32))],
            true
        )
        .err()
        .unwrap()
        .code(),
        "unknown_execution_graph_revision"
    );
    // Export validates the complete history, not just the selected manifest.
    let mut invalid = control.clone();
    invalid
        .set_field("generation", integer(0).unwrap())
        .unwrap();
    fs::write(
        root.path().join("state/execution/control.json"),
        invalid.compact().unwrap(),
    )
    .unwrap();
    assert!(graph_export_arguments(&args, true).is_err());
}

#[test]
fn actual_locked_reread_pending_corrupt_and_disabled_transitions() {
    struct DiskTransition {
        count: Cell<usize>,
        raw: Option<&'static [u8]>,
    }
    impl Io for DiskTransition {
        fn read(&self, root: &Path) -> Result<Option<(Value, Validated)>> {
            self.count.set(self.count.get() + 1);
            if self.count.get() == 3 {
                let control = root.join("state/execution/control.json");
                match self.raw {
                    Some(raw) => fs::write(control, raw).unwrap(),
                    None => {
                        fs::remove_file(control).unwrap();
                        fs::remove_dir(root.join("state/execution")).unwrap();
                    }
                }
            }
            read_typed(root)
        }
        fn source(&self, _: &Path, _: &Value, _: &WorkspaceGuard, _: &mut Work) -> Result<Value> {
            panic!("nonactive authority must not capture source");
        }
    }
    for (raw, expected, reads) in [
        (None, "disabled", 3),
        (Some(b"null".as_slice()), "unavailable", 4),
    ] {
        let root = fixture(true, true);
        let io = DiskTransition {
            count: Cell::new(0),
            raw,
        };
        assert_eq!(
            status(&inspect(root.path(), &io, &mut Work::default()).unwrap()),
            expected
        );
        assert_eq!(io.count.get(), reads);
    }
    struct Pending(Cell<usize>);
    impl Io for Pending {
        fn read(&self, root: &Path) -> Result<Option<(Value, Validated)>> {
            self.0.set(self.0.get() + 1);
            if self.0.get() == 3 {
                fs::remove_file(root.join("state/execution/control.json")).unwrap();
            }
            read_typed(root)
        }
    }
    let root = fixture(true, true);
    let io = Pending(Cell::new(0));
    assert_eq!(
        status(&inspect(root.path(), &io, &mut Work::default()).unwrap()),
        "pending"
    );
    assert_eq!(io.0.get(), 4);
}

#[cfg(unix)]
#[test]
fn graph_selector_utf8_is_checked_after_inactive_and_v1_handling() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let root = fixture(true, false);
    let items = [
        root.path().to_owned(),
        PathBuf::from(OsString::from_vec(vec![0xff])),
    ];
    assert_eq!(
        graph_inspect_arguments(&items, true).err().unwrap().code(),
        "execution_upgrade_required"
    );
    fs::write(root.path().join("state/execution/control.json"), b"null").unwrap();
    assert_eq!(
        status(&parsed(graph_inspect_arguments(&items, true).unwrap())),
        "unavailable"
    );
    let control = super::super::tests::upgraded(enabled());
    fs::write(
        root.path().join("state/execution/control.json"),
        control.compact().unwrap(),
    )
    .unwrap();
    assert_eq!(
        graph_inspect_arguments(&items, true).err().unwrap().code(),
        "non_unicode_path_unsupported"
    );
}

#[derive(Default)]
struct CaptureCounts {
    reads: Cell<usize>,
    sources: Cell<usize>,
}

impl Io for CaptureCounts {
    fn read(&self, root: &Path) -> Result<Option<(Value, Validated)>> {
        self.reads.set(self.reads.get() + 1);
        read_typed(root)
    }
    fn source(
        &self,
        root: &Path,
        snapshot: &Value,
        guard: &WorkspaceGuard,
        work: &mut Work,
    ) -> Result<Value> {
        self.sources.set(self.sources.get() + 1);
        super::super::source::capture(root, snapshot, guard, work)
    }
}

fn assert_capture_profile(root: &Path, expected: &str) {
    let io = CaptureCounts::default();
    let mut work = Work::default();
    assert_eq!(inspect(root, &io, &mut work).unwrap_err().code(), expected);
    assert_eq!(
        io.reads.get(),
        3,
        "fatal source failure must not reclassify authority"
    );
    assert_eq!(io.sources.get(), 1);
    assert_eq!(
        work.0, 0,
        "preflight refusal must precede source/projection work"
    );
    assert_eq!(
        inspect_arguments(root, true).err().unwrap().code(),
        expected
    );
    // The failing capture also releases, rather than replaces, the existing lock.
    let _guard = WorkspaceGuard::existing(root, Duration::ZERO).unwrap();
    assert_eq!(
        fs::read(root.join("state/.workspace.lock")).unwrap(),
        b"retained lock metadata"
    );
}

#[test]
fn actual_action_control_size_and_encodings_are_fatal_across_source_capture() {
    let root = fixture(true, true);
    let actual = crate::platform::workspace_location(root.path()).unwrap();
    let control = object(vec![
        ("version", integer(1).unwrap()),
        ("action", string("runtime_launch")),
        ("workspace_id", string("b".repeat(32))),
        ("epoch_id", string("c".repeat(32))),
        (
            "location",
            object(vec![
                ("root", string(actual.root)),
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
    .unwrap()
    .compact()
    .unwrap();
    let path = root.path().join("state/runtime-launch-control.json");
    fs::write(&path, &control).unwrap();
    let report = parsed(inspect_arguments(root.path(), true).unwrap());
    assert!(matches!(
        field(&report, &["source_assessment", "coordinated"]),
        Value::Bool(true)
    ));
    let execution_before = fs::read(root.path().join("state/execution/control.json")).unwrap();

    let mut oversized = control.clone();
    oversized.resize(values::MAX_INPUT + 1, b' ');
    fs::write(&path, oversized).unwrap();
    assert_capture_profile(root.path(), "unsupported_action_input_profile");

    let mut utf8_bom = b"\xef\xbb\xbf".to_vec();
    utf8_bom.extend_from_slice(&control);
    fs::write(&path, utf8_bom).unwrap();
    assert_capture_profile(root.path(), "unsupported_action_encoding_profile");
    let text = std::str::from_utf8(&control).unwrap();
    for little_endian in [true, false] {
        for bom in [true, false] {
            let mut raw = if bom {
                if little_endian {
                    vec![0xff, 0xfe]
                } else {
                    vec![0xfe, 0xff]
                }
            } else {
                Vec::new()
            };
            for unit in text.encode_utf16() {
                raw.extend_from_slice(&if little_endian {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
            fs::write(&path, raw).unwrap();
            assert_capture_profile(root.path(), "unsupported_action_encoding_profile");
        }
    }
    assert_eq!(
        fs::read(root.path().join("state/execution/control.json")).unwrap(),
        execution_before
    );
}

#[test]
fn actual_bad_or_null_action_control_remains_an_active_source_diagnostic() {
    let root = fixture(true, true);
    for (raw, code) in [
        (b"null".as_slice(), "invalid_action_fields"),
        (b"{".as_slice(), "invalid_compat_json"),
    ] {
        fs::write(root.path().join("state/runtime-launch-control.json"), raw).unwrap();
        let io = CaptureCounts::default();
        let report = inspect(root.path(), &io, &mut Work::default()).unwrap();
        assert_eq!(status(&report), "active");
        assert_eq!(
            field(&report, &["source_assessment", "error"]).as_str(),
            Some(format!("Current source could not be assessed: {code}").as_str())
        );
        assert!(matches!(
            field(&report, &["source_assessment", "coordinated"]),
            Value::Bool(false)
        ));
        assert_eq!(io.reads.get(), 3);
        assert_eq!(io.sources.get(), 1);
    }
}

#[test]
fn injected_action_work_profile_is_hard_without_a_fourth_read() {
    let root = fixture(true, true);
    let mut io = Script::new(vec![
        Ok(Some(enabled())),
        Ok(Some(enabled())),
        Ok(Some(enabled())),
    ]);
    io.source_error = Some(Error("unsupported_action_work_profile"));
    assert_eq!(
        inspect(root.path(), &io, &mut Work::default())
            .unwrap_err()
            .code(),
        "unsupported_action_work_profile"
    );
    assert_eq!(io.read_count.get(), 3);
    assert_eq!(io.sources.get(), 1);
}

#[test]
fn actual_workspace_ledger_and_json_caps_stay_fatal_across_source_capture() {
    let root = fixture(true, true);
    let mut large_string = vec![b'"'];
    large_string.resize(values::MAX_STRING + 2, b'x');
    large_string.push(b'"');
    let mut deep = vec![b'['; values::MAX_DEPTH + 1];
    deep.extend(std::iter::repeat_n(b']', values::MAX_DEPTH + 1));
    for (relative, raw, code) in [
        (
            "coven.toml",
            vec![b' '; crate::workspace::MAX_TEXT + 1],
            "unsupported_checkpoint_workspace_input_profile",
        ),
        (
            "coven.toml",
            b"ignored = 9223372036854775808\n".to_vec(),
            "unsupported_checkpoint_toml_scalar_profile",
        ),
        (
            "state/task-authority.json",
            vec![b' '; values::MAX_INPUT + 1],
            "handoff_size_limit",
        ),
        (
            "state/tasks.json",
            large_string,
            "unsupported_json_string_profile",
        ),
        ("state/agents.json", deep, "unsupported_json_depth_profile"),
    ] {
        let path = root.path().join(relative);
        fs::write(&path, raw).unwrap();
        assert_capture_profile(root.path(), code);
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn actual_multiplexer_nonfinite_and_surrogate_source_errors_remain_diagnostics() {
    let root = fixture(true, true);
    for (relative, raw, code) in [
        (
            "coven.toml",
            b"[runtime]\nmultiplexer = 'invalid'\n".as_slice(),
            "unsupported_multiplexer",
        ),
        (
            "state/tasks.json",
            br#"[{"id":"task","title":"T","status":"open","extra":NaN}]"#.as_slice(),
            "unsupported_nonfinite_json",
        ),
        (
            "state/tasks.json",
            br#"[{"id":"task","title":"T","status":"open","extra":"\ud800"}]"#.as_slice(),
            "unsupported_or_invalid_json_scalar",
        ),
    ] {
        let path = root.path().join(relative);
        fs::write(&path, raw).unwrap();
        let io = CaptureCounts::default();
        let report = inspect(root.path(), &io, &mut Work::default()).unwrap();
        assert_eq!(status(&report), "active");
        assert_eq!(
            field(&report, &["source_assessment", "error"]).as_str(),
            Some(format!("Current source could not be assessed: {code}").as_str())
        );
        assert!(matches!(
            field(&report, &["source_assessment", "coordinated"]),
            Value::Bool(false)
        ));
        assert_eq!(io.reads.get(), 3);
        assert_eq!(io.sources.get(), 1);
        fs::remove_file(path).unwrap();
    }
}
