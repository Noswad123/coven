#![cfg(unix)]
#[path = "support/physical_temp.rs"]
mod physical_temp;
use claude_hooks::Operation;
use coven::{
    claude_hooks, codex_hooks, copilot_hooks, integration_route, opencode_extension, pi_extension,
};
use serde_json::Value;
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn run(
    provider: &str,
    root: &Path,
    exe: &Path,
    pin: Option<String>,
    op: Operation,
) -> coven::Result<Value> {
    let out = match provider {
        "claude" | "copilot" => {
            let r = claude_hooks::Request {
                config_root: root.into(),
                executable: Some(exe.to_str().unwrap().into()),
                socket: pin,
                migrate_kitsune: false,
                apply: true,
                operation: op,
            };
            if provider == "claude" {
                claude_hooks::arguments(&r, true)
            } else {
                copilot_hooks::arguments(&r, true)
            }
        }
        "codex" => codex_hooks::arguments(
            &codex_hooks::Request {
                config_root: root.into(),
                executable: Some(exe.to_str().unwrap().into()),
                socket: pin,
                migrate_kitsune: false,
                apply: true,
                operation: op,
            },
            true,
        ),
        _ => {
            let r = pi_extension::Request {
                extension_root: root.into(),
                executable: Some(exe.to_str().unwrap().into()),
                socket: pin,
                migrate_kitsune: false,
                apply: true,
                operation: op,
            };
            if provider == "pi" {
                pi_extension::arguments(&r, true)
            } else {
                opencode_extension::arguments(&r, true)
            }
        }
    }?;
    Ok(serde_json::from_str(&out.stdout).unwrap())
}

#[test]
fn all_five_literal_execution_version_fences_and_guarded_migration() {
    for provider in ["claude", "codex", "copilot", "pi", "opencode"] {
        for explicit in [false, true] {
            let work = physical_temp::tempdir();
            let root = work.path().join("config");
            fs::create_dir(&root).unwrap();
            let exe = work.path().join("reporter ' $literal");
            fs::write(&exe, "#!/usr/bin/env node\nconst fs = require('node:fs'); fs.writeFileSync(process.env.COVEN_ROUTE_LOG, JSON.stringify({argv:process.argv.slice(2), input:fs.readFileSync(0,'utf8')}));\n").unwrap();
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
            let state = work.path().join("state ' $(no-eval) ${HOME}");
            let pin =
                integration_route::installed_pin(explicit.then_some(state.as_path())).unwrap();
            let fixed = Some("/fixed/receiver".into());
            run(provider, &root, &exe, fixed.clone(), Operation::Install).unwrap();
            assert!(run(provider, &root, &exe, Some(pin.clone()), Operation::Install).is_err());
            run(provider, &root, &exe, None, Operation::Uninstall).unwrap();
            assert!(
                run(
                    provider,
                    &root,
                    &exe,
                    Some("/other/fixed".into()),
                    Operation::Install
                )
                .is_err()
            );
            let leaves: &[&str] = match provider {
                "claude" | "copilot" => &["settings.json"],
                "codex" => &["hooks.json", "config.toml"],
                "pi" => &["coven-agent-reference.ts", "kitsune-agent-state.ts"],
                _ => &["coven-agent-reference.js", "kitsune-agent-state.js"],
            };
            let original: Vec<_> = leaves
                .iter()
                .map(|leaf| fs::read(root.join(leaf)).ok())
                .collect();
            let plan = run(provider, &root, &exe, Some(pin.clone()), Operation::Plan).unwrap();
            assert_eq!(plan["route"]["kind"], "from_kitsune");
            run(provider, &root, &exe, Some(pin.clone()), Operation::Install).unwrap();
            assert_eq!(
                run(provider, &root, &exe, Some(pin.clone()), Operation::Install).unwrap()["status"],
                "unchanged"
            );
            let receipt_path = root.join(format!(
                "coven-{provider}-{}.json",
                if provider == "pi" || provider == "opencode" {
                    "extension"
                } else {
                    "hooks"
                }
            ));
            let receipt_bytes = fs::read(&receipt_path).unwrap();
            let mut receipt: Value = serde_json::from_slice(&receipt_bytes).unwrap();
            assert_eq!(
                receipt["version"],
                if provider == "claude" || provider == "copilot" {
                    3
                } else {
                    2
                }
            );
            let mut pending = receipt.clone();
            if provider == "claude" || provider == "copilot" {
                pending["stage"] = "pending_install".into();
            } else {
                pending["pending"] = true.into();
            }
            fs::write(&receipt_path, serde_json::to_vec(&pending).unwrap()).unwrap();
            assert_eq!(
                run(provider, &root, &exe, None, Operation::Plan).unwrap()["status"],
                "recovery_required"
            );
            assert_eq!(
                run(provider, &root, &exe, Some(pin.clone()), Operation::Install).unwrap()["status"],
                "installed"
            );
            receipt["version"] = Value::from(if provider == "claude" || provider == "copilot" {
                2
            } else {
                1
            });
            fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
            assert!(run(provider, &root, &exe, None, Operation::Rollback).is_err());
            fs::write(&receipt_path, receipt_bytes).unwrap();
            let mut corrupt = receipt.clone();
            corrupt["version"] = Value::from(if provider == "claude" || provider == "copilot" {
                3
            } else {
                2
            });
            corrupt["route"]["state_root"] = "/different/state".into();
            fs::write(&receipt_path, serde_json::to_vec(&corrupt).unwrap()).unwrap();
            assert!(run(provider, &root, &exe, None, Operation::Rollback).is_err());
            corrupt["route"]["state_root"] = if explicit {
                state.to_str().unwrap().into()
            } else {
                Value::Null
            };
            fs::write(&receipt_path, serde_json::to_vec(&corrupt).unwrap()).unwrap();

            let log = work.path().join("argv.json");
            let event = if provider == "pi" {
                "session_start"
            } else if provider == "opencode" {
                "session.created"
            } else {
                "SessionStart"
            };
            let mut command;
            if provider == "pi" || provider == "opencode" {
                let leaf = root.join(if provider == "pi" {
                    "coven-agent-reference.ts"
                } else {
                    "coven-agent-reference.js"
                });
                let module = work.path().join("generated.mjs");
                fs::copy(leaf, &module).unwrap();
                command = Command::new("node");
                command
                    .arg("--input-type=module")
                    .arg("-e")
                    .arg(include_str!("integration_route_asset.mjs"))
                    .arg(module)
                    .arg(provider);
            } else {
                let file = root.join(if provider == "codex" {
                    "hooks.json"
                } else {
                    "settings.json"
                });
                let settings: Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
                let entry = &settings["hooks"]["SessionStart"][0];
                let shell = if provider == "copilot" {
                    entry["bash"].as_str().unwrap()
                } else {
                    entry["hooks"][0]["command"].as_str().unwrap()
                };
                command = Command::new("sh");
                command.arg("-c").arg(shell);
            }
            let output = command.env("COVEN_ROUTE_LOG", &log).output().unwrap();
            assert!(
                output.status.success(),
                "{provider}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let report: Value = serde_json::from_slice(&fs::read(log).unwrap()).unwrap();
            let mut expected = vec![
                "agent".to_owned(),
                if provider == "pi" || provider == "opencode" {
                    "report-extension"
                } else {
                    "report"
                }
                .into(),
                "--provider".into(),
                provider.into(),
                "--from-kitsune".into(),
            ];
            if explicit {
                expected.extend(["--state-root".into(), state.to_str().unwrap().into()]);
            }
            expected.extend(["--event".into(), event.into()]);
            assert_eq!(report["argv"], serde_json::to_value(expected).unwrap());
            assert!(
                !fs::read_to_string(&receipt_path)
                    .unwrap()
                    .contains("ses_route")
            );
            run(provider, &root, &exe, None, Operation::Uninstall).unwrap();
            run(provider, &root, &exe, Some(pin), Operation::Install).unwrap();
            run(provider, &root, &exe, None, Operation::Rollback).unwrap();
            assert_eq!(
                leaves
                    .iter()
                    .map(|leaf| fs::read(root.join(leaf)).ok())
                    .collect::<Vec<_>>(),
                original
            );
        }
    }
}

#[test]
fn root_pins_require_absolute_literal_paths() {
    for bad in ["relative", "/root/../other", "/root\nsecret"] {
        assert!(integration_route::installed_pin(Some(Path::new(bad))).is_err());
    }
    assert!(integration_route::installed_pin(None).is_ok());
}
