use super::*;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn shared_admission_and_exclusive_exclusion_in_both_orders() {
    let root = tempfile::tempdir().unwrap();
    let shared = TransportGuard::shared(root.path(), Duration::ZERO).unwrap();
    let other = TransportGuard::shared(root.path(), Duration::ZERO).unwrap();
    assert!(TransportGuard::exclusive(root.path(), Duration::from_millis(20)).is_err());
    drop(shared);
    assert!(TransportGuard::exclusive(root.path(), Duration::ZERO).is_err());
    drop(other);
    let exclusive = TransportGuard::exclusive(root.path(), Duration::ZERO).unwrap();
    assert!(TransportGuard::shared(root.path(), Duration::ZERO).is_err());
    assert!(TransportGuard::exclusive(root.path(), Duration::ZERO).is_err());
    exclusive.validate_for(root.path(), true).unwrap();
    exclusive.validate_for(root.path(), false).unwrap();
    drop(exclusive);
    TransportGuard::shared(root.path(), Duration::ZERO).unwrap();
}

#[test]
fn owned_root_no_upgrade_and_persistent_lock() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let guard = TransportGuard::shared(root.path(), Duration::ZERO).unwrap();
    assert_eq!(guard.root(), root.path().canonicalize().unwrap());
    guard.validate_for(root.path(), false).unwrap();
    assert!(guard.validate_for(other.path(), false).is_err());
    assert!(guard.validate_for(root.path(), true).is_err());
    // Deliberately corrupt ownership to exercise the runtime validation too.
    let mut guard = guard;
    let owner = guard.owner;
    guard.owner = owner.wrapping_add(1);
    assert!(guard.validate_for(root.path(), false).is_err());
    guard.owner = owner;
    let thread = guard.thread;
    guard.thread = std::thread::spawn(|| std::thread::current().id())
        .join()
        .unwrap();
    assert!(guard.validate_for(root.path(), false).is_err());
    guard.thread = thread;
    let path = root.path().join("state/.runtime-transport.lock");
    std::fs::write(&path, b"persistent metadata").unwrap();
    drop(guard);
    assert_eq!(std::fs::read(&path).unwrap(), b"persistent metadata");
    let _next = TransportGuard::exclusive(root.path(), Duration::ZERO).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"persistent metadata");
}

fn python(root: &Path, exclusive: bool, hold: bool) -> std::process::Child {
    Command::new("python3")
        .args(["-B", "-c", r#"
import sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
import storage
root = Path(sys.argv[2])
try:
    with storage.transport_gate(root, root/'state/.runtime-transport.lock', exclusive=sys.argv[3]=='true', timeout=0):
        print('held', flush=True)
        if sys.argv[4]=='true': sys.stdin.readline()
except storage.LockBusy:
    sys.exit(23)
"#])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("runner"))
        .arg(root)
        .arg(exclusive.to_string())
        .arg(hold.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn().unwrap()
}

#[test]
fn python_flock_interoperability_in_both_directions() {
    let root = tempfile::tempdir().unwrap();
    let shared = TransportGuard::shared(root.path(), Duration::ZERO).unwrap();
    assert!(python(root.path(), false, false).wait().unwrap().success());
    assert_eq!(
        python(root.path(), true, false).wait().unwrap().code(),
        Some(23)
    );
    drop(shared);
    for exclusive in [false, true] {
        let mut child = python(root.path(), exclusive, true);
        let mut ready = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "held\n");
        assert!(TransportGuard::exclusive(root.path(), Duration::ZERO).is_err());
        assert_eq!(
            TransportGuard::shared(root.path(), Duration::ZERO).is_err(),
            exclusive
        );
        child.stdin.take().unwrap().write_all(b"release\n").unwrap();
        assert!(child.wait().unwrap().success());
    }
    let exclusive = TransportGuard::exclusive(root.path(), Duration::ZERO).unwrap();
    for mode in [false, true] {
        assert_eq!(
            python(root.path(), mode, false).wait().unwrap().code(),
            Some(23)
        );
    }
    drop(exclusive);
}
