//! Native session workflow against private Rust Unix servers; no Python children.
#![cfg(unix)]
#[path = "support/interactive_session_server.rs"]
mod fixture;
use coven::interactive_sessions::{Operation, Request, arguments};
use fixture::{Server, ack, frame, inventory, snapshot};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;

fn request(
    workspace: &std::path::Path,
    executable: &std::path::Path,
    socket: &std::path::Path,
) -> Request {
    Request {
        workspace: workspace.into(),
        operation: Operation::Start,
        runtime: "djinn".into(),
        executable: executable.to_str().unwrap().into(),
        socket: socket.to_str().unwrap().into(),
        pane: "w1:p1".into(),
        expected_terminal: "terminal-1".into(),
        session: None,
        timeout: 1.0,
        execute: true,
    }
}

fn call(request: &Request, code: u8) -> Value {
    let output = arguments(request, true).unwrap();
    assert_eq!(output.exit_code, code, "{}", output.stdout);
    assert!(output.stdout.is_ascii());
    assert!(!output.stdout.ends_with('\n'));
    assert!(!output.stdout.contains("PRIVATE"));
    let report: Value = serde_json::from_str(&output.stdout).unwrap();
    for key in [
        "native_lifecycle",
        "native_worker_acceptance",
        "job_completion",
    ] {
        assert_eq!(report[key], "unestablished");
    }
    report
}

fn writes(server: &Server) -> usize {
    server
        .requests()
        .iter()
        .filter(|r| r["method"] == "pane.submit_command")
        .count()
}

// One test owns the process-wide interrupt handler; scenarios deliberately run sequentially.
#[test]
fn native_interactive_session_workflow() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let exe = fixture::executable(root.path());
    let mut preview = request(&workspace, &exe, std::path::Path::new("/absent/api.sock"));
    preview.execute = false;
    let report = call(&preview, 0);
    assert_eq!(report["outcome"], "preview");
    assert_eq!(report["authorization"], "none");
    assert_eq!(report["argv"], json!([exe.to_str().unwrap(), "--ui"]));
    assert!(!workspace.exists());
    preview.operation = Operation::Resume;
    preview.session = Some("literal ' folder; $(not executed) λ".into());
    let report = call(&preview, 0);
    assert_eq!(
        report["argv"][2],
        preview.session.as_ref().unwrap().as_str()
    );
    let human = arguments(&preview, false).unwrap().stdout;
    assert!(human.contains("'\"'\"'"));
    assert!(human.contains("Preview only;"));
    for timeout in [f64::NAN, f64::INFINITY, 0.0, -1.0, 30.01] {
        preview.timeout = timeout;
        assert!(arguments(&preview, true).is_err());
    }
    preview.timeout = 1.0;
    for selector in ["", " ", "--help", "bad\nselector"] {
        preview.session = Some(selector.into());
        assert!(arguments(&preview, true).is_err());
    }

    coven::init::arguments(
        &workspace,
        &coven::init::Options {
            blank: true,
            agents: "reviewer".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let agents = std::fs::read(workspace.join("state/agents.json")).unwrap();
    let tasks = std::fs::read(workspace.join("state/tasks.json")).unwrap();
    for operation in [Operation::Start, Operation::Resume] {
        let server = Server::new(|_, req| {
            if req["method"] == "session.snapshot" {
                snapshot(req, inventory())
            } else {
                frame(ack(req))
            }
        });
        let mut request = request(&workspace, &exe, &server.path);
        request.operation = operation;
        if operation == Operation::Resume {
            request.session = Some("folder ' $(literal); λ".into());
        }
        let report = call(&request, 0);
        assert_eq!(report["delivery"], "queued");
        assert_eq!(
            report["authorization"],
            "interactive_terminal_submission_only"
        );
        let requests = server.requests();
        assert_eq!(
            requests
                .iter()
                .map(|r| r["method"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "session.snapshot",
                "session.snapshot",
                "pane.submit_command"
            ]
        );
        assert_eq!(
            requests[2]["params"],
            json!({"pane_id":"w1:p1","expected_terminal_id":"terminal-1","argv":report["argv"]})
        );
        assert!(workspace.join("state/.runtime-transport.lock").exists());
    }
    assert_eq!(
        std::fs::read(workspace.join("state/agents.json")).unwrap(),
        agents
    );
    assert_eq!(
        std::fs::read(workspace.join("state/tasks.json")).unwrap(),
        tasks
    );

    for descriptor in [
        Value::Null,
        json!(false),
        json!({}),
        json!({"version":true}),
        json!({"version":"1"}),
        json!({"version":0}),
        json!({"version":2}),
        json!({"version":1.0}),
    ] {
        let server = Server::new(move |_, req| {
            let mut value = inventory();
            value["command_submission"] = descriptor.clone();
            snapshot(req, value)
        });
        let report = call(&request(&workspace, &exe, &server.path), 1);
        assert_eq!(
            report["diagnostics"],
            json!(["command_submission_unsupported"])
        );
        assert_eq!(writes(&server), 0);
    }
    for mode in [
        "capability",
        "terminal",
        "missing",
        "tab",
        "workspace",
        "executable",
    ] {
        let exe_path = exe.clone();
        let server = Server::new(move |n, req| {
            let mut value = inventory();
            if n == 2 {
                match mode {
                    "capability" => value["command_submission"] = Value::Null,
                    "terminal" => value["panes"][0]["terminal_id"] = json!("replacement"),
                    "missing" => value["panes"] = json!([]),
                    "tab" => value["panes"][0]["tab_id"] = json!("new-tab"),
                    "workspace" => value["panes"][0]["workspace_id"] = json!("new-workspace"),
                    "executable" => std::fs::write(&exe_path, b"changed metadata").unwrap(),
                    _ => unreachable!(),
                }
            }
            snapshot(req, value)
        });
        let report = call(&request(&workspace, &exe, &server.path), 1);
        assert_eq!(report["delivery"], "unsent", "{mode}");
        assert_eq!(writes(&server), 0, "{mode}");
    }
    for mode in [
        "lost",
        "partial",
        "id",
        "pane",
        "terminal",
        "argv",
        "type",
        "both",
        "trailing",
        "nonfinite",
        "duplicate",
        "bad_error",
    ] {
        let server = Server::new(move |_, req| {
            if req["method"] == "session.snapshot" {
                return snapshot(req, inventory());
            }
            let mut value = ack(req);
            match mode {
                "lost" => return vec![],
                "partial" => return b"{\"id\":".to_vec(),
                "id" => value["id"] = json!("wrong"),
                "pane" => value["result"]["pane_id"] = json!("wrong"),
                "terminal" => value["result"]["terminal_id"] = json!("wrong"),
                "argv" => value["result"]["argv"] = json!(["wrong"]),
                "type" => value["result"]["type"] = json!("wrong"),
                "both" => value["error"] = json!({"code":"busy","message":"PRIVATE"}),
                "bad_error" => {
                    value.as_object_mut().unwrap().remove("result");
                    value["error"] = json!({"code":"PRIVATE\nunsafe","message":"PRIVATE"});
                }
                "nonfinite" | "duplicate" => {
                    let mut bytes = serde_json::to_string(&value).unwrap();
                    bytes.pop();
                    bytes.push_str(if mode == "nonfinite" {
                        ",\"extension\":1e9999}\n"
                    } else {
                        ",\"extension\":1,\"extension\":2}\n"
                    });
                    return bytes.into_bytes();
                }
                _ => {}
            }
            let mut bytes = frame(value);
            if mode == "trailing" {
                bytes.extend_from_slice(b"extra\n");
            }
            bytes
        });
        let report = call(&request(&workspace, &exe, &server.path), 2);
        assert_eq!(report["delivery"], "uncertain", "{mode}");
        assert_eq!(writes(&server), 1, "{mode}");
    }
    let server = Server::new(|_, req| {
        if req["method"] == "session.snapshot" {
            snapshot(req, inventory())
        } else {
            frame(
                json!({"id":req["id"],"error":{"code":"pane_command_busy","message":"PRIVATE peer message"}}),
            )
        }
    });
    let report = call(&request(&workspace, &exe, &server.path), 1);
    assert_eq!(report["outcome"], "rejected");
    assert_eq!(
        report["diagnostics"],
        json!(["server_error:pane_command_busy"])
    );
    assert_eq!(writes(&server), 1);

    // Extensions are fully validated in the native profile, not narrowed through serde.
    let server = Server::new(|_, req| {
        let mut bytes = if req["method"] == "session.snapshot" {
            snapshot(req, inventory())
        } else {
            frame(ack(req))
        };
        bytes.truncate(bytes.len() - 2);
        bytes.extend_from_slice(b",\"extension\":184467440737095516160}\n");
        bytes
    });
    assert_eq!(
        call(&request(&workspace, &exe, &server.path), 0)["delivery"],
        "queued"
    );

    for after_send in [false, true] {
        let server = Server::new(move |_, req| {
            if after_send == (req["method"] == "pane.submit_command") {
                // Individually valid strings, but collectively over the shared response budget.
                let extension = vec!["x".repeat(4096); 260];
                if after_send {
                    let mut value = ack(req);
                    value["extension"] = json!(extension);
                    return frame(value);
                }
                let mut value = inventory();
                value["extension"] = json!(extension);
                return snapshot(req, value);
            }
            snapshot(req, inventory())
        });
        let report = call(
            &request(&workspace, &exe, &server.path),
            if after_send { 2 } else { 1 },
        );
        assert_eq!(report["diagnostics"], json!(["response_limit"]));
        assert_eq!(writes(&server), usize::from(after_send));
    }

    for after_send in [false, true] {
        let server = Server::new(move |_, req| {
            if after_send == (req["method"] == "pane.submit_command") {
                // Deliver to this fixture thread synchronously while the production
                // process-wide handler is installed by arguments().
                assert_eq!(unsafe { libc::raise(libc::SIGINT) }, 0);
            }
            if req["method"] == "session.snapshot" {
                snapshot(req, inventory())
            } else {
                frame(ack(req))
            }
        });
        let report = call(&request(&workspace, &exe, &server.path), 130);
        assert_eq!(report["interrupted"], true);
        assert_eq!(
            report["delivery"],
            if after_send { "uncertain" } else { "unsent" }
        );
        assert_eq!(writes(&server), usize::from(after_send));
    }

    let server = Server::new(|_, req| {
        if req["method"] == "session.snapshot" {
            let mut value = inventory();
            value["extension"] = json!(vec!["x".repeat(4096); 110]);
            snapshot(req, value)
        } else {
            let mut value = ack(req);
            value["extension"] = json!(vec!["x".repeat(4096); 40]);
            frame(value)
        }
    });
    let report = call(&request(&workspace, &exe, &server.path), 2);
    assert_eq!(report["diagnostics"], json!(["response_limit"]));
    assert_eq!(server.requests().len(), 3);

    let server = Server::new(|_, req| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        snapshot(req, inventory())
    });
    let mut shared_deadline = request(&workspace, &exe, &server.path);
    shared_deadline.timeout = 0.15;
    let report = call(&shared_deadline, 1);
    assert_eq!(report["diagnostics"], json!(["timeout"]));
    assert_eq!(server.requests().len(), 2);
    assert_eq!(writes(&server), 0);

    // A pathname replacement during a snapshot must not be adopted for subsequent reads.
    let socket_path = std::sync::Arc::new(std::sync::Mutex::new(None::<std::path::PathBuf>));
    let replace = socket_path.clone();
    let server = Server::new(move |n, req| {
        if n == 1 {
            let path = replace.lock().unwrap().clone().unwrap();
            std::fs::remove_file(&path).unwrap();
            let _replacement = std::os::unix::net::UnixListener::bind(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            return snapshot(req, inventory());
        }
        panic!("replaced endpoint must not be followed")
    });
    *socket_path.lock().unwrap() = Some(server.path.clone());
    let report = call(&request(&workspace, &exe, &server.path), 1);
    assert_eq!(report["delivery"], "unsent");
    assert_eq!(writes(&server), 0);

    for after_send in [false, true] {
        let server = Server::new(move |_, req| {
            if after_send == (req["method"] == "pane.submit_command") {
                std::thread::sleep(std::time::Duration::from_millis(180));
            }
            if req["method"] == "session.snapshot" {
                snapshot(req, inventory())
            } else {
                frame(ack(req))
            }
        });
        let mut request = request(&workspace, &exe, &server.path);
        request.timeout = 0.1;
        let report = call(&request, if after_send { 2 } else { 1 });
        assert_eq!(report["diagnostics"], json!(["timeout"]));
        assert_eq!(writes(&server), usize::from(after_send));
    }
    let server = Server::new(|_, req| snapshot(req, inventory()));
    std::fs::set_permissions(&server.path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let report = call(&request(&workspace, &exe, &server.path), 1);
    assert_eq!(report["diagnostics"], json!(["unsafe_endpoint"]));
    assert!(server.requests().is_empty());

    // Actual orphaned authority blocks before any socket request, even with valid metadata.
    std::fs::create_dir_all(workspace.join("state/actions")).unwrap();
    std::fs::write(workspace.join("state/actions/orphan.json"), b"{}\n").unwrap();
    assert!(arguments(&request(&workspace, &exe, &server.path), true).is_err());
    assert!(server.requests().is_empty());

    for invalid in [false, true] {
        let blocked = root
            .path()
            .join(if invalid { "invalid" } else { "enabled" });
        coven::init::arguments(
            &blocked,
            &coven::init::Options {
                blank: true,
                agents: "reviewer".into(),
                ..Default::default()
            },
        )
        .unwrap();
        if invalid {
            std::fs::write(blocked.join("state/runtime-launch-control.json"), b"{}\n").unwrap();
        } else {
            coven::actions::mutation::enable(&blocked, true).unwrap();
        }
        assert!(arguments(&request(&blocked, &exe, &server.path), true).is_err());
        assert!(server.requests().is_empty());
        let mut preview = request(&blocked, &exe, &server.path);
        preview.execute = false;
        assert_eq!(call(&preview, 0)["outcome"], "preview");
    }
}
