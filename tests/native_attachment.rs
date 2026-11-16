#![cfg(unix)]

use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const INSTANCE: &str = "12345678-1234-1234-1234-123456789abc";
const CLIENT: &str = "12345678-1234-1234-1234-123456789abd";
const EXE: &str = env!("CARGO_BIN_EXE_coven");

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        self.0.wait().expect("reap exact owned CLI child");
    }
}
impl OwnedChild {
    fn finish(&mut self) -> Output {
        let deadline = Instant::now() + Duration::from_secs(8);
        let status = loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "owned CLI deadline");
            thread::sleep(Duration::from_millis(5));
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        if let Some(mut pipe) = self.0.stdout.take() {
            pipe.read_to_end(&mut stdout).unwrap();
        }
        if let Some(mut pipe) = self.0.stderr.take() {
            pipe.read_to_end(&mut stderr).unwrap();
        }
        Output {
            status,
            stdout,
            stderr,
        }
    }
}

struct Kit {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<(u64, String, String)>>,
    methods: Arc<Mutex<Vec<String>>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Drop for Kit {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker
            .take()
            .unwrap()
            .join()
            .expect("join owned fake Kit");
    }
}
impl Kit {
    fn new(parent: &Path) -> Self {
        let path = parent.join("k");
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new((1, INSTANCE.to_owned(), "live".to_owned())));
        let methods = Arc::new(Mutex::new(Vec::new()));
        let (cancel, current, seen) = (stop.clone(), state.clone(), methods.clone());
        let worker = thread::spawn(move || {
            while !cancel.load(Ordering::Acquire) {
                let mut socket = match listener.accept() {
                    Ok((s, _)) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(e) => panic!("fake Kit accept: {e}"),
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut line = String::new();
                if BufReader::new(socket.try_clone().unwrap())
                    .read_line(&mut line)
                    .is_err()
                    || line.is_empty()
                {
                    continue;
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                let method = request["method"].as_str().unwrap();
                seen.lock().unwrap().push(method.to_owned());
                let state = current.lock().unwrap().clone();
                let result = match method {
                    "session.snapshot" => {
                        json!({"snapshot":{"terminal_navigation":{"version":state.0},
                        "panes":[{"pane_id":"pane","terminal_id":"terminal","workspace_id":"w","tab_id":"t"}]}})
                    }
                    "terminal.inspect" => {
                        assert_eq!(request["params"], json!({"terminal_id":"terminal"}));
                        json!({"type":"terminal_inspected","terminal_id":"terminal","runtime_generation":state.1,
                            "availability":state.2,"workspace_id":"w","tab_id":"t","pane_id":"pane"})
                    }
                    "terminal.focus" => {
                        assert_eq!(
                            request["params"],
                            json!({"terminal_id":"terminal","expected_runtime_generation":state.1})
                        );
                        json!({"type":"terminal_focused","terminal_id":"terminal","runtime_generation":state.1,
                            "workspace_id":"w","tab_id":"t","pane_id":"pane"})
                    }
                    "view.open" => {
                        let _ = writeln!(
                            socket,
                            "{}",
                            json!({"id":request["id"],"error":{"code":"unsupported","message":"fixture"}})
                        );
                        continue;
                    }
                    other => panic!("forbidden/unexpected Kit method: {other}"),
                };
                let _ = writeln!(socket, "{}", json!({"id":request["id"],"result":result}));
            }
        });
        Self {
            path,
            stop,
            state,
            methods,
            worker: Some(worker),
        }
    }
}

fn routed(root: &Path, kit: &Kit, args: &[&str]) -> Command {
    let mut command = Command::new(EXE);
    command
        .args(args)
        .env("KITSUNE_SOCKET_PATH", &kit.path)
        .env("KITSUNE_PANE_ID", "pane")
        .env("KITSUNE_AGENT_MANAGEMENT", "external")
        .env("KITSUNE_INTEGRATION_SOURCE", "coven")
        .env(
            "KITSUNE_INTEGRATION_REPORT_COMMAND",
            json!([
                EXE,
                "agent",
                "report-native",
                "--from-kitsune",
                "--state-root",
                root
            ])
            .to_string(),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}
fn invoke(command: &mut Command, payload: Option<&Value>) -> Output {
    let mut child = OwnedChild(command.spawn().unwrap());
    if let Some(p) = payload {
        child
            .0
            .stdin
            .take()
            .unwrap()
            .write_all(p.to_string().as_bytes())
            .unwrap();
    } else {
        drop(child.0.stdin.take());
    }
    child.finish()
}
fn value(output: Output) -> Value {
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn runtime(root: &Path, kit: &Kit) -> OwnedChild {
    OwnedChild(
        Command::new(EXE)
            .args(["integration", "runtime", "--state-root"])
            .arg(root)
            .arg("--json")
            .env("KITSUNE_SOCKET_PATH", &kit.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    )
}
fn capture(root: &Path, kit: &Kit) -> Output {
    invoke(
        &mut routed(
            root,
            kit,
            &["agent", "attachment-target", "--from-kitsune", "--json"],
        ),
        None,
    )
}
fn ready(root: &Path, kit: &Kit, child: &mut OwnedChild) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let output = capture(root, kit);
        if output.status.success() {
            return value(output);
        }
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "runtime exited before ready"
        );
        assert!(Instant::now() < deadline, "runtime readiness deadline");
        thread::sleep(Duration::from_millis(10));
    }
}
fn report(root: &Path, kit: &Kit, p: &Value) -> Output {
    invoke(
        &mut routed(
            root,
            kit,
            &["agent", "report-attachment", "--from-kitsune", "--json"],
        ),
        Some(p),
    )
}

#[test]
fn real_cli_readonly_capture_attachment_focus_runtime_fences_detach_and_restart() {
    // Parent owns storage; children are always killed/reaped before fixture teardown.
    let temp = tempfile::Builder::new().prefix("a2").tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let kit = Kit::new(temp.path());
    let root = temp.path().join("s");
    assert!(!capture(&root, &kit).status.success());
    assert!(
        !root.exists(),
        "read-only capture must not create unknown state"
    );
    let mut receiver = runtime(&root, &kit);
    let captured = ready(&root, &kit, &mut receiver);
    assert_eq!(captured.as_object().unwrap().len(), 3);
    assert_eq!(captured["contract"], "coven.attachment-target.v1");
    assert_eq!(captured["target"].as_object().unwrap().len(), 5);
    assert_eq!(captured["target"]["socket"], kit.path.to_str().unwrap());
    let epoch = captured["server_epoch"].as_str().unwrap();
    let mut p = json!({"contract":"djinn.client-attachment.v1","expected_epoch":epoch,"client_id":CLIENT,
        "revision":1,"operation":"attach","instance_id":INSTANCE,"session_id":"PRIVATE-SID",
        "scope":"local_full_tui_root","target":captured["target"]});
    assert_eq!(
        value(report(&root, &kit, &p)),
        json!({"type":"accepted","delivery":"volatile"})
    );
    let observations = value(invoke(
        &mut routed(
            &root,
            &kit,
            &["agent", "observations", "--from-kitsune", "--json"],
        ),
        None,
    ));
    let row = &observations["observations"][0];
    assert_eq!(row["origin"], "djinn_client_attachment");
    assert_eq!(row["reported_state"], "unknown");
    assert_eq!(row["age_ms"], Value::Null);
    assert_eq!(row["hosting_state"], "local_full_tui_root");
    assert!(!observations.to_string().contains("PRIVATE-SID"));
    assert!(!observations.to_string().contains(CLIENT));
    let id = row["observation_id"].as_str().unwrap();
    let revision = row["association_revision"].as_str().unwrap();
    let focused = value(invoke(
        &mut routed(
            &root,
            &kit,
            &[
                "agent",
                "focus",
                id,
                "--expected-epoch",
                epoch,
                "--expected-revision",
                revision,
                "--from-kitsune",
                "--execute",
                "--json",
            ],
        ),
        None,
    ));
    assert_eq!(focused["provenance"], "producer_reported");
    assert_eq!(focused["native_hosting"], "local_full_tui_root");
    p["revision"] = json!(2);
    kit.state.lock().unwrap().1 = CLIENT.into();
    assert!(
        !report(&root, &kit, &p).status.success(),
        "never rebind replacement runtime"
    );
    kit.state.lock().unwrap().1 = INSTANCE.into();
    kit.state.lock().unwrap().0 = 0;
    assert!(!capture(&root, &kit).status.success());
    assert!(
        !report(&root, &kit, &p).status.success(),
        "unsupported has no fallback"
    );
    kit.state.lock().unwrap().0 = 1;
    let original = temp.path().join("old");
    std::fs::rename(&kit.path, &original).unwrap();
    let replacement = UnixListener::bind(&kit.path).unwrap();
    std::fs::set_permissions(&kit.path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        !report(&root, &kit, &p).status.success(),
        "endpoint identity replacement refused"
    );
    drop(replacement);
    std::fs::remove_file(&kit.path).unwrap();
    std::fs::rename(&original, &kit.path).unwrap();
    kit.state.lock().unwrap().2 = "exited".into();
    assert!(!report(&root, &kit, &p).status.success());
    let before = kit.methods.lock().unwrap().len();
    let detach = json!({"contract":"djinn.client-attachment.v1","expected_epoch":epoch,"client_id":CLIENT,"revision":3,"operation":"detach"});
    value(report(&root, &kit, &detach));
    assert_eq!(
        kit.methods.lock().unwrap().len(),
        before,
        "detach requires no terminal inspection"
    );
    drop(receiver.0.stdin.take());
    value(receiver.finish());
    drop(receiver);
    kit.state.lock().unwrap().2 = "live".into();
    let mut receiver = runtime(&root, &kit);
    let new = ready(&root, &kit, &mut receiver);
    assert_ne!(new["server_epoch"], captured["server_epoch"]);
    assert!(
        !report(&root, &kit, &p).status.success(),
        "late old-epoch frame refused"
    );
    drop(receiver.0.stdin.take());
    value(receiver.finish());
    drop(receiver);
    drop(kit);
    temp.close()
        .expect("clean exact parent-owned attachment fixture");
}
