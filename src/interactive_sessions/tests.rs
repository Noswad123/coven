use super::*;

fn request() -> Request {
    Request {
        workspace: "/missing/control".into(),
        operation: Operation::Start,
        runtime: "djinn".into(),
        executable: "/missing/djinn".into(),
        socket: "/missing/socket".into(),
        pane: "w1:p1".into(),
        expected_terminal: "terminal-1".into(),
        session: None,
        session_kind: None,
        timeout: 3.0,
        execute: false,
    }
}

#[test]
fn canonical_preview_and_validation_are_independent_of_existing_paths() {
    let mut request = request();
    let report = Plan::new(&request).unwrap().report;
    assert_eq!(report["argv"], json!(["/missing/djinn", "--ui"]));
    assert_eq!(report["delivery"], "not_attempted");
    assert_eq!(report["limitations"], json!(LIMITATIONS));
    request.operation = Operation::Resume;
    request.session = Some("folder ' ; $(literal) λ".into());
    let plan = Plan::new(&request).unwrap();
    request.session = Some("changed after planning".into());
    assert_eq!(plan.report["argv"][2], "folder ' ; $(literal) λ");
    for value in [
        "",
        " ",
        "--help",
        "bad\rselector",
        "bad\u{85}selector",
        &"x".repeat(4097),
    ] {
        request.session = Some(value.into());
        assert!(Plan::new(&request).is_err(), "{value:?}");
    }
    request.session = Some("valid".into());
    for value in ["relative", "/a/../b", "/a/", "/bad\npath"] {
        request.executable = value.into();
        assert!(Plan::new(&request).is_err());
    }
    request.executable = "/missing/djinn".into();
    request.workspace = "/control/../other".into();
    assert!(Plan::new(&request).is_err());
    request.workspace = "/control".into();
    for value in [" ", "two words", "a\tb", "a\u{85}b", &"λ".repeat(2049)] {
        request.pane = value.into();
        assert!(Plan::new(&request).is_err());
    }
}

#[test]
fn native_ack_validates_ignored_extensions_without_narrowing_them() {
    let plan = Plan::new(&request()).unwrap();
    let prefix = r#"{"id":"id","result":{"type":"pane_command_submitted","pane_id":"w1:p1","terminal_id":"terminal-1","argv":["/missing/djinn","--ui"]}"#;
    for extension in [
        "18446744073709551616",
        "-18446744073709551617",
        "1.25",
        "{\"nested\":[null,true]}",
    ] {
        let payload = format!("{prefix},\"extension\":{extension}}}");
        acknowledgement(payload.as_bytes(), "id", &plan.report).unwrap();
    }
    for extension in [
        "NaN",
        "Infinity",
        "1e9999",
        "{\"duplicate\":1,\"duplicate\":2}",
    ] {
        let payload = format!("{prefix},\"extension\":{extension}}}");
        assert!(acknowledgement(payload.as_bytes(), "id", &plan.report).is_err());
    }
    for error in [
        json!({"code":"busy","message":"PRIVATE"}),
        json!({"code":"a.b_c-1","message":""}),
    ] {
        let bytes = serde_json::to_vec(&json!({"id":"id","error":error})).unwrap();
        let failure = acknowledgement(&bytes, "id", &plan.report).unwrap_err();
        assert_eq!(failure.delivery, Delivery::Rejected);
        assert!(!failure.code.contains("PRIVATE"));
        assert_eq!(
            acknowledgement(&bytes, "wrong-id", &plan.report)
                .unwrap_err()
                .delivery,
            Delivery::Unsent
        );
    }
    for error in [
        json!({"code":"bad\ncode","message":"PRIVATE"}),
        json!({"code":"","message":"PRIVATE"}),
        json!({"code":"busy","message":false}),
    ] {
        let bytes = serde_json::to_vec(&json!({"id":"id","error":error})).unwrap();
        assert_ne!(
            acknowledgement(&bytes, "id", &plan.report)
                .unwrap_err()
                .delivery,
            Delivery::Rejected
        );
    }
}

#[test]
fn interrupt_exit_preserves_recorded_queue_receipt() {
    let mut report = Plan::new(&request()).unwrap().report;
    for (outcome, delivery, code) in [
        ("preview", "not_attempted", 0),
        ("input_queued", "queued", 0),
        ("unavailable", "unsent", 1),
        ("rejected", "rejected", 1),
        ("uncertain", "uncertain", 2),
    ] {
        report["outcome"] = json!(outcome);
        report["delivery"] = json!(delivery);
        report.as_object_mut().unwrap().remove("interrupted");
        assert_eq!(render(&report, true).unwrap().exit_code, code);
        report["interrupted"] = json!(true);
        let rendered = render(&report, true).unwrap();
        assert_eq!(rendered.exit_code, 130);
        let receipt: Value = serde_json::from_str(&rendered.stdout).unwrap();
        assert_eq!(receipt["outcome"], outcome);
        assert_eq!(receipt["delivery"], delivery);
    }
}

#[cfg(unix)]
use crate::interactive_session_fixture as fixture;

#[cfg(unix)]
#[test]
fn gate_is_required_and_held_without_workspace_lock_during_io() {
    use fixture::{Server, ack, frame, inventory, snapshot};
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    crate::init::arguments(
        &root,
        &crate::init::Options {
            blank: true,
            agents: "reviewer".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let inspected = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = inspected.clone();
    let workspace = root.clone();
    let server = Server::new(move |_, req| {
        assert!(TransportGuard::exclusive(&workspace, Duration::ZERO).is_err());
        let _workspace_lock =
            crate::storage::WorkspaceGuard::acquire(&workspace, Duration::ZERO).unwrap();
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if req["method"] == "session.snapshot" {
            snapshot(req, inventory())
        } else {
            frame(ack(req))
        }
    });
    let mut request = request();
    request.workspace = root.clone();
    request.socket = server.path.to_str().unwrap().into();
    request.executable = fixture::executable(temporary.path())
        .to_str()
        .unwrap()
        .into();
    let other = temporary.path().join("other");
    std::fs::create_dir_all(other.join("state")).unwrap();
    let wrong_gate = TransportGuard::shared(&other, Duration::ZERO).unwrap();
    assert!(execute_with_guard(&request, &wrong_gate, &wire::NoCancel, &mut || Ok(())).is_err());
    assert!(server.requests().is_empty());
    let gate = crate::session_support::admit(&root).unwrap();
    assert_eq!(
        execute_with_guard(&request, &gate, &wire::NoCancel, &mut || Ok(())).unwrap()["outcome"],
        "input_queued"
    );
    assert_eq!(inspected.load(std::sync::atomic::Ordering::SeqCst), 3);
    drop(gate);
    assert!(TransportGuard::exclusive(&root, Duration::ZERO).is_ok());
}

#[cfg(unix)]
#[test]
fn cancellation_before_io_and_after_possible_send_are_not_replayed() {
    use fixture::{Server, ack, frame, inventory, snapshot};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    struct Flag(Arc<AtomicBool>);
    impl Cancel for Flag {
        fn interrupted(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let mut request = request();
    request.executable = fixture::executable(temp.path()).to_str().unwrap().into();
    for during_send in [false, true] {
        let flag = Arc::new(AtomicBool::new(!during_send));
        let trigger = flag.clone();
        let server = Server::new(move |_, req| {
            if req["method"] == "session.snapshot" {
                snapshot(req, inventory())
            } else {
                trigger.store(true, Ordering::SeqCst);
                frame(ack(req))
            }
        });
        request.socket = server.path.to_str().unwrap().into();
        let plan = Plan::new(&request).unwrap();
        let endpoint = wire::select(&request.socket).unwrap();
        let failure = submit(
            &plan,
            &endpoint,
            Instant::now() + Duration::from_secs(1),
            &mut ReadBudget::new(1_048_576),
            &Flag(flag),
            &mut || Ok(()),
        )
        .unwrap_err();
        assert!(failure.interrupted);
        assert_eq!(
            failure.delivery,
            if during_send {
                Delivery::Uncertain
            } else {
                Delivery::Unsent
            }
        );
        assert_eq!(server.requests().len(), usize::from(during_send));
    }
}

#[cfg(unix)]
#[test]
fn failure_at_first_send_is_conservatively_uncertain_even_without_a_complete_frame() {
    use std::cell::Cell;
    struct FirstSend(Cell<usize>);
    impl Cancel for FirstSend {
        fn interrupted(&self) -> bool {
            let checks = self.0.get() + 1;
            self.0.set(checks);
            // Immediate local connect: submit check, three connect checks, two
            // pre-send checks, then Socket::send's first check after possible_send.
            checks >= 7
        }
    }
    let server = fixture::Server::new(|_, _| panic!("no complete command may arrive"));
    let mut request = request();
    request.socket = server.path.to_str().unwrap().into();
    let plan = Plan::new(&request).unwrap();
    let endpoint = wire::select(&request.socket).unwrap();
    let failure = submit(
        &plan,
        &endpoint,
        Instant::now() + Duration::from_secs(1),
        &mut ReadBudget::new(1_048_576),
        &FirstSend(Cell::new(0)),
        &mut || Ok(()),
    )
    .unwrap_err();
    assert!(failure.interrupted);
    assert_eq!(failure.delivery, Delivery::Uncertain);
    assert!(server.requests().is_empty());
}

#[cfg(unix)]
#[test]
fn metadata_follows_symlinks_and_checks_execute_permission_without_launching() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let temp = tempfile::tempdir().unwrap();
    let path = fixture::executable(temp.path());
    let alias = temp.path().join("alias");
    symlink(&path, &alias).unwrap();
    let first = executable(alias.to_str().unwrap()).unwrap();
    assert_eq!(first, executable(path.to_str().unwrap()).unwrap());
    std::fs::write(&path, b"changed fixture metadata").unwrap();
    assert_ne!(first, executable(alias.to_str().unwrap()).unwrap());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        executable(alias.to_str().unwrap()).unwrap_err().code,
        "executable_unavailable"
    );
    assert!(executable(temp.path().to_str().unwrap()).is_err());
}

#[test]
fn all_paired_adapters_have_exact_start_and_literal_resume_arguments() {
    let selector = "  saved; $(literal) 'quoted'  ";
    for (runtime, flag, joined) in [
        ("djinn", "--session", false),
        ("claude", "--resume", false),
        ("codex", "resume", false),
        ("copilot", "--resume=", true),
        ("devin", "--resume", false),
        ("droid", "--resume", false),
        ("kimi", "--session", false),
        ("mastracode", "--thread", false),
        ("pi", "--session", false),
        ("omp", "--resume=", true),
        ("hermes", "--resume", false),
        ("opencode", "--session", false),
        ("qodercli", "--resume", false),
        ("kilo", "--session", false),
        ("cursor", "--resume", false),
        ("grok", "--resume", false),
    ] {
        let mut request = request();
        request.runtime = runtime.into();
        let start = preview(&request).unwrap();
        assert_eq!(
            start["argv"],
            if runtime == "djinn" {
                json!([request.executable, "--ui"])
            } else {
                json!([request.executable])
            },
            "{runtime}"
        );
        assert_eq!(start["native_cli_compatibility"], "unverified");
        assert_eq!(start["argv_evidence"], "characterized_legacy_policy");
        request.operation = Operation::Resume;
        request.session = Some(selector.into());
        let resume = preview(&request).unwrap();
        assert_eq!(resume["session_kind"], "id");
        assert_eq!(
            resume["argv"],
            if joined {
                json!([request.executable, format!("{flag}{selector}")])
            } else {
                json!([request.executable, flag, selector])
            },
            "{runtime}"
        );
        request.session_kind = Some(SessionKind::Id);
        assert_eq!(preview(&request).unwrap(), resume);
        let payload = serde_json::to_vec(&json!({"id":"id","result":{
            "type":"pane_command_submitted","pane_id":request.pane,
            "terminal_id":request.expected_terminal,"argv":resume["argv"]}}))
        .unwrap();
        acknowledgement(&payload, "id", &resume).unwrap();
        let mut wrong = resume.clone();
        wrong["argv"][0] = json!("/different");
        assert!(acknowledgement(&payload, "id", &wrong).is_err());
    }
}

#[test]
fn selector_kinds_bounds_and_start_exclusions() {
    let mut request = request();
    request.session_kind = Some(SessionKind::Id);
    assert!(preview(&request).is_err());
    request.session_kind = None;
    request.session = Some("saved".into());
    assert!(preview(&request).is_err());
    request.operation = Operation::Resume;
    for selector in ["", " ", "  --help", "bad\n", "bad\u{85}", &"x".repeat(513)] {
        request.session = Some(selector.into());
        assert!(preview(&request).is_err(), "{selector:?}");
    }
    request.session = Some("x".repeat(512));
    preview(&request).unwrap();
    request.session_kind = Some(SessionKind::Path);
    for runtime in ["pi", "omp", "djinn", "claude", "copilot", "cursor"] {
        request.runtime = runtime.into();
        request.session = Some(format!("/{}", "x".repeat(4095)));
        if matches!(runtime, "pi" | "omp") {
            let report = preview(&request).unwrap();
            assert_eq!(report["session_kind"], "path");
            assert_eq!(
                report["argv"],
                if runtime == "pi" {
                    json!([request.executable, "--session", request.session])
                } else {
                    json!([
                        request.executable,
                        format!("--resume={}", request.session.as_ref().unwrap())
                    ])
                }
            );
            request.session.as_mut().unwrap().push('x');
            assert!(preview(&request).is_err());
            for path in ["relative", " /absolute", "  --option"] {
                request.session = Some(path.into());
                assert!(preview(&request).is_err());
            }
        } else {
            assert!(preview(&request).is_err());
        }
    }
}

#[cfg(unix)]
#[test]
fn before_send_runs_once_after_preflight_and_failure_prevents_mutation() {
    use fixture::{Server, ack, frame, inventory, snapshot};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    crate::init::arguments(
        &root,
        &crate::init::Options {
            blank: true,
            agents: "reviewer".into(),
            ..Default::default()
        },
    )
    .unwrap();
    for mode in ["success", "callback_failure", "preflight_failure"] {
        let server = Server::new(move |_, req| {
            if req["method"] == "session.snapshot" {
                let mut value = inventory();
                if mode == "preflight_failure" {
                    value["panes"][0]["terminal_id"] = json!("changed");
                }
                snapshot(req, value)
            } else {
                frame(ack(req))
            }
        });
        let mut request = request();
        request.workspace = root.clone();
        request.runtime = "copilot".into();
        request.operation = Operation::Resume;
        request.session = Some("literal; $(data)".into());
        request.executable = fixture::executable(temp.path()).to_str().unwrap().into();
        request.socket = server.path.to_str().unwrap().into();
        let gate = crate::session_support::admit(&root).unwrap();
        let mut calls = 0;
        let report = execute_with_guard(&request, &gate, &wire::NoCancel, &mut || {
            calls += 1;
            assert_eq!(server.requests().len(), 2);
            let _workspace_lock =
                crate::storage::WorkspaceGuard::acquire(&root, Duration::ZERO).unwrap();
            if mode == "callback_failure" {
                Err(Error("journal_failed"))
            } else {
                Ok(())
            }
        })
        .unwrap();
        assert_eq!(calls, usize::from(mode != "preflight_failure"));
        assert_eq!(
            server
                .requests()
                .iter()
                .filter(|req| req["method"] == "pane.submit_command")
                .count(),
            usize::from(mode == "success")
        );
        assert_eq!(
            report["delivery"],
            match mode {
                "success" => "queued",
                "callback_failure" => "not_attempted",
                _ => "unsent",
            }
        );
        if mode == "callback_failure" {
            assert_eq!(report["diagnostics"], json!(["before_send_failed"]));
        }
    }
}

#[cfg(unix)]
#[test]
fn successful_callback_cancellation_or_deadline_exhaustion_is_known_unsent() {
    use fixture::{Server, inventory, snapshot};
    use std::cell::Cell;
    struct Flag(Cell<bool>);
    impl Cancel for Flag {
        fn interrupted(&self) -> bool {
            self.0.get()
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    crate::init::arguments(
        &root,
        &crate::init::Options {
            blank: true,
            agents: "reviewer".into(),
            ..Default::default()
        },
    )
    .unwrap();
    for cancel_after_callback in [true, false] {
        let server = Server::new(|_, req| {
            assert_eq!(req["method"], "session.snapshot");
            snapshot(req, inventory())
        });
        let mut request = request();
        request.workspace = root.clone();
        request.executable = fixture::executable(temp.path()).to_str().unwrap().into();
        request.socket = server.path.to_str().unwrap().into();
        request.timeout = 1.0;
        let gate = crate::session_support::admit(&root).unwrap();
        let cancel = Flag(Cell::new(false));
        let mut calls = 0;
        let report = execute_with_guard(&request, &gate, &cancel, &mut || {
            calls += 1;
            assert_eq!(server.requests().len(), 2);
            if cancel_after_callback {
                cancel.0.set(true);
            } else {
                // Sleeping a full budget after preflight guarantees deadline expiry.
                std::thread::sleep(Duration::from_secs_f64(request.timeout));
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(report["outcome"], "unavailable");
        assert_eq!(report["delivery"], "unsent");
        assert_eq!(
            report["diagnostics"],
            json!([if cancel_after_callback {
                "interrupted"
            } else {
                "timeout"
            }])
        );
        assert_eq!(server.requests().len(), 2);
        if cancel_after_callback {
            assert_eq!(report["interrupted"], true);
        }
    }
}
