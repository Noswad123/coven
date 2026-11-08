//! Counterparts of command_send/send_to_agent_pane/notify and legacy down:
//! preserve ordered exact IDs, lookup argv, missing counts, text-before-Enter,
//! history only after complete delivery, no stale-load authority or fallback.
use super::*;
use crate::runtime_command::{Output, Request, Status};
use std::{cell::RefCell, collections::VecDeque, fs};

struct Fake {
    outputs: RefCell<VecDeque<(i32, String)>>,
    calls: RefCell<Vec<Vec<String>>>,
    after_call: Option<fn(&Request, usize)>,
}
impl Fake {
    fn new(outputs: Vec<(i32, String)>) -> Self {
        Self {
            outputs: RefCell::new(outputs.into()),
            calls: RefCell::new(Vec::new()),
            after_call: None,
        }
    }
}
impl Runner for Fake {
    fn run(&self, request: &Request) -> Output {
        assert!(Path::new(&request.argv[0]).is_absolute());
        assert!(!request.timeout.is_zero() && request.timeout <= Duration::from_secs(60));
        assert_eq!(request.max_output_bytes, 1_048_576);
        let root = Path::new(&request.env["HOME"]);
        let mut expected: std::collections::BTreeMap<_, _> = std::env::vars().collect();
        expected.insert("HOME".into(), root.to_string_lossy().into_owned());
        assert_eq!(request.env, expected);
        assert_eq!(request.cwd, std::env::current_dir().unwrap());
        // Lookup, text, Enter and stop all permit the native child to take the
        // workspace lock (e.g. a backend wrapper recording its own state).
        let workspace = WorkspaceGuard::acquire(root, Duration::ZERO)
            .expect("native I/O must not hold workspace exclusion");
        drop(workspace);
        // Every backend call is fenced; no enable may pass this point.
        assert_eq!(
            TransportGuard::exclusive(root, Duration::ZERO)
                .err()
                .unwrap()
                .code(),
            "runtime_transport_gate_busy"
        );
        self.calls.borrow_mut().push(request.argv[1..].to_vec());
        if let Some(after_call) = self.after_call {
            after_call(request, self.calls.borrow().len());
        }
        let (code, body) = self
            .outputs
            .borrow_mut()
            .pop_front()
            .expect("unexpected backend call");
        Output {
            status: Status::Exited(code),
            stdout: body.into_bytes(),
            stderr: Vec::new(),
        }
    }
}
fn root_fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("coven.json"), br#"{"name":"My Team","agents":["Beta","alpha"],"display_names":{"Beta":"Display Beta"},"multiplexer":"herdr"}"#).unwrap();
    temp
}
fn run_send(
    root: &Path,
    selector: &str,
    text: &str,
    fake: &Fake,
    tmux: bool,
) -> Result<CommandResult> {
    send_using(root, selector, text, |root, _| {
        Ok(transport::Backend::injected(root, tmux, fake))
    })
}
fn json(value: serde_json::Value) -> (i32, String) {
    (0, value.to_string())
}
fn herdr(pane: &str) -> Vec<(i32, String)> {
    vec![
        json(
            serde_json::json!({"result":{"workspaces":[{"label":"other","workspace_id":"other"},{"label":"coven-my-team","workspace_id":"w1"}]}}),
        ),
        json(
            serde_json::json!({"result":{"tabs":[{"label":"agent-Beta","workspace_id":"w1","tab_id":"t1"}]}}),
        ),
        json(serde_json::json!({"result":{"panes":[{"tab_id":"t1","pane_id":pane}]}})),
        (0, String::new()),
        (0, String::new()),
    ]
}

#[test]
fn characterization_roster_order_exact_ids_missing_and_history() {
    let root = root_fixture();
    let mut outputs = herdr("p1");
    outputs.push(json(serde_json::json!({"result":{"workspaces":[]}})));
    let fake = Fake::new(outputs);
    let result = run_send(root.path(), "all", " hello\n世界 ", &fake, false).unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        result.body,
        "[INFO] No running pane found for: alpha\n[INFO] Sent message to 1 agent(s)"
    );
    assert_eq!(
        *fake.calls.borrow(),
        vec![
            vec!["workspace", "list"],
            vec!["tab", "list", "--workspace", "w1"],
            vec!["pane", "list", "--workspace", "w1"],
            vec!["pane", "send-text", "p1", " hello\n世界 "],
            vec!["pane", "send-keys", "p1", "enter"],
            vec!["workspace", "list"],
        ]
    );
    let messages = logs::read(root.path(), LogKind::Messages).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0].as_object().unwrap().get("to").unwrap().as_str(),
        Some("Beta")
    );
    assert_eq!(
        messages[0]
            .as_object()
            .unwrap()
            .get("body")
            .unwrap()
            .as_str(),
        Some(" hello\n世界 ")
    );
    let events = logs::read(root.path(), LogKind::Events).unwrap();
    assert_eq!(
        events
            .iter()
            .map(|v| v
                .as_object()
                .unwrap()
                .get("type")
                .unwrap()
                .as_str()
                .unwrap())
            .collect::<Vec<_>>(),
        ["message.sent", "agent.message.sent"]
    );
}

#[test]
fn characterization_enter_failure_never_records_delivery_or_retries() {
    let root = root_fixture();
    let mut outputs = herdr("p1");
    outputs.last_mut().unwrap().0 = 1;
    let fake = Fake::new(outputs);
    assert_eq!(
        run_send(root.path(), "Beta", "text", &fake, false)
            .unwrap_err()
            .code(),
        "agent_pane_delivery_failed"
    );
    assert_eq!(fake.calls.borrow().len(), 5);
    assert!(!root.path().join("logs/messages.jsonl").exists());
    assert!(!root.path().join("logs/events.jsonl").exists());
    assert!(!root.path().join("dashboard.md").exists());
}

#[test]
fn characterization_tmux_first_pane_and_single_text_enter_command() {
    let root = root_fixture();
    let fake = Fake::new(vec![(0, "%12\n%13\n".into()), (0, String::new())]);
    run_send(root.path(), "Beta", "text", &fake, true).unwrap();
    assert_eq!(
        *fake.calls.borrow(),
        vec![
            vec![
                "list-panes",
                "-t",
                "coven-my-team:agent-Beta",
                "-F",
                "#{pane_id}"
            ],
            vec!["send-keys", "-t", "%12", "text", "Enter"],
        ]
    );
}

#[test]
fn all_deterministic_inputs_admitted_before_lookup() {
    for (path, bytes) in [
        ("logs/messages.jsonl", "bad\n"),
        ("logs/events.jsonl", "bad\n"),
        ("state/agents.json", "{}"),
        ("goal.md", "\0"),
    ] {
        let root = root_fixture();
        fs::create_dir_all(root.path().join(Path::new(path).parent().unwrap())).unwrap();
        // NUL in a goal is valid text; invalid UTF-8 is not.
        fs::write(
            root.path().join(path),
            if path == "goal.md" {
                &[0xff][..]
            } else {
                bytes.as_bytes()
            },
        )
        .unwrap();
        let fake = Fake::new(vec![]);
        assert!(
            run_send(root.path(), "all", "text", &fake, false).is_err(),
            "{path}"
        );
        assert!(fake.calls.borrow().is_empty());
    }
    let root = root_fixture();
    for selector in ["beta", "Display Beta", "../Beta"] {
        let fake = Fake::new(vec![]);
        assert_eq!(
            run_send(root.path(), selector, "text", &fake, false)
                .unwrap_err()
                .code(),
            "unknown_agent"
        );
    }
    assert!(run_send(root.path(), "all", "bad\0text", &Fake::new(vec![]), false).is_err());
}

fn enable_control(root: &Path) {
    let location = platform::workspace_location(root).unwrap();
    fs::create_dir_all(root.join("state")).unwrap();
    fs::write(root.join(CONTROL), serde_json::to_vec(&serde_json::json!({
        "version":1,"action":"runtime_launch","workspace_id":"a".repeat(32),
        "epoch_id":"b".repeat(32),"location":{"root":location.root,"device":location.device,"inode":location.inode},
        "head":null,"claims":{}
    })).unwrap()).unwrap();
}

#[test]
fn stale_team_snapshot_never_grants_authority_and_stop_ignores_mutable_config() {
    let root = root_fixture();
    assert_eq!(
        notice_targets(root.path(), "all").unwrap(),
        ["Beta", "alpha"]
    );
    workspace::load(root.path()).unwrap();
    enable_control(root.path());
    let fake = Fake::new(vec![]);
    assert_eq!(
        notice_with(root.path(), "Beta", "text", &fake)
            .unwrap_err()
            .code(),
        "legacy_runtime_disabled"
    );
    assert_eq!(
        run_send(root.path(), "all", "text", &fake, false)
            .unwrap_err()
            .code(),
        "legacy_runtime_disabled"
    );
    fs::write(root.path().join("coven.toml"), "not valid toml [").unwrap();
    let result = down_with(root.path(), &fake).unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.body.contains("No scoped launch attempt recorded"));
    fs::write(root.path().join(CONTROL), "{}").unwrap();
    assert!(down_with(root.path(), &fake).is_err());
    fs::remove_file(root.path().join(CONTROL)).unwrap();
    fs::create_dir(root.path().join("state/actions")).unwrap();
    assert_eq!(
        down_with(root.path(), &fake).unwrap_err().code(),
        "action_orphan_bundles"
    );
    assert!(fake.calls.borrow().is_empty());
}

#[test]
fn unsafe_backend_handles_never_become_options_or_effect_targets() {
    let root = root_fixture();
    for pane in ["--all", "bad:id", "p\n1", "-target"] {
        let fake = Fake::new(herdr(pane));
        assert_eq!(
            run_send(root.path(), "Beta", "text", &fake, false)
                .unwrap_err()
                .code(),
            "invalid_runtime_handle"
        );
        assert_eq!(fake.calls.borrow().len(), 3);
    }
}

#[test]
fn characterization_down_scopes_human_namespace_and_never_stops_unrelated_workspace() {
    let root = root_fixture();
    let fake = Fake::new(vec![
        json(
            serde_json::json!({"result":{"workspaces":[{"label":"other","workspace_id":"wrong"},{"label":"coven-my-team","workspace_id":"right"}]}}),
        ),
        (0, String::new()),
    ]);
    let result = down_using(root.path(), |root, _| {
        Ok(transport::Backend::injected(root, false, &fake))
    })
    .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        *fake.calls.borrow(),
        vec![
            vec!["workspace", "list"],
            vec!["workspace", "close", "right"]
        ]
    );
    let absent = Fake::new(vec![json(
        serde_json::json!({"result":{"workspaces":[{"label":"other","workspace_id":"wrong"}]}}),
    )]);
    let result = down_using(root.path(), |root, _| {
        Ok(transport::Backend::injected(root, false, &absent))
    })
    .unwrap();
    assert_eq!(
        result.body,
        "[INFO] Coven Herdr workspace is not running: coven-my-team"
    );
    assert_eq!(absent.calls.borrow().len(), 1);
    let tmux = Fake::new(vec![(0, String::new()), (7, String::new())]);
    let result = down_using(root.path(), |root, _| {
        Ok(transport::Backend::injected(root, true, &tmux))
    })
    .unwrap();
    assert_eq!(result.exit_code, 7);
    assert_eq!(
        *tmux.calls.borrow(),
        vec![
            vec!["has-session", "-t", "coven-my-team"],
            vec!["kill-session", "-t", "coven-my-team"]
        ]
    );
}

#[test]
fn mode_published_while_waiting_for_admission_is_rechecked_before_workspace_load() {
    let root = root_fixture();
    workspace::load(root.path()).unwrap();
    let gate = TransportGuard::exclusive(root.path(), TIMEOUT).unwrap();
    let path = root.path().to_owned();
    let (started, waiting) = std::sync::mpsc::channel();
    let child = std::thread::spawn(move || {
        started.send(()).unwrap();
        notice_with(&path, "Beta", "text", &Fake::new(vec![]))
            .unwrap_err()
            .code()
            .to_owned()
    });
    waiting.recv().unwrap();
    enable_control(root.path());
    // Mode must win over broken mutable config after admission, not be inferred
    // from an earlier team snapshot or from a load failure.
    fs::write(root.path().join("coven.toml"), "broken [").unwrap();
    drop(gate);
    assert_eq!(child.join().unwrap(), "legacy_runtime_disabled");
}

#[test]
fn relocated_control_and_invalid_checkpoint_prevent_effects() {
    let root = root_fixture();
    enable_control(root.path());
    let mut control: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join(CONTROL)).unwrap()).unwrap();
    control["location"]["root"] = serde_json::json!("/different/location");
    fs::write(root.path().join(CONTROL), control.to_string()).unwrap();
    assert!(down_with(root.path(), &Fake::new(vec![])).is_err());
    fs::remove_file(root.path().join(CONTROL)).unwrap();
    fs::create_dir(root.path().join("checkpoints")).unwrap();
    fs::write(
        root.path().join("checkpoints/plan-approval.md"),
        "<!-- coven-checkpoint-bad -->",
    )
    .unwrap();
    // A malformed recognized decision block is rejected in dashboard preflight.
    let fake = Fake::new(vec![]);
    if run_send(root.path(), "Beta", "text", &fake, false).is_ok() {
        panic!("invalid checkpoint admitted");
    }
    assert!(fake.calls.borrow().is_empty());
}

#[test]
fn characterization_notice_is_transport_only_with_no_workspace_lock_or_history() {
    let root = root_fixture();
    // A notifier must not hold or acquire workspace exclusion across runtime I/O.
    // Its caller owns the separate post-delivery checkpoint.notice.sent append.
    let fake = Fake::new(herdr("p1"));
    assert!(
        notice_using(root.path(), "Beta", "notice", |root, _| {
            Ok(transport::Backend::injected(root, false, &fake))
        })
        .unwrap()
    );
    assert_eq!(fake.calls.borrow().len(), 5);
    assert!(!root.path().join("logs/messages.jsonl").exists());
    assert!(!root.path().join("logs/events.jsonl").exists());
}

#[test]
fn delivered_message_with_changed_workspace_reports_history_failure_without_retry() {
    let root = root_fixture();
    let mut fake = Fake::new(herdr("p1"));
    fake.after_call = Some(|request, count| {
        if count == 5 {
            let root = Path::new(&request.env["HOME"]);
            let _guard = WorkspaceGuard::acquire(root, Duration::ZERO).unwrap();
            fs::write(root.join("state/agents.json"), "{}").unwrap();
        }
    });
    let result = run_send(root.path(), "all", "text", &fake, false).unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(
        result
            .body
            .contains("Message delivered to Beta; history update failed")
    );
    assert!(result.body.contains("no retry performed"));
    assert!(!result.body.ends_with('\n'));
    assert_eq!(fake.calls.borrow().len(), 5);
    assert!(!root.path().join("logs/messages.jsonl").exists());
    assert!(!root.path().join("logs/events.jsonl").exists());
}

#[test]
fn dashboard_failure_after_delivery_preserves_history_without_retry() {
    let root = root_fixture();
    let mut fake = Fake::new(herdr("p1"));
    fake.after_call = Some(|request, count| {
        if count == 5 {
            fs::write(Path::new(&request.env["HOME"]).join("goal.md"), [0xff]).unwrap();
        }
    });
    let result = run_send(root.path(), "Beta", "text", &fake, false).unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(
        result
            .body
            .contains("Delivery completed for 1 agent(s); dashboard update failed")
    );
    assert!(!result.body.ends_with('\n'));
    assert_eq!(fake.calls.borrow().len(), 5);
    assert_eq!(logs::read(root.path(), LogKind::Messages).unwrap().len(), 1);
    assert_eq!(logs::read(root.path(), LogKind::Events).unwrap().len(), 2);
}

#[test]
fn legacy_down_bodies_never_have_a_terminal_newline() {
    let root = root_fixture();
    let absent = Fake::new(vec![(1, String::new())]);
    let result = down_using(root.path(), |root, _| {
        Ok(transport::Backend::injected(root, true, &absent))
    })
    .unwrap();
    assert_eq!(
        result.body,
        "[INFO] Coven tmux session is not running: coven-my-team"
    );
    let stopped = Fake::new(vec![(0, String::new()), (0, "stopped\n\n".into())]);
    let result = down_using(root.path(), |root, _| {
        Ok(transport::Backend::injected(root, true, &stopped))
    })
    .unwrap();
    assert_eq!(result.body, "stopped");
}
