use super::{Delivery, WorkspaceGuard};
use crate::{Result, error::require, platform, read_fs};
use std::{ffi::OsString, fs::File, io::Write, path::Path};

// Transport ceiling only, independent of immutable publication's 8 MiB limit.
const MAX_REPLACEMENT_BYTES: usize = crate::values::MAX_OUTPUT;

trait ReplacementIo {
    fn target_mode(&self, target: &Path) -> std::io::Result<Option<u32>> {
        platform::replacement_mode(target)
    }
    fn temporary(
        &self,
        parent: &Path,
        name: &std::ffi::OsStr,
    ) -> std::io::Result<tempfile::NamedTempFile> {
        let mut prefix = OsString::from(".");
        prefix.push(name);
        prefix.push(".");
        tempfile::Builder::new()
            .prefix(&prefix)
            .suffix(".tmp")
            .tempfile_in(parent)
    }
    fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
        file.write_all(bytes)
    }
    fn apply_mode(&self, file: &File, mode: u32) -> std::io::Result<()> {
        platform::apply_replacement_mode(file, mode)
    }
    fn verify_mode(&self, file: &File, mode: u32) -> std::io::Result<()> {
        platform::verify_replacement_mode(file, mode)
    }
    fn sync_file(&self, file: &File) -> std::io::Result<()> {
        file.sync_all()
    }
    fn recheck(&self, guard: &WorkspaceGuard, relative: &Path) -> Result<()> {
        safe_target(guard, relative).map(|_| ())
    }
    fn rename(&self, source: &Path, target: &Path) -> std::io::Result<()> {
        platform::rename_replace(source, target)
    }
    fn sync_dir(&self, parent: &Path) -> std::io::Result<()> {
        platform::sync_directory(parent)
    }
    fn cleanup(&self, temp: tempfile::NamedTempFile) -> std::io::Result<()> {
        temp.close()
    }
}

struct SystemReplacementIo;
impl ReplacementIo for SystemReplacementIo {}

fn safe_target(guard: &WorkspaceGuard, relative: &Path) -> Result<std::path::PathBuf> {
    let target = read_fs::managed(guard.root(), relative, false)?;
    let parent = target
        .parent()
        .ok_or(crate::Error("invalid_replacement_parent"))?;
    require(
        std::fs::symlink_metadata(parent)?.is_dir(),
        "unsafe_replacement_parent",
    )?;
    Ok(target)
}

/// Atomically replace a mutable file under an owned workspace coordination capability.
/// This is NOT CAS, approval, proof of receipt absence, execution permission, or
/// whole-ledger confirmation. Future domain writers must still enforce their
/// domain/control/projection limits and native reader profiles; this low-level
/// byte transport allows 32 MiB.
/// Never use for immutable frames, manifests, baselines, INITIAL task markers,
/// or action bundles. Immutable publication retains its separate 8 MiB limit.
///
/// Requires an existing safe parent and rejects coordination lock basenames in
/// every directory. Preserves only mode bits (07777), not owner, ACLs or xattrs;
/// a newly created target gets 0600. No orphan adoption, copy/truncate fallback,
/// or rollback is performed. Pre-rename errors leave the target unchanged;
/// after rename, directory-sync failure returns `DurabilityUncertain` with the
/// new bytes visible. No post-rename reread is required or used.
///
/// Assumes static paths on a cooperating local filesystem. This is not a sandbox
/// against hostile path races or a distributed-filesystem unknown-rename protocol.
pub fn atomic_replace_bytes(
    guard: &WorkspaceGuard,
    relative: &Path,
    bytes: &[u8],
) -> Result<Delivery> {
    atomic_replace_with(guard, relative, bytes, &SystemReplacementIo)
}

fn atomic_replace_with(
    guard: &WorkspaceGuard,
    relative: &Path,
    bytes: &[u8],
    io: &impl ReplacementIo,
) -> Result<Delivery> {
    guard.validate_for(guard.root())?;
    require(
        bytes.len() <= MAX_REPLACEMENT_BYTES,
        "unsupported_replacement_size",
    )?;
    let name = relative
        .file_name()
        .ok_or(crate::Error("unsafe_replacement_path"))?;
    require(
        name != ".workspace.lock"
            && name != ".runtime-transport.lock"
            && name != ".group-publisher.lock",
        "unsafe_replacement_lock",
    )?;
    let target = safe_target(guard, relative)?;
    let parent = target
        .parent()
        .ok_or(crate::Error("invalid_replacement_parent"))?;
    let mode = io.target_mode(&target)?.unwrap_or(0o600);
    let mut temp = io.temporary(parent, name)?;
    let result = (|| {
        io.write(temp.as_file_mut(), bytes)?;
        // Writes can clear special bits; restore them on the descriptor afterward.
        io.apply_mode(temp.as_file(), mode)?;
        // A successful chmod can silently drop SGID; reject before publication.
        io.verify_mode(temp.as_file(), mode)?;
        io.sync_file(temp.as_file())?;
        io.recheck(guard, relative)?;
        io.rename(temp.path(), &target)?;
        // Rename is the visibility boundary. Cleanup cannot undo this result.
        Ok(if io.sync_dir(parent).is_ok() {
            Delivery::Committed
        } else {
            Delivery::DurabilityUncertain
        })
    })();
    let _ = io.cleanup(temp);
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        cell::RefCell,
        fs,
        os::unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, PermissionsExt, symlink},
        },
        path::PathBuf,
        time::Duration,
    };

    fn workspace() -> (tempfile::TempDir, WorkspaceGuard) {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("coven.json"), b"marker").unwrap();
        let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap();
        (root, guard)
    }

    struct FaultIo {
        fail: &'static str,
        cleanup_fails: bool,
        events: RefCell<Vec<&'static str>>,
        temp: RefCell<Option<PathBuf>>,
    }
    impl FaultIo {
        fn new(fail: &'static str, cleanup_fails: bool) -> Self {
            Self {
                fail,
                cleanup_fails,
                events: RefCell::new(Vec::new()),
                temp: RefCell::new(None),
            }
        }
        fn step(&self, name: &'static str) -> std::io::Result<()> {
            self.events.borrow_mut().push(name);
            if self.fail == name {
                Err(std::io::Error::other(name))
            } else {
                Ok(())
            }
        }
    }
    impl ReplacementIo for FaultIo {
        fn target_mode(&self, target: &Path) -> std::io::Result<Option<u32>> {
            self.step("target_mode")?;
            SystemReplacementIo.target_mode(target)
        }
        fn temporary(
            &self,
            parent: &Path,
            name: &std::ffi::OsStr,
        ) -> std::io::Result<tempfile::NamedTempFile> {
            self.step("temporary")?;
            let temp = SystemReplacementIo.temporary(parent, name)?;
            *self.temp.borrow_mut() = Some(temp.path().to_owned());
            Ok(temp)
        }
        fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
            // Even a partial write must leave the old target wholly intact.
            file.write_all(&bytes[..bytes.len() / 2])?;
            self.step("write")?;
            file.write_all(&bytes[bytes.len() / 2..])
        }
        fn apply_mode(&self, file: &File, mode: u32) -> std::io::Result<()> {
            self.step("mode")?;
            SystemReplacementIo.apply_mode(file, mode)
        }
        fn verify_mode(&self, file: &File, mode: u32) -> std::io::Result<()> {
            // Model descriptor metadata I/O failure, independently of chmod.
            self.step("verify_mode")
                .map_err(|_| std::io::Error::from_raw_os_error(libc::EIO))?;
            SystemReplacementIo.verify_mode(file, mode)
        }
        fn sync_file(&self, file: &File) -> std::io::Result<()> {
            self.step("file_sync")?;
            SystemReplacementIo.sync_file(file)
        }
        fn recheck(&self, guard: &WorkspaceGuard, relative: &Path) -> Result<()> {
            self.step("recheck")
                .map_err(|_| crate::Error("injected_recheck"))?;
            SystemReplacementIo.recheck(guard, relative)
        }
        fn rename(&self, source: &Path, target: &Path) -> std::io::Result<()> {
            self.step("rename")?;
            SystemReplacementIo.rename(source, target)
        }
        fn sync_dir(&self, parent: &Path) -> std::io::Result<()> {
            self.step("parent_sync")?;
            SystemReplacementIo.sync_dir(parent)
        }
        fn cleanup(&self, temp: tempfile::NamedTempFile) -> std::io::Result<()> {
            self.events.borrow_mut().push("cleanup");
            if self.cleanup_fails {
                // Simulate an inert orphan, without NamedTempFile's drop retry.
                let (_, path) = temp.keep()?;
                assert_eq!(Some(path), *self.temp.borrow());
                Err(std::io::Error::other("cleanup"))
            } else {
                SystemReplacementIo.cleanup(temp)
            }
        }
    }

    #[test]
    fn fault_boundaries_preserve_full_bytes_modes_inodes_and_known_results() {
        let steps = [
            "target_mode",
            "temporary",
            "write",
            "mode",
            "verify_mode",
            "file_sync",
            "recheck",
            "rename",
            "parent_sync",
            "success",
        ];
        let old = vec![b'o'; 8193];
        let new = vec![b'n'; 16387];
        for existing in [false, true] {
            for mode in [0o600, 0o640, 0o7755] {
                for fail in steps {
                    for cleanup_fails in [false, true] {
                        let (root, guard) = workspace();
                        let target = root.path().join("state/snapshot");
                        if existing {
                            fs::write(&target, &old).unwrap();
                            fs::set_permissions(&target, fs::Permissions::from_mode(mode)).unwrap();
                        }
                        let before = fs::metadata(&target).ok();
                        let lock = root.path().join("state/.workspace.lock");
                        let lock_before = fs::metadata(&lock).unwrap();
                        let orphan = root.path().join("state/.snapshot.abandoned.tmp");
                        fs::write(&orphan, b"not authority").unwrap();
                        let orphan_inode = fs::metadata(&orphan).unwrap().ino();
                        let io = FaultIo::new(fail, cleanup_fails);
                        let result =
                            atomic_replace_with(&guard, Path::new("state/snapshot"), &new, &io);
                        let renamed = matches!(fail, "parent_sync" | "success");
                        if renamed {
                            assert_eq!(
                                result.unwrap(),
                                if fail == "parent_sync" {
                                    Delivery::DurabilityUncertain
                                } else {
                                    Delivery::Committed
                                }
                            );
                            assert_eq!(fs::read(&target).unwrap(), new);
                            let after = fs::metadata(&target).unwrap();
                            assert_eq!(after.mode() & 0o7777, if existing { mode } else { 0o600 });
                            if let Some(before) = &before {
                                assert_ne!(after.ino(), before.ino());
                            }
                        } else {
                            assert_eq!(
                                result.unwrap_err().code(),
                                if fail == "recheck" {
                                    "injected_recheck"
                                } else {
                                    "input_or_storage_unavailable"
                                }
                            );
                            if let Some(before) = before {
                                let after = fs::metadata(&target).unwrap();
                                assert_eq!(fs::read(&target).unwrap(), old);
                                assert_eq!(
                                    (after.ino(), after.mode()),
                                    (before.ino(), before.mode())
                                );
                            } else {
                                assert!(!target.exists());
                            }
                        }
                        let events = io.events.borrow();
                        let through = steps.iter().position(|step| *step == fail).unwrap();
                        let mut expected = steps[..=through.min(8)].to_vec();
                        if !matches!(fail, "target_mode" | "temporary") {
                            expected.push("cleanup");
                        }
                        assert_eq!(*events, expected);
                        if let Some(temp) = io.temp.borrow().as_ref() {
                            assert_eq!(temp.parent(), target.parent());
                            let name = temp.file_name().unwrap().as_bytes();
                            assert!(name.starts_with(b".snapshot.") && name.ends_with(b".tmp"));
                            assert_ne!(temp, &orphan);
                            assert_eq!(temp.exists(), cleanup_fails && !renamed);
                            if temp.exists() {
                                assert!(fs::read(temp).unwrap() != b"not authority");
                            }
                        }
                        assert_eq!(fs::read(&orphan).unwrap(), b"not authority");
                        assert_eq!(fs::metadata(&orphan).unwrap().ino(), orphan_inode);
                        let lock_after = fs::metadata(&lock).unwrap();
                        assert_eq!(
                            (lock_after.dev(), lock_after.ino()),
                            (lock_before.dev(), lock_before.ino())
                        );
                        assert!(WorkspaceGuard::existing(root.path(), Duration::ZERO).is_err());
                    }
                }
            }
        }
    }

    #[test]
    fn successful_chmod_that_drops_sgid_is_rejected_by_real_descriptor_verification() {
        struct ClearSgidIo;
        impl ReplacementIo for ClearSgidIo {
            fn apply_mode(&self, file: &File, mode: u32) -> std::io::Result<()> {
                assert_ne!(mode & 0o2000, 0);
                // Simulate Linux reporting success while silently clearing SGID.
                platform::apply_replacement_mode(file, mode & !0o2000)?;
                assert_eq!(file.metadata()?.mode() & 0o2000, 0);
                Ok(())
            }
            fn sync_file(&self, _: &File) -> std::io::Result<()> {
                panic!("mode mismatch must fail before file sync/recheck/rename");
            }
        }

        let (root, guard) = workspace();
        let target = root.path().join("state/snapshot");
        fs::write(&target, b"whole old snapshot").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o2750)).unwrap();
        let before = fs::metadata(&target).unwrap();
        assert_eq!(before.mode() & 0o7777, 0o2750);
        let error = atomic_replace_with(
            &guard,
            Path::new("state/snapshot"),
            b"whole new snapshot",
            &ClearSgidIo,
        )
        .unwrap_err();
        assert_eq!(error.code(), "input_or_storage_unavailable");
        assert_eq!(fs::read(&target).unwrap(), b"whole old snapshot");
        let after = fs::metadata(&target).unwrap();
        assert_eq!(
            (after.dev(), after.ino(), after.mode()),
            (before.dev(), before.ino(), before.mode())
        );
        assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 2);
    }

    #[test]
    fn rejects_unsafe_paths_and_all_lock_basenames_without_creating_parents() {
        let (root, guard) = workspace();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("file"), b"outside").unwrap();
        symlink(outside.path(), root.path().join("state/link")).unwrap();
        symlink(
            outside.path().join("file"),
            root.path().join("state/symlink"),
        )
        .unwrap();
        fs::create_dir(root.path().join("state/directory")).unwrap();
        fs::write(root.path().join("state/regular"), b"regular").unwrap();
        let fifo =
            std::ffi::CString::new(root.path().join("state/fifo").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let lock = root.path().join("state/.workspace.lock");
        let inode = fs::metadata(&lock).unwrap().ino();
        for relative in [
            "",
            ".",
            "../escape",
            "state/../escape",
            "state/symlink",
            "state/link/file",
            "state/fifo",
            "state/directory",
            "missing/file",
            "state/regular/file",
            ".workspace.lock",
            ".runtime-transport.lock",
            ".group-publisher.lock",
            "state/.workspace.lock",
            "state/.runtime-transport.lock",
            "state/.group-publisher.lock",
            "state/directory/.workspace.lock",
            "state/directory/.runtime-transport.lock",
            "state/directory/.group-publisher.lock",
        ] {
            let io = FaultIo::new("success", false);
            assert!(
                atomic_replace_with(&guard, Path::new(relative), b"bad", &io).is_err(),
                "{relative}"
            );
            assert!(io.events.borrow().is_empty());
        }
        assert!(atomic_replace_bytes(&guard, &outside.path().join("file"), b"bad").is_err());
        assert_eq!(fs::read(outside.path().join("file")).unwrap(), b"outside");
        assert!(!root.path().join("missing").exists());
        assert_eq!(fs::metadata(lock).unwrap().ino(), inode);
    }

    #[test]
    fn recheck_requires_target_and_parent_to_still_be_safe() {
        let (root, guard) = workspace();
        let parent = root.path().join("state/parent");
        let target = parent.join("snapshot");
        let relative = Path::new("state/parent/snapshot");
        fs::create_dir(&parent).unwrap();
        SystemReplacementIo.recheck(&guard, relative).unwrap();
        fs::write(&target, b"old").unwrap();
        SystemReplacementIo.recheck(&guard, relative).unwrap();
        fs::remove_file(&target).unwrap();
        symlink(root.path().join("coven.json"), &target).unwrap();
        assert!(SystemReplacementIo.recheck(&guard, relative).is_err());
        fs::remove_file(&target).unwrap();
        fs::remove_dir(&parent).unwrap();
        assert!(SystemReplacementIo.recheck(&guard, relative).is_err());
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), &parent).unwrap();
        assert!(SystemReplacementIo.recheck(&guard, relative).is_err());
        assert!(!outside.path().join("snapshot").exists());
    }

    #[test]
    fn validates_guard_before_io_for_wrong_pid_thread_and_root() {
        let (root, mut guard) = workspace();
        let io = FaultIo::new("success", false);
        let owner = guard.owner;
        guard.owner = owner.wrapping_add(1);
        assert!(atomic_replace_with(&guard, Path::new("state/file"), b"x", &io).is_err());
        guard.owner = owner;
        let thread = guard.thread;
        guard.thread = std::thread::spawn(|| std::thread::current().id())
            .join()
            .unwrap();
        assert!(atomic_replace_with(&guard, Path::new("state/file"), b"x", &io).is_err());
        guard.thread = thread;
        let other = tempfile::tempdir().unwrap();
        assert!(guard.validate_for(other.path()).is_err());
        let original = guard.root.clone();
        guard.root = root.path().join("nonexistent");
        assert!(atomic_replace_with(&guard, Path::new("state/file"), b"x", &io).is_err());
        guard.root = original;
        assert!(io.events.borrow().is_empty());
    }

    #[test]
    fn accepts_exact_transport_cap_rejects_over_and_never_promotes_orphans() {
        let (root, guard) = workspace();
        let target = root.path().join("state/bytes");
        let bytes = vec![42; MAX_REPLACEMENT_BYTES];
        assert_eq!(
            atomic_replace_bytes(&guard, Path::new("state/bytes"), &bytes).unwrap(),
            Delivery::Committed
        );
        assert_eq!(fs::read(&target).unwrap(), bytes);
        let before = fs::metadata(&target).unwrap();
        let io = FaultIo::new("success", false);
        let over = vec![43; MAX_REPLACEMENT_BYTES + 1];
        assert!(atomic_replace_with(&guard, Path::new("state/bytes"), &over, &io).is_err());
        assert!(io.events.borrow().is_empty());
        assert_eq!(fs::metadata(&target).unwrap().ino(), before.ino());
        assert_eq!(fs::read(&target).unwrap(), bytes);
        fs::write(root.path().join("state/.absent.orphan.tmp"), b"orphan").unwrap();
        assert!(
            atomic_replace_with(
                &guard,
                Path::new("state/absent"),
                b"new",
                &FaultIo::new("temporary", false)
            )
            .is_err()
        );
        assert!(!root.path().join("state/absent").exists());
        atomic_replace_bytes(&guard, Path::new("state/absent"), b"new").unwrap();
        assert_eq!(fs::read(root.path().join("state/absent")).unwrap(), b"new");
        assert_eq!(
            fs::read(root.path().join("state/.absent.orphan.tmp")).unwrap(),
            b"orphan"
        );
    }
}
