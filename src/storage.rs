//! Shared persistent workspace-lock primitive. No native mutation command uses it
//! yet. Never replace/unlink an inode or steal a lock based on process metadata.
use crate::{Result, error::require, platform, read_fs};
use std::{
    fs::File,
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

pub struct WorkspaceGuard {
    root: PathBuf,
    file: File,
    owner: u32,
    thread: ThreadId,
    // Python tokens are owned by a PID/thread; enforce thread ownership statically.
    not_send: PhantomData<Rc<()>>,
}

impl WorkspaceGuard {
    /// Explicit mutation-side acquisition may create state/lock, never reset it.
    pub fn acquire(workspace: &Path, timeout: Duration) -> Result<Self> {
        Self::open(workspace, timeout, true)
    }
    /// Read-side coordination must never create a lock or a missing directory.
    pub fn existing(workspace: &Path, timeout: Duration) -> Result<Self> {
        Self::open(workspace, timeout, false)
    }
    fn open(workspace: &Path, timeout: Duration, create: bool) -> Result<Self> {
        let root = read_fs::workspace(workspace)?;
        let state = read_fs::managed(&root, Path::new("state"), true)?;
        if create {
            std::fs::create_dir_all(state)?;
        }
        let path = read_fs::managed(&root, Path::new("state/.workspace.lock"), false)?;
        let file = platform::open_workspace_lock(&path, create)?;
        require(file.metadata()?.is_file(), "unsafe_workspace_lock")?;
        let started = Instant::now();
        loop {
            match platform::try_workspace_lock(&file) {
                Ok(true) => break,
                Ok(false) => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error.into()),
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            require(!remaining.is_zero(), "workspace_lock_busy")?;
            thread::sleep(remaining.min(Duration::from_millis(50)));
        }
        Ok(Self {
            root,
            file,
            owner: std::process::id(),
            thread: thread::current().id(),
            not_send: PhantomData,
        })
    }
    pub fn validate_for(&self, workspace: &Path) -> Result<()> {
        require(
            self.owner == std::process::id()
                && self.thread == thread::current().id()
                && workspace.canonicalize()? == self.root,
            "invalid_workspace_lock_owner",
        )
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for WorkspaceGuard {
    fn drop(&mut self) {
        // A forked child must not explicitly unlock the parent's shared open-file
        // description. Closing only its inherited descriptor leaves the parent held.
        if self.owner == std::process::id() && self.thread == thread::current().id() {
            platform::unlock_workspace(&self.file);
        }
    }
}
