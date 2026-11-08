use super::*;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
};

fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("coven.json"), "invalid team JSON").unwrap();
    std::fs::create_dir(root.path().join("state")).unwrap();
    coven::group_commands::create(root.path(), "g", None, None, true).unwrap();
    root
}

fn run(tail: &[&str]) -> coven::task_mutation::TaskOutput {
    let (cli, _) = parse(&args(&[&["coven"][..], tail].concat())).unwrap();
    super::super::super::run(cli).unwrap()
}

fn snapshot() -> Value {
    let pane = json!({"pane_id":"p","terminal_id":"t","workspace_id":"w","tab_id":"tab",
        "focused":false,"agent_status":"idle","revision":1,"agent":"djinn","agent_session":null});
    json!({"version":"test","protocol":18,"panes":[pane.clone()],"agents":[pane],
        "workspaces":[],"tabs":[],"layouts":[]})
}

fn socket(
    replies: Vec<Value>,
    methods: Vec<&'static str>,
) -> (tempfile::TempDir, String, std::thread::JoinHandle<()>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("api.sock");
    let listener = UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let thread = std::thread::spawn(move || {
        for (reply, method) in replies.into_iter().zip(methods) {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], method);
            let response = json!({"id":request["id"],"result":reply});
            writeln!(stream, "{response}").unwrap();
        }
    });
    (dir, path.to_str().unwrap().into(), thread)
}

#[test]
fn group_add_routes_use_fresh_selected_evidence_in_both_modes() {
    for process in [false, true] {
        let root = workspace();
        let snap = json!({"type":"session_snapshot","snapshot":snapshot()});
        let (replies, methods) = if process {
            (
                vec![
                    snap.clone(),
                    json!({"type":"pane_process_info","process_info":{"pane_id":"p",
                "foreground_processes":[{"pid":7,"name":"djinn","argv":["/bin/djinn"]}]}}),
                    snap,
                ],
                vec!["session.snapshot", "pane.process_info", "session.snapshot"],
            )
        } else {
            (vec![snap], vec!["session.snapshot"])
        };
        let (_socket_dir, socket, server) = socket(replies, methods);
        let mut tail = vec![
            "group",
            "add",
            root.path().to_str().unwrap(),
            "g",
            "--member",
            "m",
            "--role",
            "review",
            "--socket",
            &socket,
            "--pane",
            "p",
            "--json",
        ];
        if process {
            tail.push("--process-evidence");
        }
        let output = run(&tail);
        server.join().unwrap();
        assert_eq!(output.exit_code, 0);
        let body: Value = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(
            body["member"]["binding"]["presentation"]["pane_id_at_binding"],
            "p"
        );
        assert_eq!(
            body["member"]["binding"]["evidence"]["provenance"],
            if process {
                "kitsune.pane.process_info"
            } else {
                "kitsune.session.snapshot"
            }
        );
        assert_eq!(body["authorization"], "none");
    }
}

#[test]
fn group_add_rejects_malformed_publication_annotations_before_mutation() {
    let root = workspace();
    let before = std::fs::read(root.path().join("state/groups.json")).unwrap();
    let mut snapshot = snapshot();
    snapshot["annotations"] = json!({"unexpected":"not valid publication annotations"});
    let (_dir, socket, server) = socket(
        vec![json!({"type":"session_snapshot","snapshot":snapshot})],
        vec!["session.snapshot"],
    );
    let (cli, _) = parse(&args(&[
        "coven",
        "group",
        "add",
        root.path().to_str().unwrap(),
        "g",
        "--member",
        "m",
        "--role",
        "role",
        "--socket",
        &socket,
        "--pane",
        "p",
    ]))
    .unwrap();
    assert_eq!(
        super::super::super::run(cli).err().unwrap().code(),
        "groups_observation_failed"
    );
    server.join().unwrap();
    assert_eq!(
        std::fs::read(root.path().join("state/groups.json")).unwrap(),
        before
    );
}

#[test]
fn kitsune_observe_maps_observed_incompatible_and_unavailable() {
    for (protocol, code) in [(18, 0), (19, 2)] {
        let mut snapshot = snapshot();
        snapshot["protocol"] = json!(protocol);
        let (_dir, socket, server) = socket(
            vec![json!({"type":"session_snapshot","snapshot":snapshot})],
            vec!["session.snapshot"],
        );
        let output = run(&["observe", "kitsune", "--socket", &socket, "--json"]);
        server.join().unwrap();
        assert_eq!(output.exit_code, code);
        assert!(output.stdout.is_ascii());
        assert!(!output.stdout.contains('\n'));
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("absent.sock");
    assert_eq!(
        run(&["observe", "kitsune", "--socket", path.to_str().unwrap()]).exit_code,
        1
    );
}

const RUNTIME: &str = "616fed3e-ae7f-41a0-99e5-80546d659d76";
const INSTANCE: &str = "e3c0e2ce-544c-4f1e-a489-5d3e0cf1978e";

fn live_value() -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    json!({"schema_version":1,"contract":"djinn.live-observation.v1","runtime_id":RUNTIME,"sequence":1,
        "sampled_at_ms":now,"valid_for_ms":3000,"source":"legacy_session_run_state",
        "activity_scope":"legacy_runner_ownership","capabilities":{"activity":true,"idle":false,
        "catalog_complete":false,"decision_state":false,"retry_state":false,"durable_lifecycle":false,"job_completion":false},
        "authorization":"none","dispatchable":false,"native_lifecycle":"unestablished","job_completion":"unestablished",
        "instances":[{"instance_id":INSTANCE,"sessions":[{"session_id":"selected","activity":"unknown"}]}]})
}

#[test]
fn group_live_routes_preserve_raw_authority_and_status_unknown_is_success() {
    let root = workspace();
    let binding = coven::values::parse(br#"{"presentation":{"adapter":"kitsune","endpoint":{"path":"/not/socket","device":184467440737095516160000000000000000000,"inode":0},"terminal_id":"t","pane_id_at_binding":"p"},"evidence":{"provenance":"kitsune.session.snapshot","observed_at":"2024-W01T12Z","agent_kind_hint":null,"native_reference":null,"native_binding":"unestablished"}}"#, coven::values::Profile::Authority).unwrap();
    coven::groups::add_member(
        root.path(),
        coven::groups::AddMember {
            group: "g",
            member: "m",
            role: "role",
            binding: &binding,
            rebind: false,
            expected_revision: None,
        },
    )
    .unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::set_permissions(source.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let file = source.path().join(format!("{RUNTIME}.json"));
    std::fs::write(&file, serde_json::to_vec(&live_value()).unwrap()).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let workspace = root.path().to_str().unwrap();
    let file = file.to_str().unwrap();
    let output = run(&[
        "group",
        "attach-live",
        workspace,
        "g",
        "--member",
        "m",
        "--file",
        file,
        "--runtime",
        RUNTIME,
        "--instance",
        INSTANCE,
        "--session",
        "selected",
        "--json",
    ]);
    assert_eq!(output.exit_code, 0);
    assert!(
        output
            .stdout
            .contains("184467440737095516160000000000000000000")
    );
    let status = run(&["group", "status", workspace, "g", "--member", "m", "--json"]);
    assert_eq!(status.exit_code, 0);
    let value: Value = serde_json::from_str(&status.stdout).unwrap();
    assert_eq!(value["participants"][0]["assessment"], "observed");
    assert_eq!(value["participants"][0]["activity"], "unknown");
    assert!(
        !status
            .stdout
            .contains("184467440737095516160000000000000000000")
    );
    let output = run(&["group", "detach-live", workspace, "g", "--member", "m"]);
    assert_eq!(output.exit_code, 0);
    assert!(
        output
            .stdout
            .contains("184467440737095516160000000000000000000")
    );
    assert!(output.stdout.contains("\"live_attachment\": null"));
    assert_eq!(
        run(&["group", "remove", workspace, "g", "--member", "m"]).exit_code,
        0
    );
}

#[test]
fn live_observe_and_publish_operational_reports_keep_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let file = dir.path().join(format!("{RUNTIME}.json"));
    let tail = [
        "observe",
        "djinn-live",
        "--file",
        file.to_str().unwrap(),
        "--runtime",
        RUNTIME,
        "--instance",
        INSTANCE,
        "--session",
        "selected",
        "--json",
    ];
    assert_eq!(run(&tail).exit_code, 1);
    for (contract, code) in [("djinn.live-observation.v1", 0), ("unsupported.v2", 2)] {
        let mut value = live_value();
        value["contract"] = json!(contract);
        std::fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(run(&tail).exit_code, code);
    }
    let root = workspace();
    let absent = dir.path().join("missing.sock");
    let output = run(&[
        "group",
        "publish",
        root.path().to_str().unwrap(),
        "g",
        "--socket",
        absent.to_str().unwrap(),
        "--source",
        "coven",
        "--duration",
        "1",
        "--json",
    ]);
    assert_eq!(output.exit_code, 1);
    assert!(output.stdout.contains("\"outcome\":\"failed\""));
}
