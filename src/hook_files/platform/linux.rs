use super::MoveKind;
use std::{ffi::CStr, fs::File, io, os::fd::RawFd};

pub(super) fn move_entry(
    from: RawFd,
    source: &CStr,
    to: RawFd,
    target: &CStr,
    kind: MoveKind,
) -> io::Result<()> {
    let flags = match kind {
        MoveKind::Exchange => libc::RENAME_EXCHANGE,
        MoveKind::NoReplace => libc::RENAME_NOREPLACE,
    };
    if unsafe { libc::renameat2(from, source.as_ptr(), to, target.as_ptr(), flags) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(super) fn validate_access(_file: &File) -> io::Result<()> {
    // Shared ownership and mode admission checks remain in Root. Linux adds no
    // admission checks here; success does not claim ACL absence.
    Ok(())
}
