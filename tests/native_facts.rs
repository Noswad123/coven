#![cfg(unix)]
#[path = "support/physical_temp.rs"]
mod physical_temp;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
const EXE: &str = env!("CARGO_BIN_EXE_coven");
struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        self.0.wait().unwrap();
    }
}
fn cli(args: &[&str], payload: Option<&Value>) -> std::process::Output {
    let mut child = Owned(
        Command::new(EXE)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    if let Some(v) = payload {
        child
            .0
            .stdin
            .take()
            .unwrap()
            .write_all(v.to_string().as_bytes())
            .unwrap();
    } else {
        drop(child.0.stdin.take());
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    use std::io::Read;
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
    std::process::Output {
        status: child.0.try_wait().unwrap().unwrap(),
        stdout,
        stderr,
    }
}
struct Kit {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    frames: Arc<Mutex<Vec<Value>>>,
}
impl Drop for Kit {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}
#[test]
fn actual_fact_cli_normal_receiver_and_fake_kit_are_unknown_private_and_volatile() {
    let root = physical_temp::tempdir();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(root.path().join("coven.json"), "{}").unwrap();
    let kp = root.path().join("k");
    let rp = root.path().join("r");
    let listener = UnixListener::bind(&kp).unwrap();
    std::fs::set_permissions(&kp, std::fs::Permissions::from_mode(0o600)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let frames = Arc::new(Mutex::new(Vec::new()));
    let (cancel, seen) = (stop.clone(), frames.clone());
    let worker = thread::spawn(move || {
        let mut revision = 0;
        while !cancel.load(Ordering::Acquire) {
            let (mut socket, _) = match listener.accept() {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(e) => panic!("{e}"),
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(socket.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let r: Value = serde_json::from_str(&line).unwrap();
            let p = &r["params"];
            let result = match r["method"].as_str().unwrap() {
                "session.snapshot" => {
                    json!({"type":"session_snapshot","snapshot":{"version":"fixture","protocol":18,"integration_views":{"version":1,"sources":[{"id":"factsFixture","epoch":null}]}}})
                }
                "integration.view.open" => {
                    json!({"type":"view_opened","lease":"lease","epoch":"epoch","revision":0,"ttl_ms":5000})
                }
                "integration.view.replace" => {
                    revision = p["revision"].as_u64().unwrap();
                    let mut frames = seen.lock().unwrap();
                    assert!(frames.len() < 128);
                    frames.push(p["items"].clone());
                    json!({"type":"view_replaced","epoch":"epoch","revision":revision})
                }
                "integration.view.poll" => {
                    json!({"type":"view_intents","epoch":"epoch","revision":revision,"ttl_ms":5000,"intents":[]})
                }
                "integration.view.close" => json!({"type":"view_closed"}),
                method => panic!("unexpected Kit method {method}"),
            };
            writeln!(socket, "{}", json!({"id":r["id"],"result":result})).unwrap();
        }
    });
    let kit = Kit {
        stop,
        worker: Some(worker),
        frames,
    };
    let mut receiver = Owned(
        Command::new(EXE)
            .args(["agent", "run"])
            .arg(root.path())
            .args([
                "--socket",
                rp.to_str().unwrap(),
                "--kitsune-socket",
                kp.to_str().unwrap(),
                "--source",
                "factsFixture",
                "--json",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if cli(
            &[
                "agent",
                "observations",
                "--socket",
                rp.to_str().unwrap(),
                "--json",
            ],
            None,
        )
        .status
        .success()
        {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let saved = std::fs::read(root.path().join("state/agent-management.json")).ok();
    for provider in ["codex", "copilot"] {
        let mut p = json!({"hook_event_name":"Stop","session_id":"PRIVATE-SID","cwd":"/PRIVATE","stop_hook_active":false});
        if provider == "codex" {
            p["turn_id"] = json!("PRIVATE-TURN");
            p["model"] = json!("PRIVATE-MODEL");
            p["last_assistant_message"] = json!("PRIVATE-OUTPUT");
        } else {
            // A source timestamp from the local clock, no endpoint/provider IO.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as libc::time_t;
            let mut tm: libc::tm = unsafe { std::mem::zeroed() };
            unsafe { libc::gmtime_r(&now, &mut tm) };
            p["timestamp"] = json!(format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                tm.tm_hour,
                tm.tm_min,
                tm.tm_sec
            ));
            p["stop_reason"] = json!("end_turn");
            p["transcript_path"] = json!("/PRIVATE");
        }
        let out = cli(
            &[
                "agent",
                "report-fact",
                "--provider",
                provider,
                "--event",
                "Stop",
                "--socket",
                rp.to_str().unwrap(),
                "--best-effort",
            ],
            Some(&p),
        );
        assert!(out.status.success());
        assert!(out.stdout.is_empty());
        assert!(out.stderr.is_empty());
    }
    let out = cli(
        &[
            "agent",
            "observations",
            "--socket",
            rp.to_str().unwrap(),
            "--json",
        ],
        None,
    );
    assert!(out.status.success());
    let view: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(view["observations"].as_array().unwrap().len(), 2);
    assert!(!view.to_string().contains("PRIVATE"));
    for r in view["observations"].as_array().unwrap() {
        assert_eq!(r["reported_state"], "unknown");
        assert_eq!(r["origin"], "provider_native_fact");
        assert!(r.get("fresh_pending_count").is_none());
        assert!(r.get("sequence").is_none());
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let text = kit
            .frames
            .lock()
            .unwrap()
            .iter()
            .map(Value::to_string)
            .collect::<String>();
        assert!(!text.contains("PRIVATE"));
        if text.contains("last received native hook fact: Stop; source age unknown")
            && text.contains("copilot-hook-facts.v1")
        {
            break;
        }
        assert!(Instant::now() < deadline, "missing fact UI");
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        saved,
        std::fs::read(root.path().join("state/agent-management.json")).ok()
    );
    drop(receiver.0.stdin.take());
    let deadline = Instant::now() + Duration::from_secs(5);
    while receiver.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert!(receiver.0.try_wait().unwrap().unwrap().success());
    drop(receiver);
    drop(kit);
}

#[test]
fn hook_failures_are_empty_success_but_operator_failures_are_not_hidden() {
    for best in [false, true] {
        let mut args = vec![
            "agent",
            "report-fact",
            "--provider",
            "codex",
            "--event",
            "Stop",
            "--socket",
            "/nonexistent/s",
        ];
        if best {
            args.push("--best-effort");
        } else {
            args.push("--json");
        }
        let out = cli(&args, Some(&json!({"PRIVATE":"PRIVATE"})));
        assert_eq!(out.status.success(), best);
        assert!(!String::from_utf8_lossy(&out.stdout).contains("PRIVATE"));
        assert!(!String::from_utf8_lossy(&out.stderr).contains("PRIVATE"));
        if best {
            assert!(out.stdout.is_empty());
            assert!(out.stderr.len() < 128);
        }
    }
}

#[test]
fn malformed_peer_fact_rows_never_reach_operator_stdout() {
    let root = physical_temp::tempdir();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.path().join("p");
    let listener = UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let row = json!({"observation_id":"b".repeat(64),"provider":"codex","profile":"codex-hook-facts.v1","origin":"provider_native_fact","capability":"native_hook_fact","reported_state":"unknown","age_ms":0,"freshness":"unknown","reference_trust":"caller_supplied_unverified","root_eligibility":"unestablished","native_identity":"unestablished","job_completion":"unestablished","binding":"none","attribution":"Coven","last_received_event":"Stop","ordering":"unordered","time_basis":"local_receipt_only","source_time":"unknown"});
    let fields: Vec<_> = row.as_object().unwrap().keys().cloned().collect();
    let count = fields.len();
    let worker = thread::spawn(move || {
        for field in fields {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(socket.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            let mut bad = row.clone();
            bad.as_object_mut().unwrap().remove(&field);
            bad["private"] = json!("PRIVATE-PEER");
            let result = json!({"contract":"coven.normal-observations.v1","server_epoch":"a".repeat(32),"authorization":"none","observations":[bad],"omitted_observations":0,"retired_native_observations":0});
            writeln!(socket,"{}",json!({"id":request["id"],"contract":"coven.observation-control.v1","type":"result","result":result})).unwrap();
        }
        // Keep the selected endpoint inode alive through the final reply fence.
        thread::sleep(Duration::from_millis(50));
    });
    for _ in 0..count {
        let out = cli(
            &[
                "agent",
                "observations",
                "--socket",
                path.to_str().unwrap(),
                "--json",
            ],
            None,
        );
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("PRIVATE"));
    }
    worker.join().unwrap();
}

#[test]
fn native_hook_plan_is_inert_and_managed_mutation_routes_are_unavailable() {
    let root = physical_temp::tempdir();
    std::fs::write(
        root.path().join("settings.json"),
        b"{\"disableAllHooks\":true}",
    )
    .unwrap();
    for provider in ["codex", "copilot"] {
        let args = [
            "agent",
            "native-hooks",
            "plan",
            "--provider",
            provider,
            "--executable",
            "/inert/coven",
            "--config-root",
            root.path().to_str().unwrap(),
            "--from-kitsune",
            "--json",
        ];
        let a = cli(&args, None);
        let b = cli(&args, None);
        assert!(a.status.success());
        assert_eq!(a.stdout, b.stdout);
        let value: Value = serde_json::from_slice(&a.stdout).unwrap();
        assert_eq!(value["applied"], false);
        assert_eq!(value["installation"], "not_implemented");
        for op in ["install", "uninstall", "rollback"] {
            assert!(
                !cli(&["agent", "native-hooks", op, "--apply"], None)
                    .status
                    .success()
            );
        }
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    assert_eq!(
        std::fs::read(root.path().join("settings.json")).unwrap(),
        b"{\"disableAllHooks\":true}"
    );
}
