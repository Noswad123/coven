use super::*;
use crate::{
    runtime_command::{Output, Request},
    storage::WorkspaceGuard,
    transport_gate::TransportGuard,
};
use std::{cell::RefCell, fs, os::unix::fs::PermissionsExt};

#[path = "tests/extensions.rs"]
mod extensions;

struct CandidateRunner {
    first: Status,
    calls: RefCell<Vec<(String, Duration)>>,
    env: BTreeMap<String, String>,
}

impl Runner for CandidateRunner {
    fn run(&self, request: &Request) -> Output {
        assert!(Path::new(&request.argv[0]).is_absolute());
        assert_eq!(request.env, self.env);
        assert_eq!(request.max_output_bytes, 1_048_576);
        assert!(!request.timeout.is_zero() && request.timeout <= Duration::from_secs(60));
        assert_eq!(request.cwd, std::env::current_dir().unwrap());
        let root = Path::new(&request.env["HOME"]);
        let _workspace = WorkspaceGuard::acquire(root, Duration::ZERO).unwrap();
        assert!(TransportGuard::exclusive(root, Duration::ZERO).is_err());
        self.calls
            .borrow_mut()
            .push((request.argv[0].clone(), request.timeout));
        let bad = Path::new(&request.argv[0])
            .parent()
            .unwrap()
            .ends_with("bad");
        let status = if bad {
            assert_eq!(
                fs::read(&request.argv[0]).unwrap(),
                b"#!/missing/runtime/interpreter\n"
            );
            // Simulate Command::spawn's missing-interpreter failure. Native
            // image-execution proof is covered by the launch owner's fixture.
            std::thread::sleep(Duration::from_millis(2));
            self.first
        } else {
            Status::Exited(0)
        };
        Output {
            status,
            stdout: if request.argv[1] == "list-panes" {
                b"%12\n".to_vec()
            } else {
                Vec::new()
            },
            stderr: Vec::new(),
        }
    }
}

fn fixture() -> (tempfile::TempDir, BTreeMap<String, String>) {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("coven.json"),
        br#"{"name":"test","agents":["agent"]}"#,
    )
    .unwrap();
    for (directory, body) in [
        ("bad", "#!/missing/runtime/interpreter\n"),
        ("good", "#!/bin/sh\nexit 0\n"),
    ] {
        fs::create_dir(root.path().join(directory)).unwrap();
        let executable = root.path().join(directory).join("tmux");
        fs::write(&executable, body).unwrap();
        fs::set_permissions(executable, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut env: BTreeMap<_, _> = std::env::vars().collect();
    env.insert(
        "PATH".into(),
        format!(
            "{}:{}",
            root.path().join("bad").display(),
            root.path().join("good").display()
        ),
    );
    env.insert("HOME".into(), root.path().to_str().unwrap().into());
    env.insert("COVEN_RUNTIME_CAPTURED_ENV_TEST".into(), "preserved".into());
    (root, env)
}

#[test]
fn exec_failed_missing_interpreter_advances_path_and_send_still_completes() {
    let (root, env) = fixture();
    let _gate = TransportGuard::shared(root.path(), Duration::ZERO).unwrap();
    let runner = CandidateRunner {
        first: Status::ExecFailed,
        calls: RefCell::new(Vec::new()),
        env: env.clone(),
    };
    let mut backend = Backend::new("tmux", &runner).unwrap();
    backend.env = env;
    assert!(backend.send("coven-test", "agent", "message").unwrap());
    let calls = runner.calls.borrow();
    assert_eq!(calls.len(), 4);
    for command in calls.chunks_exact(2) {
        assert_eq!(Path::new(&command[0].0), root.path().join("bad/tmux"));
        assert_eq!(Path::new(&command[1].0), root.path().join("good/tmux"));
        assert!(
            command[1].1 < command[0].1,
            "fallback must not reset the deadline"
        );
    }
}

#[test]
fn exit_127_and_unknown_or_effectful_failures_never_advance_path() {
    for status in [
        Status::Exited(127),
        Status::Exited(1),
        Status::SpawnError,
        Status::Timeout,
        Status::OutputLimit,
    ] {
        let (root, env) = fixture();
        let _gate = TransportGuard::shared(root.path(), Duration::ZERO).unwrap();
        let runner = CandidateRunner {
            first: status,
            calls: RefCell::new(Vec::new()),
            env: env.clone(),
        };
        let mut backend = Backend::new("tmux", &runner).unwrap();
        backend.env = env;
        let result = backend.run(&["send-keys", "-t", "%12", "message", "Enter"]);
        match status {
            Status::Exited(code) => assert_eq!(result.unwrap().0, code),
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
            Status::ExecFailed => unreachable!(),
        }
        assert_eq!(runner.calls.borrow().len(), 1, "no fallback for {status:?}");
    }
}

#[test]
fn relative_path_uses_caller_not_workspace_in_isolated_native_process() {
    let temp = tempfile::tempdir().unwrap();
    let caller = temp.path().join("caller");
    let workspace = temp.path().join("workspace");
    let home = temp.path().join("control");
    for directory in [&caller, &workspace] {
        fs::create_dir_all(directory.join("relative")).unwrap();
    }
    fs::create_dir(&home).unwrap();
    fs::write(
        workspace.join("coven.json"),
        br#"{"name":"test","agents":["agent"],"multiplexer":"tmux"}"#,
    )
    .unwrap();
    let executable = temp.path().join("inert-backend");
    let compile =
        std::process::Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runtime/transport/cwd_fixture.rs"))
            .args(["--edition=2024", "-o"])
            .arg(&executable)
            .output()
            .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    for directory in [&caller, &workspace] {
        fs::copy(&executable, directory.join("relative/tmux")).unwrap();
    }
    fs::write(workspace.join("relative/decoy"), []).unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "runtime::transport::tests::native_caller_cwd_child",
            "--nocapture",
        ])
        .current_dir(&caller)
        .env("PATH", "relative")
        .env("HOME", &home)
        .env("COVEN_MULTIPLEXER", "tmux")
        .env("COVEN_RUNTIME_CWD_CHILD_WORKSPACE", &workspace)
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(!home.join("decoy-executed").exists());
    let calls = fs::read_to_string(home.join("caller-calls")).unwrap();
    assert_eq!(
        calls.lines().collect::<Vec<_>>(),
        vec![caller.to_str().unwrap(); 3]
    );
}

#[test]
fn native_caller_cwd_child() {
    let Some(workspace) = std::env::var_os("COVEN_RUNTIME_CWD_CHILD_WORKSPACE") else {
        return;
    };
    let workspace = PathBuf::from(workspace);
    let caller = std::env::current_dir().unwrap();
    let result = crate::runtime::send(&workspace, "agent", "message").unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.body.contains("No running pane found for: agent"));
    let result = crate::runtime::down(&workspace).unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.body.contains("Coven tmux session is not running"));
    let backend = Backend::new("tmux", &crate::runtime_command::NativeRunner).unwrap();
    assert_eq!(backend.cwd, caller);
    // This process runs only this exact helper test: no global cwd mutation in
    // the parent test harness. Selection and child cwd must remain frozen.
    std::env::set_current_dir(&workspace).unwrap();
    assert_eq!(
        backend.run(&["has-session", "-t", "coven-test"]).unwrap().0,
        127
    );
    extensions::foreground_retains_caller_context(&backend, &caller);
}
