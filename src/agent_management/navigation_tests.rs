use super::*;

struct NavigationChild(std::process::Child);

impl Drop for NavigationChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        self.0.wait().expect("reap owned navigation test child");
    }
}

fn isolated_navigation_scenario(mode: &str) {
    // The parent owns storage so a killed/panicking child cannot leak fixtures.
    let artifacts = tempfile::Builder::new().prefix("nc").tempdir().unwrap();
    let result = (|| -> io::Result<std::process::ExitStatus> {
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command.args([
            "--exact",
            "agent_management::unix::navigation::tests::real_receiver_and_fake_kit_focus_one_frame_and_follow_movement",
            "--nocapture",
            "--quiet",
            "--test-threads=1",
        ])
            .env("COVEN_NAVIGATION_TEST_SCENARIO", mode)
            .env("TMPDIR", artifacts.path());
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            let parent = std::process::id() as libc::pid_t;
            unsafe {
                command.pre_exec(move || {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::getppid() != parent {
                        libc::_exit(125);
                    }
                    Ok(())
                });
            }
        }
        let mut child = NavigationChild(command.spawn()?);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = child.0.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("navigation scenario deadline: {mode}"),
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
    })();
    // The child (including its socket threads) is stopped/reaped before removal.
    let cleanup = artifacts.close();
    assert!(
        cleanup.is_ok(),
        "navigation fixture cleanup failed: {cleanup:?}; scenario {mode}: {result:?}"
    );
    assert!(
        result.as_ref().is_ok_and(|status| status.success()),
        "isolated navigation scenario {mode} failed: {result:?}"
    );
}

fn report(native: &mut native::NativeStore, seq: u64, event: &str, now: Instant) {
    let mut p = json!({"instance_id":"12345678-1234-1234-1234-123456789abc", "session_id":"PRIVATE",
        "sequence":seq,"event":event});
    if event == "session.status" {
        p["status"] = json!("busy");
    }
    native.report(&p, now).unwrap();
}

#[test]
fn private_naming_hints_follow_manual_and_producer_expiry_without_public_fields() {
    let now = Instant::now();
    let epoch = "a".repeat(32);
    let mut native = native::NativeStore::default();
    report(&mut native, 1, "session.status", now);
    let legacy = json!({"server_epoch":epoch,"records":[]});
    let view = native.projection(legacy.clone(), now);
    let id = view["observations"][0]["observation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let target = json!({"socket":"/PRIVATE-SOCKET","device":1,"inode":2,
        "terminal_id":"PRIVATE-TERMINAL","runtime_generation":"12345678-1234-1234-1234-123456789abc"});
    let mut associations = Associations::default();
    assert_eq!(associations.naming_hints(&view, &native, now), json!([]));
    associations
        .control(
            "association.assert",
            &json!({"observation_id":id,"expected_epoch":epoch,
        "expected_revision":null,"provenance":"user_asserted","target":target,"apply":true}),
            &epoch,
            &native,
            now,
        )
        .unwrap();
    let hints = associations.naming_hints(&view, &native, now);
    assert_eq!(hints[0]["target"], target);
    assert_eq!(hints[0]["id"], format!("observation:{id}"));
    assert!(
        !native
            .projection(legacy.clone(), now)
            .to_string()
            .contains("PRIVATE")
    );
    assert_eq!(
        associations.naming_hints(&view, &native, now + Duration::from_secs(30)),
        json!([])
    );
    associations.0.clear();
    let mut attachment = json!({"expected_epoch":epoch,"client_id":"12345678-1234-1234-1234-123456789abd",
        "revision":1,"operation":"attach","instance_id":"12345678-1234-1234-1234-123456789abc",
        "session_id":"PRIVATE","scope":"local_full_tui_root","target":target});
    native.report_attachment(&attachment, &epoch, now).unwrap();
    assert_eq!(
        associations.naming_hints(&view, &native, now)[0]["target"],
        target
    );
    assert_eq!(
        associations.naming_hints(&view, &native, now + Duration::from_secs(5)),
        json!([])
    );
    attachment["client_id"] = json!("12345678-1234-1234-1234-123456789abe");
    attachment["target"]["terminal_id"] = json!("PRIVATE-CONFLICT");
    native.report_attachment(&attachment, &epoch, now).unwrap();
    assert_eq!(associations.naming_hints(&view, &native, now), json!([]));
    assert!(
        !native
            .projection(legacy, now)
            .to_string()
            .contains("PRIVATE")
    );
}

#[test]
fn attachment_deletion_tombstone_pressure_cannot_revive_old_focus_revision() {
    let now = Instant::now();
    let epoch = "a".repeat(32);
    let instance = "12345678-1234-1234-1234-123456789abc";
    let mut native = native::NativeStore::default();
    for sequence in 1..=127 {
        native
            .report(
                &json!({"instance_id":instance,"session_id":format!("fresh-{sequence}"),
            "sequence":sequence,"event":"session.status","status":"busy"}),
                now + Duration::from_micros(sequence),
            )
            .unwrap();
    }
    let mut attachment = json!({"expected_epoch":epoch,"client_id":"12345678-1234-1234-1234-123456789abd",
        "revision":1,"operation":"attach","instance_id":instance,"session_id":"PRIVATE",
        "scope":"local_full_tui_root","target":{"socket":"/not/contacted","device":1,"inode":2,
            "terminal_id":"producer","runtime_generation":instance}});
    let captured = now + Duration::from_micros(127);
    native
        .report_attachment(&attachment, &epoch, captured)
        .unwrap();
    let view = native.projection(json!({"server_epoch":epoch,"records":[]}), captured);
    assert_eq!(view["observations"].as_array().unwrap().len(), 128);
    let id = view["observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["origin"] == "djinn_client_attachment")
        .unwrap()["observation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let revision = native.producer_association(&id, captured).unwrap().0;
    let focus = json!({"observation_id":id,"expected_epoch":epoch,"expected_revision":revision});
    let mut associations = Associations::default();
    associations
        .control("focus.prepare", &focus, &epoch, &native, captured)
        .unwrap();
    let deleted = now + Duration::from_micros(128);
    report(&mut native, 128, "session.deleted", deleted);
    assert!(
        associations
            .control("focus.prepare", &focus, &epoch, &native, deleted)
            .is_err()
    );
    for n in 1..=512 {
        let sequence = 128 + n;
        native
            .report(
                &json!({"instance_id":instance,"session_id":format!("deleted-{n}"),
            "sequence":sequence,"event":"session.deleted"}),
                now + Duration::from_micros(sequence),
            )
            .unwrap();
    }
    let recreated = now + Duration::from_micros(641);
    // The oldest tombstone has left bounded history, with no sequence gap or lease expiry.
    report(&mut native, 641, "session.updated", recreated);
    assert!(
        native.producer_association(&id, recreated).is_none(),
        "old lease must not revive after tombstone pressure"
    );
    assert!(
        associations
            .control("focus.prepare", &focus, &epoch, &native, recreated)
            .is_err()
    );
    assert_eq!(
        native
            .report_attachment(&attachment, &epoch, recreated)
            .unwrap_err()
            .code(),
        "agent_stale_revision"
    );
    attachment["revision"] = json!(2);
    native
        .report_attachment(&attachment, &epoch, recreated)
        .unwrap();
    let next = native.producer_association(&id, recreated).unwrap().0;
    assert_ne!(next, revision);
    assert!(
        associations
            .control("focus.prepare", &focus, &epoch, &native, recreated)
            .is_err()
    );
    let mut current = focus;
    current["expected_revision"] = json!(next);
    associations
        .control("focus.prepare", &current, &epoch, &native, recreated)
        .unwrap();
    let view = native.projection(json!({"server_epoch":epoch,"records":[]}), recreated);
    let rows = view["observations"].as_array().unwrap();
    assert!(rows.len() <= 128);
    assert_eq!(
        rows.iter()
            .filter(|o| o["reported_state"] == "working")
            .count(),
        127
    );
    assert_eq!(view["omitted_observations"], 1);
}

#[test]
fn explicit_user_assertion_precedes_producer_without_relabeling_or_renewal() {
    let now = Instant::now();
    let epoch = "a".repeat(32);
    let mut native = native::NativeStore::default();
    report(&mut native, 1, "session.status", now);
    let id = native.projection(json!({"server_epoch":epoch,"records":[]}), now)["observations"][0]
        ["observation_id"]
        .clone();
    let target = json!({"socket":"/not/contacted","device":1,"inode":2,"terminal_id":"producer",
        "runtime_generation":"12345678-1234-1234-1234-123456789abc"});
    let mut attachment = json!({"expected_epoch":epoch,"client_id":"12345678-1234-1234-1234-123456789abd",
        "revision":1,"operation":"attach","instance_id":"12345678-1234-1234-1234-123456789abc",
        "session_id":"PRIVATE","scope":"local_full_tui_root","target":target});
    native.report_attachment(&attachment, &epoch, now).unwrap();
    let mut store = Associations::default();
    let inspection = json!({"observation_id":id,"expected_epoch":epoch,"expected_revision":null});
    let producer = store
        .control("association.inspect", &inspection, &epoch, &native, now)
        .unwrap();
    assert_eq!(producer["provenance"], "producer_reported");
    let mut assertion = json!({"observation_id":id,"expected_epoch":epoch,"expected_revision":null,
        "provenance":"user_asserted","apply":true,"target":target});
    assertion["target"]["terminal_id"] = json!("user");
    let applied = store
        .control("association.assert", &assertion, &epoch, &native, now)
        .unwrap();
    let mut focus = inspection.clone();
    focus["expected_revision"] = producer["association_revision"].clone();
    assert!(
        store
            .control("focus.prepare", &focus, &epoch, &native, now)
            .is_err()
    );
    focus["expected_revision"] = applied["association_revision"].clone();
    assert_eq!(
        store
            .control("focus.prepare", &focus, &epoch, &native, now)
            .unwrap()["provenance"],
        "user_asserted"
    );
    for rev in 2..=9 {
        attachment["revision"] = json!(rev);
        native
            .report_attachment(
                &attachment,
                &epoch,
                now + Duration::from_secs((rev - 1) * 4),
            )
            .unwrap();
    }
    let later = now + Duration::from_secs(32);
    assert!(
        store
            .control("focus.prepare", &focus, &epoch, &native, later)
            .is_err()
    );
    let next = store
        .control("association.inspect", &inspection, &epoch, &native, later)
        .unwrap();
    assert_eq!(next["provenance"], "producer_reported");
    assert_eq!(
        next["association_revision"],
        producer["association_revision"]
    );
    assert!(!store.0[id.as_str().unwrap()].valid);
}

#[test]
fn assertions_preview_cas_ttl_and_invalidation_are_receiver_owned() {
    let now = Instant::now();
    let mut native = native::NativeStore::default();
    report(&mut native, 1, "session.status", now);
    let legacy = json!({"server_epoch":"a".repeat(32),"records":[]});
    let view = native.projection(legacy, now);
    let id = view["observations"][0]["observation_id"].clone();
    let epoch = "a".repeat(32);
    let mut store = Associations::default();
    let mut p = json!({"observation_id":id,"expected_epoch":epoch,"expected_revision":null,
        "provenance":"user_asserted","apply":false,"target":{"socket":"/not/contacted","device":1,"inode":2,
            "terminal_id":"term","runtime_generation":"gen"}});
    store
        .control("association.assert", &p, &epoch, &native, now)
        .unwrap();
    assert!(store.0.is_empty());
    p["apply"] = json!(true);
    let applied = store
        .control("association.assert", &p, &epoch, &native, now)
        .unwrap();
    assert_eq!(store.0.len(), 1);
    assert!(
        store
            .control("association.assert", &p, &epoch, &native, now)
            .is_err()
    );
    let mut focus = json!({"observation_id":id,"expected_epoch":epoch,"expected_revision":applied["association_revision"]});
    store
        .control("focus.prepare", &focus, &epoch, &native, now)
        .unwrap();
    focus["expected_epoch"] = json!("c".repeat(32));
    assert!(
        store
            .control("focus.prepare", &focus, &epoch, &native, now)
            .is_err()
    );
    focus["expected_epoch"] = json!(epoch);
    store.refresh(&native, now + Duration::from_secs(30));
    report(
        &mut native,
        2,
        "session.status",
        now + Duration::from_secs(30),
    );
    assert!(
        store
            .control(
                "focus.prepare",
                &focus,
                &epoch,
                &native,
                now + Duration::from_secs(30)
            )
            .is_err()
    );
    p["expected_revision"] = applied["association_revision"].clone();
    let replaced = store
        .control(
            "association.assert",
            &p,
            &epoch,
            &native,
            now + Duration::from_secs(30),
        )
        .unwrap();
    focus["expected_revision"] = replaced["association_revision"].clone();
    report(
        &mut native,
        4,
        "session.status",
        now + Duration::from_secs(31),
    );
    assert!(
        store
            .control(
                "focus.prepare",
                &focus,
                &epoch,
                &native,
                now + Duration::from_secs(31)
            )
            .is_err()
    );
    assert!(
        Associations::default()
            .control("focus.prepare", &focus, &epoch, &native, now)
            .is_err()
    );
}

#[test]
fn handler_lapsed_native_fact_cannot_revive_an_unexpired_assertion() {
    let root = tempfile::Builder::new().prefix("cn").tempdir().unwrap();
    std::fs::write(root.path().join("coven.json"), b"{}").unwrap();
    let mut persistent = store::Store::open(root.path()).unwrap();
    let epoch = persistent.epoch.clone();
    let t0 = Instant::now();
    let mut native = native::NativeStore::default();
    report(&mut native, 1, "session.status", t0);
    let id =
        native.projection(persistent.projection(), t0)["observations"][0]["observation_id"].clone();
    let mut associations = Associations::default();
    let mut assertion = json!({"observation_id":id,"expected_epoch":epoch,"expected_revision":null,
        "provenance":"user_asserted","apply":true,"target":{"socket":"/not/contacted","device":1,"inode":2,
        "terminal_id":"term","runtime_generation":"gen"}});
    let applied = associations
        .control(
            "association.assert",
            &assertion,
            &epoch,
            &native,
            t0 + Duration::from_secs(20),
        )
        .unwrap();
    let boundary = t0 + Duration::from_secs(30);
    associations.refresh(&native, boundary - Duration::from_nanos(1));
    assert!(associations.0.values().next().unwrap().valid);
    let frame = serde_json::to_vec(
        &json!({"id":"f".repeat(32),"contract":native::RPC,"method":"report",
        "params":{"instance_id":"12345678-1234-1234-1234-123456789abc","session_id":"PRIVATE",
        "sequence":2,"event":"session.status","status":"busy"}}),
    )
    .unwrap();
    let after = boundary + Duration::from_nanos(1);
    let mut samples = 0;
    let reply = handle_normal_navigation_with_clock(
        &mut persistent,
        &mut native,
        &mut associations,
        &frame,
        || {
            samples += 1;
            after
        },
    )
    .unwrap();
    assert_eq!(samples, 1);
    assert_eq!(
        serde_json::from_slice::<Value>(&reply).unwrap()["type"],
        "result"
    );
    let mut focus = json!({"observation_id":id,"expected_epoch":epoch,"expected_revision":applied["association_revision"]});
    assert_eq!(
        associations
            .control("focus.prepare", &focus, &epoch, &native, after)
            .unwrap_err()
            .code(),
        "agent_association_unknown"
    );
    // The assertion still has nearly twenty seconds of TTL, but continuity was lost.
    assertion["expected_revision"] = applied["association_revision"].clone();
    let replaced = associations
        .control("association.assert", &assertion, &epoch, &native, after)
        .unwrap();
    assert_ne!(
        replaced["association_revision"],
        applied["association_revision"]
    );
    assert!(
        associations
            .control("focus.prepare", &focus, &epoch, &native, after)
            .is_err()
    );
    focus["expected_revision"] = replaced["association_revision"].clone();
    associations
        .control("focus.prepare", &focus, &epoch, &native, after)
        .unwrap();
}

#[test]
fn bounded_assertions_never_silently_evict_cas_history() {
    let now = Instant::now();
    let epoch = "a".repeat(32);
    let mut native = native::NativeStore::default();
    let mut associations = Associations::default();
    for n in 1..=129 {
        let now = if n == 129 {
            now + Duration::from_secs(601)
        } else {
            now
        };
        let event = json!({"instance_id":"12345678-1234-1234-1234-123456789abc",
            "session_id":format!("PRIVATE-{n}"),"sequence":n,"event":"permission.asked","request_id":"PRIVATE-request"});
        native.report(&event, now).unwrap();
        let view = native
            .control(
                "attention.list",
                &json!({}),
                json!({"server_epoch":epoch,"records":[]}),
                now,
            )
            .unwrap();
        let id = view["observations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["observation_id"].as_str().unwrap())
            .find(|id| !associations.0.contains_key(*id))
            .unwrap();
        let p = json!({"observation_id":id,"expected_epoch":epoch,"expected_revision":null,"apply":true,
            "provenance":"user_asserted","target":{"socket":"/never/contacted","device":1,"inode":2,
                "terminal_id":"term","runtime_generation":"gen"}});
        let result = associations.control("association.assert", &p, &epoch, &native, now);
        if n == 129 {
            assert!(result.is_err());
        } else {
            result.unwrap();
        }
    }
    assert_eq!(associations.0.len(), 128);
}

#[test]
fn metadata_cannot_associate_ending_invalidates_and_removal_requires_cas() {
    let now = Instant::now();
    let epoch = "a".repeat(32);
    let mut native = native::NativeStore::default();
    report(&mut native, 1, "session.created", now);
    let id = native.projection(json!({"server_epoch":epoch,"records":[]}), now)["observations"][0]
        ["observation_id"]
        .clone();
    let mut store = Associations::default();
    let p = json!({"observation_id":id,"expected_epoch":epoch,"expected_revision":null,"apply":true,
        "provenance":"user_asserted","target":{"socket":"/never/contacted","device":1,"inode":2,
            "terminal_id":"term","runtime_generation":"gen"}});
    assert!(
        store
            .control("association.assert", &p, &epoch, &native, now)
            .is_err()
    );
    assert!(store.0.is_empty());
    report(&mut native, 2, "session.status", now);
    assert!(store.0.is_empty());
    let applied = store
        .control("association.assert", &p, &epoch, &native, now)
        .unwrap();
    let mut remove = json!({"observation_id":id,"expected_epoch":epoch,
        "expected_revision":applied["association_revision"],"apply":false,"provenance":"user_asserted"});
    store
        .control("association.remove", &remove, &epoch, &native, now)
        .unwrap();
    assert!(store.0.values().next().unwrap().valid);
    report(&mut native, 3, "session.deleted", now);
    store.refresh(&native, now);
    assert!(!store.0.values().next().unwrap().valid);
    remove["apply"] = json!(true);
    let removed = store
        .control("association.remove", &remove, &epoch, &native, now)
        .unwrap();
    assert_ne!(
        removed["association_revision"],
        applied["association_revision"]
    );
    assert!(
        store
            .control("association.remove", &remove, &epoch, &native, now)
            .is_err()
    );
    assert_eq!(store.0.len(), 1);
}

#[test]
fn real_receiver_and_fake_kit_focus_one_frame_and_follow_movement() {
    use std::io::{BufRead, BufReader};
    const MODES: &[&str] = &[
        "success",
        "uncertain",
        "timeout",
        "generation",
        "exited",
        "detached",
        "unsupported",
        "endpoint",
        "malformed_json",
        "duplicate_keys",
        "deep_json",
        "error_null",
        "error_extra",
        "error_missing_message",
        "error_wrong_type",
        "error_code_limit",
        "error_message_limit",
        "error_wrong_id",
        "valid_error",
        "membership_before_final_prepare",
        "detach_before_final_prepare",
    ];
    let Ok(mode) = std::env::var("COVEN_NAVIGATION_TEST_SCENARIO") else {
        for mode in MODES {
            if !mode.ends_with("before_final_prepare") {
                isolated_navigation_scenario(mode);
            }
        }
        for mode in [
            "success",
            "uncertain",
            "timeout",
            "generation",
            "exited",
            "detached",
            "unsupported",
            "endpoint",
            "membership_before_final_prepare",
            "detach_before_final_prepare",
        ] {
            isolated_navigation_scenario(&format!("producer_{mode}"));
        }
        return;
    };
    let producer = mode.starts_with("producer_");
    let mode = mode.strip_prefix("producer_").unwrap_or(&mode);
    let mode = *MODES
        .iter()
        .find(|&&candidate| candidate == mode)
        .unwrap_or_else(|| panic!("unknown navigation scenario: {mode}"));
    {
        let final_race = mode.ends_with("before_final_prepare");
        let generation = if producer {
            "12345678-1234-1234-1234-123456789abc"
        } else {
            "gen"
        };
        let rejected = matches!(
            mode,
            "generation" | "exited" | "detached" | "unsupported" | "endpoint"
        );
        let root = tempfile::Builder::new().prefix("cn").tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::create_dir(root.path().join("state")).unwrap();
        std::fs::write(root.path().join("coven.json"), b"{}").unwrap();
        let bound = Bound::bind(root.path().join("r").to_str().unwrap()).unwrap();
        // Keep the endpoint alive through the client's post-reply identity fence.
        let receiver_listener = bound.listener.try_clone().unwrap();
        let kit_path = root.path().join("k");
        let listener = UnixListener::bind(&kit_path).unwrap();
        listener.set_nonblocking(true).unwrap();
        std::fs::set_permissions(&kit_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let kit_worker = thread::spawn(move || {
            let mut methods = Vec::new();
            for index in 0..if mode == "endpoint" {
                2
            } else if mode == "unsupported" {
                3
            } else if rejected || final_race {
                4
            } else {
                5
            } {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "Kit fixture awaiting frame {index}, scenario {mode}, seen {methods:?}"
                            );
                            thread::sleep(Duration::from_millis(1));
                        }
                        Err(error) => panic!("Kit fixture accept: {error}"),
                    }
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new(socket.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                let method = request["method"].as_str().unwrap();
                methods.push(method.to_owned());
                let result = match method {
                    "session.snapshot" => {
                        json!({"snapshot":{"terminal_navigation":{"version":if index > 1 && mode == "unsupported" {0} else {1}}}})
                    }
                    "terminal.inspect" => {
                        json!({"type":"terminal_inspected","terminal_id":"term",
                        "runtime_generation":if index > 1 && mode == "generation" {"new"} else {generation},
                        "availability":if index > 1 && matches!(mode,"exited" | "detached") {mode} else {"live"},
                        "workspace_id":"w","tab_id":"t","pane_id":"old"})
                    }
                    "terminal.focus" => {
                        assert_eq!(
                            request["params"],
                            json!({"terminal_id":"term","expected_runtime_generation":generation})
                        );
                        if mode == "timeout" {
                            thread::sleep(Duration::from_millis(1100));
                            break;
                        }
                        if mode == "uncertain" {
                            break;
                        }
                        let error = match mode {
                            "error_null" => Some(Value::Null),
                            "error_extra" => Some(
                                json!({"code":"PRIVATE-code","message":"PRIVATE-message","payload":"PRIVATE"}),
                            ),
                            "error_missing_message" => Some(json!({"code":"PRIVATE-code"})),
                            "error_wrong_type" => {
                                Some(json!({"code":1,"message":"PRIVATE-message"}))
                            }
                            "error_code_limit" => {
                                Some(json!({"code":"x".repeat(129),"message":"PRIVATE-message"}))
                            }
                            "error_message_limit" => {
                                Some(json!({"code":"PRIVATE-code","message":"x".repeat(4097)}))
                            }
                            "error_wrong_id" | "valid_error" => Some(
                                json!({"code":"runtime_generation_mismatch","message":"PRIVATE-message"}),
                            ),
                            _ => None,
                        };
                        let raw = match mode {
                            "malformed_json" => Some("{PRIVATE-not-json".to_owned()),
                            "duplicate_keys" => Some(format!(r#"{{"id":{},"error":null,"error":null}}"#,request["id"])),
                            "deep_json" => Some(format!(r#"{{"id":{},"result":{}0{}}}"#,request["id"],"[".repeat(40),"]".repeat(40))),
                            _ => error.map(|error| json!({"id":if mode == "error_wrong_id" {json!("wrong")} else {request["id"].clone()},"error":error}).to_string()),
                        };
                        if let Some(raw) = raw {
                            writeln!(socket, "{raw}").unwrap();
                            break;
                        }
                        json!({"type":"terminal_focused","terminal_id":"term","runtime_generation":generation,
                            "workspace_id":"w","tab_id":"t","pane_id":"moved"})
                    }
                    _ => panic!("unexpected method {method}"),
                };
                writeln!(socket, "{}", json!({"id":request["id"],"result":result})).unwrap();
            }
            methods
        });
        let mut persistent = store::Store::open(root.path()).unwrap();
        let mut native = native::NativeStore::default();
        let now = Instant::now();
        report(&mut native, 1, "session.status", now);
        let epoch = persistent.epoch.clone();
        let id =
            native.projection(persistent.projection(), now)["observations"][0]["observation_id"]
                .as_str()
                .unwrap()
                .to_owned();
        if producer {
            native = native::NativeStore::default();
        }
        let receiver = thread::spawn(move || {
            let interrupt = crate::observation_socket::InterruptGuard::install().unwrap();
            let mut associations = Associations::default();
            let mut attachment = Value::Null;
            for index in 0..(if rejected { 2 } else { 3 }) + usize::from(producer) {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match receiver_listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline);
                            thread::sleep(Duration::from_millis(1));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                stream.set_nonblocking(true).unwrap();
                let bytes = receive(&mut stream, deadline, &interrupt).unwrap();
                if final_race {
                    let request: Value = serde_json::from_slice(&bytes).unwrap();
                    if index == 0 {
                        attachment = request["params"].clone();
                    }
                    if index == 3 {
                        assert_eq!(request["method"], "focus.prepare");
                        let changed = if mode == "detach_before_final_prepare" {
                            json!({"expected_epoch":persistent.epoch,"client_id":attachment["client_id"],
                                "revision":2,"operation":"detach"})
                        } else {
                            let mut added = attachment.clone();
                            added["client_id"] = json!("12345678-1234-1234-1234-123456789abe");
                            added
                        };
                        native
                            .report_attachment(&changed, &persistent.epoch, Instant::now())
                            .unwrap();
                    }
                }
                let reply = handle_normal_navigation(
                    &mut persistent,
                    &mut native,
                    &mut associations,
                    &bytes,
                )
                .unwrap();
                send(&mut stream, &reply, deadline, &interrupt).unwrap();
            }
        });
        let path = root.path().join("r");
        let mut selection = crate::agent_navigation::Selection {
            socket: path.to_str().unwrap(),
            observation: &id,
            expected_epoch: &epoch,
            expected_revision: None,
        };
        let result = if producer {
            let endpoint = crate::observation_socket::select(kit_path.to_str().unwrap()).unwrap();
            let target = json!({"socket":endpoint.resolved,"device":endpoint.device,"inode":endpoint.inode,
                "terminal_id":"term","runtime_generation":generation});
            crate::agent_navigation::check_attachment_target(kit_path.to_str().unwrap(), &target).unwrap();
            call_contract(selection.socket, native::ATTACHMENT_RPC, "report", json!({
                "expected_epoch":epoch,"client_id":"12345678-1234-1234-1234-123456789abd","revision":1,
                "operation":"attach","instance_id":"12345678-1234-1234-1234-123456789abc",
                "session_id":"PRIVATE","scope":"local_full_tui_root","target":target})).unwrap();
            crate::agent_navigation::association_arguments(&selection, true)
        } else { crate::agent_navigation::associate_arguments(
            &selection,
            kit_path.to_str().unwrap(),
            "term",
            true,
            true,
        ) }
        .unwrap();
        let applied: Value = serde_json::from_str(&result.stdout).unwrap();
        assert_eq!(
            applied["provenance"],
            if producer {
                "producer_reported"
            } else {
                "user_asserted"
            }
        );
        selection.expected_revision = applied["association_revision"].as_str();
        let _replacement = if mode == "endpoint" {
            std::fs::remove_file(&kit_path).unwrap();
            let listener = UnixListener::bind(&kit_path).unwrap();
            std::fs::set_permissions(&kit_path, std::fs::Permissions::from_mode(0o600)).unwrap();
            Some(listener)
        } else {
            None
        };
        let result = crate::agent_navigation::focus_arguments(&selection, true, true);
        let receiver_result = receiver.join();
        let methods = kit_worker.join();
        let outcome = result.as_ref().err().map(|error| error.code());
        assert!(
            receiver_result.is_ok(),
            "receiver fixture failed: scenario {mode}, focus error {outcome:?}"
        );
        let methods = methods.unwrap_or_else(|_| {
            panic!("Kit fixture failed: scenario {mode}, focus error {outcome:?}")
        });
        if mode != "success" {
            let error = result.err().unwrap();
            if !rejected {
                assert_eq!(
                    error.code(),
                    if final_race {
                        "agent_report_rejected"
                    } else if mode == "valid_error" {
                        "agent_navigation_rejected"
                    } else {
                        "agent_delivery_uncertain"
                    },
                    "{mode}"
                );
                assert!(!error.to_string().contains("PRIVATE"));
            }
        } else {
            let focused: Value = serde_json::from_str(&result.unwrap().stdout).unwrap();
            assert_eq!(focused["pane_id"], "moved");
        }
        let expected = [
            "session.snapshot",
            "terminal.inspect",
            "session.snapshot",
            "terminal.inspect",
            "terminal.focus",
        ];
        assert_eq!(
            methods,
            expected[..if mode == "endpoint" {
                2
            } else if mode == "unsupported" {
                3
            } else if rejected || final_race {
                4
            } else {
                5
            }]
        );
    }
}
