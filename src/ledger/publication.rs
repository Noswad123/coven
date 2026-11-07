//! Private immutable ledger transport. Domain admission belongs to activation.
//! Assumes static paths on a cooperating local filesystem, not hostile path races.
use super::{DIRECTORY, MARKER, MAX_DIRECTORY_ENTRIES, MAX_FRAME_BYTES, MAX_FRAMES};
use crate::{
    Error, Result,
    error::require,
    platform, read_fs,
    records::TaskRecords,
    storage::WorkspaceGuard,
    values::{self, Profile},
};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy)]
pub(super) enum ArtifactRole<'a> {
    Marker,
    Manifest { epoch: &'a str },
    Baseline { epoch: &'a str },
    Frame { epoch: &'a str, sequence: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Publication {
    Synced,
    SyncUncertain,
}

pub(super) fn relative(role: ArtifactRole<'_>) -> Result<PathBuf> {
    let (epoch, name) = match role {
        ArtifactRole::Marker => return Ok(PathBuf::from(MARKER)),
        ArtifactRole::Manifest { epoch } => (epoch, PathBuf::from("manifest.json")),
        ArtifactRole::Baseline { epoch } => (epoch, PathBuf::from("baseline.tasks.bytes")),
        ArtifactRole::Frame { epoch, sequence } => {
            require(sequence < MAX_FRAMES, "unsupported_ledger_history_profile")?;
            (epoch, PathBuf::from(format!("frames/{sequence:020}.frame")))
        }
    };
    require(
        epoch.len() == 32
            && epoch
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "invalid_ledger_identity",
    )?;
    Ok(Path::new(DIRECTORY).join(epoch).join(name))
}

trait PublicationIo {
    fn temporary(&self, parent: &Path, name: &OsStr) -> std::io::Result<tempfile::NamedTempFile> {
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
    fn link(&self, source: &Path, target: &Path) -> std::io::Result<()> {
        fs::hard_link(source, target)
    }
    fn sync_dir(&self, parent: &Path) -> std::io::Result<()> {
        platform::sync_directory(parent)
    }
    fn cleanup(&self, temp: tempfile::NamedTempFile) -> std::io::Result<()> {
        temp.close()
    }
}
struct SystemPublicationIo;
impl PublicationIo for SystemPublicationIo {}

fn safe_target(guard: &WorkspaceGuard, relative: &Path) -> Result<PathBuf> {
    let target = read_fs::managed(guard.root(), relative, false)?;
    let parent = target.parent().ok_or(Error("invalid_publication_parent"))?;
    require(
        fs::symlink_metadata(parent)?.is_dir(),
        "unsafe_publication_parent",
    )?;
    Ok(target)
}

fn reserve_entries(guard: &WorkspaceGuard, relative: &Path) -> Result<()> {
    let parent = relative
        .parent()
        .ok_or(Error("invalid_publication_parent"))?;
    let path = read_fs::managed(guard.root(), parent, true)?;
    // No names collection: stop before exceeding the reader's bounded profile.
    for (index, entry) in fs::read_dir(path)?.enumerate() {
        require(
            index < MAX_DIRECTORY_ENTRIES - 2,
            "unsupported_ledger_directory_profile",
        )?;
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().ok_or(Error("invalid_ledger_entry_name"))?;
        // Match the reader's type policy, including otherwise inert temp names.
        read_fs::managed(
            guard.root(),
            &parent.join(name),
            parent.file_name() != Some(OsStr::new("frames")) && name == "frames",
        )?;
    }
    Ok(())
}

/// No overwrite, adoption, rollback, or post-link read. Only mode bits are
/// preserved for source backups, not ownership, ACLs, or extended attributes.
pub(super) fn publish(
    guard: &WorkspaceGuard,
    role: ArtifactRole<'_>,
    bytes: &[u8],
    source_mode: Option<u32>,
) -> Result<Publication> {
    publish_with(guard, role, bytes, source_mode, &SystemPublicationIo)
}

fn publish_with(
    guard: &WorkspaceGuard,
    role: ArtifactRole<'_>,
    bytes: &[u8],
    source_mode: Option<u32>,
    io: &impl PublicationIo,
) -> Result<Publication> {
    guard.validate_for(guard.root())?;
    let relative = relative(role)?;
    require(
        bytes.len() <= MAX_FRAME_BYTES,
        "unsupported_ledger_frame_size",
    )?;
    let mode = match (role, source_mode) {
        (ArtifactRole::Baseline { .. }, Some(mode)) if mode <= 0o7777 => mode,
        (ArtifactRole::Baseline { .. }, _) | (_, Some(_)) => {
            return Err(Error("invalid_ledger_source_mode"));
        }
        (_, None) => 0o600,
    };
    // Reparse the actual encoded envelope: encoder limits alone do not bound
    // reader node counts or the extra nesting introduced by a frame envelope.
    match role {
        ArtifactRole::Frame { .. } => {
            super::payload(bytes)?;
        }
        ArtifactRole::Baseline { .. } => {
            TaskRecords::validate(values::parse(bytes, Profile::Authority)?)?;
        }
        ArtifactRole::Marker | ArtifactRole::Manifest { .. } => {
            values::parse(bytes, Profile::Authority)?;
        }
    }
    let target = safe_target(guard, &relative)?;
    let parent = target.parent().ok_or(Error("invalid_publication_parent"))?;
    if !matches!(role, ArtifactRole::Marker) {
        reserve_entries(guard, &relative)?;
    }
    let name = target
        .file_name()
        .ok_or(Error("invalid_publication_parent"))?;
    let mut temp = io.temporary(parent, name)?;
    let result = (|| {
        io.write(temp.as_file_mut(), bytes)?;
        // Writes may clear special bits; set and verify them afterward.
        io.apply_mode(temp.as_file(), mode)?;
        io.verify_mode(temp.as_file(), mode)?;
        io.sync_file(temp.as_file())?;
        io.recheck(guard, &relative)?;
        io.link(temp.path(), &target)?;
        // Link success is known publication, even if sync or cleanup fails.
        Ok(if io.sync_dir(parent).is_ok() {
            Publication::Synced
        } else {
            Publication::SyncUncertain
        })
    })();
    let _ = io.cleanup(temp);
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{canonical, values::Value};
    use std::{
        cell::RefCell,
        os::unix::fs::{MetadataExt, PermissionsExt, symlink},
        time::Duration,
    };

    const EPOCH: &str = "0123456789abcdef0123456789abcdef";
    const ROLES: [ArtifactRole<'static>; 4] = [
        ArtifactRole::Marker,
        ArtifactRole::Manifest { epoch: EPOCH },
        ArtifactRole::Baseline { epoch: EPOCH },
        ArtifactRole::Frame {
            epoch: EPOCH,
            sequence: 0,
        },
    ];

    fn workspace() -> (tempfile::TempDir, WorkspaceGuard) {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("coven.json"), b"marker").unwrap();
        let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap();
        fs::create_dir_all(root.path().join(DIRECTORY).join(EPOCH).join("frames")).unwrap();
        (root, guard)
    }

    fn envelope(value: Value) -> Vec<u8> {
        let encoded = value.canonical().unwrap();
        let mut raw = Value::object(vec![
            ("version".into(), Value::integer("1").unwrap()),
            (
                "length".into(),
                Value::integer(&encoded.len().to_string()).unwrap(),
            ),
            ("sha256".into(), Value::String(canonical::digest(&encoded))),
            ("payload".into(), value),
        ])
        .unwrap()
        .canonical()
        .unwrap();
        raw.push(b'\n');
        raw
    }

    fn document(role: ArtifactRole<'_>) -> Vec<u8> {
        let digest = canonical::digest(b"");
        match role {
            ArtifactRole::Marker => format!(r#"{{"version":1,"workspace_id":"{EPOCH}","epoch_id":"{EPOCH}","manifest_digest":"{digest}","baseline_digest":"{digest}"}}"#).into_bytes(),
            ArtifactRole::Manifest { .. } => format!(r#"{{"version":1,"workspace_id":"{EPOCH}","epoch_id":"{EPOCH}","source_revision":"tasks-missing:v1","source_present":false,"source_mode":null,"source_length":0,"source_digest":"{digest}","baseline_digest":"{digest}"}}"#).into_bytes(),
            ArtifactRole::Baseline { .. } => b"[]\n".to_vec(),
            ArtifactRole::Frame { .. } => {
                let revision = super::super::revision(EPOCH, EPOCH, 0, &Value::Array(vec![])).unwrap();
                envelope(values::parse(format!(r#"{{"version":1,"workspace_id":"{EPOCH}","epoch_id":"{EPOCH}","sequence":0,"previous_digest":null,"source_revision":"tasks-missing:v1","tasks":[],"revision":"{revision}"}}"#).as_bytes(), Profile::Authority).unwrap())
            }
        }
    }

    fn mode(role: ArtifactRole<'_>) -> Option<u32> {
        matches!(role, ArtifactRole::Baseline { .. }).then_some(0o7755)
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
        fn step(&self, step: &'static str) -> std::io::Result<()> {
            self.events.borrow_mut().push(step);
            if self.fail == step {
                Err(std::io::Error::other(step))
            } else {
                Ok(())
            }
        }
    }
    impl PublicationIo for FaultIo {
        fn temporary(
            &self,
            parent: &Path,
            name: &OsStr,
        ) -> std::io::Result<tempfile::NamedTempFile> {
            self.step("temporary")?;
            let temp = SystemPublicationIo.temporary(parent, name)?;
            *self.temp.borrow_mut() = Some(temp.path().to_owned());
            Ok(temp)
        }
        fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
            file.write_all(&bytes[..bytes.len() / 2])?;
            self.step("write")?;
            file.write_all(&bytes[bytes.len() / 2..])
        }
        fn apply_mode(&self, file: &File, mode: u32) -> std::io::Result<()> {
            self.step("mode")?;
            SystemPublicationIo.apply_mode(file, mode)
        }
        fn verify_mode(&self, file: &File, mode: u32) -> std::io::Result<()> {
            self.step("verify")?;
            SystemPublicationIo.verify_mode(file, mode)
        }
        fn sync_file(&self, file: &File) -> std::io::Result<()> {
            self.step("file_sync")?;
            SystemPublicationIo.sync_file(file)
        }
        fn recheck(&self, guard: &WorkspaceGuard, relative: &Path) -> Result<()> {
            self.step("recheck")
                .map_err(|_| Error("injected_recheck"))?;
            SystemPublicationIo.recheck(guard, relative)
        }
        fn link(&self, source: &Path, target: &Path) -> std::io::Result<()> {
            self.step("link")?;
            SystemPublicationIo.link(source, target)
        }
        fn sync_dir(&self, parent: &Path) -> std::io::Result<()> {
            self.step("parent_sync")?;
            SystemPublicationIo.sync_dir(parent)
        }
        fn cleanup(&self, temp: tempfile::NamedTempFile) -> std::io::Result<()> {
            self.events.borrow_mut().push("cleanup");
            if self.cleanup_fails {
                let (_, path) = temp.keep()?;
                assert_eq!(Some(path), *self.temp.borrow());
                Err(std::io::Error::other("cleanup"))
            } else {
                SystemPublicationIo.cleanup(temp)
            }
        }
    }

    #[test]
    fn every_fault_preserves_full_bytes_modes_inodes_and_known_publication() {
        let steps = [
            "temporary",
            "write",
            "mode",
            "verify",
            "file_sync",
            "recheck",
            "link",
            "parent_sync",
            "success",
        ];
        // 4 roles * 2 target states * 9 boundaries * 2 cleanup outcomes.
        for role in ROLES {
            for existing in [false, true] {
                for fail in steps {
                    for cleanup_fails in [false, true] {
                        let (root, guard) = workspace();
                        let target = root.path().join(relative(role).unwrap());
                        let bytes = document(role);
                        if existing {
                            // Even identical final bytes are not adopted or replaced.
                            fs::write(&target, &bytes).unwrap();
                            fs::set_permissions(&target, fs::Permissions::from_mode(0o7755))
                                .unwrap();
                        }
                        let before = fs::metadata(&target).ok();
                        let orphan = target.with_file_name(format!(
                            ".{}.abandoned.tmp",
                            target.file_name().unwrap().to_str().unwrap()
                        ));
                        fs::write(&orphan, b"not authority").unwrap();
                        let orphan_before = fs::metadata(&orphan).unwrap();
                        let lock = root.path().join("state/.workspace.lock");
                        let lock_before = fs::metadata(&lock).unwrap();
                        let io = FaultIo::new(fail, cleanup_fails);
                        let result = publish_with(&guard, role, &bytes, mode(role), &io);
                        let linked = !existing && matches!(fail, "parent_sync" | "success");
                        if linked {
                            assert_eq!(
                                result.unwrap(),
                                if fail == "parent_sync" {
                                    Publication::SyncUncertain
                                } else {
                                    Publication::Synced
                                }
                            );
                            assert_eq!(fs::read(&target).unwrap(), bytes);
                            let after = fs::metadata(&target).unwrap();
                            assert_eq!(after.mode() & 0o7777, mode(role).unwrap_or(0o600));
                            if cleanup_fails {
                                let temp =
                                    fs::metadata(io.temp.borrow().as_ref().unwrap()).unwrap();
                                assert_eq!((after.dev(), after.ino()), (temp.dev(), temp.ino()));
                                assert_eq!(after.nlink(), 2);
                            } else {
                                assert_eq!(after.nlink(), 1);
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
                                assert_eq!(
                                    (after.dev(), after.ino(), after.mode()),
                                    (before.dev(), before.ino(), before.mode())
                                );
                                assert_eq!(fs::read(&target).unwrap(), bytes);
                            } else {
                                assert!(!target.exists());
                            }
                        }
                        let through = steps
                            .iter()
                            .position(|step| *step == fail)
                            .unwrap()
                            .min(if existing { 6 } else { 7 });
                        let mut expected = steps[..=through].to_vec();
                        if fail != "temporary" {
                            expected.push("cleanup");
                        }
                        assert_eq!(*io.events.borrow(), expected);
                        if let Some(temp) = io.temp.borrow().as_ref() {
                            assert_eq!(temp.parent(), target.parent());
                            assert_ne!(temp, &orphan);
                            let name = temp.file_name().unwrap().to_str().unwrap();
                            assert!(name.starts_with(&format!(
                                ".{}.",
                                target.file_name().unwrap().to_str().unwrap()
                            )));
                            assert!(name.ends_with(".tmp"));
                            if !matches!(role, ArtifactRole::Marker) {
                                assert!(super::super::inert(name));
                            }
                            assert_eq!(temp.exists(), cleanup_fails);
                            if cleanup_fails && fail != "write" {
                                assert_eq!(fs::read(temp).unwrap(), bytes);
                            }
                        }
                        assert_eq!(fs::read(&orphan).unwrap(), b"not authority");
                        assert_eq!(fs::metadata(&orphan).unwrap().ino(), orphan_before.ino());
                        assert_eq!(fs::metadata(lock).unwrap().ino(), lock_before.ino());
                    }
                }
            }
        }
    }

    #[test]
    fn exact_role_paths_and_strict_identifiers_and_sequences() {
        for (role, suffix) in ROLES.into_iter().zip([
            "state/task-authority.json".to_owned(),
            format!("{DIRECTORY}/{EPOCH}/manifest.json"),
            format!("{DIRECTORY}/{EPOCH}/baseline.tasks.bytes"),
            format!("{DIRECTORY}/{EPOCH}/frames/00000000000000000000.frame"),
        ]) {
            assert_eq!(relative(role).unwrap(), PathBuf::from(suffix));
        }
        for epoch in [
            "",
            ".",
            "..",
            "../escape",
            "/absolute",
            "0123456789ABCDEF0123456789ABCDEF",
            "0123456789abcdef0123456789abcdeg",
            "0123456789abcdef0123456789abcdeff",
        ] {
            for role in [
                ArtifactRole::Manifest { epoch },
                ArtifactRole::Baseline { epoch },
                ArtifactRole::Frame { epoch, sequence: 0 },
            ] {
                assert!(relative(role).is_err());
            }
        }
        assert!(
            relative(ArtifactRole::Frame {
                epoch: EPOCH,
                sequence: MAX_FRAMES - 1
            })
            .unwrap()
            .ends_with("00000000000000004095.frame")
        );
        for sequence in [MAX_FRAMES, usize::MAX] {
            assert!(
                relative(ArtifactRole::Frame {
                    epoch: EPOCH,
                    sequence
                })
                .is_err()
            );
        }
    }

    #[test]
    fn successful_chmod_with_sgid_loss_fails_real_verification_before_link() {
        struct ClearSgid;
        impl PublicationIo for ClearSgid {
            fn apply_mode(&self, file: &File, mode: u32) -> std::io::Result<()> {
                platform::apply_replacement_mode(file, mode & !0o2000)
            }
            fn sync_file(&self, _: &File) -> std::io::Result<()> {
                panic!("must fail before sync");
            }
        }
        let (root, guard) = workspace();
        let role = ROLES[2];
        assert_eq!(
            publish_with(&guard, role, &document(role), Some(0o7755), &ClearSgid)
                .unwrap_err()
                .code(),
            "input_or_storage_unavailable"
        );
        assert!(!root.path().join(relative(role).unwrap()).exists());
        assert_eq!(
            fs::read_dir(root.path().join(DIRECTORY).join(EPOCH))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn mode_contract_and_invalid_documents_fail_before_temp_creation() {
        let (_root, guard) = workspace();
        for role in ROLES {
            for source_mode in [
                None,
                Some(0),
                Some(0o600),
                Some(0o7755),
                Some(0o7777),
                Some(0o10000),
            ] {
                let io = FaultIo::new("temporary", false);
                let result = publish_with(&guard, role, &document(role), source_mode, &io);
                let valid = if matches!(role, ArtifactRole::Baseline { .. }) {
                    source_mode.is_some_and(|mode| mode <= 0o7777)
                } else {
                    source_mode.is_none()
                };
                assert_eq!(
                    result.unwrap_err().code(),
                    if valid {
                        "input_or_storage_unavailable"
                    } else {
                        "invalid_ledger_source_mode"
                    }
                );
                assert_eq!(io.events.borrow().len(), usize::from(valid));
            }
            for raw in [b"{bad".as_slice(), b"{\"x\":1,\"x\":2}", b"[NaN]"] {
                let io = FaultIo::new("success", false);
                assert!(publish_with(&guard, role, raw, mode(role), &io).is_err());
                assert!(io.events.borrow().is_empty());
            }
        }
        let io = FaultIo::new("success", false);
        assert!(publish_with(&guard, ROLES[2], b"{}", mode(ROLES[2]), &io).is_err());
        assert!(io.events.borrow().is_empty());
    }

    #[test]
    fn safe_existing_parents_targets_and_inert_types_are_required_without_mkdir() {
        for kind in [
            "missing",
            "parent_file",
            "parent_symlink",
            "target_dir",
            "target_symlink",
            "target_fifo",
            "temp_dir",
            "temp_symlink",
        ] {
            let (root, guard) = workspace();
            let role = ROLES[1];
            let target = root.path().join(relative(role).unwrap());
            let parent = target.parent().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let inert = parent.join(".manifest.json.unsafe.tmp");
            match kind {
                "missing" | "parent_file" | "parent_symlink" => {
                    fs::remove_dir_all(parent).unwrap();
                    if kind == "parent_file" {
                        fs::write(parent, b"file").unwrap();
                    }
                    if kind == "parent_symlink" {
                        symlink(outside.path(), parent).unwrap();
                    }
                }
                "target_dir" => fs::create_dir(&target).unwrap(),
                "target_symlink" => symlink(root.path().join("coven.json"), &target).unwrap(),
                "target_fifo" => {
                    use std::os::unix::ffi::OsStrExt;
                    let name = std::ffi::CString::new(target.as_os_str().as_bytes()).unwrap();
                    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
                }
                "temp_dir" => fs::create_dir(inert).unwrap(),
                "temp_symlink" => symlink(root.path().join("coven.json"), inert).unwrap(),
                _ => unreachable!(),
            }
            let io = FaultIo::new("success", false);
            assert!(
                publish_with(&guard, role, &document(role), None, &io).is_err(),
                "{kind}"
            );
            assert!(io.events.borrow().is_empty());
            assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
            if kind == "missing" {
                assert!(!parent.exists());
            }
        }
    }

    #[test]
    fn recheck_rejects_changed_parent_and_target() {
        let (root, guard) = workspace();
        let role = ROLES[3];
        let relative = relative(role).unwrap();
        let target = root.path().join(&relative);
        SystemPublicationIo.recheck(&guard, &relative).unwrap();
        symlink(root.path().join("coven.json"), &target).unwrap();
        assert!(SystemPublicationIo.recheck(&guard, &relative).is_err());
        fs::remove_file(&target).unwrap();
        fs::remove_dir(target.parent().unwrap()).unwrap();
        assert!(SystemPublicationIo.recheck(&guard, &relative).is_err());
        symlink(root.path().join("state"), target.parent().unwrap()).unwrap();
        assert!(SystemPublicationIo.recheck(&guard, &relative).is_err());
    }

    #[test]
    fn capacity_reserves_temp_and_final_in_both_enumerated_parents() {
        for role in [ROLES[1], ROLES[3]] {
            let (root, guard) = workspace();
            let target = root.path().join(relative(role).unwrap());
            let parent = target.parent().unwrap();
            let initial = fs::read_dir(parent).unwrap().count();
            for index in initial..MAX_DIRECTORY_ENTRIES - 2 {
                fs::write(
                    parent.join(format!(".manifest.json.{index}.tmp")),
                    b"orphan",
                )
                .unwrap();
            }
            let extra = parent.join("unknown-name-not-adopted");
            fs::write(&extra, b"unknown").unwrap();
            let io = FaultIo::new("success", false);
            assert_eq!(
                publish_with(&guard, role, &document(role), None, &io)
                    .unwrap_err()
                    .code(),
                "unsupported_ledger_directory_profile"
            );
            assert!(io.events.borrow().is_empty());
            assert_eq!(fs::read(&extra).unwrap(), b"unknown");
            fs::remove_file(extra).unwrap();
            let io = FaultIo::new("success", true);
            assert_eq!(
                publish_with(&guard, role, &document(role), None, &io).unwrap(),
                Publication::Synced
            );
            assert_eq!(fs::read_dir(parent).unwrap().count(), MAX_DIRECTORY_ENTRIES);
        }
    }

    #[test]
    fn marker_does_not_enumerate_state_and_unknown_names_are_never_adopted() {
        let (root, guard) = workspace();
        // Unsafe unrelated entries are not part of marker publication's parent profile.
        symlink("missing", root.path().join("state/.unrelated.tmp")).unwrap();
        fs::write(root.path().join("state/unknown"), b"not authority").unwrap();
        assert_eq!(
            publish(&guard, ROLES[0], &document(ROLES[0]), None).unwrap(),
            Publication::Synced
        );
        assert_eq!(
            fs::read(root.path().join("state/unknown")).unwrap(),
            b"not authority"
        );
    }

    #[test]
    fn exclusive_temporary_collision_never_opens_or_deletes_existing_temp() {
        struct Collision;
        impl PublicationIo for Collision {
            fn temporary(
                &self,
                parent: &Path,
                _: &OsStr,
            ) -> std::io::Result<tempfile::NamedTempFile> {
                tempfile::Builder::new()
                    .prefix(".manifest.json.collision")
                    .suffix(".tmp")
                    .rand_bytes(0)
                    .tempfile_in(parent)
            }
        }
        let (root, guard) = workspace();
        let role = ROLES[1];
        let target = root.path().join(relative(role).unwrap());
        let collision = target.with_file_name(".manifest.json.collision.tmp");
        fs::write(&collision, b"existing temp").unwrap();
        let before = fs::metadata(&collision).unwrap();
        assert!(publish_with(&guard, role, &document(role), None, &Collision).is_err());
        assert!(!target.exists());
        assert_eq!(fs::read(&collision).unwrap(), b"existing temp");
        assert_eq!(fs::metadata(collision).unwrap().ino(), before.ino());
    }

    #[test]
    fn exact_byte_cap_all_roles_and_reader_frame_profile_gaps() {
        let (_root, guard) = workspace();
        for role in ROLES {
            let mut raw = document(role);
            raw.resize(MAX_FRAME_BYTES, b' ');
            let io = FaultIo::new("temporary", false);
            let error = publish_with(&guard, role, &raw, mode(role), &io).unwrap_err();
            // Noncanonical padding is forbidden for frames, accepted otherwise.
            assert_eq!(
                error.code(),
                if matches!(role, ArtifactRole::Frame { .. }) {
                    "noncanonical_ledger_frame"
                } else {
                    "input_or_storage_unavailable"
                }
            );
            raw.push(b' ');
            let io = FaultIo::new("success", false);
            assert_eq!(
                publish_with(&guard, role, &raw, mode(role), &io)
                    .unwrap_err()
                    .code(),
                "unsupported_ledger_frame_size"
            );
            assert!(io.events.borrow().is_empty());
        }
        // A payload can encode successfully but exceed reader depth once wrapped.
        let mut nested = Value::Null;
        for _ in 0..values::MAX_DEPTH {
            nested = Value::Array(vec![nested]);
        }
        assert!(nested.canonical().is_ok());
        let raw = format!(
            "{{\"version\":1,\"length\":0,\"sha256\":\"\",\"payload\":{}}}\n",
            String::from_utf8(nested.canonical().unwrap()).unwrap()
        )
        .into_bytes();
        let io = FaultIo::new("success", false);
        assert_eq!(
            publish_with(&guard, ROLES[3], &raw, None, &io)
                .unwrap_err()
                .code(),
            "unsupported_json_depth_profile"
        );
        // Encoding does not count aggregate nodes; parsing the envelope must.
        let many = Value::Array(
            (0..5)
                .map(|_| Value::Array(vec![Value::Null; 60_000]))
                .collect(),
        );
        let raw = envelope(many);
        assert_eq!(
            publish_with(&guard, ROLES[3], &raw, None, &io)
                .unwrap_err()
                .code(),
            "unsupported_json_node_profile"
        );
        assert!(io.events.borrow().is_empty());
        let mut broken = document(ROLES[3]);
        broken[0] = b' ';
        assert!(publish_with(&guard, ROLES[3], &broken, None, &io).is_err());
    }

    #[test]
    fn reader_valid_frame_at_exact_cap_is_published_whole() {
        let (root, guard) = workspace();
        let mut chunks = vec![Value::String("x".repeat(values::MAX_STRING)); 7];
        chunks.push(Value::String("x".repeat(values::MAX_STRING - 4096)));
        let mut task = values::parse(
            br#"{"id":"large","title":"Large","status":"pending"}"#,
            Profile::Authority,
        )
        .unwrap();
        let mut payload = super::super::payload(&document(ROLES[3])).unwrap();
        let mut raw = Vec::new();
        for pass in 0..2 {
            task.set_field("extension", Value::Array(chunks.clone()))
                .unwrap();
            let tasks = Value::Array(vec![task.clone()]);
            payload
                .set_field(
                    "revision",
                    Value::String(super::super::revision(EPOCH, EPOCH, 0, &tasks).unwrap()),
                )
                .unwrap();
            payload.set_field("tasks", tasks).unwrap();
            raw = envelope(payload.clone());
            if pass == 0 {
                let deficit = MAX_FRAME_BYTES.checked_sub(raw.len()).unwrap();
                let Value::String(last) = chunks.last_mut().unwrap() else {
                    unreachable!()
                };
                last.push_str(&"x".repeat(deficit));
                assert!(last.len() <= values::MAX_STRING);
            }
        }
        assert_eq!(raw.len(), MAX_FRAME_BYTES);
        assert_eq!(
            publish(&guard, ROLES[3], &raw, None).unwrap(),
            Publication::Synced
        );
        assert_eq!(
            fs::read(root.path().join(relative(ROLES[3]).unwrap())).unwrap(),
            raw
        );
    }

    #[test]
    fn guard_root_identity_is_checked_before_any_publication_io() {
        let (root, guard) = workspace();
        let other = tempfile::tempdir().unwrap();
        assert_eq!(
            guard.validate_for(other.path()).unwrap_err().code(),
            "invalid_workspace_lock_owner"
        );
        // A noncanonical/replaced root must fail guard validation before even
        // invalid role/size/mode handling, without accessing unrelated files.
        let moved = other.path().join("moved");
        fs::rename(root.path(), &moved).unwrap();
        let io = FaultIo::new("success", false);
        assert!(publish_with(&guard, ROLES[1], &document(ROLES[1]), None, &io).is_err());
        assert_eq!(
            publish_with(
                &guard,
                ArtifactRole::Manifest {
                    epoch: "../invalid"
                },
                b"invalid",
                Some(0o10000),
                &io
            )
            .unwrap_err()
            .code(),
            "input_or_storage_unavailable"
        );
        assert!(io.events.borrow().is_empty());
        assert!(!root.path().exists());
        fs::rename(moved, root.path()).unwrap();
    }
}
