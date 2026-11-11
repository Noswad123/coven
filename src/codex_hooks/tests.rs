use super::*;
use std::{fs, os::unix::fs::PermissionsExt};

fn request(root: &Path, operation: Operation) -> Request {
    Request {
        config_root: root.into(),
        executable: Some("/abs/coven".into()),
        socket: Some("/abs/receiver".into()),
        migrate_kitsune: false,
        apply: true,
        operation,
    }
}
fn seed() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(LEAVES[0]), b"{\"description\":\"private\"}").unwrap();
    fs::write(
        dir.path().join(LEAVES[1]),
        b"# original\n[features]\nhooks = false\n",
    )
    .unwrap();
    dir
}
fn receipt(root: &Path) -> Receipt {
    serde_json::from_slice(&fs::read(root.join(RECEIPT)).unwrap()).unwrap()
}
fn snapshot(root: &Path) -> Vec<(String, Vec<u8>, u32)> {
    let mut files: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_str().unwrap().to_owned(),
                fs::read(entry.path()).unwrap(),
                entry.metadata().unwrap().permissions().mode() & 0o7777,
            )
        })
        .collect();
    files.sort();
    files
}

#[cfg(target_os = "linux")]
#[test]
fn interrupted_each_operation_at_every_boundary_resumes_only_known_prefixes() {
    for operation in [
        Operation::Install,
        Operation::Uninstall,
        Operation::Rollback,
    ] {
        for stop in 0..=2 {
            for populated in [false, true] {
                let dir = if populated {
                    seed()
                } else {
                    tempfile::tempdir().unwrap()
                };
                if operation != Operation::Install {
                    run(&request(dir.path(), Operation::Install), true).unwrap();
                }
                let r = request(dir.path(), operation);
                let error = run_inner(&r, true, &mut |index| {
                    if index == stop {
                        Err(Error("injected_crash"))
                    } else {
                        Ok(())
                    }
                })
                .err()
                .unwrap();
                assert_eq!(error.code(), "injected_crash");
                let journal = receipt(dir.path());
                assert!(journal.pending);
                let root = Root::open(dir.path(), Spec::codex()).unwrap();
                let images = [
                    root.read(LEAVES[0], LIMIT).unwrap(),
                    root.read(LEAVES[1], LIMIT).unwrap(),
                ];
                assert!(prefix(
                    &stored(&images).unwrap(),
                    &journal.source,
                    &journal.target
                ));
                let before = snapshot(dir.path());
                let preview = request(dir.path(), Operation::Plan);
                assert!(
                    run(&preview, true)
                        .unwrap()
                        .stdout
                        .contains("recovery_required")
                );
                assert_eq!(snapshot(dir.path()), before);
                let wrong = if operation == Operation::Install {
                    Operation::Rollback
                } else {
                    Operation::Install
                };
                assert_eq!(
                    run(&request(dir.path(), wrong), true).err().unwrap().code(),
                    "codex_hooks_receipt_stage_conflict"
                );
                assert_eq!(snapshot(dir.path()), before);
                run(&r, true).unwrap();
                assert!(!receipt(dir.path()).pending);
                assert!(run(&r, true).unwrap().stdout.contains("unchanged"));
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn pending_unknown_bytes_modes_and_nonprefixes_never_change_any_leaf() {
    for operation in [
        Operation::Install,
        Operation::Uninstall,
        Operation::Rollback,
    ] {
        for (leaf, name) in LEAVES.iter().enumerate() {
            for change in ["bytes", "mode", "absence"] {
                let dir = seed();
                if operation != Operation::Install {
                    run(&request(dir.path(), Operation::Install), true).unwrap();
                }
                let r = request(dir.path(), operation);
                run_inner(&r, true, &mut |i| {
                    if i == 0 { Err(Error("crash")) } else { Ok(()) }
                })
                .err()
                .unwrap();
                let path = dir.path().join(name);
                if change == "mode" {
                    fs::set_permissions(path, fs::Permissions::from_mode(0o400)).unwrap();
                } else if change == "absence" {
                    fs::remove_file(path).unwrap();
                } else {
                    fs::write(
                        path,
                        if leaf == 0 {
                            b"{\"description\":\"external\"}".as_slice()
                        } else {
                            b"external = true\n"
                        },
                    )
                    .unwrap();
                }
                let before = snapshot(dir.path());
                assert_eq!(run(&r, true).err().unwrap().code(), "codex_hooks_conflict");
                assert_eq!(snapshot(dir.path()), before);
            }
        }
    }
    let dir = seed();
    let r = request(dir.path(), Operation::Install);
    run_inner(&r, true, &mut |i| {
        if i == 0 { Err(Error("crash")) } else { Ok(()) }
    })
    .err()
    .unwrap();
    let journal = receipt(dir.path());
    fs::write(
        dir.path().join(LEAVES[1]),
        journal.target[1].as_ref().unwrap().text.as_bytes(),
    )
    .unwrap();
    fs::set_permissions(
        dir.path().join(LEAVES[1]),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let before = snapshot(dir.path());
    assert_eq!(run(&r, true).err().unwrap().code(), "codex_hooks_conflict");
    assert_eq!(snapshot(dir.path()), before);
}

#[cfg(target_os = "linux")]
#[test]
fn post_write_race_is_not_finalized_or_globally_undone() {
    let dir = seed();
    let r = request(dir.path(), Operation::Install);
    let result = run_inner(&r, true, &mut |i| {
        if i == 2 {
            fs::write(
                dir.path().join(LEAVES[0]),
                b"{\"description\":\"external\"}",
            )
            .unwrap();
        }
        Ok(())
    });
    assert_eq!(result.err().unwrap().code(), "codex_hooks_conflict");
    assert!(receipt(dir.path()).pending);
    assert_eq!(
        fs::read(dir.path().join(LEAVES[0])).unwrap(),
        b"{\"description\":\"external\"}"
    );
    let before = snapshot(dir.path());
    assert!(run(&r, true).is_err());
    assert_eq!(snapshot(dir.path()), before);
}

#[cfg(target_os = "linux")]
#[test]
fn edit_to_second_leaf_after_journaling_fences_before_first_leaf() {
    for operation in [
        Operation::Install,
        Operation::Uninstall,
        Operation::Rollback,
    ] {
        for chmod in [false, true] {
            let dir = seed();
            if operation != Operation::Install {
                run(&request(dir.path(), Operation::Install), true).unwrap();
            }
            let first = fs::read(dir.path().join(LEAVES[0])).unwrap();
            let result = run_inner(&request(dir.path(), operation), true, &mut |index| {
                if index == 0 {
                    if chmod {
                        fs::set_permissions(
                            dir.path().join(LEAVES[1]),
                            fs::Permissions::from_mode(0o400),
                        )
                        .unwrap();
                    } else {
                        fs::write(dir.path().join(LEAVES[1]), b"external = true\n").unwrap();
                    }
                }
                Ok(())
            });
            assert_eq!(result.err().unwrap().code(), "codex_hooks_conflict");
            assert_eq!(fs::read(dir.path().join(LEAVES[0])).unwrap(), first);
            assert!(receipt(dir.path()).pending);
        }
    }
}

#[test]
fn malformed_receipt_cannot_authorize_paths_or_unrelated_targets() {
    for mutation in ["path", "target", "version", "duplicate"] {
        let dir = seed();
        let r = request(dir.path(), Operation::Install);
        if !cfg!(target_os = "linux") {
            return;
        }
        run(&r, true).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join(RECEIPT)).unwrap()).unwrap();
        match mutation {
            "path" => value["path"] = json!("../external"),
            "target" => value["target"][1]["text"] = json!("hooks = false"),
            "version" => value["version"] = json!(2),
            _ => {}
        }
        let bytes = if mutation == "duplicate" {
            b"{\"version\":1,\"version\":1}".to_vec()
        } else {
            serde_json::to_vec(&value).unwrap()
        };
        fs::write(dir.path().join(RECEIPT), bytes).unwrap();
        let before = snapshot(dir.path());
        assert_eq!(
            run(&r, true).err().unwrap().code(),
            "codex_hooks_invalid_receipt"
        );
        assert_eq!(snapshot(dir.path()), before);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn invalid_native_receipt_images_are_refused_before_any_new_lock_or_journal() {
    for field in ["original", "installed", "source", "target"] {
        let dir = seed();
        let r = request(dir.path(), Operation::Install);
        run(&r, true).unwrap();
        fs::remove_file(dir.path().join(LOCK)).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join(RECEIPT)).unwrap()).unwrap();
        let text = r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"true","timeout":"bad"}]}]}}"#;
        value[field][0] = json!({"text":text, "mode":0o600, "digest":digest(text.as_bytes())});
        fs::write(
            dir.path().join(RECEIPT),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let before = snapshot(dir.path());
        assert!(run(&r, true).is_err());
        assert_eq!(snapshot(dir.path()), before);
        assert!(!dir.path().join(LOCK).exists());
    }
}
