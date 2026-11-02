use super::*;
use std::{fs, os::unix::fs::symlink};

#[test]
fn init_target_resolves_aliases_missing_tails_and_parent_in_filesystem_order() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("actual/child")).unwrap();
    symlink("actual/child", root.join("alias")).unwrap();
    symlink("missing/child", root.join("dangling")).unwrap();
    for (argument, expected) in [
        ("alias", "actual/child"),
        ("alias/../new", "actual/new"),
        ("absent/../alias/../new", "actual/new"),
        ("missing/a/../../alias", "actual/child"),
        ("dangling/../tail", "missing/tail"),
        ("actual/./child/../../new/deep", "new/deep"),
    ] {
        assert_eq!(
            resolve_init_target(&root.join(argument)).unwrap(),
            root.join(expected),
            "{argument}"
        );
    }
    symlink(root.join("actual"), root.join("absolute")).unwrap();
    assert_eq!(
        resolve_init_target(&root.join("absolute/new")).unwrap(),
        root.join("actual/new")
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 4);
    assert!(resolve_init_target(Path::new("~somebody/workspace")).is_err());
    assert!(resolve_init_target(Path::new("")).unwrap().is_absolute());
}

#[test]
fn init_target_refuses_loops_nonunicode_and_wrong_ancestor_types() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    symlink("loop-b", root.join("loop-a")).unwrap();
    symlink("loop-a", root.join("loop-b")).unwrap();
    assert!(resolve_init_target(&root.join("loop-a/tail")).is_err());
    fs::write(root.join("file"), b"unchanged").unwrap();
    for path in ["file", "file/..", "file/./tail", "missing/../file/tail"] {
        assert!(resolve_init_target(&root.join(path)).is_err(), "{path}");
    }
    assert!(resolve_init_target(&root.join(std::ffi::OsString::from_vec(vec![0xff]))).is_err());
    assert_eq!(fs::read(root.join("file")).unwrap(), b"unchanged");
}

#[test]
fn init_lock_is_exclusive_nonblocking_private_and_refuses_all_existing_types() {
    use std::os::fd::AsRawFd;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let lock = root.join("lock");
    let file = open_init_lock(&lock).unwrap();
    assert_eq!(file.metadata().unwrap().mode() & !0o600 & 0o7777, 0);
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    assert_ne!(flags & libc::O_NONBLOCK, 0);
    assert_ne!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
        0
    );
    assert!(open_init_lock(&lock).is_err());
    assert!(super::super::super::try_workspace_lock(&file).unwrap());
    let competing = super::super::super::open_workspace_lock(&lock, false).unwrap();
    assert!(!super::super::super::try_workspace_lock(&competing).unwrap());
    for (name, target) in [
        ("symlink", lock.clone()),
        ("device", PathBuf::from("/dev/null")),
        ("dangling", root.join("absent")),
    ] {
        symlink(target, root.join(name)).unwrap();
        assert!(open_init_lock(&root.join(name)).is_err());
    }
    fs::hard_link(&lock, root.join("hardlink")).unwrap();
    assert!(open_init_lock(&root.join("hardlink")).is_err());
    assert!(validate_init_identity(&lock, &file, false).is_err());
    assert!(open_init_lock(Path::new("/dev/null")).is_err());
    let fifo = root.join("fifo");
    let c_path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    assert!(open_init_lock(&fifo).is_err());
    assert!(open_init_lock(root).is_err());
}
