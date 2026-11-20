//! Opt-in acceptance against actual WebSharing/Publication/Observer/PromptControl.
use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader},
    os::unix::fs::{MetadataExt, PermissionsExt},
    sync::mpsc::{self, Receiver},
};

const AUTH: &str = "Tailscale-User-Login: fixture@example.invalid\r\n";
const OTHER: &str = "Tailscale-User-Login: other@example.invalid\r\n";
const REQUEST: &str = "31111111-2222-4333-8444-555555555555";
const LOST_REQUEST: &str = "41111111-2222-4333-8444-555555555555";
const TEXT: &str = "<script>literal follow-up</script>\t\r\n\"\\ exact";

struct Producer {
    child: Child,
    reader: Option<std::thread::JoinHandle<()>>,
    errors: Option<std::thread::JoinHandle<()>>,
    diagnostics: Receiver<Vec<u8>>,
    events: Receiver<Value>,
    runtime: String,
    root: tempfile::TempDir,
}

impl Drop for Producer {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if let Some(errors) = self.errors.take() {
            let _ = errors.join();
        }
        // TempDir is removed only after the child and its stdout reader stop.
    }
}

impl Producer {
    fn start(mode: &str) -> Self {
        let script = std::env::var_os("COVEN_DJINN_SHARING_FIXTURE")
            .expect("set COVEN_DJINN_SHARING_FIXTURE to the absolute web-sharing.fixture.ts path");
        let script = std::path::Path::new(&script);
        assert!(
            script.is_absolute() && script.is_file(),
            "absolute existing fixture required"
        );
        let tmp = std::env::var_os("TMPDIR")
            .expect("set TMPDIR to a short private disk-backed run directory");
        let tmp = std::fs::canonicalize(tmp).unwrap();
        assert!(!tmp.starts_with("/tmp") && !tmp.starts_with("/dev/shm"));
        let root = tempfile::Builder::new()
            .prefix("s")
            .rand_bytes(3)
            .tempdir_in(tmp)
            .unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["r", "work", "config", "home", "cache"] {
            let path = root.path().join(name);
            std::fs::create_dir(&path).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert!(
            root.path()
                .join(".djw-XXXXXX/.11111111-2222-4333-8444-555555555555")
                .as_os_str()
                .len()
                <= 100,
            "TMPDIR must leave room for the native socket path"
        );
        std::fs::write(
            root.path().join("work/coven.json"),
            br#"{"name":"Fixture","agents":["alpha"],"multiplexer":"herdr"}"#,
        )
        .unwrap();
        let child = Command::new("bun")
            .arg(script)
            .arg(root.path().join("r"))
            .arg(mode)
            .current_dir(root.path().join("work"))
            .env("HOME", root.path().join("home"))
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .env("XDG_CACHE_HOME", root.path().join("cache"))
            .env("XDG_RUNTIME_DIR", root.path())
            .env(
                "BUN_RUNTIME_TRANSPILER_CACHE_PATH",
                root.path().join("cache"),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start installed Bun, without installing dependencies");
        let (send, events) = mpsc::sync_channel(64);
        let (report, diagnostics) = mpsc::sync_channel(1);
        let mut producer = Self {
            child,
            reader: None,
            errors: None,
            diagnostics,
            events,
            runtime: String::new(),
            root,
        };
        let stderr = producer.child.stderr.take().unwrap();
        producer.errors = Some(std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stderr.take(16384).read_to_end(&mut bytes);
            let _ = report.send(bytes);
        }));
        let stdout = producer.child.stdout.take().unwrap();
        producer.reader = Some(std::thread::spawn(move || {
            let mut input = BufReader::new(stdout.take(64 * 4097));
            for _ in 0..64 {
                let mut line = String::new();
                match input.by_ref().take(4097).read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if line.len() > 4096 {
                    break;
                }
                let Ok(event) = serde_json::from_str(&line) else {
                    break;
                };
                if send.try_send(event).is_err() {
                    break;
                }
            }
        }));
        let ready = producer.event();
        assert_eq!(ready["type"], "ready");
        assert_eq!(ready["session_id"], "ses_fixture");
        producer.runtime = ready["runtime_id"].as_str().unwrap().to_owned();
        let registry = producer.root.path().join("r");
        assert_eq!(std::fs::metadata(&registry).unwrap().mode() & 0o777, 0o700);
        let descriptor: Value = serde_json::from_slice(
            &std::fs::read(registry.join(format!("{}.json", producer.runtime))).unwrap(),
        )
        .unwrap();
        let socket = descriptor["native_socket"].as_str().unwrap();
        assert!(socket.len() <= 100 && std::path::Path::new(socket).exists());
        producer
    }

    fn event(&self) -> Value {
        self.events
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|error| {
                let bytes = self
                    .diagnostics
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap_or_default();
                panic!(
                    "native stdout event failed: {error}; bounded stderr: {}",
                    String::from_utf8_lossy(&bytes)
                );
            })
    }

    fn control(&mut self, command: &str, value: bool, count: u64) {
        writeln!(
            self.child.stdin.as_mut().unwrap(),
            "{}",
            json!({"command":command,"value":value})
        )
        .unwrap();
        let ack = self.event();
        assert_eq!(
            ack,
            json!({"type":"control","command":command,"count":count})
        );
    }

    fn admitted(&self, count: u64) {
        assert_eq!(self.event(), json!({"type":"admitted","count":count}));
    }

    fn gateway(&self) -> Gateway {
        Gateway::start_options(
            None,
            Some((&self.root.path().join("r"), &self.root.path().join("work"))),
        )
    }

    fn sanitized(&self, response: &str) {
        for secret in [
            "ses_fixture",
            "msg_fixture",
            "msg_accepted",
            &self.runtime,
            self.root.path().to_str().unwrap(),
            "native_socket",
            "grant_id",
        ] {
            assert!(
                !response.contains(secret),
                "HTTP leaked {secret}: {response}"
            );
        }
    }

    fn wait_stopped(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "producer shutdown deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read_dir(self.root.path().join("r"))
                .unwrap()
                .count(),
            0
        );
        assert!(std::fs::read_dir(self.root.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .as_encoded_bytes()
                .starts_with(b".djw-")
        }));
    }
}

fn body(response: &str) -> Value {
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn listing(gateway: &Gateway, count: usize) -> Value {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let response = gateway.request("/api/v1/conversations", AUTH);
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let value = body(&response);
        if value["conversations"].as_array().unwrap().len() == count {
            return value;
        }
        assert!(Instant::now() < deadline, "discovery deadline: {response}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn post(
    gateway: &Gateway,
    id: &str,
    login: &str,
    token: &str,
    request: &str,
    text: &str,
    send_body: bool,
) -> String {
    let bytes = serde_json::to_vec(&json!({"contract":"coven.conversation-submit.v1","request_id":request,"execute":true,"text":text})).unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", gateway.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(6)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(stream, "POST /api/v1/conversations/{id}/prompts HTTP/1.1\r\nHost: coven-web.example.invalid\r\n{login}Origin: https://coven-web.example.invalid\r\nSec-Fetch-Site: same-origin\r\nContent-Type: application/json\r\nX-Coven-CSRF: {token}\r\nContent-Length: {}\r\n\r\n", bytes.len()).unwrap();
    if send_body {
        stream.write_all(&bytes).unwrap();
    }
    let mut response = String::new();
    stream.take(65537).read_to_string(&mut response).unwrap();
    assert!(response.len() <= 65536);
    response
}

fn read(gateway: &Gateway, id: &str) -> Value {
    let response = gateway.request(&format!("/api/v1/conversations/{id}/messages"), AUTH);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    body(&response)
}

#[test]
#[ignore = "actual Bun producer: set COVEN_DJINN_SHARING_FIXTURE and short disk-backed TMPDIR; run shared_producer --ignored"]
fn selected_native_session_read_followup_rotation_and_lost_ack() {
    let mut producer = Producer::start("read_write");
    let gateway = producer.gateway();
    listing(&gateway, 0); // Readiness does not select or authorize any session.
    producer.control("select", false, 0);
    let list = listing(&gateway, 1);
    producer.sanitized(&list.to_string());
    let row = &list["conversations"][0];
    assert_eq!(row["association_status"], "unavailable");
    assert!(row["card"].is_null());
    assert_eq!(row["can_submit"], true);
    let id = row["id"].as_str().unwrap();
    let initial = read(&gateway, id);
    producer.sanitized(&initial.to_string());
    assert_eq!(initial["source"], "djinn.conversation-read.v1");
    assert_eq!(
        initial["messages"][0]["text"],
        "Synthetic shared conversation"
    );
    assert_eq!(
        initial["projection_sha256"],
        format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&json!([[
                    "msg_fixture",
                    "assistant",
                    "Synthetic shared conversation",
                    false
                ]]))
                .unwrap()
            )
        )
    );
    let token = body(&gateway.request("/api/v1/csrf", AUTH))["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let hidden = gateway.request("/api/v1/conversations", OTHER);
    assert_eq!(body(&hidden)["conversations"], json!([]));
    assert!(
        gateway
            .request(&format!("/api/v1/conversations/{id}/messages"), OTHER)
            .starts_with("HTTP/1.1 404")
    );
    let other_token = body(&gateway.request("/api/v1/csrf", OTHER))["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        post(&gateway, id, OTHER, &other_token, REQUEST, TEXT, false).starts_with("HTTP/1.1 403")
    );
    producer.control("configured", true, 0);
    let accepted = post(&gateway, id, AUTH, &token, REQUEST, TEXT, true);
    assert!(accepted.starts_with("HTTP/1.1 200"), "{accepted}");
    assert_eq!(body(&accepted)["outcome"], "accepted");
    assert_eq!(body(&accepted)["prompt_bytes"], TEXT.len());
    assert_eq!(
        body(&accepted)["prompt_sha256"],
        format!("{:x}", Sha256::digest(TEXT.as_bytes()))
    );
    producer.sanitized(&accepted);
    assert!(!accepted.contains("literal follow-up"));
    producer.admitted(1);
    assert_eq!(read(&gateway, id)["messages"][1]["text"], TEXT);
    let repeat = post(&gateway, id, AUTH, &token, REQUEST, TEXT, true);
    assert_eq!(body(&repeat)["delivery"], "uncertain");
    producer.control("select", false, 1); // Barrier also catches any extra admitted event.

    producer.control("deselect", false, 1);
    listing(&gateway, 0);
    assert!(
        gateway
            .request(&format!("/api/v1/conversations/{id}/messages"), AUTH)
            .starts_with("HTTP/1.1 404")
    );
    assert!(
        post(&gateway, id, AUTH, &token, LOST_REQUEST, TEXT, false).starts_with("HTTP/1.1 403")
    );
    producer.control("select", false, 1);
    let fresh = listing(&gateway, 1);
    let fresh_id = fresh["conversations"][0]["id"].as_str().unwrap();
    assert_ne!(id, fresh_id);
    assert!(
        gateway
            .request(&format!("/api/v1/conversations/{id}/messages"), AUTH)
            .starts_with("HTTP/1.1 404")
    );
    let replay = post(&gateway, fresh_id, AUTH, &token, REQUEST, TEXT, true);
    assert_eq!(body(&replay)["delivery"], "uncertain");
    producer.control("lost_ack", true, 1);
    let lost_text = "Synthetic prompt whose acknowledgement is lost";
    let lost = post(
        &gateway,
        fresh_id,
        AUTH,
        &token,
        LOST_REQUEST,
        lost_text,
        true,
    );
    assert!(lost.starts_with("HTTP/1.1 200"), "{lost}");
    assert_eq!(body(&lost)["delivery"], "uncertain");
    producer.sanitized(&lost);
    producer.admitted(2);
    assert_eq!(read(&gateway, fresh_id)["messages"][2]["text"], lost_text);
    let repeated_lost = post(
        &gateway,
        fresh_id,
        AUTH,
        &token,
        LOST_REQUEST,
        lost_text,
        true,
    );
    assert_eq!(body(&repeated_lost)["delivery"], "uncertain");
    producer.control("lost_ack", false, 2);
    assert_eq!(
        read(&gateway, fresh_id)["messages"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    producer.control("configured", true, 2); // No retry during receipt handling or subsequent reads.
    drop(gateway);
    producer.control("stop", false, 2);
    producer.wait_stopped();
}

#[test]
#[ignore = "actual Bun producer: set COVEN_DJINN_SHARING_FIXTURE and short disk-backed TMPDIR; run shared_producer --ignored"]
fn selected_read_only_native_session_and_wrong_login_cannot_write() {
    let mut producer = Producer::start("read");
    let gateway = producer.gateway();
    producer.control("select", false, 0);
    let list = listing(&gateway, 1);
    assert_eq!(list["conversations"][0]["can_submit"], false);
    assert_eq!(
        list["conversations"][0]["association_status"],
        "unavailable"
    );
    assert!(list["conversations"][0]["card"].is_null());
    let id = list["conversations"][0]["id"].as_str().unwrap();
    assert_eq!(
        read(&gateway, id)["messages"][0]["text"],
        "Synthetic shared conversation"
    );
    let hidden = gateway.request("/api/v1/conversations", OTHER);
    assert!(hidden.starts_with("HTTP/1.1 200"));
    assert_eq!(body(&hidden)["conversations"], json!([]));
    assert!(
        gateway
            .request(&format!("/api/v1/conversations/{id}/messages"), OTHER)
            .starts_with("HTTP/1.1 404")
    );
    for login in [AUTH, OTHER] {
        let token = body(&gateway.request("/api/v1/csrf", login))["token"]
            .as_str()
            .unwrap()
            .to_owned();
        let started = Instant::now();
        let refused = post(&gateway, id, login, &token, REQUEST, TEXT, false);
        assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "preflight waited for a body"
        );
    }
    producer.control("configured", true, 0);
    assert_eq!(read(&gateway, id)["messages"].as_array().unwrap().len(), 1);
    drop(gateway);
    producer.control("stop", false, 0);
    producer.wait_stopped();
}
