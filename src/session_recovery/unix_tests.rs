use super::*;
use crate::interactive_session_fixture as fixture;
use fixture::{Server, ack, frame, inventory, snapshot};
use std::{
    cell::RefCell,
    fs,
    io::{Read, Seek, SeekFrom},
    process::{Child, Command, Output},
    time::Instant,
};

struct OwnedChild(Child, bool);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.1 {
            let _ = self.0.kill();
            self.0.wait().expect("reap owned recovery test child");
        }
    }
}

fn child_output(command: &mut Command, timeout: Duration) -> std::io::Result<Output> {
    // Parent-owned storage also cleans fixtures when a child times out or dies.
    let artifacts = tempfile::Builder::new().prefix("a5c-").tempdir()?;
    let mut stdout = tempfile::tempfile_in(artifacts.path())?;
    let mut stderr = tempfile::tempfile_in(artifacts.path())?;
    command
        .env("TMPDIR", artifacts.path())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        let parent = std::process::id() as libc::pid_t;
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                // Close the race where the parent died before prctl completed.
                if libc::getppid() != parent {
                    libc::_exit(125);
                }
                Ok(())
            });
        }
    }
    let mut child = OwnedChild(command.spawn()?, false);
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            child.1 = true;
            break status;
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "recovery test child deadline",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let read = |file: &mut fs::File| -> std::io::Result<Vec<u8>> {
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.take(65_537).read_to_end(&mut bytes)?;
        if bytes.len() > 65_536 {
            return Err(std::io::Error::other("recovery test child output limit"));
        }
        Ok(bytes)
    };
    Ok(Output {
        status,
        stdout: read(&mut stdout)?,
        stderr: read(&mut stderr)?,
    })
}

fn isolated(name: &str) -> bool {
    if std::env::var("COVEN_A5_ISOLATED_TEST").as_deref() == Ok(name) {
        return false;
    }
    let output = child_output(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("COVEN_A5_ISOLATED_TEST", name),
        Duration::from_secs(20),
    )
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    true
}

fn workspace() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    crate::init::arguments(
        temp.path(),
        &crate::init::Options {
            blank: true,
            agents: "reviewer".into(),
            ..Default::default()
        },
    )
    .unwrap();
    temp
}
fn request(root: &Path, socket: &Path) -> Request {
    let executable = fixture::executable(root);
    let file = root.join("selection.json");
    fs::write(&file, serde_json::to_vec(&json!({"schema_version":1,"entries":[{
        "key":"one","reference":{"source":"kitsune:djinn","runtime":"djinn","kind":"id","value":"saved"},
        "executable":executable,"pane_id":"w1:p1","expected_terminal_id":"terminal-1"
    }]})).unwrap()).unwrap();
    Request {
        workspace: root.into(),
        file,
        socket: socket.to_str().unwrap().into(),
        expect_recovery_revision: load(root).unwrap().1,
        execute: true,
    }
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
fn writes(server: &Server) -> usize {
    server
        .requests()
        .iter()
        .filter(|req| req["method"] == "pane.submit_command")
        .count()
}

struct DirectoryIo {
    root: PathBuf,
    trace: RefCell<Vec<PathBuf>>,
    fail: Option<PathBuf>,
}
impl storage::EvidenceIo for DirectoryIo {
    fn sync_dir(&self, path: &Path) -> std::io::Result<()> {
        self.trace.borrow_mut().push(path.into());
        if self.fail.as_deref() == Some(path) {
            Err(std::io::Error::other("injected directory sync failure"))
        } else {
            crate::platform::sync_directory(path)
        }
    }
}
impl RecoveryIo for DirectoryIo {
    fn prepare_directory(&self, guard: &WorkspaceGuard) -> Result<()> {
        storage::prepare_directory_with(guard, Path::new("state"), self)
    }
    fn after_intent(&self) {
        assert_eq!(
            *self.trace.borrow(),
            vec![self.root.join("state"), self.root.clone()]
        );
        assert!(load(&self.root).unwrap().0.attempts[0].outcome == Outcome::SubmissionIntent);
        // The callback has released the workspace guard before returning to A1.
        WorkspaceGuard::existing(&self.root, Duration::ZERO).unwrap();
    }
}

#[test]
fn absent_state_syncs_state_and_workspace_before_send_and_each_failure_prevents_send() {
    if isolated(
        "session_recovery::unix_tests::absent_state_syncs_state_and_workspace_before_send_and_each_failure_prevents_send",
    ) {
        return;
    }
    for fail_parent in [None, Some(false), Some(true)] {
        let temp = workspace();
        fs::remove_dir_all(temp.path().join("state")).unwrap();
        let server = normal();
        let req = request(temp.path(), &server.path);
        assert!(!temp.path().join("state").exists());
        let io = DirectoryIo {
            root: temp.path().into(),
            trace: RefCell::new(vec![]),
            fail: fail_parent.map(|parent| {
                if parent {
                    temp.path().into()
                } else {
                    temp.path().join("state")
                }
            }),
        };
        let output = run_with_io(&req, true, &io).unwrap();
        let report: Value = serde_json::from_str(&output.stdout).unwrap();
        if fail_parent.is_some() {
            assert_eq!(output.exit_code, 2);
            assert_eq!(report["entries"][0]["outcome"], "intent_persistence_failed");
            assert_eq!(writes(&server), 0);
            assert!(load(temp.path()).unwrap().0.attempts.is_empty());
            assert!(!temp.path().join(STATE).exists());
        } else {
            assert_eq!(output.exit_code, 0);
            assert_eq!(writes(&server), 1);
            assert_eq!(
                *io.trace.borrow(),
                vec![
                    temp.path().join("state"),
                    temp.path().into(),
                    temp.path().join("state"),
                    temp.path().into()
                ]
            );
        }
    }
}

struct SlowIo;
impl RecoveryIo for SlowIo {
    fn after_intent(&self) {
        std::thread::sleep(Duration::from_millis(3100));
    }
}

#[test]
fn slow_durable_intent_is_known_unsent_and_does_not_block_explicit_retry() {
    if isolated(
        "session_recovery::unix_tests::slow_durable_intent_is_known_unsent_and_does_not_block_explicit_retry",
    ) {
        return;
    }
    let temp = workspace();
    let server = normal();
    let mut req = request(temp.path(), &server.path);
    let output = run_with_io(&req, true, &SlowIo).unwrap();
    let report: Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(output.exit_code, 1, "{report}");
    assert_eq!(report["entries"][0]["delivery"], "unsent");
    assert_eq!(report["entries"][0]["outcome"], "unsent");
    assert_eq!(writes(&server), 0);
    let (state, token) = load(temp.path()).unwrap();
    assert!(state.attempts[0].outcome == Outcome::Unsent);
    assert!(!state.attempts[0].outcome.blocks());
    req.expect_recovery_revision = token;
    assert_eq!(run(&req, true).unwrap().exit_code, 0);
    assert_eq!(writes(&server), 1);
}

struct KillIo {
    stage: String,
}
impl RecoveryIo for KillIo {
    fn after_intent(&self) {
        if self.stage == "after_intent" {
            unsafe {
                libc::raise(libc::SIGKILL);
            }
        }
    }
    fn before_result(&self) {
        if self.stage == "before_result" {
            unsafe {
                libc::raise(libc::SIGKILL);
            }
        }
    }
}

#[test]
#[ignore = "invoked only as an owned crash-injection child"]
fn crash_child() {
    let root = PathBuf::from(std::env::var_os("COVEN_A5_CRASH_ROOT").unwrap());
    let socket = PathBuf::from(std::env::var_os("COVEN_A5_CRASH_SOCKET").unwrap());
    let stage = std::env::var("COVEN_A5_CRASH_STAGE").unwrap();
    let req = request(&root, &socket);
    let output = run_with_io(&req, true, &KillIo { stage }).unwrap();
    let report: Value = serde_json::from_str(&output.stdout).unwrap();
    // A fresh process sees the durable blocker and never reaches either hook.
    assert_eq!(report["entries"][0]["outcome"], "skipped_journal");
}

#[test]
fn sigkill_after_intent_and_after_receipt_before_result_both_block_fresh_process() {
    use std::os::unix::process::ExitStatusExt;
    for (stage, submitted) in [("after_intent", 0), ("before_result", 1)] {
        let temp = workspace();
        let server = normal();
        let child = || {
            child_output(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "session_recovery::unix_tests::crash_child",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("COVEN_A5_CRASH_ROOT", temp.path())
                    .env("COVEN_A5_CRASH_SOCKET", &server.path)
                    .env("COVEN_A5_CRASH_STAGE", stage),
                Duration::from_secs(20),
            )
            .unwrap()
        };
        let killed = child();
        assert_eq!(killed.status.signal(), Some(libc::SIGKILL), "{killed:?}");
        assert_eq!(writes(&server), submitted);
        let state = load(temp.path()).unwrap().0;
        assert_eq!(state.attempts.len(), 1);
        assert!(state.attempts[0].outcome == Outcome::SubmissionIntent);
        let restarted = child();
        assert!(restarted.status.success(), "{restarted:?}");
        assert_eq!(writes(&server), submitted);
    }
}

#[test]
#[ignore = "invoked only as an owned deadline-cleanup child"]
fn deadline_child() {
    fs::write(
        std::env::var_os("COVEN_A5_CHILD_PID").unwrap(),
        std::process::id().to_string(),
    )
    .unwrap();
    std::thread::sleep(Duration::from_secs(60));
}

#[test]
fn child_deadline_kills_and_reaps_before_returning() {
    let temp = tempfile::tempdir().unwrap();
    let pid_file = temp.path().join("pid");
    let error = child_output(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "session_recovery::unix_tests::deadline_child",
                "--ignored",
            ])
            .env("COVEN_A5_CHILD_PID", &pid_file),
        Duration::from_secs(2),
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    let pid: libc::pid_t = fs::read_to_string(pid_file).unwrap().parse().unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}
