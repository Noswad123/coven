use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::{fs::PermissionsExt, net::UnixListener};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;

pub const RUNTIME: &str = "6cd93c2e-7146-4dca-8d2f-9c7afcfe85f0";
pub const REQUEST: &str = "832492cf-947d-4ba4-8fa6-9c43a347a81b";

pub struct Server {
    pub path: PathBuf,
    pub requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    _directory: tempfile::TempDir,
}
impl Server {
    pub fn new(change: impl Fn(&Value, &mut Value) -> Option<Vec<u8>> + Send + 'static) -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("prompt.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let thread = std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("accept: {e}"),
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut line = String::new();
                if BufReader::new(&stream).read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                observed.lock().unwrap().push(request.clone());
                assert_eq!(request["version"], 1);
                let result = match request["method"].as_str().unwrap() {
                    "capabilities" => {
                        json!({"type":"capabilities","contract":"djinn.interactive-prompt.v1",
                        "version":1,"runtime_id":RUNTIME,"max_prompt_bytes":65536})
                    }
                    "submit" => {
                        let raw = request["params"]["text"].as_str().unwrap().as_bytes();
                        json!({"type":"prompt_accepted","contract":"djinn.interactive-prompt.v1",
                            "session_id":request["params"]["session_id"],"message_id":"msg_fixture",
                            "prompt_sha256":format!("{:x}",Sha256::digest(raw)),"prompt_bytes":raw.len()})
                    }
                    method => panic!("unexpected method {method}"),
                };
                let mut response =
                    json!({"version":1,"id":request["id"],"runtime_id":RUNTIME,"result":result});
                if let Some(raw) = change(&request, &mut response) {
                    let _ = stream.write_all(&raw);
                }
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
    pub fn normal() -> Self {
        Self::new(|_, response| Some(frame(response)))
    }
    pub fn methods(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r["method"].as_str().unwrap().to_owned())
            .collect()
    }
}
pub fn frame(response: &Value) -> Vec<u8> {
    let mut raw = serde_json::to_vec(response).unwrap();
    raw.push(b'\n');
    raw
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}
