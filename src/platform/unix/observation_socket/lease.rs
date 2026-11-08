use std::ffi::CString;
use std::fs::File;
use std::marker::PhantomData;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::rc::Rc;
use std::thread::ThreadId;

/// Invocation-local, root-bound lease. Not transferable across threads. Forked
/// copies may only be closed: LOCK_UN would unlock the parent's shared OFD too.
pub(crate) struct PublisherLease {
    _lock: File,
    _state: File,
    _root: File,
    _pid: libc::pid_t,
    _thread: ThreadId,
    _local: PhantomData<Rc<()>>,
}

impl PublisherLease {
    pub(crate) fn acquire(workspace: &Path) -> crate::Result<Self> {
        let error = || crate::Error("publisher_lease_error");
        let path = CString::new(workspace.as_os_str().as_bytes()).map_err(|_| error())?;
        let raw = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            return Err(error());
        }
        let root = unsafe { File::from_raw_fd(raw) };
        let raw = unsafe {
            libc::openat(
                root.as_raw_fd(),
                c"state".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            return Err(error());
        }
        let state = unsafe { File::from_raw_fd(raw) };
        let raw = unsafe {
            libc::openat(
                state.as_raw_fd(),
                c".group-publisher.lock".as_ptr(),
                libc::O_RDWR
                    | libc::O_CREAT
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
                0o600,
            )
        };
        if raw < 0 {
            return Err(error());
        }
        let lock = unsafe { File::from_raw_fd(raw) };
        let metadata = lock.metadata().map_err(|_| error())?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::getuid() }
            || metadata.mode() & 0o7777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(error());
        }
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let failure = std::io::Error::last_os_error();
            return Err(if failure.kind() == std::io::ErrorKind::WouldBlock {
                crate::Error("publisher_busy")
            } else {
                error()
            });
        }
        // Fence the names after locking; never unlink, replace, truncate, or chmod
        // a persistent lock file (including on any failure path).
        for (file, path) in [
            (&root, workspace.to_path_buf()),
            (&state, workspace.join("state")),
            (&lock, workspace.join("state/.group-publisher.lock")),
        ] {
            let held = file.metadata().map_err(|_| error())?;
            let named = std::fs::symlink_metadata(path).map_err(|_| error())?;
            if held.dev() != named.dev() || held.ino() != named.ino() {
                return Err(error());
            }
        }
        Ok(Self {
            _lock: lock,
            _state: state,
            _root: root,
            _pid: unsafe { libc::getpid() },
            _thread: std::thread::current().id(),
            _local: PhantomData,
        })
    }
}

// File's owned Drop closes descriptors in both parent and forked child. Do not
// add explicit flock(LOCK_UN), pathname cleanup, or thread-local calls to Drop.

#[cfg(test)]
#[path = "lease_tests.rs"]
mod tests;
