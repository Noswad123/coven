use super::*;
use std::os::unix::fs::MetadataExt;
#[path = "../task_mutation/tests/contender.rs"]
mod contender;

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

fn independent(root: &Path, exclusive: bool, hold: bool) -> contender::Process {
    contender::Process::spawn(
        concat!(module_path!(), "::independent_transport_child"),
        root,
        match (exclusive, hold) {
            (false, false) => "shared",
            (true, false) => "exclusive",
            (false, true) => "shared-hold",
            (true, true) => "exclusive-hold",
        },
    )
}

#[test]
#[ignore = "IPC child entry; exercised by python_flock_interoperability_in_both_directions"]
fn independent_transport_child() {
    let root = std::env::var_os("COVEN_SOURCE_CONTENDER_ROOT").unwrap();
    let mode = std::env::var("COVEN_SOURCE_CONTENDER_MODE").unwrap();
    let file = contender::open(&Path::new(&root).join("state/.runtime-transport.lock"));
    if !contender::lock(&file, mode.starts_with("exclusive"), true) {
        std::process::exit(23);
    }
    contender::signal(&format!("held:{}", file.metadata().unwrap().ino()));
    if mode.ends_with("hold") {
        contender::receive();
    }
    std::process::exit(0);
}

#[test]
fn python_flock_interoperability_in_both_directions() {
    // Historical name only: Python-specific qualification is retained evidence,
    // not live coverage. These six launches qualify the independent OS protocol.
    let root = tempfile::tempdir().unwrap();
    let shared = TransportGuard::shared(root.path(), Duration::ZERO).unwrap();
    let inode = std::fs::metadata(root.path().join("state/.runtime-transport.lock"))
        .unwrap()
        .ino();
    let mut child = independent(root.path(), false, false);
    assert_eq!(child.line(), format!("held:{inode}"));
    assert!(child.wait().success());
    assert_eq!(
        independent(root.path(), true, false).wait().code(),
        Some(23)
    );
    drop(shared);
    for exclusive in [false, true] {
        let mut child = independent(root.path(), exclusive, true);
        assert_eq!(child.line(), format!("held:{inode}"));
        assert!(TransportGuard::exclusive(root.path(), Duration::ZERO).is_err());
        assert_eq!(
            TransportGuard::shared(root.path(), Duration::ZERO).is_err(),
            exclusive
        );
        child.send(b"release\n");
        assert!(child.wait().success());
    }
    let exclusive = TransportGuard::exclusive(root.path(), Duration::ZERO).unwrap();
    for mode in [false, true] {
        assert_eq!(
            independent(root.path(), mode, false).wait().code(),
            Some(23)
        );
    }
    drop(exclusive);
    drop(TransportGuard::exclusive(root.path(), Duration::ZERO).unwrap());
    assert_eq!(
        std::fs::metadata(root.path().join("state/.runtime-transport.lock"))
            .unwrap()
            .ino(),
        inode
    );
}
