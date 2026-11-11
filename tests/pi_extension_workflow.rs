#![cfg(unix)]
use coven::pi_extension::{NATIVE_PIN, Operation, Request, arguments};
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::Command,
};

const NEW: &str = "coven-agent-reference.ts";
const OLD: &str = "kitsune-agent-state.ts";
const RECEIPT: &str = "coven-pi-extension.json";
const KIT: &[u8] = include_bytes!("fixtures/pi_extension/kitsune-agent-state.ts");
fn request(root: &Path, operation: Operation, apply: bool) -> Request {
    Request {
        extension_root: root.into(),
        executable: Some("/abs/coven's binary".into()),
        socket: Some("/abs/socket path".into()),
        migrate_kitsune: false,
        apply,
        operation,
    }
}
fn succeeds(r: &Request) -> Value {
    serde_json::from_str(&arguments(r, true).unwrap().stdout).unwrap()
}
fn refuses(r: &Request, code: &str) {
    assert_eq!(arguments(r, true).err().unwrap().code(), code);
}

#[test]
fn plans_are_side_effect_free_and_explicit_about_activation_and_state_loss() {
    for legacy in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        if legacy {
            fs::write(dir.path().join(OLD), KIT).unwrap();
        }
        for op in [Operation::Plan, Operation::Install] {
            let mut r = request(dir.path(), op, op == Operation::Plan);
            r.migrate_kitsune = legacy;
            let value = succeeds(&r);
            assert_eq!(value["status"], "planned");
            assert_eq!(value["native_contract"], NATIVE_PIN);
            assert!(value["activation"].as_str().unwrap().contains("not proof"));
            if legacy {
                assert!(
                    value["migration_warning"]
                        .as_str()
                        .unwrap()
                        .contains("working/blocked/idle")
                );
            }
            assert_eq!(
                fs::read_dir(dir.path()).unwrap().count(),
                usize::from(legacy)
            );
        }
        let mut r = request(dir.path(), Operation::Plan, true);
        r.executable = None;
        r.socket = None;
        r.migrate_kitsune = legacy;
        assert_eq!(
            succeeds(&r)["status"],
            "planned_requires_executable_and_socket"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn immutable_original_survives_uninstall_reinstall_and_rollback_after_uninstall() {
    for legacy in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        if legacy {
            fs::write(dir.path().join(OLD), KIT).unwrap();
            fs::set_permissions(dir.path().join(OLD), fs::Permissions::from_mode(0o740)).unwrap();
        }
        fs::write(dir.path().join("custom.ts"), b"unrelated").unwrap();
        let mut r = request(dir.path(), Operation::Install, true);
        r.migrate_kitsune = legacy;
        assert_eq!(succeeds(&r)["status"], "installed");
        let original: Value =
            serde_json::from_slice(&fs::read(dir.path().join(RECEIPT)).unwrap()).unwrap();
        assert_eq!(
            fs::metadata(dir.path().join(NEW))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o600
        );
        assert!(!dir.path().join(OLD).exists());
        let installed_bytes = fs::read(dir.path().join(NEW)).unwrap();
        let receipt_bytes = fs::read(dir.path().join(RECEIPT)).unwrap();
        for operation in [Operation::Uninstall, Operation::Rollback] {
            let preview = request(dir.path(), operation, false);
            assert_eq!(succeeds(&preview)["status"], "planned");
            assert_eq!(fs::read(dir.path().join(NEW)).unwrap(), installed_bytes);
            assert_eq!(fs::read(dir.path().join(RECEIPT)).unwrap(), receipt_bytes);
            assert!(!dir.path().join(OLD).exists());
        }
        for _ in 0..2 {
            r.operation = Operation::Uninstall;
            assert_eq!(succeeds(&r)["status"], "uninstalled");
            assert!(!dir.path().join(NEW).exists() && !dir.path().join(OLD).exists());
            r.operation = Operation::Install;
            r.migrate_kitsune = false;
            succeeds(&r);
            let receipt: Value =
                serde_json::from_slice(&fs::read(dir.path().join(RECEIPT)).unwrap()).unwrap();
            assert_eq!(receipt["original"], original["original"]);
        }
        r.operation = Operation::Uninstall;
        succeeds(&r);
        r.operation = Operation::Rollback;
        succeeds(&r);
        assert!(!dir.path().join(NEW).exists());
        if legacy {
            assert_eq!(fs::read(dir.path().join(OLD)).unwrap(), KIT);
            assert_eq!(
                fs::metadata(dir.path().join(OLD))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o7777,
                0o740
            );
            r.operation = Operation::Install;
            refuses(&r, "pi_extension_migration_consent_required");
            r.migrate_kitsune = true;
            succeeds(&r);
        } else {
            assert!(!dir.path().join(OLD).exists());
        }
        assert_eq!(
            fs::read(dir.path().join("custom.ts")).unwrap(),
            b"unrelated"
        );
    }
}

#[test]
fn fresh_conflicts_never_create_private_artifacts() {
    for scenario in [
        "new",
        "both",
        "old_modified",
        "no_consent",
        "symlink",
        "hardlink",
        "mode",
        "utf8",
        "oversize",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut r = request(dir.path(), Operation::Install, cfg!(target_os = "linux"));
        r.migrate_kitsune = scenario != "no_consent";
        let code = match scenario {
            "new" | "both" => {
                fs::write(dir.path().join(NEW), b"anything, including generated bytes").unwrap();
                if scenario == "both" {
                    fs::write(dir.path().join(OLD), KIT).unwrap();
                }
                "pi_extension_unreceipted_ownership"
            }
            "old_modified" => {
                fs::write(
                    dir.path().join(OLD),
                    b"// KITSUNE_INTEGRATION_VERSION=7\nmodified",
                )
                .unwrap();
                "pi_extension_ambiguous_kitsune"
            }
            "no_consent" => {
                fs::write(dir.path().join(OLD), KIT).unwrap();
                "pi_extension_migration_consent_required"
            }
            "symlink" => {
                symlink("missing", dir.path().join(OLD)).unwrap();
                "pi_extension_unsafe_leaf"
            }
            "hardlink" => {
                fs::write(dir.path().join("other"), KIT).unwrap();
                fs::hard_link(dir.path().join("other"), dir.path().join(OLD)).unwrap();
                "pi_extension_unsafe_leaf"
            }
            "mode" => {
                fs::write(dir.path().join(OLD), KIT).unwrap();
                fs::set_permissions(dir.path().join(OLD), fs::Permissions::from_mode(0o664))
                    .unwrap();
                "pi_extension_unsafe_leaf"
            }
            "utf8" => {
                fs::write(dir.path().join(OLD), [0xff]).unwrap();
                "pi_extension_invalid_utf8"
            }
            _ => {
                fs::write(dir.path().join(OLD), vec![b'x'; 1024 * 1024 + 1]).unwrap();
                "pi_extension_size_limit"
            }
        };
        refuses(&r, code);
        assert!(!dir.path().join(RECEIPT).exists());
        assert!(!dir.path().join(".coven-pi-extension.lock").exists());
        assert!(!dir.path().join(".coven-pi-extension-recovery").exists());
    }
}

#[test]
fn root_must_exist_be_absolute_and_have_no_symlink_components() {
    let dir = tempfile::tempdir().unwrap();
    refuses(
        &request(Path::new("relative"), Operation::Plan, false),
        "pi_extension_absolute_path_required",
    );
    refuses(
        &request(&dir.path().join("missing"), Operation::Plan, false),
        "pi_extension_unsafe_root",
    );
    symlink(dir.path(), dir.path().join("alias")).unwrap();
    refuses(
        &request(&dir.path().join("alias"), Operation::Plan, false),
        "pi_extension_unsafe_root",
    );
}

#[test]
fn registered_cli_plan_is_read_only_and_requires_explicit_root() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_coven"))
        .args([
            "agent",
            "extensions",
            "plan",
            "--provider",
            "pi",
            "--extension-root",
        ])
        .arg(dir.path())
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["provider"], "pi");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    assert!(
        !Command::new(env!("CARGO_BIN_EXE_coven"))
            .args(["agent", "extensions", "plan", "--provider", "pi"])
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn cli_migration_placement_and_rollback_do_not_touch_settings() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(OLD), KIT).unwrap();
    fs::write(dir.path().join("settings.json"), b"private native settings").unwrap();
    for operation in ["install", "uninstall", "rollback"] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_coven"));
        command
            .args([
                "agent",
                "extensions",
                operation,
                "--provider",
                "pi",
                "--extension-root",
            ])
            .arg(dir.path())
            .args(["--apply", "--json"]);
        if operation == "install" {
            command.args([
                "--migrate-kitsune",
                "--executable",
                "/abs/coven",
                "--socket",
                "/abs/socket",
            ]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["provider"], "pi");
        assert_eq!(
            fs::read(dir.path().join("settings.json")).unwrap(),
            b"private native settings"
        );
        assert_eq!(dir.path().join(NEW).exists(), operation == "install");
        assert_eq!(dir.path().join(OLD).exists(), operation == "rollback");
    }
    assert_eq!(fs::read(dir.path().join(OLD)).unwrap(), KIT);
}

#[cfg(all(unix, not(target_os = "linux")))]
#[test]
fn other_unix_is_preview_only() {
    let dir = tempfile::tempdir().unwrap();
    succeeds(&request(dir.path(), Operation::Plan, false));
    refuses(
        &request(dir.path(), Operation::Install, true),
        "pi_extension_apply_unsupported_platform",
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}
