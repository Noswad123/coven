//! Runtime admission, not workspace-state exclusion. Acquire BEFORE WorkspaceGuard.
//! Shared admissions cannot be upgraded; reuse a matching token via validate_for.
use crate::{Result, error::require, platform};
use std::{
    fs::File,
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

pub(crate) struct TransportGuard {
    root: PathBuf,
    file: File,
    exclusive: bool,
    owner: u32,
    thread: ThreadId,
    not_send_sync: PhantomData<Rc<()>>,
}

impl TransportGuard {
    pub(crate) fn shared(workspace: &Path, timeout: Duration) -> Result<Self> {
        Self::open(workspace, timeout, false)
    }

    pub(crate) fn exclusive(workspace: &Path, timeout: Duration) -> Result<Self> {
        Self::open(workspace, timeout, true)
    }

    fn open(workspace: &Path, timeout: Duration, exclusive: bool) -> Result<Self> {
        let root = workspace.canonicalize()?;
        require(root.is_dir(), "invalid_transport_workspace")?;
        let file = platform::open_transport_lock(&root)?;
        let started = Instant::now();
        loop {
            match platform::try_transport_lock(&file, exclusive) {
                Ok(true) => break,
                Ok(false) => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error.into()),
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            require(!remaining.is_zero(), "runtime_transport_gate_busy")?;
            thread::sleep(remaining.min(Duration::from_millis(50)));
        }
        Ok(Self {
            root,
            file,
            exclusive,
            owner: std::process::id(),
            thread: thread::current().id(),
            not_send_sync: PhantomData,
        })
    }

    pub(crate) fn validate_for(&self, workspace: &Path, exclusive: bool) -> Result<()> {
        require(
            self.owner == std::process::id()
                && self.thread == thread::current().id()
                && (!exclusive || self.exclusive)
                && workspace.canonicalize()? == self.root,
            "invalid_transport_gate_owner",
        )
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for TransportGuard {
    fn drop(&mut self) {
        // After fork, close only: LOCK_UN would unlock the parent's shared OFD.
        if self.owner == std::process::id() && self.thread == thread::current().id() {
            platform::unlock_transport(&self.file);
        }
    }
}

#[cfg(test)]
#[path = "transport_gate/tests.rs"]
mod tests;
