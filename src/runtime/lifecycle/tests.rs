//! Named characterizations of coven.py 2334-2420, 3099-3146, 3464-3700.
use super::*;
use crate::runtime_command::{Output, Request, Runner, Status};
use std::{cell::RefCell, collections::VecDeque, fs, time::Duration};

pub(crate) mod preflight;

pub(crate) struct Fake {
    pub calls: RefCell<Vec<Vec<String>>>,
    outputs: RefCell<VecDeque<Output>>,
    pub after: Option<fn(&Path, usize)>,
    root: std::path::PathBuf,
}
impl Fake {
    pub fn new(root: &Path, outputs: Vec<(i32, String)>) -> Self {
        Self {
            root: root.to_owned(),
            calls: RefCell::new(Vec::new()),
            outputs: RefCell::new(
                outputs
                    .into_iter()
                    .map(|(code, stdout)| Output {
                        status: Status::Exited(code),
                        stdout: stdout.into_bytes(),
                        stderr: Vec::new(),
                    })
                    .collect(),
            ),
            after: None,
        }
    }
    pub fn fail_last(&self, status: Status) {
        self.outputs.borrow_mut().back_mut().unwrap().status = status;
    }
    pub fn assert_done(&self) {
        assert!(self.outputs.borrow().is_empty());
    }
}
impl Runner for Fake {
    fn run(&self, request: &Request) -> Output {
        assert!(Path::new(&request.argv[0]).is_absolute());
        assert!(!request.timeout.is_zero() && request.timeout <= Duration::from_secs(60));
        assert_eq!(request.max_output_bytes, 1_048_576);
        drop(
            WorkspaceGuard::acquire(&self.root, Duration::ZERO)
                .expect("no native I/O under workspace lock"),
        );
        assert!(
            TransportGuard::exclusive(&self.root, Duration::ZERO).is_err(),
            "shared admission must cover every native call"
        );
        self.calls.borrow_mut().push(request.argv[1..].to_vec());
        if let Some(after) = self.after {
            after(&self.root, self.calls.borrow().len());
        }
        self.outputs
            .borrow_mut()
            .pop_front()
            .expect("unexpected native call")
    }
}
pub(crate) fn fixture(multiplexer: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("coven.json"),
        format!(r#"{{"name":"Test","agents":["Beta","alpha"],"multiplexer":"{multiplexer}"}}"#),
    )
    .unwrap();
    fs::create_dir(root.path().join("prompts")).unwrap();
    root
}
pub(crate) fn ok() -> (i32, String) {
    (0, String::new())
}
pub(crate) fn json(value: serde_json::Value) -> (i32, String) {
    (0, serde_json::json!({"result":value}).to_string())
}
struct Foreground {
    root: std::path::PathBuf,
    calls: RefCell<Vec<Vec<String>>>,
}
impl runtime_foreground::Runner for Foreground {
    fn run(&self, request: &runtime_foreground::Request) -> Status {
        drop(WorkspaceGuard::acquire(&self.root, Duration::ZERO).unwrap());
        assert!(TransportGuard::exclusive(&self.root, Duration::ZERO).is_err());
        self.calls.borrow_mut().push(request.argv[1..].to_vec());
        Status::Exited(23)
    }
}
fn run(
    root: &Path,
    action: RuntimeAction,
    fake: &Fake,
    tmux: bool,
    tty: bool,
    in_tmux: bool,
) -> (Result<CommandResult>, Foreground) {
    let foreground = Foreground {
        root: root.to_owned(),
        calls: RefCell::new(Vec::new()),
    };
    let result = lifecycle_using(
        root,
        action,
        |r, _| {
            // Selection checks an existing image; neither runner executes it.
            // This also exercises foreground's real unbounded PATH selector.
            if tty {
                transport::Backend::new(std::env::current_exe().unwrap().to_str().unwrap(), fake)
            } else {
                Ok(transport::Backend::injected(r, tmux, fake))
            }
        },
        &foreground,
        (tty, tty),
        in_tmux,
    );
    (result, foreground)
}

#[test]
fn characterization_tmux_startup_transcript_required_vs_ignored_failures() {
    let root = fixture("tmux");
    let _gate = TransportGuard::shared(root.path(), TIMEOUT).unwrap();
    let outputs = vec![
        ok(),
        (8, "".into()),
        (0, "%1\n".into()),
        (8, "".into()),
        (0, "%2".into()),
        ok(),
        (0, "".into()),
        (8, "".into()),
        ok(),
        (8, "".into()),
    ];
    let fake = Fake::new(root.path(), outputs);
    let backend = transport::Backend::injected(root.path(), true, &fake);
    create::tmux(
        root.path(),
        "coven-test",
        &[("Beta".into(), "launch".into())],
        &[
            ("events", "events".into()),
            ("messages", "messages".into()),
            ("shell", "shell".into()),
        ],
        &backend,
    )
    .unwrap();
    fake.assert_done();
    let cwd = root.path().to_str().unwrap();
    assert_eq!(
        *fake.calls.borrow(),
        vec![
            vec![
                "new-session",
                "-d",
                "-s",
                "coven-test",
                "-n",
                "orchestrator",
                "-c",
                cwd,
                create::DASHBOARD
            ],
            vec![
                "select-pane",
                "-t",
                "coven-test:orchestrator.0",
                "-T",
                "dashboard"
            ],
            vec![
                "split-window",
                "-t",
                "coven-test:orchestrator",
                "-c",
                cwd,
                "-P",
                "-F",
                "#{pane_id}",
                "events"
            ],
            vec!["select-pane", "-t", "%1", "-T", "events"],
            vec![
                "split-window",
                "-t",
                "coven-test:orchestrator",
                "-c",
                cwd,
                "-P",
                "-F",
                "#{pane_id}",
                "messages"
            ],
            vec!["select-pane", "-t", "%2", "-T", "messages"],
            vec![
                "split-window",
                "-t",
                "coven-test:orchestrator",
                "-c",
                cwd,
                "-P",
                "-F",
                "#{pane_id}",
                "shell"
            ],
            vec!["select-layout", "-t", "coven-test:orchestrator", "tiled"],
            vec![
                "new-window",
                "-t",
                "coven-test",
                "-n",
                "agent-Beta",
                "-c",
                cwd,
                "launch"
            ],
            vec!["select-window", "-t", "coven-test:orchestrator"],
        ]
    );
    let fake = Fake::new(root.path(), vec![(1, "".into())]);
    assert!(
        create::tmux(
            root.path(),
            "coven-test",
            &[],
            &[],
            &transport::Backend::injected(root.path(), true, &fake)
        )
        .is_err()
    );
    fake.assert_done();
}

#[test]
fn characterization_herdr_startup_transcript_required_vs_ignored_and_optional_panes() {
    let root = fixture("herdr");
    let _gate = TransportGuard::shared(root.path(), TIMEOUT).unwrap();
    let fake = Fake::new(
        root.path(),
        vec![
            json(
                serde_json::json!({"workspace":{"workspace_id":"w1"},"root_pane":{"pane_id":"p0"},"tab":{"tab_id":"t0"}}),
            ),
            (1, "".into()),
            (1, "".into()),
            (1, "".into()),
            json(serde_json::json!({"pane":{"pane_id":"p1"}})),
            (1, "".into()),
            (1, "".into()),
            json(serde_json::json!({})),
            json(serde_json::json!({"root_pane":{"pane_id":"p2"}})),
            (1, "".into()),
            (1, "".into()),
            (1, "".into()),
        ],
    );
    let backend = transport::Backend::injected(root.path(), false, &fake);
    assert_eq!(
        create::herdr(
            root.path(),
            "coven-test",
            &[("Beta".into(), "launch".into())],
            &[("events", "events".into()), ("messages", "messages".into())],
            &backend
        )
        .unwrap(),
        "w1"
    );
    fake.assert_done();
    let cwd = root.path().to_str().unwrap();
    assert_eq!(
        *fake.calls.borrow(),
        vec![
            vec![
                "workspace",
                "create",
                "--cwd",
                cwd,
                "--label",
                "coven-test",
                "--no-focus"
            ],
            vec!["tab", "rename", "t0", "orchestrator"],
            vec!["pane", "rename", "p0", "dashboard"],
            vec!["pane", "run", "p0", create::DASHBOARD],
            vec![
                "pane",
                "split",
                "--pane",
                "p0",
                "--direction",
                "right",
                "--ratio",
                "0.5",
                "--cwd",
                cwd,
                "--no-focus"
            ],
            vec!["pane", "rename", "p1", "events"],
            vec!["pane", "run", "p1", "events"],
            vec![
                "pane",
                "split",
                "--pane",
                "p0",
                "--direction",
                "right",
                "--ratio",
                "0.5",
                "--cwd",
                cwd,
                "--no-focus"
            ],
            vec![
                "tab",
                "create",
                "--workspace",
                "w1",
                "--cwd",
                cwd,
                "--label",
                "agent-Beta",
                "--no-focus"
            ],
            vec!["pane", "rename", "p2", "Beta"],
            vec!["pane", "run", "p2", "launch"],
            vec!["tab", "focus", "t0"],
        ]
    );
    for result in [
        json(
            serde_json::json!({"workspace":{"workspace_id":"w1"},"root_pane":{"pane_id":"p0"},"tab":{"tab_id":"--bad"}}),
        ),
        (1, "".into()),
    ] {
        let fake = Fake::new(root.path(), vec![result]);
        assert!(
            create::herdr(
                root.path(),
                "coven-test",
                &[],
                &[],
                &transport::Backend::injected(root.path(), false, &fake)
            )
            .is_err()
        );
        fake.assert_done();
        assert_eq!(fake.calls.borrow().len(), 1);
    }
}

#[test]
fn characterization_existing_partial_reused_and_foreground_exit_code_fenced() {
    let root = fixture("tmux");
    for in_tmux in [false, true] {
        let fake = Fake::new(root.path(), vec![ok()]);
        let (result, foreground) = run(root.path(), RuntimeAction::Up, &fake, true, true, in_tmux);
        assert_eq!(result.unwrap().exit_code, 23);
        assert_eq!(
            *foreground.calls.borrow(),
            vec![vec![
                if in_tmux {
                    "switch-client"
                } else {
                    "attach-session"
                },
                "-t",
                "coven-test"
            ]]
        );
        fake.assert_done();
    }
    let fake = Fake::new(root.path(), vec![ok()]);
    let (result, fg) = run(root.path(), RuntimeAction::Up, &fake, true, false, false);
    assert_eq!(
        result.unwrap().body,
        "[INFO] Coven tmux session ready: coven-test"
    );
    assert!(fg.calls.borrow().is_empty());
}

#[test]
fn characterization_absent_creates_and_restart_closes_before_creation() {
    for action in [RuntimeAction::Up, RuntimeAction::Restart] {
        let root = fixture("tmux");
        let mut outputs = vec![if action == RuntimeAction::Up {
            (1, "".into())
        } else {
            ok()
        }];
        if action == RuntimeAction::Restart {
            outputs.push(ok());
        }
        // new-session, title, three empty optional split results, layout, two agents, select
        outputs.extend(vec![ok(); 9]);
        let fake = Fake::new(root.path(), outputs);
        assert_eq!(
            run(root.path(), action, &fake, true, false, false)
                .0
                .unwrap()
                .exit_code,
            0
        );
        fake.assert_done();
        let calls = fake.calls.borrow();
        let start = if action == RuntimeAction::Restart {
            assert_eq!(calls[1], ["kill-session", "-t", "coven-test"]);
            2
        } else {
            1
        };
        assert_eq!(calls[start][0], "new-session");
        assert_eq!(calls.iter().filter(|v| v[0] == "new-window").count(), 2);
        assert!(!root.path().join("dashboard.md").exists());
    }
}

#[test]
fn characterization_plan_requests_history_before_manifest_check_and_no_backend() {
    let root = fixture("tmux");
    let fake = Fake::new(root.path(), vec![]);
    assert_eq!(
        run(root.path(), RuntimeAction::Plan, &fake, true, false, false)
            .0
            .unwrap_err()
            .code(),
        "missing_tmux_manifest"
    );
    assert_eq!(
        crate::logs::read(root.path(), crate::logs::LogKind::Events)
            .unwrap()
            .len(),
        1
    );
    fs::create_dir(root.path().join("tmux")).unwrap();
    fs::write(root.path().join("tmux/coven.toml"), "authored").unwrap();
    assert!(
        run(root.path(), RuntimeAction::Plan, &fake, true, false, false)
            .0
            .unwrap()
            .body
            .ends_with("- agent-Beta: Beta\n- agent-alpha: alpha")
    );
    assert!(fake.calls.borrow().is_empty());
}

#[test]
fn characterization_batch_validates_all_before_any_effect_and_keeps_duplicates() {
    let root = fixture("tmux");
    let fake = Fake::new(root.path(), vec![]);
    let result = agent_change_using(
        root.path(),
        &["Beta".into(), "unknown".into()],
        AgentAction::Start,
        |r, _| Ok(transport::Backend::injected(r, true, &fake)),
    );
    assert_eq!(result.unwrap_err().code(), "unknown_agent");
    assert!(fake.calls.borrow().is_empty());
    assert!(!root.path().join("logs/events.jsonl").exists());
    let fake = Fake::new(root.path(), vec![(0, "%1".into()), (0, "%1".into())]);
    let result = agent_change_using(
        root.path(),
        &["Beta".into(), "Beta".into()],
        AgentAction::Start,
        |r, _| Ok(transport::Backend::injected(r, true, &fake)),
    )
    .unwrap();
    assert!(result.body.ends_with("Started 0 agent window(s)"));
    assert_eq!(fake.calls.borrow().len(), 2);
}

#[test]
fn characterization_herdr_reuse_focus_checked_and_no_tty_ready() {
    let root = fixture("herdr");
    let existing =
        json(serde_json::json!({"workspaces":[{"label":"coven-test","workspace_id":"w1"}]}));
    let fake = Fake::new(root.path(), vec![existing.clone()]);
    assert_eq!(
        run(root.path(), RuntimeAction::Up, &fake, false, false, false)
            .0
            .unwrap()
            .body,
        "[INFO] Coven Herdr workspace ready: coven-test (w1)"
    );
    let fake = Fake::new(root.path(), vec![existing, (1, "".into())]);
    assert_eq!(
        run(root.path(), RuntimeAction::Up, &fake, false, true, false)
            .0
            .unwrap_err()
            .code(),
        "runtime_backend_command_failed"
    );
    assert_eq!(fake.calls.borrow()[1], ["workspace", "focus", "w1"]);
}

pub(crate) fn enable_control(root: &Path) {
    let location = crate::platform::workspace_location(root).unwrap();
    fs::write(root.join("state/runtime-launch-control.json"), serde_json::to_vec(&serde_json::json!({
        "version":1,"action":"runtime_launch","workspace_id":"a".repeat(32),
        "epoch_id":"b".repeat(32),"location":{"root":location.root,"device":location.device,"inode":location.inode},
        "head":null,"claims":{}
    })).unwrap()).unwrap();
}

#[test]
fn characterization_admission_race_mode_rechecked_before_broken_workspace() {
    for agent_action in [false, true] {
        let root = fixture("tmux");
        let gate = TransportGuard::exclusive(root.path(), TIMEOUT).unwrap();
        let path = root.path().to_owned();
        let (started, waiting) = std::sync::mpsc::channel();
        let child = std::thread::spawn(move || {
            let fake = Fake::new(&path, vec![]);
            let admit = |root: &Path| {
                started.send(()).unwrap();
                TransportGuard::shared(root, TIMEOUT)
            };
            if agent_action {
                agent_change_admitting(
                    &path,
                    &["Beta".into()],
                    AgentAction::Start,
                    |r, _| Ok(transport::Backend::injected(r, true, &fake)),
                    admit,
                )
                .unwrap_err()
                .code()
            } else {
                let foreground = Foreground {
                    root: path.clone(),
                    calls: RefCell::new(Vec::new()),
                };
                lifecycle_admitting(
                    &path,
                    RuntimeAction::Up,
                    |r, _| Ok(transport::Backend::injected(r, true, &fake)),
                    &foreground,
                    (false, false),
                    false,
                    admit,
                )
                .unwrap_err()
                .code()
            }
        });
        waiting.recv().unwrap();
        enable_control(root.path());
        fs::write(root.path().join("coven.toml"), "broken [").unwrap();
        drop(gate);
        assert_eq!(child.join().unwrap(), "legacy_runtime_disabled");
        assert!(!root.path().join("logs/events.jsonl").exists());
    }
}

#[test]
fn characterization_orphan_and_relocated_control_block_before_effects() {
    let root = fixture("tmux");
    let gate = TransportGuard::shared(root.path(), TIMEOUT).unwrap();
    drop(gate);
    enable_control(root.path());
    let path = root.path().join("state/runtime-launch-control.json");
    let mut control: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    control["location"]["root"] = serde_json::json!("/not/the/original/root");
    fs::write(&path, control.to_string()).unwrap();
    let fake = Fake::new(root.path(), vec![]);
    assert!(
        run(root.path(), RuntimeAction::Up, &fake, true, false, false)
            .0
            .is_err()
    );
    fs::remove_file(path).unwrap();
    fs::create_dir(root.path().join("state/actions")).unwrap();
    assert_eq!(
        run(root.path(), RuntimeAction::Plan, &fake, true, false, false)
            .0
            .unwrap_err()
            .code(),
        "action_orphan_bundles"
    );
    assert!(fake.calls.borrow().is_empty());
}

#[test]
fn characterization_agent_restart_stop_then_start_and_history_before_dashboard() {
    let root = fixture("tmux");
    let fake = Fake::new(
        root.path(),
        vec![(0, "%1".into()), ok(), (1, "".into()), ok(), ok()],
    );
    let result = agent_change_using(
        root.path(),
        &["Beta".into()],
        AgentAction::Restart,
        |r, _| Ok(transport::Backend::injected(r, true, &fake)),
    )
    .unwrap();
    fake.assert_done();
    assert_eq!(result.body, "[INFO] Restarted 1 agent window(s)");
    let calls = fake.calls.borrow();
    assert_eq!(calls[1], ["kill-window", "-t", "coven-test:agent-Beta"]);
    assert_eq!(calls[2][0], "list-panes");
    assert_eq!(calls[3], ["has-session", "-t", "coven-test"]);
    assert_eq!(calls[4][0], "new-window");
    let events = crate::logs::read(root.path(), crate::logs::LogKind::Events).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(
        transport::field(&events[0], "type"),
        Some("agent.stopped.window")
    );
    assert_eq!(
        transport::field(&events[1], "type"),
        Some("agent.started.window")
    );
    assert!(root.path().join("dashboard.md").is_file());
    assert!(
        !root.path().join("prompts/Beta.start.txt").exists(),
        "no implicit refresh or starter creation during launch"
    );
}

#[test]
fn characterization_herdr_agent_start_and_stop_native_calls() {
    let root = fixture("herdr");
    let workspace =
        json(serde_json::json!({"workspaces":[{"label":"coven-test","workspace_id":"w1"}]}));
    let tabs = json(
        serde_json::json!({"tabs":[{"label":"agent-Beta","workspace_id":"w1","tab_id":"t1"}]}),
    );
    let panes = json(serde_json::json!({"panes":[{"tab_id":"t1","pane_id":"p1"}]}));
    let fake = Fake::new(
        root.path(),
        vec![
            json(serde_json::json!({"workspaces":[]})),
            workspace.clone(),
            json(serde_json::json!({"root_pane":{"pane_id":"p1"}})),
            (1, "".into()),
            (1, "".into()),
        ],
    );
    let result = agent_change_using(root.path(), &["Beta".into()], AgentAction::Start, |r, _| {
        Ok(transport::Backend::injected(r, false, &fake))
    })
    .unwrap();
    assert!(result.body.ends_with("Started 1 agent window(s)"));
    fake.assert_done();
    assert_eq!(
        &fake.calls.borrow()[2][..4],
        ["tab", "create", "--workspace", "w1"]
    );
    assert_eq!(fake.calls.borrow()[2].last().unwrap(), "--focus");
    assert_eq!(fake.calls.borrow()[3], ["pane", "rename", "p1", "Beta"]);
    let fake = Fake::new(
        root.path(),
        vec![
            workspace.clone(),
            tabs.clone(),
            panes.clone(),
            workspace,
            tabs,
            panes,
            ok(),
        ],
    );
    let result = agent_change_using(root.path(), &["Beta".into()], AgentAction::Stop, |r, _| {
        Ok(transport::Backend::injected(r, false, &fake))
    })
    .unwrap();
    assert!(result.body.ends_with("Stopped 1 agent window(s)"));
    assert_eq!(fake.calls.borrow().last().unwrap(), &["tab", "close", "t1"]);
    fake.assert_done();
}

#[test]
fn characterization_required_creation_failures_stop_without_cleanup_or_retry() {
    let root = fixture("tmux");
    let _gate = TransportGuard::shared(root.path(), TIMEOUT).unwrap();
    for outputs in [
        vec![ok(), ok(), (9, String::new())],
        vec![ok(), ok(), (0, "%1".into()), ok(), ok(), (9, String::new())],
    ] {
        let count = outputs.len();
        let fake = Fake::new(root.path(), outputs);
        let backend = transport::Backend::injected(root.path(), true, &fake);
        assert!(
            create::tmux(
                root.path(),
                "coven-test",
                &[("Beta".into(), "launch".into())],
                &[("events", "events".into())],
                &backend
            )
            .is_err()
        );
        assert_eq!(fake.calls.borrow().len(), count);
        fake.assert_done();
    }
    for (stdout, expected) in [
        ("--unsafe", "invalid_runtime_handle"),
        ("%1\n%2", "invalid_runtime_handle"),
    ] {
        let fake = Fake::new(root.path(), vec![ok(), ok(), (0, stdout.into())]);
        let backend = transport::Backend::injected(root.path(), true, &fake);
        assert_eq!(
            create::tmux(
                root.path(),
                "coven-test",
                &[],
                &[("events", "events".into())],
                &backend
            )
            .unwrap_err()
            .code(),
            expected
        );
        fake.assert_done();
    }
    let workspace =
        json(serde_json::json!({"workspace":{"workspace_id":"w1"},"root_pane":{"pane_id":"p1"}}));
    for outputs in [
        vec![workspace.clone(), ok(), ok(), (9, String::new())],
        vec![
            workspace,
            ok(),
            ok(),
            json(serde_json::json!({})),
            (9, String::new()),
        ],
    ] {
        let count = outputs.len();
        let fake = Fake::new(root.path(), outputs);
        let backend = transport::Backend::injected(root.path(), false, &fake);
        assert!(
            create::herdr(
                root.path(),
                "coven-test",
                &[("Beta".into(), "launch".into())],
                &[("events", "events".into())],
                &backend
            )
            .is_err()
        );
        assert_eq!(fake.calls.borrow().len(), count);
        fake.assert_done();
    }
}

#[test]
fn characterization_restart_failed_close_never_creates_or_attaches() {
    for tmux in [false, true] {
        let root = fixture(if tmux { "tmux" } else { "herdr" });
        let fake = Fake::new(
            root.path(),
            vec![
                if tmux {
                    ok()
                } else {
                    json(
                        serde_json::json!({"workspaces":[{"label":"coven-test","workspace_id":"w1"}]}),
                    )
                },
                (9, String::new()),
            ],
        );
        let (result, foreground) = run(
            root.path(),
            RuntimeAction::Restart,
            &fake,
            tmux,
            false,
            false,
        );
        assert_eq!(result.unwrap_err().code(), "runtime_backend_command_failed");
        assert!(foreground.calls.borrow().is_empty());
        assert_eq!(fake.calls.borrow().len(), 2);
        fake.assert_done();
    }
}

#[test]
fn characterization_only_both_ttys_allow_foreground() {
    let root = fixture("tmux");
    for ttys in [(true, false), (false, true), (false, false)] {
        let fake = Fake::new(root.path(), vec![ok()]);
        let foreground = Foreground {
            root: root.path().to_owned(),
            calls: RefCell::new(Vec::new()),
        };
        let result = lifecycle_using(
            root.path(),
            RuntimeAction::Up,
            |r, _| Ok(transport::Backend::injected(r, true, &fake)),
            &foreground,
            ttys,
            true,
        )
        .unwrap();
        assert_eq!(result.body, "[INFO] Coven tmux session ready: coven-test");
        assert!(foreground.calls.borrow().is_empty());
        fake.assert_done();
    }
}

#[test]
fn characterization_foreground_return_codes_keep_python_process_status() {
    struct Exit(i32);
    impl runtime_foreground::Runner for Exit {
        fn run(&self, _: &runtime_foreground::Request) -> Status {
            Status::Exited(self.0)
        }
    }
    let root = fixture("tmux");
    let image = std::env::current_exe().unwrap();
    for (code, expected) in [(0, 0), (7, 7), (-15, 241), (256, 0)] {
        let fake = Fake::new(root.path(), vec![ok()]);
        let result = lifecycle_using(
            root.path(),
            RuntimeAction::Up,
            |_, _| transport::Backend::new(image.to_str().unwrap(), &fake),
            &Exit(code),
            (true, true),
            false,
        )
        .unwrap();
        assert_eq!(result.exit_code, expected);
        assert!(result.body.is_empty());
    }
}
