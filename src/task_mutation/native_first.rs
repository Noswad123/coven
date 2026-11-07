//! A real Python preparer contends with a native-first writer, never an in-process
//! reentrant lock or a manufactured execution permission.
use super::*;
use std::{
    cell::RefCell,
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::unix::{fs::MetadataExt, process::CommandExt},
    process::{Child, Command, ExitStatus, Stdio},
    rc::Rc,
    thread,
    time::Instant,
};

// Anonymous file-backed diagnostics cannot fill a pipe while the native guard
// waits for readiness. Every child has a deadline and its own cleanup group.
struct PythonProcess {
    child: Child,
    stdout: File,
    stderr: File,
}
impl PythonProcess {
    fn spawn(mode: &str, root: &Path, ipc: &Path) -> Self {
        let stdout = tempfile::tempfile().unwrap();
        let stderr = tempfile::tempfile().unwrap();
        let child = Command::new("python3")
            .args(["-c", SCRIPT, mode])
            .arg(root)
            .arg(ipc)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("COVEN_MULTIPLEXER", "")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdin(Stdio::null())
            .stdout(stdout.try_clone().unwrap())
            .stderr(stderr.try_clone().unwrap())
            .process_group(0)
            .spawn()
            .unwrap();
        Self {
            child,
            stdout,
            stderr,
        }
    }
    fn diagnostics(&mut self) -> String {
        let mut bytes = Vec::new();
        for file in [&mut self.stdout, &mut self.stderr] {
            file.seek(SeekFrom::Start(0)).unwrap();
            file.take(16 * 1024).read_to_end(&mut bytes).unwrap();
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }
    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "Python fixture timed out: {}",
                self.diagnostics()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn success(&mut self) {
        let status = self.wait();
        assert!(
            status.success(),
            "Python fixture failed: {}",
            self.diagnostics()
        );
    }
}
impl Drop for PythonProcess {
    fn drop(&mut self) {
        if self
            .child
            .try_wait()
            .map_or(true, |status| status.is_none())
        {
            // SAFETY: process_group(0) created this owned child's isolated group.
            unsafe { libc::kill(-(self.child.id() as libc::pid_t), libc::SIGKILL) };
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

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
        PythonProcess::spawn("initialize", root.path(), ipc.path()).success();
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
                let mut child = PythonProcess::spawn("prepare", guard.root(), &ipc_path);
                let deadline = Instant::now() + Duration::from_secs(5);
                while !ipc_path.join("ready.json").exists() {
                    assert!(
                        child.child.try_wait().unwrap().is_none(),
                        "preparer exited before readiness: {}",
                        child.diagnostics()
                    );
                    assert!(
                        Instant::now() < deadline,
                        "preparer readiness timed out: {}",
                        child.diagnostics()
                    );
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
        preparer
            .borrow_mut()
            .as_mut()
            .expect("held hook started the real preparer")
            .success();
        let outcome = read_ipc(ipc.path(), "outcome.json");
        assert_eq!(s(&outcome, "error_type"), "AttemptError");
        assert_eq!(s(&outcome, "error"), "tasks revision conflict");
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

const SCRIPT: &str = r#"
import copy, fcntl, json, os, sys
from pathlib import Path
from runner import coven, storage, task_attempts
mode, root, ipc = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])
store = task_attempts.Store(root)
def signal(name, value):
    temporary = ipc / (name + '.tmp')
    temporary.write_text(json.dumps(value), encoding='utf-8')
    os.replace(temporary, ipc / (name + '.json'))
if mode == 'initialize':
    with storage.workspace_lock(root, root / 'state/.workspace.lock') as held:
        for operation, parameters in [('enable', {}), ('enroll', {'task_id':'test-title', 'rebind_source':False})]:
            view = store.read()
            store.mutate(operation, parameters,
                expected_execution_revision=view.revision if view else task_attempts.DISABLED,
                expected_tasks_revision=coven.read_task_snapshot(root)[0], request_id=operation,
                source_loader=lambda sid: coven.execution_source(root, sid), lock=held)
    assert not store.read().attempts
else:
    view = store.read()
    source = coven.execution_source(root, view.snapshot_source_id)
    task = copy.deepcopy(next(t for t in source['tasks'] if t['id'] == 'test-title'))
    expected_execution, expected_tasks = view.revision, source['revision']
    path = root / 'state/.workspace.lock'
    inode = path.stat().st_ino
    fd = storage._open_regular(path, os.O_RDWR)
    try:
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            contended = True
        else:
            raise AssertionError('native writer did not hold the persistent lock')
    finally:
        os.close(fd)
    signal('ready', {'task':task, 'tasks_revision':expected_tasks,
        'execution_revision':expected_execution, 'lock_contended':contended, 'lock_inode':inode})
    with storage.workspace_lock(root, path, timeout=5.0) as held:
        assert path.stat().st_ino == inode
        signal('acquired', {'lock_inode':inode})
        observed = []
        def fresh_source(sid):
            current = coven.execution_source(root, sid)
            observed.append(current)
            return current
        try:
            store.mutate('prepare', {'task_id':'test-title', 'intent':{
                'schema_version':1, 'kind':'task-intent', 'task_id':'test-title', 'owner':task['owner'],
                'acceptance_criteria':[{'id':'a', 'text':'explicit evidence'}]}},
                expected_execution_revision=expected_execution, expected_tasks_revision=expected_tasks,
                request_id='queued-prepare', source_loader=fresh_source, lock=held)
        except task_attempts.AttemptError as error:
            assert str(error) == 'tasks revision conflict', str(error)
            assert len(observed) == 1
            assert observed[0]['revision'] != expected_tasks
            assert observed[0]['identity'] == source['identity']
            current = store.read()
            assert current.revision == expected_execution
            assert not current.attempts
            assert 'queued-prepare' not in current.receipts
            signal('outcome', {'error_type':type(error).__name__, 'error':str(error),
                'tasks_revision':observed[0]['revision'], 'execution_revision':current.revision,
                'owner':next(t['owner'] for t in observed[0]['tasks'] if t['id'] == 'test-title'),
                'attempt_count':len(current.attempts), 'prepare_receipt':'queued-prepare' in current.receipts})
        else:
            raise AssertionError('queued preparation unexpectedly succeeded with stale task pins')
"#;
