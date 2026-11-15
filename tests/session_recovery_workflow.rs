//! Private Unix fixtures only: the executable fixture is never run.
#![cfg(unix)]
#[path = "support/interactive_session_server.rs"]
mod fixture;
use coven::session_recovery::{self as recovery, Request};
use fixture::{Server, ack, frame, inventory, snapshot};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::{fs, path::Path, process::Command};

fn selected(executable: &Path, key: &str, value: &str) -> Value {
    json!({"key":key,"reference":{"source":"kitsune:djinn","runtime":"djinn","kind":"id","value":value},
        "executable":executable,"pane_id":"w1:p1","expected_terminal_id":"terminal-1"})
}
fn selection(entries: Vec<Value>) -> Value {
    json!({"schema_version":1,"entries":entries})
}
fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn init(path: &Path) {
    coven::init::arguments(
        path,
        &coven::init::Options {
            blank: true,
            agents: "reviewer".into(),
            ..Default::default()
        },
    )
    .unwrap();
}
fn inspect(path: &Path) -> Value {
    serde_json::from_str(&recovery::inspect(path, true).unwrap().stdout).unwrap()
}
fn request(workspace: &Path, file: &Path, socket: &Path, execute: bool) -> Request {
    Request {
        workspace: workspace.into(),
        file: file.into(),
        socket: socket.to_str().unwrap().into(),
        expect_recovery_revision: inspect(workspace)["recovery_revision"]
            .as_str()
            .unwrap()
            .into(),
        execute,
    }
}
fn call(request: &Request, code: u8) -> Value {
    let output = recovery::run(request, true).unwrap();
    assert_eq!(output.exit_code, code, "{}", output.stdout);
    assert!(output.stdout.is_ascii());
    serde_json::from_str(&output.stdout).unwrap()
}
fn writes(server: &Server) -> usize {
    server
        .requests()
        .iter()
        .filter(|r| r["method"] == "pane.submit_command")
        .count()
}
fn normal() -> Server {
    Server::new(|_, req| {
        if req["method"] == "session.snapshot" {
            snapshot(req, inventory())
        } else {
            frame(ack(req))
        }
    })
}

#[test]
fn preview_is_read_only_even_without_workspace_or_socket() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("missing");
    let file = temp.path().join("selection.json");
    write(
        &file,
        &selection(vec![selected(
            Path::new("/absent/provider"),
            "one",
            "saved",
        )]),
    );
    let req = request(&workspace, &file, Path::new("/absent/api.sock"), false);
    let report = call(&req, 0);
    assert_eq!(report["entries"][0]["outcome"], "preview");
    assert_eq!(report["authorization"], "none");
    assert!(!workspace.exists());
    assert_eq!(inspect(&workspace)["state"]["attempts"], json!([]));
    assert_eq!(
        inspect(&workspace)["recovery_revision"],
        req.expect_recovery_revision
    );
}

#[test]
fn strict_bounded_closed_selection_and_policy_validation_precede_contact() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("missing");
    let file = temp.path().join("selection.json");
    let server = Server::new(|_, _| panic!("invalid selection contacted endpoint"));
    let entry = selected(Path::new("/absent/provider"), "one", "saved");
    write(&file, &selection(vec![entry.clone()]));
    let req = request(&workspace, &file, &server.path, true);
    let mut invalid = vec![
        json!({"schema_version":2,"entries":[entry.clone()]}),
        selection(vec![]),
        selection(vec![entry.clone(); 17]),
        selection(vec![entry.clone(), entry.clone()]),
    ];
    for (field, value) in [
        ("argv", json!(["arbitrary"])),
        ("env", json!({})),
        ("prompt", json!("secret")),
        ("executable", json!("relative")),
        ("pane_id", json!("")),
    ] {
        let mut bad = entry.clone();
        bad[field] = value;
        invalid.push(selection(vec![bad]));
    }
    for (field, value) in [
        ("source", "kitsune:claude"),
        ("value", "--help"),
        ("kind", "unknown"),
    ] {
        let mut bad = entry.clone();
        bad["reference"][field] = json!(value);
        invalid.push(selection(vec![bad]));
    }
    let mut bad = entry.clone();
    bad["reference"]["kind"] = json!("path");
    bad["reference"]["value"] = json!("relative");
    invalid.push(selection(vec![bad]));
    let mut bad = selection(vec![entry.clone()]);
    bad["permission"] = json!(true);
    invalid.push(bad);
    for bad in invalid {
        write(&file, &bad);
        assert!(recovery::run(&req, true).is_err(), "{bad}");
    }
    for raw in [
        b"{".to_vec(),
        br#"{"schema_version":1,"schema_version":1,"entries":[]}"#.to_vec(),
        br#"{"schema_version":1,"\u0073chema_version":1,"entries":[]}"#.to_vec(),
        vec![b' '; 1_048_577],
        format!("{}0{}", "[".repeat(17), "]".repeat(17)).into_bytes(),
    ] {
        fs::write(&file, raw).unwrap();
        assert!(recovery::run(&req, true).is_err());
    }
    assert!(!workspace.exists());
    assert!(server.requests().is_empty());
}

#[test]
fn whole_selection_import_veto_duplicates_and_target_conflicts() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("missing");
    let file = temp.path().join("selection.json");
    let entry = selected(Path::new("/absent/provider"), "one", "saved");
    let mut duplicate = entry.clone();
    duplicate["key"] = json!("two");
    duplicate["pane_id"] = json!("another");
    duplicate["expected_terminal_id"] = json!("another-terminal");
    duplicate["executable"] = json!("/different/exe");
    write(&file, &selection(vec![entry.clone(), duplicate.clone()]));
    let req = request(&workspace, &file, Path::new("/absent/api.sock"), false);
    assert_eq!(call(&req, 0)["entries"][1]["outcome"], "skipped_duplicate");
    let mut imported = selection(vec![entry.clone(), duplicate.clone()]);
    imported["imported_references"] = json!([entry["reference"]]);
    write(&file, &imported);
    let report = call(&req, 0);
    assert!(
        report["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["outcome"] == "skipped_imported")
    );
    // Exact tuple: a path or literal-value change is not an imported match.
    imported["imported_references"][0]["value"] = json!("elsewhere");
    write(&file, &imported);
    assert_eq!(call(&req, 0)["entries"][0]["outcome"], "preview");
    for target in ["pane_id", "expected_terminal_id"] {
        let mut other = duplicate.clone();
        other["reference"]["value"] = json!("different");
        other[target] = entry[target].clone();
        write(&file, &selection(vec![entry.clone(), other]));
        assert_eq!(
            recovery::run(&req, true).err().unwrap().code(),
            "conflicting_recovery_target"
        );
    }
    let mut unsupported = entry;
    unsupported["reference"]["runtime"] = json!("unknown");
    unsupported["reference"]["source"] = json!("kitsune:unknown");
    write(&file, &selection(vec![unsupported]));
    assert_eq!(
        call(&req, 2)["entries"][0]["outcome"],
        "adapter_unsupported"
    );
    assert!(!workspace.exists());
}

// One test owns process-wide signal installation for the library execution API.
#[test]
fn executable_recovery_journal_workflow() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    init(&workspace);
    let exe = fixture::executable(temp.path());
    let file = temp.path().join("selection.json");
    let entry = selected(&exe, "one", "saved");
    write(&file, &selection(vec![entry.clone()]));
    let agents = fs::read(workspace.join("state/agents.json")).unwrap();
    let tasks = fs::read(workspace.join("state/tasks.json")).unwrap();

    // Once the reference snapshot succeeds, an unavailable executable defers.
    // Only a new explicit CAS retries; reference-read failure itself is unsent.
    let unavailable = normal();
    let mut missing = entry.clone();
    missing["executable"] = json!("/absent/provider");
    write(&file, &selection(vec![missing]));
    let req = request(&workspace, &file, &unavailable.path, true);
    assert_eq!(call(&req, 1)["entries"][0]["outcome"], "deferred");
    assert!(recovery::run(&req, true).is_err());
    write(&file, &selection(vec![entry.clone()]));
    let server = normal();
    let req = request(&workspace, &file, &server.path, true);
    let result = call(&req, 0);
    assert_eq!(result["entries"][0]["outcome"], "queued");
    assert_eq!(writes(&server), 1);
    let req = request(&workspace, &file, &server.path, true);
    assert_eq!(call(&req, 0)["entries"][0]["outcome"], "skipped_journal");
    assert_eq!(writes(&server), 1);
    assert_eq!(
        inspect(&workspace)["state"]["attempts"][1]["outcome"],
        "queued"
    );
    // A process restart does not erase the queued tombstone.
    let output = cli(&req).output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["entries"][0]["outcome"],
        "skipped_journal"
    );
    assert_eq!(writes(&server), 1);

    // An explicit rejection releases reservation but retains its outcome.
    let rejected = Server::new(|_, req| {
        if req["method"] == "session.snapshot" {
            snapshot(req, inventory())
        } else {
            frame(json!({"id":req["id"],"error":{"code":"busy","message":"busy"}}))
        }
    });
    let mut second = entry.clone();
    second["reference"]["value"] = json!("second");
    write(&file, &selection(vec![second.clone()]));
    let req = request(&workspace, &file, &rejected.path, true);
    assert_eq!(call(&req, 1)["entries"][0]["outcome"], "rejected");
    let req = request(&workspace, &file, &server.path, true);
    assert_eq!(call(&req, 0)["entries"][0]["outcome"], "queued");

    // Lost acknowledgement stops the batch and conservatively blocks after restart.
    let lost = Server::new(|_, req| {
        if req["method"] == "session.snapshot" {
            snapshot(req, inventory())
        } else {
            vec![]
        }
    });
    let mut third = entry.clone();
    third["reference"]["value"] = json!("third");
    let mut fourth = selected(&exe, "four", "fourth");
    fourth["pane_id"] = json!("w2:p2");
    fourth["expected_terminal_id"] = json!("terminal-2");
    write(&file, &selection(vec![third.clone(), fourth]));
    let req = request(&workspace, &file, &lost.path, true);
    let report = call(&req, 2);
    assert_eq!(report["entries"][0]["outcome"], "uncertain");
    assert_eq!(report["entries"][1]["outcome"], "unattempted_batch_stopped");
    assert_eq!(writes(&lost), 1);
    write(&file, &selection(vec![third]));
    let req = request(&workspace, &file, &server.path, true);
    let output = cli(&req).output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["entries"][0]["outcome"],
        "skipped_journal"
    );

    // Callback failure is unsent and stops remaining entries; no corrupt journal rewrite.
    let mut fifth = entry.clone();
    fifth["reference"]["value"] = json!("fifth");
    write(&file, &selection(vec![fifth]));
    let state_dir = workspace.join("state");
    let altered = state_dir.clone();
    let failure = Server::new(move |index, req| {
        assert_eq!(req["method"], "session.snapshot");
        if index == 3 {
            fs::set_permissions(&altered, fs::Permissions::from_mode(0o500)).unwrap();
        }
        snapshot(req, inventory())
    });
    let req = request(&workspace, &file, &failure.path, true);
    let output = recovery::run(&req, true);
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let report: Value = serde_json::from_str(&output.unwrap().stdout).unwrap();
    assert_eq!(report["entries"][0]["outcome"], "intent_persistence_failed");
    assert_eq!(writes(&failure), 0);
    let req = request(&workspace, &file, &server.path, true);
    assert_eq!(call(&req, 0)["entries"][0]["outcome"], "queued");

    // Post-send persistence failure leaves the durable intent, never a retry permit.
    let mut sixth = entry;
    sixth["reference"]["value"] = json!("sixth");
    write(&file, &selection(vec![sixth]));
    let altered = state_dir.clone();
    let post = Server::new(move |_, req| {
        if req["method"] == "session.snapshot" {
            snapshot(req, inventory())
        } else {
            fs::set_permissions(&altered, fs::Permissions::from_mode(0o500)).unwrap();
            frame(ack(req))
        }
    });
    let req = request(&workspace, &file, &post.path, true);
    let output = recovery::run(&req, true);
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let output = output.unwrap();
    assert_eq!(output.exit_code, 2);
    assert_eq!(
        inspect(&workspace)["state"]["attempts"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["outcome"],
        "submission_intent"
    );
    let req = request(&workspace, &file, &server.path, true);
    assert_eq!(call(&req, 0)["entries"][0]["outcome"], "skipped_journal");
    assert_eq!(
        fs::read(workspace.join("state/agents.json")).unwrap(),
        agents
    );
    assert_eq!(fs::read(workspace.join("state/tasks.json")).unwrap(), tasks);
}

fn cli(req: &Request) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_coven"));
    command
        .args(["session", "recovery", "run"])
        .arg(&req.workspace)
        .arg("--file")
        .arg(&req.file)
        .args([
            "--socket",
            &req.socket,
            "--expect-recovery-revision",
            &req.expect_recovery_revision,
            "--json",
        ]);
    if req.execute {
        command.arg("--execute");
    }
    command
}

#[test]
fn concurrent_cli_cas_allows_at_most_one_submission() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    init(&workspace);
    let exe = fixture::executable(temp.path());
    let file = temp.path().join("selection.json");
    write(&file, &selection(vec![selected(&exe, "one", "saved")]));
    let server = normal();
    let req = request(&workspace, &file, &server.path, true);
    let a = cli(&req)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let b = cli(&req)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let a = a.wait_with_output().unwrap();
    let b = b.wait_with_output().unwrap();
    assert!(a.status.success() || b.status.success(), "{a:?} {b:?}");
    assert_eq!(writes(&server), 1);
    assert_eq!(
        inspect(&workspace)["state"]["attempts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn corrupt_newer_full_state_and_admission_denial_are_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    init(&workspace);
    let exe = fixture::executable(temp.path());
    let file = temp.path().join("selection.json");
    let entry = selected(&exe, "one", "saved");
    write(&file, &selection(vec![entry.clone()]));
    let server = Server::new(|_, _| panic!("invalid state/admission contacted endpoint"));
    let req = request(&workspace, &file, &server.path, true);
    let state = workspace.join("state/session-recovery.json");
    for raw in [
        b"{".to_vec(),
        br#"{"schema_version":2,"revision":0,"attempts":[]}"#.to_vec(),
        br#"{"schema_version":1,"revision":0,"revision":1,"attempts":[]}"#.to_vec(),
    ] {
        fs::write(&state, raw).unwrap();
        assert!(recovery::inspect(&workspace, true).is_err());
        assert!(recovery::run(&req, true).is_err());
    }
    write(
        &state,
        &json!({"schema_version":1,"revision":128,"attempts":vec![json!({"entry":entry,"outcome":"uncertain"});128]}),
    );
    let mut new = selected(&exe, "new", "new");
    new["pane_id"] = json!("w2:p2");
    write(&file, &selection(vec![new]));
    let req = request(&workspace, &file, &server.path, true);
    // Use CLI: library workflow tests own their own process signal handler.
    let output = cli(&req).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("session_recovery_record_limit"));
    fs::remove_file(&state).unwrap();
    fs::write(workspace.join("state/execution.json"), b"{}").unwrap();
    // A native execution authority marker denies legacy interactive admission.
    fs::write(workspace.join("state/task-authority.json"), b"{}").unwrap();
    let req = request(&workspace, &file, &server.path, true);
    let output = cli(&req).output().unwrap();
    assert!(!output.status.success());
    assert!(!state.exists());
    assert!(server.requests().is_empty());
}

#[test]
fn all_adapters_use_typed_preview_and_invalid_later_entry_vetoes_whole_batch() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    init(&workspace);
    let exe = fixture::executable(temp.path());
    let file = temp.path().join("selection.json");
    let server = Server::new(|_, _| panic!("preview/invalid selection contacted endpoint"));
    let runtimes = [
        "djinn",
        "claude",
        "codex",
        "copilot",
        "devin",
        "droid",
        "kimi",
        "mastracode",
        "pi",
        "omp",
        "hermes",
        "opencode",
        "qodercli",
        "kilo",
        "cursor",
        "grok",
    ];
    let entries = runtimes
        .iter()
        .map(|runtime| {
            let mut entry = selected(&exe, runtime, "saved");
            entry["reference"]["runtime"] = json!(runtime);
            entry["reference"]["source"] = json!(format!("kitsune:{runtime}"));
            entry["pane_id"] = json!(format!("pane-{runtime}"));
            entry["expected_terminal_id"] = json!(format!("terminal-{runtime}"));
            entry
        })
        .collect();
    write(&file, &selection(entries));
    let req = request(&workspace, &file, &server.path, false);
    let before = fs::read_dir(workspace.join("state")).unwrap().count();
    let report = call(&req, 0);
    assert_eq!(report["entries"].as_array().unwrap().len(), 16);
    assert!(
        report["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["outcome"] == "preview")
    );
    assert_eq!(
        fs::read_dir(workspace.join("state")).unwrap().count(),
        before
    );
    assert!(!workspace.join("state/session-recovery.json").exists());
    for runtime in ["pi", "omp"] {
        let mut path = selected(&exe, "path", "/absolute/saved session");
        path["reference"]["runtime"] = json!(runtime);
        path["reference"]["source"] = json!(format!("kitsune:{runtime}"));
        path["reference"]["kind"] = json!("path");
        write(&file, &selection(vec![path]));
        assert_eq!(call(&req, 0)["entries"][0]["outcome"], "preview");
    }
    let valid = selected(&exe, "valid", "saved");
    let mut invalid = selected(&exe, "invalid", "another");
    invalid["reference"]["source"] = json!("kitsune:claude");
    write(&file, &selection(vec![valid, invalid]));
    let req = request(&workspace, &file, &server.path, true);
    assert!(recovery::run(&req, true).is_err());
    assert!(server.requests().is_empty());
    assert!(!workspace.join("state/session-recovery.json").exists());
}

#[test]
fn only_selected_commands_execute_and_interrupt_keeps_earlier_queue() {
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    init(&workspace);
    let exe = fixture::executable(temp.path());
    let file = temp.path().join("selection.json");
    let first = selected(&exe, "first", "first");
    let mut second = selected(&exe, "second", "second");
    second["pane_id"] = json!("w2:p2");
    second["expected_terminal_id"] = json!("terminal-2");
    let mut third = selected(&exe, "third", "third");
    third["pane_id"] = json!("w3:p3");
    third["expected_terminal_id"] = json!("terminal-3");
    write(&file, &selection(vec![first, second, third]));
    let child_pid = Arc::new(AtomicU32::new(0));
    let pid = child_pid.clone();
    let mut submissions = 0;
    let server = Server::new(move |_, req| {
        if req["method"] == "session.snapshot" {
            let mut inv = inventory();
            for number in 2..=4 {
                let mut pane = inv["panes"][0].clone();
                pane["pane_id"] = json!(format!("w{number}:p{number}"));
                pane["terminal_id"] = json!(format!("terminal-{number}"));
                inv["panes"].as_array_mut().unwrap().push(pane);
            }
            snapshot(req, inv)
        } else {
            submissions += 1;
            if submissions == 2 {
                while pid.load(Ordering::SeqCst) == 0 {
                    std::thread::yield_now();
                }
                assert_eq!(
                    unsafe { libc::kill(pid.load(Ordering::SeqCst) as i32, libc::SIGINT) },
                    0
                );
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            frame(ack(req))
        }
    });
    let req = request(&workspace, &file, &server.path, true);
    let child = cli(&req)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child_pid.store(child.id(), Ordering::SeqCst);
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(130), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["entries"][0]["outcome"], "queued");
    assert_eq!(report["entries"][1]["outcome"], "uncertain");
    assert_eq!(report["entries"][2]["outcome"], "unattempted_batch_stopped");
    let state = inspect(&workspace);
    assert_eq!(state["state"]["attempts"][0]["outcome"], "queued");
    assert_eq!(state["state"]["attempts"][1]["outcome"], "uncertain");
    assert_eq!(writes(&server), 2);
    assert!(
        server
            .requests()
            .iter()
            .filter(|r| r["method"] == "pane.submit_command")
            .all(|r| ["w1:p1", "w2:p2"].contains(&r["params"]["pane_id"].as_str().unwrap()))
    );
}

#[test]
fn state_symlink_and_admission_denied_before_workspace_lock_fail_closed() {
    use std::os::fd::AsRawFd;
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    init(&workspace);
    let exe = fixture::executable(temp.path());
    let file = temp.path().join("selection.json");
    write(&file, &selection(vec![selected(&exe, "one", "saved")]));
    let server = Server::new(|_, _| panic!("denied admission contacted endpoint"));
    let req = request(&workspace, &file, &server.path, true);
    // Hold transport exclusive while leaving the workspace lock absent. The child
    // must fail at admission, not create a workspace lock while waiting for it.
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(workspace.join("state/.runtime-transport.lock"))
        .unwrap();
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let workspace_lock = workspace.join("state/.workspace.lock");
    let before = fs::read(&workspace_lock).ok();
    let output = cli(&req).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("runtime_transport_gate_busy"));
    assert_eq!(fs::read(&workspace_lock).ok(), before);
    assert!(!workspace.join("state/session-recovery.json").exists());
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) }, 0);
    let outside = temp.path().join("outside.json");
    fs::write(&outside, b"{}").unwrap();
    std::os::unix::fs::symlink(&outside, workspace.join("state/session-recovery.json")).unwrap();
    assert!(recovery::inspect(&workspace, true).is_err());
    assert!(recovery::run(&req, true).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"{}");
    assert!(server.requests().is_empty());
}

#[test]
fn capacity_preflights_all_eligible_entries_before_contact_without_hidden_partial_execution() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    init(&workspace);
    let exe = fixture::executable(temp.path());
    let file = temp.path().join("selection.json");
    let state = workspace.join("state/session-recovery.json");
    let attempts: Vec<_> = (0..127)
        .map(|i| {
            json!({
                "entry":selected(&exe,&format!("old-{i}"),&format!("old-{i}")),"outcome":"queued"
            })
        })
        .collect();
    write(
        &state,
        &json!({"schema_version":1,"revision":254,"attempts":attempts}),
    );
    let before = fs::read(&state).unwrap();
    let first = selected(&exe, "first", "first");
    let mut second = selected(&exe, "second", "second");
    second["pane_id"] = json!("w2:p2");
    second["expected_terminal_id"] = json!("terminal-2");
    write(&file, &selection(vec![first.clone(), second]));
    let denied = Server::new(|_, _| panic!("insufficient whole-batch capacity contacted endpoint"));
    let req = request(&workspace, &file, &denied.path, true);
    let output = cli(&req).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("session_recovery_record_limit"));
    assert_eq!(fs::read(&state).unwrap(), before);
    assert!(denied.requests().is_empty());

    // Only eligible tuples consume capacity, not duplicates, imports, blocked
    // journal tuples or unsupported adapters. One available slot suffices.
    let mut duplicate = first.clone();
    duplicate["key"] = json!("duplicate");
    duplicate["pane_id"] = json!("w2:p2");
    duplicate["expected_terminal_id"] = json!("terminal-2");
    let mut imported = selected(&exe, "imported", "imported");
    imported["pane_id"] = json!("w3:p3");
    imported["expected_terminal_id"] = json!("terminal-3");
    let mut unsupported = selected(&exe, "unsupported", "unsupported");
    unsupported["reference"]["runtime"] = json!("unknown");
    unsupported["reference"]["source"] = json!("kitsune:unknown");
    unsupported["pane_id"] = json!("w4:p4");
    unsupported["expected_terminal_id"] = json!("terminal-4");
    let mut blocked = selected(&exe, "blocked", "old-0");
    blocked["pane_id"] = json!("w5:p5");
    blocked["expected_terminal_id"] = json!("terminal-5");
    let mut value = selection(vec![
        first,
        duplicate,
        imported.clone(),
        unsupported,
        blocked,
    ]);
    value["imported_references"] = json!([imported["reference"]]);
    write(&file, &value);
    let server = normal();
    let req = request(&workspace, &file, &server.path, true);
    let output = cli(&req).output().unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["entries"][0]["outcome"], "queued");
    assert_eq!(report["entries"][1]["outcome"], "skipped_duplicate");
    assert_eq!(report["entries"][2]["outcome"], "skipped_imported");
    assert_eq!(report["entries"][3]["outcome"], "adapter_unsupported");
    assert_eq!(report["entries"][4]["outcome"], "skipped_journal");
    assert_eq!(writes(&server), 1);
    assert_eq!(
        inspect(&workspace)["state"]["attempts"]
            .as_array()
            .unwrap()
            .len(),
        128
    );
}

fn reference_hint(reference: &Value) -> Value {
    json!({"source":reference["source"],"agent":reference["runtime"],
        "kind":reference["kind"],"value":reference["value"]})
}

fn extra_pane(id: usize, hint: Value) -> Value {
    let mut pane = inventory()["panes"][0].clone();
    pane["pane_id"] = json!(format!("w{id}:p{id}"));
    pane["terminal_id"] = json!(format!("terminal-{id}"));
    pane["agent_session"] = hint;
    pane
}

#[test]
fn existing_references_from_all_panes_veto_both_selection_orders_without_running_claim() {
    for reverse_selection in [false, true] {
        for hints_first in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let workspace = temp.path().join("workspace");
            init(&workspace);
            let exe = fixture::executable(temp.path());
            let file = temp.path().join("selection.json");
            let first = selected(&exe, "first", "same-literal");
            let mut second = selected(
                Path::new("/absent/other-provider"),
                "second",
                "same-literal",
            );
            second["reference"]["source"] = json!("kitsune:claude");
            second["reference"]["runtime"] = json!("claude");
            second["pane_id"] = json!("w2:p2");
            second["expected_terminal_id"] = json!("terminal-2");
            let mut entries = vec![first.clone(), second.clone()];
            if reverse_selection {
                entries.reverse();
            }
            write(&file, &selection(entries));
            let mut inv = inventory();
            // Persisted historical references need not belong to the selected
            // target, executable, foreground provider, or a currently live UI.
            let mut historical = extra_pane(3, reference_hint(&first["reference"]));
            historical["external"] = json!({"history":"saved, stale, unauthenticated"});
            let another = extra_pane(4, reference_hint(&second["reference"]));
            inv["panes"]
                .as_array_mut()
                .unwrap()
                .extend([historical, another]);
            if hints_first {
                inv["panes"].as_array_mut().unwrap().reverse();
            }
            inv["unrelated_optional_field"] = json!({"not":"authority"});
            let server = Server::new(move |_, req| {
                assert_eq!(req["method"], "session.snapshot");
                snapshot(req, inv.clone())
            });
            let preview = request(&workspace, &file, &server.path, false);
            let output = cli(&preview).output().unwrap();
            assert!(output.status.success(), "{output:?}");
            assert!(server.requests().is_empty());
            assert!(!workspace.join("state/session-recovery.json").exists());
            let req = request(&workspace, &file, &server.path, true);
            let output = cli(&req).output().unwrap();
            assert!(output.status.success(), "{output:?}");
            let report: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["native_running"], "unestablished");
            for row in report["entries"].as_array().unwrap() {
                assert_eq!(row["outcome"], "existing_reference_present");
                assert_eq!(row["native_running"], "unestablished");
                assert_eq!(row["delivery"], "not_attempted");
            }
            assert_eq!(server.requests().len(), 1);
            assert_eq!(writes(&server), 0);
            assert!(!workspace.join("state/session-recovery.json").exists());
            let mut explicit: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
            explicit["imported_references"] = json!([first["reference"]]);
            write(&file, &explicit);
            let output = cli(&request(&workspace, &file, &server.path, true))
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            let report: Value = serde_json::from_slice(&output.stdout).unwrap();
            for row in report["entries"].as_array().unwrap() {
                assert_eq!(
                    row["outcome"],
                    if row["reference"] == first["reference"] {
                        "skipped_imported"
                    } else {
                        "existing_reference_present"
                    }
                );
            }
            assert_eq!(server.requests().len(), 2);
            assert_eq!(writes(&server), 0);
            assert!(!workspace.join("state/session-recovery.json").exists());
        }
    }
}

#[test]
fn present_hints_match_exact_source_provider_kind_and_literal_only() {
    for difference in ["source", "agent", "kind", "value"] {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        init(&workspace);
        let exe = fixture::executable(temp.path());
        let file = temp.path().join("selection.json");
        let mut entry = selected(&exe, "one", "/literal/saved");
        entry["reference"]["source"] = json!("kitsune:pi");
        entry["reference"]["runtime"] = json!("pi");
        let mut hint = reference_hint(&entry["reference"]);
        hint[difference] = json!(match difference {
            "source" => "external:history",
            "agent" => "djinn",
            "kind" => "path",
            _ => "/other/saved",
        });
        write(&file, &selection(vec![entry]));
        let mut inv = inventory();
        inv["panes"]
            .as_array_mut()
            .unwrap()
            .push(extra_pane(2, hint));
        let server = Server::new(move |_, req| {
            if req["method"] == "session.snapshot" {
                snapshot(req, inv.clone())
            } else {
                frame(ack(req))
            }
        });
        let req = request(&workspace, &file, &server.path, true);
        let output = cli(&req).output().unwrap();
        assert!(output.status.success(), "{difference}: {output:?}");
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["entries"][0]["outcome"], "queued");
        assert_eq!(writes(&server), 1);
        assert_eq!(
            server
                .requests()
                .iter()
                .filter(|r| r["method"] == "session.snapshot")
                .count(),
            3
        );
    }
}

#[test]
fn malformed_present_hints_and_failed_snapshot_stop_whole_batch_without_journal_or_send() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    init(&workspace);
    let exe = fixture::executable(temp.path());
    let file = temp.path().join("selection.json");
    let first = selected(&exe, "first", "first");
    let mut second = selected(&exe, "second", "second");
    second["pane_id"] = json!("w2:p2");
    second["expected_terminal_id"] = json!("terminal-2");
    write(&file, &selection(vec![first.clone(), second]));
    let valid = reference_hint(&first["reference"]);
    let mut hints = vec![Value::Null, json!(false), json!("wrong"), json!({})];
    for (field, value) in [
        ("source", json!("")),
        ("agent", json!(32)),
        ("kind", json!("future")),
        ("value", json!("--help")),
        ("value", json!("bad\nvalue")),
        ("value", json!("x".repeat(513))),
    ] {
        let mut hint = valid.clone();
        hint[field] = value;
        hints.push(hint);
    }
    for hint in hints {
        let mut inv = inventory();
        inv["panes"][0]["agent_session"] = valid.clone();
        inv["panes"]
            .as_array_mut()
            .unwrap()
            .push(extra_pane(3, hint));
        let server = Server::new(move |_, req| {
            assert_eq!(req["method"], "session.snapshot");
            snapshot(req, inv.clone())
        });
        assert_reference_read_failure(&workspace, &file, &server);
    }
    for failure in [
        "lost_ack",
        "id",
        "protocol",
        "command_submission",
        "type",
        "error",
    ] {
        let server = Server::new(move |_, req| {
            assert_eq!(req["method"], "session.snapshot");
            if failure == "lost_ack" {
                return vec![];
            }
            let mut inv = inventory();
            if failure == "protocol" {
                inv["protocol"] = json!(17);
            }
            if failure == "command_submission" {
                inv["command_submission"] = json!({"version":2});
            }
            let mut response =
                json!({"id":req["id"],"result":{"type":"session_snapshot","snapshot":inv}});
            if failure == "id" {
                response["id"] = json!("uncorrelated");
            }
            if failure == "type" {
                response["result"]["type"] = json!("other");
            }
            if failure == "error" {
                response.as_object_mut().unwrap().remove("result");
                response["error"] = json!({"code":"unavailable","message":"fixture"});
            }
            frame(response)
        });
        assert_reference_read_failure(&workspace, &file, &server);
    }
    let req = request(&workspace, &file, Path::new("/absent/api.sock"), true);
    let output = cli(&req).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!workspace.join("state/session-recovery.json").exists());
}

fn assert_reference_read_failure(workspace: &Path, file: &Path, server: &Server) {
    let req = request(workspace, file, &server.path, true);
    let output = cli(&req).output().unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["outcome"], "batch_stopped");
    assert!(report["reference_read_error"].is_string());
    assert_eq!(report["recovery_revision"], req.expect_recovery_revision);
    assert!(
        report["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["outcome"] == "unattempted_reference_read_failed"
                && row["delivery"] == "not_attempted")
    );
    assert_eq!(server.requests().len(), 1);
    assert_eq!(writes(server), 0);
    assert!(!workspace.join("state/session-recovery.json").exists());
}
