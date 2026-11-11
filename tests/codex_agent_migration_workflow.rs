#![cfg(target_os = "linux")]
//! Executes a generated Coven client via sh, not native Codex or native approval.
//! No native providers, Kit assets, or user configuration participate in this test.

use coven::agent_management::{self, InventoryRequest, ReportRequest};
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
        "PRIVATE-THREAD",
        "SECRET",
        "session_id",
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
fn registered_codex_hooks_reach_private_receiver_and_rollback_both_native_files() {
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let workspace = private_dir(root.path());
    fs::write(workspace.path().join("coven.json"), b"marker").unwrap();
    let config = private_dir(root.path());
    let hooks_path = config.path().join("hooks.json");
    let config_path = config.path().join("config.toml");
    let hooks_preimage = br#"{ "description": "DESCRIPTION-SECRET", "hooks": {"Stop": [{"matcher":"custom","hooks":[{"type":"command","command":"echo CUSTOM-SECRET"}]}]} }
"#;
    let config_preimage = "# native configuration comment\nmodel = 'MODEL-SECRET'\n\n[features] # global flags\nhooks = false # preserve feature comment\ncodex_hooks = true\nother = true\n\n[hooks.state.\"key\"]\nenabled = false # user-managed disposition\ntrusted_hash = 'TRUST-SECRET' # user-managed approval\ntrusted_content_hash = 'CONTENT-SECRET'\n";
    fs::write(&hooks_path, hooks_preimage).unwrap();
    fs::write(&config_path, config_preimage).unwrap();
    fs::set_permissions(&hooks_path, fs::Permissions::from_mode(0o640)).unwrap();
    fs::set_permissions(&config_path, fs::Permissions::from_mode(0o400)).unwrap();
    let original_hooks: Value = serde_json::from_slice(hooks_preimage).unwrap();
    let original_config: toml::Value = toml::from_str(config_preimage).unwrap();
    let socket = root.path().join("receiver.sock");
    let binary = env!("CARGO_BIN_EXE_coven");

    // Registration is required, never a skip if the second provider is absent.
    let help = cli(root.path())
        .args(["agent", "hooks", "install", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("[possible values: claude, codex]"));
    let installed = cli(root.path())
        .args([
            "agent",
            "hooks",
            "install",
            "--provider",
            "codex",
            "--config-root",
        ])
        .arg(config.path())
        .args(["--executable", binary, "--socket"])
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
    assert_eq!(installed["provider"], "codex");
    assert_eq!(installed["status"], "installed");
    assert_eq!(installed["features"]["hooks_after"], true);
    let hooks: Value = serde_json::from_slice(&fs::read(&hooks_path).unwrap()).unwrap();
    assert_eq!(hooks["description"], original_hooks["description"]);
    assert_eq!(hooks["hooks"]["Stop"], original_hooks["hooks"]["Stop"]);
    assert_eq!(hooks["hooks"].as_object().unwrap().len(), 2);
    let generated = hooks["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(generated.len(), 1);
    assert!(generated[0].get("matcher").is_none());
    assert_eq!(generated[0]["hooks"].as_array().unwrap().len(), 1);
    assert_eq!(generated[0]["hooks"][0]["type"], "command");
    assert_eq!(generated[0]["hooks"][0]["timeout"], 2);
    // Only the freshly generated manager-owned image is shell command authority.
    let command = generated[0]["hooks"][0]["command"].as_str().unwrap();
    assert!(command.contains("agent report --provider codex"));
    assert!(command.ends_with("# coven-managed:codex-hooks:v1"));
    let edited_config = fs::read_to_string(&config_path).unwrap();
    assert_eq!(
        edited_config,
        config_preimage
            .replacen("hooks = false", "hooks = true", 1)
            .replacen("codex_hooks = true\n", "", 1)
    );
    let edited: toml::Value = toml::from_str(&edited_config).unwrap();
    assert_eq!(edited["features"]["hooks"].as_bool(), Some(true));
    assert!(edited["features"].get("codex_hooks").is_none());
    assert_eq!(edited["hooks"]["state"], original_config["hooks"]["state"]);
    assert_eq!(
        edited["hooks"], original_config["hooks"],
        "must not fabricate native approval"
    );
    for path in [
        &hooks_path,
        &config_path,
        &config.path().join("coven-codex-hooks.json"),
    ] {
        assert_eq!(fs::metadata(path).unwrap().mode() & 0o7777, 0o600);
    }

    let stdout_path = root.path().join("serve.stdout");
    let stderr_path = root.path().join("serve.stderr");
    // Files avoid blocking the long-lived server on full output pipes.
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
    // openai/codex rust-v0.167.1 SessionStart command input's seven fields,
    // plus unknown secrets that must be discarded before transport/storage.
    let payload = json!({"session_id":"private-native-session", "hook_event_name":"SessionStart",
        "source":"startup", "cwd":"CWD-SECRET", "model":"MODEL-SECRET",
        "permission_mode":"default", "transcript_path":"TRANSCRIPT-SECRET",
        "prompt":"PROMPT-SECRET", "unknown":{"secret":"UNKNOWN-SECRET"},
        "agent_id":{"private":"AGENT-SECRET"}});
    run_hook(command, &payload, root.path());
    let codex_view = inventory();
    assert_eq!(codex_view["authorization"], "none");
    assert_eq!(codex_view["records"].as_array().unwrap().len(), 1);
    let codex = &codex_view["records"][0];
    assert_eq!(codex["provider"], "codex");
    assert_eq!(codex["last_event"], "SessionStart");
    assert_eq!(codex["reported_state"], "unknown");
    assert!(codex["age_ms"].is_u64());
    assert_eq!(codex["native_identity"], "unestablished");
    assert_eq!(codex["job_completion"], "unestablished");
    assert_eq!(codex["reference_trust"], "caller_supplied_unverified");
    assert_eq!(
        codex["record_id"],
        format!("{:x}", Sha256::digest(b"codex\0private-native-session"))
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
        "agent_id",
    ] {
        assert!(!saved.contains(forbidden));
    }
    assert_eq!(fs::metadata(&storage_path).unwrap().mode() & 0o7777, 0o600);

    let claude = agent_management::report_payload(
        &ReportRequest {
            socket: inventory_request.socket.clone(),
            provider: "claude".into(),
            event: Some("SessionStart".into()),
        },
        br#"{"session_id":"private-native-session","hook_event_name":"SessionStart"}"#,
        true,
    )
    .unwrap();
    assert_redacted(claude.stdout.as_bytes());
    let claude: Value = serde_json::from_str(&claude.stdout).unwrap();
    // Preserve the established Claude hash while namespacing the second provider.
    assert_eq!(
        claude["record_id"],
        format!("{:x}", Sha256::digest(b"claude\0private-native-session"))
    );
    assert_ne!(claude["record_id"], codex["record_id"]);
    let both = inventory();
    assert_eq!(both["records"].as_array().unwrap().len(), 2);
    for (provider, id) in [
        ("claude", &claude["record_id"]),
        ("codex", &codex["record_id"]),
    ] {
        let record = both["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["provider"] == provider)
            .unwrap();
        assert_eq!(&record["record_id"], id);
        assert_eq!(record["last_event"], "SessionStart");
        assert_eq!(record["reported_state"], "unknown");
        assert!(record["age_ms"].is_u64());
    }
    let committed = fs::read(&storage_path).unwrap();
    let mut wrong_event = payload.clone();
    wrong_event["hook_event_name"] = json!("Stop");
    for rejected in [
        wrong_event,
        json!({"thread_id":"PRIVATE-THREAD", "hook_event_name":"SessionStart"}),
    ] {
        assert!(
            agent_management::report_payload(
                &ReportRequest {
                    socket: inventory_request.socket.clone(),
                    provider: "codex".into(),
                    event: Some("SessionStart".into())
                },
                rejected.to_string().as_bytes(),
                true,
            )
            .is_err()
        );
        run_hook(command, &rejected, root.path());
        assert_eq!(fs::read(&storage_path).unwrap(), committed);
        let mut unchanged = inventory();
        for (record, previous) in unchanged["records"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .zip(both["records"].as_array().unwrap())
        {
            record["age_ms"] = previous["age_ms"].clone();
        }
        assert_eq!(unchanged, both);
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
            "codex",
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
    assert_eq!(fs::read(&hooks_path).unwrap(), hooks_preimage);
    assert_eq!(fs::read(&config_path).unwrap(), config_preimage.as_bytes());
    assert_eq!(fs::metadata(&hooks_path).unwrap().mode() & 0o7777, 0o640);
    assert_eq!(fs::metadata(&config_path).unwrap().mode() & 0o7777, 0o400);
    assert_eq!(
        fs::read(workspace.path().join("coven.json")).unwrap(),
        b"marker"
    );
}
