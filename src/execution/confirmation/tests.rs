use super::*;
use crate::execution::tests::{enabled, enroll, prepare, upgraded};
use std::{cell::RefCell, time::Duration};

const STEPS: &[&str] = &[
    "read",
    "open",
    "metadata",
    "file",
    "execution",
    "state",
    "root",
];

fn prepared_v2() -> Value {
    let mut legacy = enabled();
    enroll(&mut legacy);
    let mut control = upgraded(legacy.clone());
    let selected = validated(&control).unwrap();
    prepare(
        &mut legacy,
        "ffffffffffffffffffffffffffffffff",
        Value::Null,
        1,
    );
    let mut entry = get(&legacy, "history")
        .unwrap()
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    let mut effect = get(&entry, "effect").unwrap().clone();
    let mut record = get(&effect, "prepared").unwrap().clone();
    record
        .set_field("schema_version", integer(2).unwrap())
        .unwrap();
    record.set_field("graph_binding", Value::Null).unwrap();
    let mut payload = object(vec![
        ("explicit_intent", get(&record, "intent").unwrap().clone()),
        ("graph_binding", Value::Null),
    ])
    .unwrap();
    for key in [
        "execution_authority_id",
        "enrollment_id",
        "task_authority_identity",
        "task_source_revision",
        "complete_task_snapshot",
    ] {
        payload
            .set_field(key, get(&record, key).unwrap().clone())
            .unwrap();
    }
    record
        .set_field(
            "intent_digest",
            string(
                Work::default()
                    .digest(b"coven-task-attempt-intent-v2\0", &payload)
                    .unwrap(),
            ),
        )
        .unwrap();
    effect.set_field("prepared", record.clone()).unwrap();
    entry.set_field("effect", effect).unwrap();
    let mut binding = get(&entry, "binding").unwrap().clone();
    binding
        .set_field("schema_version", integer(2).unwrap())
        .unwrap();
    binding
        .set_field(
            "expected_execution_revision",
            string(selected.replayed.revision),
        )
        .unwrap();
    entry.set_field("binding", binding).unwrap();
    let mut result = get(&entry, "result").unwrap().clone();
    result.set_field("attempt", record).unwrap();
    result.set_field("generation", integer(4).unwrap()).unwrap();
    entry.set_field("result", result).unwrap();
    entry.set_field("generation", integer(4).unwrap()).unwrap();
    let mut history = get(&control, "history")
        .unwrap()
        .as_array()
        .unwrap()
        .to_vec();
    history.push(entry);
    control.set_field("history", Value::Array(history)).unwrap();
    control
        .set_field("generation", integer(4).unwrap())
        .unwrap();
    assert!(validated(&control).unwrap().replayed.state.attempts[0].active());
    control
}

fn save(root: &Path, control: &Value) {
    fs::create_dir_all(root.join("state/execution")).unwrap();
    fs::write(
        root.join("state/execution/control.json"),
        control.storage().unwrap(),
    )
    .unwrap();
}

#[derive(Default)]
struct TestIo {
    calls: RefCell<Vec<&'static str>>,
    fail: Option<&'static str>,
    nonregular: bool,
}
impl TestIo {
    fn step(&self, step: &'static str) -> io::Result<()> {
        self.calls.borrow_mut().push(step);
        if self.fail == Some(step) {
            Err(io::Error::other("injected"))
        } else {
            Ok(())
        }
    }
}
impl Io for TestIo {
    fn read(&self, root: &Path) -> Result<Option<Validated>> {
        self.step("read")?;
        SystemIo.read(root)
    }
    fn open(&self, path: &Path) -> Result<File> {
        self.step("open")?;
        SystemIo.open(path)
    }
    fn regular(&self, file: &File) -> io::Result<bool> {
        self.step("metadata")?;
        Ok(!self.nonregular && SystemIo.regular(file)?)
    }
    fn sync_file(&self, file: &File) -> io::Result<()> {
        self.step("file")?;
        SystemIo.sync_file(file)
    }
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        self.step(match path.file_name().and_then(|s| s.to_str()) {
            Some("execution") => "execution",
            Some("state") => "state",
            _ => "root",
        })?;
        SystemIo.sync_dir(path)
    }
}

#[test]
fn prepared_v1_and_v2_source_free_order_and_every_transport_fault_is_hard_error() {
    let mut control = enabled();
    enroll(&mut control);
    prepare(
        &mut control,
        "ffffffffffffffffffffffffffffffff",
        Value::Null,
        1,
    );
    for control in [control, prepared_v2()] {
        let temp = tempfile::tempdir().unwrap();
        let guard = WorkspaceGuard::execution_maintenance(temp.path(), Duration::ZERO).unwrap();
        save(temp.path(), &control);
        fs::write(temp.path().join("coven.json"), b"corrupt").unwrap();
        fs::create_dir(temp.path().join("state/tasks.json")).unwrap();
        let selected = validated(&control).unwrap();
        let io = TestIo::default();
        confirm_with(temp.path(), &selected, &guard, &io).unwrap();
        assert_eq!(*io.calls.borrow(), STEPS);
        for (index, step) in STEPS.iter().enumerate() {
            let io = TestIo {
                fail: Some(step),
                ..TestIo::default()
            };
            assert!(confirm_with(temp.path(), &selected, &guard, &io).is_err());
            assert_eq!(*io.calls.borrow(), STEPS[..=index]);
        }
        let io = TestIo {
            nonregular: true,
            ..TestIo::default()
        };
        assert_eq!(
            confirm_with(temp.path(), &selected, &guard, &io)
                .unwrap_err()
                .code(),
            "unsafe_archive_path"
        );
        assert_eq!(*io.calls.borrow(), STEPS[..3]);
        let other = tempfile::tempdir().unwrap();
        let io = TestIo::default();
        assert_eq!(
            confirm_with(other.path(), &selected, &guard, &io)
                .unwrap_err()
                .code(),
            "invalid_workspace_lock_owner"
        );
        assert!(io.calls.borrow().is_empty());
    }
}

#[test]
fn full_reread_rejects_disabled_stale_schema_corruption_and_profile() {
    let temp = tempfile::tempdir().unwrap();
    let guard = WorkspaceGuard::execution_maintenance(temp.path(), Duration::ZERO).unwrap();
    let control = enabled();
    let selected = validated(&control).unwrap();
    let mut changed = control.clone();
    changed
        .set_field(
            "extensions",
            object(vec![("changed", Value::Bool(true))]).unwrap(),
        )
        .unwrap();
    for bad in [changed, upgraded(control.clone()), Value::Null] {
        save(temp.path(), &bad);
        let io = TestIo::default();
        assert!(confirm_with(temp.path(), &selected, &guard, &io).is_err());
        assert_eq!(*io.calls.borrow(), ["read"]);
    }
    let mut bad = control.clone();
    bad.set_field("schema_version", integer(3).unwrap())
        .unwrap();
    save(temp.path(), &bad);
    assert!(confirm(temp.path(), &selected, &guard).is_err());
    fs::write(
        temp.path().join("state/execution/control.json"),
        vec![b' '; values::MAX_INPUT + 1],
    )
    .unwrap();
    assert_eq!(
        confirm(temp.path(), &selected, &guard).unwrap_err().code(),
        "handoff_size_limit"
    );
    fs::remove_dir_all(temp.path().join("state/execution")).unwrap();
    assert_eq!(
        confirm(temp.path(), &selected, &guard).unwrap_err().code(),
        "execution_authority_changed_before_confirmation"
    );
    fs::create_dir(temp.path().join("state/execution")).unwrap();
    assert_eq!(
        confirm(temp.path(), &selected, &guard).unwrap_err().code(),
        "pending_execution_authority"
    );
}

#[test]
fn mismatching_authority_even_with_matching_revision_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let guard = WorkspaceGuard::execution_maintenance(temp.path(), Duration::ZERO).unwrap();
    let control = enabled();
    save(temp.path(), &control);
    let mut selected = validated(&control).unwrap();
    selected.authority = "0".repeat(32);
    let io = TestIo::default();
    assert_eq!(
        confirm_with(temp.path(), &selected, &guard, &io)
            .unwrap_err()
            .code(),
        "execution_authority_changed_before_confirmation"
    );
    assert_eq!(*io.calls.borrow(), ["read"]);
}

#[cfg(unix)]
#[test]
fn unsafe_control_and_directory_paths_are_hard_errors_before_open() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};
    for target in ["state/execution", "state/execution/control.json"] {
        for kind in ["symlink", "fifo", "wrong_type"] {
            let temp = tempfile::tempdir().unwrap();
            let guard = WorkspaceGuard::execution_maintenance(temp.path(), Duration::ZERO).unwrap();
            let control = enabled();
            let selected = validated(&control).unwrap();
            save(temp.path(), &control);
            let path = temp.path().join(target);
            if target.ends_with("json") {
                fs::remove_file(&path).unwrap();
            } else {
                fs::remove_dir_all(&path).unwrap();
            }
            match kind {
                "symlink" => symlink("missing", &path).unwrap(),
                "fifo" => {
                    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
                    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
                }
                _ if target.ends_with("json") => fs::create_dir(&path).unwrap(),
                _ => fs::write(&path, b"wrong type").unwrap(),
            }
            let io = TestIo::default();
            assert_eq!(
                confirm_with(temp.path(), &selected, &guard, &io)
                    .unwrap_err()
                    .code(),
                "unsafe_archive_path"
            );
            assert_eq!(*io.calls.borrow(), ["read"]);
        }
    }
}
