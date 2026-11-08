//! Interactive native CLI execution, separate from bounded runtime capture.
//! The lifecycle coordinator retains its shared transport gate across this call,
//! without a workspace lock. Executable fallback belongs to the caller.
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) struct Request {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
}

pub(crate) trait Runner {
    fn run(&self, request: &Request) -> crate::runtime_command::Status;
}

pub(crate) struct NativeRunner;

impl Runner for NativeRunner {
    fn run(&self, request: &Request) -> crate::runtime_command::Status {
        crate::platform::runtime_foreground_run(request)
    }
}

pub(crate) fn terminal_ttys() -> (bool, bool) {
    crate::platform::runtime_terminal_ttys()
}
