use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    sync::{Arc, Mutex},
    thread,
};

#[test]
fn navigation_result_is_closed_and_nullable_fields_must_be_present() {
    let v = json!({"contract":"coven.agent-navigation.v1","server_epoch":"a".repeat(32),
        "observation_id":"b".repeat(64),"association_revision":null,"provenance":"user_asserted",
        "native_hosting":"unestablished","valid":false,"applied":false,"target":null});
    validate_control_result(&v).unwrap();
    for key in v.as_object().unwrap().keys() {
        let mut bad = v.clone();
        bad.as_object_mut().unwrap().remove(key);
        bad["provider_payload"] = json!("PRIVATE");
        assert!(validate_control_result(&bad).is_err(), "{key}");
    }
    let mut producer = v.clone();
    producer["provenance"] = json!("producer_reported");
    producer["native_hosting"] = json!("local_full_tui_root");
    validate_control_result(&producer).unwrap();
    producer["native_hosting"] = json!("unestablished");
    assert!(validate_control_result(&producer).is_err());
}

#[test]
fn mutation_replies_cannot_relabel_user_assertions_as_producer_evidence() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    let root = tempfile::Builder::new().prefix("mp").tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let receiver = root.path().join("r");
    let kit = root.path().join("k");
    let peer = UnixListener::bind(&receiver).unwrap();
    let kit_peer = UnixListener::bind(&kit).unwrap();
    for path in [&receiver, &kit] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let serve = |listener: UnixListener, frames, is_kit| {
        listener.set_nonblocking(true).unwrap();
        thread::spawn(move || {
            for index in 0..frames {
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((s, _)) => break s,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "reply fixture awaiting frame {index}"
                            );
                            thread::sleep(Duration::from_millis(1));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new(socket.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let r: Value = serde_json::from_str(&line).unwrap();
                let p = &r["params"];
                let result = if is_kit {
                    match r["method"].as_str().unwrap() {
                        "session.snapshot" => {
                            json!({"snapshot":{"terminal_navigation":{"version":1}}})
                        }
                        "terminal.inspect" => {
                            json!({"type":"terminal_inspected","terminal_id":"PRIVATE-terminal",
                            "runtime_generation":"generation","availability":"live","workspace_id":"w","tab_id":"t","pane_id":"p"})
                        }
                        other => panic!("unexpected Kit mutation: {other}"),
                    }
                } else {
                    let producer = index % 2 == 1;
                    json!({"contract":"coven.agent-navigation.v1","server_epoch":p["expected_epoch"],
                        "observation_id":p["observation_id"],"association_revision":p["expected_revision"],
                        "provenance":if producer {"producer_reported"} else {"user_asserted"},
                        "native_hosting":if producer {"local_full_tui_root"} else {"unestablished"},
                        "valid":true,"applied":p["apply"],"target":p.get("target").cloned().unwrap_or(json!({
                            "socket":"/PRIVATE-socket","device":1,"inode":2,"terminal_id":"PRIVATE-terminal","runtime_generation":"generation"}))})
                };
                let reply = if is_kit {
                    json!({"id":r["id"],"result":result})
                } else {
                    json!({"id":r["id"],"contract":r["contract"],"type":"result","result":result})
                };
                writeln!(socket, "{reply}").unwrap();
            }
        })
    };
    let peer_worker = serve(peer, 8, false);
    let kit_worker = serve(kit_peer, 8, true);
    let selection = Selection {
        socket: receiver.to_str().unwrap(),
        observation: &"b".repeat(64),
        expected_epoch: &"a".repeat(32),
        expected_revision: Some(&"c".repeat(32)),
    };
    let mut results = Vec::new();
    for remove in [false, true] {
        for apply in [false, true] {
            for producer in [false, true] {
                let result = if remove {
                    dissociate_arguments(&selection, apply, true)
                } else {
                    associate_arguments(
                        &selection,
                        kit.to_str().unwrap(),
                        "PRIVATE-terminal",
                        apply,
                        true,
                    )
                };
                results.push((producer, result));
            }
        }
    }
    peer_worker.join().unwrap();
    kit_worker.join().unwrap();
    for (producer, result) in results {
        if producer {
            let error = result
                .err()
                .expect("relabeled mutation must not construct public output");
            assert_eq!(error.code(), "agent_delivery_uncertain");
            assert!(!error.to_string().contains("PRIVATE"));
        } else {
            let value: Value = serde_json::from_str(&result.unwrap().stdout).unwrap();
            assert_eq!(value["provenance"], "user_asserted");
            assert_eq!(value["native_hosting"], "unestablished");
        }
    }
    root.close().expect("clean exact reply fixture");
}

#[test]
fn initial_target_capture_uses_exact_pane_mapping_and_never_resamples_reported_target() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    for mode in [
        "ok",
        "missing",
        "duplicate",
        "unsupported",
        "generation",
        "endpoint",
    ] {
        let root = tempfile::Builder::new().prefix("at").tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.path().join("k");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let worker = thread::spawn(move || {
            let frames = if matches!(mode, "missing" | "duplicate" | "unsupported") {
                1
            } else if mode == "generation" {
                5
            } else {
                3
            };
            for index in 0..frames {
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((s, _)) => break s,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline);
                            thread::sleep(Duration::from_millis(1));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new(socket.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let r: Value = serde_json::from_str(&line).unwrap();
                let result = match r["method"].as_str().unwrap() {
                    "session.snapshot" => {
                        let mut panes = vec![json!({"pane_id":"inherited","terminal_id":"exact"})];
                        if mode == "missing" {
                            panes.clear();
                        }
                        if mode == "duplicate" {
                            panes.push(panes[0].clone());
                        }
                        json!({"snapshot":{"terminal_navigation":{"version":if mode == "unsupported" {0} else {1}},"panes":panes}})
                    }
                    "terminal.inspect" => {
                        assert_eq!(r["params"], json!({"terminal_id":"exact"}));
                        json!({"type":"terminal_inspected","terminal_id":"exact","runtime_generation":
                            if index > 2 {"12345678-1234-1234-1234-123456789abd"} else {"12345678-1234-1234-1234-123456789abc"},
                            "availability":"live","workspace_id":"w","tab_id":"t","pane_id":"inherited"})
                    }
                    other => panic!("forbidden method: {other}"),
                };
                writeln!(socket, "{}", json!({"id":r["id"],"result":result})).unwrap();
            }
        });
        let result = attachment_target(path.to_str().unwrap(), "inherited");
        if matches!(mode, "missing" | "duplicate" | "unsupported") {
            assert!(result.is_err());
        } else {
            let target = result.unwrap();
            assert_eq!(target.as_object().unwrap().len(), 5);
            assert_eq!(target["terminal_id"], "exact");
            if mode == "generation" {
                assert!(check_attachment_target(path.to_str().unwrap(), &target).is_err());
            }
            if mode == "endpoint" {
                std::fs::rename(&path, root.path().join("old")).unwrap();
                let _replacement = UnixListener::bind(&path).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
                assert!(check_attachment_target(path.to_str().unwrap(), &target).is_err());
            }
        }
        worker.join().unwrap();
    }
}

#[test]
fn inspect_checks_capability_exact_id_and_live_generation_without_fallback() {
    for (version, availability, exact) in [
        (0, "live", true),
        (1, "exited", true),
        (1, "detached", true),
        (1, "live", false),
        (1, "live", true),
    ] {
        let root = tempfile::Builder::new().prefix("cn").tempdir().unwrap();
        let path = root.path().join("k");
        let listener = UnixListener::bind(&path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let methods = Arc::new(Mutex::new(Vec::new()));
        let seen = methods.clone();
        let worker = thread::spawn(move || {
            for _ in 0..if version == 1 { 2 } else { 1 } {
                let (mut socket, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(socket.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                seen.lock()
                    .unwrap()
                    .push(request["method"].as_str().unwrap().to_owned());
                let result = if request["method"] == "session.snapshot" {
                    json!({"snapshot":{"terminal_navigation":{"version":version}}})
                } else {
                    assert_eq!(request["params"], json!({"terminal_id":"term"}));
                    json!({"type":"terminal_inspected","terminal_id":if exact {"term"} else {"other"},
                        "runtime_generation":"generation","availability":availability,"workspace_id":"w","tab_id":"t","pane_id":"p"})
                };
                writeln!(socket, "{}", json!({"id":request["id"],"result":result})).unwrap();
            }
        });
        let endpoint = crate::observation_socket::select(path.to_str().unwrap()).unwrap();
        assert_eq!(
            unix::inspect(&endpoint, "term").is_ok(),
            version == 1 && availability == "live" && exact
        );
        worker.join().unwrap();
        assert!(
            methods
                .lock()
                .unwrap()
                .iter()
                .all(|m| matches!(m.as_str(), "session.snapshot" | "terminal.inspect"))
        );
    }
}
