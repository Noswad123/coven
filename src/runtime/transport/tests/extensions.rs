use super::*;
use std::collections::VecDeque;

struct Script {
    outputs: RefCell<VecDeque<Output>>,
    calls: RefCell<Vec<Vec<String>>>,
}

impl Script {
    fn new(responses: &[&str]) -> Self {
        Self {
            outputs: RefCell::new(
                responses
                    .iter()
                    .map(|text| Output {
                        status: Status::Exited(0),
                        stdout: text.as_bytes().to_vec(),
                        stderr: Vec::new(),
                    })
                    .collect(),
            ),
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl Runner for Script {
    fn run(&self, request: &Request) -> Output {
        self.calls.borrow_mut().push(request.argv[1..].to_vec());
        self.outputs
            .borrow_mut()
            .pop_front()
            .expect("unexpected backend call")
    }
}

#[test]
fn capture_normalizes_stdout_and_strictly_validates_both_streams() {
    let root = tempfile::tempdir().unwrap();
    let script = Script::new(&[]);
    script.outputs.borrow_mut().push_back(Output {
        status: Status::Exited(17),
        stdout: b"out\r\nnext\rlast\n".to_vec(),
        stderr: b"err\r\nnext\rlast\n".to_vec(),
    });
    let backend = Backend::injected(root.path(), false, &script);
    let captured = backend.capture(&["workspace", "new"]).unwrap();
    assert_eq!(captured.code, 17);
    assert_eq!(captured.stdout, "out\nnext\nlast\n");
    for (stdout, stderr) in [(vec![0xff], Vec::new()), (Vec::new(), vec![0xff])] {
        script.outputs.borrow_mut().push_back(Output {
            status: Status::Exited(0),
            stdout,
            stderr,
        });
        assert!(backend.capture(&["workspace", "new"]).is_err());
    }
}

const WORKSPACES: &str = r#"{"result":{"workspaces":[{"label":"coven-test","workspace_id":"w1"},{"label":"coven-test","workspace_id":"w2"}]}}"#;
const TABS: &str = r#"{"result":{"tabs":[{"label":"agent-agent","workspace_id":"w1","tab_id":"t1"},{"label":"agent-agent","workspace_id":"w2","tab_id":"t2"}]}}"#;
const PANES: &str = r#"{"result":{"panes":[{"tab_id":"other","pane_id":"decoy"},{"tab_id":"t1","pane_id":"p1"},{"tab_id":"t1","pane_id":"p2"}]}}"#;

#[test]
fn first_duplicate_wins_and_tab_repeats_independent_workspace_lookup() {
    let root = tempfile::tempdir().unwrap();
    let script = Script::new(&[WORKSPACES, TABS, PANES, WORKSPACES, TABS, PANES]);
    let backend = Backend::injected(root.path(), false, &script);
    assert_eq!(
        backend.pane("coven-test", "agent").unwrap().as_deref(),
        Some("p1")
    );
    assert_eq!(
        backend.tab("coven-test", "agent").unwrap().as_deref(),
        Some("t1")
    );
    let expected = vec![
        vec!["workspace", "list"],
        vec!["tab", "list", "--workspace", "w1"],
        vec!["pane", "list", "--workspace", "w1"],
    ];
    assert_eq!(
        *script.calls.borrow(),
        [expected.clone(), expected].concat()
    );
}

#[test]
fn first_duplicate_missing_target_does_not_search_later_duplicates() {
    let root = tempfile::tempdir().unwrap();
    for (workspaces, tabs) in [
        (
            r#"{"result":{"workspaces":[{"label":"coven-test"},{"label":"coven-test","workspace_id":"w2"}]}}"#,
            None,
        ),
        (
            WORKSPACES,
            Some(
                r#"{"result":{"tabs":[{"label":"agent-agent"},{"label":"agent-agent","workspace_id":"w2","tab_id":"t2"}]}}"#,
            ),
        ),
    ] {
        let mut responses = vec![workspaces];
        responses.extend(tabs);
        let script = Script::new(&responses);
        let backend = Backend::injected(root.path(), false, &script);
        assert!(backend.tab("coven-test", "agent").unwrap().is_none());
        assert!(script.outputs.borrow().is_empty());
    }
}

#[test]
fn malformed_native_handles_fail_before_mutation() {
    let root = tempfile::tempdir().unwrap();
    for value in [
        r#"123"#,
        r#""-option""#,
        r#""a/b""#,
        r#""a b""#,
        r#""a\nb""#,
    ] {
        for stage in 0..3 {
            let broken = match stage {
                0 => format!(
                    r#"{{"result":{{"workspaces":[{{"label":"coven-test","workspace_id":{value}}}]}}}}"#
                ),
                1 => format!(
                    r#"{{"result":{{"tabs":[{{"label":"agent-agent","workspace_id":"w1","tab_id":{value}}}]}}}}"#
                ),
                _ => format!(r#"{{"result":{{"panes":[{{"tab_id":"t1","pane_id":{value}}}]}}}}"#),
            };
            let mut responses = [WORKSPACES, TABS, PANES];
            responses[stage] = &broken;
            let script = Script::new(&responses[..=stage]);
            let backend = Backend::injected(root.path(), false, &script);
            assert_eq!(
                backend.tab("coven-test", "agent").unwrap_err().code(),
                "invalid_runtime_handle"
            );
            assert_eq!(script.calls.borrow().len(), stage + 1);
        }
    }
}

struct Foreground {
    statuses: RefCell<VecDeque<Status>>,
    calls: RefCell<Vec<crate::runtime_foreground::Request>>,
}

impl crate::runtime_foreground::Runner for Foreground {
    fn run(&self, request: &crate::runtime_foreground::Request) -> Status {
        self.calls
            .borrow_mut()
            .push(crate::runtime_foreground::Request {
                argv: request.argv.clone(),
                cwd: request.cwd.clone(),
                env: request.env.clone(),
            });
        self.statuses
            .borrow_mut()
            .pop_front()
            .expect("unexpected foreground retry")
    }
}

pub(super) fn foreground_retains_caller_context(backend: &Backend<'_>, caller: &Path) {
    let foreground = Foreground {
        statuses: RefCell::new([Status::Exited(0)].into()),
        calls: RefCell::new(Vec::new()),
    };
    assert_eq!(backend.foreground(&["attach"], &foreground).unwrap(), 0);
    let calls = foreground.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].cwd, caller);
    assert_eq!(
        calls[0].argv[0],
        caller.join("relative/tmux").to_str().unwrap()
    );
    assert_eq!(calls[0].env, backend.env);
    assert_eq!(calls[0].env["PATH"], "relative");
}

#[test]
fn foreground_frozen_context_and_only_preimage_failures_advance_candidates() {
    for first in [
        Status::ExecFailed,
        Status::Exited(127),
        Status::Exited(9),
        Status::SpawnError,
        Status::Timeout,
        Status::OutputLimit,
    ] {
        let (root, env) = fixture();
        let capture = Script::new(&[]);
        let mut backend = Backend::new("tmux", &capture).unwrap();
        backend.cwd = root.path().to_owned();
        backend.env = env.clone();
        // Relative PATH entries are resolved against the frozen caller cwd.
        backend.env.insert("PATH".into(), "bad:good".into());
        let foreground = Foreground {
            statuses: RefCell::new([first, Status::Exited(23)].into()),
            calls: RefCell::new(Vec::new()),
        };
        let result = backend.foreground(&["attach-session", "-t", "coven-test"], &foreground);
        match first {
            Status::ExecFailed => assert_eq!(result.unwrap(), 23),
            Status::Exited(code) => assert_eq!(result.unwrap(), code),
            Status::SpawnError => assert_eq!(
                result.unwrap_err().code(),
                "runtime_command_effects_unknown"
            ),
            Status::Timeout => assert_eq!(
                result.unwrap_err().code(),
                "runtime_timeout_effects_unknown"
            ),
            Status::OutputLimit => assert_eq!(
                result.unwrap_err().code(),
                "runtime_output_limit_effects_unknown"
            ),
        }
        let calls = foreground.calls.borrow();
        assert_eq!(calls.len(), if first == Status::ExecFailed { 2 } else { 1 });
        for (call, directory) in calls.iter().zip(["bad", "good"]) {
            assert_eq!(
                call.argv,
                [
                    root.path().join(directory).join("tmux").to_str().unwrap(),
                    "attach-session",
                    "-t",
                    "coven-test"
                ]
            );
            assert_eq!(call.cwd, backend.cwd);
            assert_eq!(call.env, backend.env);
            assert_eq!(call.env["COVEN_RUNTIME_CAPTURED_ENV_TEST"], "preserved");
        }
        assert!(capture.calls.borrow().is_empty());
    }
}

#[test]
fn foreground_missing_executable_and_preimage_exhaustion_have_no_further_effects() {
    let (root, env) = fixture();
    let capture = Script::new(&[]);
    let mut backend = Backend::new("tmux", &capture).unwrap();
    backend.env = env;
    let foreground = Foreground {
        statuses: RefCell::new([Status::ExecFailed, Status::ExecFailed].into()),
        calls: RefCell::new(Vec::new()),
    };
    assert_eq!(
        backend
            .foreground(&["attach"], &foreground)
            .unwrap_err()
            .code(),
        "runtime_executable_unavailable"
    );
    assert_eq!(foreground.calls.borrow().len(), 2);
    backend.env.insert(
        "PATH".into(),
        root.path().join("missing").to_str().unwrap().into(),
    );
    assert_eq!(
        backend
            .foreground(&["attach"], &foreground)
            .unwrap_err()
            .code(),
        "runtime_executable_unavailable"
    );
    assert_eq!(foreground.calls.borrow().len(), 2);
}
