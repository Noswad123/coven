use super::*;
use crate::execution::tests::{enabled, upgraded};
use std::cell::RefCell;

const ID: &str = "11111111111141118111111111111111";
#[derive(Default)]
struct TestIo {
    calls: RefCell<Vec<&'static str>>,
    fail: Option<&'static str>,
}
impl TestIo {
    fn step(&self, step: &'static str) -> io::Result<()> {
        self.calls.borrow_mut().push(step);
        if self.fail == Some(step) {
            Err(io::Error::other("injected"))
        } else {
            Ok(())
        }
    }
}
impl Io for TestIo {
    fn lock(&self, root: &Path) -> Result<WorkspaceGuard> {
        self.step("lock")?;
        WorkspaceGuard::execution_maintenance(root, Duration::ZERO)
    }
    fn uuid(&self) -> Result<String> {
        self.step("uuid")?;
        Ok(ID.to_owned())
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.step("rename")?;
        fs::rename(from, to)
    }
    fn sync_state(&self, state: &Path) -> io::Result<()> {
        self.step("sync")?;
        platform::sync_directory(state)
    }
}

fn pending(root: &Path) -> std::path::PathBuf {
    let path = root.join("state/execution");
    fs::create_dir_all(path.join("nested")).unwrap();
    fs::write(path.join("nested/orphan"), b"unparsed inventory\0").unwrap();
    path
}
fn target(root: &Path) -> std::path::PathBuf {
    root.join(format!("state/execution-pending-archive-{ID}"))
}

#[test]
fn confirm_pin_before_lock_archive_pin_after_lock() {
    let temp = tempfile::tempdir().unwrap();
    let malformed = ":".repeat(values::MAX_STRING + 1);
    for pin in [
        None,
        Some(DISABLED),
        Some("execution-control-v3:bad"),
        Some(""),
        Some("execution-control-v1:a:1:b"),
        Some(malformed.as_str()),
    ] {
        let io = TestIo::default();
        assert!(recover_with(temp.path(), RecoveryMode::ConfirmAuthority, pin, true, &io).is_err());
        assert!(io.calls.borrow().is_empty());
        assert!(!temp.path().join("state").exists());
    }
    let io = TestIo::default();
    assert_eq!(
        recover_with(
            temp.path(),
            RecoveryMode::ArchivePending,
            Some("anything"),
            true,
            &io
        )
        .err()
        .unwrap()
        .code(),
        "archive_does_not_accept_execution_revision"
    );
    assert_eq!(*io.calls.borrow(), ["lock"]);
    assert!(temp.path().join("state/.workspace.lock").is_file());
    let io = TestIo {
        fail: Some("lock"),
        ..TestIo::default()
    };
    assert_eq!(
        recover_with(
            temp.path(),
            RecoveryMode::ArchivePending,
            Some("anything"),
            true,
            &io
        )
        .err()
        .unwrap()
        .code(),
        "input_or_storage_unavailable"
    );
}

#[test]
fn confirm_v1_v2_exact_pin_without_marker_or_source_no_history_change() {
    for control in [enabled(), upgraded(enabled())] {
        let temp = tempfile::tempdir().unwrap();
        let path = pending(temp.path()).join("control.json");
        let bytes = control.storage().unwrap();
        fs::write(&path, &bytes).unwrap();
        fs::write(temp.path().join("state/tasks.json"), b"corrupt").unwrap();
        let revision = validated(&control).unwrap().replayed.revision;
        for json in [false, true] {
            let output = recover_arguments(
                temp.path(),
                RecoveryMode::ConfirmAuthority,
                Some(&revision),
                json,
            )
            .unwrap();
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.is_ascii());
            assert!(!output.stdout.ends_with('\n'));
            let value = values::parse(output.stdout.as_bytes(), Profile::Authority).unwrap();
            assert_eq!(text(get(&value, "outcome").unwrap()).unwrap(), "confirmed");
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        let wrong = format!(
            "execution-control-v1:{}:1:{}",
            "0".repeat(32),
            "0".repeat(64)
        );
        assert_eq!(
            recover_arguments(
                temp.path(),
                RecoveryMode::ConfirmAuthority,
                Some(&wrong),
                true
            )
            .err()
            .unwrap()
            .code(),
            "execution_revision_conflict"
        );
    }
}

#[test]
fn archive_absent_pending_is_error_not_noop_and_collisions_are_never_adopted() {
    let temp = tempfile::tempdir().unwrap();
    assert_eq!(
        recover_with(
            temp.path(),
            RecoveryMode::ArchivePending,
            None,
            true,
            &TestIo::default()
        )
        .err()
        .unwrap()
        .code(),
        "no_pending_execution_directory"
    );
    let path = pending(temp.path());
    for directory in [false, true] {
        let target = target(temp.path());
        if directory {
            fs::create_dir(&target).unwrap();
        } else {
            fs::write(&target, b"retain").unwrap();
        }
        let io = TestIo::default();
        assert_eq!(
            recover_with(temp.path(), RecoveryMode::ArchivePending, None, true, &io)
                .err()
                .unwrap()
                .code(),
            "execution_archive_target_exists"
        );
        assert_eq!(*io.calls.borrow(), ["lock", "uuid"]);
        assert!(path.join("nested/orphan").is_file());
        if directory {
            assert!(target.is_dir());
            fs::remove_dir(target).unwrap();
        } else {
            assert_eq!(fs::read(&target).unwrap(), b"retain");
            fs::remove_file(target).unwrap();
        }
    }
}

#[cfg(unix)]
#[test]
fn every_present_control_type_blocks_without_parsing() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};
    for kind in [
        "null",
        "malformed",
        "regular",
        "directory",
        "symlink",
        "fifo",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = pending(temp.path()).join("control.json");
        match kind {
            "null" => fs::write(&path, b"null").unwrap(),
            "malformed" => fs::write(&path, b"{").unwrap(),
            "regular" => fs::write(&path, enabled().storage().unwrap()).unwrap(),
            "directory" => fs::create_dir(&path).unwrap(),
            "symlink" => symlink("missing", &path).unwrap(),
            "fifo" => {
                let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
            }
            _ => unreachable!(),
        }
        let io = TestIo::default();
        assert!(
            recover_with(temp.path(), RecoveryMode::ArchivePending, None, true, &io).is_err(),
            "{kind}"
        );
        assert_eq!(*io.calls.borrow(), ["lock"]);
        assert!(fs::symlink_metadata(path).is_ok());
        assert!(!target(temp.path()).exists());
    }
}

#[cfg(unix)]
#[test]
fn whole_inventory_inode_mode_symlink_preserved_and_postrename_failure_retains_path() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
    for fail in [None, Some("rename"), Some("sync")] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("unicode-\u{1f980}");
        fs::create_dir(&root).unwrap();
        let pending = pending(&root);
        fs::set_permissions(&pending, fs::Permissions::from_mode(0o750)).unwrap();
        symlink("../missing", pending.join("nested/link")).unwrap();
        fs::write(root.join("coven.json"), b"corrupt unrelated marker").unwrap();
        fs::write(root.join("state/tasks.json"), b"corrupt unrelated source").unwrap();
        let before = fs::metadata(&pending).unwrap();
        let file_before = fs::metadata(pending.join("nested/orphan")).unwrap();
        let io = TestIo {
            fail,
            ..TestIo::default()
        };
        let result = recover_with(&root, RecoveryMode::ArchivePending, None, true, &io);
        if fail == Some("rename") {
            assert!(result.is_err());
            assert!(pending.exists());
            assert!(!target(&root).exists());
            continue;
        }
        let output = result.unwrap();
        assert!(!pending.exists());
        let target = target(&root);
        assert_eq!(fs::metadata(&target).unwrap().ino(), before.ino());
        assert_eq!(fs::metadata(&target).unwrap().mode(), before.mode());
        assert_eq!(
            fs::metadata(target.join("nested/orphan")).unwrap().ino(),
            file_before.ino()
        );
        assert_eq!(
            fs::read(target.join("nested/orphan")).unwrap(),
            b"unparsed inventory\0"
        );
        assert_eq!(
            fs::read_link(target.join("nested/link")).unwrap(),
            Path::new("../missing")
        );
        if fail == Some("sync") {
            assert_eq!(output.exit_code, 1);
            assert!(output.stdout.is_empty());
            assert!(output.warnings[0].is_ascii());
            assert!(output.warnings[0].contains("may already have been renamed"));
            assert!(output.warnings[0].contains(ID));
            assert!(output.warnings[0].contains("\\ud83e\\udd80"));
            assert!(output.warnings[0].contains("no rollback"));
        } else {
            assert_eq!(output.exit_code, 0);
            let value = values::parse(output.stdout.as_bytes(), Profile::Authority).unwrap();
            assert_eq!(
                Path::new(text(get(&value, "archive_path").unwrap()).unwrap()),
                target
            );
            assert_eq!(text(get(&value, "revision").unwrap()).unwrap(), DISABLED);
        }
    }
}

#[test]
fn large_unicode_path_presentation_is_bounded_before_effects() {
    let value = object(vec![(
        "archive_path",
        string("\u{ffff}".repeat(values::MAX_OUTPUT / 6 + 1)),
    )])
    .unwrap();
    for json in [true, false] {
        assert_eq!(
            render(&value, json).err().unwrap().code(),
            "unsupported_json_string_profile"
        );
    }
    let path = format!("/{}", "\u{1f980}".repeat(4096));
    let value = object(vec![("archive_path", string(&path))]).unwrap();
    for json in [true, false] {
        let output = render(&value, json).unwrap();
        assert!(output.stdout.is_ascii());
        assert!(!output.stdout.ends_with('\n'));
        let decoded = values::parse(output.stdout.as_bytes(), Profile::Authority).unwrap();
        assert_eq!(text(get(&decoded, "archive_path").unwrap()).unwrap(), path);
    }
}
