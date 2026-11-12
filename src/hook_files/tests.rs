use super::*;
use test_support::tempdir;

const SETTINGS: &str = "settings.json";
const RECOVERY: &str = ".coven-claude-hooks-recovery";
const LIMIT: usize = 1024 * 1024;

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn inode_fence_rejects_identical_replacement() {
    let temp = tempdir();
    std::fs::write(temp.path().join(SETTINGS), b"{}").unwrap();
    let root = Root::open(temp.path(), Spec::claude()).unwrap();
    let original = root.read(SETTINGS, LIMIT).unwrap();
    std::fs::write(temp.path().join("replacement"), b"{}").unwrap();
    std::fs::rename(temp.path().join("replacement"), temp.path().join(SETTINGS)).unwrap();
    assert_eq!(
        root.replace(SETTINGS, &original, Some(b"{\"changed\":true}"), 0o600)
            .unwrap_err()
            .code(),
        "claude_hooks_conflict"
    );
    assert_eq!(std::fs::read(temp.path().join(SETTINGS)).unwrap(), b"{}");
}

#[test]
fn fd_walk_cannot_be_redirected_after_ancestor_open() {
    use std::os::unix::fs::symlink;
    let temp = tempdir();
    let ancestor = temp.path().join("ancestor");
    let moved = temp.path().join("moved");
    let other = temp.path().join("other");
    std::fs::create_dir_all(ancestor.join("root/hooks")).unwrap();
    std::fs::create_dir_all(other.join("root/hooks")).unwrap();
    std::fs::write(ancestor.join("root/settings.json"), b"{\"pinned\":true}").unwrap();
    std::fs::write(other.join("root/settings.json"), b"{\"redirected\":true}").unwrap();
    std::fs::write(
        ancestor.join("root/hooks/kitsune-agent-state.sh"),
        b"pinned",
    )
    .unwrap();
    let root = Root::walk(&ancestor.join("root"), Spec::claude(), |opened| {
        if opened == ancestor {
            std::fs::rename(&ancestor, &moved).unwrap();
            symlink(&other, &ancestor).unwrap();
        }
    })
    .unwrap();
    assert_eq!(
        root.read(SETTINGS, LIMIT).unwrap().unwrap().bytes,
        b"{\"pinned\":true}"
    );
    assert_eq!(
        root.read_relative("hooks/kitsune-agent-state.sh", LIMIT)
            .unwrap()
            .unwrap()
            .bytes,
        b"pinned"
    );
    assert!(Root::open(&ancestor.join("root"), Spec::claude()).is_err());
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn exchange_retains_external_replacement_after_final_fence() {
    let temp = tempdir();
    std::fs::write(temp.path().join(SETTINGS), b"original").unwrap();
    let root = Root::open(temp.path(), Spec::claude()).unwrap();
    let original = root.read(SETTINGS, LIMIT).unwrap();
    let error = root
        .commit(SETTINGS, &original, Some(b"managed"), 0o600, || {
            std::fs::write(temp.path().join("external"), b"external edit").unwrap();
            std::fs::rename(temp.path().join("external"), temp.path().join(SETTINGS)).unwrap();
        })
        .unwrap_err();
    assert_eq!(error.code(), "claude_hooks_recovery_required");
    assert_eq!(
        std::fs::read(temp.path().join(SETTINGS)).unwrap(),
        b"managed"
    );
    assert_eq!(
        std::fs::read(temp.path().join(RECOVERY).join("replacement")).unwrap(),
        b"external edit"
    );
    assert_eq!(
        std::fs::read(temp.path().join(RECOVERY).join("expected")).unwrap(),
        b"original"
    );
    assert_eq!(
        std::fs::metadata(temp.path().join(RECOVERY))
            .unwrap()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        root.recovery_absent().unwrap_err().code(),
        "claude_hooks_recovery_required"
    );
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn absence_move_restores_unexpected_entry_without_clobbering() {
    let temp = tempdir();
    std::fs::write(temp.path().join(SETTINGS), b"managed").unwrap();
    let root = Root::open(temp.path(), Spec::claude()).unwrap();
    let original = root.read(SETTINGS, LIMIT).unwrap();
    let error = root
        .commit(SETTINGS, &original, None, 0o600, || {
            std::fs::write(temp.path().join("external"), b"external edit").unwrap();
            std::fs::rename(temp.path().join("external"), temp.path().join(SETTINGS)).unwrap();
        })
        .unwrap_err();
    assert_eq!(error.code(), "claude_hooks_recovery_required");
    assert_eq!(
        std::fs::read(temp.path().join(SETTINGS)).unwrap(),
        b"external edit"
    );
    assert_eq!(
        std::fs::read(temp.path().join(RECOVERY).join("expected")).unwrap(),
        b"managed"
    );
    let mut stash = root.open_directory(RECOVERY.as_bytes()).unwrap();
    stash.1.leaves = &["expected", "replacement", "displaced", "transaction.json"];
    stash.1.private = &[];
    std::fs::write(
        temp.path().join(RECOVERY).join("displaced"),
        b"another retained edit",
    )
    .unwrap();
    assert!(Root::move_entry(&stash, "displaced", &root, SETTINGS, MoveKind::NoReplace).is_err());
    assert_eq!(
        std::fs::read(temp.path().join(SETTINGS)).unwrap(),
        b"external edit"
    );
    assert_eq!(
        std::fs::read(temp.path().join(RECOVERY).join("displaced")).unwrap(),
        b"another retained edit"
    );
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn creation_after_absent_fence_never_clobbers() {
    let temp = tempdir();
    let root = Root::open(temp.path(), Spec::claude()).unwrap();
    let error = root
        .commit(SETTINGS, &None, Some(b"managed"), 0o600, || {
            std::fs::write(temp.path().join(SETTINGS), b"new external entry").unwrap();
        })
        .unwrap_err();
    assert_eq!(error.code(), "claude_hooks_recovery_required");
    assert_eq!(
        std::fs::read(temp.path().join(SETTINGS)).unwrap(),
        b"new external entry"
    );
    assert_eq!(
        std::fs::read(temp.path().join(RECOVERY).join("replacement")).unwrap(),
        b"managed"
    );
}

#[test]
fn static_names_and_provider_errors_are_enforced() {
    let temp = tempdir();
    for spec in [
        Spec::claude(),
        Spec::codex(),
        Spec::copilot(),
        Spec::pi(),
        Spec::opencode(),
    ] {
        let root = Root::open(temp.path(), spec).unwrap();
        for name in [
            "../settings.json",
            "/settings.json",
            "hooks/../settings.json",
            "",
            ".",
            "..",
            "arbitrary",
        ] {
            assert_eq!(
                root.read(name, LIMIT).err().unwrap().code(),
                spec.errors.unsafe_leaf
            );
            assert_eq!(
                root.replace(name, &None, Some(b"data"), 0o600)
                    .unwrap_err()
                    .code(),
                if supports_apply() {
                    spec.errors.unsafe_leaf
                } else {
                    spec.errors.apply_unsupported_platform
                }
            );
        }
        assert!(root.read_relative("hooks/../settings.json", LIMIT).is_err());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    }
    let root = Root::open(temp.path(), Spec::codex()).unwrap();
    std::fs::create_dir(temp.path().join(".coven-codex-hooks-recovery")).unwrap();
    assert_eq!(
        root.recovery_absent().unwrap_err().code(),
        "codex_hooks_recovery_required"
    );
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn transaction_preserves_exact_bytes_and_modes() {
    let temp = tempdir();
    let root = Root::open(temp.path(), Spec::claude()).unwrap();
    let bytes = b"\0secret\xff\n";
    root.replace(SETTINGS, &None, Some(bytes), 0o640).unwrap();
    let image = root.read(SETTINGS, LIMIT).unwrap();
    assert_eq!(image.as_ref().unwrap().bytes, bytes);
    assert_eq!(image.as_ref().unwrap().mode & 0o7777, 0o640);
    root.replace(SETTINGS, &image, Some(b"next"), 0o600)
        .unwrap();
    let image = root.read(SETTINGS, LIMIT).unwrap();
    assert_eq!(image.as_ref().unwrap().bytes, b"next");
    assert_eq!(image.as_ref().unwrap().mode & 0o7777, 0o600);
    root.replace(SETTINGS, &image, None, 0o600).unwrap();
    assert!(root.read(SETTINGS, LIMIT).unwrap().is_none());
    root.recovery_absent().unwrap();
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn same_directory_no_replace_refuses_same_object_and_hardlink_alias() {
    let temp = tempdir();
    std::fs::write(temp.path().join(SETTINGS), b"original").unwrap();
    let other = "kitsune-agent-state.sh";
    std::fs::hard_link(temp.path().join(SETTINGS), temp.path().join(other)).unwrap();
    let root = Root::open(temp.path(), Spec::claude()).unwrap();
    let alias = Root::open(temp.path(), Spec::claude()).unwrap();
    for target in [SETTINGS, other] {
        assert!(Root::move_entry(&root, SETTINGS, &alias, target, MoveKind::NoReplace).is_err());
        assert_eq!(
            std::fs::read(temp.path().join(target)).unwrap(),
            b"original"
        );
    }
}

#[test]
fn unsafe_leaf_types_modes_and_root_alias_are_rejected() {
    use std::os::unix::fs::symlink;
    let temp = tempdir();
    let path = temp.path().join(SETTINGS);
    let root = Root::open(temp.path(), Spec::claude()).unwrap();
    std::fs::write(&path, b"private").unwrap();
    std::fs::set_permissions(&path, Permissions::from_mode(0o666)).unwrap();
    assert_eq!(
        root.read(SETTINGS, LIMIT).err().unwrap().code(),
        "claude_hooks_unsafe_leaf"
    );
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o666);
    std::fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
    std::fs::hard_link(&path, temp.path().join("hardlink")).unwrap();
    assert!(root.read(SETTINGS, LIMIT).is_err());
    std::fs::remove_file(&path).unwrap();
    symlink("hardlink", &path).unwrap();
    assert!(root.read(SETTINGS, LIMIT).is_err());
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(root.read(SETTINGS, LIMIT).is_err());
    std::fs::remove_dir(&path).unwrap();
    use std::os::unix::ffi::OsStrExt;
    let fifo = CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(root.read(SETTINGS, LIMIT).is_err());
    std::fs::create_dir(temp.path().join("real")).unwrap();
    symlink("real", temp.path().join("alias")).unwrap();
    assert!(Root::open(&temp.path().join("alias"), Spec::claude()).is_err());
    std::fs::create_dir(temp.path().join("hooks")).unwrap();
    std::fs::set_permissions(temp.path().join("hooks"), Permissions::from_mode(0o777)).unwrap();
    assert!(root.directory(b"hooks").is_err());
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn checkpoint_failures_retain_recovery_and_distinguish_final_sync() {
    for fault in [
        Checkpoint::StashAccess,
        Checkpoint::StageAccess,
        Checkpoint::StageWrite,
        Checkpoint::StageSync,
        Checkpoint::PreMoveSync,
        Checkpoint::Move,
        Checkpoint::PostMoveSync,
        Checkpoint::Verify,
        Checkpoint::Cleanup,
        Checkpoint::CleanupSync,
        Checkpoint::FinalSync,
    ] {
        let temp = tempdir();
        let path = temp.path().join(SETTINGS);
        std::fs::write(&path, b"original\0\xff").unwrap();
        std::fs::set_permissions(&path, Permissions::from_mode(0o640)).unwrap();
        let root = Root::open(temp.path(), Spec::claude()).unwrap();
        let expected = root.read(SETTINGS, LIMIT).unwrap();
        let mut hit = false;
        let error = root
            .commit_with_fault(
                SETTINGS,
                &expected,
                Some(b"secret replacement"),
                0o600,
                || {},
                |point, _| {
                    if point == fault {
                        hit = true;
                        // Unsupported native rename is not permission to use ordinary rename.
                        return Err(std::io::Error::from_raw_os_error(
                            if point == Checkpoint::Move {
                                libc::ENOTSUP
                            } else {
                                libc::EIO
                            },
                        ));
                    }
                    Ok(())
                },
            )
            .unwrap_err();
        assert!(hit, "{fault:?}");
        if fault == Checkpoint::FinalSync {
            assert_eq!(error.code(), "claude_hooks_durability_uncertain");
            assert!(!temp.path().join(RECOVERY).exists());
        } else {
            assert_eq!(error.code(), "claude_hooks_recovery_required", "{fault:?}");
            assert!(temp.path().join(RECOVERY).is_dir());
        }
        let before_move = matches!(
            fault,
            Checkpoint::StashAccess
                | Checkpoint::StageAccess
                | Checkpoint::StageWrite
                | Checkpoint::StageSync
                | Checkpoint::PreMoveSync
                | Checkpoint::Move
        );
        let original = if before_move {
            path.clone()
        } else {
            temp.path().join(RECOVERY).join("replacement")
        };
        // Cleanup has already removed the backup when its sync fails.
        if !matches!(fault, Checkpoint::CleanupSync | Checkpoint::FinalSync) {
            assert_eq!(
                std::fs::read(&original).unwrap(),
                b"original\0\xff",
                "{fault:?}"
            );
            assert_eq!(std::fs::metadata(&original).unwrap().mode() & 0o7777, 0o640);
        }
        if matches!(
            fault,
            Checkpoint::StashAccess | Checkpoint::StageAccess | Checkpoint::StageWrite
        ) {
            for entry in std::fs::read_dir(temp.path().join(RECOVERY)).unwrap() {
                assert_eq!(entry.unwrap().metadata().unwrap().len(), 0, "{fault:?}");
            }
        }
    }
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn invalid_exact_staged_mode_is_never_published() {
    let temp = tempdir();
    let root = Root::open(temp.path(), Spec::claude()).unwrap();
    assert!(
        root.replace(SETTINGS, &None, Some(b"secret"), 0o100600)
            .is_err()
    );
    assert!(!temp.path().join(SETTINGS).exists());
    assert!(temp.path().join(RECOVERY).is_dir());
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn root_and_lock_modes_are_revalidated_without_repair() {
    let temp = tempdir();
    let root = Root::open(temp.path(), Spec::claude()).unwrap();
    std::fs::set_permissions(temp.path(), Permissions::from_mode(0o777)).unwrap();
    assert_eq!(
        root.lock(".coven-claude-hooks.lock").err().unwrap().code(),
        "claude_hooks_unsafe_root"
    );
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    std::fs::set_permissions(temp.path(), Permissions::from_mode(0o700)).unwrap();
    let path = temp.path().join(".coven-claude-hooks.lock");
    std::fs::write(&path, b"").unwrap();
    std::fs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        root.lock(".coven-claude-hooks.lock").err().unwrap().code(),
        "claude_hooks_unsafe_private_file"
    );
    assert_eq!(std::fs::metadata(path).unwrap().mode() & 0o7777, 0o644);
}

#[cfg(target_os = "macos")]
fn add_acl(path: &Path) {
    assert!(
        std::process::Command::new("/bin/chmod")
            .arg("+a")
            .arg(format!("user:{} allow read", unsafe { libc::geteuid() }))
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
#[cfg(target_os = "macos")]
fn acl_refused_new_stash_and_stage_receive_no_secret_bytes() {
    for fault in [Checkpoint::StashAccess, Checkpoint::StageAccess] {
        let temp = tempdir();
        let root = Root::open(temp.path(), Spec::claude()).unwrap();
        let mut hit = false;
        let error = root
            .commit_with_fault(
                SETTINGS,
                &None,
                Some(b"secret"),
                0o600,
                || {},
                |point, _| {
                    if point == fault {
                        hit = true;
                        let path = temp.path().join(RECOVERY);
                        add_acl(&if point == Checkpoint::StageAccess {
                            path.join("replacement")
                        } else {
                            path
                        });
                    }
                    Ok(())
                },
            )
            .unwrap_err();
        assert!(hit);
        assert_eq!(error.code(), "claude_hooks_recovery_required");
        assert!(!temp.path().join(SETTINGS).exists());
        for entry in std::fs::read_dir(temp.path().join(RECOVERY)).unwrap() {
            assert_eq!(entry.unwrap().metadata().unwrap().len(), 0);
        }
    }
}

#[test]
#[cfg(target_os = "macos")]
fn acl_admission_and_physical_root_aliases_are_refused() {
    let temp = tempdir();
    let root = Root::open(temp.path(), Spec::claude()).unwrap();
    let leaf = temp.path().join(SETTINGS);
    std::fs::write(&leaf, b"secret").unwrap();
    add_acl(&leaf);
    assert_eq!(
        root.read(SETTINGS, LIMIT).err().unwrap().code(),
        "claude_hooks_unsafe_leaf"
    );
    let lock = temp.path().join(".coven-claude-hooks.lock");
    std::fs::write(&lock, b"").unwrap();
    std::fs::set_permissions(&lock, Permissions::from_mode(0o600)).unwrap();
    add_acl(&lock);
    assert_eq!(
        root.lock(".coven-claude-hooks.lock").err().unwrap().code(),
        "claude_hooks_unsafe_leaf"
    );
    let directory = temp.path().join("hooks");
    std::fs::create_dir(&directory).unwrap();
    add_acl(&directory);
    assert_eq!(
        root.directory(b"hooks").err().unwrap().code(),
        "claude_hooks_unsafe_root"
    );
    add_acl(temp.path());
    assert_eq!(
        root.lock(".coven-claude-hooks.lock").err().unwrap().code(),
        "claude_hooks_unsafe_root"
    );
    assert_eq!(
        Root::open(temp.path(), Spec::claude())
            .err()
            .unwrap()
            .code(),
        "claude_hooks_unsafe_root"
    );
    for alias in ["/tmp", "/var/tmp"] {
        if std::fs::symlink_metadata(alias)
            .unwrap()
            .file_type()
            .is_symlink()
            || alias.starts_with("/var/")
                && std::fs::symlink_metadata("/var")
                    .unwrap()
                    .file_type()
                    .is_symlink()
        {
            assert!(Root::open(Path::new(alias), Spec::claude()).is_err());
        }
    }
}

#[test]
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unsupported_apply_refuses_before_lock_creation() {
    let temp = tempdir();
    let root = Root::open(temp.path(), Spec::claude()).unwrap();
    assert!(!supports_apply());
    assert_eq!(
        root.lock(".coven-claude-hooks.lock").err().unwrap().code(),
        "claude_hooks_apply_unsupported_platform"
    );
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}
