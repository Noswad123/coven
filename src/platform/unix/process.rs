//! Private same-binary supervisor. This bounds cooperating descendants, not hostile
//! programs that deliberately leave their session or attack same-user processes.
//! The drain deadline counts elapsed setup, but cannot interrupt synchronous
//! entropy/filesystem/FD-scan/self-exec setup. Cleanup has its own allowance. The
//! independent watchdog is armed after private bootstrap parsing, before native
//! launch; this is not a universal setup-inclusive wall-clock guarantee.
use crate::process::{ProbeRequest, ProbeResult, ProbeStatus, failed_result};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd},
        unix::{
            ffi::{OsStrExt, OsStringExt},
            fs::MetadataExt,
            net::UnixStream,
            process::{CommandExt, ExitStatusExt},
        },
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const MODE: &str = "--coven-private-probe-supervisor-v1";
const MAX_REQUEST: u64 = 1_048_576;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    capability: String,
    executable: Vec<u8>,
    arguments: Vec<Vec<u8>>,
    cwd: Vec<u8>,
    environment: Vec<(Vec<u8>, Vec<u8>)>,
    timeout_ns: u64,
}

fn invalid() -> io::Error {
    io::ErrorKind::InvalidData.into()
}

fn default_sigchld() -> bool {
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    (unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) == 0 })
        && action.sa_sigaction == libc::SIG_DFL
        && action.sa_flags & libc::SA_NOCLDWAIT == 0
}

fn nonblocking(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn open_fds() -> io::Result<Vec<RawFd>> {
    #[cfg(target_os = "linux")]
    let directory = "/proc/self/fd";
    #[cfg(not(target_os = "linux"))]
    let directory = "/dev/fd";
    std::fs::read_dir(directory)?
        .map(|entry| {
            entry?
                .file_name()
                .to_str()
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())
        })
        .collect()
}

fn duplicate(fd: RawFd) -> io::Result<OwnedFd> {
    let new = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
    if new < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(new) })
    }
}

fn output_pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [-1; 2];
    #[cfg(target_os = "linux")]
    let result = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    #[cfg(not(target_os = "linux"))]
    let result = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    // Adopt both ends before any fallible setup so every error closes them.
    let reader = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    let writer = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    #[cfg(not(target_os = "linux"))]
    // macOS has no pipe2: unlike Linux, CLOEXEC setup is not atomic with pipe.
    for fd in [reader.as_raw_fd(), writer.as_raw_fd()] {
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    // Low slots may be concurrently freed. Avoid ordered-dup2 aliasing and
    // dup2(fd, fd), which would leave CLOEXEC set on a low-slot pipe writer.
    // Keep both ends out of stdio and bootstrap slots.
    Ok((
        duplicate(reader.as_raw_fd())?,
        duplicate(writer.as_raw_fd())?,
    ))
}

fn close(fd: impl IntoRawFd) -> io::Result<()> {
    // Never retry close(EINTR): another thread may already have reused the fd.
    let result = if unsafe { libc::close(fd.into_raw_fd()) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    };
    #[cfg(test)]
    let result = tests::adversarial::cleanup_result("close", result);
    result
}

fn poll(fds: &mut [libc::pollfd], milliseconds: i32) -> io::Result<()> {
    let code = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, milliseconds) };
    if code < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn watch(fd: RawFd) -> libc::pollfd {
    libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }
}

pub(crate) fn process_run(request: &ProbeRequest, supervisor: &Path) -> ProbeResult {
    let start = Instant::now();
    let mut result = failed_result();
    if !default_sigchld()
        || !request.executable.is_absolute()
        || !request.cwd.is_absolute()
        || request.timeout.is_zero()
        || request.timeout > Duration::from_secs(30)
        || request.max_output_bytes > 1_048_576
    {
        return result;
    }
    let setup = || -> io::Result<_> {
        let raw_bytes = request
            .executable
            .as_os_str()
            .len()
            .checked_add(request.cwd.as_os_str().len())
            .and_then(|size| {
                request
                    .arguments
                    .iter()
                    .try_fold(size, |size, arg| size.checked_add(arg.len()))
            })
            .and_then(|size| {
                request
                    .environment
                    .iter()
                    .try_fold(size, |size, (key, value)| {
                        size.checked_add(key.len())?.checked_add(value.len())
                    })
            })
            .ok_or_else(invalid)?;
        if raw_bytes > MAX_REQUEST as usize / 5
            || request.arguments.len() > 8192
            || request.environment.len() > 1024
        {
            return Err(invalid());
        }
        let capability = super::random_uuid_hex().map_err(|_| invalid())?;
        let wire = WireRequest {
            capability: capability.clone(),
            executable: request.executable.as_os_str().as_bytes().to_vec(),
            arguments: request
                .arguments
                .iter()
                .map(|s| s.as_bytes().to_vec())
                .collect(),
            cwd: request.cwd.as_os_str().as_bytes().to_vec(),
            environment: request
                .environment
                .iter()
                .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
                .collect(),
            timeout_ns: request
                .timeout
                .as_nanos()
                .try_into()
                .map_err(|_| invalid())?,
        };
        let payload = serde_json::to_vec(&wire)?;
        if payload.len() as u64 > MAX_REQUEST {
            return Err(invalid());
        }
        let mut file = tempfile::tempfile()?;
        file.write_all(&payload)?;
        file.seek(SeekFrom::Start(0))?;
        let (parent, child) = UnixStream::pair()?;
        parent.set_nonblocking(true)?;
        let request_fd = duplicate(file.as_raw_fd())?;
        let control_fd = duplicate(child.as_raw_fd())?;
        let stdin = duplicate(File::open("/dev/null")?.as_raw_fd())?;
        let (stdout, child_stdout) = output_pipe()?;
        let (stderr, child_stderr) = output_pipe()?;
        let fds = open_fds()?;
        let mut command = Command::new(supervisor);
        command
            .arg(MODE)
            .arg(capability)
            .env_clear()
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(child_stdout))
            .stderr(Stdio::from(child_stderr));
        let rf = request_fd.as_raw_fd();
        let cf = control_fd.as_raw_fd();
        // Enumerated before fork; never close Command's subsequently-created exec
        // error pipe. Only async-signal-safe syscalls run in the child hook.
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                for &fd in &fds {
                    if fd > 2
                        && libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) < 0
                        && io::Error::last_os_error().raw_os_error() != Some(libc::EBADF)
                    {
                        return Err(io::Error::last_os_error());
                    }
                }
                if libc::dup2(rf, 3) < 0 || libc::dup2(cf, 4) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let process = command.spawn()?;
        Ok((process, parent, stdout, stderr))
    };
    let Ok((mut child, control, stdout, stderr)) = setup() else {
        result.duration_ms = start.elapsed().as_secs_f64() * 1000.0;
        return result;
    };
    let mut stdout = Some(stdout);
    let mut stderr = Some(stderr);
    let mut transport = || -> io::Result<ProbeStatus> {
        let out = stdout.as_ref().ok_or_else(invalid)?.as_raw_fd();
        let err = stderr.as_ref().ok_or_else(invalid)?.as_raw_fd();
        nonblocking(out)?;
        nonblocking(err)?;
        let mut fds = [watch(out), watch(err), watch(control.as_raw_fd())];
        let mut complete = Vec::new();
        loop {
            if fds[0].fd < 0 && fds[1].fd < 0 && complete.len() == 5 {
                return Ok(if result.returncode == Some(0) {
                    ProbeStatus::Ok
                } else {
                    ProbeStatus::Nonzero
                });
            }
            if start.elapsed() >= request.timeout {
                return Ok(ProbeStatus::Timeout);
            }
            poll(&mut fds, 10)?;
            for index in 0..3 {
                if fds[index].fd < 0 || fds[index].revents == 0 {
                    continue;
                }
                let mut buffer = [0_u8; 65536];
                let available =
                    request.max_output_bytes - result.stdout.len() - result.stderr.len();
                let limit = if index == 2 {
                    6 - complete.len()
                } else {
                    buffer.len().min(available + 1)
                };
                let count = unsafe { libc::read(fds[index].fd, buffer.as_mut_ptr().cast(), limit) };
                if count < 0 {
                    let error = io::Error::last_os_error();
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) {
                        continue;
                    }
                    return Err(error);
                }
                if count == 0 {
                    if index == 2 {
                        return Err(invalid());
                    }
                    fds[index].fd = -1;
                    continue;
                }
                let count = count as usize;
                if index == 2 {
                    complete.extend_from_slice(&buffer[..count]);
                    if complete.len() > 5 {
                        return Err(invalid());
                    }
                    if complete.len() == 5 {
                        if complete[0] != 1 {
                            return Err(invalid());
                        }
                        result.returncode = Some(i32::from_le_bytes(
                            complete[1..5].try_into().map_err(|_| invalid())?,
                        ));
                        fds[2].fd = -1;
                    }
                } else {
                    let target = if index == 0 {
                        &mut result.stdout
                    } else {
                        &mut result.stderr
                    };
                    target.extend_from_slice(&buffer[..count.min(available)]);
                    if count > available {
                        return Ok(ProbeStatus::OutputLimit);
                    }
                }
            }
        }
    };
    result.status = transport().unwrap_or(ProbeStatus::SpawnError);

    // No try_wait/wait has occurred: this default-SIGCHLD child still owns its
    // unreaped identity even if it exited. Never signal this PGID after reaping.
    let killed = unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
    let signaled = if killed < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    };
    #[cfg(test)]
    let signaled = tests::adversarial::cleanup_result("signal", signaled);
    if signaled.is_err() {
        result.status = ProbeStatus::SpawnError;
    }
    // EOF independently asks the supervisor to self-kill its cooperating group.
    let shutdown = control.shutdown(std::net::Shutdown::Both);
    #[cfg(test)]
    let shutdown = tests::adversarial::cleanup_result("shutdown", shutdown);
    if shutdown.is_err() | close(control).is_err() {
        result.status = ProbeStatus::SpawnError;
    }
    if stdout.take().is_some_and(|pipe| close(pipe).is_err())
        | stderr.take().is_some_and(|pipe| close(pipe).is_err())
    {
        result.status = ProbeStatus::SpawnError;
    }
    if killed < 0 && child.kill().is_err() {
        result.status = ProbeStatus::SpawnError;
    }
    let cleanup = Instant::now();
    loop {
        let waited = child.try_wait();
        #[cfg(test)]
        let waited = if matches!(waited, Ok(Some(_))) {
            tests::adversarial::cleanup_result("reap", waited)
        } else {
            waited
        };
        match waited {
            Ok(Some(status)) => {
                if status.signal() != Some(libc::SIGKILL) {
                    result.status = ProbeStatus::SpawnError;
                }
                break;
            }
            Ok(None) if cleanup.elapsed() < Duration::from_secs(1) => {
                std::thread::sleep(Duration::from_millis(5))
            }
            _ => {
                result.status = ProbeStatus::SpawnError;
                break;
            }
        }
    }
    result.duration_ms = start.elapsed().as_secs_f64() * 1000.0;
    result
}

fn stop_group() -> ! {
    unsafe {
        libc::kill(-libc::getpgrp(), libc::SIGKILL);
        libc::_exit(125);
    }
}

fn parent_present(control: &mut UnixStream) -> bool {
    let mut byte = [0_u8];
    loop {
        match control.read(&mut byte) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return true,
            _ => return false,
        }
    }
}

fn supervise(capability: &str) -> io::Result<()> {
    if !default_sigchld() || unsafe { libc::getsid(0) != libc::getpid() } {
        return Err(invalid());
    }
    // Only accept the exact inherited regular, anonymous request and socket.
    let mut request_stat = unsafe { std::mem::zeroed::<libc::stat>() };
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    if unsafe { libc::fstat(3, &mut request_stat) } != 0
        || request_stat.st_mode & libc::S_IFMT != libc::S_IFREG
        || unsafe { libc::fstat(4, &mut stat) } != 0
        || stat.st_mode & libc::S_IFMT != libc::S_IFSOCK
    {
        return Err(invalid());
    }
    let mut request = unsafe { File::from_raw_fd(3) };
    let metadata = request.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 0 || metadata.len() > MAX_REQUEST {
        return Err(invalid());
    }
    let mut control = unsafe { UnixStream::from_raw_fd(4) };
    control.set_nonblocking(true)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut request)
        .take(MAX_REQUEST + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_REQUEST {
        return Err(invalid());
    }
    let wire: WireRequest = serde_json::from_slice(&bytes)?;
    if wire.capability != capability
        || capability.len() != 32
        || !capability.bytes().all(|c| c.is_ascii_hexdigit())
        || wire.timeout_ns == 0
        || wire.timeout_ns > 30_000_000_000
    {
        return Err(invalid());
    }
    drop(request);
    for fd in open_fds()? {
        if fd > 2
            && unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0
            && io::Error::last_os_error().raw_os_error() != Some(libc::EBADF)
        {
            return Err(io::Error::last_os_error());
        }
    }
    let budget = Duration::from_nanos(wire.timeout_ns) + Duration::from_secs(1);
    let deadline = Instant::now() + budget;
    let mut liveness = control.try_clone()?;
    // Independent of the caller (including a SIGSTOPed caller) and armed before
    // native spawn. No joins: group SIGKILL terminates this thread as well.
    std::thread::Builder::new()
        .name("probe-watchdog".into())
        .spawn(move || {
            loop {
                if Instant::now() >= deadline || !parent_present(&mut liveness) {
                    stop_group();
                }
                if poll(&mut [watch(liveness.as_raw_fd())], 10).is_err() {
                    stop_group();
                }
            }
        })?;
    if !parent_present(&mut control) {
        stop_group();
    }
    let executable = PathBuf::from(OsString::from_vec(wire.executable));
    let cwd = PathBuf::from(OsString::from_vec(wire.cwd));
    if !executable.is_absolute() || !cwd.is_absolute() {
        return Err(invalid());
    }
    let mut command = Command::new(executable);
    command
        .args(wire.arguments.into_iter().map(OsString::from_vec))
        .current_dir(cwd)
        .env_clear()
        .envs(
            wire.environment
                .into_iter()
                .map(|(k, v)| (OsString::from_vec(k), OsString::from_vec(v))),
        )
        .stdin(Stdio::null());
    let mut child = command.spawn().ok();
    if (unsafe { libc::close(1) } < 0) | (unsafe { libc::close(2) } < 0) {
        return Err(io::Error::last_os_error());
    }
    let mut sent = false;
    loop {
        if !parent_present(&mut control) {
            stop_group();
        }
        if !sent {
            let completion = match child.as_mut() {
                Some(child) => child.try_wait()?.map(|status| {
                    (
                        true,
                        status
                            .code()
                            .unwrap_or_else(|| -status.signal().unwrap_or(0)),
                    )
                }),
                None => Some((false, 0)),
            };
            if let Some((spawned, code)) = completion {
                let mut frame = [0_u8; 5];
                frame[0] = u8::from(spawned);
                frame[1..].copy_from_slice(&code.to_le_bytes());
                control.write_all(&frame)?;
                sent = true;
            }
        }
        poll(&mut [watch(control.as_raw_fd())], 10)?;
    }
}

pub(crate) fn supervisor_entry() -> Option<u8> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(MODE)) {
        return None;
    }
    let Some(capability) = args.next().and_then(|s| s.into_string().ok()) else {
        return Some(125);
    };
    if args.next().is_some() {
        return Some(125);
    }
    // Errors after a valid session boundary must also clean any native children.
    if supervise(&capability).is_err() && unsafe { libc::getsid(0) == libc::getpid() } {
        stop_group();
    }
    Some(125)
}

#[cfg(test)]
mod tests;
