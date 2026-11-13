use super::*;

fn request() -> WatchRequest {
    WatchRequest {
        discovery: Request {
            socket: "/missing/coven-discovery.sock".into(),
            pane: None,
            expected_terminal: None,
            timeout: 1.0,
            max_response_bytes: 1024 * 1024,
        },
        duration: 0.1,
        interval: 0.1,
        source: None,
    }
}

#[test]
fn operands_are_validated_before_io() {
    let mut r = request();
    for duration in [0.0, 0.099, 3600.1, f64::NAN, f64::INFINITY] {
        r.duration = duration;
        assert!(validate(&r).is_err());
    }
    r.duration = 0.1;
    for interval in [0.0, 0.099, 60.1, f64::NAN, f64::INFINITY] {
        r.interval = interval;
        assert!(validate(&r).is_err());
    }
    r.interval = 0.1;
    r.source = Some("coven-discovery".into());
    assert!(validate(&r).is_ok());
    r.discovery.timeout = 3.1;
    assert!(validate(&r).is_err());
    r.discovery.timeout = 1.0;
    r.interval = 2.1;
    assert!(validate(&r).is_err());
    r.interval = 0.1;
    for source in ["", "bad source", "bad\nsource", "\u{e9}", &"a".repeat(65)] {
        r.source = Some(source.into());
        assert!(validate(&r).is_err());
    }
}

#[test]
fn unavailable_is_streamed_and_completion_is_successful() {
    let mut bytes = Vec::new();
    let output = run(
        &request(),
        true,
        &socket::NoCancel,
        &mut bytes,
        &mut |_, _| panic!("no endpoint authority"),
    )
    .unwrap();
    let sample: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(sample["assessment"], "unavailable");
    assert_eq!(sample["targets"], json!([]));
    let summary: Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(summary["samples"], 1);
    assert_eq!(summary["unavailable"], 1);
    assert_eq!(output.exit_code, 0);
    assert!(!output.stdout.ends_with('\n'));
}

#[test]
fn output_failure_stops_without_retry() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let out = run(
        &request(),
        true,
        &socket::NoCancel,
        &mut Broken,
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    assert_eq!(out.exit_code, 1);
    assert!(out.stdout.contains("output_failure"));
}

#[test]
fn cancellation_before_first_attempt_is_130() {
    struct Stopped;
    impl Cancel for Stopped {
        fn interrupted(&self) -> bool {
            true
        }
    }
    let mut bytes = Vec::new();
    let out = run(
        &request(),
        true,
        &Stopped,
        &mut bytes,
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    assert_eq!(out.exit_code, 130);
    assert!(bytes.is_empty());
}

fn positive() -> Value {
    json!({"assessment":"observed","targets":[{"terminal_id":"t", "argv":"PRIVATE", "cwd":"SECRET",
        "recognition":"multiple_provider_families","matches":[
            {"provider":"claude","pid":10,"rule":"exact"},
            {"provider":"codex","pid":11,"rule":"exact"}]}]})
}

fn platform_samples() -> [Value; 2] {
    [
        json!({"version":1,"platform":"linux","scope":"pane_foreground_cohort","selection":"unclassified"}),
        json!({"version":2,"platform":"macos","scope":"pane_foreground_cohort","selection":"unclassified","argument_policy":"darwin_procargs2_lp64_v1"}),
    ].map(|descriptor| {
        let mut sample = positive();
        sample["process_observation"] = descriptor;
        sample
    })
}

#[test]
fn mixed_projection_is_ephemeral_and_never_displays_raw_fields() {
    let (rows, omitted) = presentation::rows(&positive(), "nonce", None);
    assert_eq!(rows.len(), 3);
    assert_eq!(omitted, 0);
    assert!(rows[1]["label"].as_str().unwrap().contains("claude"));
    assert!(rows[2]["label"].as_str().unwrap().contains("codex"));
    let text = serde_json::to_string(&rows).unwrap();
    assert!(!text.contains("PRIVATE"));
    assert!(!text.contains("SECRET"));
    assert!(text.contains("activity/attention unknown"));
    assert_ne!(rows, presentation::rows(&positive(), "another", None).0);
    let selected = presentation::rows(&positive(), "nonce", rows[1]["id"].as_str()).0;
    assert!(
        selected[1]["detail"]
            .as_str()
            .unwrap()
            .contains("Selected (read-only)")
    );
}

#[test]
fn row_limit_discloses_omissions_and_outage_clears_every_positive() {
    let mut sample = positive();
    sample["targets"][0]["matches"] = json!(
        (1..=200)
            .map(|pid| json!({"provider":"claude","pid":pid}))
            .collect::<Vec<_>>()
    );
    let (rows, omitted) = presentation::rows(&sample, "nonce", None);
    assert_eq!(rows.len(), 128);
    assert_eq!(omitted, 73);
    assert!(rows[0]["detail"].as_str().unwrap().contains("73 omitted"));
    assert!(serde_json::to_vec(&rows).unwrap().len() < 96 * 1024);
    sample["assessment"] = json!("unavailable");
    let (rows, _) = presentation::rows(&sample, "nonce", None);
    assert_eq!(rows.len(), 1);
    assert!(rows[0]["label"].as_str().unwrap().contains("unavailable"));
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::{
        fs,
        io::{BufRead, BufReader},
        os::unix::{fs::PermissionsExt, net::UnixListener},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        thread,
    };

    struct Server {
        _root: tempfile::TempDir,
        path: String,
        calls: Arc<Mutex<Vec<Value>>>,
        stop: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
    }
    impl Server {
        fn new(fail: &'static str) -> Self {
            Self::with_cancel(fail, None)
        }

        fn with_cancel(fail: &'static str, cancel: Option<Arc<AtomicBool>>) -> Self {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("kit.sock");
            let listener = UnixListener::bind(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            listener.set_nonblocking(true).unwrap();
            let calls = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let recorded = calls.clone();
            let stopped = stop.clone();
            let worker = thread::spawn(move || {
                let mut revision = 0;
                let mut items = json!([]);
                let mut pending = Vec::<Value>::new();
                let mut polls = 0;
                while !stopped.load(Ordering::Relaxed) {
                    let (mut stream, _) = match listener.accept() {
                        Ok(v) => v,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(1));
                            continue;
                        }
                        Err(e) => panic!("{e}"),
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut line = String::new();
                    BufReader::new(stream.try_clone().unwrap())
                        .read_line(&mut line)
                        .unwrap();
                    let request: Value = serde_json::from_str(&line).unwrap();
                    recorded.lock().unwrap().push(request.clone());
                    let method = request["method"].as_str().unwrap();
                    if method == "integration.view.poll" {
                        polls += 1;
                    }
                    if fail == "cancel_pre_poll" && polls == 2 && method == "integration.view.poll"
                    {
                        cancel.as_ref().unwrap().store(true, Ordering::Release);
                        let _ = stream.write_all(b"{\"id\":");
                        continue;
                    }
                    if method == fail {
                        continue;
                    } // Unknown delivery, never a safe retry.
                    let result = match method {
                        "session.snapshot" => json!({"type":"session_snapshot","snapshot":{
                            "version":"fixture","protocol":18,"integration_views":{"version":1,"sources":[
                                {"id":"build","epoch":"build-epoch"},{"id":"coven-discovery","epoch":null}]}}}),
                        "integration.view.open" => {
                            json!({"type":"view_opened","lease":"lease","epoch":"epoch","revision":0,"ttl_ms":5000})
                        }
                        "integration.view.replace" => {
                            revision = request["params"]["revision"].as_u64().unwrap();
                            items = request["params"]["items"].clone();
                            pending.clear(); // Real Kit replace discards queued intents.
                            json!({"type":"view_replaced","epoch":"epoch","revision":revision})
                        }
                        "integration.view.poll" => {
                            let intents = std::mem::take(&mut pending);
                            json!({"type":"view_intents","epoch":"epoch","revision":revision,"ttl_ms":5000,"intents":intents})
                        }
                        "integration.view.close" => json!({"type":"view_closed"}),
                        _ => panic!("forbidden method {method}"),
                    };
                    let reply = json!({"id":request["id"],"result":result});
                    let _ = writeln!(stream, "{reply}");
                    if method == "integration.view.poll" && !items[1]["id"].is_null() {
                        // A user clicks after this poll, before the next sample.
                        pending = vec![
                            json!({"epoch":"old","revision":revision,"item_id":items[1]["id"]}),
                            json!({"epoch":"epoch","revision":revision - 1,"item_id":items[1]["id"]}),
                            json!({"epoch":"epoch","revision":revision,"item_id":"missing"}),
                            json!({"epoch":"epoch","revision":revision,"item_id":items[1]["id"]}),
                        ];
                    }
                }
            });
            Self {
                _root: root,
                path: path.to_str().unwrap().into(),
                calls,
                stop,
                worker: Some(worker),
            }
        }
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            self.worker.take().unwrap().join().unwrap();
        }
    }

    #[test]
    fn serial_watch_selects_read_only_then_clears_outage() {
        let server = Server::new("");
        let mut r = request();
        r.discovery.socket = server.path.clone();
        r.duration = 0.35;
        r.source = Some("coven-discovery".into());
        let mut count = 0;
        let mut bytes = Vec::new();
        let out = run(&r, true, &socket::NoCancel, &mut bytes, &mut |_, _| {
            count += 1;
            Ok(if count <= 2 {
                positive()
            } else {
                unavailable(count, "fixture_outage")
            })
        })
        .unwrap();
        assert_eq!(out.exit_code, 0);
        let summary: Value = serde_json::from_str(&out.stdout).unwrap();
        assert!(summary["unavailable"].as_u64().unwrap() >= 1);
        assert!(summary["presentation"]["selections"].as_u64().unwrap() >= 1);
        assert!(summary["presentation"]["ignored_intents"].as_u64().unwrap() >= 1);
        let calls = server.calls.lock().unwrap();
        let frames: Vec<_> = calls
            .iter()
            .filter(|r| r["method"] == "integration.view.replace")
            .collect();
        assert_eq!(frames[0]["params"]["items"].as_array().unwrap().len(), 3);
        assert!(
            frames[1]["params"]["items"][1]["detail"]
                .as_str()
                .unwrap()
                .contains("Selected (read-only)")
        );
        assert_eq!(
            frames.last().unwrap()["params"]["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(calls.last().unwrap()["method"], "integration.view.close");
        for call in calls.iter().filter(|c| c["method"] != "session.snapshot") {
            assert_eq!(call["params"]["source"], "coven-discovery");
        }
    }

    #[test]
    fn uncertain_ui_is_never_retried_closed_or_reopened() {
        for method in [
            "integration.view.open",
            "integration.view.replace",
            "integration.view.poll",
        ] {
            let server = Server::new(method);
            let mut r = request();
            r.discovery.socket = server.path.clone();
            r.duration = 0.35;
            r.source = Some("coven-discovery".into());
            let out = run(&r, true, &socket::NoCancel, &mut Vec::new(), &mut |_, _| {
                Ok(positive())
            })
            .unwrap();
            assert_eq!(out.exit_code, 0);
            let summary: Value = serde_json::from_str(&out.stdout).unwrap();
            assert_eq!(summary["presentation"]["outcome"], "uncertain");
            assert!(summary["samples"].as_u64().unwrap() >= 3);
            let calls = server.calls.lock().unwrap();
            assert_eq!(calls.iter().filter(|c| c["method"] == method).count(), 1);
            assert_eq!(calls.last().unwrap()["method"], method);
        }
    }

    #[test]
    fn slow_collection_skips_ticks_and_uses_remaining_budget() {
        let server = Server::new("");
        let mut r = request();
        r.discovery.socket = server.path.clone();
        r.duration = 0.25;
        let mut count = 0;
        let out = run(
            &r,
            true,
            &socket::NoCancel,
            &mut Vec::new(),
            &mut |req, _| {
                assert!(req.timeout <= 0.25);
                count += 1;
                thread::sleep(Duration::from_millis(160));
                Ok(positive())
            },
        )
        .unwrap();
        let summary: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(count, 2);
        assert!(summary["skipped_ticks"].as_u64().unwrap() >= 2);
        assert!(server.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn lease_expiry_never_renews_stale_positives() {
        let server = Server::new("");
        let endpoint = socket::select(&server.path);
        let mut ui = presentation::Presenter::new(Some("coven-discovery"), endpoint);
        // The first delayed collection cannot be published or renewed.
        ui.sample(
            &positive(),
            Instant::now() - Duration::from_secs(5),
            Instant::now() + Duration::from_secs(1),
            &socket::NoCancel,
        );
        ui.finish();
        assert_eq!(ui.report["outcome"], "expired");
        assert!(server.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn queued_click_is_checked_against_fresh_candidates_before_selection() {
        let server = Server::new("");
        let mut ui =
            presentation::Presenter::new(Some("coven-discovery"), socket::select(&server.path));
        let end = Instant::now() + Duration::from_secs(3);
        ui.sample(&positive(), Instant::now(), end, &socket::NoCancel);
        let mut fresh = positive();
        fresh["targets"][0]["matches"][0]["pid"] = json!(99);
        ui.sample(&fresh, Instant::now(), end, &socket::NoCancel);
        assert_eq!(ui.report["selections"], 0);
        assert_eq!(ui.report["ignored_intents"], 4);
        ui.finish();
        let calls = server.calls.lock().unwrap();
        let frames: Vec<_> = calls
            .iter()
            .filter(|r| r["method"] == "integration.view.replace")
            .collect();
        assert!(
            !frames[1]["params"]["items"]
                .to_string()
                .contains("Selected (read-only)")
        );
    }

    #[test]
    fn failure_or_incompatibility_clears_positives_before_polling() {
        for code in ["timeout", "schema_error", "incompatible"] {
            let server = Server::new("");
            let mut ui =
                presentation::Presenter::new(Some("coven-discovery"), socket::select(&server.path));
            let end = Instant::now() + Duration::from_secs(3);
            ui.sample(&positive(), Instant::now(), end, &socket::NoCancel);
            let before = server.calls.lock().unwrap().len();
            let mut failed = unavailable(2, code);
            if code == "incompatible" {
                failed["assessment"] = json!("incompatible");
            }
            ui.sample(&failed, Instant::now(), end, &socket::NoCancel);
            assert_eq!(ui.report["selections"], 0);
            let calls = server.calls.lock().unwrap();
            assert_eq!(calls[before]["method"], "integration.view.replace");
            assert_eq!(
                calls[before]["params"]["items"].as_array().unwrap().len(),
                1
            );
            assert_eq!(calls[before + 1]["method"], "integration.view.poll");
            drop(calls);
            ui.finish();
        }
    }

    #[test]
    fn collection_gap_over_five_seconds_never_polls_renews_or_reopens() {
        let server = Server::new("");
        let mut ui =
            presentation::Presenter::new(Some("coven-discovery"), socket::select(&server.path));
        let end = Instant::now() + Duration::from_secs(10);
        ui.sample(&positive(), Instant::now(), end, &socket::NoCancel);
        let before = server.calls.lock().unwrap().len();
        thread::sleep(Duration::from_millis(5100));
        ui.sample(&positive(), Instant::now(), end, &socket::NoCancel);
        ui.sample(&positive(), Instant::now(), end, &socket::NoCancel);
        ui.finish();
        assert_eq!(ui.report["outcome"], "expired");
        assert_eq!(ui.report["cleanup"], "ttl_only_expired");
        assert_eq!(server.calls.lock().unwrap().len(), before);
    }

    #[test]
    fn aging_displayed_sample_is_not_polled_even_with_a_live_lease() {
        let server = Server::new("");
        let mut ui =
            presentation::Presenter::new(Some("coven-discovery"), socket::select(&server.path));
        let end = Instant::now() + Duration::from_secs(10);
        ui.sample(
            &positive(),
            Instant::now() - Duration::from_secs(2),
            end,
            &socket::NoCancel,
        );
        assert_eq!(ui.report["frames"], 1);
        let before = server.calls.lock().unwrap().len();
        // The displayed observation ages out of the one-second call budget before
        // its later-opened lease does. A fresh collection does not refresh that age.
        thread::sleep(Duration::from_millis(2100));
        ui.sample(&positive(), Instant::now(), end, &socket::NoCancel);
        ui.finish();
        assert_eq!(ui.report["outcome"], "expired");
        assert_eq!(server.calls.lock().unwrap().len(), before);
    }

    #[test]
    fn cancellation_during_pre_poll_response_stops_all_ui_calls() {
        struct Stopped(Arc<AtomicBool>);
        impl Cancel for Stopped {
            fn interrupted(&self) -> bool {
                self.0.load(Ordering::Acquire)
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let server = Server::with_cancel("cancel_pre_poll", Some(stop.clone()));
        let mut ui =
            presentation::Presenter::new(Some("coven-discovery"), socket::select(&server.path));
        let end = Instant::now() + Duration::from_secs(3);
        let cancel = Stopped(stop);
        ui.sample(&positive(), Instant::now(), end, &cancel);
        ui.sample(&positive(), Instant::now(), end, &cancel);
        assert_eq!(ui.report["outcome"], "uncertain");
        assert_eq!(ui.report["cleanup"], "ttl_only_uncertain");
        let before = server.calls.lock().unwrap().len();
        ui.sample(&positive(), Instant::now(), end, &socket::NoCancel);
        ui.finish();
        let calls = server.calls.lock().unwrap();
        assert_eq!(calls.len(), before);
        assert_eq!(calls.last().unwrap()["method"], "integration.view.poll");
        assert_eq!(
            calls
                .iter()
                .filter(|r| r["method"] == "integration.view.replace")
                .count(),
            1
        );
        assert!(
            !calls
                .iter()
                .any(|r| r["method"] == "integration.view.close")
        );
    }

    #[test]
    fn replacement_endpoint_never_acquires_new_authority() {
        let server = Server::new("");
        let mut r = request();
        r.discovery.socket = server.path.clone();
        r.duration = 0.35;
        r.source = Some("coven-discovery".into());
        let mut replacement = None;
        let mut collections = 0;
        let mut bytes = Vec::new();
        let out = run(&r, true, &socket::NoCancel, &mut bytes, &mut |_, _| {
            collections += 1;
            if collections == 2 {
                fs::rename(&server.path, server._root.path().join("old.sock")).unwrap();
                let listener = UnixListener::bind(&server.path).unwrap();
                fs::set_permissions(&server.path, fs::Permissions::from_mode(0o600)).unwrap();
                listener.set_nonblocking(true).unwrap();
                replacement = Some(listener);
            }
            Ok(positive())
        })
        .unwrap();
        assert_eq!(out.exit_code, 0);
        assert_eq!(collections, 2);
        let samples: Vec<Value> = String::from_utf8(bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(samples[0]["assessment"], "observed");
        for sample in &samples[1..] {
            assert_eq!(sample["assessment"], "unavailable");
            assert_eq!(sample["targets"], json!([]));
        }
        assert!(replacement.unwrap().accept().is_err());
        let calls = server.calls.lock().unwrap();
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
                .filter(|r| r["method"] == "integration.view.replace")
                .count(),
            1
        );
    }

    #[test]
    fn both_platforms_preserve_queued_selection_and_clear_it_before_failure_poll() {
        for sample in platform_samples() {
            for code in ["timeout", "schema_error", "incompatible"] {
                let server = Server::new("");
                let mut ui = presentation::Presenter::new(
                    Some("coven-discovery"),
                    socket::select(&server.path),
                );
                let end = Instant::now() + Duration::from_secs(3);
                ui.sample(&sample, Instant::now(), end, &socket::NoCancel);
                ui.sample(&sample, Instant::now(), end, &socket::NoCancel);
                assert_eq!(ui.report["selections"], 1);
                assert_eq!(ui.report["ignored_intents"], 3);
                let before = server.calls.lock().unwrap().len();
                let mut failed = unavailable(3, code);
                if code == "incompatible" {
                    failed["assessment"] = json!("incompatible");
                }
                ui.sample(&failed, Instant::now(), end, &socket::NoCancel);
                assert_eq!(ui.report["selections"], 1);
                let calls = server.calls.lock().unwrap();
                assert_eq!(calls[before]["method"], "integration.view.replace");
                assert_eq!(
                    calls[before]["params"]["items"].as_array().unwrap().len(),
                    1
                );
                assert_eq!(calls[before + 1]["method"], "integration.view.poll");
                let frames: Vec<_> = calls
                    .iter()
                    .filter(|r| r["method"] == "integration.view.replace")
                    .collect();
                assert!(
                    frames[1]["params"]["items"][1]["detail"]
                        .as_str()
                        .unwrap()
                        .contains("Selected (read-only)")
                );
                assert!(
                    !calls[before]["params"]
                        .to_string()
                        .contains("Selected (read-only)")
                );
                drop(calls);
                ui.finish();
            }
        }
    }

    #[test]
    fn both_platforms_expired_display_never_renews_even_for_fresh_failure() {
        let end = Instant::now() + Duration::from_secs(10);
        let mut displays = platform_samples().map(|sample| {
            let server = Server::new("");
            let mut ui =
                presentation::Presenter::new(Some("coven-discovery"), socket::select(&server.path));
            ui.sample(&sample, Instant::now(), end, &socket::NoCancel);
            let before = server.calls.lock().unwrap().len();
            (server, ui, sample, before)
        });
        thread::sleep(Duration::from_millis(5100));
        for (server, ui, sample, before) in &mut displays {
            ui.sample(
                &unavailable(2, "timeout"),
                Instant::now(),
                end,
                &socket::NoCancel,
            );
            ui.sample(sample, Instant::now(), end, &socket::NoCancel);
            ui.finish();
            assert_eq!(ui.report["outcome"], "expired");
            assert_eq!(ui.report["cleanup"], "ttl_only_expired");
            assert_eq!(server.calls.lock().unwrap().len(), *before);
        }
    }

    #[test]
    fn both_platforms_candidate_drift_invalidates_queued_selection() {
        for sample in platform_samples() {
            let server = Server::new("");
            let mut ui =
                presentation::Presenter::new(Some("coven-discovery"), socket::select(&server.path));
            let end = Instant::now() + Duration::from_secs(3);
            ui.sample(&sample, Instant::now(), end, &socket::NoCancel);
            let mut fresh = sample.clone();
            fresh["targets"][0]["matches"][0]["pid"] = json!(u32::MAX);
            ui.sample(&fresh, Instant::now(), end, &socket::NoCancel);
            assert_eq!(ui.report["selections"], 0);
            assert_eq!(ui.report["ignored_intents"], 4);
            ui.finish();
        }
    }
}
