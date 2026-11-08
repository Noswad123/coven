use crate::{Error, Result};

pub(crate) struct Artifact;
pub(crate) fn open(_: &str) -> Result<Artifact> {
    Err(Error("unsupported_platform"))
}
impl Artifact {
    pub(crate) fn bytes(&self) -> Result<&[u8]> {
        Err(Error("unsupported_platform"))
    }
    pub(crate) fn resolved(&self) -> &str {
        ""
    }
    pub(crate) fn device(&self) -> Option<u64> {
        None
    }
    pub(crate) fn inode(&self) -> Option<u64> {
        None
    }
    pub(crate) fn recheck(&self) -> Result<()> {
        Err(Error("unsupported_platform"))
    }
}
