use crate::{Error, Result};
use std::{fs::File, path::Path};

pub(crate) fn ensure_supported() -> Result<()> {
    Err(Error("unsupported_platform"))
}
pub(crate) fn open_readonly(_: &Path) -> Result<File> {
    Err(Error("unsupported_platform"))
}

pub(crate) fn open_workspace_lock(_: &Path, _: bool) -> Result<File> {
    Err(Error("unsupported_platform"))
}
pub(crate) fn try_workspace_lock(_: &File) -> std::io::Result<bool> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}
pub(crate) fn unlock_workspace(_: &File) {}
pub(crate) fn workspace_argument(_: &Path) -> Result<std::path::PathBuf> {
    Err(Error("unsupported_platform"))
}
