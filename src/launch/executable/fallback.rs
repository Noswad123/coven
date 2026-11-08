use super::*;
use crate::runtime_command::NativeRunner;
use serde_json::{Value as Json, json};
use std::{
    collections::VecDeque,
    io::Write,
    process::{Command, Stdio},
    sync::OnceLock,
    time::Duration,
};

struct Injected {
    outputs: RefCell<VecDeque<Status>>,
    requests: RefCell<Vec<(String, Duration)>>,
    delay: Duration,
}
impl Runner for Injected {
    fn run(&self, request: &Request) -> Output {
        assert!(Path::new(&request.argv[0]).is_absolute());
        assert_eq!(&request.argv[1..], ["literal argument"]);
        assert_eq!(request.cwd, Path::new("/frozen"));
        assert_eq!(
            request.env,
            BTreeMap::from([("HOME".into(), "/only-frozen".into())])
        );
        assert!(request.timeout <= Duration::from_secs(60) && !request.timeout.is_zero());
        assert_eq!(request.max_output_bytes, 1_048_576);
        self.requests
            .borrow_mut()
            .push((request.argv[0].clone(), request.timeout));
        std::thread::sleep(self.delay);
        Output {
            status: self
                .outputs
                .borrow_mut()
                .pop_front()
                .expect("no unapproved retry"),
            stdout: vec![],
            stderr: vec![],
        }
    }
}
fn injected(statuses: &[Status], delay: Duration) -> Injected {
    Injected {
        outputs: RefCell::new(statuses.iter().copied().collect()),
        requests: RefCell::new(vec![]),
        delay,
    }
}
fn choices<'a>(
    _: &'a str,
    _: &'a Path,
    _: &'a BTreeMap<String, String>,
    _: Instant,
) -> Candidates<'a> {
    Box::new([Ok("/first/tmux".into()), Ok("/second/tmux".into())].into_iter())
}
fn command(
    runner: &dyn Runner,
    select: &Selector<'_>,
    budget: Duration,
) -> TransportResult<Output> {
    super::super::run(
        vec!["tmux".into(), "literal argument".into()],
        Path::new("/frozen"),
        &BTreeMap::from([("HOME".into(), "/only-frozen".into())]),
        runner,
        select,
        budget,
    )
}

#[test]
fn only_exec_failed_allows_fallback_and_all_attempts_share_one_deadline() {
    for status in [
        Status::SpawnError,
        Status::Timeout,
        Status::OutputLimit,
        Status::Exited(127),
        Status::Exited(9),
        Status::Exited(0),
    ] {
        let runner = injected(&[status], Duration::ZERO);
        assert_eq!(
            command(&runner, &choices, Duration::from_secs(60))
                .unwrap()
                .status,
            status
        );
        assert_eq!(runner.requests.borrow().len(), 1);
        assert_eq!(runner.requests.borrow()[0].0, "/first/tmux");
    }
    let runner = injected(
        &[Status::ExecFailed, Status::Exited(0)],
        Duration::from_millis(5),
    );
    assert_eq!(
        command(&runner, &choices, Duration::from_secs(60))
            .unwrap()
            .status,
        Status::Exited(0)
    );
    let requests = runner.requests.borrow();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].0, "/first/tmux");
    assert_eq!(requests[1].0, "/second/tmux");
    assert!(requests[1].1 < requests[0].1);
    assert!(requests[0].1.saturating_sub(requests[1].1) >= Duration::from_millis(5));
    drop(requests);

    let exhausted = injected(&[Status::ExecFailed, Status::ExecFailed], Duration::ZERO);
    assert_eq!(
        command(&exhausted, &choices, Duration::from_secs(60))
            .unwrap_err()
            .0,
        "OSError"
    );
    assert_eq!(exhausted.requests.borrow().len(), 2);

    let expired = injected(&[Status::ExecFailed], Duration::from_millis(10));
    assert_eq!(
        command(&expired, &choices, Duration::from_millis(5))
            .unwrap_err()
            .0,
        "TimeoutExpired"
    );
    assert_eq!(expired.requests.borrow().len(), 1);
    let lookup = injected(&[], Duration::ZERO);
    assert_eq!(
        command(
            &lookup,
            &|name, cwd, env, deadline| {
                std::thread::sleep(Duration::from_millis(10));
                choices(name, cwd, env, deadline)
            },
            Duration::from_millis(5)
        )
        .unwrap_err()
        .0,
        "TimeoutExpired"
    );
    assert!(lookup.requests.borrow().is_empty());
}

fn compiled() -> &'static Path {
    static FIXTURE: OnceLock<(tempfile::TempDir, PathBuf)> = OnceLock::new();
    &FIXTURE
        .get_or_init(|| {
            let root = tempfile::tempdir().unwrap();
            let executable = root.path().join("compiled-inert-backend");
            let output = Command::new("rustc")
                .args(["--edition=2024", "-O"])
                .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/launch/executable/fixture.rs"))
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            (root, executable)
        })
        .1
}
struct Native {
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    candidates: Vec<PathBuf>,
    calls: RefCell<Vec<Json>>,
    attempts: RefCell<Vec<(PathBuf, Status, Duration)>>,
}
impl Runner for Native {
    fn run(&self, request: &Request) -> Output {
        assert!(Path::new(&request.argv[0]).is_absolute());
        assert!(
            self.candidates
                .iter()
                .any(|p| p.as_os_str() == Path::new(&request.argv[0]).as_os_str())
        );
        assert_eq!(request.cwd, self.cwd);
        assert_eq!(request.env, self.env);
        assert!(!request.timeout.is_zero() && request.timeout <= Duration::from_secs(60));
        let output = NativeRunner.run(request);
        self.attempts.borrow_mut().push((
            request.argv[0].clone().into(),
            output.status,
            request.timeout,
        ));
        if let Status::Exited(code) = output.status {
            self.calls.borrow_mut().push(json!({"args":request.argv[1..],"cwd":request.cwd,"env":request.env,
                "code":code,"stdout":String::from_utf8(output.stdout.clone()).unwrap(),"stderr":String::from_utf8(output.stderr.clone()).unwrap()}));
        }
        output
    }
}
fn python(plan: &Value) -> Json {
    let mut oracle = Command::new("python3")
        .arg("-B")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/launch/executable/native_oracle.py"))
        .arg(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    oracle
        .stdin
        .take()
        .unwrap()
        .write_all(&plan.canonical().unwrap())
        .unwrap();
    let output = oracle.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn compiled_native_missing_interpreter_fallback_matches_python_but_exit_127_never_falls_back() {
    for backend in ["tmux", "herdr"] {
        for stopping in [false, true] {
            for first in ["missing-interpreter", "missing-file", "exit127"] {
                let root = tempfile::tempdir().unwrap();
                let cwd = root.path().join(if stopping {
                    "workspace-stop"
                } else {
                    "workspace-launch"
                });
                fs::create_dir(&cwd).unwrap();
                let bad = cwd.join(first).join(backend);
                fs::create_dir(bad.parent().unwrap()).unwrap();
                if first == "missing-interpreter" {
                    file(&bad, true);
                    fs::write(&bad, b"#!/nonexistent-coven-inert-interpreter\n").unwrap();
                } else if first == "exit127" {
                    fs::copy(compiled(), &bad).unwrap();
                }
                let good = cwd.join("compiled fixture bin").join(backend);
                fs::create_dir(good.parent().unwrap()).unwrap();
                fs::copy(compiled(), &good).unwrap();
                let env = BTreeMap::from([
                    ("PATH".into(), format!("{first}:compiled fixture bin")),
                    ("HOME".into(), "/frozen".into()),
                ]);
                let plan = plan(&cwd, backend, &env);
                // Oracle selects stop via an explicit wrapper field, not plan mutation.
                let oracle_input = Value::object(vec![
                    ("plan".into(), plan.clone()),
                    ("stopping".into(), Value::Bool(stopping)),
                ])
                .unwrap();
                let expected = python(&oracle_input);
                let runner = Native {
                    cwd,
                    env,
                    candidates: vec![bad.clone(), good.clone()],
                    calls: RefCell::new(vec![]),
                    attempts: RefCell::new(vec![]),
                };
                let report = launch::run_with(&plan, &runner, &mut |_| Ok(()), stopping);
                let actual: Json = serde_json::from_slice(&report.canonical().unwrap()).unwrap();
                assert_eq!(actual, expected["report"], "{backend} {stopping} {first}");
                assert_eq!(
                    *runner.calls.borrow(),
                    expected["calls"].as_array().unwrap().clone()
                );
                let attempts = runner.attempts.borrow();
                if first == "missing-interpreter" {
                    assert_eq!(attempts.len(), runner.calls.borrow().len() * 2);
                    for pair in attempts.chunks_exact(2) {
                        assert_eq!(pair[0].0, bad);
                        assert_eq!(pair[0].1, Status::ExecFailed);
                        assert_eq!(pair[1].0, good);
                        assert!(pair[1].2 < pair[0].2);
                    }
                    assert_eq!(actual["status"], "transport_completed");
                } else if first == "exit127" {
                    assert_eq!(attempts.len(), 1);
                    assert_eq!(attempts[0].0, bad);
                    assert_eq!(attempts[0].1, Status::Exited(127));
                } else {
                    assert!(attempts.iter().all(|(p, _, _)| p == &good));
                    assert_eq!(actual["status"], "transport_completed");
                }
            }
        }
    }
}

#[test]
fn exhausted_pre_image_failures_are_known_without_stop_attempt_or_lost_progress() {
    struct FailImage;
    impl Runner for FailImage {
        fn run(&self, _: &Request) -> Output {
            Output {
                status: Status::ExecFailed,
                stdout: vec![],
                stderr: vec![],
            }
        }
    }
    let root = tempfile::tempdir().unwrap();
    file(&root.path().join("tmux"), true);
    let plan = plan(
        root.path(),
        "tmux",
        &BTreeMap::from([("PATH".into(), ".:.".into())]),
    );
    for stopping in [false, true] {
        let report = launch::run_with(&plan, &FailImage, &mut |_| Ok(()), stopping);
        assert!(matches!(
            get(&report, "outcome_unknown"),
            Value::Bool(false)
        ));
        assert_eq!(get(&report, "completed_steps").canonical().unwrap(), b"0");
        assert_eq!(
            get(&get(&report, "diagnostics").as_array().unwrap()[0], "error").as_str(),
            Some("OSError")
        );
        if stopping {
            assert!(matches!(
                get(&report, "stop_command_attempted"),
                Value::Bool(false)
            ));
        }
    }

    struct StopFailure {
        status: Status,
        calls: RefCell<usize>,
    }
    impl Runner for StopFailure {
        fn run(&self, request: &Request) -> Output {
            assert!(Path::new(&request.argv[0]).is_absolute());
            let mut calls = self.calls.borrow_mut();
            *calls += 1;
            Output {
                status: if *calls == 1 {
                    Status::Exited(0)
                } else {
                    self.status
                },
                stdout: vec![],
                stderr: vec![],
            }
        }
    }
    for status in [
        Status::ExecFailed,
        Status::SpawnError,
        Status::Timeout,
        Status::OutputLimit,
        Status::Exited(127),
    ] {
        let runner = StopFailure {
            status,
            calls: RefCell::new(0),
        };
        let report = launch::run_with(&plan, &runner, &mut |_| Ok(()), true);
        let image_failed = status == Status::ExecFailed;
        assert_eq!(*runner.calls.borrow(), if image_failed { 3 } else { 2 });
        assert_eq!(get(&report, "completed_steps").canonical().unwrap(), b"1");
        assert!(matches!(
            get(&report, "stop_command_succeeded"),
            Value::Bool(false)
        ));
        assert_eq!(
            get(&report, "stop_command_attempted").canonical().unwrap(),
            if image_failed {
                b"false".as_slice()
            } else {
                b"true".as_slice()
            }
        );
        assert_eq!(
            get(&report, "outcome_unknown").canonical().unwrap(),
            if image_failed {
                b"false".as_slice()
            } else {
                b"true".as_slice()
            }
        );
    }
}
