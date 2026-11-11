#![cfg(unix)]
use coven::claude_hooks::{Operation, Request, arguments};
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
fn fixture() -> (tempfile::TempDir, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let pre = br#"{ "credential": "PRIVATE-SECRET", "hooks": {"Stop": [{"matcher":"custom","hooks":[{"type":"command","command":"custom-secret-command"}]}]}, "other": [1,true] }"#.to_vec();
    fs::write(dir.path().join("settings.json"), &pre).unwrap();
    (dir, pre)
}
fn settings(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(root.join("settings.json")).unwrap()).unwrap()
}
fn succeeds(r: &Request) -> String {
    arguments(r, true).unwrap().stdout
}
fn refuses(r: &Request, code: &str) {
    assert_eq!(arguments(r, true).err().unwrap().code(), code);
}

#[test]
fn dry_runs_are_read_only_and_redacted() {
    let (dir, pre) = fixture();
    for op in [Operation::Plan, Operation::Install] {
        let text = succeeds(&request(dir.path(), op, false));
        assert!(!text.contains("PRIVATE-SECRET") && !text.contains("custom-secret-command"));
        assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), pre);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    succeeds(&request(dir.path(), Operation::Plan, true));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    let mut unspecified = request(dir.path(), Operation::Plan, false);
    unspecified.executable = None;
    unspecified.socket = None;
    assert!(succeeds(&unspecified).contains("planned_requires_executable_and_socket"));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[cfg(target_os = "linux")]
#[test]
fn setgid_root_does_not_leave_an_empty_recovery_blocker() {
    let (dir, preimage) = fixture();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o2700)).unwrap();
    succeeds(&request(dir.path(), Operation::Install, true));
    assert!(!dir.path().join(".coven-claude-hooks-recovery").exists());
    succeeds(&request(dir.path(), Operation::Rollback, true));
    assert_eq!(
        fs::read(dir.path().join("settings.json")).unwrap(),
        preimage
    );
    assert!(!dir.path().join(".coven-claude-hooks-recovery").exists());
    assert_eq!(
        fs::metadata(dir.path()).unwrap().permissions().mode() & 0o7777,
        0o2700
    );
}

#[test]
fn registered_cli_plan_is_required_and_read_only() {
    use std::process::Command;
    let binary = env!("CARGO_BIN_EXE_coven");
    let help = Command::new(binary)
        .args(["agent", "hooks", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success(), "registered hooks CLI is required");
    let (dir, pre) = fixture();
    let output = Command::new(binary)
        .args([
            "agent",
            "hooks",
            "plan",
            "--provider",
            "claude",
            "--config-root",
        ])
        .arg(dir.path())
        .args([
            "--executable",
            "/abs/coven",
            "--socket",
            "/abs/receiver",
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
    assert_eq!(value["provider"], "claude");
    assert_eq!(value["status"], "planned");
    assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), pre);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn install_repeat_uninstall_and_rollback() {
    for rollback in [false, true] {
        let (dir, pre) = fixture();
        let install = request(dir.path(), Operation::Install, true);
        succeeds(&install);
        let post = fs::read(dir.path().join("settings.json")).unwrap();
        assert!(succeeds(&install).contains("unchanged"));
        assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), post);
        let v = settings(dir.path());
        assert_eq!(v["credential"], "PRIVATE-SECRET");
        assert_eq!(
            v["hooks"]["Stop"][0]["hooks"][0]["command"],
            "custom-secret-command"
        );
        for event in [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "Stop",
            "SessionEnd",
        ] {
            let block = v["hooks"][event].as_array().unwrap().last().unwrap();
            assert_eq!(block["matcher"], "");
            assert_eq!(block["hooks"][0]["timeout"], 2);
            let cmd = block["hooks"][0]["command"].as_str().unwrap();
            assert!(cmd.starts_with("'/abs/coven'\"'\"'s binary' agent report --provider claude"));
            assert!(cmd.contains(&format!(
                "--event {event} >/dev/null 2>&1 || true # coven-managed:claude-hooks:v1"
            )));
            assert!(!cmd.contains("kitsune"));
        }
        for name in [
            "settings.json",
            "coven-claude-hooks.json",
            ".coven-claude-hooks.lock",
        ] {
            assert_eq!(
                fs::metadata(dir.path().join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let op = if rollback {
            Operation::Rollback
        } else {
            Operation::Uninstall
        };
        let finish = request(dir.path(), op, true);
        succeeds(&finish);
        assert!(succeeds(&finish).contains("unchanged"));
        if rollback {
            assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), pre);
        } else {
            let v = settings(dir.path());
            assert_eq!(v["credential"], "PRIVATE-SECRET");
            assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 1);
            assert!(v["hooks"]["SessionStart"].as_array().unwrap().is_empty());
        }
        succeeds(&install);
        assert!(succeeds(&install).contains("unchanged"));
    }
}

#[test]
fn conflicts_do_not_overwrite_user_edits() {
    let (dir, _) = fixture();
    succeeds(&request(dir.path(), Operation::Install, true));
    let mut v = settings(dir.path());
    v["user_edit"] = json!(true);
    let edit = serde_json::to_vec(&v).unwrap();
    fs::write(dir.path().join("settings.json"), &edit).unwrap();
    for op in [
        Operation::Plan,
        Operation::Install,
        Operation::Uninstall,
        Operation::Rollback,
    ] {
        refuses(&request(dir.path(), op, true), "claude_hooks_conflict");
        assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), edit);
    }
}

#[test]
fn unsafe_files_and_corrupt_json_are_refused_without_side_effects() {
    for raw in [
        b"{".as_slice(),
        br#"{"a":1,"a":2}"#,
        br#"{"hooks":null}"#,
        b"[]",
    ] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("settings.json"), raw).unwrap();
        assert!(arguments(&request(dir.path(), Operation::Plan, false), true).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    for hard in [false, true] {
        let (dir, _) = fixture();
        let settings = dir.path().join("settings.json");
        fs::rename(&settings, dir.path().join("original")).unwrap();
        if hard {
            fs::hard_link(dir.path().join("original"), &settings).unwrap();
        } else {
            symlink(dir.path().join("original"), &settings).unwrap();
        }
        refuses(
            &request(dir.path(), Operation::Plan, false),
            "claude_hooks_unsafe_leaf",
        );
    }
    let (dir, _) = fixture();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o777)).unwrap();
    refuses(
        &request(dir.path(), Operation::Plan, false),
        "claude_hooks_unsafe_root",
    );
}

#[test]
fn missing_root_symlink_root_directory_leaf_and_oversized_settings_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    assert!(arguments(&request(&missing, Operation::Plan, false), true).is_err());
    assert!(!missing.exists());
    symlink(dir.path(), dir.path().join("alias")).unwrap();
    refuses(
        &request(&dir.path().join("alias"), Operation::Plan, false),
        "claude_hooks_unsafe_root",
    );
    fs::create_dir(dir.path().join("settings.json")).unwrap();
    refuses(
        &request(dir.path(), Operation::Plan, false),
        "claude_hooks_unsafe_leaf",
    );
    fs::remove_dir(dir.path().join("settings.json")).unwrap();
    fs::write(
        dir.path().join("settings.json"),
        vec![b' '; 1024 * 1024 + 1],
    )
    .unwrap();
    refuses(
        &request(dir.path(), Operation::Plan, false),
        "claude_hooks_size_limit",
    );
    assert!(!dir.path().join(".coven-claude-hooks.lock").exists());
}

#[test]
fn suspicious_custom_commands_are_conflicts_never_ownership_evidence() {
    for command in [
        "echo coven-managed:claude-hooks:v1",
        "echo kitsune-agent-state.sh",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let raw = serde_json::to_vec(
            &json!({"hooks":{"Stop":[{"hooks":[{"type":"command","command":command}]}]}}),
        )
        .unwrap();
        fs::write(dir.path().join("settings.json"), &raw).unwrap();
        let mut r = request(dir.path(), Operation::Install, false);
        r.migrate_kitsune = true;
        assert!(arguments(&r, true).is_err());
        assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), raw);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn receipts_are_strict_private_data_not_path_or_command_authority() {
    let (dir, _) = fixture();
    succeeds(&request(dir.path(), Operation::Install, true));
    let receipt_path = dir.path().join("coven-claude-hooks.json");
    let raw = fs::read(&receipt_path).unwrap();
    let mut value: Value = serde_json::from_slice(&raw).unwrap();
    value["settings_path"] = json!("/must/not/be/touched");
    fs::write(&receipt_path, serde_json::to_vec(&value).unwrap()).unwrap();
    refuses(
        &request(dir.path(), Operation::Rollback, false),
        "claude_hooks_invalid_receipt",
    );
    fs::write(&receipt_path, raw).unwrap();
    fs::set_permissions(&receipt_path, fs::Permissions::from_mode(0o644)).unwrap();
    refuses(
        &request(dir.path(), Operation::Plan, false),
        "claude_hooks_unsafe_private_file",
    );
}

#[test]
fn missing_settings_rollback_restores_absence_and_pending_can_resume() {
    let dir = tempfile::tempdir().unwrap();
    succeeds(&request(dir.path(), Operation::Install, true));
    let path = dir.path().join("coven-claude-hooks.json");
    let mut receipt: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    receipt["stage"] = json!("pending_install");
    fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    assert!(succeeds(&request(dir.path(), Operation::Plan, false)).contains("recovery_required"));
    succeeds(&request(dir.path(), Operation::Install, true));
    succeeds(&request(dir.path(), Operation::Rollback, true));
    assert!(!dir.path().join("settings.json").exists());
}

#[test]
fn pending_before_image_resumes_and_existing_lock_is_respected() {
    use std::os::fd::AsRawFd;
    let (dir, pre) = fixture();
    succeeds(&request(dir.path(), Operation::Install, true));
    let receipt_path = dir.path().join("coven-claude-hooks.json");
    let mut receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    receipt["stage"] = json!("pending_install");
    fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    fs::write(dir.path().join("settings.json"), pre).unwrap();
    fs::set_permissions(
        dir.path().join("settings.json"),
        fs::Permissions::from_mode(receipt["pre_mode"].as_u64().unwrap() as u32),
    )
    .unwrap();
    succeeds(&request(dir.path(), Operation::Install, true));
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.path().join(".coven-claude-hooks.lock"))
        .unwrap();
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    refuses(
        &request(dir.path(), Operation::Uninstall, true),
        "claude_hooks_locked",
    );
    succeeds(&request(dir.path(), Operation::Uninstall, false));
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) }, 0);
    succeeds(&request(dir.path(), Operation::Uninstall, true));
}

#[test]
fn original_modes_are_restored_and_chmod_only_edits_conflict() {
    for mode in [0o400, 0o640] {
        let (dir, pre) = fixture();
        let path = dir.path().join("settings.json");
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        let install = request(dir.path(), Operation::Install, true);
        succeeds(&install);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
            0o600
        );
        let receipt_path = dir.path().join("coven-claude-hooks.json");
        let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
        assert_eq!(receipt["version"], 2);
        assert_eq!(receipt["pre_mode"], mode);
        assert_eq!(receipt["post_mode"], 0o600);
        assert_eq!(
            fs::metadata(&receipt_path).unwrap().permissions().mode() & 0o7777,
            0o600
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        for op in [
            Operation::Plan,
            Operation::Install,
            Operation::Uninstall,
            Operation::Rollback,
        ] {
            refuses(&request(dir.path(), op, true), "claude_hooks_conflict");
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        succeeds(&request(dir.path(), Operation::Rollback, true));
        assert_eq!(fs::read(&path).unwrap(), pre);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
            mode
        );
        assert!(succeeds(&request(dir.path(), Operation::Rollback, false)).contains("unchanged"));
        fs::set_permissions(
            &path,
            fs::Permissions::from_mode(if mode == 0o400 { 0o600 } else { 0o400 }),
        )
        .unwrap();
        refuses(
            &request(dir.path(), Operation::Rollback, false),
            "claude_hooks_conflict",
        );
    }
}

#[test]
fn pending_uninstall_and_rollback_resume_at_source_or_target_including_absence() {
    for op in [Operation::Uninstall, Operation::Rollback] {
        for at_target in [false, true] {
            for originally_present in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("settings.json");
                if originally_present {
                    fs::write(&path, b"{ \"original\": true }\n").unwrap();
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
                }
                succeeds(&request(dir.path(), Operation::Install, true));
                if at_target {
                    succeeds(&request(dir.path(), op, true));
                }
                let receipt_path = dir.path().join("coven-claude-hooks.json");
                let mut receipt: Value =
                    serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
                receipt["stage"] = json!(if op == Operation::Uninstall {
                    "pending_uninstall"
                } else {
                    "pending_rollback"
                });
                fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
                let receipt_before = fs::read(&receipt_path).unwrap();
                let settings_before = fs::read(&path).ok();
                assert!(
                    succeeds(&request(dir.path(), Operation::Plan, false))
                        .contains("recovery_required_apply_")
                );
                assert!(succeeds(&request(dir.path(), op, false)).contains("recovery_required"));
                assert_eq!(fs::read(&receipt_path).unwrap(), receipt_before);
                assert_eq!(fs::read(&path).ok(), settings_before);
                succeeds(&request(dir.path(), op, true));
                assert!(succeeds(&request(dir.path(), op, true)).contains("unchanged"));
                if op == Operation::Rollback {
                    assert_eq!(path.exists(), originally_present);
                    if originally_present {
                        assert_eq!(
                            fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
                            0o400
                        );
                    }
                }
                assert!(!dir.path().join(".coven-claude-hooks-recovery").exists());
            }
        }
    }
}

#[test]
fn recovery_artifact_blocks_cli_and_api_without_exposing_its_data() {
    let (dir, _) = fixture();
    let recovery = dir.path().join(".coven-claude-hooks-recovery");
    fs::create_dir(&recovery).unwrap();
    fs::set_permissions(&recovery, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(recovery.join("replacement"), b"PRIVATE-RECOVERED-EDIT").unwrap();
    for op in [
        Operation::Plan,
        Operation::Install,
        Operation::Uninstall,
        Operation::Rollback,
    ] {
        refuses(
            &request(dir.path(), op, true),
            "claude_hooks_recovery_required",
        );
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_coven"))
        .args([
            "agent",
            "hooks",
            "plan",
            "--provider",
            "claude",
            "--config-root",
        ])
        .arg(dir.path())
        .args(["--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"[ERROR] claude_hooks_recovery_required\n");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-RECOVERED-EDIT"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE-RECOVERED-EDIT"));
    assert_eq!(
        fs::read(recovery.join("replacement")).unwrap(),
        b"PRIVATE-RECOVERED-EDIT"
    );
}

#[test]
fn unknown_legacy_and_modified_assets_never_migrate() {
    for action in ["session", "unknown-historical-action"] {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("hooks/kitsune-agent-state.sh");
        fs::create_dir(dir.path().join("hooks")).unwrap();
        fs::write(&script, "modified script").unwrap();
        let cmd = format!("bash '{}' {action}", script.display());
        fs::write(dir.path().join("settings.json"),serde_json::to_vec(&json!({"hooks":{"SessionStart":[{"matcher":"*","hooks":[{"type":"command","command":cmd,"timeout":10}]}]}})).unwrap()).unwrap();
        let mut r = request(dir.path(), Operation::Install, false);
        if action == "session" {
            refuses(&r, "claude_hooks_migration_consent_required");
        }
        r.migrate_kitsune = true;
        refuses(&r, "claude_hooks_ambiguous_kitsune");
        assert!(!dir.path().join("coven-claude-hooks.json").exists());
    }
}

#[test]
fn generated_command_passes_native_stdin_and_is_best_effort_offline() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("coven's executable");
    fs::write(&exe, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CAPTURE\"\nIFS= read -r payload\nprintf '%s' \"$payload\" > \"$PAYLOAD\"\nexit 42\n").unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
    let mut r = request(dir.path(), Operation::Install, true);
    r.executable = Some(exe.to_str().unwrap().into());
    succeeds(&r);
    let v = settings(dir.path());
    let command = v["hooks"]["SessionStart"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    let mut child = Command::new("/bin/sh")
        .args(["-c", command])
        .env("CAPTURE", dir.path().join("args"))
        .env("PAYLOAD", dir.path().join("payload"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let payload = "{\"hook_event_name\":\"SessionStart\",\"session_id\":\"native-session\"}";
    writeln!(child.stdin.take().unwrap(), "{payload}").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    assert_eq!(
        fs::read_to_string(dir.path().join("payload")).unwrap(),
        payload
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("args")).unwrap(),
        "agent\nreport\n--provider\nclaude\n--socket\n/abs/receiver socket\n--event\nSessionStart\n"
    );
}

// Verbatim compatibility test data from Kitsune, Apache-2.0:
// https://github.com/Noswad123/kitsune, src/integration/assets/claude/kitsune-agent-state.sh
// License: https://www.apache.org/licenses/LICENSE-2.0
// Full license copy: fixtures/claude_hooks/KITSUNE_LICENSE.
// This fixture is never executed or installed by Coven.
const CURRENT_KIT_ASSET: &str = r##"#!/bin/sh
# installed by kitsune
# managed by kitsune; reinstalling or updating the integration overwrites this file.
# add custom hooks beside this file instead of editing it.
# KITSUNE_INTEGRATION_ID=claude
# KITSUNE_INTEGRATION_VERSION=7

set -eu

action="${1:-}"
hook_input_file="$(mktemp "${TMPDIR:-/tmp}/kitsune-claude-hook.XXXXXX")" || exit 0
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

source = "kitsune:claude"
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
is_subagent = bool(hook_input.get("agent_id"))
if is_subagent:
    raise SystemExit(0)
if hook_event_name == "SubagentStop":
    # SubagentStop is a completion event. Older Kitsune integrations mapped it
    # to durable working, but Claude recap/away-summary can emit it after the
    # main turn has already stopped. Never let it revive an idle pane.
    raise SystemExit(0)
request_id = f"{source}:{int(time.time() * 1000)}:{random.randrange(1_000_000):06d}"
report_seq = time.time_ns()
session_id = hook_input.get("session_id")
agent_session_id = session_id if isinstance(session_id, str) and session_id else None
transcript_path = hook_input.get("transcript_path")
agent_session_path = transcript_path if isinstance(transcript_path, str) and transcript_path else None
session_start_source = hook_input.get("source") if hook_event_name == "SessionStart" else None
if not isinstance(session_start_source, str) or not session_start_source:
    session_start_source = None
if agent_session_id:
    params = {
        "pane_id": pane_id,
        "source": source,
        "agent": "claude",
        "seq": report_seq,
        "agent_session_id": agent_session_id,
    }
    if agent_session_path:
        params["agent_session_path"] = agent_session_path
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

#[test]
fn pending_migration_revalidates_asset_only_at_uncommitted_source() {
    for changed in ["modified", "missing", "symlink", "directory_symlink"] {
        for at_target in [false, true] {
            let (dir, _) = fixture();
            let hooks = dir.path().join("hooks");
            fs::create_dir(&hooks).unwrap();
            let script = hooks.join("kitsune-agent-state.sh");
            fs::write(&script, CURRENT_KIT_ASSET).unwrap();
            let mut v = settings(dir.path());
            v["hooks"]["SessionStart"] = json!([{"matcher":"*","hooks":[{"type":"command","command":format!("bash '{}' session",script.display()),"timeout":10}]}]);
            let pre = serde_json::to_vec(&v).unwrap();
            let settings_path = dir.path().join("settings.json");
            fs::write(&settings_path, &pre).unwrap();
            let mut r = request(dir.path(), Operation::Install, true);
            r.migrate_kitsune = true;
            succeeds(&r);
            let receipt_path = dir.path().join("coven-claude-hooks.json");
            let mut receipt: Value =
                serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
            receipt["stage"] = json!("pending_install");
            fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
            if !at_target {
                fs::write(&settings_path, &pre).unwrap();
                fs::set_permissions(
                    &settings_path,
                    fs::Permissions::from_mode(receipt["pre_mode"].as_u64().unwrap() as u32),
                )
                .unwrap();
            }
            match changed {
                "modified" => fs::write(&script, "locally modified Kit script").unwrap(),
                "missing" => fs::remove_file(&script).unwrap(),
                "symlink" => {
                    fs::rename(&script, dir.path().join("retired-script")).unwrap();
                    symlink(dir.path().join("retired-script"), &script).unwrap();
                }
                "directory_symlink" => {
                    fs::rename(&hooks, dir.path().join("retired-hooks")).unwrap();
                    symlink(dir.path().join("retired-hooks"), &hooks).unwrap();
                }
                _ => unreachable!(),
            }
            if at_target {
                succeeds(&r);
            } else {
                let receipt_before = fs::read(&receipt_path).unwrap();
                assert!(arguments(&r, true).is_err());
                assert_eq!(fs::read(&settings_path).unwrap(), pre);
                assert_eq!(fs::read(&receipt_path).unwrap(), receipt_before);
                assert!(!dir.path().join(".coven-claude-hooks-recovery").exists());
            }
        }
    }
}

#[test]
fn verified_current_kit_migration_preserves_asset_and_uninstall_never_restores_kit() {
    for rollback in [false, true] {
        let (dir, _) = fixture();
        fs::create_dir(dir.path().join("hooks")).unwrap();
        let script = dir.path().join("hooks/kitsune-agent-state.sh");
        fs::write(&script, CURRENT_KIT_ASSET).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let mut v = settings(dir.path());
        v["hooks"]["SessionStart"] = json!([{"matcher":"*","hooks":[{"type":"command","command":format!("bash '{}' session",script.display()),"timeout":10}]}]);
        let pre = serde_json::to_vec(&v).unwrap();
        fs::write(dir.path().join("settings.json"), &pre).unwrap();
        let mut r = request(dir.path(), Operation::Install, true);
        r.migrate_kitsune = true;
        succeeds(&r);
        assert!(
            !fs::read_to_string(dir.path().join("settings.json"))
                .unwrap()
                .contains("kitsune")
        );
        assert_eq!(fs::read_to_string(&script).unwrap(), CURRENT_KIT_ASSET);
        r.operation = if rollback {
            Operation::Rollback
        } else {
            Operation::Uninstall
        };
        succeeds(&r);
        if rollback {
            assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), pre);
        } else {
            assert!(
                !fs::read_to_string(dir.path().join("settings.json"))
                    .unwrap()
                    .contains("kitsune")
            );
        }
        assert_eq!(fs::read_to_string(&script).unwrap(), CURRENT_KIT_ASSET);
    }
}
