//! Additional OS privacy policy, not owner/mode admission or peer authentication.
use std::{ffi::CStr, fs::File, io};

pub(crate) fn supports_privacy() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    // Project-owned flat ABI. SDK types and parsing remain in the single C unit.
    fn coven_hook_acl_state(fd: libc::c_int) -> libc::c_int;
    fn coven_socket_entry_acl_state(
        parent: libc::c_int,
        leaf: *const libc::c_char,
        device: u64,
        inode: u64,
        uid: u32,
        mode: u32,
    ) -> libc::c_int;
}

#[cfg(target_os = "macos")]
fn acl_result(result: libc::c_int) -> io::Result<()> {
    match result {
        0 => Ok(()),
        1 => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "private object has an ACL",
        )),
        -1 => Err(io::Error::last_os_error()),
        _ => Err(io::Error::other("invalid private ACL adapter result")),
    }
}

pub(crate) fn validate_fd_access(file: &File) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;
        acl_result(unsafe { coven_hook_acl_state(file.as_raw_fd()) })
    }
    #[cfg(target_os = "linux")]
    {
        // Callers retain owner/mode policy. This does not assert ACL absence.
        let _ = file;
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = file;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "private access policy is unsupported on this OS",
        ))
    }
}

/// Inspect a basename relative to a retained private parent, never a socket FD.
/// `mode` is the exact permission/special-bit snapshot (st_mode & 07777).
/// Consumers enforce socket type, owner, nlink and usable client mode 0600.
pub(crate) fn validate_socket_entry(
    parent: &File,
    leaf: &CStr,
    device: u64,
    inode: u64,
    uid: u32,
    mode: u32,
) -> io::Result<()> {
    let name = leaf.to_bytes();
    if name.is_empty() || name == b"." || name == b".." || name.contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "socket entry must be a basename",
        ));
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;
        acl_result(unsafe {
            coven_socket_entry_acl_state(
                parent.as_raw_fd(),
                leaf.as_ptr(),
                device,
                inode,
                uid,
                mode,
            )
        })
    }
    #[cfg(target_os = "linux")]
    {
        let _ = (parent, device, inode, uid, mode);
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (parent, device, inode, uid, mode);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "private access policy is unsupported on this OS",
        ))
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    /// Physical known temporary base for receiver tests; not production resolution.
    pub(crate) fn tempdir() -> tempfile::TempDir {
        let base = std::env::temp_dir()
            .canonicalize()
            .expect("physical temporary base");
        tempfile::tempdir_in(base).expect("private test directory")
    }
}
