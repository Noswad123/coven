//! Real HTTP/CLI boundary tests against synthetic, unavailable endpoints only.
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

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
        let directory = tempfile::tempdir().expect("fixture directory");
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserve port");
        let port = listener.local_addr().expect("port").port();
        drop(listener);
        let config = directory.path().join("observer.json");
        std::fs::write(&config, serde_json::to_vec(&serde_json::json!({
            "listen":format!("127.0.0.1:{port}"),
            "public_origin":"https://coven-web.example.invalid",
            "allowed_logins":["fixture@example.invalid"],
            "endpoints":[{"id":"fixture","label":"Synthetic endpoint","socket":directory.path().join("absent.sock")}]
        })).expect("encode")).expect("fixture config");
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
        assert_eq!(
            stream
                .read(&mut byte)
                .expect("header deadline closes socket"),
            0
        );
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
