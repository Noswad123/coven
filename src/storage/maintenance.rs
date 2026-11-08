//! Source-independent coordination for execution cancellation and recovery.
use super::WorkspaceGuard;
use crate::{Result, error::require, platform};
use std::{path::Path, time::Duration};

impl WorkspaceGuard {
    /// Acquire the persistent workspace lock without admitting any current sources.
    pub(crate) fn execution_maintenance(workspace: &Path, timeout: Duration) -> Result<Self> {
        platform::ensure_supported()?;
        let root = workspace.canonicalize()?;
        require(root.is_dir(), "invalid_handoff_workspace")?;
        Self::open_canonical_root(root, timeout, true)
    }
}

#[cfg(all(test, unix))]
mod tests;
