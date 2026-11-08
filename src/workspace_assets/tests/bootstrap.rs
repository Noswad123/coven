use super::super::{agent_launch_command, shell_quote};
use crate::transport_gate::TransportGuard;
use std::{
    fs,
    os::unix::process::CommandExt,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

// Test-only containment: the shell's epilogue waits/reaps its delayed sender.
// On a panic/watchdog failure, kill its isolated group and reap our direct child.
struct Shell(Option<Child>);

impl Drop for Shell {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
        }
    }
}

fn wait_for(shell: &mut Shell, marker: &Path, deadline: Instant) {
    while !marker.exists() {
        assert!(
            Instant::now() < deadline,
            "bootstrap test watchdog: {}",
            marker.display()
        );
        assert!(
            shell.0.as_mut().unwrap().try_wait().unwrap().is_none(),
            "shell exited before {}",
            marker.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn argv(home: &Path, verb: &str) -> Vec<String> {
    fs::read(home.join(format!("{verb}.argv")))
        .unwrap()
        .strip_suffix(&[0])
        .unwrap()
        .split(|byte| *byte == 0)
        .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
        .collect()
}

#[test]
fn literal_delayed_bootstrap_keeps_legacy_bytes_and_outlives_transport_admission() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let fixture = bin.join("foreground");
    super::run(
        Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg(Path::new(file!()).with_file_name("bootstrap_fixture.rs"))
            .args(["--edition=2024", "-o"])
            .arg(&fixture)
            .stdin(Stdio::null()),
    );
    fs::copy(&fixture, bin.join("tmux")).unwrap();
    let herdr = bin.join("herdr peer's%");
    fs::copy(&fixture, &herdr).unwrap();

    for backend in ["tmux", "herdr"] {
        let home = temp.path().join(backend);
        fs::create_dir(&home).unwrap();
        let root = home.join("workspace a'b%\\literal");
        fs::create_dir_all(root.join("prompts")).unwrap();
        // No roster, provider, or backend server exists. Only a legacy-safe ID.
        let agent = "Legacy_SAFE-7";
        let starter = b"literal 'quotes' % \\ $HOME\nsecond line \t\n\n\n";
        fs::write(root.join(format!("prompts/{agent}.start.txt")), starter).unwrap();
        let target = if backend == "tmux" {
            "%731"
        } else {
            "native-pane_7"
        };
        fs::write(home.join("target"), target).unwrap();
        let launch = agent_launch_command(
            "Bootstrap Test",
            root.to_str().unwrap(),
            agent,
            &format!("{} foreground", shell_quote(fixture.to_str().unwrap())),
            backend,
        )
        .unwrap();
        assert!(launch.contains("(sleep 5;"));
        let stdout = fs::File::create(home.join("stdout")).unwrap();
        let stderr = fs::File::create(home.join("stderr")).unwrap();
        let shared = TransportGuard::shared(&root, Duration::ZERO).unwrap();
        let started = Instant::now();
        let deadline = started + Duration::from_secs(20);
        let mut shell = Shell(Some(
            Command::new("/bin/sh")
                // Execute the literal generator output; only the test epilogue adds
                // wait so the shell reaps its own delayed subshell before exiting.
                .args([
                    "-c",
                    &format!("{launch}\nstatus=$?\nwait\nexit \"$status\""),
                ])
                .current_dir(&root)
                .env_clear()
                .env("HOME", &home)
                .env("PATH", format!("{}:/bin", bin.display()))
                .env("HERDR_PANE_ID", target)
                .env("HERDR_BIN_PATH", &herdr)
                .stdin(Stdio::null())
                .stdout(stdout)
                .stderr(stderr)
                .process_group(0)
                .spawn()
                .unwrap(),
        ));
        wait_for(&mut shell, &home.join("foreground-started"), deadline);
        assert!(!home.join("delivered").exists());
        assert!(TransportGuard::exclusive(&root, Duration::ZERO).is_err());
        drop(shared);
        let exclusive = TransportGuard::exclusive(&root, Duration::ZERO).unwrap();
        fs::write(home.join("exclusive-held"), []).unwrap();
        // Deliberately hold actual exclusive admission through native delivery:
        // the literal legacy background sender does NOT reacquire this gate.
        wait_for(&mut shell, &home.join("done"), deadline);
        assert!(started.elapsed() >= Duration::from_secs(5));
        exclusive.validate_for(&root, true).unwrap();
        assert!(TransportGuard::shared(&root, Duration::ZERO).is_err());
        let delivered = fs::read(home.join("delivered")).unwrap();
        let events = fs::read_to_string(home.join("events")).unwrap();
        if backend == "tmux" {
            assert_eq!(delivered, starter);
            assert_eq!(
                events,
                "display-message\nforeground-started\nload-buffer\npaste-buffer\nsend-keys\n"
            );
            assert_eq!(
                argv(&home, "display-message"),
                ["display-message", "-p", "#{pane_id}"]
            );
            assert_eq!(
                argv(&home, "load-buffer"),
                [
                    "load-buffer",
                    "-b",
                    "coven-bootstrap-test-Legacy_SAFE-7-starter",
                    "prompts/Legacy_SAFE-7.start.txt"
                ]
            );
            assert_eq!(
                argv(&home, "paste-buffer"),
                [
                    "paste-buffer",
                    "-b",
                    "coven-bootstrap-test-Legacy_SAFE-7-starter",
                    "-t",
                    target
                ]
            );
            assert_eq!(
                argv(&home, "send-keys"),
                ["send-keys", "-t", target, "Enter"]
            );
        } else {
            let stripped = starter.strip_suffix(b"\n\n\n").unwrap();
            assert_eq!(delivered, stripped);
            assert_eq!(events, "foreground-started\nsend-text\nsend-keys\n");
            assert_eq!(
                argv(&home, "send-text"),
                [
                    "pane",
                    "send-text",
                    target,
                    std::str::from_utf8(stripped).unwrap()
                ]
            );
            assert_eq!(
                argv(&home, "send-keys"),
                ["pane", "send-keys", target, "enter"]
            );
        }
        fs::write(home.join("release-foreground"), []).unwrap();
        loop {
            assert!(
                Instant::now() < deadline,
                "shell did not reap delayed sender"
            );
            if let Some(status) = shell.0.as_mut().unwrap().try_wait().unwrap() {
                assert!(
                    status.success(),
                    "generated shell failed: {}",
                    fs::read_to_string(home.join("stderr")).unwrap()
                );
                shell.0.take().unwrap().wait().unwrap();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fs::read(home.join("stderr")).unwrap(), b"");
        assert!(
            fs::read_to_string(home.join("stdout"))
                .unwrap()
                .starts_with(&format!(
                    "Agent: {agent}\nPrompt: {}/prompts/{agent}.md\n",
                    root.display()
                ))
        );
        drop(exclusive);
    }
}
