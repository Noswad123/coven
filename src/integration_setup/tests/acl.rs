use super::*;
use std::{ffi::CStr, fs::File, os::fd::AsRawFd};

fn read_acl(file: &File, name: &CStr) -> Vec<u8> {
    let mut bytes = vec![0; 128];
    let len = unsafe {
        libc::fgetxattr(
            file.as_raw_fd(),
            name.as_ptr(),
            bytes.as_mut_ptr().cast(),
            bytes.len(),
        )
    };
    assert!(
        len >= 0,
        "ACL fixture must remain readable: {}",
        std::io::Error::last_os_error()
    );
    bytes.truncate(len as usize);
    bytes
}

fn install_acl(file: &File, name: &CStr) -> Vec<u8> {
    // Named-user access is masked out, so st_mode stays exactly 0700.
    // Default ACLs likewise leave the directory's access mode unchanged.
    let mut acl = 2u32.to_le_bytes().to_vec();
    for (tag, perms, id) in [
        (1u16, 7u16, u32::MAX),
        (2, 7, unsafe { libc::geteuid() } ^ 1),
        (4, 0, u32::MAX),
        (16, 0, u32::MAX),
        (32, 0, u32::MAX),
    ] {
        acl.extend(tag.to_le_bytes());
        acl.extend(perms.to_le_bytes());
        acl.extend(id.to_le_bytes());
    }
    assert_eq!(
        unsafe {
            libc::fsetxattr(
                file.as_raw_fd(),
                name.as_ptr(),
                acl.as_ptr().cast(),
                acl.len(),
                0,
            )
        },
        0,
        "ACL fixture requires Linux POSIX ACL support: {}",
        std::io::Error::last_os_error()
    );
    assert_eq!(file.metadata().unwrap().mode() & 0o7777, 0o700);
    assert_eq!(read_acl(file, name), acl);
    acl
}

fn refuses_acl_without_setup_mutation(name: &CStr, missing_root: bool) {
    for missing_config_parent in [false, true] {
        let mut f = Fixture::new();
        if missing_config_parent {
            f.request.kitsune_config = Some(f.dir.path().join("missing-kit/deep/config.toml"));
        } else {
            f.write(b"# unchanged user config\nx = 1\n", 0o640);
        }
        let state = f.dir.path().join("s");
        fs::create_dir(&state).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        let file = File::open(&state).unwrap();
        let acl = install_acl(&file, name);
        if missing_root {
            f.request.state_root = Some(state.join("missing/deep"));
        }
        let config_before = fs::read(f.config()).ok();
        let config_metadata = fs::metadata(f.config()).ok();
        let state_metadata = file.metadata().unwrap();
        let mut entries_before = fs::read_dir(f.dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>();
        entries_before.sort();
        for (operation, apply) in [
            (Operation::Plan, false),
            (Operation::Plan, true),
            (Operation::Enable, false),
            (Operation::Enable, true),
        ] {
            f.request.operation = operation;
            f.request.apply = apply;
            assert_eq!(f.run().unwrap_err().code(), "integration_acl_denied");
            assert_eq!(fs::read(f.config()).ok(), config_before);
            if let Some(before) = &config_metadata {
                let after = fs::metadata(f.config()).unwrap();
                assert_eq!(
                    (
                        after.ino(),
                        after.mode(),
                        after.mtime(),
                        after.mtime_nsec(),
                        after.ctime(),
                        after.ctime_nsec()
                    ),
                    (
                        before.ino(),
                        before.mode(),
                        before.mtime(),
                        before.mtime_nsec(),
                        before.ctime(),
                        before.ctime_nsec()
                    )
                );
            }
            let parent = f.config().parent().unwrap();
            for leaf in [LOCK, RECEIPT, ".coven-kit-integration-recovery"] {
                assert!(!parent.join(leaf).exists());
            }
            assert!(!f.dir.path().join("missing-kit").exists());
            assert!(!state.join("missing").exists());
            assert_eq!(fs::read_dir(&state).unwrap().count(), 0);
            let mut entries_after = fs::read_dir(f.dir.path())
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect::<Vec<_>>();
            entries_after.sort();
            assert_eq!(entries_after, entries_before);
            assert_eq!(read_acl(&file, name), acl);
            let after = file.metadata().unwrap();
            assert_eq!(
                (
                    after.ino(),
                    after.mode(),
                    after.mtime(),
                    after.mtime_nsec(),
                    after.ctime(),
                    after.ctime_nsec()
                ),
                (
                    state_metadata.ino(),
                    state_metadata.mode(),
                    state_metadata.mtime(),
                    state_metadata.mtime_nsec(),
                    state_metadata.ctime(),
                    state_metadata.ctime_nsec()
                )
            );
        }
        // TempDir teardown removes only this owned fixture, including its ACL.
        // No pre-existing directory's mode or ACL is changed or restored.
    }
}

#[test]
fn masked_access_acl_on_0700_state_root_refuses_plan_and_enable() {
    refuses_acl_without_setup_mutation(c"system.posix_acl_access", false);
}

#[test]
fn default_acl_on_0700_state_root_refuses_plan_and_enable() {
    refuses_acl_without_setup_mutation(c"system.posix_acl_default", false);
}

#[test]
fn missing_state_root_under_default_acl_parent_refuses_before_mkdir() {
    refuses_acl_without_setup_mutation(c"system.posix_acl_default", true);
}

#[test]
fn unsafe_recorded_acl_is_reported_by_plan_but_never_blocks_or_changes_during_undo() {
    for name in [c"system.posix_acl_access", c"system.posix_acl_default"] {
        let mut f = Fixture::new();
        f.write(b"# original config\n", 0o640);
        let state = f.request.state_root.clone().unwrap();
        fs::create_dir(&state).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        f.run().unwrap();
        let receipt = f.receipt();
        let file = File::open(&state).unwrap();
        let acl = install_acl(&file, name);
        let before = file.metadata().unwrap();
        f.request.executable = None;
        f.request.state_root = None;
        f.request.operation = Operation::Plan;
        let output = f.run().unwrap();
        assert_eq!(output["runtime_ready"], false);
        assert_eq!(output["runtime_readiness_error"], "integration_acl_denied");
        assert_eq!(output["state_root"], json!(state));
        f.request.operation = Operation::Enable;
        assert_eq!(f.run().unwrap_err().code(), "integration_acl_denied");
        f.request.operation = Operation::Disable;
        for apply in [false, true] {
            f.request.apply = apply;
            let output = f.run().unwrap();
            assert!(output["runtime_ready"].is_null());
            assert_eq!(output["command"][4], json!(state));
        }
        assert_eq!(
            fs::read(f.config()).unwrap(),
            receipt.preimage.unwrap().bytes
        );
        assert_eq!(read_acl(&file, name), acl);
        let after = file.metadata().unwrap();
        assert_eq!(
            (after.ino(), after.mode(), after.ctime(), after.ctime_nsec()),
            (
                before.ino(),
                before.mode(),
                before.ctime(),
                before.ctime_nsec()
            )
        );
        assert_eq!(fs::read_dir(state).unwrap().count(), 0);
    }
}
