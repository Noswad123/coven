#![cfg(unix)]
#[path = "support/physical_temp.rs"]
mod physical_temp;
use coven::native_extensions::{Operation, Request, arguments};
use serde_json::Value;
use std::{fs, process::Command};

fn request(root: &std::path::Path, operation: Operation, apply: bool) -> Request {
    Request {
        extension_root: root.into(),
        executable: Some("/absolute/coven \"${literal}".into()),
        socket: Some("/absolute/socket ' literal".into()),
        migrate_kitsune: false,
        apply,
        operation,
    }
}

#[test]
fn independent_receipts_preview_uninstall_rollback_and_conflicts() {
    for (provider, ext, kit) in [
        (
            "pi",
            "ts",
            &include_bytes!("fixtures/pi_extension/kitsune-agent-state.ts")[..],
        ),
        (
            "opencode",
            "js",
            &include_bytes!("fixtures/opencode_extension/kitsune-agent-state.js")[..],
        ),
    ] {
        let root = physical_temp::tempdir();
        let old = root.path().join(format!("kitsune-agent-state.{ext}"));
        let reference = root.path().join(format!("coven-agent-reference.{ext}"));
        let native = root.path().join(format!("coven-native-events.{ext}"));
        let receipt = root.path().join(format!("coven-{provider}-native.json"));
        fs::write(&reference, b"independent unknown reference").unwrap();
        let mut r = request(root.path(), Operation::Plan, true);
        let v: Value =
            serde_json::from_str(&arguments(provider, &r, true).unwrap().stdout).unwrap();
        assert_eq!(v["status"], "planned");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        fs::write(&old, kit).unwrap();
        r.operation = Operation::Install;
        assert_eq!(
            arguments(provider, &r, true).err().unwrap().code(),
            format!("{provider}_native_migration_consent_required")
        );
        r.migrate_kitsune = true;
        arguments(provider, &r, true).unwrap();
        assert!(native.exists() && receipt.exists() && !old.exists());
        assert!(
            !root
                .path()
                .join(format!("coven-{provider}-extension.json"))
                .exists()
        );
        let bytes = fs::read(&native).unwrap();
        fs::write(&native, b"local edit").unwrap();
        r.operation = Operation::Uninstall;
        assert!(arguments(provider, &r, true).is_err());
        fs::write(&native, bytes).unwrap();
        arguments(provider, &r, true).unwrap();
        assert!(!native.exists() && !old.exists());
        r.operation = Operation::Rollback;
        arguments(provider, &r, true).unwrap();
        assert_eq!(fs::read(&old).unwrap(), kit);
        assert_eq!(
            fs::read(&reference).unwrap(),
            b"independent unknown reference"
        );
    }
}

#[test]
fn trusted_roots_fixed_leaves_and_native_recovery_namespace() {
    use std::os::unix::fs::symlink;
    for (provider, ext) in [("pi", "ts"), ("opencode", "js")] {
        let root = physical_temp::tempdir();
        let mut r = request(root.path(), Operation::Install, true);
        let leaf = root.path().join(format!("coven-native-events.{ext}"));
        fs::write(&leaf, b"unreceipted").unwrap();
        assert!(arguments(provider, &r, true).is_err());
        fs::remove_file(&leaf).unwrap();
        symlink("/not-followed", &leaf).unwrap();
        assert!(arguments(provider, &r, true).is_err());
        fs::remove_file(&leaf).unwrap();
        let recovery = root
            .path()
            .join(format!(".coven-{provider}-native-recovery"));
        fs::create_dir(&recovery).unwrap();
        assert_eq!(
            arguments(provider, &r, true).err().unwrap().code(),
            format!("{provider}_native_recovery_required")
        );
        fs::remove_dir(&recovery).unwrap();
        // Historical recovery directories are outside the native policy.
        fs::create_dir(
            root.path()
                .join(format!(".coven-{provider}-extension-recovery")),
        )
        .unwrap();
        arguments(provider, &r, true).unwrap();
        r.extension_root = root.path().join("missing");
        assert!(arguments(provider, &r, true).is_err());
        assert!(!r.extension_root.exists());
    }
}

#[test]
fn existing_reference_asset_and_receipt_remain_byte_identical() {
    for (provider, ext) in [("pi", "ts"), ("opencode", "js")] {
        let root = physical_temp::tempdir();
        let r = request(root.path(), Operation::Install, true);
        match provider {
            "pi" => coven::pi_extension::arguments(&r, true).unwrap(),
            _ => coven::opencode_extension::arguments(&r, true).unwrap(),
        };
        let reference = root.path().join(format!("coven-agent-reference.{ext}"));
        let receipt = root.path().join(format!("coven-{provider}-extension.json"));
        let before = (fs::read(&reference).unwrap(), fs::read(&receipt).unwrap());
        arguments(provider, &r, true).unwrap();
        assert_eq!(
            before,
            (fs::read(&reference).unwrap(), fs::read(&receipt).unwrap())
        );
    }
}

#[test]
fn generated_assets_inert_callbacks_and_cli_routes() {
    let root = physical_temp::tempdir();
    for extra in [
        vec![],
        vec!["--socket", "/socket", "--state-root", "/state"],
        vec!["--socket", "relative"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_coven"))
            .args([
                "agent",
                "native-extensions",
                "plan",
                "--provider",
                "pi",
                "--extension-root",
            ])
            .arg(root.path())
            .args(extra)
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
    for provider in ["pi", "opencode"] {
        let dir = root.path().join(provider);
        fs::create_dir(&dir).unwrap();
        arguments(provider, &request(&dir, Operation::Install, true), true).unwrap();
    }
    let status = Command::new("node")
        .arg("--test")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/native_extension_asset.test.mjs"
        ))
        .env("COVEN_NATIVE_ASSETS", root.path())
        .status()
        .unwrap();
    assert!(status.success());
    for op in ["plan", "install", "uninstall", "rollback"] {
        let dir = root.path().join(op);
        fs::create_dir(&dir).unwrap();
        let installed = Command::new(env!("CARGO_BIN_EXE_coven"))
            .args([
                "agent",
                "native-extensions",
                "install",
                "--provider",
                "pi",
                "--extension-root",
            ])
            .arg(&dir)
            .args([
                "--from-kitsune",
                "--state-root",
                "/absolute/state",
                "--apply",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(
            installed.status.success(),
            "{}",
            String::from_utf8_lossy(&installed.stderr)
        );
        let out = Command::new(env!("CARGO_BIN_EXE_coven"))
            .args([
                "agent",
                "native-extensions",
                op,
                "--provider",
                "pi",
                "--extension-root",
            ])
            .arg(&dir)
            .args([
                "--from-kitsune",
                "--state-root",
                "/absolute/state",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let asset = fs::read_to_string(dir.join("coven-native-events.ts")).unwrap();
        assert!(asset.contains("[\"--from-kitsune\",\"--state-root\",\"/absolute/state\"]"));
    }
}

#[test]
fn generated_modules_report_through_actual_cli_and_normal_receiver() {
    use std::os::unix::fs::PermissionsExt;
    let root = physical_temp::tempdir();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.path().join("coven.json"), "{}").unwrap();
    for (provider, ext) in [("pi", "ts"), ("opencode", "js")] {
        let dir = root.path().join(provider);
        fs::create_dir(&dir).unwrap();
        let mut r = request(&dir, Operation::Install, true);
        r.executable = Some(env!("CARGO_BIN_EXE_coven").into());
        r.socket = Some(root.path().join("r.sock").to_str().unwrap().into());
        arguments(provider, &r, true).unwrap();
        fs::copy(
            dir.join(format!("coven-native-events.{ext}")),
            root.path().join(format!("{provider}.mjs")),
        )
        .unwrap();
    }
    let status = Command::new("node")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/native_extension_receiver.mjs"
        ))
        .arg(root.path())
        .arg(env!("CARGO_BIN_EXE_coven"))
        .status()
        .unwrap();
    assert!(status.success());
    assert!(!root.path().join("r.sock").exists());
    assert!(!root.path().join("state/agent-management.json").exists());
}
