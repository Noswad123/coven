use super::super::*;
use crate::hook_files::test_support::tempdir;
use std::{fs, os::unix::fs::PermissionsExt};

fn request(root: &Path, operation: Operation) -> Request {
    Request {
        extension_root: root.into(),
        executable: Some("/abs/coven".into()),
        socket: Some("/abs/socket".into()),
        migrate_kitsune: true,
        apply: true,
        operation,
    }
}
fn seed(legacy: bool) -> tempfile::TempDir {
    let dir = tempdir();
    if legacy {
        fs::write(dir.path().join(POLICY.leaves[1]), POLICY.kit).unwrap();
        fs::set_permissions(
            dir.path().join(POLICY.leaves[1]),
            fs::Permissions::from_mode(0o750),
        )
        .unwrap();
    }
    dir
}
fn receipt(root: &Path) -> Receipt {
    serde_json::from_slice(&fs::read(root.join(POLICY.receipt)).unwrap()).unwrap()
}
fn save(root: &Path, r: &Receipt) {
    fs::write(
        root.join(POLICY.receipt),
        journal_bytes(&POLICY, r).unwrap(),
    )
    .unwrap();
}
fn snapshot(root: &Path) -> Vec<(String, Vec<u8>, u32)> {
    let mut files: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let e = entry.unwrap();
            (
                e.file_name().to_str().unwrap().into(),
                if e.path().is_dir() {
                    vec![]
                } else {
                    fs::read(e.path()).unwrap()
                },
                e.metadata().unwrap().permissions().mode() & 0o7777,
            )
        })
        .collect();
    files.sort();
    files
}
fn crash(r: &Request, stop: usize) {
    assert_eq!(
        run_inner(&POLICY, r, true, &mut |i| if i == stop {
            Err(Error("injected"))
        } else {
            Ok(())
        })
        .err()
        .unwrap()
        .code(),
        "injected"
    );
}
fn set(root: &Path, i: usize, value: &Option<Stored>) {
    let path = root.join(POLICY.leaves[i]);
    if let Some(v) = value {
        fs::write(&path, &v.text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(v.mode)).unwrap();
    } else if path.exists() {
        fs::remove_file(path).unwrap();
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn every_boundary_install_uninstall_rollback_reinstall_resumes() {
    for legacy in [false, true] {
        for scenario in [
            "install",
            "uninstall",
            "rollback",
            "rollback_uninstalled",
            "reinstall",
            "reinstall_rollback",
        ] {
            for stop in 0..=3 {
                let dir = seed(legacy);
                let operation = match scenario {
                    "uninstall" => Operation::Uninstall,
                    "rollback" | "rollback_uninstalled" => Operation::Rollback,
                    _ => Operation::Install,
                };
                if scenario != "install" {
                    run(&POLICY, &request(dir.path(), Operation::Install), true).unwrap();
                }
                if matches!(scenario, "rollback_uninstalled" | "reinstall") {
                    run(&POLICY, &request(dir.path(), Operation::Uninstall), true).unwrap();
                }
                if scenario == "reinstall_rollback" {
                    run(&POLICY, &request(dir.path(), Operation::Rollback), true).unwrap();
                }
                let r = request(dir.path(), operation);
                crash(&r, stop);
                let journal = receipt(dir.path());
                assert_eq!(journal.pending, stop != 3);
                let root = Root::open(dir.path(), POLICY.spec).unwrap();
                let current = stored(
                    &POLICY,
                    &[
                        root.read(POLICY.leaves[0], LIMIT).unwrap(),
                        root.read(POLICY.leaves[1], LIMIT).unwrap(),
                    ],
                )
                .unwrap();
                assert!(prefix(&current, &journal));
                let before = snapshot(dir.path());
                let mut plan = request(dir.path(), Operation::Plan);
                plan.apply = true;
                run(&POLICY, &plan, true).unwrap();
                assert_eq!(before, snapshot(dir.path()));
                if journal.pending {
                    let wrong = if operation == Operation::Install {
                        Operation::Rollback
                    } else {
                        Operation::Install
                    };
                    assert_eq!(
                        run(&POLICY, &request(dir.path(), wrong), true)
                            .err()
                            .unwrap()
                            .code(),
                        POLICY.errors.receipt_stage_conflict
                    );
                    assert_eq!(before, snapshot(dir.path()));
                }
                run(&POLICY, &r, true).unwrap();
                assert!(!receipt(dir.path()).pending);
                assert!(run(&POLICY, &r, true).unwrap().stdout.contains("unchanged"));
                assert_eq!(receipt(dir.path()).original[1].is_some(), legacy);
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn only_operation_order_prefixes_are_legal() {
    for op in [Operation::Install, Operation::Rollback] {
        let dir = seed(true);
        if op == Operation::Rollback {
            run(&POLICY, &request(dir.path(), Operation::Install), true).unwrap();
        }
        let r = request(dir.path(), op);
        crash(&r, 0);
        let journal = receipt(dir.path());
        let second = journal.operation.order()[1];
        set(dir.path(), second, &journal.target[second]);
        let before = snapshot(dir.path());
        assert_eq!(
            run(&POLICY, &r, true).err().unwrap().code(),
            POLICY.errors.conflict
        );
        assert_eq!(before, snapshot(dir.path()));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn races_fence_active_invocation_and_retries_accept_only_legal_prefixes() {
    for stop in 0..=3 {
        for change in ["mode", "bytes", "absence", "receipt"] {
            let dir = seed(true);
            let r = request(dir.path(), Operation::Install);
            let result = run_inner(&POLICY, &r, true, &mut |i| {
                if i == stop {
                    let name = if change == "receipt" {
                        POLICY.receipt
                    } else if stop < 2 {
                        POLICY.leaves[1]
                    } else {
                        POLICY.leaves[0]
                    };
                    let path = dir.path().join(name);
                    match change {
                        "mode" => {
                            fs::set_permissions(path, fs::Permissions::from_mode(0o400)).unwrap()
                        }
                        "absence" => fs::remove_file(path).unwrap(),
                        _ => fs::write(path, b"external edit").unwrap(),
                    }
                }
                Ok(())
            });
            assert!(result.is_err(), "{stop} {change}");
            let before = snapshot(dir.path());
            if stop == 1 && change == "absence" {
                // External legacy deletion after creating the new file is
                // indistinguishable on retry from the legal completed prefix.
                assert_eq!(result.err().unwrap().code(), POLICY.errors.conflict);
                assert!(receipt(dir.path()).pending);
                let new_bytes = fs::read(dir.path().join(POLICY.leaves[0])).unwrap();
                assert!(!dir.path().join(POLICY.leaves[1]).exists());
                assert!(run(&POLICY, &r, true).unwrap().stdout.contains("installed"));
                assert!(!receipt(dir.path()).pending);
                assert_eq!(
                    fs::read(dir.path().join(POLICY.leaves[0])).unwrap(),
                    new_bytes
                );
                assert!(!dir.path().join(POLICY.leaves[1]).exists());
                assert_eq!(
                    receipt(dir.path()).original[1]
                        .as_ref()
                        .unwrap()
                        .text
                        .as_bytes(),
                    POLICY.kit
                );
            } else {
                assert!(run(&POLICY, &r, true).is_err(), "{stop} {change}");
                assert_eq!(before, snapshot(dir.path()), "{stop} {change}");
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn journal_images_rebuilt_not_merely_digest_checked() {
    for field in [
        "original_new",
        "original_legacy",
        "installed",
        "source",
        "target",
        "pin",
        "unknown",
        "duplicate",
    ] {
        let dir = seed(true);
        let r = request(dir.path(), Operation::Install);
        crash(&r, 0);
        let mut journal = receipt(dir.path());
        let invented = Some(Stored::new(&POLICY, b"attacker controlled", 0o600).unwrap());
        match field {
            "original_new" => journal.original[0] = invented,
            "original_legacy" => journal.original[1] = invented,
            "installed" => journal.installed[0] = invented,
            "source" => journal.source[0] = invented,
            "target" => journal.target[0] = invented,
            "pin" => journal.native_pin = "1.5.2".into(),
            _ => (),
        }
        save(dir.path(), &journal);
        if field == "unknown" {
            let mut value = serde_json::to_value(&journal).unwrap();
            value["path"] = json!("/elsewhere");
            fs::write(
                dir.path().join(POLICY.receipt),
                serde_json::to_vec(&value).unwrap(),
            )
            .unwrap();
        }
        if field == "duplicate" {
            let raw = String::from_utf8(journal_bytes(&POLICY, &journal).unwrap()).unwrap();
            fs::write(
                dir.path().join(POLICY.receipt),
                raw.replacen('{', "{\"version\":1,", 1),
            )
            .unwrap();
        }
        fs::remove_file(dir.path().join(POLICY.lock)).unwrap();
        let before = snapshot(dir.path());
        assert_eq!(
            run(&POLICY, &r, true).err().unwrap().code(),
            POLICY.errors.invalid_receipt,
            "{field}"
        );
        assert_eq!(before, snapshot(dir.path()));
        assert!(!dir.path().join(POLICY.lock).exists());
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn recovery_is_retained_and_blocks_even_previews() {
    let dir = seed(false);
    let recovery = dir
        .path()
        .join(format!(".coven-{}-extension-recovery", POLICY.provider));
    fs::create_dir(&recovery).unwrap();
    fs::write(recovery.join("displaced"), b"manual recovery evidence").unwrap();
    for op in [
        Operation::Plan,
        Operation::Install,
        Operation::Uninstall,
        Operation::Rollback,
    ] {
        assert_eq!(
            run(&POLICY, &request(dir.path(), op), true)
                .err()
                .unwrap()
                .code(),
            format!("{}_extension_recovery_required", POLICY.provider)
        );
        assert_eq!(
            fs::read(recovery.join("displaced")).unwrap(),
            b"manual recovery evidence"
        );
        assert!(!dir.path().join(POLICY.lock).exists());
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn migration_coexists_only_in_install_and_reverse_rollback_windows() {
    let dir = seed(true);
    let r = request(dir.path(), Operation::Install);
    crash(&r, 1);
    assert!(
        POLICY
            .leaves
            .iter()
            .all(|name| dir.path().join(name).exists())
    );
    run(&POLICY, &r, true).unwrap();
    assert!(!dir.path().join(POLICY.leaves[1]).exists());
    let rollback = request(dir.path(), Operation::Rollback);
    crash(&rollback, 1);
    assert!(
        POLICY
            .leaves
            .iter()
            .all(|name| dir.path().join(name).exists())
    );
    assert_eq!(
        fs::metadata(dir.path().join(POLICY.leaves[1]))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o750
    );
    run(&POLICY, &rollback, true).unwrap();
    let mut no_consent = r;
    no_consent.migrate_kitsune = false;
    assert_eq!(
        run(&POLICY, &no_consent, true).err().unwrap().code(),
        POLICY.errors.migration_consent_required
    );
}

#[test]
fn fixture_pin_and_license_are_exact() {
    assert_eq!(digest(POLICY.kit), POLICY.kit_sha);
    for license in [
        include_bytes!("../../../tests/fixtures/pi_extension/KITSUNE_LICENSE").as_slice(),
        include_bytes!("../../../tests/fixtures/opencode_extension/KITSUNE_LICENSE").as_slice(),
    ] {
        assert_eq!(
            digest(license),
            "c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4"
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn generated_bytes_without_receipt_are_not_ownership() {
    let dir = seed(false);
    let r = request(dir.path(), Operation::Install);
    run(&POLICY, &r, true).unwrap();
    fs::remove_file(dir.path().join(POLICY.receipt)).unwrap();
    fs::remove_file(dir.path().join(POLICY.lock)).unwrap();
    let before = snapshot(dir.path());
    assert_eq!(
        run(&POLICY, &r, true).err().unwrap().code(),
        POLICY.errors.unreceipted_ownership
    );
    assert_eq!(before, snapshot(dir.path()));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn reprepare_under_lock_rejects_edits_before_any_journal() {
    for stop in [4, 5] {
        let dir = seed(true);
        let r = request(dir.path(), Operation::Install);
        let result = run_inner(&POLICY, &r, true, &mut |i| {
            if i == stop {
                fs::write(dir.path().join(POLICY.leaves[0]), b"external owner").unwrap();
            }
            Ok(())
        });
        assert_eq!(
            result.err().unwrap().code(),
            POLICY.errors.unreceipted_ownership
        );
        assert_eq!(
            fs::read(dir.path().join(POLICY.leaves[0])).unwrap(),
            b"external owner"
        );
        assert!(!dir.path().join(POLICY.receipt).exists());
        assert_eq!(
            fs::read(dir.path().join(POLICY.leaves[1])).unwrap(),
            POLICY.kit
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn every_leaf_and_receipt_is_fenced_at_every_boundary() {
    for operation in [
        Operation::Install,
        Operation::Uninstall,
        Operation::Rollback,
    ] {
        for stop in 0..=3 {
            for name in [POLICY.leaves[0], POLICY.leaves[1], POLICY.receipt] {
                for change in ["bytes", "inode", "mode", "absence"] {
                    let dir = seed(true);
                    if operation != Operation::Install {
                        run(&POLICY, &request(dir.path(), Operation::Install), true).unwrap();
                    }
                    let path = dir.path().join(name);
                    let mut changed = false;
                    let result =
                        run_inner(&POLICY, &request(dir.path(), operation), true, &mut |i| {
                            if i == stop {
                                changed = true;
                                if path.exists() {
                                    match change {
                                        "inode" => {
                                            let bytes = fs::read(&path).unwrap();
                                            let mode = fs::metadata(&path).unwrap().permissions();
                                            // Retain the original inode so reuse cannot hide replacement.
                                            fs::rename(
                                                &path,
                                                dir.path().join("external-displaced"),
                                            )
                                            .unwrap();
                                            fs::write(&path, bytes).unwrap();
                                            fs::set_permissions(&path, mode).unwrap();
                                        }
                                        "mode" => fs::set_permissions(
                                            &path,
                                            fs::Permissions::from_mode(0o400),
                                        )
                                        .unwrap(),
                                        "absence" => fs::remove_file(&path).unwrap(),
                                        _ => fs::write(&path, b"external bytes").unwrap(),
                                    }
                                } else {
                                    fs::write(&path, b"external creation").unwrap();
                                }
                            }
                            Ok(())
                        });
                    assert!(changed);
                    assert!(result.is_err(), "{operation:?} {stop} {name} {change}");
                }
            }
        }
    }
}
