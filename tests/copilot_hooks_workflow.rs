#![cfg(unix)]
#[path = "support/physical_temp.rs"]
mod physical_temp;
use coven::copilot_hooks::{Operation, Request, arguments};
use physical_temp::tempdir;
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};

fn request(root: &Path, operation: Operation, apply: bool) -> Request {
    Request {
        config_root: root.into(),
        executable: Some("/abs/coven's binary".into()),
        socket: Some("/abs/receiver socket".into()),
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
fn settings(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(root.join("settings.json")).unwrap()).unwrap()
}
fn receipt(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(root.join("coven-copilot-hooks.json")).unwrap()).unwrap()
}
fn stage(root: &Path, stage: &str) {
    let mut r = receipt(root);
    r["stage"] = json!(stage);
    fs::write(
        root.join("coven-copilot-hooks.json"),
        serde_json::to_vec(&r).unwrap(),
    )
    .unwrap();
}
fn fixture() -> (tempfile::TempDir, Vec<u8>) {
    let dir = tempdir();
    let pre = br#"{ "secret":"PRIVATE-SECRET", "disableAllHooks":true, "sandbox":{"enabled":true}, "trust":{"custom":true}, "hooks":{"sessionStart":[{"bash":"custom-secret-command"}]}, "unknown":[1,true] }"#.to_vec();
    fs::write(dir.path().join("settings.json"), &pre).unwrap();
    fs::set_permissions(
        dir.path().join("settings.json"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    (dir, pre)
}

#[test]
fn previews_are_effect_free_redacted_and_do_not_grant_native_approval() {
    let (dir, pre) = fixture();
    for op in [Operation::Plan, Operation::Install] {
        let r = request(dir.path(), op, false);
        let output = succeeds(&r);
        assert_eq!(output["provider"], "copilot");
        assert_eq!(output["events"], json!(["SessionStart"]));
        let text = output.to_string();
        assert!(!text.contains("PRIVATE-SECRET") && !text.contains("custom-secret-command"));
        assert!(text.contains("not granted") && text.contains("7c3387d3"));
    }
    succeeds(&request(dir.path(), Operation::Plan, true));
    let mut unspecified = request(dir.path(), Operation::Plan, true);
    unspecified.executable = None;
    unspecified.socket = None;
    assert_eq!(
        succeeds(&unspecified)["status"],
        "planned_requires_executable_and_socket"
    );
    assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), pre);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
#[test]
fn other_unix_is_preview_only() {
    let (dir, pre) = fixture();
    succeeds(&request(dir.path(), Operation::Plan, true));
    succeeds(&request(dir.path(), Operation::Install, false));
    for operation in [
        Operation::Install,
        Operation::Uninstall,
        Operation::Rollback,
    ] {
        refuses(
            &request(dir.path(), operation, true),
            "copilot_hooks_apply_unsupported_platform",
        );
        assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), pre);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn install_repeat_uninstall_rollback_and_reinstall_preserve_native_policy() {
    for rollback in [false, true] {
        let (dir, pre) = fixture();
        let original = settings(dir.path());
        let install = request(dir.path(), Operation::Install, true);
        assert_eq!(succeeds(&install)["status"], "installed");
        assert_eq!(succeeds(&install)["status"], "unchanged");
        let post = settings(dir.path());
        for key in ["disableAllHooks", "sandbox", "trust", "unknown", "secret"] {
            assert_eq!(post[key], original[key]);
        }
        assert_eq!(
            post["hooks"]["sessionStart"],
            original["hooks"]["sessionStart"]
        );
        let handler = &post["hooks"]["SessionStart"][0];
        assert_eq!(handler["type"], "command");
        assert_eq!(handler["timeoutSec"], 10);
        assert!(handler.get("hooks").is_none() && handler.get("matcher").is_none());
        let command = handler["bash"].as_str().unwrap();
        assert!(command.starts_with("'/abs/coven'\"'\"'s binary' agent report --provider copilot"));
        assert!(command.ends_with(
            "--event SessionStart >/dev/null 2>&1 || true # coven-managed:copilot-hooks:v1"
        ));
        assert_eq!(receipt(dir.path())["version"], 2);
        for name in [
            "settings.json",
            "coven-copilot-hooks.json",
            ".coven-copilot-hooks.lock",
        ] {
            assert_eq!(
                fs::metadata(dir.path().join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o7777,
                0o600
            );
        }
        let finish = request(
            dir.path(),
            if rollback {
                Operation::Rollback
            } else {
                Operation::Uninstall
            },
            true,
        );
        succeeds(&finish);
        assert_eq!(succeeds(&finish)["status"], "unchanged");
        if rollback {
            assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), pre);
            assert_eq!(
                fs::metadata(dir.path().join("settings.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o7777,
                0o640
            );
        } else {
            assert!(
                settings(dir.path())["hooks"]["SessionStart"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(settings(dir.path())["disableAllHooks"], true);
        }
        succeeds(&install);
        assert_eq!(succeeds(&install)["status"], "unchanged");
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn edits_and_chmod_only_changes_conflict_for_every_operation() {
    for chmod in [false, true] {
        let (dir, _) = fixture();
        succeeds(&request(dir.path(), Operation::Install, true));
        let path = dir.path().join("settings.json");
        if chmod {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        } else {
            let mut v = settings(dir.path());
            v["edit"] = json!(true);
            fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
        }
        let before = fs::read(&path).unwrap();
        for op in [
            Operation::Plan,
            Operation::Install,
            Operation::Uninstall,
            Operation::Rollback,
        ] {
            refuses(&request(dir.path(), op, true), "copilot_hooks_conflict");
            assert_eq!(fs::read(&path).unwrap(), before);
        }
    }
}

#[test]
fn unsafe_inputs_and_recovery_entries_refuse_without_preview_effects() {
    for kind in [
        "symlink",
        "hardlink",
        "directory",
        "oversize",
        "invalid",
        "duplicate",
        "jsonc",
        "private",
        "root",
        "recovery",
    ] {
        let (dir, _) = fixture();
        let path = dir.path().join("settings.json");
        let code = match kind {
            "symlink" | "hardlink" => {
                fs::rename(&path, dir.path().join("original")).unwrap();
                if kind == "symlink" {
                    symlink(dir.path().join("original"), &path).unwrap();
                } else {
                    fs::hard_link(dir.path().join("original"), &path).unwrap();
                }
                "copilot_hooks_unsafe_leaf"
            }
            "directory" => {
                fs::remove_file(&path).unwrap();
                fs::create_dir(&path).unwrap();
                "copilot_hooks_unsafe_leaf"
            }
            "oversize" => {
                fs::write(&path, vec![b' '; 1024 * 1024 + 1]).unwrap();
                "copilot_hooks_size_limit"
            }
            "invalid" => {
                fs::write(&path, br#"{"hooks":{"SessionStart":[{"bash":false}]}}"#).unwrap();
                "copilot_hooks_invalid_settings"
            }
            "duplicate" => {
                fs::write(&path, br#"{"a":1,"a":2}"#).unwrap();
                "copilot_hooks_invalid_json"
            }
            "jsonc" => {
                fs::write(&path, b"{/*comment*/}").unwrap();
                "copilot_hooks_invalid_json"
            }
            "private" => {
                fs::write(dir.path().join("coven-copilot-hooks.json"), b"{}").unwrap();
                fs::set_permissions(
                    dir.path().join("coven-copilot-hooks.json"),
                    fs::Permissions::from_mode(0o644),
                )
                .unwrap();
                "copilot_hooks_unsafe_private_file"
            }
            "root" => {
                fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o777)).unwrap();
                "copilot_hooks_unsafe_root"
            }
            "recovery" => {
                fs::create_dir(dir.path().join(".coven-copilot-hooks-recovery")).unwrap();
                "copilot_hooks_recovery_required"
            }
            _ => unreachable!(),
        };
        let count = fs::read_dir(dir.path()).unwrap().count();
        refuses(&request(dir.path(), Operation::Plan, false), code);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), count);
    }
}

#[test]
fn invalid_http_environment_expansion_refuses_before_configuration_effects() {
    for allowed in [json!([]), json!(["TOKEN"])] {
        let dir = tempdir();
        let path = dir.path().join("settings.json");
        let pre = serde_json::to_vec(&json!({"hooks":{"SessionStart":[{
            "type":"http", "url":"http://localhost:8080", "allowedEnvVars":allowed
        }]}}))
        .unwrap();
        fs::write(&path, &pre).unwrap();
        for (op, apply) in [
            (Operation::Plan, true),
            (Operation::Install, false),
            (Operation::Install, true),
        ] {
            refuses(
                &request(dir.path(), op, apply),
                if apply
                    && op != Operation::Plan
                    && !cfg!(any(target_os = "linux", target_os = "macos"))
                {
                    "copilot_hooks_apply_unsupported_platform"
                } else {
                    "copilot_hooks_invalid_settings"
                },
            );
            assert_eq!(fs::read(&path).unwrap(), pre);
            assert!(!dir.path().join("coven-copilot-hooks.json").exists());
            // Explicit apply may acquire the persistent operation lock before
            // validating settings; previews must remain entirely effect-free.
            assert_eq!(
                fs::read_dir(dir.path()).unwrap().count(),
                if apply
                    && op != Operation::Plan
                    && cfg!(any(target_os = "linux", target_os = "macos"))
                {
                    2
                } else {
                    1
                }
            );
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn pending_operations_resume_only_at_source_or_target_and_same_stage() {
    for op in [
        Operation::Install,
        Operation::Uninstall,
        Operation::Rollback,
    ] {
        for at_target in [false, true] {
            for present in [false, true] {
                let dir = tempdir();
                let path = dir.path().join("settings.json");
                let pre = b"{ \"original\":true }\n";
                if present {
                    fs::write(&path, pre).unwrap();
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
                }
                succeeds(&request(dir.path(), Operation::Install, true));
                if op == Operation::Install && !at_target {
                    if present {
                        fs::write(&path, pre).unwrap();
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
                    } else {
                        fs::remove_file(&path).unwrap();
                    }
                } else if op != Operation::Install && at_target {
                    succeeds(&request(dir.path(), op, true));
                }
                stage(
                    dir.path(),
                    match op {
                        Operation::Install => "pending_install",
                        Operation::Uninstall => "pending_uninstall",
                        Operation::Rollback => "pending_rollback",
                        _ => unreachable!(),
                    },
                );
                let before = fs::read(&path).ok();
                let receipt_before = fs::read(dir.path().join("coven-copilot-hooks.json")).unwrap();
                assert!(
                    succeeds(&request(dir.path(), Operation::Plan, false))["status"]
                        .as_str()
                        .unwrap()
                        .starts_with("recovery_required")
                );
                assert_eq!(
                    succeeds(&request(dir.path(), op, false))["status"],
                    "recovery_required"
                );
                assert_eq!(fs::read(&path).ok(), before);
                assert_eq!(
                    fs::read(dir.path().join("coven-copilot-hooks.json")).unwrap(),
                    receipt_before
                );
                refuses(
                    &request(
                        dir.path(),
                        if op == Operation::Rollback {
                            Operation::Uninstall
                        } else {
                            Operation::Rollback
                        },
                        false,
                    ),
                    "copilot_hooks_receipt_stage_conflict",
                );
                succeeds(&request(dir.path(), op, true));
                assert_eq!(
                    succeeds(&request(dir.path(), op, true))["status"],
                    "unchanged"
                );
                if op == Operation::Rollback {
                    assert_eq!(path.exists(), present);
                    if present {
                        assert_eq!(fs::read(&path).unwrap(), pre);
                        assert_eq!(
                            fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
                            0o400
                        );
                    }
                }
            }
        }
    }
}

// Verbatim inert compatibility fixture, never executed or installed by Coven.
// Source: Noswad123/kitsune src/integration/assets/copilot/kitsune-agent-state.sh,
// Apache-2.0. Full license: fixtures/copilot_hooks/KITSUNE_LICENSE.
const KIT: &[u8] = include_bytes!("fixtures/copilot_hooks/kitsune-agent-state.sh");
fn legacy(root: &Path) -> Vec<u8> {
    fs::create_dir(root.join("hooks")).unwrap();
    let script = root.join("hooks/kitsune-agent-state.sh");
    fs::write(&script, KIT).unwrap();
    let quoted = script.to_str().unwrap().replace('\'', "'\"'\"'");
    serde_json::to_vec(&json!({"disableAllHooks":true, "hooks":{"SessionStart":[
        {"type":"command","bash":format!("bash '{quoted}'"),"timeoutSec":10},
        {"bash":"custom","metadata":{"command":"kitsune-agent-state"}}]}}))
    .unwrap()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn verified_migration_leaves_script_and_custom_hooks_uninstall_never_revives_kit() {
    for rollback in [false, true] {
        let dir = tempdir();
        let pre = legacy(dir.path());
        fs::write(dir.path().join("settings.json"), &pre).unwrap();
        let mut r = request(dir.path(), Operation::Install, false);
        refuses(&r, "copilot_hooks_migration_consent_required");
        r.migrate_kitsune = true;
        assert_eq!(succeeds(&r)["migrate_kitsune"], true);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        r.apply = true;
        succeeds(&r);
        assert_eq!(
            settings(dir.path())["hooks"]["SessionStart"][0]["bash"],
            "custom"
        );
        r.operation = if rollback {
            Operation::Rollback
        } else {
            Operation::Uninstall
        };
        succeeds(&r);
        if rollback {
            assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), pre);
        } else {
            assert_eq!(
                settings(dir.path())["hooks"]["SessionStart"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
        }
        assert_eq!(
            fs::read(dir.path().join("hooks/kitsune-agent-state.sh")).unwrap(),
            KIT
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn pending_migration_revalidates_script_only_at_uncommitted_source() {
    for changed in ["modified", "missing", "symlink", "directory_symlink"] {
        for at_target in [false, true] {
            let dir = tempdir();
            let pre = legacy(dir.path());
            let path = dir.path().join("settings.json");
            fs::write(&path, &pre).unwrap();
            let mut r = request(dir.path(), Operation::Install, true);
            r.migrate_kitsune = true;
            succeeds(&r);
            stage(dir.path(), "pending_install");
            if !at_target {
                fs::write(&path, &pre).unwrap();
                fs::set_permissions(
                    &path,
                    fs::Permissions::from_mode(
                        receipt(dir.path())["pre_mode"].as_u64().unwrap() as u32
                    ),
                )
                .unwrap();
            }
            let script = dir.path().join("hooks/kitsune-agent-state.sh");
            match changed {
                "modified" => fs::write(&script, "modified").unwrap(),
                "missing" => fs::remove_file(&script).unwrap(),
                "symlink" => {
                    fs::rename(&script, dir.path().join("old")).unwrap();
                    symlink(dir.path().join("old"), &script).unwrap();
                }
                "directory_symlink" => {
                    fs::rename(dir.path().join("hooks"), dir.path().join("old")).unwrap();
                    symlink(dir.path().join("old"), dir.path().join("hooks")).unwrap();
                }
                _ => unreachable!(),
            }
            if at_target {
                succeeds(&r);
            } else {
                let before = fs::read(dir.path().join("coven-copilot-hooks.json")).unwrap();
                assert!(arguments(&r, true).is_err());
                assert_eq!(fs::read(&path).unwrap(), pre);
                assert_eq!(
                    fs::read(dir.path().join("coven-copilot-hooks.json")).unwrap(),
                    before
                );
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn strict_receipts_and_resupplied_paths_are_data_not_authority() {
    let (dir, _) = fixture();
    let mut r = request(dir.path(), Operation::Install, true);
    succeeds(&r);
    r.socket = Some("/other/socket".into());
    refuses(&r, "copilot_hooks_conflict");
    let path = dir.path().join("coven-copilot-hooks.json");
    let raw = fs::read(&path).unwrap();
    for key in ["arbitrary_path", "post_digest", "version", "stage"] {
        let mut v: Value = serde_json::from_slice(&raw).unwrap();
        v[key] = json!("untrusted");
        fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
        refuses(
            &request(dir.path(), Operation::Plan, false),
            "copilot_hooks_invalid_receipt",
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn generated_command_forwards_stdin_quotes_paths_and_tolerates_failure_silently() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let dir = tempdir();
    let exe = dir.path().join("coven's executable");
    fs::write(&exe, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CAPTURE\"\nIFS= read -r payload\nprintf '%s' \"$payload\" > \"$PAYLOAD\"\nexit 42\n").unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
    let mut r = request(dir.path(), Operation::Install, true);
    r.executable = Some(exe.to_str().unwrap().into());
    succeeds(&r);
    let v = settings(dir.path());
    let command = v["hooks"]["SessionStart"][0]["bash"].as_str().unwrap();
    let mut child = Command::new("/bin/sh")
        .args(["-c", command])
        .env("CAPTURE", dir.path().join("args"))
        .env("PAYLOAD", dir.path().join("payload"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let payload = r#"{"hook_event_name":"SessionStart","session_id":"native-session","timestamp":"2026-11-10T00:00:00Z","cwd":"/example","source":"startup"}"#;
    writeln!(child.stdin.take().unwrap(), "{payload}").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success() && output.stdout.is_empty() && output.stderr.is_empty());
    assert_eq!(
        fs::read_to_string(dir.path().join("payload")).unwrap(),
        payload
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("args")).unwrap(),
        "agent\nreport\n--provider\ncopilot\n--socket\n/abs/receiver socket\n--event\nSessionStart\n"
    );
    fs::remove_file(&exe).unwrap();
    let offline = Command::new("/bin/sh")
        .args(["-c", command])
        .output()
        .unwrap();
    assert!(offline.status.success() && offline.stdout.is_empty() && offline.stderr.is_empty());
}
