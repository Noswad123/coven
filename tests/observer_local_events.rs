//! Real Rust HTTP gateway, private synthetic Kit only. No native providers or UI.
#![cfg(unix)]

use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    process::{Child, Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const REGULAR: Duration = Duration::from_secs(3);

struct State {
    title: String,
    label: String,
    subscription: Option<UnixStream>,
    subscriptions: usize,
    snapshots: usize,
    processes: usize,
    completed: Vec<Instant>,
    hold: bool,
    held: bool,
    stop: bool,
    failure: Option<String>,
}

struct Fixture {
    child: Option<Child>,
    worker: Option<JoinHandle<()>>,
    shared: Arc<(Mutex<State>, Condvar)>,
    port: u16,
    _root: tempfile::TempDir,
}

fn pane(title: &str, label: &str) -> Value {
    json!({"terminal_id":"terminal","pane_id":"pane","workspace_id":"workspace",
        "tab_id":"tab","title":title,"label":label})
}

impl Fixture {
    fn start() -> Self {
        let tmp = std::fs::canonicalize(
            std::env::var_os("TMPDIR").expect("set TMPDIR to a short disk-backed directory"),
        )
        .unwrap();
        assert!(!tmp.starts_with("/tmp") && !tmp.starts_with("/dev/shm"));
        let root = tempfile::Builder::new()
            .prefix("le")
            .rand_bytes(2)
            .tempdir_in(tmp)
            .unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = root.path().join("k");
        assert!(
            socket.as_os_str().len() < 104,
            "Unix socket path too long: {socket:?}"
        );
        let listener = UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = tcp.local_addr().unwrap().port();
        drop(tcp);
        let config = root.path().join("observer.json");
        std::fs::write(
            &config,
            serde_json::to_vec(&json!({
                "listen":format!("127.0.0.1:{port}"),
                "public_origin":"https://coven-web.example.invalid",
                "allowed_logins":["fixture@example.invalid"],
                "endpoints":[{"id":"kit","label":"Synthetic Kit","socket":socket}]
            }))
            .unwrap(),
        )
        .unwrap();
        let shared = Arc::new((
            Mutex::new(State {
                title: "Initial snapshot title".into(),
                label: "Initial label".into(),
                subscription: None,
                subscriptions: 0,
                snapshots: 0,
                processes: 0,
                completed: vec![],
                hold: false,
                held: false,
                stop: false,
                failure: None,
            }),
            Condvar::new(),
        ));
        // Install teardown ownership before starting either worker or process.
        let mut fixture = Self {
            child: None,
            worker: None,
            shared: shared.clone(),
            port,
            _root: root,
        };
        fixture.worker = Some(thread::spawn(move || {
            let serve = || -> Result<(), String> {
                loop {
                    if shared.0.lock().unwrap().stop {
                        return Ok(());
                    }
                    let (mut stream, _) = match listener.accept() {
                        Ok(pair) => pair,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            let guard = shared.0.lock().unwrap();
                            drop(
                                shared
                                    .1
                                    .wait_timeout(guard, Duration::from_millis(5))
                                    .unwrap(),
                            );
                            continue;
                        }
                        Err(e) => return Err(e.to_string()),
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut line = String::new();
                    BufReader::new(&stream)
                        .take(8193)
                        .read_line(&mut line)
                        .map_err(|e| e.to_string())?;
                    if line.len() > 8192 {
                        return Err("oversized request".into());
                    }
                    let request: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
                    let mut state = shared.0.lock().unwrap();
                    let result = match request["method"].as_str().unwrap_or("") {
                        "events.subscribe" => {
                            let selectors: Vec<_> = [
                                "pane.created",
                                "pane.closed",
                                "pane.updated",
                                "pane.moved",
                                "pane.exited",
                                "tab.created",
                                "tab.closed",
                                "tab.moved",
                                "workspace.created",
                                "workspace.closed",
                                "workspace.moved",
                                "layout.updated",
                            ]
                            .into_iter()
                            .map(|kind| json!({"type":kind}))
                            .collect();
                            if request["params"] != json!({"subscriptions":selectors}) {
                                return Err(format!("unexpected subscription: {request}"));
                            }
                            if state.subscriptions != 0 {
                                return Err("unexpected reconnect".into());
                            }
                            state.subscriptions += 1;
                            json!({"type":"subscription_started"})
                        }
                        "session.snapshot" => {
                            if request["params"] != json!({}) {
                                return Err(format!("snapshot params: {request}"));
                            }
                            state.snapshots += 1;
                            if state.hold {
                                state.held = true;
                                shared.1.notify_all();
                                let deadline = Instant::now() + Duration::from_secs(2);
                                while state.hold && !state.stop {
                                    let remaining =
                                        deadline.saturating_duration_since(Instant::now());
                                    if remaining.is_zero() {
                                        return Err("held snapshot release deadline".into());
                                    }
                                    state = shared.1.wait_timeout(state, remaining).unwrap().0;
                                }
                            }
                            if state.stop {
                                return Ok(());
                            }
                            let p = pane(&state.title, &state.label);
                            // Same descriptor/DTO shape as observer's discovery_snapshot helper.
                            json!({"type":"session_snapshot","snapshot":{
                                "protocol":18,"version":"fixture-v1","terminal_navigation":{"version":1},
                                "process_observation":{"version":1,"platform":"linux",
                                    "scope":"pane_foreground_cohort","selection":"unclassified"},
                                "panes":[p],"agents":[{"terminal_id":"terminal","pane_id":"pane",
                                    "workspace_id":"workspace","tab_id":"tab","agent":"djinn","agent_status":"idle"}]}})
                        }
                        "pane.process_info" => {
                            if request["params"] != json!({"pane_id":"pane"}) {
                                return Err(format!("process params: {request}"));
                            }
                            state.processes += 1;
                            json!({"type":"pane_process_info","process_info":{
                                "pane_id":"pane","foreground_processes":[]}})
                        }
                        _ => return Err(format!("forbidden Kit RPC: {request}")),
                    };
                    writeln!(stream, "{}", json!({"id":request["id"],"result":result}))
                        .map_err(|e| e.to_string())?;
                    if request["method"] == "events.subscribe" {
                        // Publish the sender only after the ACK has been written.
                        state.subscription = Some(stream.try_clone().unwrap());
                    } else if request["method"] == "session.snapshot" && state.snapshots % 2 == 0 {
                        let expected = state.completed.len() + 1;
                        if state.processes != expected {
                            return Err("missing inventory process bracket".into());
                        }
                        state.completed.push(Instant::now());
                    }
                    shared.1.notify_all();
                    // Normal requests close at EOF; only the subscription clone stays open.
                }
            };
            if let Err(error) = serve() {
                shared.0.lock().unwrap().failure = Some(error);
                shared.1.notify_all();
            }
        }));
        fixture.child = Some(
            Command::new(env!("CARGO_BIN_EXE_coven"))
                .args(["observer", "serve", "--config"])
                .arg(config)
                .env("HOME", fixture._root.path())
                .env("XDG_CONFIG_HOME", fixture._root.path())
                .env("XDG_CACHE_HOME", fixture._root.path())
                .env("XDG_DATA_HOME", fixture._root.path())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            assert!(
                fixture
                    .child
                    .as_mut()
                    .unwrap()
                    .try_wait()
                    .unwrap()
                    .is_none(),
                "gateway exited"
            );
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            assert!(Instant::now() < deadline, "HTTP startup deadline");
            thread::sleep(Duration::from_millis(10));
        }
        fixture
    }

    fn wait(&self, deadline: Instant, predicate: impl Fn(&State) -> bool) {
        let mut state = self.shared.0.lock().unwrap();
        loop {
            assert!(state.failure.is_none(), "Kit fixture: {:?}", state.failure);
            if predicate(&state) {
                return;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "fixture deadline: snapshots={}, processes={}, inventories={}, held={}",
                state.snapshots,
                state.processes,
                state.completed.len(),
                state.held
            );
            state = self.shared.1.wait_timeout(state, remaining).unwrap().0;
        }
    }

    fn fleet(&self) -> Value {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        write!(stream, "GET /api/v1/fleet HTTP/1.1\r\nHost: coven-web.example.invalid\r\nTailscale-User-Login: fixture@example.invalid\r\nConnection: close\r\n\r\n").unwrap();
        let mut response = String::new();
        stream
            .take(128 * 1024)
            .read_to_string(&mut response)
            .unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let value: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert!(
            !value.to_string().contains("RAW EVENT TITLE"),
            "event payload patched fleet: {value}"
        );
        value
    }

    fn card(&self, title: &str, label: &str, deadline: Instant) -> Value {
        loop {
            let fleet = self.fleet();
            let endpoint = &fleet["endpoints"][0];
            let card = &endpoint["agents"][0];
            if endpoint["connection"] == "reachable"
                && card["title"] == title
                && card["pane_name"] == label
            {
                assert_eq!(endpoint["agents"].as_array().unwrap().len(), 1);
                assert_eq!(endpoint["discovery_status"], "observed");
                return card.clone();
            }
            assert!(
                Instant::now() < deadline,
                "fleet publication deadline: {fleet}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn event(&self, copies: usize) -> Instant {
        let mut state = self.shared.0.lock().unwrap();
        let event = json!({"event":"pane_updated","data":{"type":"pane_updated",
            "pane":pane("RAW EVENT TITLE", "Untrusted event label")}});
        let sent = Instant::now();
        for _ in 0..copies {
            writeln!(
                state
                    .subscription
                    .as_mut()
                    .expect("subscription ACK barrier"),
                "{event}"
            )
            .unwrap();
        }
        sent
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        {
            let mut state = self.shared.0.lock().unwrap_or_else(|p| p.into_inner());
            state.stop = true;
            state.hold = false;
            if let Some(stream) = state.subscription.take() {
                let _ = stream.shutdown(Shutdown::Both);
            }
            self.shared.1.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

#[test]
fn local_events_accelerate_fenced_inventory_not_raw_metadata_and_periodic_still_reconciles() {
    let fixture = Fixture::start();
    // Initial acquisition, ACK-triggered resync, then a regular acquisition:
    // the resync's independent event budget has now expired. No startup hint can pass this test.
    fixture.wait(Instant::now() + Duration::from_secs(8), |s| {
        s.subscription.is_some() && s.completed.len() >= 3
    });
    let initial = fixture.card(
        "Initial snapshot title",
        "Initial label",
        Instant::now() + Duration::from_secs(1),
    );
    let (baseline, last) = {
        let mut state = fixture.shared.0.lock().unwrap();
        let count = state.completed.len();
        let last = *state.completed.last().unwrap();
        assert!(
            last.elapsed() < Duration::from_millis(500),
            "missed regular baseline"
        );
        state.title = "Authoritative snapshot title".into();
        state.hold = true;
        (count, last)
    };
    let sent = fixture.event(1);
    let fast_deadline =
        (sent + Duration::from_secs(2)).min(last + REGULAR - Duration::from_millis(300));
    fixture.wait(fast_deadline, |s| s.held);
    // A hint has reached the poller, but acquisition is deliberately unfinished.
    fixture.card("Initial snapshot title", "Initial label", fast_deadline);
    {
        let mut state = fixture.shared.0.lock().unwrap();
        state.hold = false;
        fixture.shared.1.notify_all();
    }
    fixture.wait(fast_deadline, |s| s.completed.len() == baseline + 1);
    let updated = fixture.card(
        "Authoritative snapshot title",
        "Initial label",
        fast_deadline,
    );
    assert_eq!(initial["id"], updated["id"]);
    assert!(Instant::now() < fast_deadline, "not event-accelerated");

    // No event accompanies this manual-label change. The unchanged three-second
    // reconciliation must pick it up, rather than relying on event coverage.
    let completed = {
        let mut state = fixture.shared.0.lock().unwrap();
        state.label = "Periodic-only label".into();
        *state.completed.last().unwrap()
    };
    fixture.wait(completed + REGULAR + Duration::from_secs(1), |s| {
        s.completed.len() >= baseline + 2
    });
    let periodic = fixture.card(
        "Authoritative snapshot title",
        "Periodic-only label",
        completed + REGULAR + Duration::from_secs(1),
    );
    assert_eq!(initial["id"], periodic["id"]);
    {
        let state = fixture.shared.0.lock().unwrap();
        let at = state.completed[baseline + 1];
        assert!(
            at.duration_since(completed) >= REGULAR - Duration::from_millis(100),
            "unexpected early reconciliation"
        );
        assert!(
            at.elapsed() < Duration::from_millis(500),
            "missed duplicate-test baseline"
        );
    }
    // Event budget is available again. Replayed identical pane history must not
    // dirty inventory; a pending refresh cannot hide behind the event throttle.
    fixture.event(64);
    let quiet_until = Instant::now() + Duration::from_secs(1);
    let state = fixture.shared.0.lock().unwrap();
    let (state, _) = fixture
        .shared
        .1
        .wait_timeout_while(
            state,
            quiet_until.saturating_duration_since(Instant::now()),
            |s| {
                s.failure.is_none()
                    && s.completed.len() == baseline + 2
                    && s.snapshots == (baseline + 2) * 2
            },
        )
        .unwrap();
    assert!(state.failure.is_none(), "Kit fixture: {:?}", state.failure);
    assert_eq!(
        state.completed.len(),
        baseline + 2,
        "duplicate history caused inventory loop"
    );
    assert_eq!(
        state.snapshots,
        (baseline + 2) * 2,
        "duplicate history started acquisition"
    );
    assert_eq!(state.subscriptions, 1, "subscription did not remain open");
}
