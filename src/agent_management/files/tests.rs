#![cfg(any(target_os = "linux", target_os = "macos"))]
use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

fn workspace() -> tempfile::TempDir {
    let root = crate::private_access::test_support::tempdir();
    fs::write(root.path().join("coven.json"), b"marker").unwrap();
    root
}
struct Hook;
impl Hook {
    fn set(hook: impl Fn(&str, &File) -> std::io::Result<()> + 'static) -> Self {
        CHECKPOINT.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
        Self
    }
}
impl Drop for Hook {
    fn drop(&mut self) {
        CHECKPOINT.with(|slot| *slot.borrow_mut() = None);
    }
}
fn deny(role: &'static str) -> Hook {
    Hook::set(move |at, _| {
        if at == role {
            Err(std::io::ErrorKind::PermissionDenied.into())
        } else {
            Ok(())
        }
    })
}
const NONCE: &str = "0123456789abcdef0123456789abcdef";
fn temporary(root: &Path) -> PathBuf {
    root.join(format!("state/.agent-management-{NONCE}.tmp"))
}

#[test]
fn physical_directory_contract_and_replacement_fences() {
    let root = workspace();
    let directory = Directory::open(root.path()).unwrap();
    assert_eq!(directory.path(), root.path());
    assert_eq!(
        directory.identity(),
        identity(&directory.file().metadata().unwrap())
    );
    assert!(directory.stat(c"missing").unwrap().is_none());
    for bad in [c"", c".", c"..", c"a/b"] {
        assert!(directory.stat(bad).is_err());
    }
    assert!(Directory::open(Path::new("relative")).is_err());
    assert!(Directory::open(&root.path().join("..")).is_err());
    let alias = root.path().join("alias");
    symlink(root.path(), &alias).unwrap();
    assert!(Directory::open(&alias).is_err());
    assert!(Files::open(&alias).is_err());
    assert!(Files::open(&alias.join("..")).is_err());
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o777)).unwrap();
    assert!(directory.fence().is_err());
    assert!(Directory::open(root.path()).is_err());
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let child = root.path().join("child");
    fs::create_dir(&child).unwrap();
    let held = Directory::open(&child).unwrap();
    fs::rename(&child, root.path().join("old-child")).unwrap();
    fs::create_dir(&child).unwrap();
    assert!(held.fence().is_err());
}

#[test]
fn marker_never_followed_and_state_only_created_after_admission() {
    let root = workspace();
    let _hook = deny("root");
    assert!(Files::open(root.path()).is_err());
    assert!(!root.path().join("state").exists());
    drop(_hook);
    fs::remove_file(root.path().join("coven.json")).unwrap();
    symlink("absent", root.path().join("coven.json")).unwrap();
    assert!(Files::open(root.path()).is_err());
    assert!(!root.path().join("state").exists());
}

#[test]
fn relative_workspace_is_lexical_and_descriptor_flags_are_private() {
    let cwd = std::env::current_dir().unwrap();
    let root = tempfile::tempdir_in(&cwd).unwrap();
    fs::write(root.path().join("coven.json"), b"marker").unwrap();
    let relative = root.path().strip_prefix(&cwd).unwrap().join(".");
    let files = Files::open(&relative).unwrap();
    assert_eq!(files.root(), root.path());
    for file in [
        files.root.file(),
        files.state.file(),
        &files.lock.file,
        &files.lease.file,
    ] {
        assert_ne!(
            unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
    }
    let guard = files.guard(Duration::ZERO).unwrap();
    assert!(guard.validate(&files).is_ok());
    let other_root = workspace();
    let other = Files::open(other_root.path()).unwrap();
    assert_eq!(
        guard.validate(&other).unwrap_err().code(),
        "invalid_workspace_lock_owner"
    );
}

#[test]
fn inherited_files_and_invalid_guard_owner_cannot_unlock_parent_description() {
    let root = workspace();
    let mut files = Files::open(root.path()).unwrap();
    files.pid = files.pid.wrapping_add(1);
    assert_eq!(
        files.guard(Duration::ZERO).err().unwrap().code(),
        "invalid_workspace_lock_owner"
    );
    files.pid = std::process::id();
    let mut guard = files.guard(Duration::ZERO).unwrap();
    guard.pid = guard.pid.wrapping_add(1);
    assert_eq!(
        guard.validate(&files).unwrap_err().code(),
        "invalid_workspace_lock_owner"
    );
    // Dropping a foreign-PID token must not issue LOCK_UN on an inherited OFD.
    drop(guard);
    let other = File::open(root.path().join("state/.workspace.lock")).unwrap();
    assert!(flock(&other, Duration::ZERO, "busy").is_err());
    drop(files);
    flock(&other, Duration::ZERO, "busy").unwrap();
}

#[test]
fn admission_boundary_mode_changes_refuse_before_read_or_write() {
    let root = workspace();
    let files = Files::open(root.path()).unwrap();
    let guard = files.guard(Duration::ZERO).unwrap();
    let image = files.load(&guard).unwrap();
    let hook = Hook::set(|role, file| {
        if role == "pre_temp_write" {
            assert_eq!(file.metadata()?.len(), 0);
            file.set_permissions(fs::Permissions::from_mode(0o644))?;
        }
        Ok(())
    });
    assert!(files.replace(&guard, &image, NONCE, b"secret").is_err());
    drop(hook);
    assert!(!root.path().join("state/agent-management.json").exists());
    assert!(!temporary(root.path()).exists());
    files.replace(&guard, &image, NONCE, b"old secret").unwrap();
    let hook = Hook::set(|role, file| {
        if role == "existing_store" {
            assert_eq!(
                unsafe { libc::lseek(file.as_raw_fd(), 0, libc::SEEK_CUR) },
                0
            );
            file.set_permissions(fs::Permissions::from_mode(0o644))?;
        }
        Ok(())
    });
    assert!(files.load(&guard).is_err());
    drop(hook);
}

#[test]
fn read_same_fd_metadata_and_name_are_rechecked() {
    for replace_entry in [false, true] {
        let root = workspace();
        let files = Files::open(root.path()).unwrap();
        let guard = files.guard(Duration::ZERO).unwrap();
        let image = files.load(&guard).unwrap();
        files.replace(&guard, &image, NONCE, b"old secret").unwrap();
        let path = root.path().join("state/agent-management.json");
        let calls = Cell::new(0);
        let hook = Hook::set(move |role, file| {
            if role == "existing_store" {
                calls.set(calls.get() + 1);
                if calls.get() == 2 {
                    assert_eq!(
                        unsafe { libc::lseek(file.as_raw_fd(), 0, libc::SEEK_CUR) },
                        10
                    );
                    if replace_entry {
                        fs::rename(&path, path.with_extension("old"))?;
                    } else {
                        fs::write(&path, b"changed")?;
                    }
                }
            }
            Ok(())
        });
        assert_eq!(
            files.load(&guard).err().unwrap().code(),
            "agent_store_replaced"
        );
        drop(hook);
    }
}

#[test]
fn state_admitted_before_any_lock_and_old_modes_are_not_repaired() {
    let root = workspace();
    fs::create_dir(root.path().join("state")).unwrap();
    fs::set_permissions(root.path().join("state"), fs::Permissions::from_mode(0o755)).unwrap();
    let hook = deny("state");
    assert!(Files::open(root.path()).is_err());
    assert!(!root.path().join("state/.workspace.lock").exists());
    assert!(
        !root
            .path()
            .join("state/.agent-management.receiver.lock")
            .exists()
    );
    drop(hook);
    let lock = root.path().join("state/.workspace.lock");
    fs::write(&lock, b"keep").unwrap();
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o644)).unwrap();
    let files = Files::open(root.path()).unwrap();
    assert_eq!(
        fs::metadata(root.path().join("state")).unwrap().mode() & 0o7777,
        0o755
    );
    assert_eq!(fs::metadata(&lock).unwrap().mode() & 0o7777, 0o644);
    assert_eq!(fs::read(&lock).unwrap(), b"keep");
    drop(files);
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(Files::open(root.path()).is_err());
    assert_eq!(fs::metadata(&lock).unwrap().mode() & 0o7777, 0o666);
}

#[test]
fn admission_precedes_flock_and_lease_is_persistent() {
    let root = workspace();
    for role in ["workspace_lock", "receiver_lease"] {
        let hook = deny(role);
        assert!(Files::open(root.path()).is_err());
        drop(hook);
        let lock = File::open(root.path().join("state/.workspace.lock")).unwrap();
        flock(&lock, Duration::ZERO, "busy").unwrap();
        unsafe {
            libc::flock(lock.as_raw_fd(), libc::LOCK_UN);
        }
    }
    let files = Files::open(root.path()).unwrap();
    let inode = files.lease.inode;
    assert_eq!(
        Files::open(root.path()).err().unwrap().code(),
        "agent_receiver_busy"
    );
    let guard = files.guard(Duration::ZERO).unwrap();
    assert_eq!(
        files.guard(Duration::ZERO).err().unwrap().code(),
        "workspace_lock_busy"
    );
    let other = File::open(root.path().join("state/.workspace.lock")).unwrap();
    assert!(flock(&other, Duration::ZERO, "busy").is_err());
    drop(guard);
    flock(&other, Duration::ZERO, "busy").unwrap();
    let started = Instant::now();
    assert_eq!(
        files.guard(Duration::from_millis(10)).err().unwrap().code(),
        "workspace_lock_busy"
    );
    assert!(started.elapsed() >= Duration::from_millis(10));
    drop(other);
    assert!(files.guard(Duration::ZERO).is_ok());
    drop(files);
    let files = Files::open(root.path()).unwrap();
    assert_eq!(files.lease.inode, inode);
    fs::rename(
        root.path().join("state/.agent-management.receiver.lock"),
        root.path().join("old-lease"),
    )
    .unwrap();
    fs::write(
        root.path().join("state/.agent-management.receiver.lock"),
        b"foreign",
    )
    .unwrap();
    assert_eq!(
        files.fence().unwrap_err().code(),
        "agent_receiver_lease_replaced"
    );
    drop(files);
    assert_eq!(
        fs::read(root.path().join("state/.agent-management.receiver.lock")).unwrap(),
        b"foreign"
    );
}

#[test]
fn bounded_private_read_rejects_before_read_and_detects_changed_entry() {
    let root = workspace();
    let files = Files::open(root.path()).unwrap();
    let guard = files.guard(Duration::ZERO).unwrap();
    assert!(files.load(&guard).unwrap().bytes.is_none());
    let path = root.path().join("state/agent-management.json");
    fs::write(&path, b"secret").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let hook = Hook::set(|role, file| {
        if role == "existing_store" {
            assert_eq!(
                unsafe { libc::lseek(file.as_raw_fd(), 0, libc::SEEK_CUR) },
                0
            );
            return Err(std::io::ErrorKind::PermissionDenied.into());
        }
        Ok(())
    });
    assert!(files.load(&guard).is_err());
    drop(hook);
    assert_eq!(files.load(&guard).unwrap().bytes.unwrap(), b"secret");
    fs::write(&path, vec![b'x'; LIMIT + 1]).unwrap();
    assert_eq!(
        files.load(&guard).err().unwrap().code(),
        "agent_store_limit"
    );
    fs::write(&path, b"secret").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(files.load(&guard).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::hard_link(&path, root.path().join("extra-link")).unwrap();
    assert!(files.load(&guard).is_err());
    fs::remove_file(root.path().join("extra-link")).unwrap();
    let image = files.load(&guard).unwrap();
    fs::rename(&path, root.path().join("old-store")).unwrap();
    fs::write(&path, b"foreign").unwrap();
    assert!(files.replace(&guard, &image, NONCE, b"new secret").is_err());
    assert_eq!(fs::read(&path).unwrap(), b"foreign");
    assert!(!temporary(root.path()).exists());
}

#[test]
fn all_prepublication_denials_preserve_old_target_and_clean_only_our_temp() {
    for role in [
        "root",
        "state",
        "workspace_lock",
        "receiver_lease",
        "existing_store",
        "pre_temp_write",
        "after_write",
        "prepublication",
    ] {
        let root = workspace();
        let files = Files::open(root.path()).unwrap();
        let guard = files.guard(Duration::ZERO).unwrap();
        let missing = files.load(&guard).unwrap();
        files.replace(&guard, &missing, NONCE, b"old").unwrap();
        let image = files.load(&guard).unwrap();
        let hook = Hook::set(move |at, file| {
            if at == role {
                if role == "pre_temp_write" {
                    assert_eq!(file.metadata()?.len(), 0);
                }
                if role == "after_write" {
                    assert_eq!(file.metadata()?.len(), 10);
                }
                return Err(std::io::ErrorKind::PermissionDenied.into());
            }
            Ok(())
        });
        assert!(
            files.replace(&guard, &image, NONCE, b"new secret").is_err(),
            "{role}"
        );
        drop(hook);
        assert_eq!(
            fs::read(root.path().join("state/agent-management.json")).unwrap(),
            b"old",
            "{role}"
        );
        assert!(!temporary(root.path()).exists(), "{role}");
    }
}

#[test]
fn foreign_temporary_is_never_removed_and_failed_admission_stays_empty() {
    let root = workspace();
    let files = Files::open(root.path()).unwrap();
    let guard = files.guard(Duration::ZERO).unwrap();
    let image = files.load(&guard).unwrap();
    let temp = temporary(root.path());
    let saved_temp = root.path().join("retained-temp");
    let (temp_clone, saved_clone) = (temp.clone(), saved_temp.clone());
    let hook = Hook::set(move |role, file| {
        if role == "pre_temp_write" {
            assert_eq!(file.metadata()?.len(), 0);
            fs::rename(&temp_clone, &saved_clone)?;
            fs::write(&temp_clone, b"foreign")?;
            return Err(std::io::ErrorKind::PermissionDenied.into());
        }
        Ok(())
    });
    assert!(files.replace(&guard, &image, NONCE, b"secret").is_err());
    drop(hook);
    assert_eq!(fs::read(&temp).unwrap(), b"foreign");
    assert!(fs::read(&saved_temp).unwrap().is_empty());
    assert!(files.replace(&guard, &image, NONCE, b"secret").is_err());
    assert_eq!(fs::read(&temp).unwrap(), b"foreign");
    for nonce in [
        "../secret",
        "session-id",
        "",
        "gggggggggggggggggggggggggggggggg",
    ] {
        assert!(files.replace(&guard, &image, nonce, b"secret").is_err());
    }
}

#[test]
fn prepublication_replacement_and_mode_changes_are_fenced() {
    for change_mode in [false, true] {
        let root = workspace();
        let files = Files::open(root.path()).unwrap();
        let guard = files.guard(Duration::ZERO).unwrap();
        let image = files.load(&guard).unwrap();
        let path = root.path().to_owned();
        let hook = Hook::set(move |role, _| {
            if role == "prepublication" {
                if change_mode {
                    fs::set_permissions(temporary(&path), fs::Permissions::from_mode(0o644))?;
                } else {
                    fs::rename(path.join("state"), path.join("old-state"))?;
                    fs::create_dir(path.join("state"))?;
                }
            }
            Ok(())
        });
        assert!(files.replace(&guard, &image, NONCE, b"secret").is_err());
        drop(hook);
        assert!(!root.path().join("state/agent-management.json").exists());
        assert!(!root.path().join("old-state/agent-management.json").exists());
    }
}

#[test]
fn publication_exact_mode_and_directory_sync_uncertainty_never_rollback() {
    let root = workspace();
    let files = Files::open(root.path()).unwrap();
    files.sync_startup().unwrap();
    let guard = files.guard(Duration::ZERO).unwrap();
    let image = files.load(&guard).unwrap();
    let hook = deny("postrename_sync");
    assert_eq!(
        files
            .replace(&guard, &image, NONCE, b"committed secret")
            .unwrap(),
        Delivery::DurabilityUncertain
    );
    drop(hook);
    let path = root.path().join("state/agent-management.json");
    assert_eq!(fs::read(&path).unwrap(), b"committed secret");
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o7777, 0o600);
    assert!(!temporary(root.path()).exists());
    let image = files.load(&guard).unwrap();
    assert_eq!(
        files.replace(&guard, &image, NONCE, b"next").unwrap(),
        Delivery::Committed
    );
}

#[test]
fn failed_cleanup_can_leave_one_owned_temp_without_changing_old_target() {
    // Root bypasses the directory write-permission failure used by this test.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let root = workspace();
    let files = Files::open(root.path()).unwrap();
    let guard = files.guard(Duration::ZERO).unwrap();
    let image = files.load(&guard).unwrap();
    files.replace(&guard, &image, NONCE, b"old").unwrap();
    let image = files.load(&guard).unwrap();
    let state = root.path().join("state");
    let state_clone = state.clone();
    let hook = Hook::set(move |role, _| {
        if role == "after_write" {
            fs::set_permissions(&state_clone, fs::Permissions::from_mode(0o500))?;
            return Err(std::io::ErrorKind::PermissionDenied.into());
        }
        Ok(())
    });
    let result = files.replace(&guard, &image, NONCE, b"new secret");
    drop(hook);
    // Restore permissions before assertions so fixture teardown can clean up.
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err());
    assert_eq!(
        fs::read(state.join("agent-management.json")).unwrap(),
        b"old"
    );
    let temp = temporary(root.path());
    assert_eq!(fs::read(&temp).unwrap(), b"new secret");
    let metadata = fs::metadata(&temp).unwrap();
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    assert_eq!(metadata.nlink(), 1);
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    assert_ne!(
        metadata.ino(),
        fs::metadata(state.join("agent-management.json"))
            .unwrap()
            .ino()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn real_temporary_acl_is_denied_while_still_empty() {
    let root = workspace();
    let files = Files::open(root.path()).unwrap();
    let guard = files.guard(Duration::ZERO).unwrap();
    let image = files.load(&guard).unwrap();
    let path = temporary(root.path());
    let hook = Hook::set(move |role, file| {
        if role == "pre_temp_write" {
            assert_eq!(file.metadata()?.len(), 0);
            assert!(
                std::process::Command::new("/bin/chmod")
                    .arg("+a")
                    .arg("everyone allow read")
                    .arg(&path)
                    .status()?
                    .success()
            );
        }
        Ok(())
    });
    assert!(files.replace(&guard, &image, NONCE, b"secret").is_err());
    drop(hook);
    assert!(!temporary(root.path()).exists());
    assert!(!root.path().join("state/agent-management.json").exists());
}

#[cfg(target_os = "macos")]
#[test]
fn real_acl_denial_on_directories_leases_and_store() {
    use std::process::Command;
    for relative in [
        "",
        "state",
        "state/.workspace.lock",
        "state/.agent-management.receiver.lock",
        "state/agent-management.json",
    ] {
        let root = workspace();
        let files = Files::open(root.path()).unwrap();
        let guard = files.guard(Duration::ZERO).unwrap();
        let image = files.load(&guard).unwrap();
        files.replace(&guard, &image, NONCE, b"secret").unwrap();
        drop(guard);
        drop(files);
        let path = root.path().join(relative);
        assert!(
            Command::new("/bin/chmod")
                .arg("+a")
                .arg("everyone allow read")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Files::open(root.path())
                .and_then(|files| {
                    let guard = files.guard(Duration::ZERO)?;
                    files.load(&guard).map(|_| ())
                })
                .is_err()
        );
        assert!(
            Command::new("/bin/chmod")
                .arg("-N")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
    }
}
