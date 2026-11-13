//! Keep command parsing portable without promising unsupported filesystem applies.
use crate::{Error, Result, task_mutation::TaskOutput};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Plan,
    Enable,
    Disable,
}

pub struct Request {
    pub operation: Operation,
    pub kitsune_config: Option<PathBuf>,
    pub executable: Option<PathBuf>,
    pub state_root: Option<PathBuf>,
    pub apply: bool,
}

pub fn arguments(_: &Request, _: bool) -> Result<TaskOutput> {
    Err(Error("integration_setup_unsupported_platform"))
}
