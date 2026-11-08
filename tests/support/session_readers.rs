//! Independent native private-socket peer, with no Python or native app children.
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::Duration;

pub fn snapshot(count: usize) -> Value {
    json!({"version":"fixture","protocol":18,"agents":{"SECRET":"ignored"},"workspaces":false,"tabs":null,
        "panes":(0..count).map(|i|json!({"pane_id":format!("p{i}"),"terminal_id":format!("term{i}"),"tab_id":"tab","workspace_id":"workspace",
            "agent_status":{"SECRET":true},"agent_session":"SECRET","revision":-1,"title":"SECRET"})).collect::<Vec<_>>()})
}
pub fn processes(pane: &Value) -> Value {
    json!({"type":"pane_process_info","process_info":{"pane_id":pane,"foreground_processes":[
        {"pid":22,"name":"SECRET","argv":["/private/djinn-ui","SECRET"]},
        {"pid":3,"name":"SECRET","argv":["/private/djinn","SECRET"]}]}})
}
pub fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("state")).unwrap();
    fs::write(root.path().join("coven.json"), b"not parsed by readers").unwrap();
    for name in [
        "tasks.json",
        "agents.json",
        "execution.json",
        "actions.json",
    ] {
        fs::write(
            root.path().join("state").join(name),
            b"deliberately corrupt unrelated authority",
        )
        .unwrap();
    }
    root
}
pub struct Peer {
    pub path: String,
    pub requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    _root: tempfile::TempDir,
}
impl Peer {
    pub fn new(mut handler: impl FnMut(&Value, usize) -> String + Send + 'static) -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.path().join("s");
        let listener = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (done, log) = (stop.clone(), requests.clone());
        let thread = std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut line = String::new();
                        if BufReader::new(stream.try_clone().unwrap())
                            .read_line(&mut line)
                            .unwrap_or(0)
                            == 0
                        {
                            continue;
                        }
                        let request: Value = serde_json::from_str(&line).unwrap();
                        assert!(matches!(
                            request["method"].as_str(),
                            Some("session.snapshot" | "pane.process_info")
                        ));
                        let index = {
                            let mut log = log.lock().unwrap();
                            log.push(request.clone());
                            log.len() - 1
                        };
                        let reply = handler(&request, index);
                        let _ = stream.write_all(reply.as_bytes());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1))
                    }
                    Err(e) => panic!("accept: {e}"),
                }
            }
        });
        Self {
            path: path.to_str().unwrap().into(),
            requests,
            stop,
            thread: Some(thread),
            _root: root,
        }
    }
    pub fn normal(count: usize) -> Self {
        Self::new(move |request, _| {
            reply(
                request,
                if request["method"] == "session.snapshot" {
                    json!({"type":"session_snapshot","snapshot":snapshot(count)})
                } else {
                    processes(&request["params"]["pane_id"])
                },
            )
        })
    }
    pub fn endpoint(&self) -> Value {
        let info = fs::metadata(&self.path).unwrap();
        json!({"path":self.path,"device":info.dev(),"inode":info.ino()})
    }
    pub fn calls(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}
pub fn reply(request: &Value, result: Value) -> String {
    format!("{}\n", json!({"id":request["id"],"result":result}))
}
pub fn request(workspace: &Path, peer: &Peer) -> coven::session_inspection::Request {
    coven::session_inspection::Request {
        workspace: PathBuf::from(workspace),
        socket: peer.path.clone(),
        pane: None,
        expected_terminal: None,
        runtime: "djinn".into(),
        timeout: 3.,
        max_response_bytes: 1048576,
        group: None,
        member: None,
        recognized_only: false,
    }
}
pub fn authority(endpoint: Value) -> Value {
    json!({"schema_version":2,"workspace_id":"00000000-0000-4000-8000-000000000001","content_revision":1,"publication_counters":[],
        "groups":[{"group_id":"00000000-0000-4000-8000-000000000002","key":"team","name":"Review team","members":[{
            "participant_id":"00000000-0000-4000-8000-000000000003","key":"reviewer","role":"Reviewer",
            "binding":{"presentation":{"adapter":"kitsune","endpoint":endpoint,"terminal_id":"term0","pane_id_at_binding":"old-pane"},
                "evidence":{"provenance":"kitsune.pane.process_info","observed_at":"2026-10-26T00:00:00+00:00","agent_kind_hint":"djinn",
                    "native_reference":null,"native_binding":"unestablished","process_evidence":{"interpreter":"coven.kitsune.djinn-process.v1",
                        "matches":[{"pid":3,"executable_family":"djinn","rule":"exact_posix_argv0_basename"},{"pid":22,"executable_family":"djinn-ui","rule":"exact_posix_argv0_basename"}]}}}}]}]})
}
