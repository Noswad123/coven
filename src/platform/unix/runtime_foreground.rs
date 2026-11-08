//! Own and reap only the direct CLI child. No deadlines or server cleanup.
use super::runtime_child::{child_cloexec, descriptor_bound};
use crate::{runtime_command::Status, runtime_foreground::Request};
use std::{
    io,
    os::unix::process::{CommandExt, ExitStatusExt},
    path::Path,
    process::{Command, Stdio},
};

pub(crate) fn runtime_terminal_ttys() -> (bool, bool) {
    unsafe {
        (
            libc::isatty(libc::STDIN_FILENO) == 1,
            libc::isatty(libc::STDOUT_FILENO) == 1,
        )
    }
}

pub(crate) fn runtime_foreground_run(request: &Request) -> Status {
    // The embedding runtime must not install a competing child reaper either.
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) } != 0
        || action.sa_sigaction != libc::SIG_DFL
        || action.sa_flags & libc::SA_NOCLDWAIT != 0
        || request.argv.is_empty()
        || !Path::new(&request.argv[0]).is_absolute()
        || !request.cwd.is_absolute()
        || request.argv.iter().any(|arg| arg.contains('\0'))
        || request
            .env
            .iter()
            .any(|(key, value)| key.is_empty() || key.contains(['=', '\0']) || value.contains('\0'))
    {
        return Status::SpawnError;
    }
    let bound = match descriptor_bound() {
        Ok(bound) => bound,
        Err(_) => return Status::SpawnError,
    };
    let mut command = Command::new(&request.argv[0]);
    command
        .args(&request.argv[1..])
        .current_dir(&request.cwd)
        .env_clear()
        .envs(&request.env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // Only async-signal-safe syscalls and stack operations after fork.
    unsafe {
        command.pre_exec(move || child_cloexec(bound));
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        // Rust's private CLOEXEC pipe proves the selected image never ran.
        Err(_) => return Status::ExecFailed,
    };
    loop {
        match child.wait() {
            Ok(status) => {
                return status
                    .code()
                    .or_else(|| status.signal().map(|signal| -signal))
                    .map_or(Status::SpawnError, Status::Exited);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            // Unknown ownership is not permission to signal, clean up, or fallback.
            Err(_) => return Status::SpawnError,
        }
    }
}

#[cfg(test)]
#[path = "runtime_foreground/tests.rs"]
mod tests;
