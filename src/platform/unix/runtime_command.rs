//! No process groups, rollback, detached reader threads, or descendant cleanup.
//! The deadline bounds capture, including inherited pipes, not synchronous OS
//! spawn/filesystem/descriptor-scan setup or an uninterruptible kernel wait after
//! SIGKILL. The portable sweep assumes RLIMIT_NOFILE is not concurrently raised.
#[cfg(test)]
use super::runtime_child::sweep_cloexec;
use super::runtime_child::{child_cloexec, descriptor_bound};
use crate::runtime_command::{Output, Request, Status};
use std::{
    fs::File,
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::{
            fs::OpenOptionsExt,
            process::{CommandExt, ExitStatusExt},
        },
    },
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub(crate) fn open_transport_lock(root: &Path) -> crate::Result<File> {
    let directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(root)?;
    let state = c"state";
    if unsafe { libc::mkdirat(directory.as_raw_fd(), state.as_ptr(), 0o700) } < 0
        && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists
    {
        return Err(io::Error::last_os_error().into());
    }
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            state.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error().into());
    }
    let state = unsafe { OwnedFd::from_raw_fd(fd) };
    let fd = unsafe {
        libc::openat(
            state.as_raw_fd(),
            c".runtime-transport.lock".as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error().into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    crate::error::require(file.metadata()?.is_file(), "unsafe_transport_lock")?;
    Ok(file)
}

pub(crate) fn try_transport_lock(file: &File, exclusive: bool) -> io::Result<bool> {
    let mode = if exclusive {
        libc::LOCK_EX
    } else {
        libc::LOCK_SH
    };
    if unsafe { libc::flock(file.as_raw_fd(), mode | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if matches!(
        error.raw_os_error(),
        Some(libc::EAGAIN) | Some(libc::EACCES)
    ) {
        Ok(false)
    } else {
        Err(error)
    }
}

pub(crate) fn unlock_transport(file: &File) {
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
}

fn nonblocking(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

// A successful try_wait reaps on Unix. Never signal a reaped PID (it can be reused).
fn stop(child: &mut Child, reaped: bool) {
    if !reaped {
        let _ = child.kill();
        loop {
            match child.wait() {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                _ => break,
            }
        }
    }
}

pub(crate) fn runtime_command_run(request: &Request) -> Output {
    let mut output = Output {
        status: Status::SpawnError,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    // Auto-reaping would invalidate PID ownership before we could send a signal.
    // The embedding runtime must also not install a competing child waiter.
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) } != 0
        || action.sa_sigaction != libc::SIG_DFL
        || action.sa_flags & libc::SA_NOCLDWAIT != 0
    {
        return output;
    }
    // Require an explicit executable and working directory; no ambient PATH/env.
    if request.argv.is_empty()
        || !Path::new(&request.argv[0]).is_absolute()
        || !request.cwd.is_absolute()
        || request.argv.iter().any(|arg| arg.contains('\0'))
        || request
            .env
            .iter()
            .any(|(key, value)| key.is_empty() || key.contains(['=', '\0']) || value.contains('\0'))
    {
        return output;
    }
    let started = Instant::now();
    let bound = match descriptor_bound() {
        Ok(bound) => bound,
        Err(_) => return output,
    };
    let mut command = Command::new(&request.argv[0]);
    command
        .args(&request.argv[1..])
        .current_dir(&request.cwd)
        .env_clear()
        .envs(&request.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Only async-signal-safe syscalls and stack operations run after fork.
    unsafe {
        command.pre_exec(move || child_cloexec(bound));
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            // Rust's CLOEXEC exec-error pipe distinguishes failed exec/setup
            // from a successfully executed program returning any exit code.
            output.status = Status::ExecFailed;
            return output;
        }
    };
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let mut reaped = false;
    let mut exit = None;
    if stdout
        .as_ref()
        .is_none_or(|pipe| nonblocking(pipe.as_raw_fd()).is_err())
        || stderr
            .as_ref()
            .is_none_or(|pipe| nonblocking(pipe.as_raw_fd()).is_err())
    {
        stop(&mut child, reaped);
        return output;
    }
    'capture: loop {
        if !reaped {
            match child.try_wait() {
                Ok(Some(status)) => {
                    reaped = true;
                    exit = Some(Status::Exited(
                        status
                            .code()
                            .unwrap_or_else(|| -status.signal().unwrap_or(1)),
                    ));
                }
                Ok(None) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    // ECHILD means ownership has already been lost: never signal.
                    if error.raw_os_error() == Some(libc::ECHILD) {
                        reaped = true;
                    }
                    break;
                }
            }
        }
        if reaped && stdout.is_none() && stderr.is_none() {
            output.status = exit.unwrap_or(Status::SpawnError);
            break;
        }
        if started.elapsed() >= request.timeout {
            output.status = Status::Timeout;
            break;
        }
        // One bounded read per stream per turn prevents continuous stdout starving
        // stderr, child reaping, or deadline checks. One extra byte detects overflow.
        let mut read_pipe = |pipe: &mut dyn Read, bytes: &mut Vec<u8>, other_len: usize| {
            let remaining = request
                .max_output_bytes
                .saturating_sub(bytes.len() + other_len);
            let mut buffer = [0u8; 8192];
            let length = buffer.len().min(remaining.saturating_add(1));
            match pipe.read(&mut buffer[..length]) {
                Ok(0) => Ok(true),
                Ok(count) => {
                    bytes.extend_from_slice(&buffer[..count.min(remaining)]);
                    if count > remaining {
                        output.status = Status::OutputLimit;
                        Err(())
                    } else {
                        Ok(false)
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    Ok(false)
                }
                Err(_) => Err(()),
            }
        };
        if let Some(pipe) = stdout.as_mut() {
            match read_pipe(pipe, &mut output.stdout, output.stderr.len()) {
                Ok(true) => stdout = None,
                Ok(false) => {}
                Err(()) => break 'capture,
            }
        }
        if let Some(pipe) = stderr.as_mut() {
            match read_pipe(pipe, &mut output.stderr, output.stdout.len()) {
                Ok(true) => stderr = None,
                Ok(false) => {}
                Err(()) => break 'capture,
            }
        }
        let mut fds = [
            stdout.as_ref().map_or(-1, AsRawFd::as_raw_fd),
            stderr.as_ref().map_or(-1, AsRawFd::as_raw_fd),
        ]
        .map(|fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        });
        let remaining = request.timeout.saturating_sub(started.elapsed());
        let milliseconds = remaining.min(Duration::from_millis(10)).as_millis() as i32;
        if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, milliseconds) } < 0
            && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted
        {
            break;
        }
    }
    // Closing inherited pipes is intentional. Native daemon/server effects survive.
    drop(stdout);
    drop(stderr);
    stop(&mut child, reaped);
    output
}

#[cfg(test)]
#[path = "runtime_command/tests.rs"]
mod tests;
