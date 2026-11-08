use super::*;
use crate::runtime_command::{NativeRunner, Runner};
use crate::transport_gate::TransportGuard;
use std::{
    collections::BTreeMap,
    os::unix::fs::{MetadataExt, symlink},
    sync::OnceLock,
};

fn fixture() -> &'static Path {
    static FIXTURE: OnceLock<(tempfile::TempDir, std::path::PathBuf)> = OnceLock::new();
    &FIXTURE
        .get_or_init(|| {
            let root = tempfile::tempdir().unwrap();
            let executable = root.path().join("inert-runtime-child");
            let status = Command::new("rustc")
                .arg(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("src/platform/unix/runtime_command/fixture.rs"),
                )
                .arg("--edition=2024")
                .arg("-o")
                .arg(&executable)
                .status()
                .unwrap();
            assert!(status.success());
            (root, executable)
        })
        .1
}

fn request(mode: &str) -> Request {
    Request {
        argv: vec![fixture().to_str().unwrap().to_owned(), mode.to_owned()],
        cwd: std::env::temp_dir().canonicalize().unwrap(),
        env: BTreeMap::from([("ONLY".into(), "explicit".into())]),
        timeout: Duration::from_secs(2),
        max_output_bytes: 1_048_576,
    }
}

#[test]
fn native_exit_env_null_stdin_stderr_and_exact_combined_cap() {
    let output = NativeRunner.run(&request("success"));
    assert_eq!(output.status, Status::Exited(7));
    assert_eq!(output.stdout, b"success");
    assert_eq!(output.stderr, b"stderr");
    let mut req = request("bytes");
    req.max_output_bytes = 8;
    let output = NativeRunner.run(&req);
    assert_eq!(output.status, Status::Exited(0));
    assert_eq!(output.stdout, b"abcd");
    assert_eq!(output.stderr, b"EFGH");
    for cap in [0, 1, 7] {
        req.max_output_bytes = cap;
        let output = NativeRunner.run(&req);
        assert_eq!(output.status, Status::OutputLimit);
        assert_eq!(output.stdout.len() + output.stderr.len(), cap);
    }
}

#[test]
fn malformed_requests_never_spawn() {
    let mut req = request("success");
    req.argv.clear();
    assert_eq!(NativeRunner.run(&req).status, Status::SpawnError);
    // A real exec failure must reach Rust's private exec-error pipe, not look
    // like a successfully spawned child exiting with an implementation code.
    req = request("success");
    req.argv[0] = "/nonexistent-coven-runtime-executable".into();
    assert_eq!(NativeRunner.run(&req).status, Status::ExecFailed);
    req = request("success");
    req.env.insert("bad=key".into(), "x".into());
    assert_eq!(NativeRunner.run(&req).status, Status::SpawnError);
    req = request("success");
    req.argv.push("nul\0byte".into());
    assert_eq!(NativeRunner.run(&req).status, Status::SpawnError);
    req = request("success");
    req.cwd = "/nonexistent-coven-runtime-directory".into();
    assert_eq!(NativeRunner.run(&req).status, Status::ExecFailed);
    req = request("success");
    req.argv[0] = "relative-executable".into();
    assert_eq!(NativeRunner.run(&req).status, Status::SpawnError);
}

#[test]
fn failed_exec_is_distinct_from_executed_image_returning_127() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("image-effect");
    let executable = root.path().join("missing-interpreter");
    std::fs::write(
        &executable,
        b"#!/nonexistent-coven-interpreter\nnot executed\n",
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut req = request("exit127");
    req.argv.push(marker.to_str().unwrap().into());
    req.argv[0] = executable.to_str().unwrap().into();
    let output = NativeRunner.run(&req);
    assert_eq!(output.status, Status::ExecFailed);
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    assert!(!marker.exists());

    // The native fixture proves its image executed before returning 127.
    req.argv[0] = fixture().to_str().unwrap().into();
    assert_eq!(NativeRunner.run(&req).status, Status::Exited(127));
    assert_eq!(std::fs::read(&marker).unwrap(), b"image executed");
    std::fs::remove_file(&marker).unwrap();
    req.cwd = root.path().join("missing-cwd");
    assert_eq!(NativeRunner.run(&req).status, Status::ExecFailed);
    assert!(!marker.exists());
}

#[test]
#[ignore = "subprocess-only inherited descriptor fixture"]
fn inherited_descriptors_entry() {
    let root = tempfile::tempdir().unwrap();
    let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) },
        0
    );
    let mut limit = unsafe { limit.assume_init() };
    limit.rlim_cur = limit.rlim_cur.max(4096).min(limit.rlim_max);
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
    let source = File::open("/dev/null").unwrap();
    let low = unsafe { libc::fcntl(source.as_raw_fd(), libc::F_DUPFD, 3) };
    assert!(low >= 3);
    let low = unsafe { OwnedFd::from_raw_fd(low) };
    let high = unsafe { libc::fcntl(source.as_raw_fd(), libc::F_DUPFD, 2048) };
    assert!((2048..4096).contains(&high));
    let high = unsafe { OwnedFd::from_raw_fd(high) };
    // Keep a live descriptor above a lowered limit, not merely above FD_SETSIZE.
    limit.rlim_cur = 1024;
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
    let bound = descriptor_bound().unwrap();
    assert!(bound >= high.as_raw_fd());
    let executable = std::env::var("COVEN_RUNTIME_FD_FIXTURE_EXE").unwrap();
    // Exercise the actual runner for both lookup-like and delivery-like calls.
    for phase in ["lookup", "delivery"] {
        let marker = root.path().join(phase);
        let req = Request {
            argv: vec![
                executable.clone(),
                "fd-backend".into(),
                marker.to_str().unwrap().into(),
            ],
            cwd: root.path().to_owned(),
            env: BTreeMap::new(),
            timeout: Duration::from_secs(2),
            max_output_bytes: 1024,
        };
        let output = NativeRunner.run(&req);
        assert_eq!(output.status, Status::Exited(0));
        assert_eq!(output.stdout, b"\n", "backend inherited descriptors");
        let started = Instant::now();
        while !marker.exists() && started.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read(&marker).unwrap(),
            b"",
            "daemon inherited descriptors"
        );
    }
    // Real send lookup and delivery, using only our compiled inert backend.
    let bin = root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::copy(&executable, bin.join("tmux")).unwrap();
    std::fs::write(
        root.path().join("coven.json"),
        br#"{"name":"FD Test","agents":["Beta"],"multiplexer":"tmux"}"#,
    )
    .unwrap();
    let log = root.path().join("backend-fds");
    // This entry runs alone in its own process; no shared harness environment changes.
    unsafe {
        std::env::set_var("PATH", &bin);
        std::env::set_var("COVEN_FD_TEST_LOG", &log);
    }
    let result = crate::runtime::send(root.path(), "Beta", "descriptor regression").unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "list-panes:\nsend-keys:\n"
    );
    // Also exercise the portable sweep independently of Linux's fast path.
    let output = unsafe {
        Command::new(&executable)
            .args(["fd-backend", root.path().join("sweep").to_str().unwrap()])
            .pre_exec(move || sweep_cloexec(bound))
            .output()
            .unwrap()
    };
    assert!(output.status.success());
    assert_eq!(output.stdout, b"\n");
    let started = Instant::now();
    while !root.path().join("sweep").exists() && started.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(std::fs::read(root.path().join("sweep")).unwrap(), b"");
    for fd in [low.as_raw_fd(), high.as_raw_fd()] {
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_GETFD) },
            0,
            "parent flags changed"
        );
    }
}

#[test]
fn inherited_descriptors_are_not_passed_to_native_backend_or_daemon() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "inherited_descriptors_entry",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("COVEN_RUNTIME_FD_FIXTURE_EXE", fixture())
        .spawn()
        .unwrap();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if started.elapsed() > Duration::from_secs(10) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("inherited-descriptor subprocess exceeded deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_reaped(pid: i32) {
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn timeout_reaps_only_direct_child_and_leaves_unrelated_child_alive() {
    let mut unrelated = Command::new(fixture())
        .arg("sleep")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut req = request("sleep");
    req.timeout = Duration::from_millis(150);
    let started = Instant::now();
    let output = NativeRunner.run(&req);
    assert_eq!(output.status, Status::Timeout);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_reaped(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap(),
    );
    assert!(unrelated.try_wait().unwrap().is_none());
    unrelated.kill().unwrap();
    unrelated.wait().unwrap();
}

#[test]
fn daemon_effects_survive_exit_and_timeout_with_or_without_inherited_pipes() {
    for (mode, expected) in [
        ("parent", Status::Timeout),
        ("parent-sleep", Status::Timeout),
        ("parent-closed", Status::Exited(0)),
        ("parent-flood", Status::OutputLimit),
    ] {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("daemon-effect");
        let mut req = request(mode);
        req.argv.push(marker.to_str().unwrap().into());
        req.timeout = Duration::from_millis(200);
        req.max_output_bytes = 1024;
        let output = NativeRunner.run(&req);
        assert_eq!(output.status, expected, "{mode}");
        let pid = std::str::from_utf8(&output.stdout)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_reaped(pid);
        let started = Instant::now();
        while !marker.exists() && started.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(std::fs::read(marker).unwrap(), b"survived", "{mode}");
    }
}

#[test]
fn gate_inode_cloexec_and_static_redirection_safety() {
    let root = tempfile::tempdir().unwrap();
    let guard = TransportGuard::shared(root.path(), Duration::ZERO).unwrap();
    let lock = root.path().join("state/.runtime-transport.lock");
    let inode = std::fs::metadata(&lock).unwrap().ino();
    let file = open_transport_lock(root.path()).unwrap();
    assert_ne!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
        0
    );
    drop(file);
    drop(guard);
    let _guard = TransportGuard::exclusive(root.path(), Duration::ZERO).unwrap();
    assert_eq!(std::fs::metadata(&lock).unwrap().ino(), inode);
    let other = tempfile::tempdir().unwrap();
    symlink(root.path().join("state"), other.path().join("state")).unwrap();
    assert!(TransportGuard::shared(other.path(), Duration::ZERO).is_err());
    let other = tempfile::tempdir().unwrap();
    std::fs::create_dir(other.path().join("state")).unwrap();
    symlink(&lock, other.path().join("state/.runtime-transport.lock")).unwrap();
    assert!(TransportGuard::shared(other.path(), Duration::ZERO).is_err());
}

#[test]
fn forked_drop_does_not_unlock_parent_open_file_description() {
    let root = tempfile::tempdir().unwrap();
    let guard = TransportGuard::exclusive(root.path(), Duration::ZERO).unwrap();
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        // Avoid allocator/runtime work in the multithreaded post-fork child.
        if guard.validate_for(root.path(), false).is_ok() {
            unsafe { libc::_exit(1) };
        }
        drop(guard);
        unsafe { libc::_exit(0) };
    }
    let mut status = 0;
    assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
    assert_eq!(status, 0);
    assert!(TransportGuard::shared(root.path(), Duration::ZERO).is_err());
    drop(guard);
    TransportGuard::exclusive(root.path(), Duration::ZERO).unwrap();
}

#[test]
#[ignore = "subprocess-only fixture; closes standard descriptors"]
fn closed_stdio_entry() {
    let executable = std::env::var("COVEN_RUNTIME_FD_FIXTURE_EXE").unwrap();
    let req = Request {
        argv: vec![executable, "success".into()],
        cwd: std::env::temp_dir(),
        env: BTreeMap::from([("ONLY".into(), "explicit".into())]),
        timeout: Duration::from_secs(2),
        max_output_bytes: 1024,
    };
    for fd in 0..=2 {
        unsafe { libc::close(fd) };
    }
    let output = NativeRunner.run(&req);
    let valid = output.status == Status::Exited(7)
        && output.stdout == b"success"
        && output.stderr == b"stderr";
    unsafe { libc::_exit(if valid { 0 } else { 1 }) };
}

#[test]
fn closed_standard_descriptors_are_handled_in_isolated_process() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "closed_stdio_entry",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("COVEN_RUNTIME_FD_FIXTURE_EXE", fixture())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if started.elapsed() > Duration::from_secs(5) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("closed-descriptor subprocess exceeded deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
