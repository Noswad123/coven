use super::*;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    sync::{Arc, Barrier},
    time::Duration,
};

fn root() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().canonicalize().unwrap();
    (directory, path)
}

fn artifact_names(root: &Path) -> Vec<PathBuf> {
    let mut names = Vec::new();
    let mut directories = vec![PathBuf::new()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(root.join(directory)).unwrap() {
            let entry = entry.unwrap();
            let relative = entry.path().strip_prefix(root).unwrap().to_owned();
            if entry.file_type().unwrap().is_dir() {
                directories.push(relative.clone());
            }
            names.push(relative);
        }
    }
    names.sort();
    names
}

#[test]
fn reservation_failure_matrix_preserves_exact_partials_without_adoption_or_lock_stealing() {
    use std::cell::{Cell, RefCell};
    struct ReserveFault<'a> {
        phase: &'a str,
        root: &'a Path,
        state_inode: Cell<Option<u64>>,
        lock_inode: Cell<Option<u64>>,
        competing: RefCell<Option<File>>,
    }
    impl ReservationIo for ReserveFault<'_> {
        fn open_root(&self, path: &Path) -> Result<File> {
            if self.phase == "root-open" {
                return Err(crate::Error("test_root_open"));
            }
            platform::open_init_directory(path)
        }
        fn create_state(&self, path: &Path) -> std::io::Result<()> {
            if self.phase == "state-create" {
                return Err(std::io::Error::other("state create"));
            }
            fs::create_dir(path)?;
            self.state_inode.set(Some(fs::metadata(path)?.ino()));
            Ok(())
        }
        fn open_state(&self, path: &Path) -> Result<File> {
            if self.phase == "state-open" {
                return Err(crate::Error("test_state_open"));
            }
            platform::open_init_directory(path)
        }
        fn create_lock(&self, path: &Path) -> Result<File> {
            if self.phase == "lock-create" {
                return Err(crate::Error("test_lock_create"));
            }
            let file = platform::open_init_lock(path)?;
            self.lock_inode.set(Some(file.metadata()?.ino()));
            Ok(file)
        }
        fn lock(&self, file: &File) -> std::io::Result<bool> {
            let competing =
                platform::open_workspace_lock(&self.root.join("state/.workspace.lock"), false)
                    .unwrap();
            assert!(platform::try_workspace_lock(&competing)?);
            *self.competing.borrow_mut() = Some(competing);
            if self.phase == "flock-error" {
                return Err(std::io::Error::other("flock"));
            }
            assert_eq!(self.phase, "flock-busy");
            platform::try_workspace_lock(file)
        }
    }
    for phase in [
        "root-open",
        "state-create",
        "state-open",
        "lock-create",
        "flock-error",
        "flock-busy",
    ] {
        let (_directory, root) = root();
        let foreign = root.join("foreign.txt");
        fs::write(&foreign, b"foreign bytes").unwrap();
        let foreign_inode = fs::metadata(&foreign).unwrap().ino();
        let io = ReserveFault {
            phase,
            root: &root,
            state_inode: Cell::new(None),
            lock_inode: Cell::new(None),
            competing: RefCell::new(None),
        };
        let result = reserve_with(&root, &io);
        assert!(result.is_err(), "{phase}");
        assert_eq!(
            result.err().unwrap().code(),
            match phase {
                "root-open" => "test_root_open",
                "state-open" => "test_state_open",
                "lock-create" => "test_lock_create",
                "flock-busy" => "workspace_lock_busy",
                _ => "input_or_storage_unavailable",
            },
            "{phase}"
        );
        let mut expected = vec![PathBuf::from("foreign.txt")];
        if !matches!(phase, "root-open" | "state-create") {
            expected.push("state".into());
            assert!(root.join("state").is_dir());
            assert_eq!(
                fs::metadata(root.join("state")).unwrap().ino(),
                io.state_inode.get().unwrap()
            );
        } else {
            assert!(io.state_inode.get().is_none());
        }
        if matches!(phase, "flock-error" | "flock-busy") {
            expected.push("state/.workspace.lock".into());
            assert_eq!(
                fs::metadata(root.join("state/.workspace.lock"))
                    .unwrap()
                    .ino(),
                io.lock_inode.get().unwrap()
            );
            assert_eq!(fs::read(root.join("state/.workspace.lock")).unwrap(), b"");
            let probe =
                platform::open_workspace_lock(&root.join("state/.workspace.lock"), false).unwrap();
            assert!(
                !platform::try_workspace_lock(&probe).unwrap(),
                "{phase}: competitor still owns lock"
            );
        }
        assert_eq!(artifact_names(&root), expected, "{phase}");
        assert!(!root.join("coven.json").exists());
        assert!(super::super::WorkspaceGuard::acquire(&root, Duration::ZERO).is_err());
        assert!(super::super::WorkspaceGuard::existing(&root, Duration::ZERO).is_err());
        if io.state_inode.get().is_some() {
            assert!(
                reserve(&root).is_err(),
                "{phase}: must not adopt partial state"
            );
        }
        assert_eq!(
            artifact_names(&root),
            expected,
            "{phase}: refusals must not create artifacts"
        );
        assert_eq!(fs::metadata(&foreign).unwrap().ino(), foreign_inode);
        assert_eq!(fs::read(&foreign).unwrap(), b"foreign bytes");
        drop(io.competing.borrow_mut().take());
        if let Some(inode) = io.lock_inode.get() {
            let probe =
                platform::open_workspace_lock(&root.join("state/.workspace.lock"), false).unwrap();
            assert!(platform::try_workspace_lock(&probe).unwrap());
            assert_eq!(probe.metadata().unwrap().ino(), inode);
            assert_eq!(fs::read(root.join("state/.workspace.lock")).unwrap(), b"");
        }
        if let Some(inode) = io.state_inode.get() {
            assert_eq!(fs::metadata(root.join("state")).unwrap().ino(), inode);
        }
    }
}

#[test]
fn cleanup_failure_retains_only_owned_inert_temp_without_changing_publication_result() {
    use std::cell::{Cell, RefCell};
    struct CleanupFault<'a> {
        phase: &'a str,
        owned: RefCell<Option<(PathBuf, u64)>>,
        cleanup_called: Cell<bool>,
    }
    impl MarkerIo for CleanupFault<'_> {
        fn temporary(&self, path: &Path) -> Result<File> {
            let file = platform::open_generated(path, true)?;
            *self.owned.borrow_mut() = Some((path.to_owned(), file.metadata()?.ino()));
            Ok(file)
        }
        fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
            if self.phase == "write" {
                file.write_all(b"inert prefix")?;
                return Err(std::io::Error::other("write failed"));
            }
            file.write_all(bytes)
        }
        fn sync_root(&self, root: &File) -> std::io::Result<()> {
            if self.phase == "root-sync" {
                return Err(std::io::Error::other("sync failed"));
            }
            root.sync_all()
        }
        fn cleanup(&self, path: &Path) -> std::io::Result<()> {
            assert_eq!(path, self.owned.borrow().as_ref().unwrap().0);
            assert!(!self.cleanup_called.replace(true));
            Err(std::io::Error::other("unlink failed"))
        }
    }
    for phase in ["success", "root-sync", "write"] {
        let (_directory, root) = root();
        let reservation = reserve(&root).unwrap();
        let foreign = root.join(".coven-init-foreign.tmp");
        fs::write(&foreign, b"foreign bytes").unwrap();
        let foreign_inode = fs::metadata(&foreign).unwrap().ino();
        let io = CleanupFault {
            phase,
            owned: RefCell::new(None),
            cleanup_called: Cell::new(false),
        };
        let result = reservation.publish_with(b"complete bytes", &io);
        assert!(io.cleanup_called.get());
        match phase {
            "success" => assert_eq!(result.unwrap(), Delivery::Committed),
            "root-sync" => assert_eq!(result.unwrap(), Delivery::DurabilityUncertain),
            _ => assert_eq!(result.err().unwrap().code(), "input_or_storage_unavailable"),
        }
        let (temporary, inode) = io.owned.borrow().clone().unwrap();
        assert_eq!(fs::metadata(&temporary).unwrap().ino(), inode);
        assert!(
            temporary
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with(".coven-init-")
        );
        assert_eq!(temporary.extension().unwrap(), "tmp");
        assert_eq!(
            fs::read(&temporary).unwrap(),
            if phase == "write" {
                &b"inert prefix"[..]
            } else {
                &b"complete bytes"[..]
            }
        );
        let mut expected = vec![
            temporary.strip_prefix(&root).unwrap().to_owned(),
            ".coven-init-foreign.tmp".into(),
            "state".into(),
            "state/.workspace.lock".into(),
        ];
        if phase == "write" {
            assert!(!root.join("coven.json").exists());
            assert_eq!(fs::metadata(&temporary).unwrap().nlink(), 1);
        } else {
            expected.push("coven.json".into());
            assert_eq!(fs::metadata(root.join("coven.json")).unwrap().ino(), inode);
            assert_eq!(fs::metadata(&temporary).unwrap().nlink(), 2);
            assert_eq!(
                fs::read(root.join("coven.json")).unwrap(),
                b"complete bytes"
            );
        }
        expected.sort();
        assert_eq!(artifact_names(&root), expected);
        assert_eq!(fs::metadata(&foreign).unwrap().ino(), foreign_inode);
        assert_eq!(fs::read(&foreign).unwrap(), b"foreign bytes");
        reservation.validate().unwrap();
        assert!(reserve(&root).is_err());
        assert_eq!(artifact_names(&root), expected);
    }
}

#[test]
fn exclusive_reservation_marker_last_and_original_guard_admission() {
    let (_directory, root) = root();
    let reservation = reserve(&root).unwrap();
    assert!(reserve(&root).is_err());
    assert!(!root.join("coven.json").exists());
    assert!(super::super::WorkspaceGuard::acquire(&root, Duration::ZERO).is_err());
    assert!(super::super::WorkspaceGuard::existing(&root, Duration::ZERO).is_err());
    assert_eq!(
        reservation.publish_marker(b"{}\n").unwrap(),
        Delivery::Committed
    );
    assert_eq!(fs::read(root.join("coven.json")).unwrap(), b"{}\n");
    assert!(super::super::WorkspaceGuard::existing(&root, Duration::ZERO).is_err());
    drop(reservation);
    assert!(super::super::WorkspaceGuard::existing(&root, Duration::ZERO).is_ok());
    assert!(reserve(&root).is_err());
    let names: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 2);
}

#[test]
fn native_contenders_have_exactly_one_winner() {
    let (_directory, root) = root();
    let barrier = Arc::new(Barrier::new(8));
    let contenders: Vec<_> = (0..8)
        .map(|_| {
            let root = root.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                let reservation = reserve(&root);
                let won = reservation.is_ok();
                barrier.wait();
                won
            })
        })
        .collect();
    assert_eq!(
        contenders
            .into_iter()
            .map(|join| usize::from(join.join().unwrap()))
            .sum::<usize>(),
        1
    );
    assert!(!root.join("coven.json").exists());
}

#[test]
fn refuses_existing_state_of_every_kind_without_adoption() {
    for kind in ["directory", "file", "symlink", "dangling"] {
        let (_directory, root) = root();
        let state = root.join("state");
        match kind {
            "directory" => fs::create_dir(&state).unwrap(),
            "file" => fs::write(&state, b"untouched").unwrap(),
            "symlink" => symlink(&root, &state).unwrap(),
            _ => symlink(root.join("missing"), &state).unwrap(),
        }
        assert!(reserve(&root).is_err(), "{kind}");
        assert!(fs::symlink_metadata(state).is_ok());
        assert!(!root.join("coven.json").exists());
    }
}

#[test]
fn owner_pid_thread_and_root_state_lock_identities_are_bound() {
    let (_directory, root) = root();
    let mut reservation = reserve(&root).unwrap();
    let owner = reservation.owner;
    reservation.owner = owner.wrapping_add(1);
    assert!(reservation.validate().is_err());
    reservation.owner = owner;
    reservation.thread = thread::spawn(|| thread::current().id()).join().unwrap();
    assert!(reservation.validate().is_err());
    reservation.thread = thread::current().id();
    reservation.validate().unwrap();
    fs::rename(
        root.join("state/.workspace.lock"),
        root.join("state/old.lock"),
    )
    .unwrap();
    fs::write(root.join("state/.workspace.lock"), b"").unwrap();
    assert!(reservation.validate().is_err());
    fs::remove_file(root.join("state/.workspace.lock")).unwrap();
    fs::rename(
        root.join("state/old.lock"),
        root.join("state/.workspace.lock"),
    )
    .unwrap();
    reservation.validate().unwrap();
    fs::hard_link(root.join("state/.workspace.lock"), root.join("extra-lock")).unwrap();
    assert!(reservation.validate().is_err());
    fs::remove_file(root.join("extra-lock")).unwrap();
    fs::rename(root.join("state"), root.join("old-state")).unwrap();
    fs::create_dir(root.join("state")).unwrap();
    assert!(reservation.validate().is_err());
    assert!(reservation.publish_marker(b"{}").is_err());
    fs::remove_dir(root.join("state")).unwrap();
    fs::rename(root.join("old-state"), root.join("state")).unwrap();
    reservation.validate().unwrap();
    // A different root inode at the same canonical pathname is not the owner.
    let moved = root.with_extension("moved");
    fs::rename(&root, &moved).unwrap();
    fs::create_dir(&root).unwrap();
    assert!(reservation.validate().is_err());
    fs::remove_dir(&root).unwrap();
    fs::rename(&moved, &root).unwrap();
}

#[test]
fn marker_collisions_including_dangling_links_are_never_replaced() {
    for kind in ["file", "directory", "symlink"] {
        let (_directory, root) = root();
        let reservation = reserve(&root).unwrap();
        let marker = root.join("coven.json");
        match kind {
            "file" => fs::write(&marker, b"original").unwrap(),
            "directory" => fs::create_dir(&marker).unwrap(),
            _ => symlink(root.join("absent"), &marker).unwrap(),
        }
        let inode = fs::symlink_metadata(&marker).unwrap().ino();
        assert!(reservation.publish_marker(b"replacement").is_err());
        assert_eq!(fs::symlink_metadata(&marker).unwrap().ino(), inode);
        if kind == "file" {
            assert_eq!(fs::read(marker).unwrap(), b"original");
        }
    }
}

struct FaultIo<'a> {
    phase: &'a str,
    root: &'a Path,
}
impl MarkerIo for FaultIo<'_> {
    fn temporary(&self, path: &Path) -> Result<File> {
        if self.phase == "open" {
            return Err(crate::Error("input_or_storage_unavailable"));
        }
        platform::open_generated(path, true)
    }
    fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
        assert!(!self.root.join("coven.json").exists());
        if self.phase == "write" {
            file.write_all(b"partial")?;
            return Err(std::io::Error::other("write"));
        }
        file.write_all(bytes)
    }
    fn sync_file(&self, file: &File) -> std::io::Result<()> {
        assert!(!self.root.join("coven.json").exists());
        if self.phase == "file-sync" {
            return Err(std::io::Error::other("sync"));
        }
        file.sync_all()
    }
    fn link(&self, source: &Path, target: &Path) -> std::io::Result<()> {
        assert!(!target.exists());
        if self.phase == "collision" {
            fs::write(target, b"other initializer")?;
        }
        if self.phase == "link" {
            return Err(std::io::Error::other("link"));
        }
        fs::hard_link(source, target)
    }
    fn sync_root(&self, root: &File) -> std::io::Result<()> {
        assert_eq!(fs::read(self.root.join("coven.json"))?, b"complete");
        if self.phase == "root-sync" {
            return Err(std::io::Error::other("sync"));
        }
        root.sync_all()
    }
}

#[test]
fn publication_fault_matrix_preserves_known_link_and_only_cleans_owned_temp() {
    for phase in [
        "open",
        "write",
        "file-sync",
        "link",
        "collision",
        "root-sync",
        "success",
    ] {
        let (_directory, root) = root();
        let reservation = reserve(&root).unwrap();
        let unrelated = root.join(".coven-init-unowned.tmp");
        fs::write(&unrelated, b"preserved").unwrap();
        let result = reservation.publish_with(b"complete", &FaultIo { phase, root: &root });
        let failed = result.is_err();
        match phase {
            "success" => assert_eq!(result.unwrap(), Delivery::Committed),
            "root-sync" => assert_eq!(result.unwrap(), Delivery::DurabilityUncertain),
            _ => assert!(result.is_err()),
        }
        if phase == "collision" {
            assert_eq!(
                fs::read(root.join("coven.json")).unwrap(),
                b"other initializer"
            );
        } else if failed {
            assert!(!root.join("coven.json").exists());
        }
        assert_eq!(fs::read(unrelated).unwrap(), b"preserved");
        assert_eq!(
            fs::read_dir(&root).unwrap().count(),
            if root.join("coven.json").exists() {
                3
            } else {
                2
            }
        );
        reservation.validate().unwrap();
    }
}

#[test]
fn late_state_and_temporary_replacements_refuse_publication_and_preserve_foreign_names() {
    struct ReplaceIo<'a> {
        root: &'a Path,
        state: bool,
    }
    impl MarkerIo for ReplaceIo<'_> {
        fn sync_file(&self, file: &File) -> std::io::Result<()> {
            file.sync_all()?;
            if self.state {
                fs::rename(self.root.join("state"), self.root.join("owned-state"))?;
                fs::create_dir(self.root.join("state"))?;
            } else {
                let temporary = fs::read_dir(self.root)?
                    .map(|entry| entry.unwrap().path())
                    .find(|path| path.extension().is_some_and(|extension| extension == "tmp"))
                    .unwrap();
                fs::rename(&temporary, self.root.join("moved-owned-temp"))?;
                fs::write(temporary, b"foreign replacement")?;
            }
            Ok(())
        }
    }
    for state in [true, false] {
        let (_directory, root) = root();
        let reservation = reserve(&root).unwrap();
        assert!(
            reservation
                .publish_with(b"complete", &ReplaceIo { root: &root, state })
                .is_err()
        );
        assert!(!root.join("coven.json").exists());
        if state {
            assert!(root.join("state").is_dir());
            assert!(root.join("owned-state/.workspace.lock").is_file());
            assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        } else {
            let temporary = fs::read_dir(&root)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| path.extension().is_some_and(|extension| extension == "tmp"))
                .unwrap();
            assert_eq!(fs::read(temporary).unwrap(), b"foreign replacement");
            assert_eq!(
                fs::read(root.join("moved-owned-temp")).unwrap(),
                b"complete"
            );
        }
    }
}

#[test]
fn reservation_is_statically_neither_send_nor_sync() {
    // Type inference becomes ambiguous if Reservation ever implements the trait.
    trait AmbiguousIfSend<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    struct SendMarker;
    impl<T: ?Sized + Send> AmbiguousIfSend<SendMarker> for T {}
    let _ = <Reservation as AmbiguousIfSend<_>>::check;
    trait AmbiguousIfSync<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfSync<()> for T {}
    struct SyncMarker;
    impl<T: ?Sized + Sync> AmbiguousIfSync<SyncMarker> for T {}
    let _ = <Reservation as AmbiguousIfSync<_>>::check;
}

#[test]
fn noncanonical_and_missing_roots_and_oversized_markers_have_no_new_effects() {
    let (_directory, root) = root();
    let alias = root.join("alias");
    symlink(&root, &alias).unwrap();
    assert!(reserve(&alias).is_err());
    assert!(reserve(&root.join("missing")).is_err());
    assert!(!root.join("state").exists());
    let reservation = reserve(&root).unwrap();
    assert!(
        reservation
            .publish_marker(&vec![b'x'; 8 * 1_048_576 + 1])
            .is_err()
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
    assert!(!root.join("coven.json").exists());
}

#[test]
fn init_modes_umask_and_sgid_in_controlled_child() {
    const CHILD: &str = "COVEN_BOOTSTRAP_MODE_TEST";
    if let Some(mask) = std::env::var_os(CHILD) {
        let mask = u32::from_str_radix(mask.to_str().unwrap(), 8).unwrap();
        // Isolated test process: never change the parallel test runner's umask.
        unsafe {
            libc::umask(mask);
        }
        let (_directory, root) = root();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o2777)).unwrap();
        let parent = fs::metadata(&root).unwrap();
        let reservation = reserve(&root).unwrap();
        let state = fs::metadata(root.join("state")).unwrap();
        assert_eq!(state.mode() & 0o777, 0o777 & !mask);
        if parent.mode() & 0o2000 != 0 {
            assert_eq!(state.mode() & 0o2000, 0o2000);
            assert_eq!(state.gid(), parent.gid());
        }
        assert_eq!(
            fs::metadata(root.join("state/.workspace.lock"))
                .unwrap()
                .mode()
                & 0o777,
            0o600 & !mask
        );
        reservation.publish_marker(b"{}").unwrap();
        assert_eq!(
            fs::metadata(root.join("coven.json")).unwrap().mode() & 0o777,
            0o666 & !mask
        );
        return;
    }
    for mask in ["022", "077", "002"] {
        assert!(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "storage::bootstrap::tests::init_modes_umask_and_sgid_in_controlled_child",
                    "--nocapture"
                ])
                .env(CHILD, mask)
                .status()
                .unwrap()
                .success()
        );
    }
}
