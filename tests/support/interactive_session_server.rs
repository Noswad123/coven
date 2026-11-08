//! Real private Unix API fixture. No interpreter or native runtime is launched.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::{fs::PermissionsExt, net::UnixListener};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub fn inventory() -> Value {
    json!({"version":"fixture","protocol":18,"command_submission":{"version":1},
        "agents":"PRIVATE legacy schema is not consumed",
        "panes":[{"pane_id":"w1:p1","terminal_id":"terminal-1","workspace_id":"workspace-1",
            "tab_id":"tab-1","agent_status":{"ignored":true}}]})
}

pub fn snapshot(request: &Value, value: Value) -> Vec<u8> {
    frame(json!({"id":request["id"],"result":{"type":"session_snapshot","snapshot":value}}))
}

pub fn ack(request: &Value) -> Value {
    json!({"id":request["id"],"result":{"type":"pane_command_submitted",
        "pane_id":request["params"]["pane_id"],"terminal_id":request["params"]["expected_terminal_id"],
        "argv":request["params"]["argv"]}})
}

pub fn frame(value: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    bytes
}

pub struct Server {
    pub path: PathBuf,
    requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    _directory: tempfile::TempDir,
}

impl Server {
    pub fn new(mut reply: impl FnMut(usize, &Value) -> Vec<u8> + Send + 'static) -> Self {
        // Short paths fit sockaddr_un even when the checkout path is long.
        let directory = tempfile::Builder::new()
            .prefix("coven-is-")
            .tempdir()
            .unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("api.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let transcript = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("accept fixture: {error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut line = String::new();
                if BufReader::new(&mut stream).read_line(&mut line).is_err() || line.is_empty() {
                    continue;
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                let index = {
                    let mut requests = transcript.lock().unwrap();
                    requests.push(request.clone());
                    requests.len()
                };
                let bytes = reply(index, &request);
                // The client may intentionally close on a response limit or interrupt.
                let _ = stream.write_all(&bytes);
            }
        });
        Self {
            path,
            requests,
            stop,
            thread: Some(thread),
            _directory: directory,
        }
    }

    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

pub fn executable(root: &Path) -> PathBuf {
    let path = root.join("trusted djinn");
    // If erroneously invoked this cannot silently pretend to be a real runtime.
    std::fs::write(&path, b"not an executable image; metadata fixture only\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
