use super::*;
use crate::{
    launch,
    runtime_command::{Output, Request, Runner, Status},
    values::{self, Profile, Value},
};
use std::{
    cell::RefCell,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};

#[path = "fallback.rs"]
mod fallback;

fn file(path: &Path, executable: bool) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"selected fixture\n").unwrap();
    fs::set_permissions(
        path,
        fs::Permissions::from_mode(if executable { 0o700 } else { 0o600 }),
    )
    .unwrap();
}

struct Boundary {
    executable: PathBuf,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    stopping: bool,
    calls: RefCell<Vec<Vec<String>>>,
    remove_after: Option<usize>,
}
impl Runner for Boundary {
    fn run(&self, request: &Request) -> Output {
        assert!(Path::new(&request.argv[0]).is_absolute());
        // Compare spelling too: Path equality can ignore dot/duplicate separators.
        assert_eq!(request.argv[0], self.executable.to_str().unwrap());
        assert_eq!(request.cwd.as_os_str(), self.cwd.as_os_str());
        assert_eq!(request.env, self.env);
        assert_eq!(fs::read(&request.argv[0]).unwrap(), b"selected fixture\n");
        self.calls.borrow_mut().push(request.argv.clone());
        if self.remove_after == Some(self.calls.borrow().len()) {
            fs::remove_file(&request.argv[0]).unwrap();
        }
        let args = request.argv[1..]
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let mut stdout = "{}";
        let mut stderr = "";
        let mut status = Status::Exited(0);
        if args.starts_with(&["has-session"]) && !self.stopping {
            status = Status::Exited(1);
            stderr = "can't find session: coven-a-b";
        } else if args.starts_with(&["workspace", "list"]) {
            stdout = if self.stopping {
                r#"{"result":{"workspaces":[{"label":"coven-a-b","workspace_id":"w"}]}}"#
            } else {
                r#"{"result":{"workspaces":[]}}"#
            };
        } else if args.starts_with(&["workspace", "create"]) {
            stdout =
                r#"{"result":{"workspace":{"workspace_id":"w"},"root_pane":{"pane_id":"root"}}}"#;
        } else if args.starts_with(&["tab", "create"]) {
            stdout = r#"{"result":{"root_pane":{"pane_id":"p"}}}"#;
        }
        Output {
            status,
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }
}
fn plan(cwd: &Path, backend: &str, env: &BTreeMap<String, String>) -> Value {
    let agents = values::parse(
        br#"[{"id":"x","command":"exec worker 'literal'","starter":"- $HOME ; literal"}]"#,
        Profile::Authority,
    )
    .unwrap();
    let env = Value::object(
        env.iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect(),
    )
    .unwrap();
    launch::build_plan(cwd.to_str().unwrap(), "coven-a-b", backend, &agents, &env).unwrap()
}
fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.as_object().unwrap().get(key).unwrap()
}
fn run(boundary: &Boundary, backend: &str) -> Value {
    let frozen = plan(&boundary.cwd, backend, &boundary.env);
    let before = frozen.canonical().unwrap();
    let report = launch::run_with(&frozen, boundary, &mut |_| Ok(()), boundary.stopping);
    assert_eq!(before, frozen.canonical().unwrap());
    assert_eq!(
        get(
            get(&frozen, "steps").as_array().unwrap().first().unwrap(),
            "argv"
        )
        .as_array()
        .unwrap()[0]
            .as_str(),
        Some(backend)
    );
    report
}

#[test]
fn real_frozen_path_matrix_reaches_absolute_boundary_for_both_backends_launch_and_stop() {
    for backend in ["tmux", "herdr"] {
        for stopping in [false, true] {
            for scenario in [
                "absolute",
                "relative",
                "dot",
                "empty",
                "leading_empty",
                "trailing_empty",
                "middle_empty",
                "skipped_entries",
                "file_alias",
                "parent_alias",
            ] {
                let root = tempfile::tempdir().unwrap();
                let cwd = root.path().join("workspace");
                fs::create_dir(&cwd).unwrap();
                assert_ne!(std::env::current_dir().unwrap(), cwd);
                let (path, executable) = match scenario {
                    "absolute" => {
                        let directory = root.path().join("absolute tools");
                        (
                            directory.to_str().unwrap().to_owned(),
                            directory.join(backend),
                        )
                    }
                    "relative" => ("bin".into(), cwd.join("bin").join(backend)),
                    "dot" => (".".into(), cwd.join(".").join(backend)),
                    "empty" => ("".into(), cwd.join("").join(backend)),
                    "leading_empty" => (":missing".into(), cwd.join(backend)),
                    "trailing_empty" => ("missing:".into(), cwd.join(backend)),
                    "middle_empty" => ("missing::tail".into(), cwd.join(backend)),
                    "skipped_entries" => {
                        file(&cwd.join("blocked").join(backend), false);
                        fs::create_dir_all(cwd.join("directory").join(backend)).unwrap();
                        (
                            "blocked:directory:missing:bin".into(),
                            cwd.join("bin").join(backend),
                        )
                    }
                    "file_alias" => {
                        let target = root.path().join("real executable");
                        file(&target, true);
                        let directory = cwd.join("alias bin");
                        fs::create_dir(&directory).unwrap();
                        let alias = directory.join(backend);
                        symlink(target, &alias).unwrap();
                        ("alias bin".into(), alias)
                    }
                    "parent_alias" => {
                        let real = root.path().join("real");
                        fs::create_dir_all(real.join("child")).unwrap();
                        symlink(real.join("child"), cwd.join("alias")).unwrap();
                        file(&real.join("bin").join(backend), true);
                        file(&cwd.join("bin").join(backend), true);
                        fs::write(cwd.join("bin").join(backend), b"wrong lexical collapse\n")
                            .unwrap();
                        (
                            "alias/../bin".into(),
                            cwd.join("alias/../bin").join(backend),
                        )
                    }
                    _ => unreachable!(),
                };
                if !["file_alias", "parent_alias"].contains(&scenario) {
                    file(&executable, true);
                }
                let boundary = Boundary {
                    executable,
                    cwd,
                    env: BTreeMap::from([
                        ("PATH".into(), path),
                        ("HOME".into(), "/frozen-only".into()),
                    ]),
                    stopping,
                    calls: RefCell::new(vec![]),
                    remove_after: None,
                };
                let report = run(&boundary, backend);
                assert_eq!(
                    get(&report, "status").as_str(),
                    Some("transport_completed"),
                    "{backend} {stopping} {scenario}: {report:?}"
                );
                let calls = boundary.calls.borrow();
                if stopping {
                    assert_eq!(calls.len(), 2);
                    assert!(matches!(
                        get(&report, "stop_command_attempted"),
                        Value::Bool(true)
                    ));
                    assert!(matches!(
                        get(&report, "stop_command_succeeded"),
                        Value::Bool(true)
                    ));
                    let expected = if backend == "tmux" {
                        vec!["kill-session", "-t", "=coven-a-b"]
                    } else {
                        vec!["workspace", "close", "w"]
                    };
                    assert_eq!(calls[1][1..], expected);
                } else {
                    assert_eq!(calls.len(), if backend == "tmux" { 5 } else { 7 });
                    assert_eq!(calls[calls.len() - 2].last().unwrap(), "- $HOME ; literal");
                }
            }
        }
    }
}

#[test]
fn unset_path_uses_posix_defpath_but_empty_path_uses_frozen_cwd() {
    let root = tempfile::tempdir().unwrap();
    file(&root.path().join("sh"), true);
    // The cwd decoy must not win when PATH is absent. This never executes a shell.
    assert_eq!(
        candidates(
            "sh",
            root.path(),
            &BTreeMap::new(),
            Instant::now() + Duration::from_secs(60)
        )
        .next()
        .unwrap()
        .unwrap(),
        "/bin/sh"
    );
    assert_eq!(
        candidates(
            "sh",
            root.path(),
            &BTreeMap::from([("PATH".into(), String::new())]),
            Instant::now() + Duration::from_secs(60)
        )
        .next()
        .unwrap()
        .unwrap(),
        root.path().join("sh").to_str().unwrap()
    );
}

#[test]
fn missing_frozen_selection_never_calls_runner_or_falls_back_to_ambient_path() {
    for backend in ["tmux", "herdr"] {
        for stopping in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let boundary = Boundary {
                executable: root.path().join(backend),
                cwd: root.path().into(),
                env: BTreeMap::from([("PATH".into(), "only-missing".into())]),
                stopping,
                calls: RefCell::new(vec![]),
                remove_after: None,
            };
            let report = run(&boundary, backend);
            assert_eq!(
                get(&report, "status").as_str(),
                Some("transport_failed_or_partial")
            );
            assert!(boundary.calls.borrow().is_empty());
            assert!(matches!(
                get(&report, "outcome_unknown"),
                Value::Bool(false)
            ));
            let diagnostic = &get(&report, "diagnostics").as_array().unwrap()[0];
            assert_eq!(get(diagnostic, "error").as_str(), Some("OSError"));
            if stopping {
                assert!(matches!(
                    get(&report, "stop_command_attempted"),
                    Value::Bool(false)
                ));
            }
        }
    }
}

#[test]
fn disappeared_selection_preserves_observed_handles_without_retry() {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("herdr");
    file(&executable, true);
    let boundary = Boundary {
        executable,
        cwd: root.path().into(),
        env: BTreeMap::from([("PATH".into(), ".".into())]),
        stopping: false,
        calls: RefCell::new(vec![]),
        remove_after: Some(2),
    };
    // Preserve the explicit dot component in the boundary spelling.
    let boundary = Boundary {
        executable: root.path().join(".").join("herdr"),
        ..boundary
    };
    let report = run(&boundary, "herdr");
    assert_eq!(
        get(&report, "status").as_str(),
        Some("transport_failed_or_partial")
    );
    assert_eq!(boundary.calls.borrow().len(), 2);
    assert_eq!(
        get(get(&report, "handles"), "workspace").as_str(),
        Some("w")
    );
    assert_eq!(
        get(get(&report, "handles"), "orchestrator").as_str(),
        Some("root")
    );
    assert_eq!(get(&report, "completed_steps").canonical().unwrap(), b"2");
}
