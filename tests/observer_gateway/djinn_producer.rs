//! Opt-in cross-language acceptance using Djinn's real native wire producer.
use super::*;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    sync::mpsc,
};

struct Producer {
    child: Child,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Producer {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            reader.join().expect("readiness reader");
        }
    }
}

#[test]
#[ignore = "opt-in: set COVEN_DJINN_CONVERSATION_FIXTURE to Djinn's real fixture script and run with --ignored"]
fn real_djinn_producer_through_authenticated_http() {
    let script = std::env::var_os("COVEN_DJINN_CONVERSATION_FIXTURE")
        .expect("explicit opt-in requires COVEN_DJINN_CONVERSATION_FIXTURE");
    let script = std::path::Path::new(&script);
    assert!(
        script.is_absolute() && script.is_file(),
        "absolute existing fixture script required"
    );
    let directory = tempfile::tempdir().expect("private producer directory");
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let address = directory.path().join("d.sock");
    assert!(
        address.as_os_str().len() <= 100,
        "use a short disk-backed TMPDIR"
    );
    let child = Command::new("bun")
        .arg(script)
        .arg(&address)
        .env(
            "BUN_RUNTIME_TRANSPILER_CACHE_PATH",
            directory.path().join("bun-cache"),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start installed bun with real producer");
    let mut producer = Producer {
        child,
        reader: None,
    };
    let stdout = producer.child.stdout.take().unwrap();
    let (send, receive) = mpsc::channel();
    producer.reader = Some(std::thread::spawn(move || {
        let mut line = String::new();
        let read = BufReader::new(stdout.take(4097)).read_line(&mut line);
        let _ = send.send((read, line));
    }));
    let (ready, line) = receive
        .recv_timeout(Duration::from_secs(10))
        .expect("producer readiness deadline");
    ready.expect("bounded readiness line");
    assert!(line.len() <= 4096);
    let ready: Value = serde_json::from_str(&line).expect("readiness JSON");
    assert_eq!(ready["type"], "ready");
    assert_eq!(ready["contract"], "djinn.conversation-read.v1");
    assert_eq!(ready["session_id"], "ses_fixture");

    let mut socket = UnixStream::connect(&address).expect("private producer socket");
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    writeln!(socket, "{}", json!({"version":1,"id":"11111111-2222-4333-8444-555555555555","method":"capabilities","params":{}})).unwrap();
    let mut bytes = Vec::new();
    socket.take(65537).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 65536 && bytes.last() == Some(&b'\n'));
    let capability: Value = serde_json::from_slice(&bytes).unwrap();
    let runtime = capability["runtime_id"]
        .as_str()
        .expect("runtime from real capabilities");
    assert_eq!(
        capability["result"]["conversation_read"]["contract"],
        "djinn.conversation-read.v1"
    );

    let gateway = Gateway::start_with(Some((&address, runtime, "ses_fixture")));
    let auth = "Tailscale-User-Login: fixture@example.invalid\r\nOrigin: https://coven-web.example.invalid\r\nSec-Fetch-Site: same-origin\r\n";
    let listing = gateway.request("/api/v1/conversations", auth);
    assert!(listing.starts_with("HTTP/1.1 200"));
    let listing: Value = serde_json::from_str(listing.split_once("\r\n\r\n").unwrap().1).unwrap();
    let id = listing["conversations"][0]["id"].as_str().unwrap();
    let route = format!("/api/v1/conversations/{id}/messages");
    assert!(
        gateway
            .request(&route, "Tailscale-User-Login: other@example.invalid\r\n")
            .starts_with("HTTP/1.1 404")
    );
    let response = gateway.request(&route, auth);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "native HTTP read failed"
    );
    let value: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(value["source"], "djinn.conversation-read.v1");
    assert_eq!(
        value["projection_sha256"],
        "4b55cfc034c79e9c1f6bbc31c7443fe11a9e18f14777f16f988d520a3a9ddbb8"
    );
    assert_eq!(
        value["messages"][0]["text"],
        "\0\u{0008}\t\n\u{000c}\r\u{001f}\"\\/\u{00e9}\u{2028}\u{2029}\u{1f600}"
    );
    assert!(value["messages"][0].get("id").is_none());
    drop(gateway);

    // EOF is the producer's documented graceful shutdown, not a signal/restart
    // against any real service. Wait for all children before fixture teardown.
    drop(producer.child.stdin.take());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = producer.child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "producer EOF shutdown deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!address.exists(), "producer must clean its endpoint on EOF");
}
