//! Synthetic Kit hosting, actual AppRuntime native waits/CAS and automatic decision wire.
//! No model, transcript, live endpoint, or browser is involved.
use super::*;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
};

const AUTH: &str = "Tailscale-User-Login: fixture@example.invalid\r\n";
const OTHER: &str = "Tailscale-User-Login: other@example.invalid\r\n";
const GENERATION: &str = "11111111-2222-4333-8444-555555555555";

struct Fixture {
    child: Child,
    events: Receiver<Value>,
    readers: Vec<std::thread::JoinHandle<()>>,
    kit: Option<std::thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    ready: Value,
    root: tempfile::TempDir,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // EOF lets the native fixture interrupt/reap its fibers and unpublish first.
        drop(self.child.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        self.stop.store(true, Ordering::Release);
        if let Some(kit) = self.kit.take() {
            let _ = kit.join();
        }
    }
}

impl Fixture {
    fn start(mode: &str) -> Self {
        let script = PathBuf::from(
            std::env::var_os("COVEN_DJINN_DECISION_FIXTURE")
                .expect("set COVEN_DJINN_DECISION_FIXTURE to absolute decision-control.fixture.ts"),
        );
        assert!(script.is_absolute() && script.is_file());
        let tmp = std::fs::canonicalize(
            std::env::var_os("TMPDIR").expect("short disk-backed TMPDIR required"),
        )
        .unwrap();
        assert!(!tmp.starts_with("/tmp") && !tmp.starts_with("/dev/shm"));
        let root = tempfile::Builder::new()
            .prefix("d")
            .rand_bytes(2)
            .tempdir_in(tmp)
            .unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            root.path()
                .join(".djw-XXXXXX/.00000000-0000-0000-0000-000000000000")
                .as_os_str()
                .len()
                <= 100
        );
        let socket = root.path().join("k");
        let listener = UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let metadata = std::fs::metadata(&socket).unwrap();
        let target = json!({"socket":socket,"device":metadata.dev(),"inode":metadata.ino(),
            "terminal_id":"terminal","runtime_generation":GENERATION});
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let kit = std::thread::spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("Kit accept: {e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut line = String::new();
                if BufReader::new(&stream)
                    .take(8192)
                    .read_line(&mut line)
                    .unwrap_or(0)
                    == 0
                {
                    continue;
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                let pane = json!({"terminal_id":"terminal","pane_id":"pane","workspace_id":"workspace","tab_id":"tab"});
                let result = match request["method"].as_str().unwrap() {
                    "session.snapshot" => json!({"type":"session_snapshot","snapshot":{
                        "protocol":18,"version":"fixture-v1","terminal_navigation":{"version":1},
                        "panes":[pane],"agents":[{"terminal_id":"terminal","pane_id":"pane",
                            "workspace_id":"workspace","tab_id":"tab","agent_status":"blocked","agent":"djinn"}]}}),
                    "terminal.inspect" => {
                        json!({"type":"terminal_inspected","terminal_id":"terminal",
                        "runtime_generation":GENERATION,"availability":"live","workspace_id":"workspace","tab_id":"tab","pane_id":"pane"})
                    }
                    method => panic!("unexpected Kit method: {method}"),
                };
                let _ = writeln!(stream, "{}", json!({"id":request["id"],"result":result}));
            }
        });
        let child = Command::new("bun")
            .arg(script)
            .arg(root.path())
            .arg(mode)
            .arg(target.to_string())
            .env(
                "BUN_RUNTIME_TRANSPILER_CACHE_PATH",
                root.path().join("bun-cache"),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (send, events) = mpsc::sync_channel(128);
        let mut fixture = Self {
            child,
            events,
            readers: vec![],
            kit: Some(kit),
            stop,
            ready: Value::Null,
            root,
        };
        let stdout = fixture.child.stdout.take().unwrap();
        fixture.readers.push(std::thread::spawn(move || {
            let mut input = BufReader::new(stdout.take(128 * 8193));
            for _ in 0..128 {
                let mut line = String::new();
                if input.by_ref().take(8193).read_line(&mut line).unwrap_or(0) == 0
                    || line.len() > 8192
                {
                    break;
                }
                if let Ok(event) = serde_json::from_str(&line) {
                    if send.try_send(event).is_err() {
                        break;
                    }
                }
            }
        }));
        let stderr = fixture.child.stderr.take().unwrap();
        fixture.readers.push(std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stderr.take(16384).read_to_end(&mut bytes);
            if !bytes.is_empty() {
                eprintln!(
                    "native fixture bounded stderr: {}",
                    String::from_utf8_lossy(&bytes)
                );
            }
        }));
        fixture.ready = fixture.event();
        assert_eq!(fixture.ready["type"], "ready");
        assert_eq!(fixture.ready["synthetic"], true);
        assert_eq!(fixture.ready["pending"], 2);
        assert_eq!(fixture.ready["settled"], 0);
        fixture
    }

    fn event(&self) -> Value {
        self.events
            .recv_timeout(Duration::from_secs(20))
            .expect("native fixture event deadline")
    }

    fn control(&mut self, command: &str, value: bool) -> Value {
        writeln!(
            self.child.stdin.as_mut().unwrap(),
            "{}",
            json!({"command":command,"value":value})
        )
        .unwrap();
        let result = self.event();
        assert_eq!(result["type"], "control", "{result}");
        assert_eq!(result["command"], command);
        result
    }

    fn counts(&mut self, pending: u64, settled: u64, rejected: u64) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let counts = self.control("counts", false);
            if counts["pending"] == pending
                && counts["settled"] == settled
                && counts["rejected"] == rejected
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "actual native counts: {counts}; expected {pending}/{settled}/{rejected}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn gateway(&self, writer: bool) -> Gateway {
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path().join("work");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::write(
            workspace.join("coven.json"),
            br#"{"name":"Fixture","agents":["alpha"],"multiplexer":"herdr"}"#,
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let config = directory.path().join("observer.json");
        std::fs::write(&config, serde_json::to_vec(&json!({
            "listen":format!("127.0.0.1:{port}"),"public_origin":"https://coven-web.example.invalid",
            "allowed_logins":["fixture@example.invalid","other@example.invalid"],
            "conversation_sources":[{"id":"native","label":"Native fixture","team":"research",
                "directory":self.ready["registry"],"allowed_logins":["fixture@example.invalid"],
                "decision_logins":if writer { vec!["fixture@example.invalid"] } else { vec![] },"control_workspace":workspace}],
            "endpoints":[{"id":"kit","label":"Synthetic Kit","socket":self.root.path().join("k")}]
        })).unwrap()).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_coven"))
            .args(["observer", "serve", "--config"])
            .arg(&config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut gateway = Gateway {
            child,
            port,
            _directory: directory,
        };
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            assert!(
                gateway.child.try_wait().unwrap().is_none(),
                "gateway exited"
            );
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        gateway
    }

    fn sanitized(&self, value: &Value) {
        let text = value.to_string();
        for secret in [
            self.ready["session_id"].as_str().unwrap(),
            self.ready["runtime_id"].as_str().unwrap(),
            self.root.path().to_str().unwrap(),
            "request_id",
            "occurrence_id",
            "details_sha256",
            "grant_id",
            "\"tool\"",
        ] {
            assert!(
                !text.contains(secret),
                "public response leaked {secret}: {text}"
            );
        }
    }

    fn wire(&self, method: &str, id: &str, params: Value) -> Value {
        let mut stream = UnixStream::connect(self.ready["socket"].as_str().unwrap()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        writeln!(
            stream,
            "{}",
            json!({"version":1,"id":id,"method":method,"params":params})
        )
        .unwrap();
        let mut line = String::new();
        BufReader::new(stream)
            .take(65537)
            .read_line(&mut line)
            .unwrap();
        serde_json::from_str(&line).unwrap()
    }

    fn stop(mut self) {
        self.control("stop", false);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "native shutdown deadline");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!std::path::Path::new(self.ready["socket"].as_str().unwrap()).exists());
        assert!(!std::path::Path::new(self.ready["registry"].as_str().unwrap()).exists());
    }
}

fn body(response: &str) -> Value {
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}
fn get(gateway: &Gateway, path: &str) -> Value {
    let response = gateway.request(path, AUTH);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    body(&response)
}
fn attention(gateway: &Gateway, count: usize) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let value = get(gateway, "/api/v1/attention");
        if value["state"] == "observed" && value["items"].as_array().unwrap().len() == count {
            return value;
        }
        assert!(Instant::now() < deadline, "attention deadline: {value}");
        std::thread::sleep(Duration::from_millis(30));
    }
}
fn item(value: &Value, kind: &str) -> Value {
    value["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == kind)
        .unwrap()
        .clone()
}
fn post(gateway: &Gateway, row: &Value, login: &str, attempt: &str, action: Value) -> String {
    // Fresh principal-bound CSRF on every deliberate HTTP action.
    let token = body(&gateway.request("/api/v1/csrf", login))["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let bytes = serde_json::to_vec(&json!({"contract":"coven.decision-response.v1","attempt_id":attempt,"execute":true,"action":action})).unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", gateway.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(6)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(stream,"POST /api/v1/decisions/{}/responses HTTP/1.1\r\nHost: coven-web.example.invalid\r\n{login}Origin: https://coven-web.example.invalid\r\nSec-Fetch-Site: same-origin\r\nContent-Type: application/json\r\nX-Coven-CSRF: {token}\r\nContent-Length: {}\r\n\r\n",row["id"].as_str().unwrap(),bytes.len()).unwrap();
    stream.write_all(&bytes).unwrap();
    let mut response = String::new();
    stream.take(65537).read_to_string(&mut response).unwrap();
    response
}
fn applied(response: &str) {
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(body(response)["outcome"], "applied", "{response}");
    assert_eq!(body(response)["delivery"], "applied");
}
fn attempt(n: u32) -> String {
    format!("{n:08x}-2222-4333-8444-555555555555")
}

#[test]
#[ignore = "actual native service producer: set COVEN_DJINN_DECISION_FIXTURE and short disk-backed TMPDIR"]
fn actual_native_question_permission_http_settlement_and_lost_receipt() {
    let mut native = Fixture::start("read_write");
    let gateway = native.gateway(true);
    let metadata = attention(&gateway, 2);
    native.sanitized(&metadata);
    assert!(!metadata.to_string().contains("Synthetic choice"));
    assert!(!metadata.to_string().contains("synthetic-only"));
    let listing = get(&gateway, "/api/v1/conversations");
    let conversation = &listing["conversations"][0];
    assert_eq!(listing["conversations"].as_array().unwrap().len(), 1);
    assert_eq!(conversation["association_status"], "verified");
    assert_eq!(conversation["can_submit"], false); // No model configured; decision writer is independent.
    let question = item(&metadata, "question");
    for field in ["fleet_agent_id", "fleet_endpoint_id", "binding_id"] {
        assert_eq!(question[field], conversation["card"][field]);
    }
    assert_eq!(question["can_respond"], true);
    let details = get(
        &gateway,
        &format!("/api/v1/decisions/{}", question["id"].as_str().unwrap()),
    );
    native.sanitized(&details);
    assert_eq!(
        details["decision"]["questions"][0]["question"],
        "Synthetic choice only"
    );
    assert_eq!(details["decision"]["questions"][0]["custom"], false);
    let response = post(
        &gateway,
        &question,
        AUTH,
        &attempt(1),
        json!({"type":"question_answer","answers":[["A"]]}),
    );
    applied(&response);
    native.sanitized(&body(&response));
    native.counts(1, 1, 0);

    // The exact same UUID cannot reach a follow-up claim, even via the private wire.
    let status = native.wire(
        "share_status",
        &attempt(90),
        json!({"expected_runtime_id":native.ready["runtime_id"]}),
    );
    let grant = &status["result"]["grant"];
    assert!(grant.is_object(), "{status}");
    let duplicate = native.wire("shared_submit",&attempt(1),json!({"expected_runtime_id":native.ready["runtime_id"],
        "session_id":native.ready["session_id"],"grant_id":grant["grant_id"],"text":"synthetic never admitted"}));
    assert_eq!(
        duplicate["error"]["code"], "request_already_seen",
        "{duplicate}"
    );
    native.counts(1, 1, 0);

    let permission = item(&attention(&gateway, 1), "permission");
    let detail = get(
        &gateway,
        &format!("/api/v1/decisions/{}", permission["id"].as_str().unwrap()),
    );
    native.sanitized(&detail);
    assert_eq!(detail["decision"]["patterns"], json!(["synthetic-only"]));
    assert_eq!(detail["decision"]["deny_scope"], "session_pending");
    applied(&post(
        &gateway,
        &permission,
        AUTH,
        &attempt(2),
        json!({"type":"permission_once"}),
    ));
    native.counts(0, 2, 0);
    native.control("permission", false); // Once has not installed remembered allow rules.
    native.counts(1, 2, 0);
    let permission = item(&attention(&gateway, 1), "permission");
    applied(&post(
        &gateway,
        &permission,
        AUTH,
        &attempt(3),
        json!({"type":"permission_deny","scope":"session_pending"}),
    ));
    native.counts(0, 2, 1);
    native.control("permission", false); // Denial does not capture a future occurrence.
    native.counts(1, 2, 1);
    native.control("question", false);
    let question = item(&attention(&gateway, 2), "question");
    native.control("lost_ack", true);
    let lost = post(
        &gateway,
        &question,
        AUTH,
        &attempt(4),
        json!({"type":"question_answer","answers":[["B"]]}),
    );
    assert!(lost.starts_with("HTTP/1.1 200"), "{lost}");
    assert_eq!(body(&lost)["outcome"], "uncertain");
    assert_eq!(body(&lost)["delivery"], "uncertain");
    native.counts(1, 3, 1);
    native.control("question", false);
    let fresh = item(&attention(&gateway, 2), "question");
    assert_ne!(fresh["id"], question["id"]);
    assert_ne!(fresh["alert_id"], question["alert_id"]);
    assert!(
        !gateway
            .request(
                &format!("/api/v1/decisions/{}", question["id"].as_str().unwrap()),
                AUTH
            )
            .starts_with("HTTP/1.1 200")
    );
    let retry = post(
        &gateway,
        &fresh,
        AUTH,
        &attempt(4),
        json!({"type":"question_answer","answers":[["A"]]}),
    );
    assert_ne!(body(&retry)["outcome"], "applied", "{retry}");
    attention(&gateway, 2); // Refresh cannot retarget the old request or automatically act.
    native.counts(2, 3, 1);
    drop(gateway);
    native.stop();
}

#[test]
#[ignore = "actual native service producer: set COVEN_DJINN_DECISION_FIXTURE and short disk-backed TMPDIR"]
fn actual_native_refusals_readmode_acl_invalid_actions_and_occurrence_fences() {
    let mut native = Fixture::start("read");
    let gateway = native.gateway(true);
    let question = item(&attention(&gateway, 2), "question");
    assert_eq!(question["can_respond"], false);
    let detail = get(
        &gateway,
        &format!("/api/v1/decisions/{}", question["id"].as_str().unwrap()),
    );
    assert_eq!(detail["can_respond"], false);
    let refused = post(
        &gateway,
        &question,
        AUTH,
        &attempt(10),
        json!({"type":"question_reject"}),
    );
    assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
    native.counts(2, 0, 0);
    native.control("readmode", false);
    let question = item(&attention(&gateway, 2), "question");
    assert_eq!(question["can_respond"], true);
    let no_writer = native.gateway(false);
    let readonly = item(&attention(&no_writer, 2), "question");
    assert_eq!(readonly["can_respond"], false);
    for (server, row, login) in [(&no_writer, &readonly, AUTH), (&gateway, &question, OTHER)] {
        let refused = post(
            server,
            row,
            login,
            &attempt(11),
            json!({"type":"question_reject"}),
        );
        assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
    }
    native.counts(2, 0, 0);
    for action in [
        json!({"type":"permission_always"}),
        json!({"type":"question_answer","answers":[["unknown"]]}),
        json!({"type":"permission_deny","scope":"future"}),
    ] {
        let rejected = post(&gateway, &question, AUTH, &attempt(12), action);
        assert_ne!(body(&rejected)["outcome"], "applied", "{rejected}");
    }
    native.counts(2, 0, 0);
    let old = item(&attention(&gateway, 2), "permission");
    native.control("replace", false);
    native.counts(2, 0, 1);
    let fresh = item(&attention(&gateway, 2), "permission");
    assert_ne!(old["id"], fresh["id"]);
    assert_ne!(old["alert_id"], fresh["alert_id"]);
    let stale = post(
        &gateway,
        &old,
        AUTH,
        &attempt(13),
        json!({"type":"permission_once"}),
    );
    assert_ne!(body(&stale)["outcome"], "applied", "{stale}");
    assert!(
        !gateway
            .request(
                &format!("/api/v1/decisions/{}", old["id"].as_str().unwrap()),
                AUTH
            )
            .starts_with("HTTP/1.1 200")
    );
    native.counts(2, 0, 1);
    native.control("cancel", false);
    native.counts(0, 0, 1);
    attention(&gateway, 0);
    native.control("question", false);
    attention(&gateway, 1);
    let cancelled = post(
        &gateway,
        &question,
        AUTH,
        &attempt(14),
        json!({"type":"question_answer","answers":[["A"]]}),
    );
    assert_ne!(body(&cancelled)["outcome"], "applied", "{cancelled}");
    native.counts(1, 0, 1);
    drop(no_writer);
    drop(gateway);
    native.stop();
}
