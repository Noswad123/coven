use super::*;
use std::os::unix::{
    ffi::OsStringExt,
    fs::{PermissionsExt, symlink},
};

#[test]
fn aliases_retarget_and_non_unicode_paths_remain_lossless() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory
        .path()
        .join(OsString::from_vec(b"first\xff".to_vec()));
    let second = directory.path().join("second");
    let alias = directory.path().join("alias");
    executable(&first, b"first");
    executable(&second, b"second");
    symlink(&first, &alias).unwrap();
    let resolved = first.canonicalize().unwrap();
    assert!(alias_matches(&alias, &resolved));
    assert!(fingerprint(&first).is_ok());
    assert!(fingerprint(&alias).is_err());
    assert_eq!(
        find_in_path("alias", Some(directory.path().as_os_str())),
        Some(alias.clone())
    );
    fs::remove_file(&alias).unwrap();
    symlink(&second, &alias).unwrap();
    assert!(!alias_matches(&alias, &resolved));
    fs::remove_file(&alias).unwrap();
    assert!(!alias_matches(&alias, &resolved));
}

#[test]
fn nonregular_and_nonexecutable_paths_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plain");
    fs::write(&path, b"plain").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    for rejected in [&path, directory.path(), Path::new("/dev/null")] {
        assert!(metadata(rejected).is_err());
        assert!(fingerprint(rejected).is_err());
    }
    assert_eq!(
        find_in_path("plain", Some(directory.path().as_os_str())),
        None
    );
    let fifo = directory.path().join("fifo");
    let encoded = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    // Test-only special-file construction; production OS checks belong to platform.
    assert_eq!(unsafe { libc::mkfifo(encoded.as_ptr(), 0o700) }, 0);
    assert!(metadata(&fifo).is_err());
    assert!(fingerprint(&fifo).is_err());
    assert!(platform::descriptor_metadata(&platform::open_readonly(&fifo).unwrap()).is_err());
}

#[test]
fn negative_timestamp_is_preserved() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tool");
    executable(&path, b"abc");
    let modified = std::time::UNIX_EPOCH - Duration::from_nanos(1_234_567_890);
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(
        fingerprint(&path).unwrap().metadata.mtime_ns,
        -1_234_567_890
    );
}

#[test]
fn alias_inserted_between_checks_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tool");
    let target = directory.path().join("target");
    executable(&target, b"abc");
    for stage in [Checkpoint::BeforeOpen, Checkpoint::AfterOpen] {
        executable(&path, b"abc");
        assert!(
            fingerprint_checked(&path, 64, |checkpoint| {
                if checkpoint == stage {
                    fs::remove_file(&path)?;
                    symlink(&target, &path)?;
                }
                Ok(())
            })
            .is_err()
        );
        fs::remove_file(&path).unwrap();
    }
}
