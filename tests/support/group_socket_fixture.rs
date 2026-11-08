//! Inert, disposable public-API peer. Compiled directly with pinned rustc, std only.
//! No discovery, runtime, provider, shell, or child process functionality.
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// Requests come from the tested JSON client. Preserve the raw JSON string token
// for correlation, including escapes, rather than inventing an ID or coercing it.
fn string_token<'a>(request: &'a str, key: &str) -> &'a str {
    let key = format!("\"{key}\"");
    let start = request.find(&key).expect("request field") + key.len();
    let rest = request[start..].trim_start();
    let rest = rest.strip_prefix(':').expect("field colon").trim_start();
    assert!(rest.starts_with('"'), "string request field");
    let mut escaped = false;
    for (at, byte) in rest.bytes().enumerate().skip(1) {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return &rest[..=at];
        }
    }
    panic!("unterminated request field")
}

fn config(root: &Path, name: &str) -> String {
    fs::read_to_string(root.join(name)).unwrap_or_default()
}

fn serve(mut stream: UnixStream, root: PathBuf, log: Arc<Mutex<fs::File>>) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .expect("write timeout");
    let mut request = String::new();
    BufReader::new(stream.try_clone().expect("read clone"))
        .read_line(&mut request)
        .expect("request line");
    assert!(request.len() <= 65536 && request.ends_with('\n'));
    let id = string_token(&request, "id");
    let method = string_token(&request, "method");
    assert!(
        matches!(
            method,
            "\"session.snapshot\"" | "\"pane.process_info\"" | "\"pane.annotate\""
        ),
        "forbidden method: {method}"
    );
    log.lock()
        .expect("log lock")
        .write_all(request.as_bytes())
        .expect("request log");
    let name = match method {
        "\"session.snapshot\"" => "snapshot",
        "\"pane.process_info\"" => "process",
        _ => "annotate",
    };
    let gate = root.join(format!("hold-{name}"));
    let end = Instant::now() + Duration::from_secs(8);
    while gate.exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(5));
    }
    let mode = config(&root, &format!("{name}-mode"));
    if mode == "lost" {
        return;
    }
    if mode == "eof-open" {
        stream.shutdown(Shutdown::Write).expect("EOF without close");
        std::thread::sleep(Duration::from_millis(1400));
        return;
    }
    let result = match name {
        "snapshot" => format!(
            "{{\"type\":\"session_snapshot\",\"snapshot\":{}}}",
            config(&root, "snapshot.json")
        ),
        "process" => config(&root, "process.json"),
        _ => {
            let configured = config(&root, "annotate.json");
            if configured.is_empty() {
                "{\"type\":\"ok\"}".to_owned()
            } else {
                configured
            }
        }
    };
    let reply = if mode == "error" {
        format!("{{\"id\":{id},\"error\":{{\"code\":\"annotation_stale_revision\",\"message\":\"PRIVATE_FIXTURE_ERROR\"}}}}\n")
    } else {
        let id = if mode == "wrong-id" {
            "\"not-the-request\""
        } else {
            id
        };
        format!(
            "{{\"id\":{id},\"result\":{result}}}\n{}",
            if mode == "trailing" { "x" } else { "" }
        )
    };
    // A bounded client may reject an oversized reply before the last byte.
    let _ = stream.write_all(reply.as_bytes());
    if mode == "no-eof" {
        // A complete JSON line is not a complete one-request exchange until EOF.
        std::thread::sleep(Duration::from_millis(1400));
    }
}

fn main() {
    let home = PathBuf::from(std::env::var_os("HOME").expect("private HOME"));
    let root = home.join("peer");
    assert_eq!(
        fs::metadata(&root)
            .expect("private configuration")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let socket = root.join("s");
    let listener = UnixListener::bind(&socket).expect("selected private AF_UNIX socket");
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).expect("socket mode");
    let log = Arc::new(Mutex::new(
        OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(root.join("calls.jsonl"))
            .expect("fresh log"),
    ));
    fs::write(root.join("ready"), b"ready").expect("ready marker");
    for stream in listener.incoming() {
        let stream = stream.expect("private connection");
        let root = root.clone();
        let log = log.clone();
        std::thread::spawn(move || serve(stream, root, log));
    }
}
