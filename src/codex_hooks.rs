//! Offline native Codex hook policy, pinned to openai/codex rust-v0.167.1
//! (0a94398fa60c4fac0e7659e4213a0a36e613dd46). Only SessionStart is generated.
//! Enabling the feature does not grant native hook approval. No provider, shell,
//! daemon, or integration script is executed by this module.
//!
//! The private receipt is a bounded two-file journal, not an atomic transaction.
//! Pending operations resume only across known source/target committed prefixes.
//! Detected filesystem races retain the shared engine's recovery directory for
//! manual resolution; even previews refuse while it exists. Linux and macOS have
//! apply backends; macOS is provisional and requires eligible local APFS objects.
use crate::{Result, task_mutation::TaskOutput};
use std::path::PathBuf;

pub use crate::claude_hooks::Operation;

pub struct Request {
    pub config_root: PathBuf,
    pub executable: Option<String>,
    pub socket: Option<String>,
    pub migrate_kitsune: bool,
    pub apply: bool,
    pub operation: Operation,
}

/// Preview is side-effect free, including `Plan` with `apply`. Applying Install
/// authorizes global `features.hooks = true`, but never native trust approval.
pub fn arguments(request: &Request, json: bool) -> Result<TaskOutput> {
    #[cfg(unix)]
    {
        unix::run(request, json)
    }
    #[cfg(not(unix))]
    {
        let _ = (request, json);
        Err(crate::Error("codex_hooks_unsupported_platform"))
    }
}

#[cfg(unix)]
#[path = "codex_hooks/native.rs"]
mod native;
#[cfg(unix)]
#[path = "codex_hooks/policy.rs"]
mod policy;
#[cfg(unix)]
#[path = "codex_hooks/unix.rs"]
mod unix;
