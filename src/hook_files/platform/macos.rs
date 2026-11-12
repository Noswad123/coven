use super::MoveKind;
use std::{ffi::CStr, io, os::fd::RawFd};

pub(super) fn move_entry(
    from: RawFd,
    source: &CStr,
    to: RawFd,
    target: &CStr,
    kind: MoveKind,
) -> io::Result<()> {
    // renameatx_np was introduced in macOS 10.12; this is an API floor,
    // not a declaration of the product's minimum supported macOS release.
    let flags = match kind {
        MoveKind::Exchange => libc::RENAME_SWAP,
        MoveKind::NoReplace => libc::RENAME_EXCL,
    };
    if unsafe { libc::renameatx_np(from, source.as_ptr(), to, target.as_ptr(), flags) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
