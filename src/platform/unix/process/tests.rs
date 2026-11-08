use super::*;
use crate::process::run_with_supervisor;
use std::collections::BTreeMap;

pub(super) mod adversarial;

// Integration test process_supervision supplies Cargo's actual application path.
// Kept in this module so assertions exercise the private production runner API.
#[test]
fn real_binary_supervision() {
    let Some(binary) = std::env::var_os("COVEN_TEST_SUPERVISOR") else {
        return;
    };
    let binary = PathBuf::from(binary);
    let root = tempfile::tempdir().unwrap();
    let request = |script: &str, limit: usize, timeout: u64| ProbeRequest {
        executable: "/bin/sh".into(),
        arguments: vec!["-c".into(), script.into()],
        cwd: root.path().into(),
        environment: BTreeMap::new(),
        timeout: Duration::from_millis(timeout),
        max_output_bytes: limit,
    };
    for (script, limit, expected, code, output) in [
        (
            "printf abcd",
            4,
            ProbeStatus::Ok,
            Some(0),
            b"abcd".as_slice(),
        ),
        (
            "printf abcde",
            4,
            ProbeStatus::OutputLimit,
            None,
            b"abcd".as_slice(),
        ),
        (
            "printf ab; printf cde >&2",
            4,
            ProbeStatus::OutputLimit,
            None,
            b"ab".as_slice(),
        ),
        (
            "printf abcde >&2",
            4,
            ProbeStatus::OutputLimit,
            None,
            b"".as_slice(),
        ),
        (
            "printf ab; printf cd >&2",
            4,
            ProbeStatus::Ok,
            Some(0),
            b"ab".as_slice(),
        ),
        (
            "exit 7",
            1024,
            ProbeStatus::Nonzero,
            Some(7),
            b"".as_slice(),
        ),
        (
            "kill -TERM $$",
            1024,
            ProbeStatus::Nonzero,
            Some(-15),
            b"".as_slice(),
        ),
        (
            "exec 1>&- 2>&-; sleep 5",
            1024,
            ProbeStatus::Timeout,
            None,
            b"".as_slice(),
        ),
        (
            "sleep 5 & exit 0",
            1024,
            ProbeStatus::Timeout,
            Some(0),
            b"".as_slice(),
        ),
        (
            "while :; do printf 123456789; done",
            1024,
            ProbeStatus::OutputLimit,
            None,
            b"".as_slice(),
        ),
    ] {
        let result = run_with_supervisor(&request(script, limit, 250), &binary);
        assert_eq!(result.status, expected, "{script}: {result:?}");
        if let Some(code) = code {
            assert_eq!(result.returncode, Some(code));
        }
        if expected != ProbeStatus::OutputLimit || limit == 4 {
            assert_eq!(result.stdout, output);
        }
        assert!(result.stdout.len() + result.stderr.len() <= limit);
        if expected == ProbeStatus::OutputLimit {
            assert_eq!(result.stdout.len() + result.stderr.len(), limit);
        }
        assert!(result.duration_ms < 2000.0, "{result:?}");
    }
    let mut bad = request("", 1024, 250);
    bad.executable = root.path().join("missing");
    assert_eq!(
        run_with_supervisor(&bad, &binary).status,
        ProbeStatus::SpawnError
    );
    assert_eq!(
        run_with_supervisor(&request("exit 0", 1024, 250), Path::new("/bin/true")).status,
        ProbeStatus::SpawnError,
        "an application without the bootstrap must fail closed"
    );

    #[cfg(target_os = "linux")]
    for count in [1_048_576, 1_048_577] {
        let mut exact = request("", 1_048_576, 2000);
        exact.executable = "/usr/bin/head".into();
        exact.arguments = vec!["-c".into(), count.to_string().into(), "/dev/zero".into()];
        let result = run_with_supervisor(&exact, &binary);
        assert_eq!(
            result.status,
            if count == 1_048_576 {
                ProbeStatus::Ok
            } else {
                ProbeStatus::OutputLimit
            }
        );
        assert_eq!(result.stdout.len(), 1_048_576);
        assert!(result.stderr.is_empty());
    }

    let mut environment = request("", 1024, 500);
    environment.executable = "/usr/bin/env".into();
    environment.arguments.clear();
    environment
        .environment
        .insert("EXACT".into(), OsString::from_vec(b"byte-\xff".to_vec()));
    let result = run_with_supervisor(&environment, &binary);
    assert_eq!(result.status, ProbeStatus::Ok);
    assert_eq!(result.stdout, b"EXACT=byte-\xff\n");
    let mut argv = request("", 1024, 500);
    argv.executable = "/usr/bin/printf".into();
    argv.arguments = vec!["%s".into(), OsString::from_vec(b"arg-\xff".to_vec())];
    assert_eq!(run_with_supervisor(&argv, &binary).stdout, b"arg-\xff");
    let mut cwd = request("", 1024, 500);
    cwd.executable = "/bin/pwd".into();
    cwd.arguments.clear();
    assert_eq!(
        run_with_supervisor(&cwd, &binary).stdout,
        [root.path().as_os_str().as_bytes(), b"\n"].concat()
    );
    let byte_path = root.path().join(OsString::from_vec(b"cwd-\xff".to_vec()));
    std::fs::create_dir(&byte_path).unwrap();
    cwd.cwd = byte_path.clone();
    assert_eq!(
        run_with_supervisor(&cwd, &binary).stdout,
        [byte_path.as_os_str().as_bytes(), b"\n"].concat()
    );
    let byte_executable = root
        .path()
        .join(OsString::from_vec(b"executable-\xff".to_vec()));
    std::fs::copy("/bin/true", &byte_executable).unwrap();
    cwd.executable = byte_executable;
    assert_eq!(run_with_supervisor(&cwd, &binary).status, ProbeStatus::Ok);

    let descendants = root.path().join("descendant");
    let script = format!(
        "sleep 15 >/dev/null 2>&1 & echo $! > '{}'; exit 0",
        descendants.display()
    );
    let result = run_with_supervisor(&request(&script, 1024, 500), &binary);
    assert_eq!(result.status, ProbeStatus::Ok, "{result:?}");
    let pid: i32 = std::fs::read_to_string(descendants)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_terminated(pid);

    for (tail, expected) in [
        ("exit 7", ProbeStatus::Nonzero),
        (
            "while :; do printf 123456789; done",
            ProbeStatus::OutputLimit,
        ),
        ("exec 1>&- 2>&-; sleep 15", ProbeStatus::Timeout),
    ] {
        let descendants = root.path().join("cleanup-descendant");
        let script = format!(
            "sleep 15 >/dev/null 2>&1 & echo $! > '{}'; {tail}",
            descendants.display()
        );
        let result = run_with_supervisor(&request(&script, 1024, 250), &binary);
        assert_eq!(result.status, expected, "{script}: {result:?}");
        let pid = std::fs::read_to_string(descendants)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_terminated(pid);
    }

    // No private control/request/workspace descriptor may reach the native child.
    let marker = root.path().join("descriptor-marker");
    let file = File::create(&marker).unwrap();
    unsafe {
        libc::fcntl(file.as_raw_fd(), libc::F_SETFD, 0);
    }
    #[cfg(target_os = "linux")]
    {
        let script = "for f in /proc/self/fd/*; do readlink \"$f\"; done";
        let result = run_with_supervisor(&request(script, 65536, 1000), &binary);
        assert!(
            matches!(result.status, ProbeStatus::Ok | ProbeStatus::Nonzero),
            "{result:?}"
        );
        assert!(!String::from_utf8_lossy(&result.stdout).contains("descriptor-marker"));
        assert!(!String::from_utf8_lossy(&result.stdout).contains("deleted"));
        assert!(!String::from_utf8_lossy(&result.stdout).contains("socket:"));
    }

    for mode in ["ignore", "no_cld_wait", "handler"] {
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "platform::unix::process::tests::isolated_caller",
                "--exact",
                "--nocapture",
            ])
            .env("COVEN_PROCESS_CASE", mode)
            .status()
            .unwrap();
        assert!(status.success(), "SIGCHLD case {mode}");
    }

    for mode in [
        "caller_death",
        "stopped_caller",
        "caller_death_completed",
        "stopped_caller_completed",
    ] {
        let pid_path = root.path().join(mode);
        let mut caller = Command::new(std::env::current_exe().unwrap())
            .args([
                "platform::unix::process::tests::isolated_caller",
                "--exact",
                "--nocapture",
            ])
            .env("COVEN_PROCESS_CASE", mode)
            .env("COVEN_PROCESS_PID_FILE", &pid_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let pid: i32 = loop {
            if let Ok(text) = std::fs::read_to_string(&pid_path) {
                if let Ok(pid) = text.trim().parse() {
                    break pid;
                }
            }
            assert!(
                Instant::now() < deadline,
                "native fixture did not launch: {mode}"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        let signal = if mode.starts_with("caller_death") {
            libc::SIGKILL
        } else {
            libc::SIGSTOP
        };
        assert_eq!(unsafe { libc::kill(caller.id() as i32, signal) }, 0);
        assert_terminated_within(pid, Duration::from_secs(2));
        // A stopped caller cannot run its own cleanup: only the supervisor's
        // independent watchdog can have terminated the native process above.
        if mode.starts_with("stopped_caller") {
            caller.kill().unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if caller.try_wait().unwrap().is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "caller did not reap");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    let mut unrelated = Command::new("/bin/sleep").arg("10").spawn().unwrap();
    for _ in 0..10 {
        assert_eq!(
            run_with_supervisor(&request("exit 0", 1024, 500), &binary).status,
            ProbeStatus::Ok
        );
        assert!(
            unrelated.try_wait().unwrap().is_none(),
            "cleanup signaled unrelated identity"
        );
    }
    unrelated.kill().unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while unrelated.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }

    lost_parent_before_launch(&binary, root.path());
    adversarial::run(&binary, root.path());
}

fn assert_terminated(pid: i32) {
    assert_terminated_within(pid, Duration::from_secs(1));
}

fn assert_terminated_within(pid: i32, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if unsafe { libc::kill(pid, 0) } < 0 {
            return;
        }
        #[cfg(target_os = "linux")]
        if std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
            s.rsplit_once(')')
                .is_some_and(|(_, rest)| rest.starts_with(" Z"))
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "descendant {pid} survived cleanup"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn isolated_caller() {
    let Ok(mode) = std::env::var("COVEN_PROCESS_CASE") else {
        return;
    };
    let binary = PathBuf::from(std::env::var_os("COVEN_TEST_SUPERVISOR").unwrap());
    let mut request = ProbeRequest {
        executable: "/bin/sh".into(),
        arguments: vec!["-c".into(), "exit 0".into()],
        cwd: "/".into(),
        environment: BTreeMap::new(),
        timeout: Duration::from_millis(400),
        max_output_bytes: 1024,
    };
    if mode.starts_with("caller_death") || mode.starts_with("stopped_caller") {
        let path = std::env::var("COVEN_PROCESS_PID_FILE").unwrap();
        request.arguments[1] = if mode.ends_with("completed") {
            format!("sleep 15 & echo $! > '{}'; exit 0", path).into()
        } else {
            format!("echo $$ > '{}'; exec sleep 15", path).into()
        };
        let result = run_with_supervisor(&request, &binary);
        assert_eq!(result.status, ProbeStatus::Timeout);
    } else {
        extern "C" fn handler(_: i32) {}
        let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
        match mode.as_str() {
            "ignore" => action.sa_sigaction = libc::SIG_IGN,
            "no_cld_wait" => action.sa_flags = libc::SA_NOCLDWAIT,
            "handler" => action.sa_sigaction = handler as *const () as usize,
            _ => panic!("unknown isolated case"),
        }
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) },
            0
        );
        assert_eq!(
            run_with_supervisor(&request, &binary).status,
            ProbeStatus::SpawnError
        );
    }
}

fn lost_parent_before_launch(binary: &Path, root: &Path) {
    let marker = root.join("must-not-launch");
    let capability = "0123456789abcdef0123456789abcdef";
    let wire = WireRequest {
        capability: capability.into(),
        executable: b"/bin/sh".to_vec(),
        arguments: vec![
            b"-c".to_vec(),
            format!("echo launched > '{}'", marker.display()).into_bytes(),
        ],
        cwd: b"/".to_vec(),
        environment: Vec::new(),
        timeout_ns: 500_000_000,
    };
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&serde_json::to_vec(&wire).unwrap()).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    let (parent, control) = UnixStream::pair().unwrap();
    drop(parent);
    let request_fd = duplicate(file.as_raw_fd()).unwrap();
    let control_fd = duplicate(control.as_raw_fd()).unwrap();
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
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.signal(), Some(libc::SIGKILL));
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("lost-parent supervisor failed to exit");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        !marker.exists(),
        "native target launched after parent was already lost"
    );
}
