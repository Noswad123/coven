use super::*;
use std::cell::{Cell, RefCell};

const NOW: &str = "2026-11-02T01:02:03.123456+00:00";

#[derive(Default)]
struct Script {
    calls: RefCell<Vec<&'static str>>,
    ids: Cell<usize>,
    fail: Option<&'static str>,
    uncertain: bool,
    activation_fault: Option<(usize, bool)>,
}
impl Script {
    fn step(&self, name: &'static str) -> Result<()> {
        self.calls.borrow_mut().push(name);
        require(self.fail != Some(name), "injected_failure")
    }
}
impl Metadata for Script {
    fn uuid(&self) -> Result<String> {
        self.step("uuid")?;
        self.ids.set(self.ids.get() + 1);
        Ok(format!("{:032x}", self.ids.get()))
    }
}
impl PrivateIo for Script {
    fn now(&self) -> Result<String> {
        self.step("now")?;
        Ok(NOW.to_owned())
    }
    fn read(&self, root: &Path) -> Result<Option<(Value, Validated)>> {
        self.step("read")?;
        read_typed(root)
    }
    fn lock(&self, root: &Path) -> Result<WorkspaceGuard> {
        self.step("lock")?;
        WorkspaceGuard::execution_maintenance(root, Duration::ZERO)
    }
    fn observe(&self, _: &Path, snapshot: &Value, _: &mut Work) -> Result<Value> {
        self.step("observe")?;
        Ok(source_value(snapshot))
    }
    fn capture(
        &self,
        _: &Path,
        snapshot: &Value,
        _: &WorkspaceGuard,
        _: &mut Work,
    ) -> Result<Value> {
        self.step("capture")?;
        Ok(source_value(snapshot))
    }
    fn activate(&self, root: &Path, guard: &WorkspaceGuard) -> Result<()> {
        self.step("activate")?;
        if let Some((operation, after)) = self.activation_fault {
            activate_with(
                root,
                guard,
                &ActivationFault {
                    operation,
                    after,
                    calls: Cell::new(0),
                },
            )
        } else {
            NativeIo.activate(root, guard)
        }
    }
    fn output(
        &self,
        view: &Validated,
        result: &Value,
        replayed: bool,
        json: bool,
    ) -> Result<delivery::Prepared> {
        self.step("output")?;
        NativeIo.output(view, result, replayed, json)
    }
    fn replace(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        self.step("replace")?;
        let delivery = NativeIo.replace(guard, bytes)?;
        Ok(if self.uncertain {
            Delivery::DurabilityUncertain
        } else {
            delivery
        })
    }
    fn confirm(&self, root: &Path, view: &Validated, guard: &WorkspaceGuard) -> Result<()> {
        self.step("confirm")?;
        NativeIo.confirm(root, view, guard)
    }
}

struct ActivationFault {
    operation: usize,
    after: bool,
    calls: Cell<usize>,
}
impl ActivationFault {
    fn check(&self, after: bool) -> Result<()> {
        require(
            self.calls.get() != self.operation || self.after != after,
            "activation_fault",
        )
    }
}
impl ActivationIo for ActivationFault {
    fn mkdir(&self, path: &Path) -> Result<()> {
        self.calls.set(self.calls.get() + 1);
        self.check(false)?;
        NativeActivationIo.mkdir(path)?;
        self.check(true)
    }
    fn sync_directory(&self, path: &Path) -> Result<()> {
        self.calls.set(self.calls.get() + 1);
        self.check(false)?;
        NativeActivationIo.sync_directory(path)?;
        self.check(true)
    }
}

fn source_value(snapshot: &Value) -> Value {
    let fixture = super::super::super::tests::enabled();
    let first = &get(&fixture, "history").unwrap().as_array().unwrap()[0];
    let mut source = get(get(first, "effect").unwrap(), "source")
        .unwrap()
        .clone();
    let mut identity = get(&source, "identity").unwrap().clone();
    identity.set_field("source_id", snapshot.clone()).unwrap();
    source.set_field("identity", identity).unwrap();
    source
}

fn request<'a>(
    operation: Operation,
    parameters: Value,
    expected: &'a str,
    tasks: Option<&'a str>,
    key: &'a str,
) -> Request<'a> {
    Request {
        operation,
        parameters,
        expected_execution: expected,
        expected_tasks: tasks,
        key,
    }
}
fn enable() -> Request<'static> {
    request(
        Operation::Enable,
        object(vec![]).unwrap(),
        DISABLED,
        None,
        "enable",
    )
}
fn parsed(output: &TaskOutput) -> Value {
    assert!(output.stdout.is_ascii());
    assert!(!output.stdout.ends_with('\n'));
    values::parse(output.stdout.as_bytes(), Profile::Authority).unwrap()
}
fn field<'a>(value: &'a Value, names: &[&str]) -> &'a Value {
    names.iter().fold(value, |v, name| get(v, name).unwrap())
}

#[cfg(unix)]
#[test]
fn activation_directory_matches_normal_creation_under_current_umask() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let guard = WorkspaceGuard::execution_maintenance(dir.path(), Duration::ZERO).unwrap();
    let reference = dir.path().join("state/reference");
    fs::create_dir(&reference).unwrap();
    NativeIo.activate(dir.path(), &guard).unwrap();
    let execution = dir.path().join("state/execution");
    assert_eq!(
        fs::metadata(&execution).unwrap().permissions().mode() & 0o7777,
        fs::metadata(&reference).unwrap().permissions().mode() & 0o7777,
    );
    assert!(NativeIo.activate(dir.path(), &guard).is_err());
    assert!(!execution.join("control.json").exists());
}

#[test]
fn each_activation_boundary_preserves_disabled_or_pending_authority() {
    for operation in 1..=3 {
        for after in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let io = Script {
                activation_fault: Some((operation, after)),
                ..Script::default()
            };
            assert_eq!(
                execute_with(dir.path(), &enable(), true, &io)
                    .err()
                    .unwrap()
                    .code(),
                "activation_fault"
            );
            let execution = dir.path().join("state/execution");
            assert!(!execution.join("control.json").exists());
            assert!(!io.calls.borrow().contains(&"replace"));
            if operation == 1 && !after {
                assert!(!execution.exists());
                assert!(read_typed(dir.path()).unwrap().is_none());
            } else {
                assert!(execution.is_dir());
                let evidence = execution.join("retained-evidence");
                fs::write(&evidence, b"pending activation").unwrap();
                assert_eq!(
                    execute_with(dir.path(), &enable(), true, &Script::default())
                        .err()
                        .unwrap()
                        .code(),
                    "pending_execution_authority"
                );
                assert_eq!(fs::read(&evidence).unwrap(), b"pending activation");
                assert!(!execution.join("control.json").exists());
            }
        }
    }
}

#[test]
fn activation_preserves_wrong_type_and_colliding_directory_evidence() {
    for directory in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let guard = WorkspaceGuard::execution_maintenance(dir.path(), Duration::ZERO).unwrap();
        let execution = dir.path().join("state/execution");
        let evidence = if directory {
            fs::create_dir(&execution).unwrap();
            execution.join("evidence")
        } else {
            execution.clone()
        };
        fs::write(&evidence, b"do not adopt or replace").unwrap();
        assert!(NativeIo.activate(dir.path(), &guard).is_err());
        assert_eq!(fs::read(&evidence).unwrap(), b"do not adopt or replace");
        assert_eq!(
            fs::symlink_metadata(&execution).unwrap().is_dir(),
            directory
        );
        assert!(!execution.join("control.json").exists());
    }
}

#[test]
fn fresh_standalone_workflow_and_old_cancel_interoperate() {
    for json in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let io = Script::default();
        let output = execute_with(dir.path(), &enable(), json, &io).unwrap();
        assert_eq!(output.exit_code, 0);
        let enabled = parsed(&output);
        let revision = text(field(&enabled, &["delivery", "current_revision"])).unwrap();
        let tasks = text(field(&enabled, &["result", "source", "revision"])).unwrap();
        let enroll = request(
            Operation::Enroll,
            object(vec![
                ("task_id", string("task")),
                ("rebind_source", Value::Bool(false)),
            ])
            .unwrap(),
            revision,
            Some(tasks),
            "enroll",
        );
        let enrolled = parsed(&execute_with(dir.path(), &enroll, json, &io).unwrap());
        let revision = text(field(&enrolled, &["delivery", "current_revision"])).unwrap();
        let intent = values::parse(br#"{"schema_version":1,"kind":"task-intent","task_id":"task","owner":"worker","acceptance_criteria":[{"id":"one","text":"ship"}]}"#, Profile::Authority).unwrap();
        let prepare = request(
            Operation::Prepare,
            object(vec![("task_id", string("task")), ("intent", intent)]).unwrap(),
            revision,
            Some(tasks),
            "prepare",
        );
        let prepared = parsed(&execute_with(dir.path(), &prepare, json, &io).unwrap());
        let revision = text(field(&prepared, &["delivery", "current_revision"])).unwrap();
        let attempt = text(field(&prepared, &["result", "attempt_id"])).unwrap();
        let cancel = super::super::cancel_arguments(
            &[dir.path().to_owned(), PathBuf::from(attempt)],
            "withdraw",
            revision,
            "cancel",
            json,
        )
        .unwrap();
        assert_eq!(cancel.exit_code, 0);
        let (_, view) = read_typed(dir.path()).unwrap().unwrap();
        assert_eq!(view.replayed.receipts.len(), 4);
        assert!(view.replayed.state.attempts.iter().all(|a| !a.active()));
        let upgrade = request(
            Operation::Upgrade,
            object(vec![]).unwrap(),
            &view.replayed.revision,
            None,
            "upgrade",
        );
        assert_eq!(
            execute_with(dir.path(), &upgrade, json, &io)
                .unwrap()
                .exit_code,
            0
        );
        for retained in [&enable(), &enroll, &prepare] {
            let retry = Script {
                fail: Some("capture"),
                ..Script::default()
            };
            let output = parsed(&execute_with(dir.path(), retained, json, &retry).unwrap());
            assert!(matches!(
                field(&output, &["delivery", "replayed"]),
                Value::Bool(true)
            ));
            assert!(!retry.calls.borrow().contains(&"now"));
            assert!(!retry.calls.borrow().contains(&"uuid"));
        }
        let calls = io.calls.borrow();
        assert!(
            calls.iter().position(|c| *c == "now").unwrap()
                < calls.iter().position(|c| *c == "capture").unwrap()
        );
        assert!(
            calls.iter().position(|c| *c == "output").unwrap()
                < calls.iter().position(|c| *c == "activate").unwrap()
        );
    }
}

#[test]
fn source_failure_preflight_never_creates_lock_and_fresh_failure_never_activates() {
    for fail in ["observe", "capture", "now", "output"] {
        let dir = tempfile::tempdir().unwrap();
        let io = Script {
            fail: Some(fail),
            ..Script::default()
        };
        assert_eq!(
            execute_with(dir.path(), &enable(), true, &io)
                .err()
                .unwrap()
                .code(),
            "injected_failure"
        );
        assert!(!dir.path().join("state/execution").exists());
        if fail == "observe" {
            assert!(!dir.path().join("state").exists());
        }
        assert!(!io.calls.borrow().contains(&"replace"));
    }
}

#[test]
fn replacement_failure_leaves_pending_activation_without_adoption() {
    let dir = tempfile::tempdir().unwrap();
    let io = Script {
        fail: Some("replace"),
        ..Script::default()
    };
    assert!(execute_with(dir.path(), &enable(), true, &io).is_err());
    assert!(dir.path().join("state/execution").is_dir());
    assert_eq!(
        execute_with(dir.path(), &enable(), true, &Script::default())
            .err()
            .unwrap()
            .code(),
        "pending_execution_authority"
    );
}

#[test]
fn known_receipts_skip_metadata_source_and_preserve_uncertain_result() {
    let dir = tempfile::tempdir().unwrap();
    let first = execute_with(dir.path(), &enable(), true, &Script::default()).unwrap();
    let expected = get(&parsed(&first), "result").unwrap().canonical().unwrap();
    for fail in ["capture", "now", "uuid", "observe", "confirm"] {
        let io = Script {
            fail: Some(fail),
            ..Script::default()
        };
        let output = execute_with(dir.path(), &enable(), false, &io).unwrap();
        assert_eq!(output.exit_code, if fail == "confirm" { 2 } else { 0 });
        assert_eq!(
            get(&parsed(&output), "result")
                .unwrap()
                .canonical()
                .unwrap(),
            expected
        );
        for forbidden in ["capture", "now", "uuid", "observe", "replace", "activate"] {
            assert!(!io.calls.borrow().contains(&forbidden));
        }
    }
}

#[test]
fn uncertain_atomic_replacement_never_rereads_or_confirms() {
    let dir = tempfile::tempdir().unwrap();
    let io = Script {
        uncertain: true,
        ..Script::default()
    };
    let output = execute_with(dir.path(), &enable(), true, &io).unwrap();
    assert_eq!(output.exit_code, 2);
    assert_eq!(
        text(field(&parsed(&output), &["delivery", "outcome"])).unwrap(),
        "durability_uncertain"
    );
    assert_eq!(
        io.calls.borrow().iter().filter(|c| **c == "read").count(),
        2
    );
    assert!(!io.calls.borrow().contains(&"confirm"));
}

#[test]
fn legacy_enable_receipt_replays_after_upgrade_without_source_or_backup() {
    let dir = tempfile::tempdir().unwrap();
    execute_with(dir.path(), &enable(), true, &Script::default()).unwrap();
    let (control, view) = read_typed(dir.path()).unwrap().unwrap();
    let upgrade = request(
        Operation::Upgrade,
        object(vec![]).unwrap(),
        &view.replayed.revision,
        None,
        "upgrade",
    );
    let io = Script::default();
    let output = execute_with(dir.path(), &upgrade, true, &io).unwrap();
    assert_eq!(output.exit_code, 0);
    assert!(!io.calls.borrow().contains(&"capture"));
    assert!(!io.calls.borrow().contains(&"now"));
    let backup = text(field(&parsed(&output), &["result", "backup_path"]))
        .unwrap()
        .to_owned();
    assert_eq!(
        values::parse(
            &fs::read(dir.path().join(&backup)).unwrap(),
            Profile::Authority
        )
        .unwrap()
        .canonical()
        .unwrap(),
        control.canonical().unwrap()
    );
    fs::remove_dir_all(dir.path().join("state/execution/upgrades")).unwrap();
    let io = Script {
        fail: Some("uuid"),
        ..Script::default()
    };
    assert_eq!(
        execute_with(dir.path(), &upgrade, true, &io)
            .unwrap()
            .exit_code,
        0
    );
    let io = Script {
        fail: Some("capture"),
        ..Script::default()
    };
    assert_eq!(
        execute_with(dir.path(), &enable(), true, &io)
            .unwrap()
            .exit_code,
        0
    );
    assert!(!io.calls.borrow().contains(&"capture"));
}

#[test]
fn upgrade_output_admission_failure_creates_no_backup_or_control_change() {
    let dir = tempfile::tempdir().unwrap();
    execute_with(dir.path(), &enable(), true, &Script::default()).unwrap();
    let (_, view) = read_typed(dir.path()).unwrap().unwrap();
    let before = fs::read(dir.path().join("state/execution/control.json")).unwrap();
    let upgrade = request(
        Operation::Upgrade,
        object(vec![]).unwrap(),
        &view.replayed.revision,
        None,
        "upgrade",
    );
    let io = Script {
        fail: Some("output"),
        ..Script::default()
    };
    assert!(execute_with(dir.path(), &upgrade, true, &io).is_err());
    assert!(!dir.path().join("state/execution/upgrades").exists());
    assert_eq!(
        fs::read(dir.path().join("state/execution/control.json")).unwrap(),
        before
    );
}

#[test]
fn upgrade_postpublication_uncertainty_retains_exact_result_and_backup_path() {
    for confirmation_error in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        execute_with(dir.path(), &enable(), true, &Script::default()).unwrap();
        let original = fs::read(dir.path().join("state/execution/control.json")).unwrap();
        let (_, view) = read_typed(dir.path()).unwrap().unwrap();
        let upgrade = request(
            Operation::Upgrade,
            object(vec![]).unwrap(),
            &view.replayed.revision,
            None,
            "upgrade",
        );
        let io = Script {
            uncertain: !confirmation_error,
            fail: confirmation_error.then_some("confirm"),
            ..Script::default()
        };
        let output = execute_with(dir.path(), &upgrade, true, &io).unwrap();
        assert_eq!(output.exit_code, 2);
        let envelope = parsed(&output);
        assert_eq!(
            text(field(&envelope, &["delivery", "outcome"])).unwrap(),
            "durability_uncertain"
        );
        assert!(matches!(
            field(&envelope, &["delivery", "current_revision"]),
            Value::Null
        ));
        let (control, proposed) = read_typed(dir.path()).unwrap().unwrap();
        assert_eq!(
            text(field(&envelope, &["delivery", "known_control_revision"])).unwrap(),
            proposed.replayed.revision
        );
        let entry = &proposed.replayed.receipts.last().unwrap().1;
        assert_eq!(
            field(&envelope, &["result"]).canonical().unwrap(),
            get(entry, "result").unwrap().canonical().unwrap()
        );
        let backup = text(field(&envelope, &["result", "backup_path"])).unwrap();
        assert_eq!(fs::read(dir.path().join(backup)).unwrap(), original);
        assert_eq!(
            text(field(&control, &["upgrade", "backup_path"])).unwrap(),
            backup
        );
        assert_eq!(
            io.calls
                .borrow()
                .iter()
                .filter(|call| **call == "read")
                .count(),
            1
        );
        assert_eq!(
            io.calls
                .borrow()
                .iter()
                .filter(|call| **call == "confirm")
                .count(),
            usize::from(confirmation_error)
        );
        assert_eq!(
            io.calls.borrow().last(),
            Some(&if confirmation_error {
                "confirm"
            } else {
                "replace"
            })
        );
    }
}

#[cfg(unix)]
#[test]
fn upgrade_after_backup_barrier_child() {
    let Some(root) = std::env::var_os("COVEN_TEST_UPGRADE_BACKUP_BARRIER_ROOT") else {
        return;
    };
    struct BarrierIo;
    impl Metadata for BarrierIo {
        fn uuid(&self) -> Result<String> {
            NativeIo.uuid()
        }
    }
    impl PrivateIo for BarrierIo {
        fn replace(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
            // Reaching replace proves planning, admission and verified backup
            // publication finished; authority replacement has not started.
            let candidate = values::parse(bytes, Profile::Authority)?;
            let backup = text(field(&candidate, &["upgrade", "backup_path"]));
            let backup = backup?;
            fs::write(guard.root().join("barrier.tmp"), backup)?;
            fs::rename(
                guard.root().join("barrier.tmp"),
                guard.root().join("backup-published"),
            )?;
            loop {
                std::thread::park();
            }
        }
    }
    let root = Path::new(&root);
    let (_, view) = read_typed(root).unwrap().unwrap();
    let upgrade = request(
        Operation::Upgrade,
        object(vec![]).unwrap(),
        &view.replayed.revision,
        None,
        "upgrade-after-death",
    );
    let _ = execute_with(root, &upgrade, true, &BarrierIo).unwrap();
    panic!("backup barrier unexpectedly returned");
}

#[cfg(unix)]
#[test]
fn death_after_upgrade_backup_preserves_v1_and_retry_publishes_new_evidence() {
    use std::os::unix::{
        fs::{MetadataExt, PermissionsExt},
        process::ExitStatusExt,
    };
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let dir = tempfile::tempdir().unwrap();
    execute_with(dir.path(), &enable(), true, &Script::default()).unwrap();
    let (control, view) = read_typed(dir.path()).unwrap().unwrap();
    let path = dir.path().join("state/execution/control.json");
    let raw = control.storage().unwrap();
    fs::write(&path, &raw).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
    let lock_path = dir.path().join("state/.workspace.lock");
    let lock_inode = fs::metadata(&lock_path).unwrap().ino();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "execution::mutation::lifecycle::tests::upgrade_after_backup_barrier_child",
            "--nocapture",
        ])
        .env("COVEN_TEST_UPGRADE_BACKUP_BARRIER_ROOT", dir.path())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let barrier = dir.path().join("backup-published");
    let started = Instant::now();
    while !barrier.exists() {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("child exited before backup publication: {status}");
        }
        if started.elapsed() > Duration::from_secs(15) {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("timed out waiting for upgrade backup publication");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    assert_eq!(child.wait().unwrap().signal(), Some(9));
    assert_eq!(fs::read(&path).unwrap(), raw);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
        mode
    );
    let old_backup = fs::read_to_string(&barrier).unwrap();
    let old_path = dir.path().join(&old_backup);
    assert_eq!(fs::read(&old_path).unwrap(), raw);
    assert_eq!(
        fs::metadata(&old_path).unwrap().permissions().mode() & 0o7777,
        mode
    );
    let guard = WorkspaceGuard::execution_maintenance(dir.path(), Duration::ZERO).unwrap();
    assert_eq!(fs::metadata(&lock_path).unwrap().ino(), lock_inode);
    assert_eq!(read_typed(dir.path()).unwrap().unwrap().1.schema, 1);
    drop(guard);
    let upgrade = request(
        Operation::Upgrade,
        object(vec![]).unwrap(),
        &view.replayed.revision,
        None,
        "upgrade-after-death",
    );
    let output = execute(dir.path(), &upgrade, true).unwrap();
    assert_eq!(output.exit_code, 0);
    let result = parsed(&output);
    let new_backup = text(field(&result, &["result", "backup_path"])).unwrap();
    assert_ne!(new_backup, old_backup);
    assert_eq!(fs::read(dir.path().join(new_backup)).unwrap(), raw);
    assert_eq!(
        fs::metadata(dir.path().join(new_backup))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        mode
    );
    assert_eq!(fs::read(&old_path).unwrap(), raw);
    assert_eq!(
        fs::metadata(&old_path).unwrap().permissions().mode() & 0o7777,
        mode
    );
    let (upgraded, view) = read_typed(dir.path()).unwrap().unwrap();
    assert_eq!(view.schema, 2);
    assert_eq!(
        text(field(&upgraded, &["upgrade", "backup_path"])).unwrap(),
        new_backup
    );
    assert_eq!(
        fs::read_dir(dir.path().join("state/execution/upgrades"))
            .unwrap()
            .count(),
        2
    );
}

#[test]
fn borrowed_request_profile_bounds_precede_binding_allocations_without_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let oversized = "x".repeat(values::MAX_STRING + 1);
    for (execution, tasks, key) in [
        (oversized.as_str(), None, "enable"),
        (DISABLED, Some(oversized.as_str()), "enable"),
        (DISABLED, None, oversized.as_str()),
    ] {
        let request = request(
            Operation::Enable,
            object(vec![]).unwrap(),
            execution,
            tasks,
            key,
        );
        let mut work = Work::default();
        assert_eq!(
            request.binding(1, &mut work).unwrap_err().code(),
            "unsupported_json_string_profile"
        );
        assert_eq!(work.0, 0);
        let io = Script::default();
        assert_eq!(
            execute_with(dir.path(), &request, true, &io)
                .err()
                .unwrap()
                .code(),
            "unsupported_json_string_profile"
        );
        assert_eq!(&*io.calls.borrow(), &["read"]);
        assert!(!dir.path().join("state").exists());
    }
}

#[test]
fn ordinary_binding_semantics_still_precede_key_semantics_after_authority_read() {
    let dir = tempfile::tempdir().unwrap();
    for (revision, expected) in [
        ("invalid", "invalid_execution_revision"),
        (DISABLED, "invalid_execution_request_or_criterion_id"),
    ] {
        let request = request(
            Operation::Enable,
            object(vec![]).unwrap(),
            revision,
            None,
            "bad key",
        );
        let io = Script::default();
        assert_eq!(
            execute_with(dir.path(), &request, true, &io)
                .err()
                .unwrap()
                .code(),
            expected
        );
        assert_eq!(&*io.calls.borrow(), &["read"]);
        assert!(!dir.path().join("state").exists());
    }
}

#[test]
fn retained_key_conflict_and_fresh_cas_conflict_precede_source_or_metadata() {
    let dir = tempfile::tempdir().unwrap();
    execute_with(dir.path(), &enable(), true, &Script::default()).unwrap();
    let source = source_value(&string("0".repeat(32)));
    let tasks = text(get(&source, "revision").unwrap()).unwrap();
    for (key, expected_error) in [
        ("enable", "execution_request_key_binding_conflict"),
        ("fresh", "execution_revision_conflict"),
    ] {
        let enroll = request(
            Operation::Enroll,
            object(vec![
                ("task_id", string("task")),
                ("rebind_source", Value::Bool(false)),
            ])
            .unwrap(),
            DISABLED,
            Some(tasks),
            key,
        );
        let io = Script::default();
        assert_eq!(
            execute_with(dir.path(), &enroll, true, &io)
                .err()
                .unwrap()
                .code(),
            expected_error
        );
        assert!(!io.calls.borrow().contains(&"now"));
        assert!(!io.calls.borrow().contains(&"capture"));
        assert!(!io.calls.borrow().contains(&"uuid"));
    }
}

#[test]
fn graph_lifecycle_revocation_is_source_free_and_retains_prepared_reservation() {
    let dir = tempfile::tempdir().unwrap();
    let io = Script::default();
    let enabled = parsed(&execute_with(dir.path(), &enable(), true, &io).unwrap());
    let tasks = text(field(&enabled, &["result", "source", "revision"])).unwrap();
    let mut revision = text(field(&enabled, &["delivery", "current_revision"]))
        .unwrap()
        .to_owned();
    let enroll = request(
        Operation::Enroll,
        object(vec![
            ("task_id", string("task")),
            ("rebind_source", Value::Bool(false)),
        ])
        .unwrap(),
        &revision,
        Some(tasks),
        "enroll",
    );
    let enrolled = parsed(&execute_with(dir.path(), &enroll, true, &io).unwrap());
    revision = text(field(&enrolled, &["delivery", "current_revision"]))
        .unwrap()
        .to_owned();
    let upgrade = request(
        Operation::Upgrade,
        object(vec![]).unwrap(),
        &revision,
        None,
        "upgrade",
    );
    let upgraded = parsed(&execute_with(dir.path(), &upgrade, true, &io).unwrap());
    revision = text(field(&upgraded, &["delivery", "current_revision"]))
        .unwrap()
        .to_owned();
    let document = values::parse(br#"{"schema_version":1,"kind":"task-graph","nodes":[{"task_id":"task","owner":"worker","acceptance_criteria":[{"id":"one","text":"ship"}],"depends_on":[]}]}"#, Profile::Authority).unwrap();
    let propose = request(
        Operation::GraphPropose,
        object(vec![("document", document)]).unwrap(),
        &revision,
        Some(tasks),
        "propose",
    );
    let proposed = parsed(&execute_with(dir.path(), &propose, true, &io).unwrap());
    revision = text(field(&proposed, &["delivery", "current_revision"]))
        .unwrap()
        .to_owned();
    let graph = field(&proposed, &["result", "graph_revision"]).clone();
    let digest = field(&proposed, &["result", "graph_digest"]).clone();
    let params = |decision: &str| {
        object(vec![
            ("graph_revision", graph.clone()),
            ("decision", string(decision)),
            ("reason", string("review")),
            ("expected_graph_digest", digest.clone()),
        ])
        .unwrap()
    };
    let approve = request(
        Operation::GraphDecide,
        params("approve"),
        &revision,
        Some(tasks),
        "approve",
    );
    let approved = parsed(&execute_with(dir.path(), &approve, true, &io).unwrap());
    revision = text(field(&approved, &["delivery", "current_revision"]))
        .unwrap()
        .to_owned();
    let prepare = request(
        Operation::PrepareGraph,
        object(vec![
            ("task_id", string("task")),
            ("graph_revision", graph.clone()),
        ])
        .unwrap(),
        &revision,
        Some(tasks),
        "graph-prepare",
    );
    let prepared = parsed(&execute_with(dir.path(), &prepare, true, &io).unwrap());
    revision = text(field(&prepared, &["delivery", "current_revision"]))
        .unwrap()
        .to_owned();
    let revoke = request(
        Operation::GraphDecide,
        params("changes-requested"),
        &revision,
        None,
        "revoke",
    );
    let revoke_io = Script {
        fail: Some("capture"),
        ids: Cell::new(100),
        ..Script::default()
    };
    let revoked = execute_with(dir.path(), &revoke, true, &revoke_io).unwrap();
    assert_eq!(revoked.exit_code, 0);
    assert!(!revoke_io.calls.borrow().contains(&"capture"));
    let (_, view) = read_typed(dir.path()).unwrap().unwrap();
    assert_eq!(capacity::escape_count(&view).unwrap(), 1);
    assert_eq!(
        view.replayed
            .state
            .attempts
            .iter()
            .filter(|a| a.active())
            .count(),
        1
    );
}
