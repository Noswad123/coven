#![cfg(unix)]
//! Frozen inputs reconstructed from committed 887451b, not the extracted generator.
use coven::pi_extension::{Operation, Request, arguments};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt};

const GENERATED: &[u8] = include_bytes!("fixtures/pi_extension_887451b/generated.ts");
const COMPLETED: &[u8] = include_bytes!("fixtures/pi_extension_887451b/completed.json");
const PENDING: &[u8] = include_bytes!("fixtures/pi_extension_887451b/pending.json");
const NEW: &str = "coven-agent-reference.ts";
const OLD: &str = "kitsune-agent-state.ts";
const RECEIPT: &str = "coven-pi-extension.json";

#[cfg(target_os = "linux")]
#[test]
fn frozen_v1_completed_and_pending_migration_survive_uninstall_and_rollback() {
    for frozen in [COMPLETED, PENDING] {
        let baseline: Value = serde_json::from_slice(frozen).unwrap();
        for prefix in 0..=2 {
            if frozen == COMPLETED && prefix != 2 {
                continue;
            }
            let dir = tempfile::tempdir().unwrap();
            if prefix > 0 {
                fs::write(dir.path().join(NEW), GENERATED).unwrap();
            }
            if prefix < 2 {
                fs::write(
                    dir.path().join(OLD),
                    baseline["original"][1]["text"].as_str().unwrap(),
                )
                .unwrap();
                fs::set_permissions(dir.path().join(OLD), fs::Permissions::from_mode(0o750))
                    .unwrap();
            }
            if prefix > 0 {
                fs::set_permissions(dir.path().join(NEW), fs::Permissions::from_mode(0o600))
                    .unwrap();
            }
            fs::write(dir.path().join(RECEIPT), frozen).unwrap();
            fs::set_permissions(dir.path().join(RECEIPT), fs::Permissions::from_mode(0o600))
                .unwrap();
            let mut r = Request {
                extension_root: dir.path().into(),
                executable: None,
                socket: None,
                migrate_kitsune: false,
                apply: true,
                operation: Operation::Install,
            };
            let before: Vec<_> = fs::read_dir(dir.path())
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            r.operation = Operation::Plan;
            let plan: Value = serde_json::from_str(&arguments(&r, true).unwrap().stdout).unwrap();
            assert_eq!(
                plan["status"],
                if frozen == PENDING {
                    "recovery_required"
                } else {
                    "unchanged"
                }
            );
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), before.len());
            r.operation = Operation::Install;
            let output = arguments(&r, false).unwrap();
            let warning = "Migration drops Kit working/blocked/idle reporting; Coven reports session references only.";
            assert_eq!(output.warnings, vec![warning.to_owned()]);
            assert_eq!(
                output.stdout,
                format!(
                    "Pi extension: {}; File placement is not proof of native activation; no approvals or settings are changed. {warning}",
                    if frozen == PENDING {
                        "installed"
                    } else {
                        "unchanged"
                    }
                )
            );
            assert_eq!(fs::read(dir.path().join(NEW)).unwrap(), GENERATED);
            assert_eq!(
                format!("{:x}", Sha256::digest(GENERATED)),
                baseline["installed"][0]["digest"].as_str().unwrap()
            );
            for op in [
                Operation::Uninstall,
                Operation::Install,
                Operation::Uninstall,
                Operation::Rollback,
            ] {
                r.operation = op;
                arguments(&r, true).unwrap();
                let current: Value =
                    serde_json::from_slice(&fs::read(dir.path().join(RECEIPT)).unwrap()).unwrap();
                assert_eq!(current["original"], baseline["original"]);
            }
            assert!(!dir.path().join(NEW).exists());
            assert_eq!(
                fs::read(dir.path().join(OLD)).unwrap(),
                baseline["original"][1]["text"].as_str().unwrap().as_bytes()
            );
            assert_eq!(
                fs::metadata(dir.path().join(OLD))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o7777,
                0o750
            );
        }
    }
}
