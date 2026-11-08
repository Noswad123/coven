use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("state")).unwrap();
    root
}

fn code(result: crate::Result<PublisherLease>) -> &'static str {
    match result {
        Ok(_) => panic!("unexpected acquisition"),
        Err(error) => error.code(),
    }
}

#[test]
fn persistent_private_lock_is_noninheritable_and_released_by_close() {
    assert!(subprocess("persistent", None).success());
}

#[test]
fn missing_or_symlinked_state_is_never_created_or_followed() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        code(PublisherLease::acquire(root.path())),
        "publisher_lease_error"
    );
    assert!(!root.path().join("state").exists());
    let target = tempfile::tempdir().unwrap();
    symlink(target.path(), root.path().join("state")).unwrap();
    assert_eq!(
        code(PublisherLease::acquire(root.path())),
        "publisher_lease_error"
    );
    assert!(!target.path().join(".group-publisher.lock").exists());
    let alias = root.path().join("alias");
    symlink(target.path(), &alias).unwrap();
    assert_eq!(
        code(PublisherLease::acquire(&alias)),
        "publisher_lease_error"
    );
}

#[test]
fn unsafe_existing_lock_is_not_modified() {
    for kind in ["symlink", "directory", "fifo", "public", "hardlink"] {
        let root = workspace();
        let path = root.path().join("state/.group-publisher.lock");
        let target = root.path().join("target");
        std::fs::write(&target, b"unchanged").unwrap();
        match kind {
            "symlink" => symlink(&target, &path).unwrap(),
            "directory" => std::fs::create_dir(&path).unwrap(),
            "fifo" => {
                let name = CString::new(path.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            }
            "public" => {
                std::fs::write(&path, b"unchanged").unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            }
            "hardlink" => {
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
                std::fs::hard_link(&target, &path).unwrap();
            }
            _ => unreachable!(),
        }
        let before = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(
            code(PublisherLease::acquire(root.path())),
            "publisher_lease_error",
            "{kind}"
        );
        let after = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(
            (after.dev(), after.ino(), after.mode()),
            (before.dev(), before.ino(), before.mode())
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"unchanged");
    }
}

fn subprocess(case: &str, root: Option<&Path>) -> std::process::ExitStatus {
    let test = format!(
        "{}::subprocess_case",
        module_path!().split_once("::").unwrap().1
    );
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &test, "--nocapture", "--test-threads=1"])
        .env("COVEN_SOCKET_LEASE_CASE", case);
    if let Some(root) = root {
        command.env("COVEN_SOCKET_LEASE_ROOT", root);
    }
    command.status().unwrap()
}

#[test]
fn contention_is_immediate_across_processes() {
    assert!(subprocess("contention", None).success());
}

#[test]
fn forked_child_drop_does_not_unlock_parent() {
    assert!(subprocess("fork", None).success());
}

#[test]
fn subprocess_case() {
    let Ok(case) = std::env::var("COVEN_SOCKET_LEASE_CASE") else {
        return;
    };
    // Last-close assertions need a process without unrelated tests forking and
    // transiently retaining the lease's open file description before exec/exit.
    match case.as_str() {
        "persistent" => {
            let root = workspace();
            let path = root.path().join("state/.group-publisher.lock");
            let lease = crate::observation_socket::PublisherLease::acquire(root.path()).unwrap();
            let before = std::fs::symlink_metadata(&path).unwrap();
            assert_eq!(before.mode() & 0o777, 0o600);
            for file in [&lease._root, &lease._state, &lease._lock] {
                assert_ne!(
                    unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
                    0
                );
            }
            assert_eq!(code(PublisherLease::acquire(root.path())), "publisher_busy");
            std::fs::write(&path, b"preserve contents").unwrap();
            drop(lease);
            let lease = PublisherLease::acquire(root.path()).unwrap();
            assert_eq!(lease._lock.metadata().unwrap().ino(), before.ino());
            assert_eq!(std::fs::read(&path).unwrap(), b"preserve contents");
            drop(lease);
            assert_eq!(std::fs::symlink_metadata(path).unwrap().ino(), before.ino());
        }
        "contention" => {
            let root = workspace();
            let lease = PublisherLease::acquire(root.path()).unwrap();
            assert!(subprocess("busy", Some(root.path())).success());
            drop(lease);
            assert!(subprocess("acquire", Some(root.path())).success());
        }
        "busy" => {
            let root = std::env::var_os("COVEN_SOCKET_LEASE_ROOT").unwrap();
            assert_eq!(
                code(PublisherLease::acquire(Path::new(&root))),
                "publisher_busy"
            );
        }
        "acquire" => {
            let root = std::env::var_os("COVEN_SOCKET_LEASE_ROOT").unwrap();
            PublisherLease::acquire(Path::new(&root)).unwrap();
        }
        "fork" => {
            let root = workspace();
            let lease = PublisherLease::acquire(root.path()).unwrap();
            let pid = unsafe { libc::fork() };
            assert!(pid >= 0);
            if pid == 0 {
                drop(lease);
                unsafe { libc::_exit(0) };
            }
            let mut status = 0;
            assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
            assert_eq!(status, 0);
            assert_eq!(code(PublisherLease::acquire(root.path())), "publisher_busy");
            drop(lease);
            PublisherLease::acquire(root.path()).unwrap();
        }
        _ => panic!("unknown lease scenario"),
    }
}

#[test]
fn lease_is_neither_send_nor_sync() {
    trait AmbiguousSend<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousSend<()> for T {}
    impl<T: ?Sized + Send> AmbiguousSend<u8> for T {}
    let _ = <PublisherLease as AmbiguousSend<_>>::marker;
    trait AmbiguousSync<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousSync<()> for T {}
    impl<T: ?Sized + Sync> AmbiguousSync<u8> for T {}
    let _ = <PublisherLease as AmbiguousSync<_>>::marker;
}
