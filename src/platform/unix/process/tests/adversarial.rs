use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct CleanupFault {
    operation: &'static str,
    events: Vec<&'static str>,
}

thread_local! {
    static CLEANUP_FAULT: std::cell::RefCell<Option<CleanupFault>> = const { std::cell::RefCell::new(None) };
}

// Observe/report faults only after the real cleanup operation. This keeps fault
// tests from deliberately leaving a live group or an unreaped supervisor behind.
pub(in crate::platform::unix::process) fn cleanup_result<T>(
    operation: &'static str,
    result: io::Result<T>,
) -> io::Result<T> {
    CLEANUP_FAULT.with_borrow_mut(|state| {
        if let Some(state) = state {
            state.events.push(operation);
            if state.operation == operation {
                return Err(io::Error::other("injected cleanup failure"));
            }
        }
        result
    })
}

pub(super) fn run(binary: &Path, root: &Path) {
    let fixture = root.join("probe-fixture");
    let compiler = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let mut build = Command::new(compiler)
        .args(["--edition", "2024"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/probe_fixture.rs"))
        .arg("-o")
        .arg(&fixture)
        .spawn()
        .unwrap();
    assert!(wait(&mut build, Duration::from_secs(30)).success());

    let request = ProbeRequest {
        executable: fixture.clone(),
        arguments: vec!["delayed-output".into()],
        cwd: root.into(),
        environment: BTreeMap::new(),
        timeout: Duration::from_secs(2),
        max_output_bytes: 25,
    };
    let result = run_with_supervisor(&request, binary);
    assert_eq!(result.status, ProbeStatus::Nonzero, "{result:?}");
    assert_eq!(result.returncode, Some(7));
    assert_eq!(result.stdout, b"prefix-suffix");
    assert_eq!(result.stderr, b"error-suffix");
    assert!(
        result.duration_ms >= 120.0,
        "returned before delayed EOF: {result:?}"
    );

    for mask in 0..8 {
        let mut caller = Command::new(std::env::current_exe().unwrap())
            .args([
                "platform::unix::process::tests::adversarial::closed_stdio_caller",
                "--exact",
                "--nocapture",
            ])
            .env("COVEN_PROCESS_STDIO_MASK", mask.to_string())
            .env("COVEN_PROCESS_FIXTURE", &fixture)
            .spawn()
            .unwrap();
        assert!(
            wait(&mut caller, Duration::from_secs(15)).success(),
            "closed stdio mask {mask}"
        );
    }
    malformed_bootstrap(binary, root);
    cleanup_failures(binary, &fixture, root);
}

fn cleanup_failures(binary: &Path, fixture: &Path, root: &Path) {
    let request = ProbeRequest {
        executable: fixture.into(),
        arguments: vec!["descriptors".into()],
        cwd: root.into(),
        environment: BTreeMap::new(),
        timeout: Duration::from_secs(2),
        max_output_bytes: 1024,
    };
    for operation in ["signal", "shutdown", "close", "reap"] {
        CLEANUP_FAULT.with_borrow_mut(|state| {
            *state = Some(CleanupFault {
                operation,
                events: Vec::new(),
            })
        });
        let result = run_with_supervisor(&request, binary);
        let events = CLEANUP_FAULT.with_borrow_mut(|state| state.take().unwrap().events);
        assert_eq!(
            result.status,
            ProbeStatus::SpawnError,
            "{operation}: {result:?}"
        );
        assert_eq!(
            result.returncode,
            Some(0),
            "native success must not hide cleanup failure"
        );
        assert_eq!(result.stdout, b"only-stdio");
        assert!(result.duration_ms < 2000.0, "{operation}: {result:?}");
        assert_eq!(
            events,
            ["signal", "shutdown", "close", "close", "close", "reap"],
            "{operation}: all cleanup precedes the sole reap; nothing signals after reap"
        );
    }
}

fn wait(child: &mut std::process::Child, budget: Duration) -> std::process::ExitStatus {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("controlled child exceeded {budget:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn closed_stdio_caller() {
    let Ok(mask) = std::env::var("COVEN_PROCESS_STDIO_MASK") else {
        return;
    };
    let mask: u8 = mask.parse().unwrap();
    let binary = PathBuf::from(std::env::var_os("COVEN_TEST_SUPERVISOR").unwrap());
    let fixture = PathBuf::from(std::env::var_os("COVEN_PROCESS_FIXTURE").unwrap());
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("inheritable-churn-marker");
    File::create(&marker).unwrap();
    let mut request = ProbeRequest {
        executable: fixture,
        arguments: vec!["descriptors".into()],
        cwd: root.path().into(),
        environment: BTreeMap::new(),
        timeout: Duration::from_secs(2),
        max_output_bytes: 1024,
    };
    // Only this disposable harness process loses stdio, never the outer suite.
    for fd in 0..3 {
        if mask & (1 << fd) != 0 {
            assert_eq!(unsafe { libc::close(fd) }, 0);
        }
    }
    // Allocate pipes before bootstrap can occupy the closed slots. Every mask
    // deterministically exercises low-slot allocation without relying on churn.
    let (reader, writer) = output_pipe().unwrap();
    assert!(reader.as_raw_fd() >= 10);
    assert_eq!(
        unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_GETFD) },
        libc::FD_CLOEXEC
    );
    assert!(writer.as_raw_fd() >= 10);
    assert_eq!(
        unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETFD) },
        libc::FD_CLOEXEC
    );
    let mut reader = File::from(reader);
    let mut writer = File::from(writer);
    writer.write_all(b"low-slot-pipe").unwrap();
    drop(writer);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"low-slot-pipe");
    drop(reader);
    for fd in 0..3 {
        if mask & (1 << fd) != 0 {
            assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
        }
    }
    // First exercise deterministically closed stdio without a churn thread that
    // could reopen the low slots before the runner starts.
    let result = run_with_supervisor(&request, &binary);
    assert_eq!(
        result.status,
        ProbeStatus::Ok,
        "closed mask {mask}: {result:?}"
    );
    assert_eq!(result.stdout, b"only-stdio");
    assert!(result.stderr.is_empty());
    for fd in 0..3 {
        if mask & (1 << fd) != 0 {
            assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let churn_stop = stop.clone();
    let churn = std::thread::spawn(move || {
        while !churn_stop.load(Ordering::Relaxed) {
            let files: Vec<_> = (0..16)
                .map(|_| {
                    let file = File::open(&marker).unwrap();
                    assert_eq!(
                        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, 0) },
                        0
                    );
                    file
                })
                .collect();
            std::thread::sleep(Duration::from_micros(100));
            drop(files);
        }
    });
    for _ in 0..12 {
        let result = run_with_supervisor(&request, &binary);
        assert_eq!(result.status, ProbeStatus::Ok, "mask {mask}: {result:?}");
        assert_eq!(result.returncode, Some(0));
        assert_eq!(result.stdout, b"only-stdio");
        assert!(result.stderr.is_empty());
        let valid = request.executable.clone();
        request.executable = root.path().join("missing-native");
        let result = run_with_supervisor(&request, &binary);
        assert_eq!(
            result.status,
            ProbeStatus::SpawnError,
            "mask {mask}: {result:?}"
        );
        assert_eq!(result.returncode, None);
        request.executable = valid;
        let result = run_with_supervisor(&request, &root.path().join("missing-supervisor"));
        assert_eq!(result.status, ProbeStatus::SpawnError);
        assert_eq!(result.returncode, None);
    }
    stop.store(true, Ordering::Relaxed);
    churn.join().unwrap();
}

fn malformed_bootstrap(binary: &Path, root: &Path) {
    for case in [
        "wrong-nonce",
        "linked-request",
        "wrong-request-type",
        "wrong-control-type",
    ] {
        let marker = root.join(format!("must-not-launch-{case}"));
        let capability = "0123456789abcdef0123456789abcdef";
        let wire = WireRequest {
            capability: if case == "wrong-nonce" {
                "fedcba9876543210fedcba9876543210"
            } else {
                capability
            }
            .into(),
            executable: b"/bin/sh".to_vec(),
            arguments: vec![
                b"-c".to_vec(),
                format!("echo launched > '{}'", marker.display()).into_bytes(),
            ],
            cwd: b"/".to_vec(),
            environment: Vec::new(),
            timeout_ns: 500_000_000,
        };
        let mut file = if case == "linked-request" {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(root.join("linked-private-request"))
                .unwrap()
        } else {
            tempfile::tempfile().unwrap()
        };
        file.write_all(&serde_json::to_vec(&wire).unwrap()).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let (_parent, control) = UnixStream::pair().unwrap();
        let request_fd = duplicate(if case == "wrong-request-type" {
            control.as_raw_fd()
        } else {
            file.as_raw_fd()
        })
        .unwrap();
        let control_fd = duplicate(if case == "wrong-control-type" {
            file.as_raw_fd()
        } else {
            control.as_raw_fd()
        })
        .unwrap();
        let rf = request_fd.as_raw_fd();
        let cf = control_fd.as_raw_fd();
        let mut command = Command::new(binary);
        command
            .args([MODE, capability])
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() < 0 || libc::dup2(rf, 3) < 0 || libc::dup2(cf, 4) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();
        let status = wait(&mut child, Duration::from_secs(2));
        assert_eq!(status.signal(), Some(libc::SIGKILL), "{case}: {status:?}");
        assert!(!marker.exists(), "{case}: native target launched");
    }
}
