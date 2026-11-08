use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.path().join("source.json");
    std::fs::write(&path, b"{}").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    (dir, path)
}

#[test]
fn private_regular_descriptor_and_all_stamp_fields_are_fenced() {
    let (_dir, path) = fixture();
    let opened = open(path.to_str().unwrap()).unwrap();
    assert_eq!(opened.bytes().unwrap(), b"{}");
    use std::os::fd::AsRawFd;
    let fd = opened.read.as_ref().unwrap().file.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert_ne!(flags & libc::O_NONBLOCK, 0);
    assert_ne!(
        unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
        0
    );
    let before = stamp(std::fs::metadata(&path).unwrap()).unwrap();
    std::fs::write(&path, b"[]").unwrap();
    // Same inode and size, but mtime/ctime must still detect rewriting.
    assert_eq!(std::fs::metadata(&path).unwrap().ino(), before.inode);
    assert!(opened.recheck().is_err());
    let opened = open(path.to_str().unwrap()).unwrap();
    std::fs::rename(&path, path.with_extension("old")).unwrap();
    std::fs::write(&path, b"[]").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(opened.recheck().unwrap_err().code(), "artifact_changed");
}

#[test]
fn exact_modes_type_links_size_and_uid_are_policy_not_just_identity() {
    let (dir, path) = fixture();
    for mode in [0o400, 0o640, 0o660, 0o1600, 0o4600] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            open(path.to_str().unwrap())
                .unwrap()
                .bytes()
                .unwrap_err()
                .code(),
            "unsafe_artifact"
        );
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let opened = open(path.to_str().unwrap()).unwrap();
    let link = dir.path().join("hard");
    std::fs::hard_link(&path, &link).unwrap();
    assert_eq!(opened.recheck().unwrap_err().code(), "unsafe_artifact");
    assert_eq!(
        open(path.to_str().unwrap())
            .unwrap()
            .bytes()
            .unwrap_err()
            .code(),
        "unsafe_artifact"
    );
    std::fs::remove_file(link).unwrap();
    std::fs::write(&path, vec![b' '; 65_537]).unwrap();
    assert_eq!(
        open(path.to_str().unwrap())
            .unwrap()
            .bytes()
            .unwrap_err()
            .code(),
        "response_limit"
    );
    std::fs::remove_file(&path).unwrap();
    symlink("missing", &path).unwrap();
    assert_eq!(
        open(path.to_str().unwrap())
            .unwrap()
            .bytes()
            .unwrap_err()
            .code(),
        "unsafe_artifact"
    );
    std::fs::remove_file(&path).unwrap();
    let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    assert_eq!(
        open(path.to_str().unwrap())
            .unwrap()
            .bytes()
            .unwrap_err()
            .code(),
        "unsafe_artifact"
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(
        open(path.to_str().unwrap())
            .unwrap()
            .bytes()
            .unwrap_err()
            .code(),
        "unsafe_artifact"
    );
    for mode in [0o500, 0o750, 0o1700] {
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            open(path.to_str().unwrap()).unwrap_err().code(),
            "unsafe_directory"
        );
    }
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn aliases_are_allowed_only_above_nonsymlink_private_parent_and_rechecked() {
    let (dir, path) = fixture();
    let outer = tempfile::tempdir().unwrap();
    let alias = outer.path().join("alias");
    symlink(dir.path(), &alias).unwrap();
    assert_eq!(
        open(alias.join("source.json").to_str().unwrap())
            .unwrap_err()
            .code(),
        "unsafe_directory"
    );
    assert_eq!(
        open(alias.join(".").join("source.json").to_str().unwrap())
            .unwrap_err()
            .code(),
        "unsafe_directory"
    );
    let private = dir.path().join("private");
    std::fs::create_dir(&private).unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::rename(&path, private.join("source.json")).unwrap();
    let opened = open(alias.join("private/source.json").to_str().unwrap()).unwrap();
    assert_eq!(
        opened.resolved(),
        private.join("source.json").to_str().unwrap()
    );
    std::fs::remove_file(&alias).unwrap();
    symlink(outer.path(), &alias).unwrap();
    assert!(opened.recheck().is_err());
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o750)).unwrap();
    assert!(opened.recheck().is_err());
}

#[test]
fn stamp_to_open_races_recheck_policy_and_identity_before_reading() {
    for change in ["mode", "replace", "symlink", "parent"] {
        let (dir, path) = fixture();
        let result = open_with(path.to_str().unwrap(), || match change {
            "mode" => {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap()
            }
            "replace" => {
                std::fs::rename(&path, path.with_extension("old")).unwrap();
                std::fs::write(&path, b"[]").unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            "symlink" => {
                std::fs::remove_file(&path).unwrap();
                symlink("missing", &path).unwrap();
            }
            "parent" => {
                std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o750))
                    .unwrap()
            }
            _ => unreachable!(),
        })
        .unwrap();
        assert!(result.bytes().is_err(), "{change}");
        assert!(result.recheck().is_err(), "{change}");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}

#[test]
fn exact_byte_limit_and_parent_privacy_change_after_read_fail_closed() {
    let (dir, path) = fixture();
    std::fs::write(&path, vec![b' '; 65_536]).unwrap();
    let artifact = open(path.to_str().unwrap()).unwrap();
    assert_eq!(artifact.bytes().unwrap().len(), 65_536);
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o750)).unwrap();
    assert_eq!(artifact.recheck().unwrap_err().code(), "unsafe_directory");
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(artifact.recheck().is_err());
}

#[test]
fn ctime_only_change_is_detected_even_when_original_policy_is_restored() {
    let (_dir, path) = fixture();
    let artifact = open(path.to_str().unwrap()).unwrap();
    let before = stamp(std::fs::metadata(&path).unwrap()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let after = stamp(std::fs::metadata(&path).unwrap()).unwrap();
    assert_eq!(
        (before.device, before.inode, before.size, before.mtime),
        (after.device, after.inode, after.size, after.mtime)
    );
    assert_ne!(before.ctime, after.ctime);
    assert_eq!(artifact.recheck().unwrap_err().code(), "artifact_changed");
}
