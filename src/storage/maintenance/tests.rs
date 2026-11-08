use super::*;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt, symlink},
        net::UnixListener,
    },
    process::{Command, Stdio},
};

fn maintenance(root: &Path) -> Result<WorkspaceGuard> {
    WorkspaceGuard::execution_maintenance(root, Duration::ZERO)
}

#[test]
fn absent_or_unsafe_markers_are_ignored_without_weakening_public_admission() {
    for kind in ["absent", "directory", "symlink", "dangling", "fifo"] {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("marker");
        fs::write(&target, b"untouched").unwrap();
        let marker = root.path().join("coven.json");
        match kind {
            "directory" => fs::create_dir(&marker).unwrap(),
            "symlink" => symlink(&target, &marker).unwrap(),
            "dangling" => symlink(outside.path().join("missing"), &marker).unwrap(),
            "fifo" => fifo(&marker),
            _ => {}
        }
        assert!(WorkspaceGuard::acquire(root.path(), Duration::ZERO).is_err());
        assert!(WorkspaceGuard::existing(root.path(), Duration::ZERO).is_err());
        assert!(!root.path().join("state").exists());
        let guard = maintenance(root.path()).unwrap();
        guard.validate_for(root.path()).unwrap();
        drop(guard);
        assert!(WorkspaceGuard::acquire(root.path(), Duration::ZERO).is_err());
        assert!(WorkspaceGuard::existing(root.path(), Duration::ZERO).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"untouched");
    }
}

#[test]
fn corrupt_regular_marker_keeps_existing_public_type_only_admission() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("coven.json");
    fs::write(&marker, b"not JSON\xff").unwrap();
    drop(maintenance(root.path()).unwrap());
    drop(WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap());
    drop(WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap());
    assert_eq!(fs::read(marker).unwrap(), b"not JSON\xff");
}

#[test]
fn only_an_existing_directory_root_is_accepted_and_aliases_are_canonical() {
    let parent = tempfile::tempdir().unwrap();
    let missing = parent.path().join("missing");
    assert!(maintenance(&missing).is_err());
    assert!(!missing.exists());
    let file = parent.path().join("file");
    fs::write(&file, b"unchanged").unwrap();
    assert!(maintenance(&file).is_err());
    let root = parent.path().join("root");
    fs::create_dir(&root).unwrap();
    let alias = parent.path().join("alias");
    symlink(&root, &alias).unwrap();
    let guard = maintenance(&alias).unwrap();
    assert_eq!(guard.root(), root.canonicalize().unwrap());
    guard.validate_for(&alias).unwrap();
    guard.validate_for(&root).unwrap();
}

#[test]
fn unrelated_unsafe_sources_and_execution_files_are_not_inspected() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("state")).unwrap();
    for source in ["tasks", "checkpoints", "runtime", "state/execution"] {
        symlink("/dev/null", root.path().join(source)).unwrap();
    }
    fifo(&root.path().join("roster.json"));
    drop(maintenance(root.path()).unwrap());
    for source in ["tasks", "checkpoints", "runtime", "state/execution"] {
        assert_eq!(
            fs::read_link(root.path().join(source)).unwrap(),
            Path::new("/dev/null")
        );
    }
}

#[test]
fn state_and_lock_bad_types_are_rejected_without_following_or_blocking() {
    for relative in ["state", "state/.workspace.lock"] {
        for kind in [
            "symlink",
            "dangling",
            "device_symlink",
            "fifo",
            "socket",
            "wrong_type",
        ] {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            if relative.contains('/') {
                fs::create_dir(root.path().join("state")).unwrap();
            }
            let path = root.path().join(relative);
            let _socket = match kind {
                "symlink" => {
                    symlink(outside.path(), &path).unwrap();
                    None
                }
                "dangling" => {
                    symlink(outside.path().join("missing"), &path).unwrap();
                    None
                }
                "device_symlink" => {
                    symlink("/dev/null", &path).unwrap();
                    None
                }
                "fifo" => {
                    fifo(&path);
                    None
                }
                "socket" => Some(UnixListener::bind(&path).unwrap()),
                _ if relative == "state" => {
                    fs::write(&path, b"untouched").unwrap();
                    None
                }
                _ => {
                    fs::create_dir(&path).unwrap();
                    None
                }
            };
            assert!(maintenance(root.path()).is_err(), "{relative}: {kind}");
            assert!(fs::symlink_metadata(&path).is_ok());
            assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        }
    }
}

fn fifo(path: &Path) {
    let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
}

#[test]
fn persistent_inode_bytes_modes_and_public_lock_exclusion_are_shared() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("coven.json"), b"marker").unwrap();
    drop(maintenance(root.path()).unwrap());
    let state = root.path().join("state");
    let lock = state.join(".workspace.lock");
    fs::write(&lock, b"stale PID is not authority\n").unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o750)).unwrap();
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o640)).unwrap();
    let before = fs::metadata(&lock).unwrap();
    let state_before = fs::metadata(&state).unwrap();
    let guard = maintenance(root.path()).unwrap();
    for result in [
        maintenance(root.path()),
        WorkspaceGuard::acquire(root.path(), Duration::ZERO),
        WorkspaceGuard::existing(root.path(), Duration::ZERO),
    ] {
        assert_eq!(result.err().unwrap().code(), "workspace_lock_busy");
    }
    assert_eq!(
        WorkspaceGuard::execution_maintenance(root.path(), Duration::from_millis(10))
            .err()
            .unwrap()
            .code(),
        "workspace_lock_busy"
    );
    drop(guard);
    let public = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
    assert_eq!(
        maintenance(root.path()).err().unwrap().code(),
        "workspace_lock_busy"
    );
    drop(public);
    drop(maintenance(root.path()).unwrap());
    let after = fs::metadata(&lock).unwrap();
    assert_eq!(
        (before.dev(), before.ino(), before.mode()),
        (after.dev(), after.ino(), after.mode())
    );
    let state_after = fs::metadata(&state).unwrap();
    assert_eq!(
        (state_before.ino(), state_before.mode()),
        (state_after.ino(), state_after.mode())
    );
    assert_eq!(fs::read(lock).unwrap(), b"stale PID is not authority\n");
}

#[test]
fn creation_modes_match_public_acquisition_under_the_current_umask() {
    let public = tempfile::tempdir().unwrap();
    let source_free = tempfile::tempdir().unwrap();
    fs::write(public.path().join("coven.json"), b"marker").unwrap();
    drop(WorkspaceGuard::acquire(public.path(), Duration::ZERO).unwrap());
    drop(maintenance(source_free.path()).unwrap());
    for relative in ["state", "state/.workspace.lock"] {
        assert_eq!(
            fs::metadata(public.path().join(relative)).unwrap().mode(),
            fs::metadata(source_free.path().join(relative))
                .unwrap()
                .mode()
        );
    }
    assert_eq!(fs::read_dir(source_free.path()).unwrap().count(), 1);
    assert_eq!(
        fs::read_dir(source_free.path().join("state"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn maintenance_guard_is_statically_neither_send_nor_sync() {
    // Type inference becomes ambiguous if the guard ever implements the trait.
    trait AmbiguousIfSend<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    struct SendMarker;
    impl<T: ?Sized + Send> AmbiguousIfSend<SendMarker> for T {}
    let _ = <WorkspaceGuard as AmbiguousIfSend<_>>::check;
    trait AmbiguousIfSync<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfSync<()> for T {}
    struct SyncMarker;
    impl<T: ?Sized + Sync> AmbiguousIfSync<SyncMarker> for T {}
    let _ = <WorkspaceGuard as AmbiguousIfSync<_>>::check;
}

#[test]
fn maintenance_tokens_keep_root_pid_and_thread_validation() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let mut guard = maintenance(root.path()).unwrap();
    assert_eq!(
        guard.validate_for(other.path()).unwrap_err().code(),
        "invalid_workspace_lock_owner"
    );
    let owner = guard.owner;
    guard.owner = owner.wrapping_add(1);
    assert_eq!(
        guard.validate_for(root.path()).unwrap_err().code(),
        "invalid_workspace_lock_owner"
    );
    guard.owner = owner;
    let owner_thread = guard.thread;
    guard.thread = std::thread::spawn(|| std::thread::current().id())
        .join()
        .unwrap();
    assert_eq!(
        guard.validate_for(root.path()).unwrap_err().code(),
        "invalid_workspace_lock_owner"
    );
    guard.thread = owner_thread;
    guard.validate_for(root.path()).unwrap();
}

#[test]
fn python_and_maintenance_exclude_each_other_and_process_death_releases() {
    let root = tempfile::tempdir().unwrap();
    let guard = maintenance(root.path()).unwrap();
    let inode = fs::metadata(root.path().join("state/.workspace.lock"))
        .unwrap()
        .ino();
    let mut child = Command::new("python3")
        .args([
            "-c",
            r#"
import sys
from pathlib import Path
from runner import storage
root = Path(sys.argv[1])
lock = root / 'state/.workspace.lock'
try:
    with storage.workspace_lock(root, lock, timeout=0):
        raise AssertionError('acquired native-owned lock')
except storage.LockBusy:
    print('BUSY', flush=True)
assert sys.stdin.readline() == 'acquire\n'
with storage.workspace_lock(root, lock, timeout=0):
    print('HELD', flush=True)
    sys.stdin.readline()
"#,
        ])
        .arg(root.path())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    assert_eq!(line, "BUSY\n");
    drop(guard);
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"acquire\n")
        .unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line, "HELD\n");
    let result = maintenance(root.path());
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(result.err().unwrap().code(), "workspace_lock_busy");
    drop(maintenance(root.path()).unwrap());
    assert_eq!(
        fs::metadata(root.path().join("state/.workspace.lock"))
            .unwrap()
            .ino(),
        inode
    );
}
