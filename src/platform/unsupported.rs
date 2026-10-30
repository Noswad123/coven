use crate::{Error, Result};
use std::{fs::File, path::Path};

pub(crate) fn ensure_supported() -> Result<()> {
    Err(Error("unsupported_platform"))
}
pub(crate) fn open_readonly(_: &Path) -> Result<File> {
    Err(Error("unsupported_platform"))
}
