#![cfg(unix)]
use coven::codex_hooks::{Operation, Request, arguments};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::Command,
};

const HOOKS: &str = "hooks.json";
const CONFIG: &str = "config.toml";
const RECEIPT: &str = "coven-codex-hooks.json";
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
fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o7777
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(HOOKS), br#"{ "description":"PRIVATE-SECRET", "hooks":{"Stop":[{"matcher":"custom","hooks":[{"type":"command","command":"custom-secret-command"}]}]}}"#).unwrap();
    fs::write(dir.path().join(CONFIG), CONFIG_PRE).unwrap();
    dir
}
const CONFIG_PRE: &str = "# top comment\nmodel = 'PRIVATE-MODEL'\ncodex_hooks = 'unrelated root key'\n\n[features] # flags\nhooks = false # keep this comment\ncodex_hooks = true\nother = true\n\n[hooks.state.\"local-hook\"]\nenabled = false # user-managed disposition\ntrusted_hash = 'KEEP-TRUST-STATE' # user-managed approval\ntrusted_content_hash = 'KEEP-TRUST-CONTENT'\n\n[profiles.custom.features]\nhooks = false\ncodex_hooks = true\n";

#[test]
fn previews_are_redacted_zero_effects_with_or_without_files() {
    for populated in [false, true] {
        let dir = if populated {
            fixture()
        } else {
            tempfile::tempdir().unwrap()
        };
        let count = fs::read_dir(dir.path()).unwrap().count();
        for (operation, apply) in [
            (Operation::Plan, false),
            (Operation::Plan, true),
            (Operation::Install, false),
        ] {
            let r = request(dir.path(), operation, apply);
            let value = succeeds(&r);
            assert_eq!(value["status"], "planned");
            let text = value.to_string();
            assert!(
                !text.contains("PRIVATE")
                    && !text.contains("custom-secret")
                    && !text.contains("KEEP-TRUST")
            );
            assert!(text.contains("user-managed") && text.contains("not granted"));
            assert!(!text.contains("operational"));
            assert_eq!(value["features"]["hooks_after"], true);
            assert_eq!(value["features"]["remove_legacy_alias"], populated);
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), count);
        }
        let mut r = request(dir.path(), Operation::Plan, true);
        r.executable = None;
        r.socket = None;
        assert_eq!(
            succeeds(&r)["status"],
            "planned_requires_executable_and_socket"
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), count);
        if populated {
            assert_eq!(
                fs::read_to_string(dir.path().join(CONFIG)).unwrap(),
                CONFIG_PRE
            );
        }
    }
}

#[test]
fn registered_codex_cli_plan_is_required_and_read_only() {
    let dir = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_coven"))
        .args([
            "agent",
            "hooks",
            "plan",
            "--provider",
            "codex",
            "--config-root",
        ])
        .arg(dir.path())
        .args([
            "--executable",
            "/abs/coven",
            "--socket",
            "/abs/receiver",
            "--apply",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["provider"], "codex");
    assert_eq!(value["status"], "planned");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    assert_eq!(
        fs::read_to_string(dir.path().join(CONFIG)).unwrap(),
        CONFIG_PRE
    );
}

#[cfg(target_os = "linux")]
#[test]
fn native_session_start_only_surgical_features_and_exact_rollback_modes_absence() {
    for hooks_present in [false, true] {
        for config_present in [false, true] {
            for original_mode in [0o400, 0o640] {
                let dir = fixture();
                fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o2700)).unwrap();
                for (leaf, present) in [(HOOKS, hooks_present), (CONFIG, config_present)] {
                    if present {
                        fs::set_permissions(
                            dir.path().join(leaf),
                            fs::Permissions::from_mode(original_mode),
                        )
                        .unwrap();
                    } else {
                        fs::remove_file(dir.path().join(leaf)).unwrap();
                    }
                }
                let original_hooks = fs::read(dir.path().join(HOOKS)).ok();
                let original_config = fs::read(dir.path().join(CONFIG)).ok();
                let r = request(dir.path(), Operation::Install, true);
                assert_eq!(succeeds(&r)["status"], "installed");
                assert_eq!(succeeds(&r)["status"], "unchanged");
                let value: Value =
                    serde_json::from_slice(&fs::read(dir.path().join(HOOKS)).unwrap()).unwrap();
                let entry = &value["hooks"]["SessionStart"][0];
                assert!(entry.get("matcher").is_none());
                assert_eq!(entry["hooks"][0]["timeout"], 2);
                let cmd = entry["hooks"][0]["command"].as_str().unwrap();
                assert_eq!(
                    cmd,
                    "'/abs/coven'\"'\"'s binary' agent report --provider codex --socket '/abs/receiver socket' --event SessionStart >/dev/null 2>&1 || true # coven-managed:codex-hooks:v1"
                );
                assert_eq!(
                    value["hooks"].as_object().unwrap().len(),
                    if hooks_present { 2 } else { 1 }
                );
                if hooks_present {
                    assert_eq!(value["description"], "PRIVATE-SECRET");
                }
                let config = fs::read_to_string(dir.path().join(CONFIG)).unwrap();
                if config_present {
                    assert_eq!(
                        config,
                        CONFIG_PRE
                            .replacen("hooks = false", "hooks = true", 1)
                            .replacen("codex_hooks = true\n", "", 1)
                    );
                } else {
                    assert_eq!(config, "[features]\nhooks = true\n");
                }
                for leaf in [HOOKS, CONFIG, RECEIPT, ".coven-codex-hooks.lock"] {
                    assert_eq!(mode(&dir.path().join(leaf)), 0o600);
                }
                succeeds(&request(dir.path(), Operation::Rollback, true));
                assert_eq!(fs::read(dir.path().join(HOOKS)).ok(), original_hooks);
                assert_eq!(fs::read(dir.path().join(CONFIG)).ok(), original_config);
                for (leaf, present) in [(HOOKS, hooks_present), (CONFIG, config_present)] {
                    if present {
                        assert_eq!(mode(&dir.path().join(leaf)), original_mode);
                    }
                }
                assert_eq!(mode(dir.path()), 0o2700);
                assert!(!dir.path().join(".coven-codex-hooks-recovery").exists());
            }
        }
    }
}

#[test]
fn global_feature_variants_and_unsupported_layouts_are_explicit() {
    for config in [
        "",
        "model='x'\n",
        "[features]\nother=true\n",
        "[features]\nhooks=true\n",
        "[features]\nhooks=false\n",
        "[features]\ncodex_hooks=false\n",
        "[features]\nhooks=false\ncodex_hooks=true\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(CONFIG), config).unwrap();
        let value = succeeds(&request(dir.path(), Operation::Install, false));
        assert_eq!(value["features"]["hooks_after"], true);
        assert_eq!(fs::read_to_string(dir.path().join(CONFIG)).unwrap(), config);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    for config in ["features.hooks = false\n", "features = { hooks = false }\n"] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(CONFIG), config).unwrap();
        refuses(
            &request(dir.path(), Operation::Install, true),
            "codex_hooks_unsupported_feature_layout",
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn invalid_second_file_or_duplicate_keys_have_no_apply_artifacts() {
    for config in [
        "[features\n",
        "[features]\nhooks = 'yes'\n",
        "[features]\ncodex_hooks=1\n",
        "[features]\nhooks=true\nhooks=false\n",
        "features = false\n",
        "hooks = false\n",
    ] {
        let dir = fixture();
        fs::write(dir.path().join(CONFIG), config).unwrap();
        let before = fs::read(dir.path().join(HOOKS)).unwrap();
        assert!(arguments(&request(dir.path(), Operation::Install, true), true).is_err());
        assert_eq!(fs::read(dir.path().join(HOOKS)).unwrap(), before);
        assert_eq!(fs::read_to_string(dir.path().join(CONFIG)).unwrap(), config);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
    for hooks in [
        r#"{"hooks":{},"hooks":{}}"#,
        r#"{"hooks":{"SessionStart":"wrong"}}"#,
        r#"{"hooks":{"SessionStart":[{"hooks":[{"command":false}]}]}}"#,
    ] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(HOOKS), hooks).unwrap();
        assert!(arguments(&request(dir.path(), Operation::Install, true), true).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn uninstall_preserves_feature_and_custom_hooks_rollback_restores_original() {
    let dir = fixture();
    let original = fs::read(dir.path().join(HOOKS)).unwrap();
    succeeds(&request(dir.path(), Operation::Install, true));
    let enabled = fs::read(dir.path().join(CONFIG)).unwrap();
    succeeds(&request(dir.path(), Operation::Uninstall, false));
    succeeds(&request(dir.path(), Operation::Uninstall, true));
    let hooks: Value = serde_json::from_slice(&fs::read(dir.path().join(HOOKS)).unwrap()).unwrap();
    assert_eq!(hooks["hooks"]["SessionStart"], json!([]));
    assert_eq!(
        hooks["hooks"]["Stop"][0]["hooks"][0]["command"],
        "custom-secret-command"
    );
    assert_eq!(fs::read(dir.path().join(CONFIG)).unwrap(), enabled);
    succeeds(&request(dir.path(), Operation::Rollback, true));
    assert_eq!(fs::read(dir.path().join(HOOKS)).unwrap(), original);
    assert_eq!(
        fs::read_to_string(dir.path().join(CONFIG)).unwrap(),
        CONFIG_PRE
    );
}

#[cfg(target_os = "linux")]
#[test]
fn edits_chmod_and_absence_of_either_postimage_fence_before_first_change() {
    for leaf in [HOOKS, CONFIG] {
        for change in ["edit", "chmod", "absence"] {
            for operation in [Operation::Uninstall, Operation::Rollback] {
                let dir = fixture();
                succeeds(&request(dir.path(), Operation::Install, true));
                match change {
                    "edit" => {
                        let mut bytes = fs::read(dir.path().join(leaf)).unwrap();
                        bytes.push(b'\n');
                        fs::write(dir.path().join(leaf), bytes).unwrap();
                    }
                    "chmod" => fs::set_permissions(
                        dir.path().join(leaf),
                        fs::Permissions::from_mode(0o400),
                    )
                    .unwrap(),
                    _ => fs::remove_file(dir.path().join(leaf)).unwrap(),
                }
                let hooks = fs::read(dir.path().join(HOOKS)).ok();
                let config = fs::read(dir.path().join(CONFIG)).ok();
                let receipt = fs::read(dir.path().join(RECEIPT)).unwrap();
                refuses(
                    &request(dir.path(), operation, true),
                    "codex_hooks_conflict",
                );
                assert_eq!(fs::read(dir.path().join(HOOKS)).ok(), hooks);
                assert_eq!(fs::read(dir.path().join(CONFIG)).ok(), config);
                assert_eq!(fs::read(dir.path().join(RECEIPT)).unwrap(), receipt);
            }
        }
    }
}

#[test]
fn retained_recovery_and_unsafe_leaves_refuse_all_operations_even_preview() {
    for operation in [
        Operation::Plan,
        Operation::Install,
        Operation::Uninstall,
        Operation::Rollback,
    ] {
        for apply in [false, true] {
            let dir = fixture();
            fs::create_dir(dir.path().join(".coven-codex-hooks-recovery")).unwrap();
            refuses(
                &request(dir.path(), operation, apply),
                "codex_hooks_recovery_required",
            );
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
        }
    }
    let dir = fixture();
    fs::remove_file(dir.path().join(CONFIG)).unwrap();
    symlink(dir.path().join(HOOKS), dir.path().join(CONFIG)).unwrap();
    refuses(
        &request(dir.path(), Operation::Install, true),
        "codex_hooks_unsafe_leaf",
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn suspicious_kitsune_wrong_event_root_and_modified_asset_refuse_before_lock() {
    for (event, command) in [
        ("SessionStart", "echo kitsune-agent-state.sh".to_owned()),
        ("Stop", "bash '/wrong/kitsune-agent-state.sh' idle".into()),
        (
            "PostToolUse",
            "bash 'ROOT/kitsune-agent-state.sh' working".into(),
        ),
        (
            "SessionStart",
            "bash 'ROOT/kitsune-agent-state.sh' session".into(),
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let cmd = command.replace("ROOT", dir.path().to_str().unwrap());
        fs::write(
            dir.path().join(HOOKS),
            serde_json::to_vec(
                &json!({"hooks":{event:[{"hooks":[{"type":"command","command":cmd}]}]}}),
            )
            .unwrap(),
        )
        .unwrap();
        fs::write(dir.path().join("kitsune-agent-state.sh"), b"# modified").unwrap();
        let mut r = request(dir.path(), Operation::Install, true);
        r.migrate_kitsune = true;
        refuses(&r, "codex_hooks_ambiguous_kitsune");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}

// Frozen compatibility fixture: Noswad123/kitsune, integration/assets/codex/
// kitsune-agent-state.sh, integration version 6. Copyright Kitsune contributors.
// Apache-2.0; license text is in fixtures/claude_hooks/KITSUNE_LICENSE.
// This fixture is data, never executed and never read from a sibling checkout.
const KIT_SCRIPT: &str = r##"#!/bin/sh
# installed by kitsune
# managed by kitsune; reinstalling or updating the integration overwrites this file.
# add custom hooks beside this file instead of editing it.
# KITSUNE_INTEGRATION_ID=codex
# KITSUNE_INTEGRATION_VERSION=6

set -eu

action="${1:-}"
hook_input_file="$(mktemp "${TMPDIR:-/tmp}/kitsune-codex-hook.XXXXXX")" || exit 0
trap 'rm -f "$hook_input_file"' EXIT HUP INT TERM
cat >"$hook_input_file" 2>/dev/null || true

case "$action" in
  session) ;;
  *) exit 0 ;;
esac

[ "${KITSUNE_ENV:-}" = "1" ] || exit 0
[ -n "${KITSUNE_SOCKET_PATH:-}" ] || exit 0
[ -n "${KITSUNE_PANE_ID:-}" ] || exit 0
command -v python3 >/dev/null 2>&1 || exit 0

KITSUNE_ACTION="$action" KITSUNE_HOOK_INPUT_FILE="$hook_input_file" python3 - <<'PY'
import json
import os
import random
import socket
import time

source = "kitsune:codex"
action = os.environ.get("KITSUNE_ACTION", "")
pane_id = os.environ.get("KITSUNE_PANE_ID")
socket_path = os.environ.get("KITSUNE_SOCKET_PATH")
hook_input_file = os.environ.get("KITSUNE_HOOK_INPUT_FILE")

if not pane_id or not socket_path:
    raise SystemExit(0)

hook_input = {}
if hook_input_file:
    try:
        with open(hook_input_file, encoding="utf-8") as handle:
            content = handle.read()
        if content.strip():
            hook_input = json.loads(content)
    except Exception:
        hook_input = {}

hook_event_name = str(hook_input.get("hook_event_name") or "")
if hook_event_name and hook_event_name != "SessionStart":
    raise SystemExit(0)

request_id = f"{source}:{int(time.time() * 1000)}:{random.randrange(1_000_000):06d}"
report_seq = time.time_ns()
session_id = hook_input.get("session_id")
agent_session_id = session_id if isinstance(session_id, str) and session_id else None
session_start_source = hook_input.get("source") if hook_event_name == "SessionStart" else None
if not isinstance(session_start_source, str) or not session_start_source:
    session_start_source = None
if agent_session_id:
    params = {
        "pane_id": pane_id,
        "source": source,
        "agent": "codex",
        "seq": report_seq,
        "agent_session_id": agent_session_id,
    }
    if session_start_source:
        params["session_start_source"] = session_start_source
    request = {
        "id": request_id,
        "method": "pane.report_agent_session",
        "params": params,
    }
else:
    raise SystemExit(0)

try:
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.settimeout(0.5)
    client.connect(socket_path)
    client.sendall((json.dumps(request) + "\n").encode())
    try:
        client.recv(4096)
    except Exception:
        pass
    client.close()
except Exception:
    pass
PY
"##;

#[cfg(target_os = "linux")]
#[test]
fn verified_migration_both_files_preserves_custom_trust_and_never_resurrects_on_uninstall() {
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!("{:x}", Sha256::digest(KIT_SCRIPT.as_bytes())),
        "2343713ae8e665baa22968e155ced9fdc1269f43b490af230316754a18caaf9a"
    );
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("kitsune-agent-state.sh"), KIT_SCRIPT).unwrap();
    let mut hooks = json!({"hooks":{}});
    for (event, action) in [
        ("SessionStart", "session"),
        ("SessionStart", "idle"),
        ("UserPromptSubmit", "working"),
        ("PreToolUse", "working"),
        ("PermissionRequest", "blocked"),
        ("Stop", "idle"),
    ] {
        hooks["hooks"].as_object_mut().unwrap().entry(event).or_insert(json!([])).as_array_mut().unwrap().push(json!({"hooks":[{"type":"command","command":format!("bash '{}/kitsune-agent-state.sh' {action}", dir.path().display())},{"type":"command","command":"custom"}]}));
    }
    fs::write(dir.path().join(HOOKS), serde_json::to_vec(&hooks).unwrap()).unwrap();
    let config = format!(
        "[features]\ncodex_hooks = false\n[hooks]\nSessionStart = [{{hooks = [{{type = 'command', command = \"bash '{}/kitsune-agent-state.sh' session\"}}, {{type = 'command', command = 'custom'}}]}}]\n[hooks.state.\"local-hook\"]\nenabled = false # native disposition\ntrusted_hash = 'DO-NOT-CHANGE' # native approval\ntrusted_content_hash = 'DO-NOT-CHANGE-CONTENT'\n",
        dir.path().display()
    );
    fs::write(dir.path().join(CONFIG), &config).unwrap();
    let mut r = request(dir.path(), Operation::Install, true);
    refuses(&r, "codex_hooks_migration_consent_required");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    r.migrate_kitsune = true;
    assert_eq!(succeeds(&r)["migrate_kitsune"], true);
    for leaf in [HOOKS, CONFIG] {
        let text = fs::read_to_string(dir.path().join(leaf)).unwrap();
        assert!(!text.contains("kitsune-agent-state"));
        assert!(text.contains("custom"));
    }
    assert!(
        fs::read_to_string(dir.path().join(CONFIG))
            .unwrap()
            .contains("trusted_hash = 'DO-NOT-CHANGE' # native approval")
    );
    succeeds(&request(dir.path(), Operation::Uninstall, true));
    for leaf in [HOOKS, CONFIG] {
        assert!(
            !fs::read_to_string(dir.path().join(leaf))
                .unwrap()
                .contains("kitsune-agent-state")
        );
    }
    assert_eq!(
        fs::read_to_string(dir.path().join("kitsune-agent-state.sh")).unwrap(),
        KIT_SCRIPT
    );
    succeeds(&request(dir.path(), Operation::Rollback, true));
    assert_eq!(fs::read_to_string(dir.path().join(CONFIG)).unwrap(), config);
    let restored: Value =
        serde_json::from_slice(&fs::read(dir.path().join(HOOKS)).unwrap()).unwrap();
    assert_eq!(restored, hooks);
}

#[cfg(target_os = "linux")]
#[test]
fn pending_migration_reverifies_only_source_leaves_not_committed_targets() {
    for config_has_kit in [false, true] {
        for committed in 0..=2 {
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join("kitsune-agent-state.sh"), KIT_SCRIPT).unwrap();
            let command = format!(
                "bash '{}/kitsune-agent-state.sh' session",
                dir.path().display()
            );
            fs::write(dir.path().join(HOOKS), serde_json::to_vec(&json!({"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":command}]}]}})).unwrap()).unwrap();
            if config_has_kit {
                fs::write(dir.path().join(CONFIG), format!("[hooks]\nSessionStart = [{{hooks = [{{type = 'command', command = \"{command}\"}}]}}]\n")).unwrap();
            }
            let mut r = request(dir.path(), Operation::Install, true);
            r.migrate_kitsune = true;
            succeeds(&r);
            // Recreate a persisted crash prefix. The unit fault tests separately
            // exercise these boundaries through the actual write algorithm.
            let mut receipt: Value =
                serde_json::from_slice(&fs::read(dir.path().join(RECEIPT)).unwrap()).unwrap();
            receipt["pending"] = json!(true);
            fs::write(
                dir.path().join(RECEIPT),
                serde_json::to_vec(&receipt).unwrap(),
            )
            .unwrap();
            for (i, leaf) in [HOOKS, CONFIG].iter().enumerate() {
                let image = &receipt[if i < committed { "target" } else { "source" }][i];
                if image.is_null() {
                    fs::remove_file(dir.path().join(leaf)).unwrap();
                } else {
                    fs::write(dir.path().join(leaf), image["text"].as_str().unwrap()).unwrap();
                    fs::set_permissions(
                        dir.path().join(leaf),
                        fs::Permissions::from_mode(image["mode"].as_u64().unwrap() as u32),
                    )
                    .unwrap();
                }
            }
            fs::remove_file(dir.path().join("kitsune-agent-state.sh")).unwrap();
            if committed == 0 || (committed == 1 && config_has_kit) {
                let before = fs::read(dir.path().join(RECEIPT)).unwrap();
                refuses(&r, "codex_hooks_ambiguous_kitsune");
                assert_eq!(fs::read(dir.path().join(RECEIPT)).unwrap(), before);
            } else {
                r.migrate_kitsune = false; // Consent is already in the journal.
                assert_eq!(succeeds(&r)["status"], "installed");
                succeeds(&request(dir.path(), Operation::Uninstall, true));
                for leaf in [HOOKS, CONFIG] {
                    assert!(
                        !fs::read_to_string(dir.path().join(leaf))
                            .unwrap()
                            .contains("kitsune-agent-state")
                    );
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn array_of_tables_legacy_toml_and_unrelated_layout_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("kitsune-agent-state.sh"), KIT_SCRIPT).unwrap();
    let original = format!(
        "# preamble\n[features]\n'hooks'    =    false   # exact decoration\nother = [ 1,  2 ]\n[hooks.state.\"local-hook\"]\nenabled = false # disposition\ntrusted_hash = 'UNCHANGED' # trust\ntrusted_content_hash = 'UNCHANGED-CONTENT'\n[[hooks.SessionStart]]\n[[hooks.SessionStart.hooks]]\ntype = 'command'\ncommand = \"bash '{}/kitsune-agent-state.sh' session\"\n[[hooks.SessionStart.hooks]]\ntype = 'command'\ncommand = 'custom' # custom decoration\n",
        dir.path().display()
    );
    fs::write(dir.path().join(CONFIG), &original).unwrap();
    let mut r = request(dir.path(), Operation::Install, true);
    r.migrate_kitsune = true;
    succeeds(&r);
    let edited = fs::read_to_string(dir.path().join(CONFIG)).unwrap();
    assert!(edited.contains("'hooks'    =    true   # exact decoration"));
    assert!(edited.contains("other = [ 1,  2 ]"));
    assert!(edited.contains("trusted_hash = 'UNCHANGED' # trust"));
    assert!(edited.contains("command = 'custom' # custom decoration"));
    assert!(!edited.contains("kitsune-agent-state"));
    let rollback = succeeds(&request(dir.path(), Operation::Rollback, false));
    assert_eq!(rollback["features"]["hooks_before"], true);
    assert_eq!(rollback["features"]["hooks_after"], false);
    succeeds(&request(dir.path(), Operation::Rollback, true));
    assert_eq!(
        fs::read_to_string(dir.path().join(CONFIG)).unwrap(),
        original
    );
}

#[cfg(target_os = "linux")]
#[test]
fn migration_only_visits_direct_handlers_never_mcp_input_metadata_or_state() {
    for arrays_of_tables in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("kitsune-agent-state.sh"), KIT_SCRIPT).unwrap();
        let cmd = format!(
            "bash '{}/kitsune-agent-state.sh' session",
            dir.path().display()
        );
        let payload = format!(
            "{{type = 'command', command = \"{cmd}\", keep = 'custom', nested = [{{type = 'command', command = \"{cmd}\"}}]}}"
        );
        let state = format!(
            "[hooks.state.\"local-hook\"]\nenabled = false # never enable\ntrusted_hash = 'UNCHANGED' # never approve\ntrusted_content_hash = 'UNCHANGED-CONTENT'\ntype = 'command' # ignored metadata, not a handler\ncommand = \"{cmd}\"\n"
        );
        let groups = if arrays_of_tables {
            format!(
                "[[hooks.SessionStart]]\nmatcher = 'custom'\nmetadata = {payload}\n[[hooks.SessionStart.hooks]]\ntype = 'command'\ncommand = \"{cmd}\"\n[[hooks.SessionStart.hooks]]\ntype = 'mcp_tool'\nserver = 'audit'\ntool = 'check'\ninput = {payload} # preserve entire MCP input\n"
            )
        } else {
            format!(
                "[hooks]\nSessionStart = [{{matcher = 'custom', metadata = {payload}, hooks = [{{type = 'command', command = \"{cmd}\"}}, {{type = 'mcp_tool', server = 'audit', tool = 'check', input = {payload}}}]}}]\n"
            )
        };
        let config = format!("{groups}{state}");
        fs::write(dir.path().join(CONFIG), &config).unwrap();
        let before: toml::Value = toml::from_str(&config).unwrap();
        // Exercise the same ownership distinction in JSON too.
        let input = json!({"type":"command", "command":cmd, "keep":"custom", "nested":[{"type":"command", "command":cmd}]});
        let mcp = json!({"type":"mcp_tool", "server":"audit", "tool":"check", "input":input});
        let hooks = json!({"description":"native fixture", "hooks":{"SessionStart":[{"metadata":input, "hooks":[{"type":"command", "command":cmd}, mcp]}]}});
        fs::write(dir.path().join(HOOKS), serde_json::to_vec(&hooks).unwrap()).unwrap();
        let mut r = request(dir.path(), Operation::Install, true);
        r.migrate_kitsune = true;
        assert_eq!(succeeds(&r)["migrate_kitsune"], true);
        let edited = fs::read_to_string(dir.path().join(CONFIG)).unwrap();
        assert!(edited.contains(&payload));
        assert!(edited.contains(&state));
        let after: toml::Value = toml::from_str(&edited).unwrap();
        let group = &after["hooks"]["SessionStart"][0];
        assert_eq!(group["hooks"].as_array().unwrap().len(), 1);
        assert_eq!(
            group["hooks"][0],
            before["hooks"]["SessionStart"][0]["hooks"][1]
        );
        assert_eq!(
            group["metadata"],
            before["hooks"]["SessionStart"][0]["metadata"]
        );
        assert_eq!(after["hooks"]["state"], before["hooks"]["state"]);
        let edited_hooks: Value =
            serde_json::from_slice(&fs::read(dir.path().join(HOOKS)).unwrap()).unwrap();
        assert_eq!(
            edited_hooks["hooks"]["SessionStart"][0]["hooks"],
            json!([mcp])
        );
        assert_eq!(edited_hooks["hooks"]["SessionStart"][0]["metadata"], input);
        succeeds(&request(dir.path(), Operation::Uninstall, true));
        assert!(
            fs::read_to_string(dir.path().join(CONFIG))
                .unwrap()
                .contains(&state)
        );
        succeeds(&request(dir.path(), Operation::Rollback, true));
        assert_eq!(fs::read_to_string(dir.path().join(CONFIG)).unwrap(), config);
    }
}

#[test]
fn native_json_schema_rejects_bad_types_before_any_lock_or_journal() {
    let mut invalid = vec![
        json!([]),
        json!({"secret":"not a native root field"}),
        json!({"SessionStart":[]}),
        json!({"description":false}),
        json!({"hooks":null}),
        json!({"hooks":[]}),
        json!({"hooks":{"SessionStart":null}}),
        json!({"hooks":{"SessionStart":[null]}}),
        json!({"hooks":{"SessionStart":[{"matcher":[]}]}}),
        json!({"hooks":{"SessionStart":[{"matcher":false}]}}),
        json!({"hooks":{"SessionStart":[{"hooks":null}]}}),
    ];
    let mut handlers = vec![
        json!({"type":"command"}),
        json!({"type":false,"command":"true"}),
        json!({"type":"unsupported"}),
        json!({"type":"command","command":false}),
        json!({"type":"command","command":"true","commandWindows":"true","command_windows":"true"}),
        json!({"type":"mcp_tool","server":"audit"}),
        json!({"type":"mcp_tool","server":false,"tool":"check"}),
        json!({"type":"mcp_tool","server":"audit","tool":1}),
    ];
    for (key, value) in [
        ("timeout", json!("bad")),
        ("timeout", json!(-1)),
        ("timeout", json!(1.5)),
        ("timeout", json!(18446744073709551616.0)),
        ("async", json!(null)),
        ("async", json!("true")),
        ("async", json!(1)),
        ("commandWindows", json!(false)),
        ("command_windows", json!([])),
        ("statusMessage", json!(42)),
        ("additionalContextLimit", json!(-1)),
        ("additionalContextLimit", json!(1.5)),
        ("additionalContextLimit", json!("bad")),
    ] {
        let mut command = json!({"type":"command","command":"true"});
        command[key] = value;
        handlers.push(command);
    }
    for (key, value) in [
        ("input", json!(null)),
        ("input", json!([])),
        ("input", json!("bad")),
        ("input", json!({"nested":{"value":null}})),
        ("input", json!({"nested":[null]})),
        ("input", json!({"nested":[9223372036854775808_u64]})),
        ("timeout", json!("bad")),
        ("timeout", json!(-1)),
        ("statusMessage", json!(false)),
    ] {
        let mut mcp = json!({"type":"mcp_tool","server":"audit","tool":"check"});
        mcp[key] = value;
        handlers.push(mcp);
    }
    invalid.extend(
        handlers
            .into_iter()
            .map(|handler| json!({"hooks":{"SessionStart":[{"hooks":[handler]}]}})),
    );
    for event in [
        "PreModelRequest",
        "PostModelRequest",
        "PreToolUse",
        "PermissionRequest",
        "PostToolUse",
        "PreCompact",
        "PostCompact",
        "SessionStart",
        "SessionEnd",
        "UserPromptSubmit",
        "SubagentStart",
        "SubagentStop",
        "Stop",
        "Interrupt",
    ] {
        invalid.push(json!({"hooks":{event:false}}));
    }
    for value in invalid {
        let dir = fixture();
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(dir.path().join(HOOKS), &bytes).unwrap();
        for operation in [
            Operation::Plan,
            Operation::Install,
            Operation::Uninstall,
            Operation::Rollback,
        ] {
            refuses(
                &request(dir.path(), operation, true),
                "codex_hooks_invalid_hooks",
            );
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2, "{value}");
            assert_eq!(fs::read(dir.path().join(HOOKS)).unwrap(), bytes);
            assert_eq!(
                fs::read_to_string(dir.path().join(CONFIG)).unwrap(),
                CONFIG_PRE
            );
        }
    }
}

#[test]
fn native_toml_schema_rejects_bad_handlers_groups_and_real_state_without_artifacts() {
    for config in [
        "[hooks]\nSessionStart = [{hooks = [{type = 'command', command = 'true', timeout = 'bad'}]}]\n",
        "[hooks]\nSessionStart = [{hooks = [{type = 'command', command = 'true', async = 'true'}]}]\n",
        "[hooks]\nSessionStart = [{matcher = false}]\n",
        "[hooks]\nSessionStart = [{hooks = [{type = 'mcp_tool', server = false, tool = 'check'}]}]\n",
        "[hooks]\nSessionStart = [{hooks = [{type = 'mcp_tool', server = 'audit', tool = 'check', input = []}]}]\n",
        "[hooks]\nstate = false\n",
        "[hooks.state.\"key\"]\nenabled = 'false'\n",
        "[hooks.state.\"key\"]\ntrusted_hash = false\n",
        "[hooks.state.\"key\"]\ntrusted_content_hash = []\n",
    ] {
        let dir = fixture();
        fs::write(dir.path().join(CONFIG), config).unwrap();
        let hooks = fs::read(dir.path().join(HOOKS)).unwrap();
        refuses(
            &request(dir.path(), Operation::Install, true),
            "codex_hooks_invalid_hooks",
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        assert_eq!(fs::read(dir.path().join(HOOKS)).unwrap(), hooks);
        assert_eq!(fs::read_to_string(dir.path().join(CONFIG)).unwrap(), config);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn native_optional_fields_defaults_variants_and_ignored_metadata_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let custom = json!([
        {}, {"matcher":null},
        {"matcher":"custom", "metadata":{"keep":true}, "hooks":[
            {"type":"command","command":"custom","commandWindows":null,"timeout":null,"statusMessage":null,"additionalContextLimit":null},
            {"type":"command","command":"custom","command_windows":"custom.exe","timeout":18446744073709551615_u64,"async":true,"statusMessage":"checking","additionalContextLimit":0},
            {"type":"mcp_tool","server":"audit","tool":"check","timeout":null,"statusMessage":null,"input":{"type":"command","command":"bash '/unrelated/kitsune-agent-state.sh' session","keep":[true,1,-1,1.5,{"custom":"data"}]}},
            {"type":"mcp_tool","server":"audit","tool":"check"},
            {"type":"prompt","metadata":{"keep":true}}, {"type":"agent"}
        ]}
    ]);
    let original = json!({"description":null,"hooks":{"SessionStart":custom,"IgnoredNativeMetadata":{"keep":"unchanged"}}});
    fs::write(
        dir.path().join(HOOKS),
        serde_json::to_vec(&original).unwrap(),
    )
    .unwrap();
    let r = request(dir.path(), Operation::Install, true);
    assert_eq!(succeeds(&r)["migrate_kitsune"], false);
    let installed: Value =
        serde_json::from_slice(&fs::read(dir.path().join(HOOKS)).unwrap()).unwrap();
    let mut expected = original.clone();
    expected["hooks"]["SessionStart"]
        .as_array_mut()
        .unwrap()
        .push(
            installed["hooks"]["SessionStart"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()
                .clone(),
        );
    assert_eq!(installed, expected);
    succeeds(&request(dir.path(), Operation::Uninstall, true));
    let after: Value = serde_json::from_slice(&fs::read(dir.path().join(HOOKS)).unwrap()).unwrap();
    assert_eq!(after, original);
}
