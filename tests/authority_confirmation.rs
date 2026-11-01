//! Real fsync SDK parity. Fault/race injection and exact syscall traces are core
//! unit-test responsibilities; no CLI, delivery, runtime, or production failpoint.
#![cfg(unix)]

use coven::{
    execution::{self, ReservationCheck},
    ledger::{self, LedgerConfirmation},
    storage::WorkspaceGuard,
    values::{self, Profile, Value},
};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom},
    os::unix::{fs::MetadataExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct OwnedChild(Option<Child>, bool);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            // Only the outer watchdog owns a process group. Nested children stay
            // in that group so its timeout also cleans up their descendants.
            if self.1 {
                unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Entry {
    mode: u32,
    device: u64,
    inode: u64,
    mtime: (i64, i64),
    bytes: Vec<u8>,
    link: Option<PathBuf>,
}
fn inventory(root: &Path) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, path: &Path, entries: &mut BTreeMap<PathBuf, Entry>) {
        let m = fs::symlink_metadata(path).unwrap();
        entries.insert(
            path.strip_prefix(root).unwrap().to_owned(),
            Entry {
                mode: m.mode(),
                device: m.dev(),
                inode: m.ino(),
                mtime: (m.mtime(), m.mtime_nsec()),
                bytes: if m.is_file() {
                    fs::read(path).unwrap()
                } else {
                    vec![]
                },
                link: m
                    .file_type()
                    .is_symlink()
                    .then(|| fs::read_link(path).unwrap()),
            },
        );
        if m.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), entries);
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}
fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.as_object().unwrap().get(key).unwrap()
}
fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    field(value, key).as_str().unwrap()
}
fn python(root: &Path) -> Command {
    let project = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut command = Command::new("python3");
    command
        .arg(project.join("tests/_authority_confirmation.py"))
        .arg("--root")
        .arg(root)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONINTMAXSTRDIGITS", "4300")
        .env("COVEN_MULTIPLEXER", "")
        .current_dir(project);
    command
}
fn run(command: &mut Command) -> Vec<u8> {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn lock_state(root: &Path, expected: &str) {
    assert_eq!(
        String::from_utf8(run(python(root).arg("--try-lock")))
            .unwrap()
            .trim(),
        expected
    );
}
fn reservation(result: coven::Result<ReservationCheck>) -> String {
    match result {
        Ok(ReservationCheck::DisabledObserved) => "DisabledObserved".into(),
        Ok(ReservationCheck::ActiveConfirmedClear) => "ActiveConfirmedClear".into(),
        Err(error) => error.code().to_owned(),
    }
}
fn confirmation(result: coven::Result<LedgerConfirmation>) -> String {
    match result {
        Ok(LedgerConfirmation::Confirmed) => "Confirmed".into(),
        Ok(LedgerConfirmation::SynchronizationUncertain) => "SynchronizationUncertain".into(),
        Err(error) => error.code().to_owned(),
    }
}
fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.exists() {
        assert!(Instant::now() < deadline, "IPC timeout: {}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn reap(child: &mut OwnedChild) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.0.as_mut().unwrap().try_wait().unwrap() {
            assert!(status.success(), "fixture/probe child failed: {status}");
            child.0.take();
            return;
        }
        assert!(Instant::now() < deadline, "fixture/probe child timeout");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn native_confirmation_probe() {
    let Some(root) = std::env::var_os("COVEN_CONFIRMATION_PROBE_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let ipc = PathBuf::from(std::env::var_os("COVEN_CONFIRMATION_PROBE_IPC").unwrap());
    let action = std::env::var("COVEN_CONFIRMATION_PROBE_ACTION").unwrap();
    // Capture the old opaque View while Python holds the lock but before its
    // explicit mutation. No public head-digest field or fabricated View needed.
    let old = (action == "append").then(|| ledger::read(&root).unwrap().unwrap());
    fs::write(ipc.join("waiting"), b"").unwrap();
    let guard = WorkspaceGuard::existing(&root, Duration::from_secs(10)).unwrap();
    let before = inventory(&root);
    let actual = if let Some(old) = old {
        assert_ne!(
            ledger::read(&root).unwrap().unwrap().revision(),
            old.revision()
        );
        let result = ledger::confirm_selected(&root, &old, &guard);
        assert!(
            result.is_err(),
            "old head confirmed after waiting for Python append"
        );
        "refusal".to_owned()
    } else {
        reservation(execution::guard_task_mutation(&root, "a", &guard))
    };
    assert_eq!(
        inventory(&root),
        before,
        "SDK changed workspace under borrowed lock"
    );
    guard.validate_for(&root).unwrap();
    fs::write(ipc.join("checked"), actual).unwrap();
    wait_file(&ipc.join("release-native"));
    drop(guard);
}

fn lock_pairs(root: &Path, summary: &Value) {
    let (mut python_first, mut native_first) = (0, 0);
    for action in ["append", "prepare", "cancel"] {
        let workspace = Path::new(text(field(summary, "locks"), action));
        let ipc = root.join(format!("ipc-{action}"));
        fs::create_dir(&ipc).unwrap();
        let lock = workspace.join("state/.workspace.lock");
        let before = fs::metadata(&lock).unwrap();
        let mut writer = OwnedChild(
            Some(
                python(workspace)
                    .arg("--hold")
                    .arg(&ipc)
                    .arg("--action")
                    .arg(action)
                    .spawn()
                    .unwrap(),
            ),
            false,
        );
        wait_file(&ipc.join("ready"));
        let mut probe = OwnedChild(
            Some(
                Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "native_confirmation_probe", "--nocapture"])
                    .env("COVEN_CONFIRMATION_PROBE_ROOT", workspace)
                    .env("COVEN_CONFIRMATION_PROBE_IPC", &ipc)
                    .env("COVEN_CONFIRMATION_PROBE_ACTION", action)
                    .spawn()
                    .unwrap(),
            ),
            false,
        );
        wait_file(&ipc.join("waiting"));
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            !ipc.join("checked").exists(),
            "native bypassed Python flock"
        );
        lock_state(workspace, "busy");
        // Explicit fixture mutation window ends when Python releases its flock.
        fs::write(ipc.join("release"), b"").unwrap();
        reap(&mut writer);
        wait_file(&ipc.join("checked"));
        assert_eq!(
            fs::read_to_string(ipc.join("checked")).unwrap(),
            match action {
                "append" => "refusal",
                "prepare" => "prepared_attempt_blocks_mutation",
                "cancel" => "ActiveConfirmedClear",
                _ => unreachable!(),
            }
        );
        python_first += 1;
        // Reverse direction, including SDK errors: only the owner may release.
        let snapshot = inventory(workspace);
        lock_state(workspace, "busy");
        assert_eq!(inventory(workspace), snapshot);
        fs::write(ipc.join("release-native"), b"").unwrap();
        reap(&mut probe);
        lock_state(workspace, "clear");
        let after = fs::metadata(lock).unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        native_first += 1;
    }
    assert_eq!((python_first, native_first), (3, 3));
    println!("confirmation locks: {python_first} Python-first + {native_first} native-first pairs");
}

#[test]
fn python_authority_confirmation_corpus() {
    const CHILD_ROOT: &str = "COVEN_CONFIRMATION_CORPUS_ROOT";
    let root = match std::env::var_os(CHILD_ROOT) {
        Some(root) => PathBuf::from(root),
        None => {
            // Parent owns fixtures and output files, even if a FIFO regression
            // wedges a descendant. No pipe saturation and no orphaned fixtures.
            let temporary = tempfile::tempdir().unwrap();
            let mut stdout = tempfile::tempfile().unwrap();
            let mut stderr = tempfile::tempfile().unwrap();
            let mut child = OwnedChild(
                Some(
                    Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "python_authority_confirmation_corpus",
                            "--nocapture",
                        ])
                        .env(CHILD_ROOT, temporary.path())
                        .process_group(0)
                        .stdout(Stdio::from(stdout.try_clone().unwrap()))
                        .stderr(Stdio::from(stderr.try_clone().unwrap()))
                        .spawn()
                        .unwrap(),
                ),
                true,
            );
            let deadline = Instant::now() + Duration::from_secs(120);
            let status = loop {
                if let Some(status) = child.0.as_mut().unwrap().try_wait().unwrap() {
                    break Some(status);
                }
                if Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(25));
            };
            drop(child);
            stdout.seek(SeekFrom::Start(0)).unwrap();
            stderr.seek(SeekFrom::Start(0)).unwrap();
            let (mut out, mut err) = (String::new(), String::new());
            stdout.read_to_string(&mut out).unwrap();
            stderr.read_to_string(&mut err).unwrap();
            print!("{out}");
            eprint!("{err}");
            assert!(status.is_some(), "confirmation corpus exceeded 120 seconds");
            assert!(
                status.unwrap().success(),
                "confirmation corpus child failed"
            );
            return;
        }
    };
    let output = run(&mut python(&root));
    let mut counts = BTreeMap::<String, usize>::new();
    let mut summary = None;
    for line in output
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
    {
        let case = values::parse(line, Profile::Authority).unwrap();
        if text(&case, "kind") == "summary" {
            assert!(summary.replace(case).is_none());
            continue;
        }
        assert!(summary.is_none());
        assert_eq!(text(&case, "kind"), "case");
        let workspace = Path::new(text(&case, "path"));
        assert!(workspace.starts_with(&root));
        let expected = text(&case, "expected");
        let label = text(&case, "label");
        let initial = inventory(workspace);
        let captured = if text(&case, "api") == "ledger" {
            Some(ledger::read(workspace))
        } else {
            None
        };
        assert_eq!(inventory(workspace), initial, "capture wrote: {label}");
        if let Some(action) = field(&case, "action").as_str() {
            assert!(captured.as_ref().unwrap().as_ref().unwrap().is_some());
            run(python(workspace).arg("--action").arg(action));
            if action == "head-only" {
                assert_eq!(
                    ledger::read(workspace).unwrap().unwrap().revision(),
                    captured
                        .as_ref()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .revision()
                );
            }
        }
        let before = inventory(workspace);
        let guard = WorkspaceGuard::existing(workspace, Duration::ZERO).unwrap();
        let actual = if let Some(captured) = captured {
            match captured {
                Err(_) => "refusal".to_owned(),
                Ok(None) => "absent".to_owned(),
                Ok(Some(view)) => confirmation(ledger::confirm_selected(workspace, &view, &guard)),
            }
        } else if text(&case, "api") == "task" {
            reservation(execution::guard_task_mutation(
                workspace,
                text(&case, "task"),
                &guard,
            ))
        } else {
            reservation(execution::guard_authority_change(workspace, &guard))
        };
        match expected {
            "refusal" => assert!(
                ![
                    "Confirmed",
                    "SynchronizationUncertain",
                    "DisabledObserved",
                    "ActiveConfirmedClear",
                    "prepared_attempt_blocks_mutation",
                    "execution_synchronization_uncertain"
                ]
                .contains(&actual.as_str()),
                "{label}: {actual}"
            ),
            "profile_refusal" => assert_eq!(actual, "handoff_size_limit", "{label}"),
            _ => assert_eq!(actual, expected, "{label}"),
        }
        guard.validate_for(workspace).unwrap();
        lock_state(workspace, "busy");
        assert_eq!(
            inventory(workspace),
            before,
            "SDK changed namespace/bytes/mode/inode/mtime: {label}"
        );
        drop(guard);
        lock_state(workspace, "clear");
        assert_eq!(
            inventory(workspace),
            before,
            "guard release changed workspace: {label}"
        );
        *counts
            .entry(text(&case, "category").to_owned())
            .or_default() += 1;
    }
    let summary = summary.unwrap();
    assert_eq!(
        field(&summary, "counts")
            .as_object()
            .unwrap()
            .entries()
            .len(),
        counts.len()
    );
    for (category, count) in &counts {
        assert_eq!(
            field(field(&summary, "counts"), category)
                .canonical()
                .unwrap(),
            count.to_string().as_bytes()
        );
    }
    let total = counts.values().sum::<usize>();
    assert_eq!(
        field(&summary, "total").canonical().unwrap(),
        total.to_string().as_bytes()
    );
    // Independently pin coverage: a generator and its summary shrinking together
    // must not silently remove authority-confirmation regression cases.
    assert_eq!(total, 95);
    assert_eq!(
        counts,
        BTreeMap::from([
            ("Confirmed".to_owned(), 24),
            ("ActiveConfirmedClear".to_owned(), 30),
            ("DisabledObserved".to_owned(), 2),
            ("prepared_attempt_blocks_mutation".to_owned(), 6),
            ("invalid".to_owned(), 31),
            ("native_profile".to_owned(), 1),
            ("absent".to_owned(), 1),
        ])
    );
    println!("authority confirmation parity: {total} cases, {counts:?}");
    lock_pairs(&root, &summary);
}

#[test]
fn inherited_workspace_guard_requires_a_regular_marker_not_parsed_configuration() {
    let root = tempfile::tempdir().unwrap();
    // Python guards accept roots without coven.json. Existing native guard
    // acquisition is intentionally narrower; confirmation does not widen it.
    assert!(WorkspaceGuard::acquire(root.path(), Duration::ZERO).is_err());
    fs::write(root.path().join("coven.json"), b"not JSON\n").unwrap();
    let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap();
    let before = inventory(root.path());
    assert_eq!(
        reservation(execution::guard_authority_change(root.path(), &guard)),
        "DisabledObserved"
    );
    assert_eq!(inventory(root.path()), before);
    drop(guard);
    fs::remove_file(root.path().join("coven.json")).unwrap();
    assert!(WorkspaceGuard::existing(root.path(), Duration::ZERO).is_err());
}
