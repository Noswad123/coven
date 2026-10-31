use crate::Result;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::DirBuilderExt;
use std::{ffi::OsString, path::PathBuf};
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

pub(crate) fn ensure_supported() -> Result<()> {
    Ok(())
}

pub(crate) fn open_readonly(path: &Path) -> Result<File> {
    // Match Python's descriptor-level regular-file check without FIFO blocking.
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?)
}

pub(crate) fn open_workspace_lock(path: &Path, create: bool) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?)
}

pub(crate) fn try_workspace_lock(file: &File) -> std::io::Result<bool> {
    // flock coordinates with the existing Python kernel lock on the same inode.
    let status = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if status == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if matches!(
        error.raw_os_error(),
        Some(libc::EAGAIN) | Some(libc::EACCES)
    ) {
        Ok(false)
    } else {
        Err(error)
    }
}

pub(crate) fn unlock_workspace(file: &File) {
    // Failure cannot safely be repaired by unlinking/replacing a persistent lock.
    unsafe {
        libc::flock(file.as_raw_fd(), libc::LOCK_UN);
    }
}

pub(crate) fn workspace_argument(path: &Path) -> Result<PathBuf> {
    let raw = path.as_os_str().as_bytes();
    if raw.is_empty() {
        return Ok(PathBuf::from("."));
    }
    if raw[0] != b'~' {
        return Ok(path.to_owned());
    }
    let end = raw
        .iter()
        .position(|byte| *byte == b'/')
        .unwrap_or(raw.len());
    // Avoid selecting a literal '~user' directory or implicit NSS/network lookup.
    if end != 1 {
        return Err(crate::Error("unsupported_named_user_expansion"));
    }
    let home = std::env::var_os("HOME").ok_or(crate::Error("home_expansion_unavailable"))?;
    let home = home.as_os_str().as_bytes();
    let end_home = home
        .iter()
        .rposition(|byte| *byte != b'/')
        .map_or(0, |index| index + 1);
    let mut expanded = home[..end_home].to_vec();
    expanded.extend_from_slice(&raw[end..]);
    if expanded.is_empty() {
        expanded.push(b'/');
    }
    Ok(PathBuf::from(OsString::from_vec(expanded)))
}

pub(crate) fn mkdir_private(path: &Path) -> std::io::Result<()> {
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

pub(crate) fn sync_directory(path: &Path) -> std::io::Result<()> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?
        .sync_all()
}
