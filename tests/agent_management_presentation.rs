#![cfg(any(target_os = "linux", target_os = "macos"))]
//! Private fake Kit plus receiver child: no live apps or process-global test signals.
#[path = "support/physical_temp.rs"]
mod physical_temp;
use coven::{
    agent_management::{self, InventoryRequest, ReportRequest, ServeRequest},
    agent_management_presentation::{self, PresentationRequest},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[test]
fn receiver_child() {
    let Ok(root) = std::env::var("COVEN_AGENT_VIEW_TEST_ROOT") else {
        return;
    };
    let output = agent_management_presentation::serve_arguments(
        &ServeRequest {
            workspace: root.clone().into(),
            socket: format!("{root}/receiver.sock"),
            duration: 2,
        },
        &PresentationRequest {
            socket: format!("{root}/kit.sock"),
            source: "queueSource2".into(),
        },
        true,
    )
    .unwrap();
    assert_eq!(output.exit_code, 0);
    fs::write(format!("{root}/result.json"), output.stdout).unwrap();
}

struct Receiver(Child);

#[test]
fn public_projection_receiver_child() {
    let Ok(root) = std::env::var("COVEN_PUBLIC_PROJECTION_TEST_ROOT") else {
        return;
    };
    let model = Arc::new(Mutex::new(Value::Null));
    agent_management::run_with_projection(
        &agent_management::RunRequest {
            workspace: root.clone().into(),
            socket: format!("{root}/native.sock"),
        },
        model.clone(),
    )
    .unwrap();
    fs::write(
        format!("{root}/public-projection.json"),
        serde_json::to_vec(&*model.lock().unwrap()).unwrap(),
    )
    .unwrap();
}

#[test]
fn exported_run_projection_retains_stale_and_lifecycle_djinn_without_private_fields() {
    let root = physical_temp::tempdir();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.path().join("coven.json"), "{}").unwrap();
    let path = root.path().join("native.sock");
    let mut receiver = Receiver(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "public_projection_receiver_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("COVEN_PUBLIC_PROJECTION_TEST_ROOT", root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let ready = Instant::now() + Duration::from_secs(5);
    while agent_management::observations_arguments(path.to_str().unwrap(), true).is_err() {
        assert!(
            Instant::now() < ready,
            "isolated public receiver was not ready"
        );
        thread::sleep(Duration::from_millis(10));
    }
    for (sequence, sid, event) in [
        (1, "stale", "session.status"),
        (2, "lifecycle", "session.updated"),
    ] {
        let mut p = json!({"contract":"djinn.native-events.v1",
            "instance_id":"12345678-1234-1234-1234-123456789abc", "session_id":sid,"sequence":sequence,"event":event});
        if event == "session.status" {
            p["status"] = json!("busy");
        }
        agent_management::native_report_payload(
            path.to_str().unwrap(),
            p.to_string().as_bytes(),
            true,
        )
        .unwrap();
    }
    let end = Instant::now() + Duration::from_secs(33);
    let mut rpc: Value = loop {
        let value: Value = serde_json::from_str(
            &agent_management::observations_arguments(path.to_str().unwrap(), true)
                .unwrap()
                .stdout,
        )
        .unwrap();
        if value["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["freshness"] == "expired")
        {
            break value;
        }
        assert!(Instant::now() < end, "native evidence did not expire");
        thread::sleep(Duration::from_millis(50));
    };
    // Let the receiver publish the post-RPC model before its explicitly owned stdin closes.
    thread::sleep(Duration::from_millis(100));
    drop(receiver.0.stdin.take());
    receiver.finish();
    let mut model: Value =
        serde_json::from_slice(&fs::read(root.path().join("public-projection.json")).unwrap())
            .unwrap();
    assert_eq!(model.as_object().unwrap().len(), 6);
    assert!(
        model
            .as_object()
            .unwrap()
            .keys()
            .all(|key| !key.starts_with("_publisher"))
    );
    assert_eq!(model["observations"].as_array().unwrap().len(), 2);
    for view in [&mut model, &mut rpc] {
        for row in view["observations"].as_array_mut().unwrap() {
            if row["freshness"] == "expired" {
                assert!(row["age_ms"].as_u64().unwrap() >= 30_000);
            }
            // Independent reads have different monotonic ages, but identical public facts/IDs.
            row["age_ms"] = Value::Null;
        }
    }
    assert_eq!(model, rpc);
    assert!(!path.exists());
}

fn native_call(path: &std::path::Path, contract: &str, method: &str, params: Value) -> Value {
    let mut stream = UnixStream::connect(path).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let request = json!({"id":"a".repeat(32),"contract":contract,"method":method,"params":params});
    writeln!(stream, "{request}").unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(reply["type"], "result", "{reply}");
    reply["result"].clone()
}

fn wait_name(kit: &Kit, expected: &str) -> Value {
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        let items = kit.providers.lock().unwrap()["queueSource2"]["items"].clone();
        if let Some(row) = items
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["label"] == expected))
        {
            return row.clone();
        }
        assert!(
            Instant::now() < end,
            "name not published: {expected}; {items}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn normal_names_follow_manual_pane_rename_movement_and_fail_closed_without_refreshing_facts() {
    let root = physical_temp::tempdir();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.path().join("coven.json"), "{}").unwrap();
    let kit = Kit::start(root.path(), "names");
    let generation = "12345678-1234-1234-1234-123456789abc";
    let pane = json!({"pane_id":"pane","terminal_id":"terminal","tab_id":"tab","workspace_id":"workspace",
        "label":"Review pane","title":"PRIVATE-OSC","agent_name":"PRIVATE-RETIRED"});
    let inspection = json!({"type":"terminal_inspected","terminal_id":"terminal","runtime_generation":generation,
        "availability":"live","pane_id":"pane","tab_id":"tab","workspace_id":"workspace"});
    kit.providers.lock().unwrap()["_naming"] = json!({"panes":[pane],"inspection":inspection,"hung":false,
            "workspaces":[{"workspace_id":"workspace","label":"Coven",
                "worktree":{"repo_name":"Coven","path":"/PRIVATE"},"git_branch":"feature/ui"}],
            "tabs":[{"workspace_id":"workspace","tab_id":"tab"}]});
    let path = root.path().join("native.sock");
    let mut receiver = Receiver(
        Command::new(env!("CARGO_BIN_EXE_coven"))
            .args(["agent", "run"])
            .arg(root.path())
            .arg("--socket")
            .arg(&path)
            .arg("--kitsune-socket")
            .arg(root.path().join("kit.sock"))
            .args(["--source", "queueSource2", "--json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let end = Instant::now() + Duration::from_secs(5);
    let initial = loop {
        if let Ok(output) = agent_management::observations_arguments(path.to_str().unwrap(), true) {
            break serde_json::from_str::<Value>(&output.stdout).unwrap();
        }
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(5));
    };
    agent_management::native_report_payload(path.to_str().unwrap(), json!({"contract":"djinn.native-events.v1",
        "instance_id":generation,"session_id":"PRIVATE-SESSION","sequence":1,"event":"session.status","status":"busy"})
        .to_string().as_bytes(), true).unwrap();
    let default = wait_name(&kit, "Djinn");
    let id = default["id"]
        .as_str()
        .unwrap()
        .strip_prefix("observation:")
        .unwrap();
    let metadata = fs::metadata(root.path().join("kit.sock")).unwrap();
    let target = json!({"socket":root.path().join("kit.sock").canonicalize().unwrap(),"device":metadata.dev(),
        "inode":metadata.ino(),"terminal_id":"terminal","runtime_generation":generation});
    let asserted = native_call(
        &path,
        "coven.observation-control.v1",
        "association.assert",
        json!({"observation_id":id,"expected_epoch":initial["server_epoch"],"expected_revision":null,
            "provenance":"user_asserted","target":target,"apply":true}),
    );
    let named = wait_name(&kit, "Review pane");
    assert_eq!(named["id"], default["id"]);
    assert_eq!(named["presentation"]["label"], "");
    assert_eq!(named["presentation"]["status"], "running");
    assert_eq!(
        named["information"]["context"],
        json!(["Kit project: Coven", "Kit branch: feature/ui"])
    );
    // Model the generic snapshot after pane.rename and a same-runtime move.
    {
        let mut state = kit.providers.lock().unwrap();
        state["_naming"]["panes"][0]["label"] = json!("\u{65e5}\u{672c}\u{8a9e} renamed");
        state["_naming"]["panes"][0]["pane_id"] = json!("moved");
        state["_naming"]["inspection"]["pane_id"] = json!("moved");
    }
    let renamed = wait_name(&kit, "\u{65e5}\u{672c}\u{8a9e} renamed");
    assert_eq!(renamed["id"], named["id"]);
    // Selection may add its existing read-only annotation; facts are otherwise unchanged.
    assert_eq!(
        renamed["detail"]
            .as_str()
            .unwrap()
            .split("; selected")
            .next(),
        named["detail"].as_str().unwrap().split("; selected").next()
    );
    for invalid in [
        Value::Null,
        json!(""),
        json!("\u{1b}[31munsafe"),
        json!("x".repeat(161)),
    ] {
        kit.providers.lock().unwrap()["_naming"]["panes"][0]["label"] = invalid;
        assert_eq!(wait_name(&kit, "Djinn")["id"], named["id"]);
        kit.providers.lock().unwrap()["_naming"]["panes"][0]["label"] = json!("Review pane");
        wait_name(&kit, "Review pane");
    }
    kit.providers.lock().unwrap()["_naming"]["inspection"]["runtime_generation"] =
        json!("restarted");
    assert_eq!(
        wait_name(&kit, "Djinn")["information"]["context"],
        json!(["No qualified workspace context."])
    );
    kit.providers.lock().unwrap()["_naming"]["inspection"]["runtime_generation"] =
        json!(generation);
    wait_name(&kit, "Review pane");
    kit.providers.lock().unwrap()["_naming"]["workspaces"][0]
        .as_object_mut()
        .unwrap()
        .remove("git_branch");
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        let row = wait_name(&kit, "Review pane");
        if row["information"]["context"][1] == "Kit branch: unavailable" {
            break;
        }
        assert!(
            Instant::now() < end,
            "old branch survived a successful refresh"
        );
        thread::sleep(Duration::from_millis(10));
    }
    kit.providers.lock().unwrap()["_naming"]["tabs"][0]["workspace_id"] = json!("wrong-workspace");
    assert_eq!(
        wait_name(&kit, "Djinn")["information"]["context"],
        json!(["No qualified workspace context."])
    );
    kit.providers.lock().unwrap()["_naming"]["tabs"][0]["workspace_id"] = json!("workspace");
    wait_name(&kit, "Review pane");
    let before: Value = serde_json::from_str(
        &agent_management::observations_arguments(path.to_str().unwrap(), true)
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert!(before.get("_publisher_names").is_none());
    assert!(!before.to_string().contains("Review pane"));
    let removed = native_call(
        &path,
        "coven.observation-control.v1",
        "association.remove",
        json!({"observation_id":id,
        "expected_epoch":initial["server_epoch"],"expected_revision":asserted["association_revision"],
        "provenance":"user_asserted","apply":true}),
    );
    wait_name(&kit, "Djinn");
    // A foreign socket must not even be contacted. Binding a listener makes any
    // accidental connection observable without touching a live endpoint.
    let foreign_listener = UnixListener::bind(root.path().join("foreign.sock")).unwrap();
    foreign_listener.set_nonblocking(true).unwrap();
    let mut foreign = target.clone();
    foreign["socket"] = json!(root.path().join("foreign.sock"));
    let foreign_asserted = native_call(
        &path,
        "coven.observation-control.v1",
        "association.assert",
        json!({"observation_id":id,
        "expected_epoch":initial["server_epoch"],"expected_revision":removed["association_revision"],
        "provenance":"user_asserted","target":foreign,"apply":true}),
    );
    thread::sleep(Duration::from_millis(600));
    assert_eq!(
        foreign_listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    wait_name(&kit, "Djinn");
    let local_asserted = native_call(
        &path,
        "coven.observation-control.v1",
        "association.assert",
        json!({"observation_id":id,
        "expected_epoch":initial["server_epoch"],"expected_revision":foreign_asserted["association_revision"],
        "provenance":"user_asserted","target":target,"apply":true}),
    );
    wait_name(&kit, "Review pane");
    native_call(
        &path,
        "coven.observation-control.v1",
        "association.remove",
        json!({"observation_id":id,
        "expected_epoch":initial["server_epoch"],"expected_revision":local_asserted["association_revision"],
        "provenance":"user_asserted","apply":true}),
    );
    let attachment = json!({"expected_epoch":initial["server_epoch"],"client_id":"12345678-1234-1234-1234-123456789abd",
        "revision":1,"operation":"attach","instance_id":generation,"session_id":"PRIVATE-SESSION",
        "scope":"local_full_tui_root","target":target});
    native_call(
        &path,
        "coven.client-attachments.v1",
        "report",
        attachment.clone(),
    );
    wait_name(&kit, "Review pane");
    let expiry = Instant::now() + Duration::from_secs(7);
    loop {
        if kit.providers.lock().unwrap()["queueSource2"]["items"][0]["label"] == "Djinn" {
            break;
        }
        assert!(
            Instant::now() < expiry,
            "publisher extended the producer display lease"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let mut renewed = attachment;
    renewed["revision"] = json!(2);
    native_call(&path, "coven.client-attachments.v1", "report", renewed);
    wait_name(&kit, "Review pane");
    let serial = kit
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c["method"] == "terminal.inspect")
        .count();
    kit.providers.lock().unwrap()["_naming"]["hung"] = json!(true);
    let end = Instant::now() + Duration::from_secs(2);
    while kit
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c["method"] == "terminal.inspect")
        .count()
        == serial
    {
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(5));
    }
    let started = Instant::now();
    let after: Value = serde_json::from_str(
        &agent_management::observations_arguments(path.to_str().unwrap(), true)
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "hung Kit starved receiver admission"
    );
    assert_eq!(
        before["observations"][0]["reported_state"],
        after["observations"][0]["reported_state"]
    );
    assert!(
        after["observations"][0]["age_ms"].as_u64().unwrap()
            >= before["observations"][0]["age_ms"].as_u64().unwrap()
    );
    drop(receiver.0.stdin.take());
    receiver.finish();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "naming IO prevented bounded shutdown"
    );
    let calls = kit.calls.lock().unwrap();
    assert!(calls.iter().all(|c| matches!(
        c["method"].as_str(),
        Some(
            "session.snapshot"
                | "terminal.inspect"
                | "integration.view.open"
                | "integration.view.replace"
                | "integration.view.poll"
                | "integration.view.close"
        )
    )));
    assert!(
        !calls
            .iter()
            .filter(|c| c["method"] == "integration.view.replace")
            .any(|c| c.to_string().contains("PRIVATE"))
    );
}

#[test]
fn observer_read_storm_keeps_live_hosted_unknown_available_without_renewing_evidence_or_leases() {
    let root = physical_temp::tempdir();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.path().join("coven.json"), "{}").unwrap();
    let kit = Kit::start(root.path(), "names");
    let generation = "12345678-1234-1234-1234-123456789abc";
    kit.providers.lock().unwrap()["_naming"] = json!({"hung":false,"delay_ms":150,
        "panes":[{"pane_id":"pane","terminal_id":"terminal","tab_id":"tab","workspace_id":"workspace","label":"SampleA"}],
        "inspection":{"type":"terminal_inspected","terminal_id":"terminal","runtime_generation":generation,
            "availability":"live","pane_id":"pane","tab_id":"tab","workspace_id":"workspace"},
        "workspaces":[{"workspace_id":"workspace","label":"Coven","git_branch":"feature/ui"}],
        "tabs":[{"workspace_id":"workspace","tab_id":"tab"}]});
    let path = root.path().join("native.sock");
    let mut receiver = Receiver(
        Command::new(env!("CARGO_BIN_EXE_coven"))
            .args(["agent", "run"])
            .arg(root.path())
            .arg("--socket")
            .arg(&path)
            .arg("--kitsune-socket")
            .arg(root.path().join("kit.sock"))
            .args(["--source", "queueSource2", "--json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let end = Instant::now() + Duration::from_secs(5);
    let initial: Value = loop {
        if let Ok(output) = agent_management::observations_arguments(path.to_str().unwrap(), true) {
            break serde_json::from_str(&output.stdout).unwrap();
        }
        assert!(Instant::now() < end, "receiver did not become ready");
        assert!(
            receiver.0.try_wait().unwrap().is_none(),
            "receiver exited before readiness"
        );
        thread::sleep(Duration::from_millis(5));
    };
    let reported = Instant::now();
    agent_management::native_report_payload(path.to_str().unwrap(), json!({"contract":"djinn.native-events.v1",
        "instance_id":generation,"session_id":"PRIVATE-STORM","sequence":1,"event":"session.status","status":"busy"})
        .to_string().as_bytes(), true).unwrap();
    let metadata = fs::metadata(root.path().join("kit.sock")).unwrap();
    let mut attachment = json!({"expected_epoch":initial["server_epoch"],
        "client_id":"12345678-1234-1234-1234-123456789abd","revision":1,"operation":"attach",
        "instance_id":generation,"session_id":"PRIVATE-STORM","scope":"local_full_tui_root",
        "target":{"socket":root.path().join("kit.sock").canonicalize().unwrap(),"device":metadata.dev(),
            "inode":metadata.ino(),"terminal_id":"terminal","runtime_generation":generation}});
    native_call(
        &path,
        "coven.client-attachments.v1",
        "report",
        attachment.clone(),
    );
    let named = wait_name(&kit, "SampleA");
    let mut heartbeat = Instant::now();
    let mut revision = 1;
    let end = reported + Duration::from_secs(34);
    // Model a failed native query: no further activity facts are supplied. Only
    // the independently live host attachment is renewed, never activity age.
    loop {
        if heartbeat.elapsed() >= Duration::from_secs(1) {
            revision += 1;
            attachment["revision"] = json!(revision);
            native_call(
                &path,
                "coven.client-attachments.v1",
                "report",
                attachment.clone(),
            );
            heartbeat = Instant::now();
        }
        let observed: Value = serde_json::from_str(
            &agent_management::observations_arguments(path.to_str().unwrap(), true)
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert_eq!(observed.as_object().unwrap().len(), 6);
        let state = kit.providers.lock().unwrap();
        let items = state["queueSource2"]["items"]
            .as_array()
            .expect("read-only observers made Coven unavailable");
        let row = items
            .iter()
            .find(|row| row["id"] == named["id"])
            .expect("live hosted row disappeared under observers");
        assert_eq!(row["label"], "SampleA");
        assert_eq!(
            row["information"]["context"],
            json!(["Kit project: Coven", "Kit branch: feature/ui"])
        );
        if observed["observations"][0]["freshness"] == "expired"
            && row["detail"]
                .as_str()
                .unwrap()
                .contains("UNKNOWN (expired)")
        {
            assert!(observed["observations"][0]["age_ms"].as_u64().unwrap() >= 30_000);
            assert_eq!(row["presentation"]["status"], "unknown");
            assert_eq!(
                row["information"]["activity"][0],
                "No recent activity evidence."
            );
            break;
        }
        drop(state);
        assert!(
            Instant::now() < end,
            "read storm renewed native evidence or hid expired Unknown"
        );
        thread::sleep(Duration::from_millis(50));
    }
    // Stop only host heartbeats; the same read storm must not extend the real
    // five-second attachment lease or resurrect an expired native fact.
    let end = Instant::now() + Duration::from_secs(6);
    loop {
        let observed: Value = serde_json::from_str(
            &agent_management::observations_arguments(path.to_str().unwrap(), true)
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert_eq!(observed["observations"][0]["freshness"], "expired");
        let items = kit.providers.lock().unwrap()["queueSource2"]["items"].clone();
        assert!(
            items.is_array(),
            "observer traffic terminated the available source"
        );
        if items.as_array().unwrap().is_empty() {
            break;
        }
        assert!(Instant::now() < end, "read storm renewed the host lease");
        thread::sleep(Duration::from_millis(50));
    }
    drop(receiver.0.stdin.take());
    receiver.finish();
}

#[test]
fn normal_cli_reports_before_eof_and_joins_on_eof_or_interrupt() {
    for ending in [0, libc::SIGINT, libc::SIGTERM] {
        let root = physical_temp::tempdir();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.path().join("coven.json"), "{}").unwrap();
        let version = if ending == libc::SIGINT {
            2
        } else if ending == libc::SIGTERM {
            3
        } else {
            1
        };
        let kit = Kit::start(
            root.path(),
            match version {
                2 => "v2",
                3 => "v3",
                _ => "success",
            },
        );
        let path = root.path().join("native.sock");
        let mut receiver = Receiver(
            Command::new(env!("CARGO_BIN_EXE_coven"))
                .args(["agent", "run"])
                .arg(root.path())
                .arg("--socket")
                .arg(&path)
                .arg("--kitsune-socket")
                .arg(root.path().join("kit.sock"))
                .args(["--source", "queueSource2", "--json"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let end = Instant::now() + Duration::from_secs(5);
        // A bound socket is not receiver readiness. Probe only the read path;
        // the native mutation below is still sent exactly once.
        while agent_management::observations_arguments(path.to_str().unwrap(), true).is_err() {
            assert!(Instant::now() < end);
            thread::sleep(Duration::from_millis(5));
        }
        let mut reporter = Command::new(env!("CARGO_BIN_EXE_coven"))
            .args(["agent", "report-native", "--socket"])
            .arg(&path)
            .arg("--json")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let payload = json!({"contract":"djinn.native-events.v1","instance_id":"12345678-1234-1234-1234-123456789abc","session_id":"PRIVATE-NATIVE-ID","sequence":1,"event":"session.status","status":"busy"});
        writeln!(reporter.stdin.take().unwrap(), "{payload}").unwrap();
        let report = reporter.wait_with_output().unwrap();
        assert!(
            report.status.success(),
            "{}",
            String::from_utf8_lossy(&report.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&report.stdout).unwrap(),
            json!({"type":"accepted","delivery":"volatile"})
        );
        loop {
            let calls = kit.calls.lock().unwrap();
            if calls.iter().any(|c| {
                c["method"] == "integration.view.replace"
                    && c["params"]["items"].to_string().contains("WORKING")
            }) {
                break;
            }
            drop(calls);
            assert!(
                Instant::now() < end,
                "normal report was not presented before EOF"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(receiver.0.try_wait().unwrap().is_none());
        let calls = kit.calls.lock().unwrap().clone();
        let working = calls
            .iter()
            .find(|call| {
                call["method"] == "integration.view.replace"
                    && call["params"]["items"].to_string().contains("WORKING")
            })
            .unwrap();
        let row = &working["params"]["items"][0];
        assert_eq!(row["label"], "Djinn");
        assert!(!row["label"].as_str().unwrap().contains("Coven /"));
        assert_eq!(row.get("information").is_some(), version == 3);
        if version == 3 {
            assert_eq!(row["information"]["activity"][0], "Last reported working.");
        }
        if version >= 2 {
            assert_eq!(row["presentation"]["status"], "running");
            assert_eq!(row["presentation"]["label"], "");
            let sidebar = format!(
                "{} {}",
                row["label"].as_str().unwrap(),
                row["presentation"]["label"].as_str().unwrap()
            );
            assert_eq!(sidebar.matches("Djinn").count(), 1);
            agent_management::native_report_payload(
                path.to_str().unwrap(),
                json!({"contract":"djinn.native-events.v1","instance_id":"12345678-1234-1234-1234-123456789abc",
                    "session_id":"PRIVATE-SECOND-SESSION","sequence":2,"event":"session.status","status":"idle"})
                    .to_string().as_bytes(),
                true,
            ).unwrap();
            let end = Instant::now() + Duration::from_secs(3);
            loop {
                let items = kit.providers.lock().unwrap()["queueSource2"]["items"].clone();
                if items.as_array().is_some_and(|rows| rows.len() == 2) {
                    assert_eq!(items[0]["label"], "Djinn");
                    assert_eq!(items[1]["label"], "Djinn");
                    assert_ne!(items[0]["id"], items[1]["id"]);
                    assert_eq!(items[0]["presentation"]["label"], "");
                    assert_eq!(items[1]["presentation"]["label"], "");
                    let working = items
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|item| item["id"] == row["id"])
                        .unwrap();
                    let idle = items
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|item| item["id"] != row["id"])
                        .unwrap();
                    assert_eq!(working["presentation"]["status"], "running");
                    assert!(idle["presentation"].get("status").is_none());
                    assert!(idle["detail"].as_str().unwrap().contains("IDLE"));
                    assert!(!items.to_string().contains("PRIVATE"));
                    break;
                }
                assert!(Instant::now() < end, "second Djinn was not presented");
                thread::sleep(Duration::from_millis(10));
            }
        } else {
            assert!(row.get("presentation").is_none());
            assert_eq!(row.as_object().unwrap().len(), 3);
        }
        assert!(!json!(calls).to_string().contains("PRIVATE-NATIVE-ID"));
        assert!(
            calls
                .iter()
                .any(|c| c["params"]["items"].to_string().contains("Coven"))
        );
        assert!(
            calls
                .iter()
                .any(|c| c["params"]["items"].to_string().contains("Djinn"))
        );
        assert!(!root.path().join("state/agent-management.json").exists());
        if ending == 0 {
            // Keeping the attached presenter alive must not extend evidence.
            // A duplicate is rejected and cannot renew the original receipt.
            assert!(
                agent_management::native_report_payload(
                    path.to_str().unwrap(),
                    payload.to_string().as_bytes(),
                    true
                )
                .is_err()
            );
            let expiry_end = Instant::now() + Duration::from_secs(32);
            loop {
                if kit.providers.lock().unwrap()["queueSource2"]["items"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
                {
                    break;
                }
                assert!(
                    Instant::now() < expiry_end,
                    "evidence was renewed by repainting"
                );
                thread::sleep(Duration::from_millis(20));
            }
            assert!(receiver.0.try_wait().unwrap().is_none());
            let history: Value = serde_json::from_str(
                &agent_management::observations_arguments(path.to_str().unwrap(), true)
                    .unwrap()
                    .stdout,
            )
            .unwrap();
            assert_eq!(history["observations"].as_array().unwrap().len(), 1);
            assert_eq!(history["observations"][0]["freshness"], "expired");
            assert_eq!(
                format!(
                    "observation:{}",
                    history["observations"][0]["observation_id"]
                        .as_str()
                        .unwrap()
                ),
                row["id"].as_str().unwrap()
            );
            agent_management::report_payload(
                &ReportRequest {
                    socket: path.to_string_lossy().into_owned(),
                    provider: "claude".into(),
                    event: None,
                },
                br#"{"session_id":"historical-ref","hook_event_name":"Stop"}"#,
                true,
            )
            .unwrap();
        }
        if ending == 0 {
            drop(receiver.0.stdin.take());
        } else {
            assert_eq!(unsafe { libc::kill(receiver.0.id() as i32, ending) }, 0);
        }
        receiver.finish();
        assert!(!path.exists());
        assert!(
            kit.calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c["method"] == "integration.view.close")
        );
        if ending == 0 {
            let before = kit.calls.lock().unwrap().len();
            let mut restarted = Receiver(
                Command::new(env!("CARGO_BIN_EXE_coven"))
                    .args(["agent", "run"])
                    .arg(root.path())
                    .arg("--socket")
                    .arg(&path)
                    .arg("--kitsune-socket")
                    .arg(root.path().join("kit.sock"))
                    .args(["--source", "queueSource2", "--json"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap(),
            );
            let end = Instant::now() + Duration::from_secs(5);
            loop {
                let calls = kit.calls.lock().unwrap();
                if let Some(frame) = calls[before..]
                    .iter()
                    .find(|c| c["method"] == "integration.view.replace")
                {
                    let items = frame["params"]["items"].as_array().unwrap();
                    assert_eq!(
                        items.len(),
                        1,
                        "native observations must be lost on restart"
                    );
                    assert!(
                        items[0]["detail"]
                            .as_str()
                            .unwrap()
                            .contains("UNKNOWN (historical)")
                    );
                    break;
                }
                drop(calls);
                assert!(Instant::now() < end);
                thread::sleep(Duration::from_millis(10));
            }
            drop(restarted.0.stdin.take());
            restarted.finish();
        }
    }
}

#[test]
fn native_cli_reconstructs_closed_rpc_and_rejects_peer_extra_fields() {
    for (extra, event) in [
        (false, "session.created"),
        (true, "session.created"),
        (false, "permission.cancelled"),
        (false, "question.cancelled"),
    ] {
        let root = physical_temp::tempdir();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.path().join("peer.sock");
        let listener = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request.as_object().unwrap().len(), 4);
            assert_eq!(request["contract"], "coven.native-events.v1");
            assert_eq!(request["method"], "report");
            assert_eq!(
                request["params"].as_object().unwrap().len(),
                if event == "session.created" { 4 } else { 5 }
            );
            assert_eq!(request["params"]["event"], event);
            assert!(request["params"].get("contract").is_none());
            let mut result = json!({"type":"accepted","delivery":"volatile"});
            if extra {
                result["prompt"] = json!("PRIVATE-PEER");
            }
            writeln!(stream,"{}",json!({"id":request["id"],"contract":"coven.native-events.v1","type":"result","result":result})).unwrap();
        });
        let mut reporter = Command::new(env!("CARGO_BIN_EXE_coven"))
            .args(["agent", "report-native", "--socket"])
            .arg(&path)
            .arg("--json")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut payload = json!({"contract":"djinn.native-events.v1","instance_id":"12345678-1234-1234-1234-123456789abc","session_id":"PRIVATE-SID","sequence":1,"event":event});
        if event != "session.created" {
            payload["request_id"] = json!("PRIVATE-REQUEST");
        }
        writeln!(reporter.stdin.take().unwrap(), "{payload}").unwrap();
        let result = reporter.wait_with_output().unwrap();
        worker.join().unwrap();
        assert_eq!(result.status.success(), !extra);
        assert!(!String::from_utf8_lossy(&result.stdout).contains("PRIVATE"));
        assert!(!String::from_utf8_lossy(&result.stderr).contains("PRIVATE"));
    }
}
impl Receiver {
    fn start(root: &std::path::Path) -> Self {
        Self(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "receiver_child", "--nocapture"])
                .env("COVEN_AGENT_VIEW_TEST_ROOT", root)
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    fn finish(mut self) {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(
                Instant::now() < deadline,
                "receiver did not stop within bound"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Kit {
    calls: Arc<Mutex<Vec<Value>>>,
    providers: Arc<Mutex<Value>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Kit {
    fn start(root: &std::path::Path, mode: &'static str) -> Self {
        let path = root.join("kit.sock");
        let listener = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let providers = Arc::new(Mutex::new(json!({"build":{
            "lease":"build-lease","epoch":"build-epoch","revision":7,
            "items":[{"id":"build:job","label":"Build"}],
            "intents":[{"epoch":"build-epoch","revision":7,"item_id":"build:job"}]}})));
        let stop = Arc::new(AtomicBool::new(false));
        let seen = calls.clone();
        let state = providers.clone();
        let done = stop.clone();
        let worker = thread::spawn(move || {
            let mut revision = 0;
            let mut selected = false;
            while !done.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut line = String::new();
                let count = BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                if count == 0 {
                    // A failed pre-send qualification closes the prepared socket without a frame.
                    continue;
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                seen.lock().unwrap().push(request.clone());
                let method = request["method"].as_str().unwrap();
                let params = &request["params"];
                if method.starts_with("integration.view.") {
                    assert_eq!(params["source"], "queueSource2");
                    if method != "integration.view.open" {
                        assert_eq!(params["lease"], "queue-lease");
                        assert_eq!(params["epoch"], "queue-epoch");
                    }
                }
                if matches!(mode, "busy" | "reject_v2" | "reject_v3")
                    && method == "integration.view.open"
                {
                    if matches!(mode, "reject_v2" | "reject_v3") {
                        assert_eq!(params["version"], if mode == "reject_v3" { 3 } else { 2 });
                    }
                    let reply = json!({"id":request["id"],"error":{"code":if mode.starts_with("reject_") {"unsupportedVersion"} else {"sourceBusy"},"message":"PRIVATE MESSAGE"}});
                    writeln!(stream, "{reply}").unwrap();
                    continue;
                }
                let result = match method {
                    "session.snapshot" => {
                        let mut reply = json!({"type":"session_snapshot","snapshot":{
                        "version":"fixture","protocol":18,"selected_integration_source":"build",
                        "integration_views":{"version":if mode == "unsupported" {2} else {1},
                            "sources":[{"id":"build","epoch":"build-epoch"},{"id":"queueSource2","epoch":null}]}}});
                        if matches!(mode, "v2" | "lost_open_v2" | "reject_v2" | "bad_sources") {
                            reply["snapshot"]["integration_views"]["supported_versions"] =
                                json!([1, 2]);
                        }
                        if matches!(mode, "v3" | "lost_open_v3" | "reject_v3" | "names") {
                            reply["snapshot"]["integration_views"]["supported_versions"] =
                                json!([1, 2, 3]);
                        }
                        if mode == "advertised_v1" {
                            reply["snapshot"]["integration_views"]["supported_versions"] =
                                json!([1, 99]);
                        }
                        if mode == "bad_versions" {
                            reply["snapshot"]["integration_views"]["supported_versions"] =
                                json!([1, 2, 2]);
                        }
                        if mode == "bad_sources" {
                            reply["snapshot"]["integration_views"]["sources"]
                                .as_array_mut()
                                .unwrap()
                                .push(json!({"id":"build","epoch":null}));
                        }
                        if mode == "names" {
                            reply["snapshot"]["terminal_navigation"] = json!({"version":1});
                            reply["snapshot"]["panes"] =
                                state.lock().unwrap()["_naming"]["panes"].clone();
                            reply["snapshot"]["workspaces"] =
                                state.lock().unwrap()["_naming"]["workspaces"].clone();
                            reply["snapshot"]["tabs"] =
                                state.lock().unwrap()["_naming"]["tabs"].clone();
                        }
                        reply
                    }
                    "terminal.inspect" if mode == "names" => {
                        let delay = state.lock().unwrap()["_naming"]["delay_ms"]
                            .as_u64()
                            .unwrap_or(0)
                            .min(200);
                        thread::sleep(Duration::from_millis(delay));
                        let hung = state.lock().unwrap()["_naming"]["hung"] == true;
                        if hung {
                            while !done.load(Ordering::Acquire) {
                                thread::sleep(Duration::from_millis(5));
                            }
                            continue;
                        }
                        state.lock().unwrap()["_naming"]["inspection"].clone()
                    }
                    "integration.view.open" => {
                        revision = 0;
                        assert_eq!(
                            params["version"],
                            if matches!(mode, "v3" | "lost_open_v3" | "names") {
                                3
                            } else if matches!(mode, "v2" | "lost_open_v2") {
                                2
                            } else {
                                1
                            }
                        );
                        state.lock().unwrap()["queueSource2"] = json!({"items":[],"intents":[]});
                        if matches!(mode, "lost_open" | "lost_open_v2" | "lost_open_v3") {
                            continue;
                        }
                        json!({"type":"view_opened","lease":"queue-lease","epoch":"queue-epoch","revision":0,"ttl_ms":5000})
                    }
                    "integration.view.replace" => {
                        assert_eq!(params["revision"].as_u64(), Some(revision + 1));
                        revision += 1;
                        assert!(serde_json::to_vec(params).unwrap().len() <= 96 * 1024);
                        assert!(params["items"].as_array().unwrap().len() <= 128);
                        for row in params["items"].as_array().unwrap() {
                            assert_eq!(
                                row.as_object().unwrap().len(),
                                if matches!(mode, "v3" | "names") {
                                    5
                                } else if mode == "v2" {
                                    4
                                } else {
                                    3
                                }
                            );
                            assert!(row["detail"].as_str().unwrap().len() <= 512);
                            if matches!(mode, "v2" | "v3" | "names") {
                                assert!(row["presentation"]["label"].as_str().unwrap().len() <= 48);
                                assert!(!matches!(
                                    row["presentation"]["status"].as_str(),
                                    Some("success" | "error")
                                ));
                            }
                            if matches!(mode, "v3" | "names") {
                                let info = row["information"].as_object().unwrap();
                                assert_eq!(info.len(), 3);
                                let mut bytes = 0;
                                for key in ["attention", "activity", "context"] {
                                    let lines = info[key].as_array().unwrap();
                                    assert!(lines.len() <= 4);
                                    for line in lines {
                                        let line = line.as_str().unwrap();
                                        assert!(!line.trim().is_empty() && line.len() <= 256);
                                        assert!(!line.chars().any(|c| c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}')));
                                        bytes += line.len();
                                    }
                                }
                                assert!(bytes <= 1536);
                            } else {
                                assert!(row.get("information").is_none());
                            }
                        }
                        state.lock().unwrap()["queueSource2"]["items"] = params["items"].clone();
                        json!({"type":"view_replaced","epoch":"queue-epoch","revision":revision})
                    }
                    "integration.view.poll" => {
                        if mode == "lost_poll" {
                            continue;
                        }
                        if mode == "blocked_poll" {
                            while !done.load(Ordering::Acquire) {
                                thread::sleep(Duration::from_millis(5));
                            }
                            continue;
                        }
                        let item = state.lock().unwrap()["queueSource2"]["items"][0]["id"].clone();
                        let intents = if !selected && item.is_string() {
                            selected = true;
                            json!([
                                {"epoch":"build-epoch","revision":revision,"item_id":item},
                                {"epoch":"queue-epoch","revision":revision-1,"item_id":item},
                                {"epoch":"queue-epoch","revision":revision,"item_id":"agent:unknown"},
                                {"epoch":"queue-epoch","revision":revision,"item_id":item}
                            ])
                        } else {
                            json!([])
                        };
                        json!({"type":"view_intents","epoch":"queue-epoch","revision":revision,"ttl_ms":5000,"intents":intents})
                    }
                    "integration.view.close" => {
                        state
                            .lock()
                            .unwrap()
                            .as_object_mut()
                            .unwrap()
                            .remove("queueSource2");
                        json!({"type":"view_closed"})
                    }
                    other => panic!("unexpected effect: {other}"),
                };
                let reply = json!({"id":request["id"],"result":result});
                let _ = writeln!(stream, "{reply}");
            }
        });
        Self {
            calls,
            providers,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Kit {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}

#[test]
fn optional_kit_isolated_from_receiver_and_other_sources() {
    for mode in [
        "unavailable",
        "unsupported",
        "busy",
        "success",
        "v2",
        "v3",
        "advertised_v1",
        "bad_versions",
        "bad_sources",
        "reject_v2",
        "reject_v3",
        "lost_open_v2",
        "lost_open_v3",
        "codex",
        "lost_open",
        "lost_poll",
        "blocked_poll",
        "ctrl_c",
    ] {
        let root = physical_temp::tempdir();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.path().join("coven.json"), "{}").unwrap();
        let kit = (mode != "unavailable").then(|| Kit::start(root.path(), mode));
        let build_before = kit
            .as_ref()
            .map(|kit| kit.providers.lock().unwrap()["build"].clone());
        let startup = Instant::now();
        let mut receiver = Receiver::start(root.path());
        let socket = root
            .path()
            .join("receiver.sock")
            .to_str()
            .unwrap()
            .to_owned();
        let deadline = startup + Duration::from_secs(5);
        loop {
            let readiness = agent_management::inventory_arguments(
                &InventoryRequest {
                    socket: socket.clone(),
                },
                true,
            );
            if readiness.is_ok() {
                assert!(
                    Instant::now() < deadline,
                    "receiver readiness exceeded startup bound: mode={mode}; startup_ms={}",
                    startup.elapsed().as_millis()
                );
                break;
            }
            let probe = readiness.as_ref().err().map(|error| error.code());
            let status = receiver.0.try_wait().unwrap();
            assert!(
                status.is_none(),
                "receiver exited before readiness: mode={mode}; status={status:?}; startup_ms={}; probe={probe:?}",
                startup.elapsed().as_millis()
            );
            assert!(
                Instant::now() < deadline,
                "receiver not ready: mode={mode}; startup_ms={}; probe={probe:?}",
                startup.elapsed().as_millis()
            );
            thread::sleep(Duration::from_millis(5));
        }
        // Process scheduling/store startup is not part of the two-second serve
        // interval or the admitted receiver's report/ACK and shutdown budgets.
        let started = Instant::now();
        let provider = if mode == "codex" { "codex" } else { "claude" };
        let event = if mode == "codex" {
            "SessionStart"
        } else {
            "PreToolUse"
        };
        let state = if mode == "codex" {
            "unknown"
        } else {
            "working"
        };
        let expected_id = format!(
            "{:x}",
            Sha256::digest([provider.as_bytes(), b"\0", b"PRIVATE-NATIVE-SID"].concat())
        );
        let report_started = Instant::now();
        // Send exactly once. An uncertain receipt is not evidence of non-delivery.
        let receipt = agent_management::report_payload(&ReportRequest {
            socket:socket.clone(), provider:provider.into(), event:Some(event.into()),
        }, json!({"session_id":"PRIVATE-NATIVE-SID","hook_event_name":event,"source":"startup","model":"PRIVATE-MODEL","permission_mode":"default","transcript_path":"PRIVATE-TRANSCRIPT","cwd":"/PRIVATE/CWD","prompt":"PRIVATE-PROMPT","token":"PRIVATE-TOKEN"}).to_string().as_bytes(), true);
        let report_elapsed = report_started.elapsed();
        let receipt_status = match receipt {
            Ok(accepted) => {
                let accepted: Value = serde_json::from_str(&accepted.stdout).unwrap();
                assert_eq!(accepted["type"], "accepted", "mode={mode}");
                assert_eq!(accepted["delivery"], "committed", "mode={mode}");
                assert_eq!(accepted["record_id"], expected_id, "mode={mode}");
                "acknowledged"
            }
            Err(error) => {
                assert_eq!(error.code(), "agent_delivery_uncertain", "mode={mode}");
                "agent_delivery_uncertain"
            }
        };
        eprintln!(
            "mode={mode} report={receipt_status} report_ms={} elapsed_ms={}",
            report_elapsed.as_millis(),
            started.elapsed().as_millis()
        );
        thread::sleep(Duration::from_millis(900));
        let inventory_started = Instant::now();
        let context = format!(
            "mode={mode} report={receipt_status} report_ms={} elapsed_ms={}",
            report_elapsed.as_millis(),
            started.elapsed().as_millis()
        );
        let mut stream = UnixStream::connect(&socket).expect(&context);
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let id = "1".repeat(32);
        writeln!(
            stream,
            "{}",
            json!({"id":id,"contract":agent_management::CONTRACT,
            "method":"inventory","params":{}})
        )
        .unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).expect(&context);
        let reply: Value = serde_json::from_str(&line).expect(&context);
        assert_eq!(reply["id"], id, "{context}");
        assert_eq!(reply["contract"], agent_management::CONTRACT, "{context}");
        assert_eq!(reply["type"], "result", "{context}");
        let inventory = &reply["result"];
        assert_eq!(
            inventory["contract"],
            agent_management::CONTRACT,
            "{context}"
        );
        assert_eq!(inventory["authorization"], "none", "{context}");
        assert_eq!(
            inventory["records"].as_array().unwrap().len(),
            1,
            "{context}"
        );
        let record = &inventory["records"][0];
        assert_eq!(record["record_id"], expected_id, "{context}");
        assert_eq!(record["provider"], provider, "{context}");
        assert_eq!(record["last_event"], event, "{context}");
        assert_eq!(record["reported_state"], state, "{context}");
        assert!(record["age_ms"].as_u64().is_some(), "{context}");
        assert_eq!(
            record["reference_trust"], "caller_supplied_unverified",
            "{context}"
        );
        assert_eq!(record["native_identity"], "unestablished", "{context}");
        assert_eq!(record["job_completion"], "unestablished", "{context}");
        // Require private on-disk proof as well, even when no ack was received.
        // Never print the saved value: it contains the native session reference.
        let store = root.path().join("state/agent-management.json");
        let metadata = fs::symlink_metadata(&store).expect(&context);
        assert!(metadata.is_file(), "{context}");
        assert_eq!(metadata.mode() & 0o7777, 0o600, "{context}");
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() }, "{context}");
        assert_eq!(metadata.nlink(), 1, "{context}");
        let saved_bytes = fs::read(&store).expect(&context);
        let saved: Value = serde_json::from_slice(&saved_bytes).expect(&context);
        assert_eq!(saved["schema_version"], 1, "{context}");
        assert_eq!(saved["records"].as_object().unwrap().len(), 1, "{context}");
        let persisted = &saved["records"][&expected_id];
        assert!(
            persisted["session_id"] == "PRIVATE-NATIVE-SID",
            "{context}: saved identity mismatch"
        );
        assert!(
            persisted["provider"] == provider,
            "{context}: saved provider mismatch"
        );
        assert!(
            persisted["last_event"] == event,
            "{context}: saved event mismatch"
        );
        assert!(
            persisted["server_epoch"] == inventory["server_epoch"],
            "{context}: saved epoch mismatch"
        );
        assert!(
            persisted["timestamp_ms"].as_u64().is_some_and(|ms| ms > 0),
            "{context}: missing saved timestamp"
        );
        eprintln!(
            "mode={mode} report={receipt_status} raw_inventory=original_report persisted=original_report inventory_ms={} elapsed_ms={}",
            inventory_started.elapsed().as_millis(),
            started.elapsed().as_millis()
        );
        let interrupted_at = (mode == "ctrl_c").then(|| {
            assert_eq!(
                unsafe { libc::kill(receiver.0.id() as i32, libc::SIGINT) },
                0
            );
            Instant::now()
        });
        receiver.finish();
        assert!(
            fs::read(&store).expect(&context) == saved_bytes,
            "{context}: read-only presentation changed persisted report"
        );
        if let Some(at) = interrupted_at {
            assert!(
                at.elapsed() < Duration::from_secs(1),
                "Ctrl-C shutdown exceeded bound"
            );
        }
        assert!(
            started.elapsed() < Duration::from_millis(3100),
            "shutdown exceeded bound: {mode}"
        );
        let report: Value =
            serde_json::from_slice(&fs::read(root.path().join("result.json")).unwrap()).unwrap();
        assert_eq!(report["type"], "stopped");
        assert_eq!(report["authorization"], "none");
        let ui = &report["presentation"];
        eprintln!(
            "mode={mode} completed elapsed_ms={}",
            started.elapsed().as_millis()
        );
        if let Some(kit) = &kit {
            let calls = kit.calls.lock().unwrap();
            assert_eq!(
                kit.providers.lock().unwrap()["build"],
                build_before.unwrap()
            );
            assert_eq!(
                calls
                    .iter()
                    .filter(|c| c["method"] == "session.snapshot")
                    .count(),
                1
            );
            assert!(
                calls
                    .iter()
                    .filter(|c| c["method"] == "integration.view.open")
                    .count()
                    <= 1
            );
            let serialized = serde_json::to_string(&*calls).unwrap();
            for private in [
                "PRIVATE-NATIVE-SID",
                "/PRIVATE/CWD",
                "PRIVATE-PROMPT",
                "PRIVATE-TOKEN",
                "PRIVATE-MODEL",
                "PRIVATE-TRANSCRIPT",
                "permission_mode",
            ] {
                assert!(!serialized.contains(private));
            }
            if matches!(mode, "bad_versions" | "bad_sources" | "unsupported") {
                assert!(
                    calls
                        .iter()
                        .all(|call| call["method"] == "session.snapshot")
                );
            }
            if matches!(mode, "reject_v2" | "reject_v3") {
                assert_eq!(
                    calls.len(),
                    2,
                    "a rejected open must not trigger a downgrade or mutation"
                );
                assert_eq!(calls[1]["method"], "integration.view.open");
                assert_eq!(
                    calls[1]["params"]["version"],
                    if mode == "reject_v3" { 3 } else { 2 }
                );
            }
            if matches!(
                mode,
                "success" | "ctrl_c" | "codex" | "v2" | "v3" | "advertised_v1"
            ) {
                assert_eq!(ui["cleanup"], "closed");
                assert!(calls.iter().any(|c| {
                    c["method"] == "integration.view.replace"
                        && c["params"]["items"][0]["label"]
                            .as_str()
                            .is_some_and(|s| s.to_ascii_lowercase().starts_with(provider))
                        && c["params"]["items"][0]["detail"].as_str().is_some_and(|s| {
                            s.starts_with(&format!("Last report: {}", state.to_ascii_uppercase()))
                                && s.contains("Selected (read-only)")
                        })
                }));
                assert_eq!(
                    calls
                        .iter()
                        .filter(|c| c["method"] == "integration.view.close")
                        .count(),
                    1
                );
                assert!(kit.providers.lock().unwrap().get("queueSource2").is_none());
            } else if matches!(
                mode,
                "lost_open" | "lost_open_v2" | "lost_open_v3" | "lost_poll" | "blocked_poll"
            ) {
                assert_eq!(ui["outcome"], "uncertain");
                assert!(
                    calls
                        .iter()
                        .all(|c| c["method"] != "integration.view.close")
                );
            } else {
                assert_eq!(ui["outcome"], "failed");
            }
        } else {
            assert_eq!(ui["outcome"], "failed");
        }
    }
}

#[test]
fn failed_core_load_does_not_admit_kit_discovery() {
    let root = physical_temp::tempdir();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.path().join("coven.json"), "{}").unwrap();
    fs::create_dir(root.path().join("state")).unwrap();
    let store = root.path().join("state/agent-management.json");
    fs::write(&store, "invalid persisted inventory").unwrap();
    fs::set_permissions(&store, fs::Permissions::from_mode(0o600)).unwrap();
    let kit = Kit::start(root.path(), "success");
    let started = Instant::now();
    assert!(
        agent_management_presentation::serve_arguments(
            &ServeRequest {
                workspace: root.path().into(),
                socket: root.path().join("receiver.sock").to_str().unwrap().into(),
                duration: 1
            },
            &PresentationRequest {
                socket: root.path().join("kit.sock").to_str().unwrap().into(),
                source: "queueSource2".into()
            },
            true,
        )
        .is_err()
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(kit.calls.lock().unwrap().is_empty());
}
