//! A real native preparer contends with a native-first writer. The historical
//! Python-specific qualification is retained evidence, not a live claim.
use super::*;
use std::{cell::RefCell, os::unix::fs::MetadataExt, rc::Rc, thread, time::Instant};

#[path = "tests/contender.rs"]
mod contender;

fn read_ipc(ipc: &Path, name: &str) -> Value {
    let bytes = fs::read(ipc.join(name)).unwrap();
    assert!(bytes.len() <= 16 * 1024);
    values::parse(&bytes, Profile::Authority).unwrap()
}

#[test]
fn native_first_assignment_makes_queued_python_preparation_stale_on_both_backends() {
    for active in [false, true] {
        let root = fixture(active);
        run_add(root.path(), active.then_some("add"), &io()).unwrap();
        let ipc = tempfile::tempdir().unwrap();
        initialize_execution(root.path(), false);
        fs::write(
            root.path().join("contender-ipc"),
            ipc.path().as_os_str().as_encoded_bytes(),
        )
        .unwrap();
        let control = root.path().join("state/execution/control.json");
        let frozen_control = fs::read(&control).unwrap();
        let frozen_history = execution::history(root.path()).unwrap();
        assert!(
            get(&frozen_history, "attempts")
                .as_object()
                .unwrap()
                .entries()
                .is_empty()
        );
        let (before_revision, before_tasks) = selected_tasks(root.path()).unwrap();
        let original = before_tasks.as_array().unwrap()[0].clone();
        assert_eq!(s(&original, "owner"), "Worker_ID");
        let lock = root.path().join("state/.workspace.lock");
        let identity = fs::metadata(&lock).unwrap();
        let identity = (identity.dev(), identity.ino());
        let preparer = Rc::new(RefCell::new(None));
        let queued = Rc::clone(&preparer);
        let ipc_path = ipc.path().to_owned();
        let old_revision = before_revision.clone();
        let old_execution_revision = s(&frozen_history, "revision").to_owned();
        let expected_control = frozen_control.clone();
        let test_io = TaskIo {
            held_hook: Some(Box::new(move |guard| {
                guard.validate_for(guard.root()).unwrap();
                let mut child = contender::Process::spawn(
                    concat!(module_path!(), "::queued_native_preparation_child"),
                    guard.root(),
                    "prepare",
                );
                let deadline = Instant::now() + Duration::from_secs(5);
                while !ipc_path.join("ready.json").exists() {
                    assert!(
                        child.child.try_wait().unwrap().is_none(),
                        "preparer exited before readiness"
                    );
                    assert!(Instant::now() < deadline, "preparer readiness timed out");
                    thread::sleep(Duration::from_millis(10));
                }
                let ready = read_ipc(&ipc_path, "ready.json");
                assert_eq!(
                    get(&ready, "task").canonical().unwrap(),
                    original.canonical().unwrap()
                );
                assert_eq!(s(&ready, "tasks_revision"), old_revision);
                assert_eq!(s(&ready, "execution_revision"), old_execution_revision);
                assert!(matches!(get(&ready, "lock_contended"), Value::Bool(true)));
                assert_eq!(
                    get(&ready, "lock_inode").compact().unwrap(),
                    identity.1.to_string().as_bytes()
                );
                assert!(!ipc_path.join("acquired.json").exists());
                assert!(!ipc_path.join("outcome.json").exists());
                assert!(child.child.try_wait().unwrap().is_none());
                assert_eq!(child.line(), "queued");
                child.send(b"release\n");
                assert_eq!(
                    fs::read(guard.root().join("state/execution/control.json")).unwrap(),
                    expected_control
                );
                *queued.borrow_mut() = Some(child);
            })),
            ..io()
        };
        let result = mutate(
            root.path(),
            Change::Assign {
                task_id: "test-title",
                agent: "other",
            },
            Some(&before_revision),
            active.then_some("native-assign"),
            &test_io,
        )
        .unwrap();
        assert_eq!(count(&test_io, "held"), 1);
        assert_eq!(s(status(&result), "outcome"), "committed");
        assert!(warnings(&result).is_empty());
        assert!(
            preparer
                .borrow_mut()
                .as_mut()
                .expect("held hook started the real preparer")
                .wait()
                .success()
        );
        let outcome = read_ipc(ipc.path(), "outcome.json");
        assert_eq!(s(&outcome, "error_type"), "NativeError");
        assert_eq!(s(&outcome, "error"), "execution_tasks_revision_conflict");
        assert_eq!(s(&outcome, "owner"), "other");
        assert_eq!(
            s(&outcome, "execution_revision"),
            s(&frozen_history, "revision")
        );
        assert_eq!(get(&outcome, "attempt_count").compact().unwrap(), b"0");
        assert!(matches!(
            get(&outcome, "prepare_receipt"),
            Value::Bool(false)
        ));
        let (after_revision, tasks) = selected_tasks(root.path()).unwrap();
        assert_ne!(before_revision, after_revision);
        assert_eq!(s(&outcome, "tasks_revision"), after_revision);
        let tasks = tasks.as_array().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(s(&tasks[0], "owner"), "other");
        let frozen = if active {
            get(&result, "result")
        } else {
            &result
        };
        assert_eq!(
            get(frozen, "task").canonical().unwrap(),
            tasks[0].canonical().unwrap()
        );
        assert_eq!(s(frozen, "before_revision"), before_revision);
        assert_eq!(s(frozen, "after_revision"), after_revision);
        assert_eq!(fs::read(&control).unwrap(), frozen_control);
        assert_eq!(
            execution::history(root.path())
                .unwrap()
                .canonical()
                .unwrap(),
            frozen_history.canonical().unwrap()
        );
        let after_lock = fs::metadata(&lock).unwrap();
        assert_eq!((after_lock.dev(), after_lock.ino()), identity);
        let acquired = read_ipc(ipc.path(), "acquired.json");
        assert_eq!(
            get(&acquired, "lock_inode").compact().unwrap(),
            identity.1.to_string().as_bytes()
        );
        let events = fs::read_to_string(root.path().join("logs/events.jsonl")).unwrap();
        let events: Vec<_> = events
            .lines()
            .map(|line| values::parse(line.as_bytes(), Profile::LegacySnapshot).unwrap())
            .collect();
        assert_eq!(events.len(), 2);
        assert_eq!(s(&events[0], "type"), "task.created");
        assert_eq!(s(&events[1], "type"), "task.assigned");
        assert_eq!(s(&events[1], "mutation_id"), s(frozen, "mutation_id"));
        assert_eq!(s(&events[1], "agent"), "other");
        assert!(
            fs::read_to_string(root.path().join("dashboard.md"))
                .unwrap()
                .contains("| `test-title` | assigned | other |")
        );
        if active {
            assert_eq!(get(frozen, "sequence").compact().unwrap(), b"2");
            let view = ledger::read(root.path()).unwrap().unwrap();
            assert_eq!(
                ledger::event_fields(&view).unwrap().compact().unwrap(),
                object(vec![
                    ("epoch_id", get(&events[1], "epoch_id").clone()),
                    ("sequence", get(&events[1], "sequence").clone()),
                    ("frame_digest", get(&events[1], "frame_digest").clone()),
                ])
                .unwrap()
                .compact()
                .unwrap()
            );
        }
        assert_eq!(render_result(&result, true, "assign").unwrap().exit_code, 0);
    }
}

fn publish(ipc: &Path, name: &str, value: Value) {
    let temporary = ipc.join(format!("{name}.tmp"));
    fs::write(&temporary, value.compact().unwrap()).unwrap();
    fs::rename(temporary, ipc.join(format!("{name}.json"))).unwrap();
}

#[test]
#[ignore = "IPC child entry; exercised by native_first_assignment_makes_queued_python_preparation_stale_on_both_backends"]
fn queued_native_preparation_child() {
    let root = PathBuf::from(std::env::var_os("COVEN_SOURCE_CONTENDER_ROOT").unwrap());
    let ipc = fs::read_to_string(root.join("contender-ipc"))
        .map(PathBuf::from)
        .unwrap();
    let history = execution::history(&root).unwrap();
    let execution_revision = s(&history, "revision").to_owned();
    let (tasks_revision, tasks) = selected_tasks(&root).unwrap();
    let task = tasks.as_array().unwrap()[0].clone();
    let lock_path = root.join("state/.workspace.lock");
    let file = contender::open(&lock_path);
    let inode = file.metadata().unwrap().ino();
    assert!(!contender::lock(&file, true, true));
    publish(
        &ipc,
        "ready",
        object(vec![
            ("task", task),
            ("tasks_revision", text(&tasks_revision)),
            ("execution_revision", text(&execution_revision)),
            ("lock_contended", Value::Bool(true)),
            ("lock_inode", Value::integer(&inode.to_string()).unwrap()),
        ])
        .unwrap(),
    );
    contender::signal("queued");
    contender::receive();
    assert!(contender::lock(&file, true, false));
    assert_eq!(fs::metadata(lock_path).unwrap().ino(), inode);
    publish(
        &ipc,
        "acquired",
        object(vec![(
            "lock_inode",
            Value::integer(&inode.to_string()).unwrap(),
        )])
        .unwrap(),
    );
    drop(file);
    let intent = ipc.join("intent.json");
    fs::write(&intent, br#"{"schema_version":1,"kind":"task-intent","task_id":"test-title","owner":"Worker_ID","acceptance_criteria":[{"id":"a","text":"explicit evidence"}]}"#).unwrap();
    let result = execution::prepare_arguments(
        &[root.clone(), PathBuf::from("test-title")],
        execution::PreparationInput::Intent(&intent),
        execution::WriteOptions {
            expected_execution_revision: &execution_revision,
            expected_tasks_revision: Some(&tasks_revision),
            request_id: "queued-prepare",
            json: true,
        },
    );
    let error = result.err().expect("stale preparation must fail");
    assert_eq!(error.code(), "execution_tasks_revision_conflict");
    let current = execution::history(&root).unwrap();
    let (revision, tasks) = selected_tasks(&root).unwrap();
    let attempts = get(&current, "attempts")
        .as_object()
        .unwrap()
        .entries()
        .len();
    let control = fs::read_to_string(root.join("state/execution/control.json")).unwrap();
    publish(
        &ipc,
        "outcome",
        object(vec![
            ("error_type", text("NativeError")),
            ("error", text(error.code())),
            ("tasks_revision", text(revision)),
            ("execution_revision", get(&current, "revision").clone()),
            ("owner", get(&tasks.as_array().unwrap()[0], "owner").clone()),
            (
                "attempt_count",
                Value::integer(&attempts.to_string()).unwrap(),
            ),
            (
                "prepare_receipt",
                Value::Bool(control.contains("queued-prepare")),
            ),
        ])
        .unwrap(),
    );
}
