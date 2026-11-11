#![cfg(target_os = "linux")]
//! Generated JavaScript -> real reporter -> real receiver, not native activation.
//! No OpenCode, Pi, Kitsune, SDK queries, or user configuration participates.

use coven::agent_management::{self, InventoryRequest};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const NEW: &str = "coven-agent-reference.js";
const OLD: &str = "kitsune-agent-state.js";
const RECEIPT: &str = "coven-opencode-extension.json";
const LEGACY: &[u8] = include_bytes!("fixtures/opencode_extension/kitsune-agent-state.js");
const A: &str = "ses_fixture-private-A";
const B: &str = "ses_fixture-private-B";

struct OwnedChild(Child);

impl OwnedChild {
    fn spawn(command: &mut Command) -> Self {
        // Own reporters as well as the Node parent if an assertion fails.
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Self(command.spawn().unwrap())
    }

    fn wait_bounded(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "child did not exit in time");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn output(command: &mut Command, payload: Option<&Value>) -> Output {
    let mut child = OwnedChild::spawn(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );
    let mut stdin = child.0.stdin.take().unwrap();
    if let Some(payload) = payload {
        if let Err(error) = stdin.write_all(payload.to_string().as_bytes()) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
    }
    drop(stdin);
    let status = child.wait_bounded();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    Output {
        status,
        stdout,
        stderr,
    }
}

fn assert_private(path: &Path, mode: u32) {
    let metadata = fs::metadata(path).unwrap();
    assert_eq!(metadata.mode() & 0o7777, mode);
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
}

fn private_dir(parent: &Path) -> tempfile::TempDir {
    let dir = tempfile::tempdir_in(parent).unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_private(dir.path(), 0o700);
    dir
}

fn assert_redacted(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    for secret in [
        A,
        B,
        "SECRET",
        "session_id",
        "sessionId",
        "sessionID",
        "parentID",
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
            "opencode",
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
    assert_eq!(value["provider"], "opencode");
    assert_eq!(value["status"], status);
    assert_eq!(
        value["native_contract"],
        "anomalyco/opencode v1.19.5 810c0eba1a7348036f8bdf61e6632d0c36c6876c"
    );
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

fn node_runtime() -> PathBuf {
    // Resolve before clearing the fixture environment; no application runtime pin.
    std::env::var_os("COVEN_TEST_NODE")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("PATH").and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|dir| dir.join("node"))
                    .find(|path| path.is_file())
            })
        })
        .expect("generated OpenCode fixture requires Node; set COVEN_TEST_NODE or PATH")
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

fn record_id(provider: &str, sid: &str) -> String {
    format!("{:x}", Sha256::digest(format!("{provider}\0{sid}")))
}

#[test]
fn generated_opencode_migration_reaches_receiver_and_rolls_back_after_uninstall() {
    let node = node_runtime();
    let root = tempfile::tempdir().unwrap();
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
    for name in [NEW, RECEIPT, ".coven-opencode-extension.lock"] {
        assert_private(&extensions.path().join(name), 0o600);
    }
    let receipt_path = extensions.path().join(RECEIPT);
    let receipt_bytes = fs::read(&receipt_path).unwrap();
    let receipt: Value = serde_json::from_slice(&receipt_bytes).unwrap();
    let original = receipt["original"].clone();
    assert!(!original.is_null());
    let installed_bytes = fs::read(&generated).unwrap();

    let wrapper = root.path().join("run-generated.mjs");
    fs::write(&wrapper, r#"
import assert from 'node:assert/strict';
import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
import { pathToFileURL } from 'node:url';
const realSpawn = childProcess.spawn;
const children = [];
const failures = [];
let attempts = 0;
const expected = [
  ['ses_fixture-private-A', 'session.created'],
  ['ses_fixture-private-B', 'session.updated'],
  ['ses_fixture-private-A', 'session.updated'],
];
let hooks, timer;
childProcess.spawn = (executable, argv, options) => {
  attempts++;
  const index = children.length;
  assert.ok(index < 3, 'no ignored events, retries, or extra reports');
  assert.deepEqual(argv, ['agent', 'report-extension', '--provider', 'opencode', '--socket', argv[5], '--event', expected[index][1]]);
  assert.deepEqual(options, { shell: false, stdio: ['pipe', 'ignore', 'ignore'] });
  const child = realSpawn(executable, argv, options);
  const closed = new Promise(resolve => child.once('close', resolve));
  children.push({ child, closed });
  const end = child.stdin.end.bind(child.stdin);
  child.stdin.end = (input) => {
    try {
      assert.deepEqual(JSON.parse(input), {
        contract: 'coven.opencode-extension.v1', event: expected[index][1], session_id: expected[index][0],
      });
    } catch (error) { failures.push(error); throw error; }
    return end(input);
  };
  return child;
};
syncBuiltinESMExports();
try {
  const module = await import(pathToFileURL(process.argv[2]).href);
  assert.deepEqual(Object.keys(module), ['CovenAgentReferencePlugin']);
  const opaque = new Proxy({}, {
    get() { throw Error('opaque SECRET context/metadata read'); },
    ownKeys() { throw Error('opaque SECRET enumeration'); },
  });
  hooks = await module.CovenAgentReferencePlugin(opaque);
  assert.deepEqual(Object.keys(hooks), ['event', 'dispose']);
  const info = id => ({ id, slug: 'SLUG-SECRET', projectID: 'PROJECT-SECRET', directory: 'DIRECTORY-SECRET',
    title: 'TITLE-SECRET', version: '1.19.5', time: { created: 0, updated: 1 },
    parentID: undefined, metadata: opaque, extra: opaque });
  const event = (dto, type = 'session.created', sessionID = dto.id) => ({
    event: { type, properties: { info: dto, sessionID, extra: opaque } }, context: opaque,
  });
  const a = info(expected[0][0]);
  const b = info(expected[1][0]);
  // Queue without awaiting child closes: snapshots must not retain these DTOs.
  await hooks.event(event(a));
  await hooks.event(event(b, 'session.updated'));
  await hooks.event(event(a, 'session.updated'));
  a.id = b.id = 'ses_MUTATED-SECRET';
  assert.equal(children.length, 1, 'only one FIFO report active');
  for (const parentID of ['ses_parent-SECRET', null]) {
    await hooks.event(event({ ...info(expected[0][0]), parentID }));
  }
  await hooks.event(event({ id: expected[0][0] }, 'session.updated'));
  await hooks.event(event(info(expected[0][0]), 'session.updated', expected[1][0]));
  for (const type of ['session.status', 'session.idle', 'chat.message', 'session.deleted']) {
    await hooks.event(event(info(expected[0][0]), type));
  }
  await hooks.event({ event: { type: 'session.updated', properties: { sessionID: expected[0][0] } } });
  await hooks.event(null);
  const deadline = new Promise((_, reject) => { timer = setTimeout(() => reject(Error('three original reporters did not close')), 8000); });
  await Promise.race([(async () => {
    for (let index = 0; index < 3; index++) {
      assert.ok(children[index], 'FIFO must spawn next original report on close');
      await children[index].closed;
    }
  })(), deadline]);
  assert.equal(attempts, 3, 'no swallowed extra spawn attempts');
  assert.deepEqual(failures, [], 'no swallowed payload assertion failures');
  assert.equal(children.length, 3, 'all three originals, no resend');
} finally {
  clearTimeout(timer);
  await hooks?.dispose();
  for (const { child } of children) {
    if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
  }
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
        assert!(receiver.0.try_wait().unwrap().is_none());
        assert!(Instant::now() < deadline, "receiver did not become ready");
        thread::sleep(Duration::from_millis(10));
    }
    assert_private(&socket, 0o600);
    run_generated(&node, root.path(), &wrapper, &generated);
    // Read-only polling, never resend an original with an uncertain ack.
    let deadline = Instant::now() + Duration::from_secs(5);
    let view = loop {
        let view = inventory(root.path(), &socket);
        let records = view["records"].as_array().unwrap();
        if records.len() == 2 && records.iter().all(|r| r["last_event"] == "session.updated") {
            break view;
        }
        assert!(
            Instant::now() < deadline,
            "three generated reports were not persisted"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(view["authorization"], "none");
    for sid in [A, B] {
        let id = record_id("opencode", sid);
        let record = view["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["record_id"] == id)
            .unwrap();
        assert_eq!(record["provider"], "opencode");
        assert_eq!(record["last_event"], "session.updated");
    }

    // Caller-supplied packets exercise namespace separation, not native authority.
    for provider in ["claude", "codex", "copilot", "pi"] {
        let (verb, event, payload) = if provider == "pi" {
            (
                "report-extension",
                "session_start",
                json!({"contract":"coven.pi-extension.v1","event":"session_start","session_id":A}),
            )
        } else {
            (
                "report",
                "SessionStart",
                json!({"session_id":A,"hook_event_name":"SessionStart", "prompt":"PROMPT-SECRET", "cwd":"CWD-SECRET", "unknown":{"secret":"UNKNOWN-SECRET"}}),
            )
        };
        let result = output(
            cli(root.path())
                .args([
                    "agent",
                    verb,
                    "--provider",
                    provider,
                    "--event",
                    event,
                    "--socket",
                ])
                .arg(&socket)
                .arg("--json"),
            Some(&payload),
        );
        assert_redacted(&result.stdout);
        assert_redacted(&result.stderr);
        if !result.status.success() {
            assert!(result.stdout.is_empty());
            assert_eq!(result.stderr, b"[ERROR] agent_delivery_uncertain\n");
        }
    }
    let all = inventory(root.path(), &socket);
    let records = all["records"].as_array().unwrap();
    assert_eq!(all["authorization"], "none");
    assert_eq!(
        records.len(),
        6,
        "mandatory proof of all original receipts, including uncertain acks"
    );
    for (provider, sid, event) in [
        ("opencode", A, "session.updated"),
        ("opencode", B, "session.updated"),
        ("claude", A, "SessionStart"),
        ("codex", A, "SessionStart"),
        ("copilot", A, "SessionStart"),
        ("pi", A, "session_start"),
    ] {
        let id = record_id(provider, sid);
        let record = records.iter().find(|r| r["record_id"] == id).unwrap();
        assert_eq!(record["provider"], provider);
        assert_eq!(record["last_event"], event);
        assert_eq!(record["reported_state"], "unknown");
        assert_eq!(record["native_identity"], "unestablished");
        assert_eq!(record["job_completion"], "unestablished");
        assert_eq!(record["reference_trust"], "caller_supplied_unverified");
        assert!(record["age_ms"].is_u64());
    }
    let storage = workspace.path().join("state/agent-management.json");
    assert_private(&storage, 0o600);
    let committed = fs::read(&storage).unwrap();
    let saved: Value = serde_json::from_slice(&committed).unwrap();
    assert_eq!(saved["schema_version"], 1);
    assert_eq!(saved.as_object().unwrap().len(), 2);
    let saved_records = saved["records"].as_object().unwrap();
    assert_eq!(saved_records.len(), 6);
    for public in records {
        let record = &saved_records[public["record_id"].as_str().unwrap()];
        assert_eq!(record["provider"], public["provider"]);
        assert_eq!(record["last_event"], public["last_event"]);
        assert_eq!(
            record["session_id"],
            if public["record_id"] == record_id("opencode", B) {
                B
            } else {
                A
            }
        );
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
        json!({"contract":"coven.opencode-extension.v1","event":"session.created","session_id":A});
    let mut rejected = vec![
        (
            "report-extension",
            "opencode",
            Some("session.created"),
            json!({"contract":"coven.opencode-extension.v1","event":"session.updated","session_id":A}),
        ),
        (
            "report-extension",
            "opencode",
            Some("session.updated"),
            envelope.clone(),
        ),
        ("report-extension", "opencode", None, envelope.clone()),
        (
            "report-extension",
            "OpenCode",
            Some("session.created"),
            envelope.clone(),
        ),
        (
            "report-extension",
            "opencode",
            Some("session.created"),
            json!({"contract":"wrong","event":"session.created","session_id":A}),
        ),
        (
            "report-extension",
            "opencode",
            Some("session.created"),
            json!({"contract":"coven.opencode-extension.v1","session_id":A}),
        ),
        (
            "report-extension",
            "opencode",
            Some("session.created"),
            json!({"contract":"coven.opencode-extension.v1","event":"session.created","sessionId":A}),
        ),
        (
            "report-extension",
            "opencode",
            Some("session.created"),
            json!({"session_id":A,"hook_event_name":"SessionStart"}),
        ),
        (
            "report-extension",
            "opencode",
            Some("session.status"),
            envelope.clone(),
        ),
    ];
    for key in ["prompt", "cwd", "unknown"] {
        let mut extra = envelope.clone();
        extra[key] = json!({"secret":"UNKNOWN-SECRET"});
        rejected.push((
            "report-extension",
            "opencode",
            Some("session.created"),
            extra,
        ));
    }
    for provider in ["claude", "codex", "copilot", "pi"] {
        rejected.push((
            "report-extension",
            provider,
            Some("session.created"),
            envelope.clone(),
        ));
    }
    for provider in ["claude", "codex", "copilot", "opencode", "pi"] {
        rejected.push(("report", provider, Some("SessionStart"), envelope.clone()));
    }
    rejected.push((
        "report-extension",
        "pi",
        Some("session_start"),
        json!({"contract":"coven.opencode-extension.v1","event":"session_start","session_id":A}),
    ));
    rejected.push((
        "report-extension",
        "opencode",
        Some("session.created"),
        json!({"contract":"coven.pi-extension.v1","event":"session.created","session_id":A}),
    ));
    for provider in ["opencode", "pi"] {
        for event in ["SessionStart", "session.created"] {
            rejected.push((
                "report",
                provider,
                Some(event),
                json!({"session_id":A,"hook_event_name":event}),
            ));
        }
    }
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
        unsafe { libc::kill(receiver.0.id() as i32, libc::SIGINT) },
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
    assert!(!old.exists(), "uninstall must never resurrect legacy code");
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
