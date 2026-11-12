#![cfg(any(target_os = "linux", target_os = "macos"))]
//! Executes a generated Coven client via sh, not native Copilot or native approval.
//! Contract evidence is documentation only: github/docs at
//! 7c3387d3b513ed43ea26c9b2e4c0e1b12234c010, content/copilot/reference/
//! hooks-reference.md and copilot-cli-reference/cli-config-dir-reference.md.
//! No native providers, Kitsune assets, or user settings participate.

use coven::agent_management::{self, InventoryRequest, ReportRequest};
#[path = "support/physical_temp.rs"]
mod physical_temp;
use physical_temp::tempdir;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

struct OwnedChild(Option<Child>);

impl OwnedChild {
    fn wait_bounded(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.0.as_mut().unwrap().try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "child did not exit in time");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn private_dir(parent: &Path) -> tempfile::TempDir {
    let dir = tempfile::tempdir_in(parent).unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(fs::metadata(dir.path()).unwrap().mode() & 0o7777, 0o700);
    assert_eq!(fs::metadata(dir.path()).unwrap().uid(), unsafe {
        libc::geteuid()
    });
    dir
}

fn assert_redacted(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    for secret in [
        "private-native-session",
        "SECRET",
        "session_id",
        "sessionId",
        "prompt",
        "transcript_path",
        "cwd",
        "permission_mode",
        "agent_id",
    ] {
        assert!(!text.contains(secret), "output leaked private fixture data");
    }
}

fn cli(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_coven"));
    command.env_clear().env("HOME", home).current_dir(home);
    command
}

fn run_hook(command: &str, payload: &Value, home: &Path) {
    let mut child = OwnedChild(Some(
        Command::new("/bin/sh")
            .args(["-c", command])
            .env_clear()
            .env("HOME", home)
            .current_dir(home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    child
        .0
        .as_mut()
        .unwrap()
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    assert!(child.wait_bounded().success());
    let output = child.0.take().unwrap().wait_with_output().unwrap();
    assert!(output.stdout.is_empty(), "hook must be silent");
    assert!(
        output.stderr.is_empty(),
        "hook must be best-effort and silent"
    );
}

#[test]
fn registered_copilot_hook_reaches_private_receiver_and_rolls_back_exact_settings() {
    let root = tempdir();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let workspace = private_dir(root.path());
    fs::write(workspace.path().join("coven.json"), b"marker").unwrap();
    let config = private_dir(root.path());
    let settings_path = config.path().join("settings.json");
    let preimage = br#"{ "description":"DESCRIPTION-SECRET", "disableAllHooks":true,
  "sandbox":{"enabled":true}, "trust":{"custom":"TRUST-SECRET"},
  "hooks":{"sessionStart":[{"bash":"echo CUSTOM-SECRET"}],"Stop":[{"bash":"echo STOP-SECRET"}]},
  "unknown":[1,true] }
"#;
    fs::write(&settings_path, preimage).unwrap();
    fs::set_permissions(&settings_path, fs::Permissions::from_mode(0o640)).unwrap();
    let original: Value = serde_json::from_slice(preimage).unwrap();
    let socket = root.path().join("receiver.sock");
    let installed = cli(root.path())
        .args([
            "agent",
            "hooks",
            "install",
            "--provider",
            "copilot",
            "--config-root",
        ])
        .arg(config.path())
        .args(["--executable", env!("CARGO_BIN_EXE_coven"), "--socket"])
        .arg(&socket)
        .args(["--apply", "--json"])
        .output()
        .unwrap();
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    assert!(installed.stderr.is_empty());
    assert_redacted(&installed.stdout);
    let installed: Value = serde_json::from_slice(&installed.stdout).unwrap();
    assert_eq!(installed["provider"], "copilot");
    assert_eq!(installed["status"], "installed");
    assert_eq!(installed["migrate_kitsune"], false);
    assert_eq!(
        installed["native_contract"],
        "github/docs@7c3387d3b513ed43ea26c9b2e4c0e1b12234c010"
    );
    assert!(
        installed["native_hook_approval"]
            .as_str()
            .unwrap()
            .contains("not granted")
    );
    let settings: Value = serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    let mut preserved = settings.clone();
    preserved["hooks"]
        .as_object_mut()
        .unwrap()
        .remove("SessionStart");
    assert_eq!(
        preserved, original,
        "native policy and custom hooks must survive"
    );
    let generated = settings["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0]["type"], "command");
    assert_eq!(generated[0]["timeoutSec"], 10);
    assert!(generated[0].get("hooks").is_none());
    assert!(generated[0].get("matcher").is_none());
    // Only the freshly generated manager-owned image is shell command authority.
    let command = generated[0]["bash"].as_str().unwrap();
    assert!(command.contains("agent report --provider copilot"));
    assert!(command.ends_with("# coven-managed:copilot-hooks:v1"));
    for name in [
        "settings.json",
        "coven-copilot-hooks.json",
        ".coven-copilot-hooks.lock",
    ] {
        assert_eq!(
            fs::metadata(config.path().join(name)).unwrap().mode() & 0o7777,
            0o600
        );
    }

    let stdout_path = root.path().join("serve.stdout");
    let stderr_path = root.path().join("serve.stderr");
    // Files avoid blocking the long-lived receiver on full output pipes.
    let mut receiver = OwnedChild(Some(
        cli(root.path())
            .args(["agent", "serve"])
            .arg(workspace.path())
            .arg("--socket")
            .arg(&socket)
            .args(["--duration", "60", "--json"])
            .current_dir(workspace.path())
            .stdin(Stdio::null())
            .stdout(fs::File::create(&stdout_path).unwrap())
            .stderr(fs::File::create(&stderr_path).unwrap())
            .spawn()
            .unwrap(),
    ));
    let inventory_request = InventoryRequest {
        socket: socket.to_str().unwrap().into(),
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(ready) = agent_management::inventory_arguments(&inventory_request, true) {
            assert_redacted(ready.stdout.as_bytes());
            assert_eq!(
                serde_json::from_str::<Value>(&ready.stdout).unwrap()["records"],
                json!([])
            );
            break;
        }
        assert!(
            receiver.0.as_mut().unwrap().try_wait().unwrap().is_none(),
            "receiver exited before readiness"
        );
        assert!(Instant::now() < deadline, "receiver did not become ready");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::metadata(&socket).unwrap().mode() & 0o7777, 0o600);
    assert_eq!(fs::metadata(&socket).unwrap().uid(), unsafe {
        libc::geteuid()
    });
    let inventory = || -> Value {
        let output = cli(root.path())
            .args(["agent", "inventory", "--socket"])
            .arg(&socket)
            .arg("--json")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert_redacted(&output.stdout);
        serde_json::from_slice(&output.stdout).unwrap()
    };
    // Documentation-shaped input, not evidence of native delivery. Unknown data
    // must be stripped before transport and persistence, not merely hidden in UI.
    let payload = json!({"session_id":"private-native-session", "hook_event_name":"SessionStart",
        "timestamp":"TIMESTAMP-SECRET", "cwd":"CWD-SECRET", "source":"SOURCE-SECRET",
        "prompt":"PROMPT-SECRET", "transcript_path":"TRANSCRIPT-SECRET",
        "permission_mode":"PERMISSION-SECRET", "unknown":{"secret":"UNKNOWN-SECRET"},
        "agent_id":{"private":"AGENT-SECRET"}});
    run_hook(command, &payload, root.path());
    let copilot_view = inventory();
    assert_eq!(copilot_view["authorization"], "none");
    assert_eq!(copilot_view["records"].as_array().unwrap().len(), 1);
    let copilot = &copilot_view["records"][0];
    assert_eq!(copilot["provider"], "copilot");
    assert_eq!(copilot["last_event"], "SessionStart");
    assert_eq!(copilot["reported_state"], "unknown");
    assert!(copilot["age_ms"].is_u64());
    assert_eq!(copilot["native_identity"], "unestablished");
    assert_eq!(copilot["job_completion"], "unestablished");
    assert_eq!(copilot["reference_trust"], "caller_supplied_unverified");
    assert_eq!(
        copilot["record_id"],
        format!("{:x}", Sha256::digest(b"copilot\0private-native-session"))
    );
    let storage_path = workspace.path().join("state/agent-management.json");
    let saved = fs::read_to_string(&storage_path).unwrap();
    assert!(saved.contains("private-native-session"));
    for forbidden in [
        "SECRET",
        "prompt",
        "transcript_path",
        "cwd",
        "permission_mode",
        "source",
        "\"timestamp\"",
        "unknown",
        "agent_id",
    ] {
        assert!(
            !saved.contains(forbidden),
            "persisted unknown input: {forbidden}"
        );
    }
    assert_eq!(fs::metadata(&storage_path).unwrap().mode() & 0o7777, 0o600);

    for provider in ["claude", "codex"] {
        let report = agent_management::report_payload(
            &ReportRequest {
                socket: inventory_request.socket.clone(),
                provider: provider.into(),
                event: Some("SessionStart".into()),
            },
            br#"{"session_id":"private-native-session","hook_event_name":"SessionStart"}"#,
            true,
        )
        .unwrap();
        assert_redacted(report.stdout.as_bytes());
        let report: Value = serde_json::from_str(&report.stdout).unwrap();
        assert_eq!(
            report["record_id"],
            format!(
                "{:x}",
                Sha256::digest(format!("{provider}\0private-native-session").as_bytes())
            )
        );
        assert_ne!(report["record_id"], copilot["record_id"]);
    }
    let all = inventory();
    let records = all["records"].as_array().unwrap();
    assert_eq!(records.len(), 3);
    for provider in ["claude", "codex", "copilot"] {
        let record = records
            .iter()
            .find(|record| record["provider"] == provider)
            .unwrap();
        assert_eq!(
            record["record_id"],
            format!(
                "{:x}",
                Sha256::digest(format!("{provider}\0private-native-session").as_bytes())
            )
        );
        assert_eq!(record["last_event"], "SessionStart");
        assert_eq!(record["reported_state"], "unknown");
        assert!(record["age_ms"].is_u64());
    }
    let committed = fs::read(&storage_path).unwrap();
    for rejected in [
        json!({"session_id":"private-native-session", "hook_event_name":"sessionStart"}),
        json!({"sessionId":"private-native-session", "hook_event_name":"SessionStart"}),
        json!({"session_id":"private-native-session", "hookEventName":"SessionStart"}),
        json!({"session_id":"private-native-session"}),
        json!({"session_id":"private-native-session", "hook_event_name":"Stop"}),
        json!({"session_id":"private-native-session", "hook_event_name":"SessionEnd"}),
    ] {
        // Neither an explicit CLI event nor its absence may repair malformed input
        // or broaden the provider's accepted lifecycle.
        for event in [Some("SessionStart".into()), None] {
            assert!(
                agent_management::report_payload(
                    &ReportRequest {
                        socket: inventory_request.socket.clone(),
                        provider: "copilot".into(),
                        event,
                    },
                    rejected.to_string().as_bytes(),
                    true,
                )
                .is_err()
            );
            assert_eq!(fs::read(&storage_path).unwrap(), committed);
        }
        run_hook(command, &rejected, root.path());
        assert_eq!(fs::read(&storage_path).unwrap(), committed);
        let mut unchanged = inventory();
        for (record, previous) in unchanged["records"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .zip(records)
        {
            record["age_ms"] = previous["age_ms"].clone();
        }
        assert_eq!(unchanged, all);
    }

    assert_eq!(
        unsafe { libc::kill(receiver.0.as_ref().unwrap().id() as i32, libc::SIGINT) },
        0
    );
    assert!(receiver.wait_bounded().success());
    assert!(!socket.exists(), "owned receiver socket must be removed");
    assert_redacted(&fs::read(&stdout_path).unwrap());
    assert!(fs::read(&stderr_path).unwrap().is_empty());
    run_hook(command, &payload, root.path());
    assert_eq!(fs::read(&storage_path).unwrap(), committed);

    // The actual receipt supplies rollback preimages, without command overrides.
    let rollback = cli(root.path())
        .args([
            "agent",
            "hooks",
            "rollback",
            "--provider",
            "copilot",
            "--config-root",
        ])
        .arg(config.path())
        .args(["--apply", "--json"])
        .output()
        .unwrap();
    assert!(
        rollback.status.success(),
        "{}",
        String::from_utf8_lossy(&rollback.stderr)
    );
    assert!(rollback.stderr.is_empty());
    assert_redacted(&rollback.stdout);
    assert_eq!(
        serde_json::from_slice::<Value>(&rollback.stdout).unwrap()["status"],
        "rolled_back"
    );
    assert_eq!(fs::read(&settings_path).unwrap(), preimage);
    assert_eq!(fs::metadata(&settings_path).unwrap().mode() & 0o7777, 0o640);
    assert_eq!(
        fs::read(workspace.path().join("coven.json")).unwrap(),
        b"marker"
    );
}
