use super::*;
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
    let dir = tempfile::tempdir().unwrap();
    if legacy {
        fs::write(dir.path().join(LEAVES[1]), KIT).unwrap();
        fs::set_permissions(
            dir.path().join(LEAVES[1]),
            fs::Permissions::from_mode(0o750),
        )
        .unwrap();
    }
    dir
}
fn receipt(root: &Path) -> Receipt {
    serde_json::from_slice(&fs::read(root.join(RECEIPT)).unwrap()).unwrap()
}
fn save(root: &Path, r: &Receipt) {
    fs::write(root.join(RECEIPT), journal_bytes(r).unwrap()).unwrap();
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
        run_inner(r, true, &mut |i| if i == stop {
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
    let path = root.join(LEAVES[i]);
    if let Some(v) = value {
        fs::write(&path, &v.text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(v.mode)).unwrap();
    } else if path.exists() {
        fs::remove_file(path).unwrap();
    }
}

#[cfg(target_os = "linux")]
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
                    run(&request(dir.path(), Operation::Install), true).unwrap();
                }
                if matches!(scenario, "rollback_uninstalled" | "reinstall") {
                    run(&request(dir.path(), Operation::Uninstall), true).unwrap();
                }
                if scenario == "reinstall_rollback" {
                    run(&request(dir.path(), Operation::Rollback), true).unwrap();
                }
                let r = request(dir.path(), operation);
                crash(&r, stop);
                let journal = receipt(dir.path());
                assert_eq!(journal.pending, stop != 3);
                let root = Root::open(dir.path(), Spec::pi()).unwrap();
                let current = stored(&[
                    root.read(LEAVES[0], LIMIT).unwrap(),
                    root.read(LEAVES[1], LIMIT).unwrap(),
                ])
                .unwrap();
                assert!(prefix(&current, &journal));
                let before = snapshot(dir.path());
                let mut plan = request(dir.path(), Operation::Plan);
                plan.apply = true;
                run(&plan, true).unwrap();
                assert_eq!(before, snapshot(dir.path()));
                if journal.pending {
                    let wrong = if operation == Operation::Install {
                        Operation::Rollback
                    } else {
                        Operation::Install
                    };
                    assert_eq!(
                        run(&request(dir.path(), wrong), true).err().unwrap().code(),
                        "pi_extension_receipt_stage_conflict"
                    );
                    assert_eq!(before, snapshot(dir.path()));
                }
                run(&r, true).unwrap();
                assert!(!receipt(dir.path()).pending);
                assert!(run(&r, true).unwrap().stdout.contains("unchanged"));
                assert_eq!(receipt(dir.path()).original[1].is_some(), legacy);
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn only_operation_order_prefixes_are_legal() {
    for op in [Operation::Install, Operation::Rollback] {
        let dir = seed(true);
        if op == Operation::Rollback {
            run(&request(dir.path(), Operation::Install), true).unwrap();
        }
        let r = request(dir.path(), op);
        crash(&r, 0);
        let journal = receipt(dir.path());
        let second = journal.operation.order()[1];
        set(dir.path(), second, &journal.target[second]);
        let before = snapshot(dir.path());
        assert_eq!(run(&r, true).err().unwrap().code(), "pi_extension_conflict");
        assert_eq!(before, snapshot(dir.path()));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn races_fence_active_invocation_and_retries_accept_only_legal_prefixes() {
    for stop in 0..=3 {
        for change in ["mode", "bytes", "absence", "receipt"] {
            let dir = seed(true);
            let r = request(dir.path(), Operation::Install);
            let result = run_inner(&r, true, &mut |i| {
                if i == stop {
                    let name = if change == "receipt" {
                        RECEIPT
                    } else if stop < 2 {
                        LEAVES[1]
                    } else {
                        LEAVES[0]
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
                assert_eq!(result.err().unwrap().code(), "pi_extension_conflict");
                assert!(receipt(dir.path()).pending);
                let new_bytes = fs::read(dir.path().join(LEAVES[0])).unwrap();
                assert!(!dir.path().join(LEAVES[1]).exists());
                assert!(run(&r, true).unwrap().stdout.contains("installed"));
                assert!(!receipt(dir.path()).pending);
                assert_eq!(fs::read(dir.path().join(LEAVES[0])).unwrap(), new_bytes);
                assert!(!dir.path().join(LEAVES[1]).exists());
                assert_eq!(
                    receipt(dir.path()).original[1]
                        .as_ref()
                        .unwrap()
                        .text
                        .as_bytes(),
                    KIT
                );
            } else {
                assert!(run(&r, true).is_err(), "{stop} {change}");
                assert_eq!(before, snapshot(dir.path()), "{stop} {change}");
            }
        }
    }
}

#[cfg(target_os = "linux")]
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
        let invented = Some(Stored::new(b"attacker controlled", 0o600).unwrap());
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
                dir.path().join(RECEIPT),
                serde_json::to_vec(&value).unwrap(),
            )
            .unwrap();
        }
        if field == "duplicate" {
            let raw = String::from_utf8(journal_bytes(&journal).unwrap()).unwrap();
            fs::write(
                dir.path().join(RECEIPT),
                raw.replacen('{', "{\"version\":1,", 1),
            )
            .unwrap();
        }
        fs::remove_file(dir.path().join(LOCK)).unwrap();
        let before = snapshot(dir.path());
        assert_eq!(
            run(&r, true).err().unwrap().code(),
            "pi_extension_invalid_receipt",
            "{field}"
        );
        assert_eq!(before, snapshot(dir.path()));
        assert!(!dir.path().join(LOCK).exists());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn recovery_is_retained_and_blocks_even_previews() {
    let dir = seed(false);
    let recovery = dir.path().join(".coven-pi-extension-recovery");
    fs::create_dir(&recovery).unwrap();
    fs::write(recovery.join("displaced"), b"manual recovery evidence").unwrap();
    for op in [
        Operation::Plan,
        Operation::Install,
        Operation::Uninstall,
        Operation::Rollback,
    ] {
        assert_eq!(
            run(&request(dir.path(), op), true).err().unwrap().code(),
            "pi_extension_recovery_required"
        );
        assert_eq!(
            fs::read(recovery.join("displaced")).unwrap(),
            b"manual recovery evidence"
        );
        assert!(!dir.path().join(LOCK).exists());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn migration_coexists_only_in_install_and_reverse_rollback_windows() {
    let dir = seed(true);
    let r = request(dir.path(), Operation::Install);
    crash(&r, 1);
    assert!(LEAVES.iter().all(|name| dir.path().join(name).exists()));
    run(&r, true).unwrap();
    assert!(!dir.path().join(LEAVES[1]).exists());
    let rollback = request(dir.path(), Operation::Rollback);
    crash(&rollback, 1);
    assert!(LEAVES.iter().all(|name| dir.path().join(name).exists()));
    assert_eq!(
        fs::metadata(dir.path().join(LEAVES[1]))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o750
    );
    run(&rollback, true).unwrap();
    let mut no_consent = r;
    no_consent.migrate_kitsune = false;
    assert_eq!(
        run(&no_consent, true).err().unwrap().code(),
        "pi_extension_migration_consent_required"
    );
}

#[test]
fn fixture_pin_and_license_are_exact() {
    assert_eq!(digest(KIT), KIT_SHA);
    let license = include_str!("../../tests/fixtures/pi_extension/KITSUNE_LICENSE");
    assert_eq!(
        digest(license.as_bytes()),
        "c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn generated_bytes_without_receipt_are_not_ownership() {
    let dir = seed(false);
    let r = request(dir.path(), Operation::Install);
    run(&r, true).unwrap();
    fs::remove_file(dir.path().join(RECEIPT)).unwrap();
    fs::remove_file(dir.path().join(LOCK)).unwrap();
    let before = snapshot(dir.path());
    assert_eq!(
        run(&r, true).err().unwrap().code(),
        "pi_extension_unreceipted_ownership"
    );
    assert_eq!(before, snapshot(dir.path()));
}
