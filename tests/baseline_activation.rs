//! Real Python/native SDK activation and recovery. Fault injection, deterministic
//! IDs and syscall ordering belong to core units, not public integration hooks.
#![cfg(unix)]

use coven::{
    ledger::{self, ActivationDurability, ArchiveDurability, ImportProgress},
    storage::WorkspaceGuard,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Seek, SeekFrom},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct OwnedChild(Option<Child>, bool);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            if self.1 {
                unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap()
}
fn python(executable: &Path, root: &Path) -> Command {
    let project = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut command = Command::new(executable);
    command
        .arg(project.join("tests/_baseline_activation.py"))
        .arg("--root")
        .arg(root)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONINTMAXSTRDIGITS", "4300")
        .env("COVEN_MULTIPLEXER", "")
        .current_dir(project);
    command
}
fn run(command: &mut Command) -> Vec<u8> {
    // File-backed output cannot deadlock on a full pipe. Every subprocess is
    // bounded by the enclosing process-group watchdog as well as this deadline.
    let mut stdout = tempfile::tempfile().unwrap();
    let mut stderr = tempfile::tempfile().unwrap();
    let mut child = OwnedChild(
        Some(
            command
                .stdout(Stdio::from(stdout.try_clone().unwrap()))
                .stderr(Stdio::from(stderr.try_clone().unwrap()))
                .spawn()
                .unwrap(),
        ),
        false,
    );
    let deadline = Instant::now() + Duration::from_secs(40);
    let status = loop {
        if let Some(status) = child.0.as_mut().unwrap().try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "subprocess timeout: {command:?}");
        std::thread::sleep(Duration::from_millis(10));
    };
    child.0.take();
    stdout.seek(SeekFrom::Start(0)).unwrap();
    stderr.seek(SeekFrom::Start(0)).unwrap();
    let (mut out, mut err) = (Vec::new(), String::new());
    stdout.read_to_end(&mut out).unwrap();
    stderr.read_to_string(&mut err).unwrap();
    assert!(
        status.success(),
        "{command:?}: {status}\n{}\n{err}",
        String::from_utf8_lossy(&out)
    );
    out
}
fn lock_state(python_exe: &Path, root: &Path, expected: &str) {
    assert_eq!(
        String::from_utf8(run(python(python_exe, root).arg("--try-lock")))
            .unwrap()
            .trim(),
        expected
    );
}
fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.exists() {
        assert!(Instant::now() < deadline, "IPC timeout: {}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn reap(child: &mut OwnedChild) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.0.as_mut().unwrap().try_wait().unwrap() {
            assert!(status.success(), "probe failed: {status}");
            child.0.take();
            return;
        }
        assert!(Instant::now() < deadline, "probe timeout");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn check_transition(
    before: &BTreeMap<PathBuf, Entry>,
    after: &BTreeMap<PathBuf, Entry>,
    added: BTreeSet<PathBuf>,
    archive: Option<&Path>,
) {
    let mut expected = before.clone();
    if let Some(destination) = archive {
        // A rename must preserve every entry's inode, raw bytes and mode,
        // including corrupt files, symlinks and FIFOs. No blanket write skip.
        let old = Path::new(ledger::DIRECTORY);
        for (path, entry) in before.iter().filter(|(path, _)| path.starts_with(old)) {
            expected.remove(path);
            expected.insert(
                destination.join(path.strip_prefix(old).unwrap()),
                entry.clone(),
            );
        }
    }
    for path in &added {
        assert!(
            !expected.contains_key(path),
            "replaced existing artifact: {path:?}"
        );
        expected.insert(path.clone(), after.get(path).unwrap().clone());
    }
    // Only state gains/loses immediate entries during these operations. Every
    // other directory timestamp, and all file timestamps, remain protected.
    if !added.is_empty() || archive.is_some() {
        expected.get_mut(Path::new("state")).unwrap().mtime = after[Path::new("state")].mtime;
    }
    assert_eq!(
        &expected, after,
        "unexpected namespace/bytes/mode/inode/mtime effect"
    );
}

fn imported_paths(root: &Path) -> BTreeSet<PathBuf> {
    let raw = fs::read(root.join(ledger::MARKER)).unwrap();
    let marker: Value = serde_json::from_slice(&raw).unwrap();
    let base = Path::new(ledger::DIRECTORY).join(text(&marker, "epoch_id"));
    let mut paths = BTreeSet::from([
        PathBuf::from(ledger::MARKER),
        PathBuf::from(ledger::DIRECTORY),
        base.clone(),
        base.join("frames"),
        base.join("frames/00000000000000000000.frame"),
        base.join("manifest.json"),
    ]);
    if root.join("state/tasks.json").exists() {
        paths.insert(base.join("baseline.tasks.bytes"));
    }
    for path in &paths {
        let mode = fs::metadata(root.join(path)).unwrap().mode() & 0o7777;
        if root.join(path).is_dir() {
            // Native requests 0700, subject to umask. Inherited special bits
            // are not access permissions; exact access modes are probed below.
            assert_eq!(mode & 0o777 & !0o700, 0, "directory permissions: {path:?}");
        } else if path.file_name().unwrap() == "baseline.tasks.bytes" {
            assert_eq!(
                mode,
                fs::metadata(root.join("state/tasks.json")).unwrap().mode() & 0o7777
            );
        } else {
            assert_eq!(mode, 0o600, "artifact permissions: {path:?}");
        }
    }
    paths
}

#[test]
fn native_activation_directory_mode_probe() {
    let Some(root) = std::env::var_os("COVEN_ACTIVATION_MODE_PROBE_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let mask = u32::from_str_radix(
        &std::env::var("COVEN_ACTIVATION_MODE_PROBE_UMASK").unwrap(),
        8,
    )
    .unwrap();
    assert!(matches!(mask, 0 | 0o077));
    let sgid = std::env::var("COVEN_ACTIVATION_MODE_PROBE_SGID").unwrap() == "1";
    let state = root.join("state");
    fs::create_dir(&state).unwrap();
    // Keep the fixture owned by this user and their effective group, so setting
    // SGID needs no administrative privileges or supplementary-group setup.
    assert_eq!(fs::metadata(&state).unwrap().uid(), unsafe {
        libc::geteuid()
    });
    std::os::unix::fs::chown(&state, None, Some(unsafe { libc::getegid() })).unwrap();
    fs::set_permissions(
        &state,
        fs::Permissions::from_mode(if sgid { 0o2700 } else { 0o700 }),
    )
    .unwrap();
    let parent = fs::metadata(&state).unwrap();
    assert_eq!(parent.gid(), unsafe { libc::getegid() });
    assert_eq!(parent.mode() & 0o7777, if sgid { 0o2700 } else { 0o700 });
    fs::write(root.join("coven.json"), b"not parsed configuration\n").unwrap();
    let source = state.join("tasks.json");
    fs::write(&source, b" [] \r\n").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o640)).unwrap();
    let guard = WorkspaceGuard::acquire(&root, Duration::ZERO).unwrap();
    let before = inventory(&root);

    // Only the --exact, --test-threads=1 subprocess ever changes its umask.
    // Restore it on both successful return and assertion unwinding.
    struct RestoreUmask(libc::mode_t);
    impl Drop for RestoreUmask {
        fn drop(&mut self) {
            unsafe { libc::umask(self.0) };
        }
    }
    let restore = RestoreUmask(unsafe { libc::umask(mask as libc::mode_t) });
    let report =
        ledger::import_tasks(&root, &ledger::snapshot_revision(Some(b" [] \r\n")), &guard).unwrap();
    assert_eq!(report.durability(), ActivationDurability::Confirmed);
    let added = imported_paths(&root);
    let mut directories = 0;
    for path in &added {
        let metadata = fs::metadata(root.join(path)).unwrap();
        if metadata.is_dir() {
            // Checking metadata, not writability, catches an accidental 0500
            // request even when the test is running with root privileges.
            assert_eq!(
                metadata.mode() & 0o777,
                0o700 & !mask,
                "{path:?}, umask={mask:03o}, SGID parent={sgid}"
            );
            directories += 1;
        }
    }
    assert_eq!(directories, 3);
    check_transition(&before, &inventory(&root), added, None);
    guard.validate_for(&root).unwrap();
    drop(restore);
    println!(
        "activation directory modes: umask={mask:03o}, SGID parent={sgid}, {directories} exact access-mode checks"
    );
}

#[test]
fn activation_directory_modes_are_umask_scoped() {
    for mask in ["000", "077"] {
        for sgid in ["0", "1"] {
            // Parent owns the fixture until its bounded process-group probe is
            // reaped, including when a regression wedges activation on I/O.
            let temporary = tempfile::tempdir().unwrap();
            let mut child = OwnedChild(
                Some(
                    Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "native_activation_directory_mode_probe",
                            "--nocapture",
                            "--test-threads=1",
                        ])
                        .env("COVEN_ACTIVATION_MODE_PROBE_ROOT", temporary.path())
                        .env("COVEN_ACTIVATION_MODE_PROBE_UMASK", mask)
                        .env("COVEN_ACTIVATION_MODE_PROBE_SGID", sgid)
                        .process_group(0)
                        .spawn()
                        .unwrap(),
                ),
                true,
            );
            reap(&mut child);
        }
    }
    println!(
        "activation directory modes: 4 isolated probes, 12 exact access-mode checks (separate from the 133-case corpus)"
    );
}

#[test]
fn native_activation_probe() {
    let Some(root) = std::env::var_os("COVEN_ACTIVATION_PROBE_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let ipc = PathBuf::from(std::env::var_os("COVEN_ACTIVATION_PROBE_IPC").unwrap());
    let operation = std::env::var("COVEN_ACTIVATION_PROBE_OPERATION").unwrap();
    let raw = fs::read(root.join("state/tasks.json")).ok();
    let token = ledger::snapshot_revision(raw.as_deref());
    fs::write(ipc.join("waiting-native"), b"").unwrap();
    let guard = WorkspaceGuard::existing(&root, Duration::from_secs(15)).unwrap();
    let before = inventory(&root);
    let result = if operation == "resume" {
        ledger::resume_import(&root, &token, &guard)
    } else {
        ledger::import_tasks(&root, &token, &guard)
    };
    let actual = match result {
        Ok(report) => {
            assert_eq!(report.durability(), ActivationDurability::Confirmed);
            let paths = if operation == "resume" {
                BTreeSet::from([PathBuf::from(ledger::MARKER)])
            } else {
                imported_paths(&root)
            };
            check_transition(&before, &inventory(&root), paths, None);
            "ok".to_owned()
        }
        Err(error) => {
            assert_eq!(error.progress(), &ImportProgress::NoNewArtifacts);
            assert_eq!(inventory(&root), before);
            error.code().to_owned()
        }
    };
    guard.validate_for(&root).unwrap();
    fs::write(ipc.join("checked"), actual).unwrap();
    wait_file(&ipc.join("release-native"));
    drop(guard);
}

fn lock_pairs(root: &Path, python_exe: &Path, summary: &Value) {
    let mut count = 0;
    for (name, action, expected, operation) in [
        (
            "clear",
            "prepare",
            "prepared_attempt_blocks_mutation",
            "import",
        ),
        ("prepared", "cancel", "ok", "import"),
        (
            "resume",
            "prepare",
            "prepared_attempt_blocks_mutation",
            "resume",
        ),
    ] {
        let workspace = Path::new(text(&summary["locks"], name));
        let ipc = root.join(format!("ipc-python-first-{name}"));
        fs::create_dir(&ipc).unwrap();
        let lock = workspace.join("state/.workspace.lock");
        let original = fs::metadata(&lock).unwrap();
        let mut writer = OwnedChild(
            Some(
                python(python_exe, workspace)
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
        let mut native = OwnedChild(
            Some(
                Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "native_activation_probe", "--nocapture"])
                    .env("COVEN_ACTIVATION_PROBE_ROOT", workspace)
                    .env("COVEN_ACTIVATION_PROBE_IPC", &ipc)
                    .env("COVEN_ACTIVATION_PROBE_OPERATION", operation)
                    .spawn()
                    .unwrap(),
            ),
            false,
        );
        wait_file(&ipc.join("waiting-native"));
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            !ipc.join("checked").exists(),
            "native bypassed Python flock"
        );
        fs::write(ipc.join("release"), b"").unwrap();
        reap(&mut writer);
        wait_file(&ipc.join("checked"));
        assert_eq!(fs::read_to_string(ipc.join("checked")).unwrap(), expected);
        lock_state(python_exe, workspace, "busy");
        // In the reverse lock order a real Python preparation must also wait.
        let reverse = root.join(format!("ipc-native-first-{name}"));
        fs::create_dir(&reverse).unwrap();
        let mut writer = OwnedChild(
            Some(
                python(python_exe, workspace)
                    .arg("--hold")
                    .arg(&reverse)
                    .arg("--action")
                    .arg(if expected == "ok" {
                        "prepare-current"
                    } else {
                        "cancel"
                    })
                    .spawn()
                    .unwrap(),
            ),
            false,
        );
        wait_file(&reverse.join("waiting-python"));
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            !reverse.join("ready").exists(),
            "Python bypassed native flock"
        );
        let snapshot = inventory(workspace);
        fs::write(ipc.join("release-native"), b"").unwrap();
        reap(&mut native);
        wait_file(&reverse.join("ready"));
        assert_eq!(
            inventory(workspace),
            snapshot,
            "lock handoff changed artifacts"
        );
        fs::write(reverse.join("release"), b"").unwrap();
        reap(&mut writer);
        if expected == "ok" {
            assert!(reverse.join("stale-binding").exists());
        }
        lock_state(python_exe, workspace, "clear");
        let current = fs::metadata(lock).unwrap();
        assert_eq!(
            (original.dev(), original.ino()),
            (current.dev(), current.ino())
        );
        count += 2;
    }
    assert_eq!(count, 6);
    println!("activation locks: 3 Python-first + 3 native-first pairs");
}

#[test]
fn python_baseline_activation_corpus() {
    const CHILD_ROOT: &str = "COVEN_ACTIVATION_CORPUS_ROOT";
    let root = match std::env::var_os(CHILD_ROOT) {
        Some(root) => PathBuf::from(root),
        None => {
            let temporary = tempfile::tempdir().unwrap();
            let mut stdout = tempfile::tempfile().unwrap();
            let mut stderr = tempfile::tempfile().unwrap();
            let mut child = OwnedChild(
                Some(
                    Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "python_baseline_activation_corpus",
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
            let deadline = Instant::now() + Duration::from_secs(180);
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
            assert!(status.is_some(), "activation corpus exceeded 180 seconds");
            assert!(status.unwrap().success(), "activation corpus child failed");
            return;
        }
    };
    let output = run(Command::new("python3").args(["-c", "import sys; print(sys.executable)"]));
    let python_exe = PathBuf::from(String::from_utf8(output).unwrap().trim());
    assert!(python_exe.is_absolute());
    // This dedicated single-test process has no executable search path. Python
    // fixtures use the separately captured absolute interpreter, never a native
    // fallback. No providers, runtime programs or native CLI are invoked.
    let empty_path = root.join("empty-path");
    fs::create_dir(&empty_path).unwrap();
    unsafe {
        std::env::set_var("PATH", &empty_path);
    }
    let output = run(&mut python(&python_exe, &root));
    let mut counts = BTreeMap::<String, usize>::new();
    let mut summary = None;
    for line in output
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
    {
        let case: Value = serde_json::from_slice(line).unwrap();
        if text(&case, "kind") == "summary" {
            assert!(summary.replace(case).is_none());
            continue;
        }
        assert!(summary.is_none());
        assert_eq!(text(&case, "kind"), "case");
        let workspace = Path::new(text(&case, "path"));
        assert!(workspace.starts_with(&root));
        let label = text(&case, "label");
        let expected = text(&case, "expected");
        let operation = text(&case, "operation");
        let before = inventory(workspace);
        if case["wrapped"].as_bool().unwrap() {
            // These sources fit the native value/record profile themselves.
            // Only embedding them in the baseline/envelope exceeds a bound.
            let raw = fs::read(workspace.join("state/tasks.json")).unwrap();
            let value = coven::values::parse(&raw, coven::values::Profile::Authority).unwrap();
            coven::records::TaskRecords::validate(value).unwrap();
        }
        let guard = WorkspaceGuard::existing(workspace, Duration::ZERO).unwrap();
        let mut revision = None;
        let (added, archived) = if operation == "abort" {
            match ledger::abort_import(workspace, &guard) {
                Ok(report) => {
                    assert_eq!(expected, "ok", "{label}");
                    assert_eq!(report.durability(), ArchiveDurability::Confirmed, "{label}");
                    let relative = report.path().strip_prefix(workspace).unwrap();
                    assert_eq!(relative.parent(), Some(Path::new("state")));
                    let name = relative.file_name().unwrap().to_str().unwrap();
                    let id = name.strip_prefix("task-ledger-aborted-").unwrap();
                    assert_eq!(id.len(), 32);
                    assert!(
                        id.bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    );
                    assert_eq!(id.as_bytes()[12], b'4');
                    assert!(matches!(id.as_bytes()[16], b'8' | b'9' | b'a' | b'b'));
                    (BTreeSet::new(), Some(relative.to_owned()))
                }
                Err(error) => {
                    assert_eq!(expected, "refusal", "{label}: {error}");
                    (BTreeSet::new(), None)
                }
            }
        } else {
            let result = if operation == "resume" {
                ledger::resume_import(workspace, text(&case, "token"), &guard)
            } else {
                ledger::import_tasks(workspace, text(&case, "token"), &guard)
            };
            match result {
                Ok(report) => {
                    assert_eq!(expected, "ok", "{label}");
                    assert_eq!(
                        report.durability(),
                        ActivationDurability::Confirmed,
                        "{label}"
                    );
                    let borrowed = report.view().revision().to_owned();
                    let view = report.into_view();
                    assert_eq!(view.revision(), borrowed);
                    let selected = ledger::read(workspace).unwrap().unwrap();
                    assert_eq!(selected.revision(), borrowed);
                    assert_eq!(
                        view.tasks().canonical().unwrap(),
                        selected.tasks().canonical().unwrap()
                    );
                    assert_eq!(
                        view.projection_bytes().unwrap(),
                        selected.projection_bytes().unwrap()
                    );
                    revision = Some(borrowed);
                    (
                        if operation == "import" {
                            imported_paths(workspace)
                        } else {
                            BTreeSet::from([PathBuf::from(ledger::MARKER)])
                        },
                        None,
                    )
                }
                Err(error) => {
                    assert_ne!(expected, "ok", "{label}: {error:?}");
                    if expected == "prepared" {
                        assert_eq!(error.code(), "prepared_attempt_blocks_mutation", "{label}");
                    }
                    if expected == "profile" {
                        assert!(
                            error.code().starts_with("unsupported_"),
                            "{label}: {error:?}"
                        );
                    }
                    // Pending requires marker absence, not merely an observed
                    // active epoch directory. The preparation gate runs first.
                    // No injected I/O failures occur in this suite.
                    let progress = if before.contains_key(Path::new(ledger::DIRECTORY))
                        && !before.contains_key(Path::new(ledger::MARKER))
                        && expected != "prepared"
                    {
                        ImportProgress::Pending {
                            last_published: None,
                            synchronization_uncertain: false,
                        }
                    } else {
                        ImportProgress::NoNewArtifacts
                    };
                    assert_eq!(error.progress(), &progress, "{label}: {error:?}");
                    (BTreeSet::new(), None)
                }
            }
        };
        check_transition(
            &before,
            &inventory(workspace),
            added.clone(),
            archived.as_deref(),
        );
        guard.validate_for(workspace).unwrap();
        lock_state(&python_exe, workspace, "busy");
        drop(guard);
        lock_state(&python_exe, workspace, "clear");
        check_transition(&before, &inventory(workspace), added, archived.as_deref());
        if let Some(revision) = revision {
            let snapshot = inventory(workspace);
            let mut verify = python(&python_exe, workspace);
            verify.arg("--verify");
            if operation == "import" {
                verify.arg("--canonical");
            }
            assert_eq!(
                String::from_utf8(run(&mut verify)).unwrap().trim(),
                revision,
                "{label}"
            );
            assert_eq!(inventory(workspace), snapshot, "Python read wrote: {label}");
            if case["roundtrip"].as_bool().unwrap() {
                // Explicit Python mutation window after native outcome checks:
                // native import -> Python resume -> Python add/assign -> Rust read.
                assert_eq!(
                    String::from_utf8(run(python(&python_exe, workspace).arg("--python-resume")))
                        .unwrap()
                        .trim(),
                    revision
                );
                let output = run(python(&python_exe, workspace).arg("--followup"));
                let snapshot = inventory(workspace);
                assert_eq!(
                    ledger::read(workspace).unwrap().unwrap().revision(),
                    String::from_utf8(output).unwrap().trim()
                );
                assert_eq!(inventory(workspace), snapshot);
            }
        }
        *counts
            .entry(text(&case, "category").to_owned())
            .or_default() += 1;
    }
    let summary = summary.unwrap();
    let expected_counts: BTreeMap<String, usize> =
        serde_json::from_value(summary["counts"].clone()).unwrap();
    assert_eq!(counts, expected_counts);
    let total = counts.values().sum::<usize>();
    assert_eq!(summary["total"].as_u64().unwrap(), total as u64);
    assert_eq!(total, 133, "pin corpus coverage independently of generator");
    assert_eq!(
        counts,
        BTreeMap::from([
            ("abort:ok".to_owned(), 20),
            ("abort:refusal".to_owned(), 7),
            ("import:ok".to_owned(), 14),
            ("import:prepared".to_owned(), 4),
            ("import:profile".to_owned(), 4),
            ("import:refusal".to_owned(), 18),
            ("resume:ok".to_owned(), 17),
            ("resume:prepared".to_owned(), 4),
            ("resume:refusal".to_owned(), 45),
        ])
    );
    println!("baseline activation parity: {total} cases, {counts:?}");
    lock_pairs(&root, &python_exe, &summary);
}
