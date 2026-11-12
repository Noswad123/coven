use super::*;

#[test]
fn shared_private_access_support_and_basename_policy() {
    use crate::private_access;
    let temp = private_access::test_support::tempdir();
    let parent = File::open(temp.path()).unwrap();
    assert_eq!(private_access::supports_privacy(), supports_apply());
    for leaf in [c"", c".", c"..", c"a/b", c"/socket"] {
        assert_eq!(
            private_access::validate_socket_entry(&parent, leaf, 0, 0, 0, 0)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // Mode admission is a consumer responsibility, not a new Linux ACL claim.
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let metadata = parent.metadata().unwrap();
        assert_eq!(metadata.mode() & 0o7777, 0o700);
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        private_access::validate_fd_access(&parent).unwrap();
        validate_access(&parent).unwrap();
        // A nonexistent basename and nonsense snapshots still add no OS policy.
        private_access::validate_socket_entry(&parent, c"not-created.sock", 0, 0, 0, 0).unwrap();
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        assert_eq!(
            private_access::validate_fd_access(&parent)
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(
            private_access::validate_socket_entry(&parent, c"socket", 0, 0, 0, 0)
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
    }
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn cross_directory_move_semantics_and_errors() {
    use std::{fs, os::fd::AsRawFd};
    let temp = tempfile::tempdir().unwrap();
    let left = temp.path().join("left");
    let right = temp.path().join("right");
    fs::create_dir(&left).unwrap();
    fs::create_dir(&right).unwrap();
    let a = File::open(&left).unwrap();
    let b = File::open(&right).unwrap();
    fs::write(left.join("source"), b"new").unwrap();
    fs::write(right.join("target"), b"old").unwrap();
    let move_to = |kind| move_entry(a.as_raw_fd(), c"source", b.as_raw_fd(), c"target", kind);
    assert!(supports_apply());
    assert_eq!(
        move_to(MoveKind::NoReplace).unwrap_err().raw_os_error(),
        Some(libc::EEXIST)
    );
    assert_eq!(fs::read(left.join("source")).unwrap(), b"new");
    assert_eq!(fs::read(right.join("target")).unwrap(), b"old");
    move_to(MoveKind::Exchange).unwrap();
    assert_eq!(fs::read(left.join("source")).unwrap(), b"old");
    assert_eq!(fs::read(right.join("target")).unwrap(), b"new");
    fs::remove_file(right.join("target")).unwrap();
    assert_eq!(
        move_to(MoveKind::Exchange).unwrap_err().raw_os_error(),
        Some(libc::ENOENT)
    );
    assert_eq!(fs::read(left.join("source")).unwrap(), b"old");
    move_to(MoveKind::NoReplace).unwrap();
    assert!(!left.join("source").exists());
    assert_eq!(fs::read(right.join("target")).unwrap(), b"old");
    // A delete is a no-replace move to the other directory's retained stash.
    move_entry(
        b.as_raw_fd(),
        c"target",
        a.as_raw_fd(),
        c"stash",
        MoveKind::NoReplace,
    )
    .unwrap();
    assert!(!right.join("target").exists());
    assert_eq!(fs::read(left.join("stash")).unwrap(), b"old");
    assert_eq!(
        move_entry(-1, c"stash", b.as_raw_fd(), c"target", MoveKind::NoReplace)
            .unwrap_err()
            .raw_os_error(),
        Some(libc::EBADF)
    );
    validate_access(&a).unwrap();
    let file = File::open(left.join("stash")).unwrap();
    validate_access(&file).unwrap();
    sync_file(&file).unwrap();
    sync_directory(&a).unwrap();
    sync_directory(&b).unwrap();
    // Pipes provide a real kernel sync failure, without invalid owned FDs.
    let (pipe, _) = std::os::unix::net::UnixStream::pair().unwrap();
    let file = File::from(std::os::fd::OwnedFd::from(pipe));
    // Linux preserves its existing mode policy; Darwin requires an inspectable
    // local APFS object, not an arbitrary socket descriptor.
    #[cfg(target_os = "linux")]
    validate_access(&file).unwrap();
    #[cfg(target_os = "macos")]
    assert!(validate_access(&file).is_err());
    assert!(sync_file(&file).is_err());
    assert!(sync_directory(&file).is_err());
}

#[test]
#[cfg(target_os = "linux")]
fn macos_acl_adapter_mock_logic() {
    use std::process::Command;
    // No CC override, shell expansion, native ACL calls, or user-controlled output path.
    let temp = tempfile::tempdir().unwrap();
    let owned = temp.path().canonicalize().unwrap();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/hook_files/platform")
        .canonicalize()
        .unwrap();
    let binary = owned.join("acl-mock");
    let compile = Command::new("/usr/bin/cc")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("TMPDIR", &owned)
        .env("LC_ALL", "C")
        .args([
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-pedantic",
            "-DCOVEN_HOOK_ACL_TEST",
        ])
        .arg(source.join("macos_acl.c"))
        .arg(source.join("macos_acl_mock.c"))
        .arg("-o")
        .arg(&binary)
        .current_dir(&owned)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&binary)
        .env_clear()
        .current_dir(&owned)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("33 scenarios passed"));
    assert!(stdout.contains("46 scenarios + 31 parent guard failures passed"));
    // This adapter inspects metadata only; it cannot authenticate or contact peers.
    let adapter = include_str!("macos_acl.c");
    for forbidden in ["connect(", "accept(", "socket(", "F_GETPATH", "/proc/", "acl_set_"] {
        assert!(!adapter.contains(forbidden), "unexpected operation: {forbidden}");
    }
}
