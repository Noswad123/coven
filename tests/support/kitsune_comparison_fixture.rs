//! Independent wire fixture, not an installed-server interoperability claim.
//! Producer provenance: Kitsune 01d7cef6, protocol 18, inspected source SHA-256:
//! src/app/api/session.rs c18241400a1faeb692267c328e8b89f3f0e34aa1a4d59e0b45e471ba23f8c2e6
//! src/api/schema/session.rs 0a1a5c695fa5dbcadde0dd06cdecd4f80aee7b9436296a558feb1b3e0e7e07b2
//! src/api/schema/panes.rs 97b1dc288a34a4b4cbe5e9638cd73aac77667acd0715b192ff8a01c31691caab
//! src/api/schema/agents.rs 0fd6f16a5c787079959765be89b3041336a4f31c8d96f70e90f555e5c51a41b1
//! src/api/schema/workspaces.rs 6987f0ee9186b0167eb7c7d96de03a4cf9a407e9602e674983b03dc68a39ad7f
//! src/api/schema/tabs.rs 0b6bf6d2afe6419c835082e39fac3470b832a12a7ba22e5723d3702ac4e5d810
//! No Kitsune application, provider, shell, or Python subprocess is started.
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const SECRET: &str = "PRIVATE_COMPARISON_FIXTURE_SECRET";

pub fn snapshot(count: usize) -> Value {
    let panes: Vec<Value> = (0..count)
        .map(|i| {
            json!({"pane_id":format!("pane-{i}"),"terminal_id":format!("term-{i}"),
                "workspace_id":"ws-0","tab_id":"tab-0","focused":i==0,
                "cwd":format!("/{SECRET}/cwd"),"foreground_cwd":format!("/{SECRET}/foreground"),
                "label":SECRET,"agent":"djinn","title":SECRET,"terminal_title":SECRET,
                "terminal_title_stripped":SECRET,"display_agent":SECRET,
                "agent_status":"done","state_labels":{"done":SECRET},"tokens":{"private":SECRET},
                "agent_session":{"source":"hook","agent":"djinn","kind":"path",
                    "value":format!("/{SECRET}/native-session-{i}")},
                "scroll":{"offset_from_bottom":0,"max_offset_from_bottom":0,"viewport_rows":24},
                "revision":1})
        })
        .collect();
    let agents: Vec<Value> = panes
        .iter()
        .map(|p| {
            let mut a = p.clone();
            a.as_object_mut().unwrap().remove("label");
            a.as_object_mut().unwrap().remove("scroll");
            a["name"] = json!(SECRET);
            a["launch_pending"] = json!(false);
            a["interactive_ready"] = json!(false);
            a["screen_detection_skipped"] = json!(false);
            a["state_change_seq"] = json!(3);
            a
        })
        .collect();
    let rect = json!({"x":0,"y":0,"width":80,"height":24});
    let layout_panes: Vec<Value> = panes
        .iter()
        .map(|p| {
            json!({
                "pane_id":p["pane_id"],"focused":p["focused"],"rect":rect
            })
        })
        .collect();
    json!({"version":"fixture-01d7cef6","protocol":18,
        "annotations":{"epoch":"fixture-epoch","sources":[],"valid_until_unix_ms":true},
        "command_submission":{"version":1},
        "focused_workspace_id":"ws-0","focused_tab_id":"tab-0",
        "focused_pane_id":if count==0 {Value::Null} else {json!("pane-0")},
        "workspaces":[{"workspace_id":"ws-0","number":1,"label":SECRET,"focused":true,
            "pane_count":count,"tab_count":1,"active_tab_id":"tab-0","agent_status":"done"}],
        "tabs":[{"tab_id":"tab-0","workspace_id":"ws-0","number":1,"label":SECRET,
            "focused":true,"pane_count":count,"agent_status":"done"}],
        "layouts":[{"workspace_id":"ws-0","tab_id":"tab-0","zoomed":false,
            "area":rect,"focused_pane_id":"pane-0","panes":layout_panes,"splits":[]}],
        "panes":panes,"agents":agents})
}

pub fn process(pane: &str, argv: Value) -> Value {
    json!({"type":"pane_process_info","process_info":{"pane_id":pane,
        "shell_pid":100,"foreground_process_group_id":101,"tty":format!("/{SECRET}/tty"),
        "foreground_processes":[{"pid":101,"name":SECRET,"argv0":SECRET,
            "argv":argv,"cmdline":SECRET,"cwd":format!("/{SECRET}/cwd")}]}})
}

#[derive(Clone)]
pub struct Plan {
    pub before: Value,
    pub after: Value,
    pub processes: BTreeMap<String, Value>,
    pub wrong_id_at: Option<usize>,
    pub error_at: Option<usize>,
    pub raw_at: Option<(usize, String)>,
    pub delay: Duration,
    pub hold_at: Option<usize>,
    pub swap_at: Option<usize>,
}

impl Plan {
    pub fn new(count: usize) -> Self {
        let before = snapshot(count);
        Self {
            after: before.clone(),
            before,
            processes: (0..count)
                .map(|i| {
                    let pane = format!("pane-{i}");
                    let info = process(&pane, json!(["/opt/fixture/djinn", "--help", SECRET]));
                    (pane, info)
                })
                .collect(),
            wrong_id_at: None,
            error_at: None,
            raw_at: None,
            delay: Duration::ZERO,
            hold_at: None,
            swap_at: None,
        }
    }
}

pub struct Peer {
    pub root: tempfile::TempDir,
    pub socket: PathBuf,
    stop: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<Value>>>,
    errors: Arc<Mutex<Vec<String>>>,
    worker: Option<JoinHandle<()>>,
}

impl Peer {
    pub fn start(plan: Plan) -> Self {
        let root = tempfile::Builder::new()
            .prefix("coven-compare-")
            .tempdir()
            .unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let socket = root.path().join("s");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let (halt, log, faults, path) =
            (stop.clone(), calls.clone(), errors.clone(), socket.clone());
        let worker = thread::spawn(move || {
            let watchdog = Instant::now() + Duration::from_secs(15);
            let mut snapshots = 0;
            let mut replacement = None;
            while !halt.load(Ordering::Relaxed) && Instant::now() < watchdog {
                let (mut stream, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => {
                        faults.lock().unwrap().push(e.to_string());
                        break;
                    }
                };
                let serve = || -> Result<(), String> {
                    stream
                        .set_read_timeout(Some(Duration::from_millis(500)))
                        .map_err(|e| e.to_string())?;
                    stream
                        .set_write_timeout(Some(Duration::from_millis(500)))
                        .map_err(|e| e.to_string())?;
                    let mut frame = String::new();
                    BufReader::new(stream.try_clone().map_err(|e| e.to_string())?)
                        .take(65537)
                        .read_line(&mut frame)
                        .map_err(|e| e.to_string())?;
                    if frame.len() > 65536 || !frame.ends_with('\n') {
                        return Err("invalid request frame".into());
                    }
                    let request: Value = serde_json::from_str(&frame).map_err(|e| e.to_string())?;
                    let mut calls = log.lock().unwrap();
                    calls.push(request.clone());
                    let n = calls.len();
                    drop(calls);
                    let result = match request["method"].as_str() {
                        Some("session.snapshot") => {
                            if request["params"] != json!({}) {
                                return Err("snapshot params".into());
                            }
                            snapshots += 1;
                            json!({"type":"session_snapshot","snapshot":if snapshots==1 {&plan.before} else {&plan.after}})
                        }
                        Some("pane.process_info") => {
                            let pane = request["params"]["pane_id"]
                                .as_str()
                                .ok_or("missing pane")?;
                            if request["params"] != json!({"pane_id":pane}) {
                                return Err("process params".into());
                            }
                            plan.processes.get(pane).ok_or("unplanned pane")?.clone()
                        }
                        _ => return Err("forbidden method".into()),
                    };
                    if plan.hold_at == Some(n) {
                        while !halt.load(Ordering::Relaxed) && Instant::now() < watchdog {
                            thread::sleep(Duration::from_millis(2));
                        }
                        return Ok(());
                    }
                    let end = Instant::now() + plan.delay;
                    while Instant::now() < end && !halt.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_millis(2));
                    }
                    if plan.swap_at == Some(n) {
                        fs::remove_file(&path).map_err(|e| e.to_string())?;
                        replacement = Some(UnixListener::bind(&path).map_err(|e| e.to_string())?);
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                            .map_err(|e| e.to_string())?;
                    }
                    let id = if plan.wrong_id_at == Some(n) {
                        json!("wrong-correlation")
                    } else {
                        request["id"].clone()
                    };
                    let mut reply = if plan.error_at == Some(n) {
                        json!({"id":id,"error":{"code":"method_not_found","message":SECRET}})
                    } else {
                        json!({"id":id,"result":result})
                    }
                    .to_string();
                    if let Some((at, raw)) = &plan.raw_at {
                        if *at == n {
                            reply = raw.replace("REQUEST_ID", &id.to_string());
                        }
                    }
                    reply.push('\n');
                    // Rejection/timeout may close a connection before all bytes are sent.
                    let _ = stream.write_all(reply.as_bytes());
                    Ok(())
                };
                if let Err(e) = {
                    let mut serve = serve;
                    serve()
                } {
                    faults.lock().unwrap().push(e);
                }
            }
            // A held response must not hide a queued retry or fallback connection.
            if listener.accept().is_ok() {
                faults
                    .lock()
                    .unwrap()
                    .push("unexpected queued connection on shutdown".into());
            }
            drop(replacement);
        });
        Self {
            root,
            socket,
            stop,
            calls,
            errors,
            worker: Some(worker),
        }
    }

    pub fn calls(&self) -> Vec<Value> {
        self.calls.lock().unwrap().clone()
    }

    pub fn finish(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.join().expect("fixture worker panicked");
        }
        let errors = self.errors.lock().unwrap();
        assert!(errors.is_empty(), "fixture errors: {errors:?}");
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
