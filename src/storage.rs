//! Shared persistent workspace lock and guarded immutable evidence publication.
//! Never replace/unlink a lock inode or steal it based on process metadata.
use crate::{Result, error::require, platform, read_fs};
use serde::Serialize;
use std::io::Write;
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

/// A known immutable publication is never rolled back after linking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    Committed,
    DurabilityUncertain,
}

pub(crate) trait EvidenceIo {
    fn read_for_sync(&self, path: &Path, limit: usize) -> Result<Vec<u8>> {
        read_fs::read_regular(path, limit)
    }
    fn open_for_sync(&self, path: &Path) -> std::io::Result<File> {
        platform::open_readonly(path)
            .map_err(|_| std::io::Error::other("evidence confirmation open failed"))
    }
    fn regular_for_sync(&self, file: &File) -> std::io::Result<bool> {
        Ok(file.metadata()?.is_file())
    }
    fn mkdir(&self, path: &Path) -> std::io::Result<()> {
        platform::mkdir_private(path)
    }
    fn temporary(&self, parent: &Path) -> std::io::Result<tempfile::NamedTempFile> {
        tempfile::Builder::new()
            .prefix(".handoff-")
            .suffix(".tmp")
            .tempfile_in(parent)
    }
    fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
        file.write_all(bytes)
    }
    fn sync_file(&self, file: &File) -> std::io::Result<()> {
        file.sync_all()
    }
    fn link(&self, source: &Path, target: &Path) -> std::io::Result<()> {
        std::fs::hard_link(source, target)
    }
    fn sync_dir(&self, path: &Path) -> std::io::Result<()> {
        platform::sync_directory(path)
    }
    fn cleanup(&self, temp: tempfile::NamedTempFile) -> std::io::Result<()> {
        temp.close()
    }
}
pub(crate) struct SystemEvidenceIo;
impl EvidenceIo for SystemEvidenceIo {}

pub fn prepare_directory(guard: &WorkspaceGuard, relative: &Path) -> Result<()> {
    prepare_directory_with(guard, relative, &SystemEvidenceIo)
}
pub(crate) fn prepare_directory_with(
    guard: &WorkspaceGuard,
    relative: &Path,
    io: &impl EvidenceIo,
) -> Result<()> {
    guard.validate_for(guard.root())?;
    let path = read_fs::managed(guard.root(), relative, true)?;
    if !path.exists() {
        io.mkdir(&path)?;
    }
    io.sync_dir(&path)?;
    io.sync_dir(
        path.parent()
            .ok_or(crate::Error("invalid_publication_parent"))?,
    )?;
    Ok(())
}

/// Explicit immutable publication under the matching owned workspace lock.
/// No replace/rename fallback and no promotion of pre-existing temporary files.
pub fn publish_new(guard: &WorkspaceGuard, relative: &Path, bytes: &[u8]) -> Result<Delivery> {
    publish_new_with(guard, relative, bytes, &SystemEvidenceIo)
}
pub(crate) fn publish_new_with(
    guard: &WorkspaceGuard,
    relative: &Path,
    bytes: &[u8],
    io: &impl EvidenceIo,
) -> Result<Delivery> {
    guard.validate_for(guard.root())?;
    require(bytes.len() <= 8 * 1_048_576, "unsupported_publication_size")?;
    let target = read_fs::managed(guard.root(), relative, false)?;
    let parent = target
        .parent()
        .ok_or(crate::Error("invalid_publication_parent"))?;
    let mut temp = io
        .temporary(parent)
        .map_err(|_| crate::Error("artifact_not_published"))?;
    let result = (|| {
        io.write(temp.as_file_mut(), bytes)
            .map_err(|_| crate::Error("artifact_not_published"))?;
        io.sync_file(temp.as_file())
            .map_err(|_| crate::Error("artifact_not_published"))?;
        read_fs::managed(guard.root(), relative, false)?;
        io.link(temp.path(), &target)
            .map_err(|_| crate::Error("artifact_not_published"))?;
        // From this point the final bytes exist. Never delete them or claim a
        // failed directory sync means the publication did not happen.
        Ok(if io.sync_dir(parent).is_ok() {
            Delivery::Committed
        } else {
            Delivery::DurabilityUncertain
        })
    })();
    let _ = io.cleanup(temp); // Only our own temp; cleanup failure leaves it inert.
    result
}

pub fn confirm_existing(
    guard: &WorkspaceGuard,
    relative: &Path,
    expected: &[u8],
) -> Result<Delivery> {
    confirm_existing_with(guard, relative, expected, &SystemEvidenceIo)
}

/// Proof that exact final bytes were observed under an owned workspace guard.
/// Only this typed prior observation permits operational confirmation failures
/// to become uncertainty rather than an unobserved/rejected publication.
pub struct ObservedEvidence {
    root: PathBuf,
    relative: PathBuf,
    expected: Vec<u8>,
}

pub fn observe_existing(
    guard: &WorkspaceGuard,
    relative: &Path,
    expected: &[u8],
) -> Result<ObservedEvidence> {
    guard.validate_for(guard.root())?;
    require(
        expected.len() <= 8 * 1_048_576,
        "unsupported_publication_size",
    )?;
    let path = read_fs::managed(guard.root(), relative, false)?;
    require(
        read_fs::read_regular(&path, expected.len())? == expected,
        "publication_content_mismatch",
    )?;
    Ok(ObservedEvidence {
        root: guard.root().to_owned(),
        relative: relative.to_owned(),
        expected: expected.to_vec(),
    })
}

pub fn confirm_observed(guard: &WorkspaceGuard, observed: &ObservedEvidence) -> Result<Delivery> {
    confirm_observed_with(guard, observed, &SystemEvidenceIo)
}
pub(crate) fn confirm_existing_with(
    guard: &WorkspaceGuard,
    relative: &Path,
    expected: &[u8],
    io: &impl EvidenceIo,
) -> Result<Delivery> {
    let observed = observe_existing(guard, relative, expected)?;
    confirm_observed_with(guard, &observed, io)
}
pub(crate) fn confirm_observed_with(
    guard: &WorkspaceGuard,
    observed: &ObservedEvidence,
    io: &impl EvidenceIo,
) -> Result<Delivery> {
    guard.validate_for(&observed.root)?;
    let path = match read_fs::managed(guard.root(), &observed.relative, false) {
        Ok(path) => path,
        Err(error) if error.code() == "input_or_storage_unavailable" => {
            return Ok(Delivery::DurabilityUncertain);
        }
        Err(error) => return Err(error),
    };
    let actual = match io.read_for_sync(&path, observed.expected.len()) {
        Ok(bytes) => bytes,
        Err(error) if error.code() == "input_or_storage_unavailable" => {
            return Ok(Delivery::DurabilityUncertain);
        }
        Err(error) => return Err(error),
    };
    require(actual == observed.expected, "publication_content_mismatch")?;
    let file = match io.open_for_sync(&path) {
        Ok(file) => file,
        Err(_) => return Ok(Delivery::DurabilityUncertain),
    };
    match io.regular_for_sync(&file) {
        Ok(true) => {}
        Ok(false) => return Err(crate::Error("unsafe_publication_file")),
        Err(_) => return Ok(Delivery::DurabilityUncertain),
    }
    if io.sync_file(&file).is_err() {
        return Ok(Delivery::DurabilityUncertain);
    }
    Ok(
        if io
            .sync_dir(
                path.parent()
                    .ok_or(crate::Error("invalid_publication_parent"))?,
            )
            .is_ok()
        {
            Delivery::Committed
        } else {
            Delivery::DurabilityUncertain
        },
    )
}
