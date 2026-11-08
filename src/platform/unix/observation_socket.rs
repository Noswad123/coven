//! Nonblocking Unix socket operations; no timeout restart, endpoint fallback, or retry.
use crate::observation_socket::{Cancel, Endpoint, Failure, ReadBudget, WireResult, check};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::time::Instant;

#[path = "observation_socket/interrupt.rs"]
mod interrupt;
#[path = "observation_socket/lease.rs"]
mod lease;
pub(crate) use interrupt::InterruptGuard;
pub(crate) use lease::PublisherLease;

#[cfg(test)]
thread_local! {
    static SOCKET_CONSTRUCTIONS: std::cell::Cell<Option<(usize, bool)>> = const {
        std::cell::Cell::new(None)
    };
}

/// Caller-thread probe only; fixture threads and unscoped callers are unaffected.
#[cfg(test)]
pub(crate) struct SocketConstructionGuard(std::marker::PhantomData<std::rc::Rc<()>>);

#[cfg(test)]
impl SocketConstructionGuard {
    pub(crate) fn new(poison: bool) -> Self {
        SOCKET_CONSTRUCTIONS.with(|state| {
            assert!(state.get().is_none(), "nested socket construction probe");
            state.set(Some((0, poison)));
        });
        Self(std::marker::PhantomData)
    }

    pub(crate) fn count(&self) -> usize {
        SOCKET_CONSTRUCTIONS.with(|state| state.get().unwrap().0)
    }
}

#[cfg(test)]
impl Drop for SocketConstructionGuard {
    fn drop(&mut self) {
        SOCKET_CONSTRUCTIONS.with(|state| state.set(None));
    }
}

fn io_failure(error: io::Error) -> Failure {
    Failure::new(if error.kind() == io::ErrorKind::TimedOut {
        "timeout"
    } else {
        "io_error"
    })
}

pub(crate) fn resolve(path: &str) -> WireResult<String> {
    use std::collections::{HashMap, VecDeque};
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::PathBuf;

    // Split lexically: Path::file_name normalizes a final '.', but the selected
    // leaf must retain its exact filesystem meaning (only parents are resolved).
    let (parent, leaf) = path
        .rsplit_once('/')
        .ok_or_else(|| Failure::new("invalid_socket_path"))?;
    enum Step {
        Part(OsString),
        Finish(PathBuf),
    }
    let mut pending: VecDeque<_> = parent
        .split('/')
        .map(|part| Step::Part(part.into()))
        .collect();
    let mut resolved = PathBuf::from("/");
    let mut aliases: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();
    // Match non-strict realpath: resolve existing (including dangling) aliases,
    // normalize dots/slashes, and retain missing or inaccessible parent tails.
    // Finish markers cache aliases and break cycles without recursive stack growth.
    while let Some(step) = pending.pop_front() {
        let part = match step {
            Step::Finish(alias) => {
                aliases.insert(alias, Some(resolved.clone()));
                continue;
            }
            Step::Part(part) => part,
        };
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            resolved.pop();
            continue;
        }
        resolved.push(part);
        if !std::fs::symlink_metadata(&resolved).is_ok_and(|info| info.file_type().is_symlink()) {
            continue;
        }
        if let Some(target) = aliases.get(&resolved) {
            if let Some(target) = target {
                resolved = target.clone();
            }
            continue;
        }
        let Ok(target) = std::fs::read_link(&resolved) else {
            continue;
        };
        let alias = resolved.clone();
        aliases.insert(alias.clone(), None);
        resolved.pop();
        if target.is_absolute() {
            resolved = PathBuf::from("/");
        }
        pending.push_front(Step::Finish(alias));
        for part in target
            .as_os_str()
            .as_bytes()
            .split(|&byte| byte == b'/')
            .rev()
        {
            pending.push_front(Step::Part(OsString::from_vec(part.to_vec())));
        }
    }
    resolved
        .join(leaf)
        .into_os_string()
        .into_string()
        .map_err(|_| Failure::new("invalid_socket_path"))
}

pub(crate) fn select(path: &str) -> WireResult<Endpoint> {
    let resolved = resolve(path)?;
    let info = std::fs::symlink_metadata(&resolved).map_err(io_failure)?;
    if !info.file_type().is_socket()
        || info.uid() != unsafe { libc::getuid() }
        || info.mode() & 0o077 != 0
    {
        return Err(Failure::new("unsafe_endpoint"));
    }
    Ok(Endpoint {
        selected: path.into(),
        resolved,
        device: info.dev(),
        inode: info.ino(),
    })
}

pub(crate) fn fence(endpoint: &Endpoint) -> WireResult<()> {
    let current = select(&endpoint.selected)?;
    if current != *endpoint {
        return Err(Failure::new("endpoint_replaced"));
    }
    Ok(())
}

pub(crate) struct Socket(OwnedFd);

impl Socket {
    pub(crate) fn connect(
        endpoint: &Endpoint,
        deadline: Instant,
        cancel: &dyn Cancel,
    ) -> WireResult<Self> {
        check(deadline, cancel)?;
        fence(endpoint)?;
        #[cfg(test)]
        SOCKET_CONSTRUCTIONS.with(|state| {
            if let Some((count, poison)) = state.get() {
                state.set(Some((count + 1, poison)));
                assert!(!poison, "native socket construction poisoned");
            }
        });
        let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
        if raw < 0 {
            return Err(io_failure(io::Error::last_os_error()));
        }
        let socket = Self(unsafe { OwnedFd::from_raw_fd(raw) });
        // Set these before connect: a blocking connect cannot obey an overall deadline.
        if unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } < 0
            || unsafe { libc::fcntl(raw, libc::F_SETFL, libc::O_NONBLOCK) } < 0
        {
            return Err(io_failure(io::Error::last_os_error()));
        }
        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
        {
            let yes: libc::c_int = 1;
            if unsafe {
                libc::setsockopt(
                    raw,
                    libc::SOL_SOCKET,
                    libc::SO_NOSIGPIPE,
                    (&yes as *const libc::c_int).cast(),
                    std::mem::size_of_val(&yes) as libc::socklen_t,
                )
            } < 0
            {
                return Err(io_failure(io::Error::last_os_error()));
            }
        }
        let address = if endpoint.resolved.len() <= endpoint.selected.len() {
            &endpoint.resolved
        } else {
            &endpoint.selected
        };
        let mut sockaddr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        if address.len() >= sockaddr.sun_path.len() {
            return Err(Failure::new("io_error"));
        }
        sockaddr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (dest, source) in sockaddr.sun_path.iter_mut().zip(address.bytes()) {
            *dest = source as libc::c_char;
        }
        let length = (std::mem::offset_of!(libc::sockaddr_un, sun_path) + address.len() + 1)
            as libc::socklen_t;
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd",
            target_os = "dragonfly"
        ))]
        {
            sockaddr.sun_len = length as u8;
        }
        check(deadline, cancel)?;
        let connected =
            unsafe { libc::connect(raw, (&sockaddr as *const libc::sockaddr_un).cast(), length) };
        if connected < 0 {
            let error = io::Error::last_os_error();
            // EAGAIN (including a full AF_UNIX listen queue) is not a pending connect.
            if !matches!(error.raw_os_error(), Some(libc::EINPROGRESS | libc::EINTR)) {
                return Err(io_failure(error));
            }
            socket.wait(libc::POLLOUT, deadline, cancel)?;
            let mut status: libc::c_int = 0;
            let mut size = std::mem::size_of_val(&status) as libc::socklen_t;
            if unsafe {
                libc::getsockopt(
                    raw,
                    libc::SOL_SOCKET,
                    libc::SO_ERROR,
                    (&mut status as *mut libc::c_int).cast(),
                    &mut size,
                )
            } < 0
            {
                return Err(io_failure(io::Error::last_os_error()));
            }
            if status != 0 {
                return Err(io_failure(io::Error::from_raw_os_error(status)));
            }
        }
        check(deadline, cancel)?;
        fence(endpoint)?;
        Ok(socket)
    }

    fn wait(
        &self,
        events: libc::c_short,
        deadline: Instant,
        cancel: &dyn Cancel,
    ) -> WireResult<()> {
        loop {
            check(deadline, cancel)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            // A short bounded poll also notices cancellation without requiring a signal fd.
            let timeout = remaining.as_millis().saturating_add(1).min(20) as libc::c_int;
            let mut pollfd = libc::pollfd {
                fd: self.0.as_raw_fd(),
                events,
                revents: 0,
            };
            let result = unsafe { libc::poll(&mut pollfd, 1, timeout) };
            let error = (result < 0).then(io::Error::last_os_error);
            check(deadline, cancel)?;
            if result > 0 {
                if pollfd.revents & libc::POLLNVAL != 0 {
                    return Err(Failure::new("io_error"));
                }
                return Ok(());
            }
            match error {
                Some(error) if error.kind() != io::ErrorKind::Interrupted => {
                    return Err(io_failure(error));
                }
                _ => {}
            }
        }
    }

    pub(crate) fn send(
        &self,
        bytes: &[u8],
        deadline: Instant,
        cancel: &dyn Cancel,
    ) -> WireResult<()> {
        let mut sent = 0;
        while sent < bytes.len() {
            check(deadline, cancel)?;
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            let flags = 0;
            #[cfg(not(any(target_os = "macos", target_os = "ios")))]
            let flags = libc::MSG_NOSIGNAL;
            let count = unsafe {
                libc::send(
                    self.0.as_raw_fd(),
                    bytes[sent..].as_ptr().cast(),
                    bytes.len() - sent,
                    flags,
                )
            };
            if count > 0 {
                sent += count as usize;
            } else if count == 0 {
                return Err(Failure::new("io_error"));
            } else {
                let error = io::Error::last_os_error();
                match error.kind() {
                    io::ErrorKind::Interrupted => continue,
                    io::ErrorKind::WouldBlock => self.wait(libc::POLLOUT, deadline, cancel)?,
                    _ => return Err(io_failure(error)),
                }
            }
        }
        check(deadline, cancel)
    }

    pub(crate) fn receive(
        &self,
        deadline: Instant,
        budget: &mut ReadBudget,
        cancel: &dyn Cancel,
    ) -> WireResult<Vec<u8>> {
        let mut data = Vec::new();
        let mut buffer = [0_u8; 65536];
        loop {
            check(deadline, cancel)?;
            // At the exact limit, peek only to distinguish EOF from excess data. Never
            // consume an uncharged byte, and do not accept a full frame without EOF.
            let exhausted = budget.remaining() == 0;
            let length = if exhausted {
                1
            } else {
                buffer.len().min(budget.remaining())
            };
            let flags = if exhausted { libc::MSG_PEEK } else { 0 };
            let count = unsafe {
                libc::recv(
                    self.0.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    length,
                    flags,
                )
            };
            if count == 0 {
                break;
            }
            if count < 0 {
                let error = io::Error::last_os_error();
                match error.kind() {
                    io::ErrorKind::Interrupted => continue,
                    io::ErrorKind::WouldBlock => {
                        self.wait(libc::POLLIN, deadline, cancel)?;
                        continue;
                    }
                    _ => return Err(io_failure(error)),
                }
            }
            if exhausted {
                return Err(Failure::new("response_limit"));
            }
            let count = count as usize;
            budget.charge(count)?;
            let had_newline = data.last() == Some(&b'\n');
            let chunk = &buffer[..count];
            if had_newline
                || chunk
                    .iter()
                    .position(|&byte| byte == b'\n')
                    .is_some_and(|index| index != count - 1)
            {
                return Err(Failure::new("trailing_data"));
            }
            data.extend_from_slice(chunk);
        }
        if data.pop() != Some(b'\n') {
            return Err(Failure::new("incomplete_response"));
        }
        check(deadline, cancel)?;
        Ok(data)
    }
}
