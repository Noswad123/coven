use super::*;
use crate::runtime_foreground::{NativeRunner, Runner};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::PathBuf,
    sync::OnceLock,
    time::{Duration, Instant},
};

fn fixture() -> &'static Path {
    static FIXTURE: OnceLock<(tempfile::TempDir, PathBuf)> = OnceLock::new();
    &FIXTURE
        .get_or_init(|| {
            let root = tempfile::tempdir().unwrap();
            let executable = root.path().join("inert-foreground-cli");
            assert!(
                Command::new("rustc")
                    .arg(Path::new(file!()).with_file_name("fixture.rs"))
                    .args(["--edition=2024", "-o"])
                    .arg(&executable)
                    .status()
                    .unwrap()
                    .success()
            );
            (root, executable)
        })
        .1
}

fn request(mode: &str, root: &Path) -> Request {
    Request {
        argv: vec![
            fixture().to_str().unwrap().into(),
            mode.into(),
            root.to_str().unwrap().into(),
        ],
        cwd: root.to_owned(),
        env: BTreeMap::from([
            ("ONLY".into(), "explicit".into()),
            ("EXTRA".into(), "also passed".into()),
        ]),
    }
}

fn entry(mode: &str, root: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "platform::unix::runtime_foreground::tests::isolated_entry",
        "--ignored",
        "--exact",
        "--nocapture",
        "--test-threads=1",
    ]);
    command
        .env("FOREGROUND_MODE", mode)
        .env("FOREGROUND_ROOT", root)
        .env("FOREGROUND_FIXTURE", fixture())
        .env("FOREGROUND_AMBIENT", "must not leak");
    command
}

fn wait(child: &mut std::process::Child, seconds: u64) {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "isolated entry failed: {status}");
            return;
        }
        if started.elapsed() > Duration::from_secs(seconds) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("foreground fixture exceeded test watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn pty() -> (File, File) {
    let (mut master, mut slave) = (-1, -1);
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0
    );
    let master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    (master, slave)
}

#[test]
#[ignore = "subprocess-only fixture; alters descriptor limits and SIGCHLD"]
fn isolated_entry() {
    let root = PathBuf::from(std::env::var_os("FOREGROUND_ROOT").unwrap());
    let mode = std::env::var("FOREGROUND_MODE").unwrap();
    let mut req = Request {
        argv: vec![
            std::env::var("FOREGROUND_FIXTURE").unwrap(),
            mode.clone(),
            root.to_str().unwrap().into(),
        ],
        cwd: root.clone(),
        env: BTreeMap::from([
            ("ONLY".into(), "explicit".into()),
            ("EXTRA".into(), "also passed".into()),
        ]),
    };
    let mut descriptors = Vec::new();
    if mode == "fds" {
        let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
        assert_eq!(
            unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) },
            0
        );
        let mut limit = unsafe { limit.assume_init() };
        limit.rlim_cur = limit.rlim_cur.max(4096).min(limit.rlim_max);
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
        let source = File::open("/dev/null").unwrap();
        for minimum in [3, 2048] {
            let fd = unsafe { libc::fcntl(source.as_raw_fd(), libc::F_DUPFD, minimum) };
            assert!(fd >= minimum);
            req.argv.push(fd.to_string());
            descriptors.push(unsafe { OwnedFd::from_raw_fd(fd) });
        }
        limit.rlim_cur = 1024;
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
    }
    if mode == "sigchld" {
        unsafe {
            libc::signal(libc::SIGCHLD, libc::SIG_IGN);
        }
        assert_eq!(NativeRunner.run(&req), Status::SpawnError);
        std::fs::write(root.join("status"), b"rejected").unwrap();
        return;
    }
    let mask = crate::runtime_foreground::terminal_ttys();
    let status = NativeRunner.run(&req);
    assert_eq!(status, Status::Exited(0));
    if mode == "stdio" {
        assert_eq!(
            std::fs::read_to_string(root.join("mask")).unwrap(),
            format!("{} {}", i32::from(mask.0), i32::from(mask.1))
        );
    }
    if mode == "fds" {
        for fd in &descriptors {
            assert_eq!(
                unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) },
                0,
                "parent flags changed"
            );
        }
        req.argv[0] = root.join("missing-executable").to_str().unwrap().into();
        assert_eq!(
            NativeRunner.run(&req),
            Status::ExecFailed,
            "exec-error handshake lost"
        );
        // Exercise the portable sweep even on Linux with close_range support.
        req.argv[0] = std::env::var("FOREGROUND_FIXTURE").unwrap();
        let bound = descriptor_bound().unwrap();
        assert!(bound >= descriptors[1].as_raw_fd());
        let mut command = Command::new(&req.argv[0]);
        command.args(&req.argv[1..]);
        unsafe {
            command.pre_exec(move || super::super::runtime_child::sweep_cloexec(bound));
        }
        assert!(command.status().unwrap().success());
        for fd in &descriptors {
            assert_eq!(unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) }, 0);
        }
    }
    std::fs::write(root.join("status"), b"returned").unwrap();
}

#[test]
fn inherited_stdio_all_four_tty_masks_reads_literal_input_and_writes_output() {
    for stdin_tty in [false, true] {
        for stdout_tty in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut command = entry("stdio", root.path());
            let (mut input, slave_in) = pty();
            let (mut output, slave_out) = pty();
            if stdin_tty {
                command.stdin(slave_in);
            } else {
                command.stdin(Stdio::piped());
                drop(slave_in);
            }
            if stdout_tty {
                command.stdout(slave_out);
            } else {
                command.stdout(Stdio::piped());
                drop(slave_out);
            }
            command.stderr(Stdio::piped());
            let mut child = command.spawn().unwrap();
            drop(command);
            if stdin_tty {
                input.write_all(b"literal-input\n").unwrap();
            } else {
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(b"literal-input\n")
                    .unwrap();
            }
            wait(&mut child, 10);
            let mut bytes = Vec::new();
            if stdout_tty {
                let mut buffer = [0; 4096];
                loop {
                    match output.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => bytes.extend_from_slice(&buffer[..n]),
                        Err(e) if e.raw_os_error() == Some(libc::EIO) => break,
                        Err(e) => panic!("PTY read: {e}"),
                    }
                }
            } else {
                child
                    .stdout
                    .take()
                    .unwrap()
                    .read_to_end(&mut bytes)
                    .unwrap();
            }
            assert!(String::from_utf8_lossy(&bytes).contains("foreground-stdout"));
            let mut stderr = String::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut stderr)
                .unwrap();
            assert!(stderr.contains("foreground-stderr"));
            assert_eq!(
                std::fs::read_to_string(root.path().join("mask")).unwrap(),
                format!("{} {}", i32::from(stdin_tty), i32::from(stdout_tty))
            );
            assert_eq!(
                std::fs::read(root.path().join("status")).unwrap(),
                b"returned"
            );
        }
    }
}

#[test]
fn high_and_low_fds_above_lowered_limit_are_cloexec_only_in_child() {
    let root = tempfile::tempdir().unwrap();
    let mut child = entry("fds", root.path()).spawn().unwrap();
    wait(&mut child, 10);
    assert_eq!(
        std::fs::read(root.path().join("image")).unwrap(),
        b"executed"
    );
    assert_eq!(
        std::fs::read(root.path().join("status")).unwrap(),
        b"returned"
    );
}

#[test]
fn rejects_nondefault_sigchld_in_isolated_process() {
    let root = tempfile::tempdir().unwrap();
    let mut child = entry("sigchld", root.path()).spawn().unwrap();
    wait(&mut child, 10);
    assert_eq!(
        std::fs::read(root.path().join("status")).unwrap(),
        b"rejected"
    );
}

#[test]
fn validation_exec_failure_actual_exit_and_negative_signal_are_distinct() {
    let root = tempfile::tempdir().unwrap();
    let mut req = request("exit127", root.path());
    assert_eq!(NativeRunner.run(&req), Status::Exited(127));
    assert_eq!(
        std::fs::read(root.path().join("image")).unwrap(),
        b"executed"
    );
    req.argv[1] = "signal".into();
    assert_eq!(NativeRunner.run(&req), Status::Exited(-libc::SIGTERM));
    req.argv[0] = root.path().join("absent").to_str().unwrap().into();
    assert_eq!(NativeRunner.run(&req), Status::ExecFailed);
    for invalid in [
        "empty",
        "relative-exe",
        "relative-cwd",
        "nul-arg",
        "bad-env",
        "nul-env",
    ] {
        let mut req = request("exit127", root.path());
        match invalid {
            "empty" => req.argv.clear(),
            "relative-exe" => req.argv[0] = "relative".into(),
            "relative-cwd" => req.cwd = "relative".into(),
            "nul-arg" => req.argv.push("bad\0arg".into()),
            "bad-env" => {
                req.env.insert("bad=key".into(), "x".into());
            }
            _ => {
                req.env.insert("key".into(), "bad\0value".into());
            }
        }
        assert_eq!(NativeRunner.run(&req), Status::SpawnError, "{invalid}");
    }
    let mut req = request("exit127", root.path());
    req.cwd = root.path().join("absent");
    assert_eq!(NativeRunner.run(&req), Status::ExecFailed);
}

#[test]
fn inherited_output_has_no_capture_budget() {
    let root = tempfile::tempdir().unwrap();
    let stdout = File::create(root.path().join("stdout")).unwrap();
    let stderr = File::create(root.path().join("stderr")).unwrap();
    let mut child = entry("large", root.path())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .unwrap();
    wait(&mut child, 10);
    let bytes = std::fs::read(root.path().join("stdout")).unwrap();
    assert!(
        bytes
            .windows(1_048_577)
            .any(|bytes| bytes.iter().all(|b| *b == b'x'))
    );
    assert_eq!(
        std::fs::read(root.path().join("stderr")).unwrap(),
        b"uncaptured-stderr"
    );
    assert_eq!(
        std::fs::read(root.path().join("status")).unwrap(),
        b"returned"
    );
}

#[test]
fn reaps_direct_child_without_waiting_for_or_cleaning_up_daemon_effects() {
    let root = tempfile::tempdir().unwrap();
    let unrelated_root = tempfile::tempdir().unwrap();
    let mut unrelated = Command::new(fixture())
        .arg("effect")
        .arg(unrelated_root.path())
        .spawn()
        .unwrap();
    assert_eq!(
        NativeRunner.run(&request("daemon", root.path())),
        Status::Exited(0)
    );
    let pid: i32 = std::fs::read_to_string(root.path().join("pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
    let started = Instant::now();
    while !root.path().join("effect").exists() && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read(root.path().join("effect")).unwrap(),
        b"survived"
    );
    assert!(unrelated.wait().unwrap().success());
    assert_eq!(
        std::fs::read(unrelated_root.path().join("effect")).unwrap(),
        b"survived"
    );
}

#[test]
fn foreground_can_run_beyond_sixty_seconds_without_inherited_capture_deadline() {
    let root = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let mut child = entry("long", root.path()).spawn().unwrap();
    wait(&mut child, 75);
    assert!(started.elapsed() >= Duration::from_secs(61));
    assert_eq!(
        std::fs::read(root.path().join("long-effect")).unwrap(),
        b"past capture deadline"
    );
    assert_eq!(
        std::fs::read(root.path().join("effect")).unwrap(),
        b"survived"
    );
    assert_eq!(
        std::fs::read(root.path().join("status")).unwrap(),
        b"returned"
    );
}
