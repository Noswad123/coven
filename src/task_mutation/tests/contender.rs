//! Independent OS-lock contender, not a second invocation of a production guard.
use std::{
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    path::Path,
    process::{Child, ChildStdout, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

pub(super) struct Process {
    pub(super) child: Child,
    output: ChildStdout,
}

impl Process {
    pub(super) fn spawn(entry: &str, root: &Path, mode: &str) -> Self {
        // libtest names omit the crate-name prefix from module_path!().
        let entry = entry.split_once("::").unwrap().1;
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", entry, "--ignored", "--nocapture"])
            .env("COVEN_SOURCE_CONTENDER_ROOT", root)
            .env("COVEN_SOURCE_CONTENDER_MODE", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .process_group(0)
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        Self { child, output }
    }

    pub(super) fn line(&mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut line = Vec::new();
        loop {
            assert!(Instant::now() < deadline, "contender readiness timed out");
            let mut poll = libc::pollfd {
                fd: self.output.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let result = unsafe { libc::poll(&mut poll, 1, 50) };
            assert!(result >= 0);
            if result == 0 {
                continue;
            }
            let mut byte = [0];
            assert_eq!(
                self.output.read(&mut byte).unwrap(),
                1,
                "contender closed IPC"
            );
            line.push(byte[0]);
            assert!(line.len() < 4096);
            if byte[0] == b'\n' {
                let text = String::from_utf8(std::mem::take(&mut line)).unwrap();
                // libtest may prefix the first output line with the test name.
                if let Some((_, message)) = text.split_once("CONTENDER:") {
                    return message.trim_end().to_owned();
                }
            }
        }
    }

    pub(super) fn send(&mut self, message: &[u8]) {
        self.child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(message)
            .unwrap();
    }

    pub(super) fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "contender exit timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if self
            .child
            .try_wait()
            .map_or(true, |status| status.is_none())
        {
            // Only the isolated group created above belongs to this fixture.
            unsafe {
                libc::kill(-(self.child.id() as libc::pid_t), libc::SIGKILL);
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub(super) fn open(path: &Path) -> File {
    let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    assert!(fd >= 0, "{}", std::io::Error::last_os_error());
    let file = unsafe { File::from_raw_fd(fd) };
    assert!(file.metadata().unwrap().is_file());
    file
}

pub(super) fn lock(file: &File, exclusive: bool, nonblocking: bool) -> bool {
    let operation = if exclusive {
        libc::LOCK_EX
    } else {
        libc::LOCK_SH
    } | if nonblocking { libc::LOCK_NB } else { 0 };
    if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
        return true;
    }
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EWOULDBLOCK)
    );
    false
}

pub(super) fn signal(message: &str) {
    println!("CONTENDER:{message}");
    std::io::stdout().flush().unwrap();
}

pub(super) fn receive() {
    let mut message = String::new();
    assert!(std::io::stdin().read_line(&mut message).unwrap() > 0);
    assert_eq!(message, "release\n");
}
