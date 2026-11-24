//! Real HTTP/CLI boundary tests against synthetic endpoints, never live services.
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

#[cfg(unix)]
#[path = "observer_gateway/djinn_producer.rs"]
mod djinn_producer;

#[cfg(unix)]
#[path = "observer_gateway/decisions_producer.rs"]
mod decisions_producer;

#[cfg(unix)]
#[path = "observer_gateway/prompts.rs"]
mod prompts;

#[cfg(unix)]
#[path = "observer_gateway/shared_producer.rs"]
mod shared_producer;

struct Gateway {
    child: Child,
    port: u16,
    _directory: tempfile::TempDir,
}
impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Gateway {
    fn start() -> Self {
        Self::start_with(None)
    }
    fn start_with(native: Option<(&std::path::Path, &str, &str)>) -> Self {
        Self::start_options(native, None)
    }
    fn start_options(
        native: Option<(&std::path::Path, &str, &str)>,
        source: Option<(&std::path::Path, &std::path::Path)>,
    ) -> Self {
        let directory = tempfile::tempdir().expect("fixture directory");
        let default_socket = directory.path().join("native.sock");
        let (socket, runtime, session) = native.unwrap_or((
            &default_socket,
            "11111111-2222-4333-8444-555555555555",
            "ses_private",
        ));
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserve port");
        let port = listener.local_addr().expect("port").port();
        drop(listener);
        let config = directory.path().join("observer.json");
        let mut value = serde_json::json!({
            "listen":format!("127.0.0.1:{port}"),
            "public_origin":"https://coven-web.example.invalid",
            "allowed_logins":["fixture@example.invalid","other@example.invalid"],
            "conversations":[{"id":"private-registration","label":"Synthetic conversation","team":"research",
                "allowed_logins":["fixture@example.invalid"],"native_socket":socket,
                "expected_runtime":runtime,"session":session}],
            "endpoints":[{"id":"fixture","label":"Synthetic endpoint","socket":directory.path().join("absent.sock")}]
        });
        if let Some((registry, workspace)) = source {
            value["conversations"] = serde_json::json!([]);
            value["conversation_sources"] = serde_json::json!([{"id":"shared","label":"Shared fixture","team":"research",
                "directory":registry,"allowed_logins":["fixture@example.invalid"],"submit_logins":["fixture@example.invalid"],"control_workspace":workspace}]);
        }
        std::fs::write(&config, serde_json::to_vec(&value).expect("encode"))
            .expect("fixture config");
        let check = Command::new(env!("CARGO_BIN_EXE_coven"))
            .args(["observer", "check", "--config"])
            .arg(&config)
            .output()
            .expect("check command");
        assert!(
            check.status.success(),
            "{}",
            String::from_utf8_lossy(&check.stderr)
        );
        assert!(!directory.path().join("absent.sock").exists());
        let child = Command::new(env!("CARGO_BIN_EXE_coven"))
            .args(["observer", "serve", "--config"])
            .arg(&config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("gateway process");
        let mut gateway = Self {
            child,
            port,
            _directory: directory,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            assert!(
                gateway.child.try_wait().expect("process status").is_none(),
                "gateway exited"
            );
            assert!(Instant::now() < deadline, "gateway startup timeout");
            std::thread::sleep(Duration::from_millis(20));
        }
        gateway
    }
    fn request(&self, path: &str, headers: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("read deadline");
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .expect("write deadline");
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: coven-web.example.invalid\r\n{headers}\r\n"
        )
        .expect("request");
        let mut response = String::new();
        stream
            .take(3 * 1024 * 1024)
            .read_to_string(&mut response)
            .expect("bounded response");
        response
    }
    fn ready_conversations(&self, headers: &str) -> String {
        // TCP readiness does not imply that the metadata scheduler has published.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(Instant::now() < deadline, "conversation readiness deadline");
            let response = self.request("/api/v1/conversations", headers);
            assert!(response.starts_with("HTTP/1.1 200"), "{response}");
            let value: serde_json::Value =
                serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(value["contract"], "coven.conversations.v1");
            let rows = value["conversations"]
                .as_array()
                .expect("conversation rows");
            if value["state"] != "unavailable" && !rows.is_empty() {
                return response;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "conversation readiness deadline: {response}"
            );
            std::thread::sleep(remaining.min(Duration::from_millis(20)));
        }
    }
}

#[test]
fn conversation_resource_acl_is_checked_on_list_and_read() {
    let gateway = Gateway::start();
    let auth = "Tailscale-User-Login: fixture@example.invalid\r\n";
    let other = "Tailscale-User-Login: other@example.invalid\r\n";
    let list = gateway.ready_conversations(auth);
    assert!(list.starts_with("HTTP/1.1 200"));
    let value: serde_json::Value =
        serde_json::from_str(list.split_once("\r\n\r\n").unwrap().1).unwrap();
    let id = value["conversations"][0]["id"].as_str().unwrap();
    assert_eq!(id.len(), 64);
    for secret in [
        "private-registration",
        "ses_private",
        "native.sock",
        "11111111-",
    ] {
        assert!(!list.contains(secret));
    }
    let hidden = gateway.request("/api/v1/conversations", other);
    assert!(hidden.contains("\"conversations\":[]"));
    let route = format!("/api/v1/conversations/{id}/messages");
    assert!(gateway.request(&route, "").starts_with("HTTP/1.1 403"));
    assert!(gateway.request(&route, other).starts_with("HTTP/1.1 404"));
    assert!(gateway.request(&route, auth).starts_with("HTTP/1.1 503"));
    assert!(
        gateway
            .request("/api/v1/conversations?socket=/arbitrary", auth)
            .starts_with("HTTP/1.1 404")
    );
}

#[test]
fn foreground_events_are_authenticated_exact_bodyless_bounded_and_release_on_disconnect() {
    let gateway = Gateway::start();
    let auth = "Tailscale-User-Login: fixture@example.invalid\r\n";
    for (path, headers, code) in [
        ("/api/v1/events", "", 403),
        ("/api/v1/events?x=1", auth, 404),
        ("/api/v1/events/", auth, 404),
    ] {
        assert!(
            gateway
                .request(path, headers)
                .starts_with(&format!("HTTP/1.1 {code}"))
        );
    }
    for framing in [
        "Content-Length: 1\r\n",
        "Transfer-Encoding: chunked\r\n",
        "Content-Length: 0\r\nContent-Length: 0\r\n",
    ] {
        let response = gateway.request("/api/v1/events", &format!("{auth}{framing}"));
        assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    }
    let open = || {
        let mut stream = TcpStream::connect(("127.0.0.1", gateway.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(
            stream,
            "GET /api/v1/events HTTP/1.1\r\nHost: coven-web.example.invalid\r\n{auth}\r\n"
        )
        .unwrap();
        let mut text = String::new();
        let mut bytes = [0; 1024];
        while !text.contains("event: ready\ndata: {\"contract\":\"coven.events.v1\"}\n\n") {
            let n = stream.read(&mut bytes).unwrap();
            assert!(n > 0);
            text.push_str(std::str::from_utf8(&bytes[..n]).unwrap());
        }
        assert!(text.starts_with("HTTP/1.1 200"));
        assert!(text.contains("content-type: text/event-stream"));
        assert!(!text.contains("id:") && !text.contains("native.sock"));
        stream
    };
    let first = open();
    let second = open();
    assert!(
        gateway
            .request("/api/v1/events", auth)
            .starts_with("HTTP/1.1 429")
    );
    drop(first);
    // Disconnect processing is asynchronous, but must promptly release admission.
    std::thread::sleep(Duration::from_millis(50));
    let replacement = open();
    assert!(
        gateway
            .request("/api/v1/fleet", auth)
            .starts_with("HTTP/1.1 200")
    );
    drop(second);
    drop(replacement);
}

#[cfg(unix)]
#[test]
fn real_http_native_conversation_is_read_only_and_not_reused_across_principals() {
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::{
        io::{BufRead, BufReader},
        os::unix::{fs::PermissionsExt, net::UnixListener},
    };
    let gateway = Gateway::start();
    std::fs::set_permissions(
        gateway._directory.path(),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let path = gateway._directory.path().join("native.sock");
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let auth = "Tailscale-User-Login: fixture@example.invalid\r\n";
    let list = gateway.ready_conversations(auth);
    let list: Value = serde_json::from_str(list.split_once("\r\n\r\n").unwrap().1).unwrap();
    let route = format!(
        "/api/v1/conversations/{}/messages",
        list["conversations"][0]["id"].as_str().unwrap()
    );
    // A guessed authorized reference cannot even connect under another principal.
    assert!(
        gateway
            .request(&route, "Tailscale-User-Login: other@example.invalid\r\n")
            .starts_with("HTTP/1.1 404")
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    let server = std::thread::spawn(move || {
        for method in ["capabilities", "conversation"] {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut socket = loop {
                if let Ok((socket, _)) = listener.accept() {
                    break socket;
                }
                assert!(Instant::now() < deadline, "native read did not arrive");
                std::thread::sleep(Duration::from_millis(5));
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&socket).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], method);
            let runtime = "11111111-2222-4333-8444-555555555555";
            let contract = "djinn.conversation-read.v1";
            let text = "<script>not HTML</script>\u{001b}[31m";
            let result = if method == "capabilities" {
                assert_eq!(request["params"], json!({}));
                json!({"type":"capabilities","runtime_id":runtime,"contract":"djinn.interactive-prompt.v1",
                    "version":1,"max_prompt_bytes":65536,"conversation_read":{"contract":contract,"max_messages":20,"max_text_bytes":8192,"max_response_bytes":65536}})
            } else {
                assert_eq!(
                    request["params"],
                    json!({"expected_runtime_id":runtime,"session_id":"ses_private"})
                );
                let digest = format!(
                    "{:x}",
                    Sha256::digest(
                        serde_json::to_vec(&json!([["msg_private", "assistant", text, false]]))
                            .unwrap()
                    )
                );
                json!({"type":"conversation","contract":contract,"session_id":"ses_private",
                    "messages":[{"id":"msg_private","role":"assistant","text":text,"truncated":false}],
                    "has_older":true,"truncated":false,"omitted_parts":true,"projection_sha256":digest})
            };
            writeln!(
                socket,
                "{}",
                json!({"version":1,"id":request["id"],"runtime_id":runtime,"result":result})
            )
            .unwrap();
        }
    });
    let response = gateway.request(&route, auth);
    server.join().unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("not HTML"));
    assert!(!response.contains("ses_private") && !response.contains("msg_private"));
    assert!(
        response
            .to_ascii_lowercase()
            .contains("cache-control: no-store")
    );
    assert!(
        gateway
            .request(&route, "Tailscale-User-Login: other@example.invalid\r\n")
            .starts_with("HTTP/1.1 404")
    );
}

#[test]
fn http_auth_assets_fleet_and_origin_boundary() {
    let gateway = Gateway::start();
    let denied = gateway.request("/api/v1/fleet", "");
    assert!(denied.starts_with("HTTP/1.1 403"));
    assert!(denied.contains("access_denied"));
    let auth = "Tailscale-User-Login: fixture@example.invalid\r\nSec-Fetch-Site: same-origin\r\n";
    let html = gateway.request("/", auth);
    assert!(html.starts_with("HTTP/1.1 200"));
    assert!(html.contains("<h1>Coven Web</h1>"));
    assert!(html.contains("<title>Coven Web"));
    assert!(
        html.to_ascii_lowercase()
            .contains("cache-control: no-store")
    );
    assert!(html.contains("frame-ancestors 'none'"));
    let script = gateway.request("/app.js", auth);
    assert!(script.starts_with("HTTP/1.1 200"));
    assert!(script.contains("text/javascript"));
    let worker = gateway.request("/service-worker.js", auth);
    assert!(worker.starts_with("HTTP/1.1 200"));
    assert!(worker.contains("text/javascript"));
    assert!(worker.contains("worker-src 'self'"));
    assert!(
        worker
            .to_ascii_lowercase()
            .contains("cache-control: no-store")
    );
    assert!(worker.contains("Coven needs your attention"));
    assert!(!worker.contains("addEventListener('fetch'"));
    let push = gateway.request("/api/v1/push/config", auth);
    assert!(push.starts_with("HTTP/1.1 200"));
    let push: serde_json::Value =
        serde_json::from_str(push.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(
        push,
        serde_json::json!({"contract":"coven.web-push.v1","enabled":false,"public_key":null,"subscriptions":[]})
    );
    let attention = gateway.request("/api/v1/attention", auth);
    assert!(attention.starts_with("HTTP/1.1 200"));
    let attention: serde_json::Value =
        serde_json::from_str(attention.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(attention["contract"], "coven.attention.v1");
    let fleet = gateway.request("/api/v1/fleet", auth);
    assert!(fleet.starts_with("HTTP/1.1 200"));
    let body = fleet.split_once("\r\n\r\n").expect("HTTP body").1;
    let value: serde_json::Value = serde_json::from_str(body).expect("fleet JSON");
    assert_eq!(value["contract"], "coven.fleet-observer.v1");
    assert_eq!(value["endpoints"][0]["connection"], "unavailable");
    assert_eq!(value["endpoints"][0]["agents"], serde_json::json!([]));
    let hostile = gateway.request(
        "/api/v1/fleet",
        &format!("{auth}Origin: https://attacker.invalid\r\n"),
    );
    assert!(hostile.starts_with("HTTP/1.1 403"));
    assert!(
        !gateway._directory.path().join("absent.sock").exists(),
        "observer must not create missing upstreams"
    );
}

#[test]
fn massive_unprovided_bodies_are_rejected_without_draining_or_starving_fleet() {
    let mut gateway = Gateway::start();
    let started = Instant::now();
    let mut streams = Vec::new();
    // More than the old four workers, without sending or half-closing any body.
    for index in 0..8 {
        let mut stream = TcpStream::connect(("127.0.0.1", gateway.port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("deadline");
        let auth = if index % 2 == 0 {
            "Tailscale-User-Login: fixture@example.invalid\r\n"
        } else {
            ""
        };
        write!(stream, "GET /api/v1/fleet HTTP/1.1\r\nHost: coven-web.example.invalid\r\n{auth}Content-Length: 999999999999999999\r\n\r\n").expect("headers only");
        streams.push(stream);
    }
    for stream in streams {
        let mut response = String::new();
        stream
            .take(8192)
            .read_to_string(&mut response)
            .expect("prompt response and EOF without body");
        assert!(
            response.starts_with("HTTP/1.1 400") || response.starts_with("HTTP/1.1 403"),
            "{response}"
        );
        assert!(response.to_ascii_lowercase().contains("connection: close"));
        assert!(
            response
                .to_ascii_lowercase()
                .contains("cache-control: no-store")
        );
    }
    assert!(started.elapsed() < Duration::from_secs(3));
    let fleet = gateway.request(
        "/api/v1/fleet",
        "Tailscale-User-Login: fixture@example.invalid\r\n",
    );
    assert!(fleet.starts_with("HTTP/1.1 200"));
    assert!(gateway.child.try_wait().expect("process status").is_none());
}

#[test]
fn incomplete_headers_expire_without_blocking_cached_fleet() {
    let gateway = Gateway::start();
    let started = Instant::now();
    let mut streams = Vec::new();
    for _ in 0..8 {
        let mut stream = TcpStream::connect(("127.0.0.1", gateway.port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(7)))
            .expect("deadline");
        stream
            .write_all(b"GET /api/v1/fleet HTTP/1.1\r\nHost:")
            .expect("partial headers");
        streams.push(stream);
    }
    let fleet = gateway.request(
        "/api/v1/fleet",
        "Tailscale-User-Login: fixture@example.invalid\r\n",
    );
    assert!(fleet.starts_with("HTTP/1.1 200"));
    for mut stream in streams {
        let mut byte = [0];
        // Closing without consuming incomplete peer headers can produce a TCP
        // reset rather than an orderly EOF. Both terminate the bounded request.
        match stream.read(&mut byte) {
            Ok(0) => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            result => panic!("header deadline must close socket: {result:?}"),
        }
    }
    assert!(started.elapsed() < Duration::from_secs(7));
}

#[test]
fn cli_refuses_unknown_probe_fields_and_non_loopback_listeners() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let config = directory.path().join("bad.json");
    std::fs::write(&config, r#"{"listen":"0.0.0.0:7341","public_origin":"https://example.invalid","allowed_logins":["owner"],"endpoints":[{"id":"a","label":"A","socket":"/tmp/absent.sock"}]}"#).expect("fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_coven"))
        .args(["observer", "check", "--config"])
        .arg(&config)
        .output()
        .expect("check");
    assert!(!output.status.success());
    let mut child = Command::new(env!("CARGO_BIN_EXE_coven"))
        .args([
            "observer",
            "probe",
            "--socket",
            "/tmp/absent-coven-web-fixture.sock",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("probe");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(br#"{"operation":"inventory","method":"pane.send_keys"}"#)
        .expect("input");
    let result = child.wait_with_output().expect("probe exit");
    assert!(
        result.status.success(),
        "helper failures use a closed JSON envelope"
    );
    let envelope: serde_json::Value =
        serde_json::from_slice(&result.stdout).expect("helper envelope");
    assert_eq!(envelope["error"]["code"], "invalid_probe_request");
}
