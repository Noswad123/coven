use super::*;

const SETTINGS: &str = "settings.json";
const RECOVERY: &str = ".coven-claude-hooks-recovery";
const LIMIT: usize = 1024 * 1024;

#[test]
fn inode_fence_rejects_identical_replacement() {
    let temp = tempfile::tempdir().unwrap();
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
    let temp = tempfile::tempdir().unwrap();
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
#[cfg(target_os = "linux")]
fn exchange_retains_external_replacement_after_final_fence() {
    let temp = tempfile::tempdir().unwrap();
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
#[cfg(target_os = "linux")]
fn absence_move_restores_unexpected_entry_without_clobbering() {
    let temp = tempfile::tempdir().unwrap();
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
    assert!(
        Root::move_entry(&stash, "displaced", &root, SETTINGS, libc::RENAME_NOREPLACE).is_err()
    );
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
#[cfg(target_os = "linux")]
fn creation_after_absent_fence_never_clobbers() {
    let temp = tempfile::tempdir().unwrap();
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
    let temp = tempfile::tempdir().unwrap();
    for spec in [Spec::claude(), Spec::codex(), Spec::copilot()] {
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
                spec.errors.unsafe_leaf
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
