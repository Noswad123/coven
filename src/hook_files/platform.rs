//! Descriptor-only OS primitives. Successful sync means fsync, not a power-loss guarantee.
use std::{ffi::CStr, fs::File, io, os::fd::RawFd};

#[cfg(target_os = "linux")]
#[path = "platform/linux.rs"]
mod linux;
#[cfg(target_os = "macos")]
#[path = "platform/macos.rs"]
mod macos;
#[cfg(target_os = "linux")]
use linux as native;
#[cfg(target_os = "macos")]
use macos as native;

#[derive(Clone, Copy, Debug)]
pub(super) enum MoveKind {
    Exchange,
    NoReplace,
}

pub(super) fn supports_apply() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
}

pub(super) fn move_entry(
    from: RawFd,
    source: &CStr,
    to: RawFd,
    target: &CStr,
    kind: MoveKind,
) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return native::move_entry(from, source, to, target, kind);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (from, source, to, target, kind);
        Err(unsupported())
    }
}

pub(super) fn validate_access(file: &File) -> io::Result<()> {
    crate::private_access::validate_fd_access(file)
}

pub(super) fn sync_file(file: &File) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::fd::AsRawFd;
        // Both files and directories use fsync; directory errors are never suppressed.
        if unsafe { libc::fsync(file.as_raw_fd()) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = file;
        Err(unsupported())
    }
}

pub(super) fn sync_directory(file: &File) -> io::Result<()> {
    sync_file(file)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "hook file apply is unsupported on this OS",
    )
}

#[cfg(test)]
#[path = "platform/tests.rs"]
mod tests;
