//! Exercise the actual publisher loop, including its guarded transport and polls.
use super::*;
#[cfg(unix)]
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
};

struct Shutdown {
    worker: Arc<Stop>,
    server: Arc<AtomicBool>,
}
impl Drop for Shutdown {
    fn drop(&mut self) {
        self.worker.0.store(true, Ordering::Release);
        self.server.store(true, Ordering::Release);
    }
}

#[test]
#[cfg(unix)]
fn repeated_unsent_poll_admission_races_end_at_last_successful_publication_ceiling() {
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.path().canonicalize().unwrap().join("poll-races.sock");
    let listener = UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let shared = Mutex::new(
        json!({"contract":"coven.normal-observations.v1","server_epoch":"core",
        "authorization":"none","omitted_observations":0,"retired_native_observations":0,
        "_publisher_names":[],"_publisher_current":[],"observations":[{
            "observation_id":"b".repeat(64),"provider":"codex","origin":"v1_receipt",
            "capability":"reference_only","reported_state":"unknown","freshness":"historical","age_ms":null}]}),
    );
    let naming = AtomicU64::new(0);
    let stop = Arc::new(Stop(AtomicBool::new(false)));
    let server_stop = Arc::new(AtomicBool::new(false));
    let denied = AtomicU64::new(0);
    let trace = Mutex::new(Vec::<Value>::new());
    let (send, receive) = mpsc::sync_channel(1);
    thread::scope(|scope| {
        let _shutdown = Shutdown {
            worker: stop.clone(),
            server: server_stop.clone(),
        };
        let server = scope.spawn(|| {
            let mut mutation_at = None;
            while !server_stop.load(Ordering::Acquire) {
                if mutation_at.is_some_and(|at| Instant::now() >= at) {
                    // Simulate a real mutation and its coherent publication,
                    // not an observer read. This occurs in each 250ms pre-poll
                    // pause, after the next unchanged candidate was prepared.
                    naming.fetch_add(2, Ordering::AcqRel);
                    mutation_at = None;
                }
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1)); continue;
                    },
                    Err(e) => panic!("{e}"),
                };
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut line = String::new();
                if BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap() == 0 {
                    denied.fetch_add(1, Ordering::AcqRel);
                    mutation_at = Some(Instant::now() + Duration::from_millis(100));
                    continue;
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                trace.lock().unwrap().push(request.clone());
                let result = match request["method"].as_str().unwrap() {
                    "session.snapshot" => json!({"type":"session_snapshot","snapshot":{"version":"fixture",
                        "protocol":18,"integration_views":{"version":1,"supported_versions":[1,2,3],
                        "sources":[{"id":"agents","epoch":null}]}}}),
                    "integration.view.open" => json!({"type":"view_opened","lease":"lease","epoch":"view","revision":0,"ttl_ms":5000}),
                    "integration.view.replace" => {
                        assert_eq!(request["params"]["revision"], 1);
                        assert_eq!(request["params"]["lease"], "lease");
                        assert_eq!(request["params"]["items"].as_array().unwrap().len(), 1);
                        mutation_at = Some(Instant::now() + Duration::from_millis(100));
                        json!({"type":"view_replaced","epoch":"view","revision":1})
                    },
                    "integration.view.close" => json!({"type":"view_closed"}),
                    other => panic!("a raced poll must remain unsent, not renew or reopen: {other}"),
                };
                writeln!(stream, "{}", json!({"id":request["id"],"result":result})).unwrap();
            }
        });
        let started = Instant::now();
        let worker = scope.spawn(|| {
            send.send(worker_report(
                &PresentationRequest {
                    socket: path.to_str().unwrap().into(),
                    source: "agents".into(),
                },
                &shared,
                &stop,
                Some(&naming),
            ))
            .unwrap();
        });
        let report = receive
            .recv_timeout(Duration::from_secs(6))
            .expect("unsent poll races ignored the publication ceiling");
        assert!(started.elapsed() < Duration::from_secs(6));
        assert_eq!(report["outcome"], "failed", "{report}");
        assert_eq!(report["cleanup"], "closed", "{report}");
        assert!(
            denied.load(Ordering::Acquire) >= 10,
            "fixture must exercise repeated actual pre-poll denials"
        );
        let calls = trace.lock().unwrap();
        for method in [
            "integration.view.open",
            "integration.view.replace",
            "integration.view.close",
        ] {
            assert_eq!(
                calls
                    .iter()
                    .filter(|request| request["method"] == method)
                    .count(),
                1,
                "{method}"
            );
        }
        assert!(
            !calls
                .iter()
                .any(|request| request["method"] == "integration.view.poll")
        );
        worker.join().unwrap();
        server_stop.store(true, Ordering::Release);
        server.join().unwrap();
    });
}

#[test]
#[cfg(unix)]
fn coherent_candidates_preserve_filtered_selected_rows_across_receiver_races() {
    for version in [1, 2, 3] {
        for mode in [
            "odd",
            "contention",
            "naming_race",
            "deadline",
            "frozen",
            "uncertain",
        ] {
            let root = tempfile::tempdir().unwrap();
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
            let path = root.path().canonicalize().unwrap().join("continuity.sock");
            let listener = UnixListener::bind(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            listener.set_nonblocking(true).unwrap();
            let endpoint = socket::select(path.to_str().unwrap()).unwrap();
            let id = format!("observation:{}", "a".repeat(64));
            let other_id = format!("observation:{}", "b".repeat(64));
            let until = naming_tick(
                Instant::now() + Duration::from_secs(if mode == "frozen" { 5 } else { 20 }),
            );
            let shared = Arc::new(Mutex::new(
                json!({"contract":"coven.normal-observations.v1",
                "server_epoch":"core","authorization":"none","omitted_observations":0,"retired_native_observations":0,
                "_publisher_current":[{"id":id,"until":naming_tick(Instant::now() + Duration::from_secs(30))}],
                "_publisher_names":[{"id":id,"revision":"association","until":until,"target":{
                    "socket":endpoint.resolved,"device":endpoint.device,"inode":endpoint.inode,
                    "terminal_id":"terminal","runtime_generation":"generation"}}],
                "observations":[{"observation_id":"a".repeat(64),"provider":"djinn","origin":"djinn_native_event",
                    "reported_state":"working","freshness":"fresh","capability":"activity","age_ms":0,
                    "fresh_pending_count":0,"unacknowledged_pending_count":0},
                    {"observation_id":"b".repeat(64),"provider":"codex","origin":"v1_receipt",
                    "reported_state":"unknown","freshness":"historical","capability":"reference_only","age_ms":null}]}),
            ));
            let naming = Arc::new(AtomicU64::new(0));
            let stop = Arc::new(Stop(AtomicBool::new(false)));
            let server_stop = Arc::new(AtomicBool::new(false));
            let trace = Arc::new(Mutex::new(Vec::<Value>::new()));
            let arm_race = Arc::new(AtomicBool::new(false));
            let retiring = Arc::new(AtomicBool::new(false));
            let (selected_send, selected_receive) = mpsc::sync_channel(1);
            let (permit_send, permit_receive) = mpsc::sync_channel(1);
            let (race_send, race_receive) = mpsc::sync_channel(1);
            let (result_send, result_receive) = mpsc::sync_channel(1);
            let (removed_send, removed_receive) = mpsc::sync_channel(1);
            let (release_removed_send, release_removed_receive) = mpsc::sync_channel(1);
            let (unsent_send, unsent_receive) = mpsc::sync_channel(1);
            let closing = Arc::new(AtomicBool::new(false));
            thread::scope(|scope| {
                let _shutdown = Shutdown {
                    worker: stop.clone(),
                    server: server_stop.clone(),
                };
                let server = {
                    let trace = trace.clone();
                    let naming = naming.clone();
                    let arm_race = arm_race.clone();
                    let retiring = retiring.clone();
                    let server_stop = server_stop.clone();
                    let id = id.clone();
                    let closing = closing.clone();
                    scope.spawn(move || {
                    let mut revision = 0;
                    let mut selected = false;
                    let mut announced = false;
                    let mut removed = false;
                    while !server_stop.load(Ordering::Acquire) {
                        let mut stream = match listener.accept() {
                            Ok((stream, _)) => stream,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                thread::sleep(Duration::from_millis(1));
                                continue;
                            }
                            Err(e) => panic!("{e}"),
                        };
                        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                        let mut line = String::new();
                        if BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap() == 0 {
                            if closing.swap(false, Ordering::AcqRel) { unsent_send.send(()).unwrap(); }
                            continue; // Guarded, unsent preparation is not a request.
                        }
                        let request: Value = serde_json::from_str(&line).unwrap();
                        trace.lock().unwrap().push(request.clone());
                        let params = &request["params"];
                        let result = match request["method"].as_str().unwrap() {
                            "session.snapshot" => {
                                let mut snapshot = json!({"type":"session_snapshot","snapshot":{
                                    "version":"fixture","protocol":18,"integration_views":{"version":1,
                                        "sources":[{"id":"agents","epoch":null}]},
                                    "terminal_navigation":{"version":1},
                                    "panes":[{"terminal_id":"terminal","pane_id":"pane","tab_id":"tab",
                                        "workspace_id":"workspace","label":"SampleA"}],
                                    "tabs":[{"tab_id":"tab","workspace_id":"workspace"}],
                                    "workspaces":[{"workspace_id":"workspace","label":"Coven","git_branch":"feature/ui"}]}});
                                if version >= 2 { snapshot["snapshot"]["integration_views"]["supported_versions"] = json!([1, version]); }
                                snapshot
                            }
                            "terminal.inspect" => {
                                if arm_race.swap(false, Ordering::AcqRel) {
                                    naming.store(1, Ordering::Release);
                                    race_send.send(()).unwrap();
                                }
                                json!({"type":"terminal_inspected","availability":"live","terminal_id":"terminal",
                                    "runtime_generation":"generation","pane_id":"pane","tab_id":"tab","workspace_id":"workspace"})
                            }
                            "integration.view.open" => {
                                assert_eq!(params["version"], version);
                                json!({"type":"view_opened","lease":"lease","epoch":"view","revision":0,"ttl_ms":5000})
                            }
                            "integration.view.replace" => {
                                assert_eq!(params["lease"], "lease");
                                assert_eq!(params["epoch"], "view");
                                assert_eq!(params["revision"], revision + 1);
                                let rows = params["items"].as_array().unwrap();
                                if rows.len() == 1 && !removed {
                                    removed = true;
                                    removed_send.send(()).unwrap();
                                    release_removed_receive.recv_timeout(Duration::from_secs(2)).unwrap();
                                }
                                assert!(!rows.is_empty(), "fabricated absence: {mode}/{version}");
                                if !retiring.load(Ordering::Acquire) {
                                    assert_eq!(rows.len(), 2);
                                    let row = rows.iter().find(|row| row["id"] == id).unwrap();
                                    assert_eq!(row["label"], "SampleA", "filtered selection would be cleared: {mode}/{version}");
                                    if announced { assert!(row["detail"].as_str().unwrap().contains("selected (read-only)"), "selection was lost during deferral: {mode}/{version}"); }
                                    if version == 3 { assert_eq!(row["information"]["context"], json!(["Kit project: Coven", "Kit branch: feature/ui"])); }
                                }
                                if !announced && rows.iter().any(|row| row["id"] == id && row["detail"].as_str().unwrap().contains("selected (read-only)")) {
                                    announced = true;
                                    selected_send.send(()).unwrap();
                                    permit_receive.recv_timeout(Duration::from_secs(2)).unwrap();
                                    if mode == "uncertain" { continue; } // Attempted write, lost ACK.
                                }
                                revision += 1;
                                json!({"type":"view_replaced","epoch":"view","revision":revision})
                            }
                            "integration.view.poll" => {
                                let intents = if !selected {
                                    selected = true;
                                    json!([{"epoch":"view","revision":revision,"item_id":id}])
                                } else { json!([]) };
                                json!({"type":"view_intents","epoch":"view","revision":revision,"ttl_ms":5000,"intents":intents})
                            }
                            "integration.view.close" => json!({"type":"view_closed"}),
                            other => panic!("unexpected method: {other}"),
                        };
                        writeln!(stream, "{}", json!({"id":request["id"],"result":result})).unwrap();
                    }
                    })
                };
                let worker = scope.spawn(|| {
                    let report = worker_report(
                        &PresentationRequest {
                            socket: path.to_str().unwrap().into(),
                            source: "agents".into(),
                        },
                        &shared,
                        &stop,
                        Some(&naming),
                    );
                    result_send.send(report).unwrap();
                });
                selected_receive
                    .recv_timeout(Duration::from_secs(3))
                    .expect("selected named row was not published");
                let mut held = if mode == "contention" {
                    Some(shared.lock().unwrap())
                } else {
                    None
                };
                if matches!(mode, "odd" | "deadline" | "frozen") {
                    naming.store(1, Ordering::Release);
                }
                if mode == "naming_race" {
                    arm_race.store(true, Ordering::Release);
                }
                permit_send.send(()).unwrap();
                if mode == "naming_race" {
                    race_receive.recv_timeout(Duration::from_secs(2)).unwrap();
                }
                if matches!(mode, "frozen" | "uncertain") {
                    let report = result_receive.recv_timeout(Duration::from_secs(6)).unwrap();
                    assert_eq!(
                        report["outcome"],
                        if mode == "uncertain" {
                            "uncertain"
                        } else {
                            "failed"
                        },
                        "mode={mode}; version={version}; report={report}"
                    );
                    assert_eq!(
                        report["cleanup"],
                        if mode == "uncertain" {
                            "ttl_only_uncertain"
                        } else {
                            "closed"
                        },
                        "mode={mode}; version={version}; report={report}"
                    );
                    let calls = trace.lock().unwrap();
                    assert_eq!(
                        calls
                            .iter()
                            .filter(|r| r["method"] == "integration.view.replace")
                            .count(),
                        2
                    );
                    assert_eq!(
                        calls
                            .iter()
                            .filter(|r| r["method"] == "integration.view.open")
                            .count(),
                        1
                    );
                    assert_eq!(
                        calls
                            .iter()
                            .filter(|r| r["method"] == "integration.view.poll")
                            .count(),
                        1
                    );
                    assert_eq!(
                        calls
                            .iter()
                            .filter(|r| r["method"] == "integration.view.close")
                            .count(),
                        usize::from(mode != "uncertain")
                    );
                } else {
                    let before = trace
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|r| {
                            r["method"]
                                .as_str()
                                .unwrap()
                                .starts_with("integration.view.")
                        })
                        .count();
                    thread::sleep(Duration::from_millis(400));
                    let after = trace
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|r| {
                            r["method"]
                                .as_str()
                                .unwrap()
                                .starts_with("integration.view.")
                        })
                        .count();
                    assert_eq!(
                        before, after,
                        "a deferred candidate replaced or renewed the view: {mode}/{version}"
                    );
                    if let Some(mut projection) = held.take() {
                        projection["observations"][0]["fresh_pending_count"] = json!(1);
                        projection["observations"][0]["unacknowledged_pending_count"] = json!(1);
                    } else {
                        let mut projection = shared.lock().unwrap();
                        projection["observations"][0]["fresh_pending_count"] = json!(1);
                        projection["observations"][0]["unacknowledged_pending_count"] = json!(1);
                        naming.store(2, Ordering::Release);
                    }
                    let end = Instant::now() + Duration::from_secs(3);
                    loop {
                        if trace.lock().unwrap().iter().any(|r| {
                            r["method"] == "integration.view.replace"
                                && r["params"]["items"].as_array().unwrap().iter().any(|row| {
                                    row["id"] == id
                                        && row["detail"].as_str().unwrap().contains("pending 1")
                                })
                        }) {
                            break;
                        }
                        assert!(
                            Instant::now() < end,
                            "coherent receiver did not resume: {mode}/{version}"
                        );
                        thread::sleep(Duration::from_millis(5));
                    }
                    retiring.store(true, Ordering::Release);
                    {
                        let mut projection = shared.lock().unwrap();
                        naming.store(3, Ordering::Release);
                        if mode == "deadline" {
                            projection["_publisher_current"][0]["until"] =
                                json!(naming_tick(Instant::now() + Duration::from_millis(600)));
                        } else {
                            projection["observations"].as_array_mut().unwrap().remove(0);
                            projection["_publisher_current"] = json!([]);
                            projection["_publisher_names"] = json!([]);
                        }
                        naming.store(4, Ordering::Release);
                    }
                    removed_receive
                        .recv_timeout(Duration::from_secs(3))
                        .unwrap_or_else(|error| {
                            panic!(
                                "real removal/deadline was not published: {mode}/{version}: {error}"
                            )
                        });
                    assert!(trace.lock().unwrap().iter().any(|r| {
                        r["method"] == "integration.view.replace"
                            && r["params"]["items"]
                                .as_array()
                                .is_some_and(|rows| rows.len() == 1 && rows[0]["id"] == other_id)
                    }));
                    // Let the removal ACK complete without cancellation, then
                    // observe an explicitly unsent poll while holding projection
                    // admission. STOP is now known to be between requests.
                    let held = shared.lock().unwrap();
                    closing.store(true, Ordering::Release);
                    release_removed_send.send(()).unwrap();
                    unsent_receive
                        .recv_timeout(Duration::from_secs(2))
                        .unwrap_or_else(|error| {
                            panic!("missing unsent stop barrier: {mode}/{version}: {error}")
                        });
                    stop.0.store(true, Ordering::Release);
                    drop(held);
                    let report = result_receive.recv_timeout(Duration::from_secs(2)).unwrap();
                    assert_eq!(
                        report["cleanup"], "closed",
                        "mode={mode}; version={version}; report={report}"
                    );
                }
                worker.join().unwrap();
                server_stop.store(true, Ordering::Release);
                server.join().unwrap();
            });
        }
    }
}
