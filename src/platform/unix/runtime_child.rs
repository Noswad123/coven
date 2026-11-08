//! Shared post-fork descriptor hygiene; never closes the exec-error handshake.
use std::{io, os::fd::RawFd};

pub(super) fn descriptor_bound() -> io::Result<RawFd> {
    let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let limit = unsafe { limit.assume_init() }.rlim_cur;
    if limit == libc::RLIM_INFINITY {
        return Err(io::ErrorKind::Unsupported.into());
    }
    let mut bound = limit.saturating_sub(1).min(RawFd::MAX as libc::rlim_t) as RawFd;
    // Existing FDs can exceed a subsequently-lowered soft limit. Enumerate only
    // in the parent; never allocate/read directories in the post-fork hook.
    #[cfg(target_os = "linux")]
    let directory = "/proc/self/fd";
    #[cfg(not(target_os = "linux"))]
    let directory = "/dev/fd";
    for entry in std::fs::read_dir(directory)? {
        let name = entry?.file_name();
        if let Some(fd) = name.to_str().and_then(|name| name.parse::<RawFd>().ok()) {
            bound = bound.max(fd);
        }
    }
    Ok(bound)
}

pub(super) fn child_cloexec(bound: RawFd) -> io::Result<()> {
    // CLOEXEC, not close: Command's private exec-error handshake must remain
    // writable until exec succeeds. No descriptor flags change in the parent.
    #[cfg(target_os = "linux")]
    {
        if unsafe {
            libc::syscall(
                libc::SYS_close_range,
                3u32,
                u32::MAX,
                libc::CLOSE_RANGE_CLOEXEC,
            )
        } == 0
        {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if !matches!(
            error.raw_os_error(),
            Some(libc::ENOSYS) | Some(libc::EINVAL)
        ) {
            return Err(error);
        }
    }
    sweep_cloexec(bound)
}

pub(super) fn sweep_cloexec(bound: RawFd) -> io::Result<()> {
    for fd in 3..=bound {
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0
            && io::Error::last_os_error().raw_os_error() != Some(libc::EBADF)
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
