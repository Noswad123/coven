use crate::Result;
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
