use crate::Result;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::{ffi::OsString, path::PathBuf};
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

pub(crate) fn ensure_supported() -> Result<()> {
    Ok(())
}

pub(crate) fn workspace_location(root: &Path) -> Result<super::WorkspaceLocation> {
    let metadata = root.metadata()?;
    crate::error::require(metadata.is_dir(), "invalid_action_workspace")?;
    Ok(super::WorkspaceLocation {
        root: root
            .to_str()
            .ok_or(crate::Error("non_unicode_path_unsupported"))?
            .to_owned(),
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

pub(crate) fn open_readonly(path: &Path) -> Result<File> {
    // Match Python's descriptor-level regular-file check without FIFO blocking.
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?)
}

pub(crate) fn replacement_mode(path: &Path) -> std::io::Result<Option<u32>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(metadata.mode() & 0o7777)),
        Ok(_) => Err(std::io::Error::other("unsafe replacement target type")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn apply_replacement_mode(file: &File, mode: u32) -> std::io::Result<()> {
    file.set_permissions(std::fs::Permissions::from_mode(mode & 0o7777))
}

pub(crate) fn verify_replacement_mode(file: &File, mode: u32) -> std::io::Result<()> {
    // fchmod may succeed while silently clearing SGID on a nonmember group.
    // Refuse before rename if the filesystem cannot retain the requested bits.
    if file.metadata()?.mode() & 0o7777 == mode & 0o7777 {
        Ok(())
    } else {
        Err(std::io::Error::other("replacement mode was not retained"))
    }
}

pub(crate) fn rename_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(source, target)
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
