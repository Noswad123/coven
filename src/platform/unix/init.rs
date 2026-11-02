use crate::{Result, error::require};
use std::os::unix::{
    ffi::{OsStrExt, OsStringExt},
    fs::{MetadataExt, OpenOptionsExt},
};
use std::{
    collections::VecDeque,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

/// Resolve in filesystem order, including symlinks before a following `..`.
/// Missing tails are permitted, but no filesystem objects are created.
pub(crate) fn resolve_init_target(argument: &Path) -> Result<PathBuf> {
    let expanded = super::workspace_argument(argument)?;
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()?.join(expanded)
    };
    let mut pending: VecDeque<_> = absolute
        .as_os_str()
        .as_bytes()
        .split(|b| *b == b'/')
        .filter(|part| !part.is_empty())
        .map(|part| part.to_vec())
        .collect();
    let mut resolved = PathBuf::from("/");
    let mut links = 0;
    while let Some(part) = pending.pop_front() {
        match std::fs::symlink_metadata(&resolved) {
            Ok(metadata) => require(metadata.is_dir(), "invalid_init_ancestor")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if part == b"." {
            continue;
        }
        if part == b".." {
            resolved.pop();
            continue;
        }
        let candidate = resolved.join(std::ffi::OsString::from_vec(part));
        match std::fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                links += 1;
                require(links <= 40, "init_symlink_loop")?;
                let target = std::fs::read_link(&candidate)?;
                if target.is_absolute() {
                    resolved = PathBuf::from("/");
                }
                let parts: Vec<_> = target
                    .as_os_str()
                    .as_bytes()
                    .split(|b| *b == b'/')
                    .filter(|part| !part.is_empty())
                    .map(|part| part.to_vec())
                    .collect();
                for part in parts.into_iter().rev() {
                    pending.push_front(part);
                }
            }
            Ok(metadata) => {
                require(metadata.is_dir(), "invalid_init_ancestor")?;
                resolved = candidate;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => resolved = candidate,
            Err(error) => return Err(error.into()),
        }
    }
    require(resolved.to_str().is_some(), "non_unicode_path_unsupported")?;
    Ok(resolved)
}

pub(crate) fn open_init_directory(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?)
}

pub(crate) fn open_init_lock(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    validate_init_identity(path, &file, false)?;
    Ok(file)
}

pub(crate) fn validate_init_identity(path: &Path, file: &File, directory: bool) -> Result<()> {
    let named = std::fs::symlink_metadata(path)?;
    let opened = file.metadata()?;
    require(
        if directory {
            named.is_dir() && opened.is_dir()
        } else {
            named.is_file() && opened.is_file() && named.nlink() == 1 && opened.nlink() == 1
        },
        "invalid_init_identity",
    )?;
    require(
        named.dev() == opened.dev() && named.ino() == opened.ino(),
        "invalid_init_identity",
    )
}

pub(crate) fn validate_init_temp(path: &Path, file: &File) -> Result<()> {
    let named = std::fs::symlink_metadata(path)?;
    let opened = file.metadata()?;
    require(
        named.is_file()
            && opened.is_file()
            && named.dev() == opened.dev()
            && named.ino() == opened.ino(),
        "invalid_init_identity",
    )
}

#[cfg(test)]
mod tests;
