use super::MoveKind;
use std::{
    ffi::CStr,
    fs::File,
    io,
    os::fd::{AsRawFd, RawFd},
};

unsafe extern "C" {
    // Project-owned ABI only. Apple ACL types and selectors stay in SDK-compiled C.
    fn coven_hook_acl_state(fd: libc::c_int) -> libc::c_int;
}

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

pub(super) fn validate_access(file: &File) -> io::Result<()> {
    match unsafe { coven_hook_acl_state(file.as_raw_fd()) } {
        0 => Ok(()),
        1 => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "hook file has an ACL",
        )),
        -1 => Err(io::Error::last_os_error()),
        _ => Err(io::Error::other("invalid hook ACL adapter result")),
    }
}
