//! SDK-only reservation checks under an already borrowed workspace lock.
use super::*;
use crate::{platform, storage::WorkspaceGuard};
use std::{fs::File, io};

/// An observation, not transferable permission, CAS, receipt-absence evidence,
/// or an approval token. It can inform a future writer only while that writer
/// retains the same borrowed workspace lock; it does not activate any writer.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservationCheck {
    DisabledObserved,
    ActiveConfirmedClear,
}

/// Check one task's live preparation reservation, not current task/source state.
/// Success has meaning only within the lifetime of this same borrowed lock.
pub fn guard_task_mutation(
    workspace: &Path,
    task_id: &str,
    guard: &WorkspaceGuard,
) -> Result<ReservationCheck> {
    check(workspace, Some(task_id), guard, &SystemGuardIo)
}

/// Check all live preparation reservations before an authority change.
/// This is neither authority admission nor an approval or mutation token.
pub fn guard_authority_change(
    workspace: &Path,
    guard: &WorkspaceGuard,
) -> Result<ReservationCheck> {
    check(workspace, None, guard, &SystemGuardIo)
}

trait GuardIo {
    fn read(&self, root: &Path) -> Result<Option<Validated>> {
        Ok(read_typed(root)?.map(|(_, state)| state))
    }
    fn between_reads(&self, _root: &Path) {}
    fn open(&self, path: &Path) -> Result<File> {
        platform::open_readonly(path)
    }
    fn regular(&self, file: &File) -> io::Result<bool> {
        Ok(file.metadata()?.is_file())
    }
    fn sync_file(&self, file: &File) -> io::Result<()> {
        file.sync_all()
    }
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        platform::sync_directory(path)
    }
}
struct SystemGuardIo;
impl GuardIo for SystemGuardIo {}

fn synchronization_error(error: Error) -> Error {
    if error.code() == "input_or_storage_unavailable" {
        Error("execution_synchronization_uncertain")
    } else {
        error
    }
}

fn check(
    workspace: &Path,
    task: Option<&str>,
    guard: &WorkspaceGuard,
    io: &impl GuardIo,
) -> Result<ReservationCheck> {
    if let Some(task) = task {
        identifier(&string(task))?;
    }
    guard.validate_for(workspace)?;
    let root = guard.root();
    let Some(first) = io.read(root)? else {
        return Ok(ReservationCheck::DisabledObserved);
    };
    io.between_reads(root);
    let second = io
        .read(root)?
        .ok_or(Error("execution_authority_changed_before_confirmation"))?;
    require(
        first.authority == second.authority && first.replayed.revision == second.replayed.revision,
        "execution_authority_changed_before_confirmation",
    )?;

    // Validate managed types again before the nofollow/nonblocking regular-fd
    // open. Unlike evidence publication, unsafe paths remain hard errors.
    let control = read_fs::managed(root, Path::new("state/execution/control.json"), false)
        .map_err(synchronization_error)?;
    let file = io.open(&control).map_err(synchronization_error)?;
    let uncertain = |_| Error("execution_synchronization_uncertain");
    require(io.regular(&file).map_err(uncertain)?, "unsafe_archive_path")?;
    io.sync_file(&file).map_err(uncertain)?;
    for relative in ["state/execution", "state", ""] {
        let directory = if relative.is_empty() {
            root.to_owned()
        } else {
            read_fs::managed(root, Path::new(relative), true).map_err(synchronization_error)?
        };
        io.sync_dir(&directory).map_err(uncertain)?;
    }
    require(
        !first
            .replayed
            .state
            .attempts
            .iter()
            .any(|attempt| attempt.active() && task.is_none_or(|task| attempt.task == task)),
        "prepared_attempt_blocks_mutation",
    )?;
    Ok(ReservationCheck::ActiveConfirmedClear)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::tests::{cancel, enabled, enroll, prepare, upgraded};
    use std::{cell::RefCell, time::Duration};

    const ATTEMPT: &str = "ffffffffffffffffffffffffffffffff";
    const CANCEL: &str = "dddddddddddddddddddddddddddddddd";
    const STEPS: &[&str] = &[
        "read",
        "between",
        "read",
        "open",
        "metadata",
        "file",
        "execution",
        "state",
        "root",
    ];

    fn workspace(control: Option<&Value>) -> (tempfile::TempDir, WorkspaceGuard) {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("coven.json"), b"{}").unwrap();
        let guard = WorkspaceGuard::acquire(temp.path(), Duration::ZERO).unwrap();
        if let Some(control) = control {
            save(temp.path(), control);
        }
        (temp, guard)
    }

    fn save(root: &Path, control: &Value) {
        fs::create_dir_all(root.join("state/execution")).unwrap();
        fs::write(
            root.join("state/execution/control.json"),
            control.storage().unwrap(),
        )
        .unwrap();
    }

    type BetweenReads = Box<dyn Fn(&Path)>;

    #[derive(Default)]
    struct TestIo {
        calls: RefCell<Vec<&'static str>>,
        fail: Option<&'static str>,
        unsafe_open: bool,
        nonregular: bool,
        change: Option<BetweenReads>,
    }

    impl TestIo {
        fn step(&self, step: &'static str) -> io::Result<()> {
            self.calls.borrow_mut().push(step);
            if self.fail == Some(step) {
                Err(io::Error::other("injected operational failure"))
            } else {
                Ok(())
            }
        }
    }

    impl GuardIo for TestIo {
        fn read(&self, root: &Path) -> Result<Option<Validated>> {
            self.step("read")?;
            SystemGuardIo.read(root)
        }
        fn between_reads(&self, root: &Path) {
            self.step("between").unwrap();
            if let Some(change) = &self.change {
                change(root);
            }
        }
        fn open(&self, path: &Path) -> Result<File> {
            self.step("open")?;
            if self.unsafe_open {
                return Err(Error("unsafe_archive_path"));
            }
            SystemGuardIo.open(path)
        }
        fn regular(&self, file: &File) -> io::Result<bool> {
            self.step("metadata")?;
            Ok(!self.nonregular && SystemGuardIo.regular(file)?)
        }
        fn sync_file(&self, file: &File) -> io::Result<()> {
            self.step("file")?;
            SystemGuardIo.sync_file(file)
        }
        fn sync_dir(&self, path: &Path) -> io::Result<()> {
            let step = match path.file_name().and_then(|s| s.to_str()) {
                Some("execution") => "execution",
                Some("state") => "state",
                _ => "root",
            };
            self.step(step)?;
            SystemGuardIo.sync_dir(path)
        }
    }

    fn check_all(root: &Path, guard: &WorkspaceGuard, reserved: bool) {
        for task in [Some("task"), Some("unrelated"), None] {
            let io = TestIo::default();
            let actual = check(root, task, guard, &io);
            if reserved && task != Some("unrelated") {
                assert_eq!(
                    actual.unwrap_err().code(),
                    "prepared_attempt_blocks_mutation"
                );
            } else {
                assert_eq!(actual.unwrap(), ReservationCheck::ActiveConfirmedClear);
            }
            assert_eq!(*io.calls.borrow(), STEPS);
            // Confirmation is unconditional: clear, unrelated, cancelled,
            // approved, and revoked states must all fail before reservation use.
            for (index, step) in STEPS.iter().enumerate().skip(3) {
                let io = TestIo {
                    fail: Some(step),
                    ..TestIo::default()
                };
                assert_eq!(
                    check(root, task, guard, &io).unwrap_err().code(),
                    "execution_synchronization_uncertain"
                );
                assert_eq!(*io.calls.borrow(), STEPS[..=index]);
            }
        }
    }

    #[test]
    fn v1_live_state_not_historical_prepared_receipts_and_no_current_inputs() {
        let mut control = enabled();
        let (temp, guard) = workspace(Some(&control));
        check_all(temp.path(), &guard, false);
        enroll(&mut control);
        save(temp.path(), &control);
        check_all(temp.path(), &guard, false);
        prepare(&mut control, ATTEMPT, Value::Null, 1);
        save(temp.path(), &control);
        // Current source/config/backup are not read, even when invalid types.
        fs::create_dir(temp.path().join("state/tasks.json")).unwrap();
        fs::write(temp.path().join("team.json"), b"null").unwrap();
        fs::write(temp.path().join("coven.json"), b"not json").unwrap();
        fs::write(temp.path().join("state/execution/upgrades"), b"poison").unwrap();
        check_all(temp.path(), &guard, true);
        cancel(&mut control, ATTEMPT, CANCEL);
        save(temp.path(), &control);
        check_all(temp.path(), &guard, false);
        save(temp.path(), &upgraded(control));
        check_all(temp.path(), &guard, false);
        assert_eq!(
            guard_authority_change(temp.path(), &guard).unwrap(),
            ReservationCheck::ActiveConfirmedClear
        );
    }

    #[test]
    fn confirmation_order_and_every_operational_failure_stops() {
        let (temp, guard) = workspace(Some(&enabled()));
        for (index, step) in STEPS.iter().enumerate().skip(3) {
            let io = TestIo {
                fail: Some(step),
                ..TestIo::default()
            };
            assert_eq!(
                check(temp.path(), None, &guard, &io).unwrap_err().code(),
                "execution_synchronization_uncertain"
            );
            assert_eq!(*io.calls.borrow(), STEPS[..=index]);
        }
        for (unsafe_open, nonregular, end) in [(true, false, 3), (false, true, 4)] {
            let io = TestIo {
                unsafe_open,
                nonregular,
                ..TestIo::default()
            };
            assert_eq!(
                check(temp.path(), None, &guard, &io).unwrap_err().code(),
                "unsafe_archive_path"
            );
            assert_eq!(*io.calls.borrow(), STEPS[..=end]);
        }
        let io = TestIo {
            fail: Some("read"),
            ..TestIo::default()
        };
        assert_eq!(
            check(temp.path(), None, &guard, &io).unwrap_err().code(),
            "input_or_storage_unavailable"
        );
        assert_eq!(*io.calls.borrow(), ["read"]);
    }

    #[test]
    fn known_blocker_still_confirms_and_sync_failure_takes_precedence() {
        let mut control = enabled();
        enroll(&mut control);
        prepare(&mut control, ATTEMPT, Value::Null, 1);
        let (temp, guard) = workspace(Some(&control));
        for (index, step) in STEPS.iter().enumerate().skip(3) {
            let io = TestIo {
                fail: Some(step),
                ..TestIo::default()
            };
            assert_eq!(
                check(temp.path(), Some("task"), &guard, &io)
                    .unwrap_err()
                    .code(),
                "execution_synchronization_uncertain"
            );
            assert_eq!(*io.calls.borrow(), STEPS[..=index]);
        }
    }

    #[test]
    fn identifiers_then_guard_are_validated_before_authority_io() {
        let (temp, guard) = workspace(None);
        let other = tempfile::tempdir().unwrap();
        let io = TestIo::default();
        for id in ["", "../task", "a.b", "a b", "\u{e9}", "a\n", "a/"] {
            assert_eq!(
                check(other.path(), Some(id), &guard, &io)
                    .unwrap_err()
                    .code(),
                "invalid_execution_identifier"
            );
        }
        assert_eq!(
            check(other.path(), Some("valid_-09"), &guard, &io)
                .unwrap_err()
                .code(),
            "invalid_workspace_lock_owner"
        );
        assert!(io.calls.borrow().is_empty());
        assert_eq!(
            guard_task_mutation(temp.path(), "valid_-09", &guard).unwrap(),
            ReservationCheck::DisabledObserved
        );
        assert!(!temp.path().join("state/execution").exists());
    }

    #[test]
    fn disabled_only_when_execution_directory_absent() {
        let (temp, guard) = workspace(None);
        let io = TestIo::default();
        assert_eq!(
            check(temp.path(), None, &guard, &io).unwrap(),
            ReservationCheck::DisabledObserved
        );
        assert_eq!(*io.calls.borrow(), ["read"]);
        fs::create_dir(temp.path().join("state/execution")).unwrap();
        for bytes in [
            None,
            Some(b"null".as_slice()),
            Some(b"{".as_slice()),
            Some(b"{}".as_slice()),
        ] {
            if let Some(bytes) = bytes {
                fs::write(temp.path().join("state/execution/control.json"), bytes).unwrap();
            }
            let io = TestIo::default();
            assert!(check(temp.path(), None, &guard, &io).is_err());
            assert_eq!(*io.calls.borrow(), ["read"]);
        }
    }

    #[test]
    fn complete_second_read_rejects_changed_missing_pending_and_corrupt_authority() {
        let mut control = enabled();
        enroll(&mut control);
        prepare(&mut control, ATTEMPT, Value::Null, 1);
        let mut cancelled = control.clone();
        cancel(&mut cancelled, ATTEMPT, CANCEL);
        let changed = [cancelled.clone(), upgraded(cancelled)];
        for second in changed {
            let (temp, guard) = workspace(Some(&control));
            let io = TestIo {
                change: Some(Box::new(move |root| save(root, &second))),
                ..TestIo::default()
            };
            assert_eq!(
                check(temp.path(), None, &guard, &io).unwrap_err().code(),
                "execution_authority_changed_before_confirmation"
            );
            assert_eq!(*io.calls.borrow(), STEPS[..3]);
        }
        let mut replacement = enabled();
        replacement
            .set_field("execution_authority_id", string("1".repeat(32)))
            .unwrap();
        let mut history = get(&replacement, "history")
            .unwrap()
            .as_array()
            .unwrap()
            .to_vec();
        let mut result = get(&history[0], "result").unwrap().clone();
        result
            .set_field("execution_authority_id", string("1".repeat(32)))
            .unwrap();
        history[0].set_field("result", result).unwrap();
        replacement
            .set_field("history", Value::Array(history))
            .unwrap();
        let (temp, guard) = workspace(Some(&enabled()));
        let io = TestIo {
            change: Some(Box::new(move |root| save(root, &replacement))),
            ..TestIo::default()
        };
        assert_eq!(
            check(temp.path(), None, &guard, &io).unwrap_err().code(),
            "execution_authority_changed_before_confirmation"
        );
        assert_eq!(*io.calls.borrow(), STEPS[..3]);
        for (mode, expected) in [
            ("deleted", "execution_authority_changed_before_confirmation"),
            ("pending", "pending_execution_authority"),
            ("corrupt", "invalid_execution_fields"),
            ("unsupported", "unsupported_execution_version"),
            ("receipt", "execution_receipt_effect_mismatch"),
            ("io", "input_or_storage_unavailable"),
        ] {
            let (temp, guard) = workspace(Some(&control));
            let io = TestIo {
                change: Some(Box::new(move |root| {
                    let path = root.join("state/execution/control.json");
                    match mode {
                        "deleted" => fs::remove_dir_all(root.join("state/execution")).unwrap(),
                        "pending" => fs::remove_file(path).unwrap(),
                        "corrupt" => fs::write(path, b"null").unwrap(),
                        "unsupported" => {
                            let mut bad = enabled();
                            bad.set_field("schema_version", integer(3).unwrap())
                                .unwrap();
                            save(root, &bad);
                        }
                        "receipt" => {
                            let mut bad = enabled();
                            let mut history =
                                get(&bad, "history").unwrap().as_array().unwrap().to_vec();
                            history[0].set_field("result", Value::Null).unwrap();
                            bad.set_field("history", Value::Array(history)).unwrap();
                            save(root, &bad);
                        }
                        "io" => {
                            // A missing root makes the actual second read unavailable.
                            fs::remove_dir_all(root).unwrap();
                            fs::write(root, b"not a directory").unwrap();
                        }
                        _ => unreachable!(),
                    }
                })),
                ..TestIo::default()
            };
            assert_eq!(
                check(temp.path(), None, &guard, &io).unwrap_err().code(),
                expected,
                "{mode}"
            );
            assert_eq!(*io.calls.borrow(), STEPS[..3]);
            if mode == "io" {
                fs::remove_file(temp.path()).unwrap();
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn managed_unsafe_types_are_hard_errors_without_sync() {
        use std::os::unix::{ffi::OsStrExt, fs::symlink};
        for target in ["state", "state/execution", "state/execution/control.json"] {
            for kind in ["file", "directory", "symlink", "fifo"] {
                if (target.ends_with("json") && kind == "file")
                    || (!target.ends_with("json") && kind == "directory")
                {
                    continue;
                }
                let (temp, guard) = workspace(None);
                let path = temp.path().join(target);
                if target == "state" {
                    fs::remove_file(path.join(".workspace.lock")).unwrap();
                    fs::remove_dir(&path).unwrap();
                }
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                match kind {
                    "file" => fs::write(&path, b"bad type").unwrap(),
                    "directory" => fs::create_dir(&path).unwrap(),
                    "symlink" => symlink(temp.path().join("absent"), &path).unwrap(),
                    "fifo" => {
                        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
                        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
                    }
                    _ => unreachable!(),
                }
                let io = TestIo::default();
                assert_eq!(
                    check(temp.path(), None, &guard, &io).unwrap_err().code(),
                    "unsafe_archive_path",
                    "{target} {kind}"
                );
                assert_eq!(*io.calls.borrow(), ["read"]);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn v2_prepared_cancelled_stale_revoked_and_approved_without_attempts() {
        // Complete historical controls, not current source/config or native-generated expectations.
        let corpus_bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/retirement/fixtures/source_execution_support.json"
        ));
        let provenance_bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/retirement/fixtures/source_execution_support_provenance.json"
        ));
        assert_eq!(
            canonical::digest(corpus_bytes),
            "3a3d000c7e7e38496687511af093c8172d378eab70f05faa127caa2b3e926134"
        );
        assert_eq!(
            canonical::digest(provenance_bytes),
            "5631124181a36750c20aabe89f85fdc673c19d4f7c97fdd60e4fd865bc1fc742"
        );
        let corpus = values::parse(corpus_bytes, Profile::Authority).unwrap();
        let row = &get(&corpus, "cases").unwrap().as_array().unwrap()[2];
        assert_eq!(text(get(row, "id").unwrap()).unwrap(), "guards-12-states");
        let stdout = text(get(row, "stdout").unwrap()).unwrap().as_bytes();
        assert_eq!(
            canonical::digest(stdout),
            text(get(row, "stdout_sha256").unwrap()).unwrap()
        );
        let fixtures = values::parse(stdout, Profile::Authority).unwrap();
        assert_eq!(fixtures.as_array().unwrap().len(), 12);
        assert_eq!(
            fixtures
                .as_array()
                .unwrap()
                .iter()
                .map(|fixture| { matches!(fixture.as_array().unwrap()[1], Value::Bool(true)) })
                .collect::<Vec<_>>(),
            [
                false, false, false, true, false, false, false, true, true, true, false, false
            ]
        );
        for fixture in fixtures.as_array().unwrap() {
            let pair = fixture.as_array().unwrap();
            let control = &pair[0];
            let reserved = matches!(pair[1], Value::Bool(true));
            let (temp, guard) = workspace(Some(control));
            // No coven/config, task, roster, or backup content can be authority.
            fs::write(temp.path().join("coven.json"), b"poison").unwrap();
            for path in ["state/tasks.json", "team.json", "state/execution/upgrades"] {
                fs::write(temp.path().join(path), b"poison").unwrap();
            }
            check_all(temp.path(), &guard, reserved);
        }
    }
}
