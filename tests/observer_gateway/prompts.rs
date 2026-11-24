use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

const RUNTIME: &str = "11111111-2222-4333-8444-555555555555";
const GRANT: &str = "21111111-2222-4333-8444-555555555555";
const REQUEST: &str = "31111111-2222-4333-8444-555555555555";
const AUTH: &str = "Tailscale-User-Login: fixture@example.invalid\r\n";
struct Peer {
    root: tempfile::TempDir,
    count: Arc<AtomicUsize>,
    lost: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Peer {
    fn new() -> Self {
        let root = tempfile::Builder::new().prefix("p").tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let registry = root.path().join("r");
        std::fs::create_dir(&registry).unwrap();
        std::fs::set_permissions(&registry, std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = root.path().join("s");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        let descriptor = registry.join(format!("{RUNTIME}.json"));
        std::fs::write(&descriptor, serde_json::to_vec(&json!({"contract":"djinn.web-discovery.v1","version":1,"native_socket":socket,"expected_runtime":RUNTIME})).unwrap()).unwrap();
        std::fs::set_permissions(descriptor, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(
            root.path().join("coven.json"),
            br#"{"name":"Fixture","agents":["alpha"],"multiplexer":"herdr"}"#,
        )
        .unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let lost = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (counted, losing, stopping) = (count.clone(), lost.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut raw = String::new();
                if BufReader::new(&stream).read_line(&mut raw).is_err() || raw.is_empty() {
                    continue;
                }
                let request: Value = serde_json::from_str(&raw).unwrap();
                let result = match request["method"].as_str().unwrap() {
                    "capabilities" => {
                        json!({"type":"capabilities","contract":"djinn.interactive-prompt.v1","version":1,"runtime_id":RUNTIME,"max_prompt_bytes":65536,"web_sharing":{"contract":"djinn.web-sharing.v1","version":1}})
                    }
                    "share_status" => {
                        json!({"type":"share_status","contract":"djinn.web-sharing.v1","grant":{"grant_id":GRANT,"session_id":"ses_private","can_submit":true,"lease_ms":5000}})
                    }
                    "shared_submit" => {
                        counted.fetch_add(1, Ordering::AcqRel);
                        assert_eq!(request["params"]["grant_id"], GRANT);
                        assert_eq!(request["params"]["session_id"], "ses_private");
                        if losing.load(Ordering::Acquire) {
                            continue;
                        }
                        let text = request["params"]["text"].as_str().unwrap();
                        json!({"type":"shared_prompt_accepted","contract":"djinn.web-sharing.v1","grant_id":GRANT,"session_id":"ses_private","message_id":"msg_private",
                            "prompt_bytes":text.len(),"prompt_sha256":format!("{:x}", Sha256::digest(text.as_bytes()))})
                    }
                    _ => panic!("unexpected native operation"),
                };
                writeln!(
                    stream,
                    "{}",
                    json!({"version":1,"id":request["id"],"runtime_id":RUNTIME,"result":result})
                )
                .unwrap();
            }
        });
        Self {
            root,
            count,
            lost,
            stop,
            thread: Some(thread),
        }
    }
    fn gateway(&self) -> Gateway {
        Gateway::start_options(None, Some((&self.root.path().join("r"), self.root.path())))
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}
fn body(response: &str) -> Value {
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}
fn post(gateway: &Gateway, path: &str, headers: &str, bytes: Option<&[u8]>) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", gateway.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: coven-web.example.invalid\r\n{headers}\r\n"
    )
    .unwrap();
    if let Some(bytes) = bytes {
        stream.write_all(bytes).unwrap();
    }
    let mut response = String::new();
    stream.take(65536).read_to_string(&mut response).unwrap();
    response
}
fn headers(token: &str, length: usize) -> String {
    format!(
        "{AUTH}Origin: https://coven-web.example.invalid\r\nSec-Fetch-Site: same-origin\r\nContent-Type: application/json\r\nX-Coven-CSRF: {token}\r\nContent-Length: {length}\r\n"
    )
}

#[test]
fn real_http_csrf_prebody_refusals_strict_json_and_native_lost_receipts() {
    let peer = Peer::new();
    let gateway = peer.gateway();
    assert!(
        gateway
            .request("/api/v1/csrf", "")
            .starts_with("HTTP/1.1 403")
    );
    let csrf = gateway.request("/api/v1/csrf", AUTH);
    assert!(csrf.to_lowercase().contains("cache-control: no-store"));
    let token = body(&csrf)["token"].as_str().unwrap().to_owned();
    let list = body(&gateway.ready_conversations(AUTH));
    let target = &list["conversations"][0];
    assert_eq!(target["can_submit"], true);
    let path = format!(
        "/api/v1/conversations/{}/prompts",
        target["id"].as_str().unwrap()
    );
    let bytes = serde_json::to_vec(&json!({"contract":"coven.conversation-submit.v1","request_id":REQUEST,"execute":true,"text":"<script>exact</script>\t\r\n"})).unwrap();
    let good = headers(&token, bytes.len());
    for bad in [
        good.replace(AUTH, ""),
        good.replace("https://coven-web.example.invalid", "https://evil.invalid"),
        good.replace(&format!("X-Coven-CSRF: {token}\r\n"), ""),
        format!("{good}X-Coven-CSRF: {token}\r\n"),
        format!("{good}Content-Length: {}\r\n", bytes.len()),
        good.replace(
            &format!("Content-Length: {}", bytes.len()),
            &format!("Content-Length: {0}, {0}", bytes.len()),
        ),
        headers(&token, 409601),
        format!("{good}Content-Encoding: gzip\r\n"),
        good.replace("same-origin", "cross-site"),
    ] {
        let start = Instant::now();
        let response = post(&gateway, &path, &bad, None);
        assert!(
            response.starts_with("HTTP/1.1 400") || response.starts_with("HTTP/1.1 403"),
            "{response}"
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    let invalid = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("\"execute\":true", "\"execute\":true,\"execute\":true");
    assert!(
        post(
            &gateway,
            &path,
            &headers(&token, invalid.len()),
            Some(invalid.as_bytes())
        )
        .starts_with("HTTP/1.1 400")
    );
    assert_eq!(peer.count.load(Ordering::Acquire), 0);
    let slow = post(&gateway, &path, &good, None);
    assert!(slow.starts_with("HTTP/1.1 408"), "{slow}");
    // Require usable cached metadata after deliberately waiting on a slow body.
    // This GET does not renew the native lease.
    let _ = gateway.ready_conversations(AUTH);
    let accepted = post(&gateway, &path, &good, Some(&bytes));
    assert!(accepted.starts_with("HTTP/1.1 200"), "{accepted}");
    assert_eq!(body(&accepted)["outcome"], "accepted");
    assert_eq!(peer.count.load(Ordering::Acquire), 1);
    for secret in [
        "ses_private",
        "msg_private",
        RUNTIME,
        "<script>",
        "coven.json",
    ] {
        assert!(!accepted.contains(secret));
    }
    let repeated = post(&gateway, &path, &good, Some(&bytes));
    assert_eq!(body(&repeated)["delivery"], "uncertain");
    assert_eq!(peer.count.load(Ordering::Acquire), 1);
    peer.lost.store(true, Ordering::Release);
    let lost = String::from_utf8(bytes)
        .unwrap()
        .replace(REQUEST, "41111111-2222-4333-8444-555555555555");
    let response = post(
        &gateway,
        &path,
        &headers(&token, lost.len()),
        Some(lost.as_bytes()),
    );
    assert_eq!(body(&response)["delivery"], "uncertain");
    assert_eq!(peer.count.load(Ordering::Acquire), 2);
}

#[test]
fn static_conversation_post_is_read_only_without_polling_body() {
    let gateway = Gateway::start();
    let list = body(&gateway.ready_conversations(AUTH));
    let id = list["conversations"][0]["id"].as_str().unwrap();
    let response = post(
        &gateway,
        &format!("/api/v1/conversations/{id}/prompts"),
        &headers("not-a-token", 128),
        None,
    );
    assert!(response.starts_with("HTTP/1.1 403"));
}
