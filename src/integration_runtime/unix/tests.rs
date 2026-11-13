use super::*;
use std::{
    io::{Read, Write},
    os::unix::{
        fs::{PermissionsExt, symlink},
        net::{UnixListener, UnixStream},
    },
    process::{Child, Command, Stdio},
    time::Instant,
};

#[test]
fn runtime_child() {
    let Some(root) = std::env::var_os("COVEN_RUNTIME_TEST_ROOT") else {
        return;
    };
    match runtime(Some(Path::new(&root)), true) {
        Ok(output) => {
            if std::env::var_os("COVEN_RECEIPT_TEST_TRACE").is_some() {
                let value: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
                eprintln!("presentation_result {}", value["presentation"]);
                let view = &value["presentation"];
                assert!(view["frames"].as_u64().unwrap() > 0);
                // EOF may cancel an in-flight UI RPC instead of the pause between
                // polls. That is a valid stopped/uncertain presentation outcome.
                if view["outcome"] != "completed" {
                    assert_eq!(view["diagnostics"], serde_json::json!(["interrupted"]));
                }
            }
            println!("{}", output.stdout);
        }
        Err(error) => {
            eprintln!("{}", error.code());
            std::process::exit(19);
        }
    }
}

#[test]
fn reporter_child() {
    if std::env::var_os("COVEN_REPORTER_TEST").is_none() {
        return;
    }
    crate::agent_management::receipt_trace::stage("reporter_begin");
    let result = reporter_socket(None).and_then(|socket| {
        crate::agent_management::receipt_trace::stage("route_resolved");
        if std::env::var_os("COVEN_RECEIPT_TEST_TRACE").is_some() {
            return crate::agent_management::report_payload(
                &crate::agent_management::ReportRequest { socket, provider: "codex".into(), event: None },
                &serde_json::to_vec(&serde_json::json!({"hook_event_name":"SessionStart", "session_id":format!("fixture-{}", std::process::id())})).unwrap(),
                true,
            ).inspect(|output| {
                let value: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
                assert_eq!(value["delivery"], "committed");
            });
        }
        crate::agent_management::native_report_payload(&socket, br#"{"contract":"djinn.native-events.v1","instance_id":"11111111-1111-4111-8111-111111111111","session_id":"PRIVATE-SID","sequence":1,"event":"session.created"}"#, true).inspect(|output| {
            let value: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
            assert_eq!(value, serde_json::json!({"type":"accepted","delivery":"volatile"}));
        })
    });
    if let Err(error) = result {
        eprintln!("{}", error.code());
        std::process::exit(19);
    }
}

fn report(root: &Path, api: &Path, poison: Option<(&str, &str)>) -> std::process::Output {
    let cli = std::env::var_os("COVEN_RECEIPT_TEST_CLI")
        .filter(|_| poison == Some(("COVEN_RECEIPT_TEST_TRACE", "1")));
    let executable = cli
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_exe().unwrap());
    let mut command = Command::new(&executable);
    if cli.is_some() {
        command
            .args([
                "agent",
                "report",
                "--provider",
                "codex",
                "--from-kitsune",
                "--json",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    } else {
        command.args([
            "--exact",
            "integration_runtime::unix::tests::reporter_child",
            "--nocapture",
        ]);
    }
    command
        .env("COVEN_REPORTER_TEST", "1")
        .env("KITSUNE_AGENT_MANAGEMENT", "external")
        .env("KITSUNE_INTEGRATION_SOURCE", "coven")
        .env("KITSUNE_SOCKET_PATH", api)
        .env(
            "KITSUNE_INTEGRATION_REPORT_COMMAND",
            serde_json::json!([
                executable,
                "agent",
                "report-native",
                "--from-kitsune",
                "--state-root",
                root
            ])
            .to_string(),
        );
    if let Some((key, value)) = poison {
        if value.is_empty() {
            command.env_remove(key);
        } else {
            command.env(key, value);
        }
    }
    let output = if cli.is_some() {
        let mut child = Children(vec![command.spawn().unwrap()]);
        child.0[0].stdin.take().unwrap().write_all(
            &serde_json::to_vec(&serde_json::json!({"hook_event_name":"SessionStart", "session_id":crate::platform::random_uuid_hex().unwrap()})).unwrap()
        ).unwrap();
        let output = child.0.remove(0).wait_with_output().unwrap();
        if output.status.success() {
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["delivery"], "committed");
        }
        output
    } else {
        command.output().unwrap()
    };
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-SID"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE-SID"));
    output
}

fn child(root: &Path, api: &Path) -> Child {
    runtime_command(root, api).spawn().unwrap()
}
fn runtime_command(root: &Path, api: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "integration_runtime::unix::tests::runtime_child",
            "--nocapture",
        ])
        .env("COVEN_RUNTIME_TEST_ROOT", root)
        .env("KITSUNE_SOCKET_PATH", api)
        .env_remove("KITSUNE_AGENT_MANAGEMENT")
        .env_remove("KITSUNE_INTEGRATION_SOURCE")
        .env_remove("KITSUNE_INTEGRATION_REPORT_COMMAND")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

struct Gate {
    listener: UnixListener,
    path: PathBuf,
}
impl Gate {
    fn new(parent: &Path) -> Self {
        let path = parent.join("c.sock");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        Self { listener, path }
    }
    fn child(&self, root: &Path, api: &Path, stages: &str) -> Child {
        runtime_command(root, api)
            .env("COVEN_RUNTIME_TEST_CONTROL", &self.path)
            .env("COVEN_RUNTIME_TEST_CHECKPOINTS", stages)
            .spawn()
            .unwrap()
    }
    fn arrive(&self) -> (String, UnixStream) {
        let end = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match self.listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < end, "checkpoint arrival timed out");
                    thread::sleep(Duration::from_millis(2));
                }
                Err(e) => panic!("checkpoint accept: {e}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut name = Vec::new();
        loop {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            if byte[0] == b'\n' {
                break;
            }
            name.push(byte[0]);
            assert!(name.len() < 64);
        }
        (String::from_utf8(name).unwrap(), stream)
    }
}

fn overlapping_startup(existing: bool) {
    let temp = fixture();
    let root = temp.path().join("s");
    if existing {
        ensure_private_directory(&root.join("i")).unwrap();
        let dir = Directory::open(&root, false).unwrap();
        drop(
            dir.registry_lock(&observation_socket::NoCancel, Duration::from_secs(1))
                .unwrap(),
        );
    }
    let a = temp.path().join("a.sock");
    let b = temp.path().join("b.sock");
    let _a = UnixListener::bind(&a).unwrap();
    let _b = UnixListener::bind(&b).unwrap();
    let endpoint_a = selected(&a);
    let endpoint_b = selected(&b);
    let gate = Gate::new(temp.path());
    let stages = if existing {
        "before_registry_create,after_registry_lock,before_registry_wait"
    } else {
        "before_mkdir,before_registry_create,after_registry_lock,before_registry_wait"
    };
    let mut children = Children(vec![
        gate.child(&root, &a, stages),
        gate.child(&root, &b, stages),
    ]);
    if !existing {
        let (stage_a, mut blocked_a) = gate.arrive();
        let (stage_b, mut blocked_b) = gate.arrive();
        assert_eq!(stage_a, "before_mkdir");
        assert_eq!(stage_b, "before_mkdir");
        assert!(!root.exists());
        blocked_a.write_all(&[1]).unwrap();
        blocked_b.write_all(&[1]).unwrap();
    }
    let (stage_a, mut blocked_a) = gate.arrive();
    let (stage_b, mut blocked_b) = gate.arrive();
    assert_eq!(stage_a, "before_registry_create");
    assert_eq!(stage_b, "before_registry_create");
    if !existing {
        assert!(!root.join("registry.lock").exists());
    }
    blocked_a.write_all(&[1]).unwrap();
    blocked_b.write_all(&[1]).unwrap();
    // The first helper is held inside the critical section until the other has
    // actually observed contention. This is not a sequential-startup smoke test.
    let (first, mut blocked_first) = gate.arrive();
    let (second, mut blocked_second) = gate.arrive();
    let mut stages = [first.as_str(), second.as_str()];
    stages.sort();
    assert_eq!(stages, ["after_registry_lock", "before_registry_wait"]);
    blocked_first.write_all(&[1]).unwrap();
    blocked_second.write_all(&[1]).unwrap();
    let (stage, mut blocked) = gate.arrive();
    assert_eq!(stage, "after_registry_lock");
    blocked.write_all(&[1]).unwrap();
    assert_ne!(
        ready(&root, &endpoint_a, &mut children.0[0]),
        ready(&root, &endpoint_b, &mut children.0[1])
    );
    close(&mut children.0[0]);
    close(&mut children.0[1]);
}

#[test]
fn different_apis_overlap_on_fresh_root_and_registry_creation() {
    overlapping_startup(false);
}

#[test]
fn different_apis_wait_on_existing_shared_registry() {
    overlapping_startup(true);
}

#[test]
fn registry_wait_is_bounded_and_cooperatively_cancelled() {
    struct Flag(AtomicBool);
    impl observation_socket::Cancel for Flag {
        fn interrupted(&self) -> bool {
            self.0.load(Ordering::Acquire)
        }
    }
    let temp = fixture();
    let dir = Directory::open(temp.path(), false).unwrap();
    let _held = dir
        .registry_lock(&observation_socket::NoCancel, Duration::from_secs(1))
        .unwrap();
    let start = Instant::now();
    assert_eq!(
        dir.registry_lock(&observation_socket::NoCancel, Duration::from_millis(20))
            .unwrap_err()
            .code(),
        "integration_registry_busy"
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    let cancel = Arc::new(Flag(AtomicBool::new(false)));
    let flag = cancel.clone();
    let worker = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        flag.0.store(true, Ordering::Release);
    });
    let start = Instant::now();
    assert_eq!(
        dir.registry_lock(&*cancel, Duration::from_secs(2))
            .unwrap_err()
            .code(),
        "integration_startup_cancelled"
    );
    worker.join().unwrap();
    assert!(start.elapsed() < Duration::from_secs(1));
}

fn monitor_failure(replace_api: bool) {
    let temp = fixture();
    let root = temp.path().join("s");
    let api_path = temp.path().join("a.sock");
    let _server = UnixListener::bind(&api_path).unwrap();
    let endpoint = selected(&api_path);
    let gate = Gate::new(temp.path());
    let mut children = Children(vec![gate.child(&root, &api_path, "before_publish")]);
    let (stage, mut blocked) = gate.arrive();
    assert_eq!(stage, "before_publish");
    let (instance, socket) = paths(&root, &endpoint).unwrap();
    let dir = Directory::open(&instance, false).unwrap();
    assert!(dir.socket().unwrap().is_some());
    let before = Receipt::read(&dir).unwrap();
    assert_eq!(before.receiver, None);
    assert!(!before.released);
    let _replacement = if replace_api {
        std::fs::rename(&api_path, temp.path().join("old-api.sock")).unwrap();
        let listener = UnixListener::bind(&api_path).unwrap();
        selected(&api_path);
        Some(listener)
    } else {
        // The retained receipt FD remains writable, but publication must fail its
        // real private-file admission rather than bypassing a metadata change.
        std::fs::set_permissions(
            instance.join("receipt.json"),
            std::fs::Permissions::from_mode(0o400),
        )
        .unwrap();
        None
    };
    blocked.write_all(&[1]).unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    let child = &mut children.0[0];
    assert!(child.stdin.is_some());
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < end,
            "monitor failure did not cancel receiver with stdin open"
        );
        thread::sleep(Duration::from_millis(5));
    };
    assert!(!status.success());
    let mut stderr = String::new();
    child
        .stderr
        .as_mut()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(
        stderr.contains(if replace_api {
            "integration_api_replaced"
        } else {
            "integration_unsafe_file"
        }),
        "{stderr}"
    );
    assert!(!Path::new(&socket).exists());
    let receipt: Receipt =
        serde_json::from_slice(&std::fs::read(instance.join("receipt.json")).unwrap()).unwrap();
    assert!(!receipt.released);
    assert_eq!(receipt.receiver, None);
}

#[test]
fn monitor_api_failure_cancels_and_joins_before_return_with_stdin_open() {
    monitor_failure(true);
}

#[test]
fn monitor_receipt_write_failure_cancels_and_joins_before_return_with_stdin_open() {
    monitor_failure(false);
}
struct Children(Vec<Child>);
impl Drop for Children {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn fixture() -> tempfile::TempDir {
    let temp = crate::private_access::test_support::tempdir();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    temp
}
fn selected(path: &Path) -> Endpoint {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    observation_socket::select(path.to_str().unwrap()).unwrap()
}
fn ready(root: &Path, api: &Endpoint, child: &mut Child) -> String {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let error = match resolve(root, api) {
            Ok(socket) => return socket,
            Err(error) => error,
        };
        if let Some(status) = child.try_wait().unwrap() {
            let mut stderr = String::new();
            child
                .stderr
                .as_mut()
                .unwrap()
                .read_to_string(&mut stderr)
                .unwrap();
            panic!("runtime exited {status}: {stderr}");
        }
        assert!(
            Instant::now() < until,
            "runtime did not become ready: {error}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn close(child: &mut Child) {
    drop(child.stdin.take());
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let mut error = String::new();
            child
                .stderr
                .as_mut()
                .unwrap()
                .read_to_string(&mut error)
                .unwrap();
            assert!(status.success(), "{error}");
            return;
        }
        assert!(Instant::now() < until, "runtime did not stop on EOF");
        thread::sleep(Duration::from_millis(5));
    }
}

// A real presentation worker continuously opens/replaces/polls this owned peer.
struct ReceiptKit {
    stop: Arc<AtomicBool>,
    polls: Arc<std::sync::atomic::AtomicUsize>,
    worker: Option<thread::JoinHandle<usize>>,
}

impl ReceiptKit {
    fn new(path: &Path) -> Self {
        let listener = UnixListener::bind(path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let polls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let polled = polls.clone();
        let worker = thread::spawn(move || {
            let mut revision = 0;
            let mut calls = 0;
            while !cancelled.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("fixture accept: {e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut frame = Vec::new();
                loop {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    frame.push(byte[0]);
                    assert!(frame.len() <= 128 * 1024);
                    if byte[0] == b'\n' {
                        break;
                    }
                }
                let request: serde_json::Value = serde_json::from_slice(&frame).unwrap();
                let result = match request["method"].as_str().unwrap() {
                    "session.snapshot" => {
                        serde_json::json!({"type":"session_snapshot","snapshot":{"version":"fixture","protocol":18,"integration_views":{"version":1,"sources":[{"id":"coven","epoch":null}]}}})
                    }
                    "integration.view.open" => {
                        serde_json::json!({"type":"view_opened","lease":"fixture","epoch":"fixture","revision":0,"ttl_ms":5000})
                    }
                    "integration.view.replace" => {
                        revision = request["params"]["revision"].as_u64().unwrap();
                        serde_json::json!({"type":"view_replaced","epoch":"fixture","revision":revision})
                    }
                    "integration.view.poll" => {
                        polled.fetch_add(1, Ordering::Release);
                        serde_json::json!({"type":"view_intents","epoch":"fixture","revision":revision,"ttl_ms":5000,"intents":[]})
                    }
                    "integration.view.close" => serde_json::json!({"type":"view_closed"}),
                    _ => panic!("unexpected fixture method"),
                };
                let mut reply =
                    serde_json::to_vec(&serde_json::json!({"id":request["id"],"result":result}))
                        .unwrap();
                reply.push(b'\n');
                stream.write_all(&reply).unwrap();
                calls += 1;
            }
            calls
        });
        Self {
            stop,
            polls,
            worker: Some(worker),
        }
    }
}
impl Drop for ReceiptKit {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}

#[test]
fn normal_slow_commit_acknowledges_after_commit_without_retry() {
    let temp = fixture();
    let root = temp.path().join("s");
    let api = temp.path().join("a.sock");
    let kit = ReceiptKit::new(&api);
    let endpoint = selected(&api);
    let mut expected_records = 0;
    for delay in [0, 300, 1100] {
        let mut command = runtime_command(&root, &api);
        command
            .env("COVEN_RECEIPT_TEST_TRACE", "1")
            .env("COVEN_RECEIPT_TEST_COMMIT_MS", delay.to_string());
        let polls = kit.polls.load(Ordering::Acquire);
        let mut children = Children(vec![command.spawn().unwrap()]);
        let socket = ready(&root, &endpoint, &mut children.0[0]);
        let end = Instant::now() + Duration::from_secs(5);
        while kit.polls.load(Ordering::Acquire) == polls {
            assert!(
                Instant::now() < end,
                "presentation did not poll the fixture"
            );
            thread::sleep(Duration::from_millis(5));
        }
        let count = 1;
        for _ in 0..count {
            let begin = Instant::now();
            let output = report(&root, &api, Some(("COVEN_RECEIPT_TEST_TRACE", "1")));
            eprintln!(
                "report_sample delay_ms={delay} elapsed_us={} success={}",
                begin.elapsed().as_micros(),
                output.status.success()
            );
            assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-"));
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            if delay < 1000 {
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            } else {
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains("agent_delivery_uncertain")
                );
            }
            if delay == 300 {
                assert!(begin.elapsed() >= Duration::from_millis(300));
            }
        }
        let (instance, _) = paths(&root, &endpoint).unwrap();
        if delay == 1100 {
            // Wait for the timed-out report's real storage guard to release,
            // without replaying a mutation or spending another client's budget
            // queued behind that still-running commit under concurrent disk load.
            use std::os::fd::AsRawFd;
            let lock = File::open(instance.join("w/state/.workspace.lock")).unwrap();
            let end = Instant::now() + Duration::from_secs(5);
            while unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } != 0 {
                assert!(Instant::now() < end, "commit did not release storage guard");
                thread::sleep(Duration::from_millis(5));
            }
        }
        // Native reports remain volatile and do not enter durable persistence.
        assert!(report(&root, &api, None).status.success());
        // A separate, read-only inventory request is not a mutation retry.
        let saved_path = instance.join("w/state/agent-management.json");
        let inventory = crate::agent_management::inventory_arguments(
            &crate::agent_management::InventoryRequest {
                socket: socket.clone(),
            },
            true,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&inventory.stdout).unwrap();
        expected_records += count;
        assert_eq!(value["records"].as_array().unwrap().len(), expected_records);
        // The inventory response synchronizes with completion of a report whose
        // caller timed out. Do not sample the durable image before that point.
        let after_inventory = std::fs::read(&saved_path).unwrap();
        drop(children.0[0].stdin.take());
        let end = Instant::now() + Duration::from_secs(5);
        while children.0[0].try_wait().unwrap().is_none() {
            assert!(Instant::now() < end, "runtime shutdown timed out");
            thread::sleep(Duration::from_millis(5));
        }
        assert!(children.0[0].try_wait().unwrap().unwrap().success());
        let mut trace = String::new();
        children.0[0]
            .stderr
            .as_mut()
            .unwrap()
            .read_to_string(&mut trace)
            .unwrap();
        assert!(!trace.contains("fixture-"));
        assert!(
            trace
                .lines()
                .filter(|line| line.starts_with("receipt_trace "))
                .count()
                <= 192
        );
        assert!(!trace.contains("reply_fence_failed"));
        assert_eq!(trace.matches("file_sync_begin").count(), count);
        assert_eq!(trace.matches("directory_sync_done").count(), count);
        assert_eq!(trace.matches(" accepted").count(), count + 2);
        assert!(!trace.contains("agent_connection_timeout"));
        if delay == 300 {
            assert!(trace.find("directory_sync_done").unwrap() < trace.find("sent").unwrap());
        }
        if delay == 1100 {
            assert!(trace.contains("input_or_storage_unavailable"));
        }
        eprintln!("{trace}");
        assert!(!Path::new(&socket).exists());
        // The durable image, not just the live projection, contains exactly one
        // mutation per independent report, including the timed-out client.
        let durable = std::fs::read(saved_path).unwrap();
        assert!(
            durable == after_inventory,
            "durable image changed after inventory"
        );
        let saved: serde_json::Value = serde_json::from_slice(&durable).unwrap();
        assert_eq!(
            saved["records"].as_object().unwrap().len(),
            expected_records
        );
        let mut stdout = String::new();
        children.0[0]
            .stdout
            .as_mut()
            .unwrap()
            .read_to_string(&mut stdout)
            .unwrap();
        assert!(!stdout.contains("fixture-"));
    }
}

#[test]
fn normal_expired_receive_budget_never_enters_handler() {
    let temp = fixture();
    let root = temp.path().join("s");
    let api = temp.path().join("a.sock");
    let kit = ReceiptKit::new(&api);
    let endpoint = selected(&api);
    let mut command = runtime_command(&root, &api);
    command
        .env("COVEN_RECEIPT_TEST_TRACE", "1")
        .env("COVEN_RECEIPT_TEST_ADMISSION_MS", "300");
    let mut children = Children(vec![command.spawn().unwrap()]);
    let socket = ready(&root, &endpoint, &mut children.0[0]);
    let end = Instant::now() + Duration::from_secs(5);
    while kit.polls.load(Ordering::Acquire) == 0 {
        assert!(
            Instant::now() < end,
            "presentation did not poll the fixture"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let output = report(&root, &api, Some(("COVEN_RECEIPT_TEST_TRACE", "1")));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("agent_delivery_uncertain"));
    drop(children.0[0].stdin.take());
    let end = Instant::now() + Duration::from_secs(5);
    while children.0[0].try_wait().unwrap().is_none() {
        assert!(Instant::now() < end, "runtime shutdown timed out");
        thread::sleep(Duration::from_millis(5));
    }
    assert!(children.0[0].try_wait().unwrap().unwrap().success());
    let mut trace = String::new();
    children.0[0]
        .stderr
        .as_mut()
        .unwrap()
        .read_to_string(&mut trace)
        .unwrap();
    assert!(trace.contains("received"));
    assert!(trace.contains("agent_connection_timeout"));
    assert!(!trace.contains("admitted"));
    assert!(!trace.contains("handled"));
    assert!(!trace.contains("file_sync_begin"));
    assert!(!trace.contains("fixture-"));
    assert!(
        trace
            .lines()
            .filter(|line| line.starts_with("receipt_trace "))
            .count()
            <= 192
    );
    let (instance, _) = paths(&root, &endpoint).unwrap();
    assert!(!instance.join("w/state/agent-management.json").exists());
    assert!(!Path::new(&socket).exists());
}

#[test]
fn actual_receivers_are_isolated_release_on_eof_and_reuse() {
    let temp = fixture();
    // Kit's API parent need not satisfy Coven receiver-parent privacy policy.
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let root = temp.path().join("s");
    let a = temp.path().join("a.sock");
    let b = temp.path().join("b.sock");
    let _a = UnixListener::bind(&a).unwrap();
    let _b = UnixListener::bind(&b).unwrap();
    let endpoint_a = selected(&a);
    let endpoint_b = selected(&b);
    let mut children = Children(vec![child(&root, &a)]);
    let socket_a = ready(&root, &endpoint_a, &mut children.0[0]);
    children.0.push(child(&root, &b));
    let socket_b = ready(&root, &endpoint_b, &mut children.0[1]);
    assert_ne!(socket_a, socket_b);
    assert!(report(&root, &a, None).status.success());
    for poison in [
        ("KITSUNE_AGENT_MANAGEMENT", ""),
        ("KITSUNE_INTEGRATION_SOURCE", "other"),
        ("KITSUNE_SOCKET_PATH", ""),
        ("KITSUNE_INTEGRATION_REPORT_COMMAND", "[\"/bin/sh\"]"),
    ] {
        assert!(!report(&root, &a, Some(poison)).status.success());
    }
    let mut duplicate = child(&root, &a);
    drop(duplicate.stdin.take());
    let output = duplicate.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE-SID"));
    close(&mut children.0[0]);
    assert!(!Path::new(&socket_a).exists());
    assert!(resolve(&root, &endpoint_a).is_err());
    assert!(resolve(&root, &endpoint_b).is_ok());
    children.0.push(child(&root, &a));
    assert_eq!(ready(&root, &endpoint_a, &mut children.0[2]), socket_a);
    close(&mut children.0[2]);
    close(&mut children.0[1]);
    // Released state permits the same API address with a genuinely new inode.
    std::fs::rename(&a, temp.path().join("old.sock")).unwrap();
    let _replacement = UnixListener::bind(&a).unwrap();
    let replacement = selected(&a);
    assert_ne!(replacement, endpoint_a);
    children.0.push(child(&root, &a));
    ready(&root, &replacement, &mut children.0[3]);
    close(&mut children.0[3]);
}

#[test]
fn crash_and_identity_replacement_never_adopt_or_unlink() {
    let temp = fixture();
    let root = temp.path().join("s");
    let api_path = temp.path().join("a.sock");
    let _server = UnixListener::bind(&api_path).unwrap();
    let endpoint = selected(&api_path);
    let mut children = Children(vec![child(&root, &api_path)]);
    let socket = ready(&root, &endpoint, &mut children.0[0]);
    std::fs::rename(&api_path, temp.path().join("old-api.sock")).unwrap();
    let _replacement = UnixListener::bind(&api_path).unwrap();
    let replacement = selected(&api_path);
    assert!(resolve(&root, &replacement).is_err());
    assert!(!report(&root, &api_path, None).status.success());
    // Keep the owned listener alive under a different name; never remove it or
    // a replacement node even though the transaction's receiver no longer matches.
    let old_socket = Path::new(&socket).with_file_name("old.sock");
    std::fs::rename(&socket, &old_socket).unwrap();
    let _unknown = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(resolve(&root, &endpoint).is_err());
    children.0[0].kill().unwrap();
    children.0[0].wait().unwrap();
    let output = child(&root, &api_path).wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(Path::new(&socket).exists());
    assert!(old_socket.exists());
    let (instance, _) = paths(&root, &replacement).unwrap();
    assert!(
        !Receipt::read(&Directory::open(&instance, false).unwrap())
            .unwrap()
            .released
    );
}

#[test]
fn immediate_eof_releases_without_manual_workspace_init() {
    let temp = fixture();
    let root = temp.path().join("s");
    let api_path = temp.path().join("a.sock");
    let _server = UnixListener::bind(&api_path).unwrap();
    let endpoint = selected(&api_path);
    for _ in 0..5 {
        let mut children = Children(vec![child(&root, &api_path)]);
        close(&mut children.0[0]);
        let (instance, _) = paths(&root, &endpoint).unwrap();
        assert!(
            Receipt::read(&Directory::open(&instance, false).unwrap())
                .unwrap()
                .released
        );
    }
}

#[test]
fn unsafe_directories_and_files_are_not_repaired() {
    let temp = fixture();
    let unsafe_dir = temp.path().join("public");
    std::fs::create_dir(&unsafe_dir).unwrap();
    std::fs::set_permissions(&unsafe_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(ensure_private_directory(&unsafe_dir).is_err());
    assert!(preflight_private_directory(&unsafe_dir).is_err());
    assert_eq!(
        std::fs::metadata(&unsafe_dir).unwrap().mode() & 0o7777,
        0o755
    );
    symlink(temp.path(), temp.path().join("link")).unwrap();
    assert!(ensure_private_directory(&temp.path().join("link/leaf")).is_err());
    assert!(preflight_private_directory(&temp.path().join("link/leaf")).is_err());
    let dir = Directory::open(temp.path(), false).unwrap();
    let mut file = dir.file(c"receipt.json", true).unwrap();
    dir.write(c"receipt.json", &mut file, b"{}").unwrap();
    std::fs::hard_link(temp.path().join("receipt.json"), temp.path().join("alias")).unwrap();
    assert!(dir.read(c"receipt.json").is_err());
}

#[test]
fn directory_preflight_is_read_only_and_matches_creation_policy() {
    let temp = fixture();
    let target = temp.path().join("missing/tail");
    // A normal owned 0755 creation parent is allowed; it is not repaired to 0700.
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let before = std::fs::metadata(temp.path()).unwrap();
    preflight_private_directory(&target).unwrap();
    let after = std::fs::metadata(temp.path()).unwrap();
    assert!(!temp.path().join("missing").exists());
    assert_eq!(
        (
            before.mode(),
            before.mtime(),
            before.mtime_nsec(),
            before.ctime(),
            before.ctime_nsec()
        ),
        (
            after.mode(),
            after.mtime(),
            after.mtime_nsec(),
            after.ctime(),
            after.ctime_nsec()
        )
    );
    ensure_private_directory(&target).unwrap();
    preflight_private_directory(&target).unwrap();
    assert_eq!(std::fs::metadata(&target).unwrap().mode() & 0o7777, 0o700);
    let unsafe_parent = temp.path().join("unsafe");
    std::fs::create_dir(&unsafe_parent).unwrap();
    std::fs::set_permissions(&unsafe_parent, std::fs::Permissions::from_mode(0o777)).unwrap();
    let missing = unsafe_parent.join("tail/deeper");
    let before = std::fs::metadata(&unsafe_parent).unwrap();
    assert_eq!(
        preflight_private_directory(&missing).unwrap_err().code(),
        "integration_unsafe_creation_parent"
    );
    assert!(ensure_private_directory(&missing).is_err());
    assert!(!unsafe_parent.join("tail").exists());
    let after = std::fs::metadata(&unsafe_parent).unwrap();
    assert_eq!(
        (before.mode(), before.mtime(), before.ctime()),
        (after.mode(), after.mtime(), after.ctime())
    );
}

fn readonly_creation_permission(parent: &Path, missing: &Path) {
    use std::os::fd::AsRawFd;
    let file = File::open(parent).unwrap();
    let before = file.metadata().unwrap();
    assert!(
        matches!(std::fs::symlink_metadata(missing), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
    );
    let allowed = unsafe {
        libc::faccessat(
            file.as_raw_fd(),
            c".".as_ptr(),
            libc::W_OK | libc::X_OK,
            libc::AT_EACCESS,
        )
    } == 0;
    // Root/capability-enabled runs must follow the OS result, not assume that
    // owner 0500 or another owner's 0755 necessarily forbids effective access.
    let result = preflight_private_directory(missing);
    if allowed {
        result.unwrap();
    } else {
        assert_eq!(
            result.unwrap_err().code(),
            "integration_creation_parent_access_denied"
        );
        // Never attempt creation in a system directory, even in a privileged test.
        // Exercise setup only when effective access must deny it before mutation.
        let config = fixture();
        let path = config.path().join("config.toml");
        let bytes = b"# isolated configuration\nother = 3\n";
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let config_before = std::fs::metadata(&path).unwrap();
        for (operation, apply) in [
            (crate::integration_setup::Operation::Plan, false),
            (crate::integration_setup::Operation::Enable, false),
            (crate::integration_setup::Operation::Enable, true),
        ] {
            let request = crate::integration_setup::Request {
                operation,
                kitsune_config: Some(path.clone()),
                executable: Some(std::env::current_exe().unwrap()),
                state_root: Some(missing.into()),
                apply,
            };
            assert_eq!(
                crate::integration_setup::arguments(&request, true)
                    .err()
                    .unwrap()
                    .code(),
                "integration_creation_parent_access_denied"
            );
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert_eq!(std::fs::read_dir(config.path()).unwrap().count(), 1);
        }
        let config_after = std::fs::metadata(&path).unwrap();
        assert_eq!(
            (
                config_before.mode(),
                config_before.mtime(),
                config_before.mtime_nsec(),
                config_before.ctime(),
                config_before.ctime_nsec()
            ),
            (
                config_after.mode(),
                config_after.mtime(),
                config_after.mtime_nsec(),
                config_after.ctime(),
                config_after.ctime_nsec()
            )
        );
    }
    assert!(
        matches!(std::fs::symlink_metadata(missing), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
    );
    let after = file.metadata().unwrap();
    assert_eq!(
        (
            before.mode(),
            before.mtime(),
            before.mtime_nsec(),
            before.ctime(),
            before.ctime_nsec()
        ),
        (
            after.mode(),
            after.mtime(),
            after.mtime_nsec(),
            after.ctime(),
            after.ctime_nsec()
        )
    );
}

#[test]
fn readonly_creation_preflight_checks_effective_access_on_owned_0500_parent() {
    let temp = fixture();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    readonly_creation_permission(temp.path(), &temp.path().join("x"));
}

#[test]
fn readonly_creation_preflight_checks_effective_access_on_root_owned_0755_system_parent() {
    let parent = Path::new("/usr");
    let metadata = std::fs::symlink_metadata(parent).unwrap();
    assert!(metadata.is_dir());
    assert_eq!(metadata.uid(), 0);
    assert_eq!(metadata.mode() & 0o7777, 0o755);
    let missing = parent.join(format!(
        "coven-preflight-{}",
        crate::platform::random_uuid_hex().unwrap()
    ));
    readonly_creation_permission(parent, &missing);
}

#[cfg(target_os = "linux")]
#[test]
fn directory_preflight_rejects_masked_access_and_inherited_default_acls_without_writes() {
    use std::os::fd::AsRawFd;
    for name in [c"system.posix_acl_access", c"system.posix_acl_default"] {
        let temp = fixture();
        let file = std::fs::File::open(temp.path()).unwrap();
        let mut acl = 2u32.to_le_bytes().to_vec();
        for (tag, perms, id) in [
            (1u16, 7u16, u32::MAX),
            (2, 4, unsafe { libc::geteuid() } + 1),
            (4, 0, u32::MAX),
            (16, 0, u32::MAX),
            (32, 0, u32::MAX),
        ] {
            acl.extend(tag.to_le_bytes());
            acl.extend(perms.to_le_bytes());
            acl.extend(id.to_le_bytes());
        }
        let result = unsafe {
            libc::fsetxattr(
                file.as_raw_fd(),
                name.as_ptr(),
                acl.as_ptr().cast(),
                acl.len(),
                0,
            )
        };
        if result != 0 {
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ENOTSUP)
            );
            continue;
        }
        assert_eq!(file.metadata().unwrap().mode() & 0o7777, 0o700);
        let target = temp.path().join("missing/tail");
        let before = file.metadata().unwrap();
        assert_eq!(
            preflight_private_directory(temp.path()).unwrap_err().code(),
            "integration_acl_denied"
        );
        assert_eq!(
            preflight_private_directory(&target).unwrap_err().code(),
            "integration_acl_denied"
        );
        assert_eq!(
            ensure_private_directory(&target).unwrap_err().code(),
            "integration_acl_denied"
        );
        assert!(!temp.path().join("missing").exists());
        let after = file.metadata().unwrap();
        assert_eq!(
            (
                before.mode(),
                before.mtime(),
                before.mtime_nsec(),
                before.ctime(),
                before.ctime_nsec()
            ),
            (
                after.mode(),
                after.mtime(),
                after.mtime_nsec(),
                after.ctime(),
                after.ctime_nsec()
            )
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn masked_posix_acl_is_refused_even_with_exact_private_mode() {
    use std::os::fd::AsRawFd;
    let temp = fixture();
    let dir = Directory::open(temp.path(), false).unwrap();
    let file = dir.file(c"acl", true).unwrap();
    let mut acl = 2u32.to_le_bytes().to_vec();
    for (tag, perms, id) in [
        (1u16, 6u16, u32::MAX),
        (2, 4, unsafe { libc::geteuid() } + 1),
        (4, 0, u32::MAX),
        (16, 0, u32::MAX),
        (32, 0, u32::MAX),
    ] {
        acl.extend(tag.to_le_bytes());
        acl.extend(perms.to_le_bytes());
        acl.extend(id.to_le_bytes());
    }
    let result = unsafe {
        libc::fsetxattr(
            file.as_raw_fd(),
            c"system.posix_acl_access".as_ptr(),
            acl.as_ptr().cast(),
            acl.len(),
            0,
        )
    };
    if result != 0 {
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ENOTSUP)
        );
        return;
    }
    assert_eq!(file.metadata().unwrap().mode() & 0o7777, 0o600);
    assert!(dir.file(c"acl", false).is_err());
}

#[test]
fn bounded_instance_admission_requires_deliberate_cleanup() {
    let temp = fixture();
    let root = temp.path().join("s");
    for n in 0..64 {
        ensure_private_directory(&root.join("i").join(n.to_string())).unwrap();
    }
    let api_path = temp.path().join("a.sock");
    let _server = UnixListener::bind(&api_path).unwrap();
    let endpoint = selected(&api_path);
    let output = child(&root, &api_path).wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("integration_instance_limit_cleanup_required")
    );
    let (instance, _) = paths(&root, &endpoint).unwrap();
    assert!(!instance.exists());
    assert_eq!(std::fs::read_dir(root.join("i")).unwrap().count(), 64);
}

#[test]
fn partial_state_is_preserved_and_api_replacement_is_fenced() {
    let temp = fixture();
    let root = temp.path().join("s");
    let api_path = temp.path().join("a.sock");
    let server = UnixListener::bind(&api_path).unwrap();
    let endpoint = selected(&api_path);
    let (instance, _) = paths(&root, &endpoint).unwrap();
    ensure_private_directory(&instance).unwrap();
    let dir = Directory::open(&instance, false).unwrap();
    let mut file = dir.file(RECEIPT, true).unwrap();
    dir.write(RECEIPT, &mut file, b"partial").unwrap();
    let child = child(&root, &api_path);
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert_eq!(dir.read(RECEIPT).unwrap(), b"partial");
    assert!(resolve(&root, &endpoint).is_err());
    // Keep the old inode alive so the replacement cannot reuse it.
    std::fs::rename(&api_path, temp.path().join("old.sock")).unwrap();
    let _replacement = UnixListener::bind(&api_path).unwrap();
    assert_ne!(selected(&api_path), endpoint);
    drop(server);
}

#[test]
fn route_is_closed_exact_and_never_executes_unknown_argv() {
    let exe = std::env::current_exe().unwrap();
    let root = Path::new("/private/coven");
    let argv = serde_json::json!([
        exe,
        "agent",
        "report-native",
        "--from-kitsune",
        "--state-root",
        root
    ]);
    assert_eq!(route(&argv.to_string(), None, &exe).unwrap(), root);
    assert!(route(&argv.to_string(), Some(Path::new("/other")), &exe).is_err());
    let mut poisoned = argv.clone();
    poisoned[2] = serde_json::json!("report");
    assert!(route(&poisoned.to_string(), None, &exe).is_err());
    poisoned = argv.clone();
    poisoned[0] = serde_json::json!("/bin/sh");
    assert!(route(&poisoned.to_string(), None, &exe).is_err());
    poisoned = argv;
    poisoned
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("--json"));
    assert!(route(&poisoned.to_string(), None, &exe).is_err());
    assert!(
        paths(
            &PathBuf::from(format!("/{}", "x".repeat(120))),
            &Endpoint {
                selected: "/api".into(),
                resolved: "/api".into(),
                device: 1,
                inode: 2
            }
        )
        .is_err()
    );
}
