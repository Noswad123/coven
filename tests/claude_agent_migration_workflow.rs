#![cfg(target_os = "linux")]
//! Executes generated Coven hooks, not native Claude or a provider certification.
//! Legacy Kit asset migration is covered separately by claude_hooks_workflow.

use coven::{
    agent_management::{self, InventoryRequest},
    claude_hooks::{self, Operation, Request},
};
use serde_json::{Value, json};
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
        "PROMPT-SECRET",
        "TRANSCRIPT-SECRET",
        "CWD-SECRET",
        "SETTINGS-SECRET",
        "CUSTOM-SECRET",
    ] {
        assert!(!text.contains(secret), "output leaked private fixture data");
    }
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
fn installed_native_commands_reach_private_receiver_and_rollback_exactly() {
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let workspace = private_dir(root.path());
    fs::write(workspace.path().join("coven.json"), b"marker").unwrap();
    let config = private_dir(root.path());
    let settings_path = config.path().join("settings.json");
    let preimage = br#"{ "credential": "SETTINGS-SECRET", "hooks": {"Stop": [{"matcher":"custom","hooks":[{"type":"command","command":"echo CUSTOM-SECRET"}]}]}, "other": [1,true] }
"#;
    fs::write(&settings_path, preimage).unwrap();
    fs::set_permissions(&settings_path, fs::Permissions::from_mode(0o640)).unwrap();
    let original: Value = serde_json::from_slice(preimage).unwrap();
    let socket = root.path().join("receiver.sock");
    let binary = env!("CARGO_BIN_EXE_coven");
    let stdout_path = root.path().join("serve.stdout");
    let stderr_path = root.path().join("serve.stderr");
    // Files avoid blocking the long-lived receiver on a full output pipe.
    let mut receiver = OwnedChild(Some(
        Command::new(binary)
            .args(["agent", "serve"])
            .arg(workspace.path())
            .arg("--socket")
            .arg(&socket)
            .args(["--duration", "60", "--json"])
            .env_clear()
            .env("HOME", root.path())
            .current_dir(workspace.path())
            .stdin(Stdio::null())
            .stdout(fs::File::create(&stdout_path).unwrap())
            .stderr(fs::File::create(&stderr_path).unwrap())
            .spawn()
            .unwrap(),
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !socket.exists() {
        assert!(
            receiver.0.as_mut().unwrap().try_wait().unwrap().is_none(),
            "receiver exited before binding"
        );
        assert!(Instant::now() < deadline, "receiver did not bind");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::metadata(&socket).unwrap().mode() & 0o7777, 0o600);
    // Binding alone precedes initialization; require the real core inventory RPC.
    let ready = agent_management::inventory_arguments(
        &InventoryRequest {
            socket: socket.to_str().unwrap().into(),
        },
        true,
    )
    .unwrap();
    assert_redacted(ready.stdout.as_bytes());
    assert_eq!(
        serde_json::from_str::<Value>(&ready.stdout).unwrap()["records"],
        json!([])
    );

    let mut request = Request {
        config_root: config.path().into(),
        executable: Some(binary.into()),
        socket: Some(socket.to_str().unwrap().into()),
        migrate_kitsune: false,
        apply: true,
        operation: Operation::Install,
    };
    let installed = claude_hooks::arguments(&request, true).unwrap();
    assert_eq!(installed.exit_code, 0);
    assert_redacted(installed.stdout.as_bytes());
    assert_eq!(
        serde_json::from_str::<Value>(&installed.stdout).unwrap()["status"],
        "installed"
    );
    let settings: Value = serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    assert_eq!(settings["credential"], original["credential"]);
    assert_eq!(settings["hooks"]["Stop"][0], original["hooks"]["Stop"][0]);
    assert_eq!(settings["other"], original["other"]);
    assert_eq!(fs::metadata(&settings_path).unwrap().mode() & 0o7777, 0o600);
    assert!(config.path().join("coven-claude-hooks.json").is_file());
    // Only this freshly generated, test-owned settings image is command authority.
    let generated = |event: &str| -> String {
        let commands: Vec<_> = settings["hooks"][event]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|entry| entry["hooks"].as_array().unwrap())
            .filter(|hook| hook["type"] == "command")
            .filter_map(|hook| hook["command"].as_str())
            .filter(|command| command.ends_with("# coven-managed:claude-hooks:v1"))
            .collect();
        assert_eq!(commands.len(), 1);
        commands[0].to_owned()
    };
    let prompt_command = generated("UserPromptSubmit");
    let stop_command = generated("Stop");
    let inventory = || -> Value {
        let output = Command::new(binary)
            .args(["agent", "inventory", "--socket"])
            .arg(&socket)
            .arg("--json")
            .env_clear()
            .env("HOME", root.path())
            .current_dir(workspace.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert_redacted(&output.stdout);
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let mut payload = json!({"session_id":"private-native-session", "hook_event_name":"UserPromptSubmit", "prompt":"PROMPT-SECRET", "transcript_path":"TRANSCRIPT-SECRET", "cwd":"CWD-SECRET"});
    run_hook(&prompt_command, &payload, root.path());
    let working = inventory();
    assert_eq!(working["records"].as_array().unwrap().len(), 1);
    assert_eq!(working["records"][0]["reported_state"], "working");
    assert_eq!(working["records"][0]["job_completion"], "unestablished");
    assert_eq!(working["authorization"], "none");
    let record_id = working["records"][0]["record_id"].as_str().unwrap();
    assert_eq!(record_id.len(), 64);
    assert!(record_id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let storage_path = workspace.path().join("state/agent-management.json");
    let saved = fs::read_to_string(&storage_path).unwrap();
    assert!(saved.contains("private-native-session"));
    assert!(!saved.contains("SECRET"));
    assert_eq!(fs::metadata(&storage_path).unwrap().mode() & 0o7777, 0o600);

    payload["hook_event_name"] = json!("Stop");
    run_hook(&stop_command, &payload, root.path());
    let idle = inventory();
    assert_eq!(idle["records"].as_array().unwrap().len(), 1);
    assert_eq!(idle["records"][0]["record_id"], record_id);
    assert_eq!(idle["records"][0]["reported_state"], "idle");
    assert_eq!(idle["records"][0]["job_completion"], "unestablished");
    let stopped = fs::read(&storage_path).unwrap();
    for rejected in [
        payload.clone(), // Stop payload mismatches the generated UserPromptSubmit flag.
        json!({"session_id":"private-native-session", "hook_event_name":"UserPromptSubmit", "agent_id":"subagent"}),
        json!({"session_id":"private-native-session", "hook_event_name":"SubagentStop"}),
    ] {
        run_hook(&prompt_command, &rejected, root.path());
        assert_eq!(fs::read(&storage_path).unwrap(), stopped);
        let unchanged = inventory();
        // age_ms advances naturally; all other record fields must stay identical.
        let mut record = unchanged["records"][0].clone();
        record["age_ms"] = idle["records"][0]["age_ms"].clone();
        assert_eq!(unchanged["records"].as_array().unwrap().len(), 1);
        assert_eq!(record, idle["records"][0]);
    }

    assert_eq!(
        unsafe { libc::kill(receiver.0.as_ref().unwrap().id() as i32, libc::SIGINT) },
        0
    );
    assert!(receiver.wait_bounded().success());
    assert!(!socket.exists(), "owned receiver socket must be removed");
    assert_redacted(&fs::read(&stdout_path).unwrap());
    assert!(fs::read(&stderr_path).unwrap().is_empty());
    payload["hook_event_name"] = json!("UserPromptSubmit");
    run_hook(&prompt_command, &payload, root.path());
    assert_eq!(fs::read(&storage_path).unwrap(), stopped);

    // Resolve the preimage from the actual receipt, without resupplying command paths.
    request.operation = Operation::Rollback;
    request.executable = None;
    request.socket = None;
    let rollback = claude_hooks::arguments(&request, true).unwrap();
    assert_eq!(rollback.exit_code, 0);
    assert_redacted(rollback.stdout.as_bytes());
    assert_eq!(
        serde_json::from_str::<Value>(&rollback.stdout).unwrap()["status"],
        "rolled_back"
    );
    assert_eq!(fs::read(&settings_path).unwrap(), preimage);
    assert_eq!(fs::metadata(&settings_path).unwrap().mode() & 0o7777, 0o640);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&settings_path).unwrap()).unwrap(),
        original
    );
    assert_eq!(
        fs::read(workspace.path().join("coven.json")).unwrap(),
        b"marker"
    );
}
