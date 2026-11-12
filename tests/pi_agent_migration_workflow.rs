#![cfg(any(target_os = "linux", target_os = "macos"))]
//! Runs the actual generated TypeScript under Node, not native Pi activation.
//! No native providers, Kitsune process, or user settings participate.

use coven::agent_management::{self, InventoryRequest, ReportRequest};
#[path = "support/physical_temp.rs"]
mod physical_temp;
use physical_temp::tempdir;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        process::CommandExt,
    },
    path::Path,
    process::{Child, Command, ExitStatus, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const NEW: &str = "coven-agent-reference.ts";
const OLD: &str = "kitsune-agent-state.ts";
const RECEIPT: &str = "coven-pi-extension.json";
const LEGACY: &[u8] = include_bytes!("fixtures/pi_extension/kitsune-agent-state.ts");
const SID: &str = "private-native-session";

struct OwnedChild(Option<Child>);

impl OwnedChild {
    fn spawn(command: &mut Command) -> Self {
        // Own the process group too: a failing Rust assertion must not orphan
        // the real CLI reporter spawned by the Node fixture.
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Self(Some(command.spawn().unwrap()))
    }

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
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn output(command: &mut Command, payload: Option<&Value>) -> Output {
    let mut child = OwnedChild::spawn(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );
    let mut stdin = child.0.as_mut().unwrap().stdin.take().unwrap();
    if let Some(payload) = payload {
        // Parser-level rejection can close stdin before the fixture writes it.
        if let Err(error) = stdin.write_all(payload.to_string().as_bytes()) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
    }
    drop(stdin);
    child.wait_bounded();
    // Keep the owner alive until output collection has completed.
    let result = child.0.as_mut().unwrap();
    use std::io::Read;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    result
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    result
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    Output {
        status: result.wait().unwrap(),
        stdout,
        stderr,
    }
}

fn private_dir(parent: &Path) -> tempfile::TempDir {
    let dir = tempfile::tempdir_in(parent).unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_private(dir.path(), 0o700);
    dir
}

fn assert_private(path: &Path, mode: u32) {
    let metadata = fs::metadata(path).unwrap();
    assert_eq!(metadata.mode() & 0o7777, mode);
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
}

fn assert_redacted(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    for secret in [
        SID,
        "SECRET",
        "session_id",
        "sessionId",
        "prompt",
        "cwd",
        "transcript_path",
        "permission_mode",
        "agent_id",
    ] {
        assert!(!text.contains(secret), "output leaked {secret}");
    }
}

fn cli(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_coven"));
    command.env_clear().env("HOME", home).current_dir(home);
    command
}

fn extension_cli(home: &Path, extensions: &Path, operation: &str) -> Command {
    let mut command = cli(home);
    command
        .args([
            "agent",
            "extensions",
            operation,
            "--provider",
            "pi",
            "--extension-root",
        ])
        .arg(extensions)
        .arg("--json");
    command
}

fn migration_result(result: Output, status: &str) -> Value {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_redacted(&result.stdout);
    assert_redacted(&result.stderr);
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["provider"], "pi");
    assert_eq!(value["status"], status);
    assert_eq!(value["native_contract"], coven::pi_extension::NATIVE_PIN);
    let warning = value["migration_warning"].as_str().unwrap();
    assert!(warning.contains("working/blocked/idle"));
    assert!(warning.contains("session references only"));
    assert!(String::from_utf8_lossy(&result.stderr).contains(warning));
    assert!(value["activation"].as_str().unwrap().contains("not proof"));
    value
}

fn inventory(home: &Path, socket: &Path) -> Value {
    let result = output(
        cli(home)
            .args(["agent", "inventory", "--socket"])
            .arg(socket)
            .arg("--json"),
        None,
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    assert_redacted(&result.stdout);
    serde_json::from_slice(&result.stdout).unwrap()
}

// Select the test runtime before clearing child environment. This dependency is
// confined to the fixture, not the Rust application or generated extension pins.
fn node_runtime() -> std::path::PathBuf {
    let candidate = std::env::var_os("COVEN_TEST_NODE")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("PATH").and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|dir| dir.join("node"))
                    .find(|path| path.is_file())
            })
        })
        .expect("Pi generated-asset fixture requires Node >=22.19; set COVEN_TEST_NODE or PATH");
    candidate
        .canonicalize()
        .expect("selected Node test runtime must exist")
}

fn run_generated(node: &Path, home: &Path, wrapper: &Path, generated: &Path) {
    let result = output(
        Command::new(node)
            .env_clear()
            .env("HOME", home)
            .current_dir(home)
            .arg("--no-warnings")
            .arg(wrapper)
            .arg(generated),
        None,
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        result.stdout.is_empty(),
        "generated callback must be silent"
    );
    assert!(
        result.stderr.is_empty(),
        "generated callback must be best-effort and silent"
    );
}

#[test]
fn generated_pi_migration_reaches_receiver_rejects_native_reports_and_rolls_back_after_uninstall() {
    let node = node_runtime();
    let root = tempdir();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_private(root.path(), 0o700);
    let workspace = private_dir(root.path());
    let extensions = private_dir(root.path());
    fs::write(workspace.path().join("coven.json"), b"marker").unwrap();
    let old = extensions.path().join(OLD);
    let generated = extensions.path().join(NEW);
    fs::write(&old, LEGACY).unwrap();
    fs::set_permissions(&old, fs::Permissions::from_mode(0o640)).unwrap();
    let socket = root.path().join("receiver.sock");
    assert!(socket.is_absolute() && extensions.path().is_absolute());

    for operation in ["plan", "install"] {
        let mut command = extension_cli(root.path(), extensions.path(), operation);
        command
            .args(["--executable", env!("CARGO_BIN_EXE_coven"), "--socket"])
            .arg(&socket)
            .arg("--migrate-kitsune");
        migration_result(output(&mut command, None), "planned");
        assert_eq!(fs::read(&old).unwrap(), LEGACY);
        assert_private(&old, 0o640);
        assert_eq!(fs::read_dir(extensions.path()).unwrap().count(), 1);
    }
    let mut install = extension_cli(root.path(), extensions.path(), "install");
    install
        .args(["--executable", env!("CARGO_BIN_EXE_coven"), "--socket"])
        .arg(&socket)
        .args(["--migrate-kitsune", "--apply"]);
    migration_result(output(&mut install, None), "installed");
    assert!(!old.exists());
    for name in [NEW, RECEIPT, ".coven-pi-extension.lock"] {
        assert_private(&extensions.path().join(name), 0o600);
    }
    let receipt_path = extensions.path().join(RECEIPT);
    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    let original = receipt["original"].clone();
    assert!(!original.is_null());
    let installed_bytes = fs::read(&generated).unwrap();
    let receipt_bytes = fs::read(&receipt_path).unwrap();

    let wrapper = root.path().join("run-generated.mjs");
    fs::write(&wrapper, r#"
import assert from 'node:assert/strict';
import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
import { pathToFileURL } from 'node:url';
const [major, minor] = process.versions.node.split('.').map(Number);
assert.ok(major > 22 || (major === 22 && minor >= 19), 'fixture requires Node >=22.19 with TypeScript loading');
const realSpawn = childProcess.spawn;
const children = [];
childProcess.spawn = (...args) => {
  const child = realSpawn(...args);
  const closed = new Promise(resolve => child.once('close', resolve));
  children.push({ child, closed });
  return child;
};
syncBuiltinESMExports();
const callbacks = new Map();
let timer;
try {
  const extension = (await import(pathToFileURL(process.argv[2]).href)).default;
  extension({ on(name, callback) {
    assert.ok(!callbacks.has(name));
    callbacks.set(name, callback);
  } });
  assert.deepEqual([...callbacks.keys()], ['session_start', 'session_shutdown']);
  let reads = 0;
  const accesses = [];
  const sessionManager = new Proxy({ getSessionId() { reads++; return 'private-native-session'; } }, {
    get(target, key) { accesses.push(`manager.${String(key)}`); assert.equal(key, 'getSessionId'); return target[key]; }
  });
  const ctx = new Proxy({ hasUI: true, sessionManager }, {
    get(target, key) { accesses.push(`ctx.${String(key)}`); assert.ok(key === 'hasUI' || key === 'sessionManager'); return target[key]; }
  });
  const event = new Proxy({}, { get() { accesses.push('event'); throw Error('event inspection forbidden'); } });
  assert.equal(callbacks.get('session_start')(event, ctx), undefined);
  assert.equal(reads, 1);
  assert.deepEqual(accesses, ['ctx.hasUI', 'ctx.sessionManager', 'manager.getSessionId']);
  assert.equal(children.length, 1, 'one callback, one real report, no retry');
  // The generated helper owns a two-second timeout. Do not shutdown early:
  // wait for its actual child close, even when transport completion is uncertain.
  await Promise.race([
    children[0].closed,
    new Promise((_, reject) => { timer = setTimeout(() => reject(Error('report child did not close')), 4000); })
  ]);
} finally {
  clearTimeout(timer);
  callbacks.get('session_shutdown')?.();
  for (const { child } of children) { if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL'); }
  await Promise.all(children.map(({ closed }) => closed));
  childProcess.spawn = realSpawn;
  syncBuiltinESMExports();
}
"#).unwrap();

    let stdout_path = root.path().join("serve.stdout");
    let stderr_path = root.path().join("serve.stderr");
    let mut receiver = OwnedChild::spawn(
        cli(root.path())
            .args(["agent", "serve"])
            .arg(workspace.path())
            .arg("--socket")
            .arg(&socket)
            .args(["--duration", "60", "--json"])
            .current_dir(workspace.path())
            .stdin(Stdio::null())
            .stdout(fs::File::create(&stdout_path).unwrap())
            .stderr(fs::File::create(&stderr_path).unwrap()),
    );
    let request = InventoryRequest {
        socket: socket.to_str().unwrap().into(),
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(ready) = agent_management::inventory_arguments(&request, true) {
            assert_redacted(ready.stdout.as_bytes());
            assert_eq!(
                serde_json::from_str::<Value>(&ready.stdout).unwrap()["records"],
                json!([])
            );
            break;
        }
        assert!(receiver.0.as_mut().unwrap().try_wait().unwrap().is_none());
        assert!(Instant::now() < deadline, "receiver did not become ready");
        thread::sleep(Duration::from_millis(10));
    }
    assert_private(&socket, 0o600);
    run_generated(&node, root.path(), &wrapper, &generated);
    // Read-only polling is safe; an uncertain report is never resent.
    let deadline = Instant::now() + Duration::from_secs(5);
    let pi_view = loop {
        let view = inventory(root.path(), &socket);
        if !view["records"].as_array().unwrap().is_empty() {
            break view;
        }
        assert!(
            Instant::now() < deadline,
            "generated report was not persisted"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(pi_view["authorization"], "none");
    assert_eq!(pi_view["records"].as_array().unwrap().len(), 1);
    let pi = &pi_view["records"][0];
    assert_eq!(pi["provider"], "pi");
    assert_eq!(pi["last_event"], "session_start");
    assert_eq!(pi["reported_state"], "unknown");
    assert_eq!(pi["native_identity"], "unestablished");
    assert_eq!(pi["job_completion"], "unestablished");
    assert_eq!(pi["reference_trust"], "caller_supplied_unverified");
    assert_eq!(
        pi["record_id"],
        format!("{:x}", Sha256::digest(format!("pi\0{SID}")))
    );

    for provider in ["claude", "codex", "copilot"] {
        let report = agent_management::report_payload(
            &ReportRequest {
                socket: request.socket.clone(),
                provider: provider.into(),
                event: Some("SessionStart".into()),
            },
            json!({"session_id":SID,"hook_event_name":"SessionStart","prompt":"PROMPT-SECRET",
            "cwd":"CWD-SECRET","unknown":{"secret":"UNKNOWN-SECRET"}})
            .to_string()
            .as_bytes(),
            true,
        );
        match report {
            Ok(accepted) => assert_redacted(accepted.stdout.as_bytes()),
            Err(error) => {
                // A timed-out acknowledgement is not proof of no effect. Never
                // resend: the mandatory inventory and private-store assertions
                // below must still prove all four original receipts arrived.
                assert_eq!(error.code(), "agent_delivery_uncertain");
                eprintln!(
                    "fixture: uncertain acknowledgement; verifying original receipt without resend"
                );
            }
        }
    }
    let all = inventory(root.path(), &socket);
    let records = all["records"].as_array().unwrap();
    assert_eq!(records.len(), 4);
    for provider in ["pi", "claude", "codex", "copilot"] {
        let record = records.iter().find(|r| r["provider"] == provider).unwrap();
        assert_eq!(
            record["record_id"],
            format!("{:x}", Sha256::digest(format!("{provider}\0{SID}")))
        );
        assert_eq!(
            record["last_event"],
            if provider == "pi" {
                "session_start"
            } else {
                "SessionStart"
            }
        );
        assert_eq!(record["reported_state"], "unknown");
        assert!(record["age_ms"].is_u64());
    }
    let storage = workspace.path().join("state/agent-management.json");
    assert_private(&storage, 0o600);
    let committed = fs::read(&storage).unwrap();
    let saved: Value = serde_json::from_slice(&committed).unwrap();
    assert_eq!(saved["schema_version"], 1);
    assert_eq!(saved.as_object().unwrap().len(), 2);
    let saved_records = saved["records"].as_object().unwrap();
    assert_eq!(saved_records.len(), 4);
    for record in saved_records.values() {
        assert_eq!(record["session_id"], SID);
        let mut keys: Vec<_> = record
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "last_event",
                "provider",
                "server_epoch",
                "session_id",
                "timestamp_ms"
            ]
        );
    }
    assert!(!String::from_utf8_lossy(&committed).contains("SECRET"));

    let envelope =
        json!({"contract":"coven.pi-extension.v1","event":"session_start","session_id":SID});
    let rejected = [
        (
            "report",
            "pi",
            Some("SessionStart"),
            json!({"session_id":SID,"hook_event_name":"SessionStart"}),
        ),
        (
            "report-extension",
            "pi",
            Some("session_start"),
            json!({"session_id":SID,"hook_event_name":"SessionStart"}),
        ),
        (
            "report-extension",
            "pi",
            Some("session_start"),
            json!({"contract":"wrong","event":"session_start","session_id":SID}),
        ),
        (
            "report-extension",
            "pi",
            Some("session_start"),
            json!({"contract":"coven.pi-extension.v1","session_id":SID}),
        ),
        (
            "report-extension",
            "pi",
            Some("session_start"),
            json!({"contract":"coven.pi-extension.v1","event":"SessionStart","session_id":SID}),
        ),
        (
            "report-extension",
            "pi",
            Some("session_start"),
            json!({"contract":"coven.pi-extension.v1","event":"session_start","session_id":SID,"prompt":"PROMPT-SECRET"}),
        ),
        (
            "report-extension",
            "pi",
            Some("session_start"),
            json!({"contract":"coven.pi-extension.v1","event":"session_start","session_id":SID,
                "cwd":"CWD-SECRET","unknown":{"secret":"UNKNOWN-SECRET"}}),
        ),
        (
            "report-extension",
            "pi",
            Some("session_start"),
            json!({"contract":"coven.pi-extension.v1","event":"session_start","sessionId":SID}),
        ),
        (
            "report-extension",
            "pi",
            Some("SessionStart"),
            envelope.clone(),
        ),
        ("report-extension", "pi", None, envelope.clone()),
        (
            "report-extension",
            "Pi",
            Some("session_start"),
            envelope.clone(),
        ),
        (
            "report-extension",
            "claude",
            Some("session_start"),
            envelope,
        ),
    ];
    for (verb, provider, event, payload) in rejected {
        let mut command = cli(root.path());
        command
            .args(["agent", verb, "--provider", provider, "--socket"])
            .arg(&socket)
            .arg("--json");
        if let Some(event) = event {
            command.args(["--event", event]);
        }
        let result = output(&mut command, Some(&payload));
        assert!(
            !result.status.success(),
            "accepted {verb}/{provider}/{event:?}"
        );
        assert_redacted(&result.stdout);
        assert_redacted(&result.stderr);
        assert_eq!(fs::read(&storage).unwrap(), committed);
    }
    let mut unchanged = inventory(root.path(), &socket);
    for (record, previous) in unchanged["records"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(records)
    {
        record["age_ms"] = previous["age_ms"].clone();
    }
    assert_eq!(unchanged, all);

    assert_eq!(
        unsafe { libc::kill(receiver.0.as_ref().unwrap().id() as i32, libc::SIGINT) },
        0
    );
    assert!(receiver.wait_bounded().success());
    assert!(!socket.exists());
    assert_redacted(&fs::read(&stdout_path).unwrap());
    assert!(fs::read(&stderr_path).unwrap().is_empty());
    run_generated(&node, root.path(), &wrapper, &generated);
    assert_eq!(fs::read(&storage).unwrap(), committed);
    assert_eq!(fs::read(&generated).unwrap(), installed_bytes);
    assert_eq!(fs::read(&receipt_path).unwrap(), receipt_bytes);

    migration_result(
        output(
            extension_cli(root.path(), extensions.path(), "uninstall").arg("--apply"),
            None,
        ),
        "uninstalled",
    );
    assert!(!generated.exists());
    assert!(!old.exists(), "uninstall must never restore legacy code");
    let uninstalled: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    assert_eq!(uninstalled["original"], original);
    migration_result(
        output(
            extension_cli(root.path(), extensions.path(), "rollback").arg("--apply"),
            None,
        ),
        "rolled_back",
    );
    assert!(!generated.exists());
    assert_eq!(fs::read(&old).unwrap(), LEGACY);
    assert_private(&old, 0o640);
    let rolled_back: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    assert_eq!(
        rolled_back["original"], original,
        "receipt preimage is immutable"
    );
    assert_private(&receipt_path, 0o600);
    assert_eq!(
        fs::read(workspace.path().join("coven.json")).unwrap(),
        b"marker"
    );
    assert_eq!(fs::read(&storage).unwrap(), committed);
}
