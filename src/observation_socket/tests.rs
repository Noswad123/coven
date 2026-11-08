use super::*;

#[test]
fn paths_are_explicit_and_byte_bounded() {
    for path in ["", "sock", "/", "/a/", "/a/../sock", "/a\0sock"] {
        assert!(validate_path(path).is_err(), "{path:?}");
        assert_eq!(
            resolve(path).unwrap_err().code,
            "invalid_socket_path",
            "{path:?}"
        );
    }
    assert!(validate_path("/var/run/private.sock").is_ok());
    assert!(validate_path(&format!("/{}", "a".repeat(4095))).is_ok());
    assert!(validate_path(&format!("/{}", "a".repeat(4096))).is_err());
    assert!(validate_path(&format!("/{}", "\u{e9}".repeat(2048))).is_err());
}

#[test]
fn budget_is_exact_and_exhaustion_cannot_wrap() {
    let mut budget = ReadBudget::new(3);
    budget.charge(2).unwrap();
    assert_eq!(budget.remaining(), 1);
    budget.charge(1).unwrap();
    budget.charge(0).unwrap();
    assert_eq!(
        budget.charge(usize::MAX).unwrap_err().code,
        "response_limit"
    );
    assert_eq!(budget.remaining(), 0);
}

#[test]
fn only_correlated_safe_rejections_are_rejected() {
    let rejection =
        json!({"id": "id", "error": {"code": "disabled.Source-1", "message": "PRIVATE"}});
    let failure = acknowledgement(&rejection, "id").unwrap_err();
    assert_eq!(failure.code, "server_error:disabled.Source-1");
    assert_eq!(failure.delivery, Delivery::Rejected);
    for root in [
        json!(null),
        json!({"id": "wrong", "result": {"type": "ok"}}),
        json!({"id": "id", "error": {"code": "bad:code", "message": "PRIVATE"}}),
        json!({"id": "id", "error": {"code": "", "message": "PRIVATE"}}),
        json!({"id": "id", "error": {"code": "x".repeat(65), "message": "PRIVATE"}}),
        json!({"id": "id", "error": {"code": "bad", "message": 1}}),
        json!({"id": "id", "error": {}, "result": {"type": "ok"}}),
        json!({"id": "id", "result": {"type": "ok", "extra": true}}),
        json!({"id": "id", "result": {"type": "wrong"}}),
    ] {
        assert_eq!(
            acknowledgement(&root, "id").unwrap_err().delivery,
            Delivery::Unsent
        );
    }
    acknowledgement(&json!({"id": "id", "result": {"type": "ok"}}), "id").unwrap();
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::thread;
    use std::time::Duration;

    struct Server {
        _dir: tempfile::TempDir,
        path: String,
        listener: UnixListener,
    }

    impl Server {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("api.sock");
            let listener = UnixListener::bind(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            Self {
                _dir: dir,
                path: path.to_str().unwrap().into(),
                listener,
            }
        }

        fn serve(
            &self,
            handler: impl FnOnce(UnixStream, Value) + Send + 'static,
        ) -> thread::JoinHandle<()> {
            let listener = self.listener.try_clone().unwrap();
            thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let request = serde_json::from_str(&line).unwrap();
                handler(stream, request);
            })
        }

        fn endpoint(&self) -> Value {
            select(&self.path).unwrap().identity().unwrap()
        }
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(2)
    }

    fn read_with(server: &Server, budget: &mut ReadBudget) -> WireResult<Reply> {
        read(
            &server.path,
            ReadMethod::Snapshot,
            &json!({}),
            deadline(),
            budget,
            &NoCancel,
        )
    }

    fn write_with(server: &Server) -> WireResult<()> {
        annotate(
            &server.path,
            &server.endpoint(),
            &mut json!({}),
            deadline(),
            1024,
            &NoCancel,
            &mut |_| Ok(()),
        )
    }

    fn respond(mut stream: UnixStream, request: Value, result: Value) {
        writeln!(stream, "{}", json!({"id": request["id"], "result": result})).unwrap();
    }

    #[test]
    fn reads_are_narrow_correlated_and_charge_lf() {
        for (method, name) in [
            (ReadMethod::Snapshot, "session.snapshot"),
            (ReadMethod::ProcessInfo, "pane.process_info"),
        ] {
            let server = Server::new();
            let worker = server.serve(move |mut stream, request| {
                assert_eq!(request["method"], name);
                assert_eq!(request["params"], json!({"pane_id": "p"}));
                stream.write_all(b"{\"ok\":").unwrap();
                stream.write_all(b"true}\n").unwrap();
            });
            let mut budget = ReadBudget::new(12);
            let reply = read(
                &server.path,
                method,
                &json!({"pane_id":"p"}),
                deadline(),
                &mut budget,
                &NoCancel,
            )
            .unwrap();
            assert_eq!(reply.payload, b"{\"ok\":true}");
            assert_eq!(reply.request_id.len(), 32);
            assert_eq!(reply.endpoint.identity().unwrap(), server.endpoint());
            assert_eq!(budget.remaining(), 0);
            worker.join().unwrap();
        }
    }

    #[test]
    fn failed_framing_still_spends_received_bytes() {
        for (bytes, code) in [
            (b"{}".as_slice(), "incomplete_response"),
            (b"{}\nx", "trailing_data"),
            (b"{}\n\n", "trailing_data"),
        ] {
            let server = Server::new();
            let worker = server.serve(move |mut stream, _| {
                stream.write_all(bytes).unwrap();
            });
            let mut budget = ReadBudget::new(bytes.len());
            assert_eq!(read_with(&server, &mut budget).unwrap_err().code, code);
            assert_eq!(budget.remaining(), 0);
            worker.join().unwrap();
        }
    }

    #[test]
    fn shared_budget_is_checked_before_any_followup_connection() {
        let server = Server::new();
        let worker = server.serve(|mut stream, _| {
            stream.write_all(b"{}\n").unwrap();
        });
        let mut budget = ReadBudget::new(3);
        read_with(&server, &mut budget).unwrap();
        worker.join().unwrap();
        assert_eq!(
            read_with(&server, &mut budget).unwrap_err().code,
            "response_limit"
        );
        server.listener.set_nonblocking(true).unwrap();
        assert_eq!(
            server.listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn overflow_never_consumes_more_than_remaining_budget() {
        let server = Server::new();
        let worker = server.serve(|mut stream, _| {
            stream.write_all(b"{}\nx").unwrap();
        });
        let mut budget = ReadBudget::new(3);
        assert_eq!(
            read_with(&server, &mut budget).unwrap_err().code,
            "response_limit"
        );
        assert_eq!(budget.remaining(), 0);
        worker.join().unwrap();
    }

    #[test]
    fn complete_frame_without_eof_times_out_and_is_charged() {
        let server = Server::new();
        let worker = server.serve(|mut stream, _| {
            stream.write_all(b"{}\n").unwrap();
            thread::sleep(Duration::from_millis(100));
        });
        let mut budget = ReadBudget::new(3);
        let result = read(
            &server.path,
            ReadMethod::Snapshot,
            &json!({}),
            Instant::now() + Duration::from_millis(40),
            &mut budget,
            &NoCancel,
        );
        assert_eq!(result.unwrap_err().code, "timeout");
        assert_eq!(budget.remaining(), 0);
        worker.join().unwrap();
    }

    #[test]
    fn dribbling_response_does_not_restart_the_deadline() {
        let server = Server::new();
        let worker = server.serve(|mut stream, _| {
            for _ in 0..20 {
                if stream.write_all(b" ").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
        let mut budget = ReadBudget::new(1024);
        let started = Instant::now();
        let failure = read(
            &server.path,
            ReadMethod::Snapshot,
            &json!({}),
            started + Duration::from_millis(50),
            &mut budget,
            &NoCancel,
        )
        .unwrap_err();
        assert_eq!(failure.code, "timeout");
        assert!(budget.remaining() < 1024);
        assert!(started.elapsed() < Duration::from_millis(180));
        worker.join().unwrap();
    }

    #[test]
    fn annotation_ack_and_safe_rejection() {
        let server = Server::new();
        let worker = server.serve(|stream, request| {
            assert_eq!(request["method"], "pane.annotate");
            respond(stream, request, json!({"type":"ok"}));
        });
        write_with(&server).unwrap();
        worker.join().unwrap();
        let worker = server.serve(|mut stream, request| {
            writeln!(
                stream,
                "{}",
                json!({"id":request["id"], "error":{"code":"disabled", "message":"SECRET"}})
            )
            .unwrap();
        });
        let failure = write_with(&server).unwrap_err();
        assert_eq!(failure.delivery, Delivery::Rejected);
        assert_eq!(failure.code, "server_error:disabled");
        worker.join().unwrap();
    }

    #[test]
    fn acknowledgements_allow_huge_ignored_integers_at_both_extension_levels() {
        for rejected in [false, true] {
            let server = Server::new();
            let worker = server.serve(move |mut stream, request| {
                let huge = "9".repeat(4000);
                let body = if rejected {
                    format!(r#""error":{{"code":"disabled.Source-1","message":"PRIVATE PEER TEXT","extension":{{"integer":{huge}}}}}"#)
                } else {
                    format!(r#""result":{{"type":"ok","extension":{{"integer":{huge}}}}}"#)
                };
                writeln!(stream, r#"{{"id":{},{body},"extension":{huge}}}"#, request["id"]).unwrap();
            });
            let result = annotate(
                &server.path,
                &server.endpoint(),
                &mut json!({}),
                deadline(),
                16384,
                &NoCancel,
                &mut |_| Ok(()),
            );
            if rejected {
                assert_eq!(
                    result.unwrap_err(),
                    Failure {
                        code: "server_error:disabled.Source-1".into(),
                        delivery: Delivery::Rejected,
                        interrupted: false,
                    }
                );
            } else {
                result.unwrap();
            }
            worker.join().unwrap();
        }
    }

    #[test]
    fn huge_integers_in_known_ack_fields_remain_uncertain() {
        for field in ["id", "type", "code", "message"] {
            let server = Server::new();
            let worker = server.serve(move |mut stream, request| {
                let huge = "9".repeat(4000);
                let id = if field == "id" {
                    huge.clone()
                } else {
                    request["id"].to_string()
                };
                let body = match field {
                    "id" => r#""result":{"type":"ok"}"#.to_owned(),
                    "type" => format!(r#""result":{{"type":{huge}}}"#),
                    "code" => format!(r#""error":{{"code":{huge},"message":"PRIVATE PEER TEXT"}}"#),
                    "message" => format!(r#""error":{{"code":"disabled","message":{huge}}}"#),
                    _ => unreachable!(),
                };
                writeln!(stream, r#"{{"id":{id},{body}}}"#).unwrap();
            });
            let failure = annotate(
                &server.path,
                &server.endpoint(),
                &mut json!({}),
                deadline(),
                16384,
                &NoCancel,
                &mut |_| Ok(()),
            )
            .unwrap_err();
            assert_eq!(failure.delivery, Delivery::Uncertain, "{field}");
            assert!(!failure.code.contains("PRIVATE"));
            assert!(!failure.code.contains(&"9".repeat(64)));
            worker.join().unwrap();
        }
    }

    #[test]
    fn malformed_lost_and_uncorrelated_acknowledgements_are_uncertain() {
        for bytes in [
            b"{\n".as_slice(),
            b"",
            b"{}\n",
            b"{\"id\":\"wrong\",\"result\":{\"type\":\"ok\"}}\n",
            b"{}\nextra",
        ] {
            let server = Server::new();
            let worker = server.serve(move |mut stream, _| {
                stream.write_all(bytes).unwrap();
            });
            assert_eq!(
                write_with(&server).unwrap_err().delivery,
                Delivery::Uncertain
            );
            worker.join().unwrap();
        }
    }

    #[test]
    fn preparation_runs_twice_after_connect_and_never_sends_on_failure() {
        let server = Server::new();
        let listener = server.listener.try_clone().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).unwrap();
            assert!(bytes.is_empty());
        });
        let mut calls = 0;
        let result = annotate(
            &server.path,
            &server.endpoint(),
            &mut json!({}),
            deadline(),
            1024,
            &NoCancel,
            &mut |params| {
                calls += 1;
                params["ttl_ms"] = json!(3 - calls);
                if calls == 2 {
                    Err(Failure::new("expired"))
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(calls, 2);
        assert_eq!(result.unwrap_err().delivery, Delivery::Unsent);
        worker.join().unwrap();
    }

    #[test]
    fn only_final_prepared_params_are_sent() {
        let server = Server::new();
        let worker = server.serve(|stream, request| {
            assert_eq!(request["params"]["ttl_ms"], 1);
            respond(stream, request, json!({"type":"ok"}));
        });
        let mut calls = 0;
        annotate(
            &server.path,
            &server.endpoint(),
            &mut json!({"ttl_ms":3}),
            deadline(),
            1024,
            &NoCancel,
            &mut |params| {
                calls += 1;
                params["ttl_ms"] = json!(3 - calls);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(calls, 2);
        worker.join().unwrap();
    }

    #[test]
    fn endpoint_change_during_prepare_is_unsent() {
        let server = Server::new();
        let listener = server.listener.try_clone().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).unwrap();
            assert!(bytes.is_empty());
        });
        let mut replacement = None;
        let failure = annotate(
            &server.path,
            &server.endpoint(),
            &mut json!({}),
            deadline(),
            1024,
            &NoCancel,
            &mut |_| {
                if replacement.is_none() {
                    std::fs::remove_file(&server.path).unwrap();
                    replacement = Some(UnixListener::bind(&server.path).unwrap());
                    std::fs::set_permissions(&server.path, std::fs::Permissions::from_mode(0o600))
                        .unwrap();
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(failure.code, "endpoint_replaced");
        assert_eq!(failure.delivery, Delivery::Unsent);
        worker.join().unwrap();
    }

    struct Flag(Arc<AtomicBool>);
    impl Cancel for Flag {
        fn interrupted(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn cancellation_before_and_after_send_has_distinct_delivery() {
        let server = Server::new();
        let flag = Arc::new(AtomicBool::new(true));
        let cancel = Flag(flag.clone());
        let failure = annotate(
            &server.path,
            &server.endpoint(),
            &mut json!({}),
            deadline(),
            1024,
            &cancel,
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert_eq!(failure.delivery, Delivery::Unsent);
        assert!(failure.interrupted);
        flag.store(false, Ordering::SeqCst);
        let worker = server.serve(move |_stream, _| {
            flag.store(true, Ordering::SeqCst);
        });
        let failure = annotate(
            &server.path,
            &server.endpoint(),
            &mut json!({}),
            deadline(),
            1024,
            &cancel,
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert_eq!(failure.delivery, Delivery::Uncertain);
        assert!(failure.interrupted);
        worker.join().unwrap();
    }

    #[test]
    fn endpoints_reject_leaf_alias_and_public_permissions() {
        let server = Server::new();
        assert!(select(&format!("{}/.", server.path)).is_err());
        let alias = server._dir.path().join("alias.sock");
        symlink(&server.path, &alias).unwrap();
        assert_eq!(
            select(alias.to_str().unwrap()).unwrap_err().code,
            "unsafe_endpoint"
        );
        std::fs::set_permissions(&server.path, std::fs::Permissions::from_mode(0o660)).unwrap();
        assert_eq!(select(&server.path).unwrap_err().code, "unsafe_endpoint");
    }

    #[test]
    fn resolution_retains_missing_parent_tails_and_missing_leaf() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        symlink(&real, dir.path().join("alias")).unwrap();
        let canonical = std::fs::canonicalize(&real).unwrap();
        for (selected, expected) in [
            (
                format!("{}/alias/missing.sock", dir.path().display()),
                canonical.join("missing.sock"),
            ),
            (
                format!("{}/alias/missing/./nested//api.sock", dir.path().display()),
                canonical.join("missing/nested/api.sock"),
            ),
        ] {
            assert_eq!(resolve(&selected).unwrap(), expected.to_str().unwrap());
            assert_eq!(select(&selected).unwrap_err().code, "io_error");
        }
        assert!(!real.join("missing").exists());
        assert!(!real.join("missing.sock").exists());
    }

    #[test]
    fn resolution_is_metadata_only_and_never_resolves_the_leaf() {
        let server = Server::new();
        let root = server._dir.path();
        let canonical = std::fs::canonicalize(root).unwrap();
        std::fs::write(root.join("regular"), b"not a socket").unwrap();
        symlink(&server.path, root.join("leaf-alias")).unwrap();
        symlink("missing", root.join("dangling-leaf")).unwrap();
        symlink(root, root.join("parent-alias")).unwrap();
        for leaf in [
            "api.sock",
            "regular",
            "leaf-alias",
            "dangling-leaf",
            "missing",
            ".",
        ] {
            let path = format!("{}/parent-alias//./{leaf}", root.display());
            assert_eq!(
                resolve(&path).unwrap(),
                canonical.join(leaf).to_str().unwrap()
            );
        }
        for leaf in ["regular", "leaf-alias", "dangling-leaf", "."] {
            let path = format!("{}/parent-alias/{leaf}", root.display());
            assert_eq!(select(&path).unwrap_err().code, "unsafe_endpoint");
        }
        assert_eq!(
            select(&format!("{}/parent-alias/api.sock", root.display()))
                .unwrap()
                .identity()
                .unwrap(),
            server.endpoint()
        );
        server.listener.set_nonblocking(true).unwrap();
        assert_eq!(
            server.listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn resolution_expands_dangling_parent_aliases_and_normalizes_their_targets() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = std::fs::canonicalize(dir.path()).unwrap();
        symlink("missing/deep", dir.path().join("relative")).unwrap();
        symlink(canonical.join("missing/deep"), dir.path().join("absolute")).unwrap();
        symlink("missing/../other/.", dir.path().join("dots")).unwrap();
        for (alias, tail) in [
            ("relative", "missing/deep/tail/api.sock"),
            ("absolute", "missing/deep/tail/api.sock"),
            ("dots", "other/tail/api.sock"),
        ] {
            let selected = format!("{}/{alias}/tail/api.sock", dir.path().display());
            assert_eq!(
                resolve(&selected).unwrap(),
                canonical.join(tail).to_str().unwrap()
            );
            assert_eq!(select(&selected).unwrap_err().code, "io_error");
        }
    }

    #[test]
    fn resolution_keeps_nonstrict_parent_cycles_and_nondirectory_tails() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = std::fs::canonicalize(dir.path()).unwrap();
        symlink("cycle", dir.path().join("cycle")).unwrap();
        std::fs::write(dir.path().join("regular"), b"not a directory").unwrap();
        for tail in ["cycle/tail/api.sock", "regular/tail/api.sock"] {
            let selected = dir.path().join(tail);
            assert_eq!(
                resolve(selected.to_str().unwrap()).unwrap(),
                canonical.join(tail).to_str().unwrap()
            );
            assert_eq!(
                select(selected.to_str().unwrap()).unwrap_err().code,
                "io_error"
            );
        }
    }

    #[test]
    fn parent_alias_is_resolved_and_retargeting_is_fenced() {
        let server = Server::new();
        let other = Server::new();
        let dir = tempfile::tempdir().unwrap();
        let alias = dir.path().join("alias");
        symlink(server._dir.path(), &alias).unwrap();
        let selected = alias.join("api.sock");
        let endpoint = select(selected.to_str().unwrap()).unwrap();
        assert_eq!(endpoint.identity().unwrap(), server.endpoint());
        std::fs::remove_file(&alias).unwrap();
        symlink(other._dir.path(), &alias).unwrap();
        assert_eq!(
            crate::platform::observation_socket::fence(&endpoint)
                .unwrap_err()
                .code,
            "endpoint_replaced"
        );
    }

    #[test]
    fn shorter_address_works_in_both_alias_directions() {
        for long_resolved in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let short = dir.path().join("short");
            let long = dir.path().join("a".repeat(80)).join("b".repeat(80));
            std::fs::create_dir_all(&long).unwrap();
            let (selected, bound, resolved) = if long_resolved {
                symlink(&long, &short).unwrap();
                (
                    short.join("api.sock"),
                    short.join("api.sock"),
                    long.join("api.sock"),
                )
            } else {
                std::fs::create_dir(&short).unwrap();
                let alias = long.join("alias");
                symlink(&short, &alias).unwrap();
                (
                    alias.join("api.sock"),
                    short.join("api.sock"),
                    short.join("api.sock"),
                )
            };
            let listener = UnixListener::bind(&bound).unwrap();
            std::fs::set_permissions(&bound, std::fs::Permissions::from_mode(0o600)).unwrap();
            let worker = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                stream.write_all(b"{}\n").unwrap();
            });
            let reply = read(
                selected.to_str().unwrap(),
                ReadMethod::Snapshot,
                &json!({}),
                deadline(),
                &mut ReadBudget::new(3),
                &NoCancel,
            )
            .unwrap();
            assert_eq!(reply.endpoint.resolved, resolved.to_str().unwrap());
            worker.join().unwrap();
        }
    }

    #[test]
    fn cancellation_is_observed_while_polling_without_peer_data() {
        let server = Server::new();
        let flag = Arc::new(AtomicBool::new(false));
        let cancel = Flag(flag.clone());
        let worker = server.serve(move |_stream, _| {
            thread::sleep(Duration::from_millis(10));
            flag.store(true, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(70));
        });
        let failure = annotate(
            &server.path,
            &server.endpoint(),
            &mut json!({}),
            deadline(),
            1024,
            &cancel,
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert_eq!(failure.code, "interrupted");
        assert_eq!(failure.delivery, Delivery::Uncertain);
        assert!(failure.interrupted);
        worker.join().unwrap();
    }

    #[test]
    fn deadline_expiring_in_prepare_is_unsent() {
        let server = Server::new();
        let listener = server.listener.try_clone().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).unwrap();
            assert!(bytes.is_empty());
        });
        let failure = annotate(
            &server.path,
            &server.endpoint(),
            &mut json!({}),
            Instant::now() + Duration::from_millis(30),
            1024,
            &NoCancel,
            &mut |_| {
                thread::sleep(Duration::from_millis(40));
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(failure.code, "timeout");
        assert_eq!(failure.delivery, Delivery::Unsent);
        worker.join().unwrap();
    }

    #[test]
    fn partial_send_backpressure_obeys_deadline_and_is_uncertain() {
        let server = Server::new();
        let listener = server.listener.try_clone().unwrap();
        let (done, wait) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            let _ = wait.recv_timeout(Duration::from_secs(2));
        });
        let mut params = json!({"text": "a".repeat(512 * 1024)});
        let failure = annotate(
            &server.path,
            &server.endpoint(),
            &mut params,
            Instant::now() + Duration::from_millis(500),
            1024,
            &NoCancel,
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert_eq!(failure.code, "timeout");
        assert_eq!(failure.delivery, Delivery::Uncertain);
        done.send(()).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn frozen_mismatch_never_connects() {
        let server = Server::new();
        let mut identity = server.endpoint();
        identity["inode"] = json!(0);
        let failure = annotate(
            &server.path,
            &identity,
            &mut json!({}),
            deadline(),
            1024,
            &NoCancel,
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert_eq!(failure.code, "endpoint_replaced");
        assert_eq!(failure.delivery, Delivery::Unsent);
        server.listener.set_nonblocking(true).unwrap();
        assert!(server.listener.accept().is_err());
    }

    #[test]
    fn endpoint_change_after_send_is_uncertain() {
        let server = Server::new();
        let path = server.path.clone();
        let worker = server.serve(move |stream, request| {
            std::fs::remove_file(&path).unwrap();
            let _replacement = UnixListener::bind(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            respond(stream, request, json!({"type":"ok"}));
        });
        let failure = write_with(&server).unwrap_err();
        assert_eq!(failure.code, "endpoint_replaced");
        assert_eq!(failure.delivery, Delivery::Uncertain);
        worker.join().unwrap();
    }
}
