//! Projection recovery only: no authority mutation or execution permission.
//! Like storage replacement, assumes static paths on a cooperating local filesystem.
use super::*;
use crate::{
    platform,
    storage::{self, Delivery, WorkspaceGuard},
};
use std::{
    fmt,
    fs::File,
    io::{self, Write},
    path::PathBuf,
};

const RECOVERY: &str = "state/task-recovery";
const PROJECTION: &str = "state/tasks.json";
const MAX_RAW: usize = 32 * 1_048_576;

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairStatus {
    /// Confirmed authority and positively equal captured projection; no backup.
    Current,
    /// Replacement was observed, even if its directory synchronization failed.
    Repaired,
    /// Authority synchronization was uncertain; the projection was not accessed.
    AuthorityUnconfirmed,
    /// Backup synchronization was uncertain; replacement was not attempted.
    BackupUnconfirmed,
}

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairDurability {
    Confirmed,
    SynchronizationUncertain,
}

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairFailurePhase {
    Authority,
    ProjectionCapture,
    BackupPreparation,
    BackupPublication,
    ProjectionRevalidation,
    ProjectionReplacement,
}

/// Captured selected state and this repair's known outcome, not a whole-FS claim.
#[must_use]
pub struct RepairReport {
    view: View,
    status: RepairStatus,
    durability: RepairDurability,
    backup_path: Option<PathBuf>,
}
impl RepairReport {
    pub fn view(&self) -> &View {
        &self.view
    }
    pub fn into_view(self) -> View {
        self.view
    }
    pub fn status(&self) -> RepairStatus {
        self.status
    }
    pub fn durability(&self) -> RepairDurability {
        self.durability
    }
    /// Absolute path of the exclusively created backup, including partial backups.
    pub fn backup_path(&self) -> Option<&Path> {
        self.backup_path.as_deref()
    }
}
impl fmt::Debug for RepairReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RepairReport")
            .field("status", &self.status)
            .field("durability", &self.durability)
            .field("backup_path", &self.backup_path)
            .finish_non_exhaustive()
    }
}

pub struct RepairFailure {
    cause: Error,
    phase: RepairFailurePhase,
    known_view: Option<View>,
    backup_path: Option<PathBuf>,
}
impl RepairFailure {
    pub fn code(&self) -> &'static str {
        self.cause.code()
    }
    pub fn phase(&self) -> RepairFailurePhase {
        self.phase
    }
    pub fn known_view(&self) -> Option<&View> {
        self.known_view.as_ref()
    }
    /// Absolute path only after this call successfully created the backup child.
    pub fn backup_path(&self) -> Option<&Path> {
        self.backup_path.as_deref()
    }
}
impl fmt::Debug for RepairFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RepairFailure")
            .field("cause", &self.cause)
            .field("phase", &self.phase)
            .field("has_known_view", &self.known_view.is_some())
            .field("backup_path", &self.backup_path)
            .finish()
    }
}
impl fmt::Display for RepairFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.cause.fmt(f)
    }
}
impl std::error::Error for RepairFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

// Domain-local seams keep fault injection out of shared storage and public APIs.
trait RepairIo {
    fn reload(&self, root: &Path) -> Result<Option<View>> {
        super::read(root)
    }
    fn confirm(
        &self,
        root: &Path,
        view: &View,
        guard: &WorkspaceGuard,
    ) -> Result<LedgerConfirmation> {
        confirm_selected(root, view, guard)
    }
    fn capture(&self, root: &Path) -> Result<Option<Vec<u8>>> {
        let path = read_fs::managed(root, Path::new(PROJECTION), false)?;
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
            Ok(_) => read_fs::read_regular(&path, MAX_RAW)
                .map(Some)
                .map_err(|error| match error.code() {
                    "handoff_size_limit" => Error("unsupported_recovery_size"),
                    _ => error,
                }),
        }
    }
    fn id(&self) -> Result<String> {
        platform::random_uuid_hex()
    }
    fn mkdir(&self, path: &Path) -> io::Result<()> {
        platform::mkdir_private(path)
    }
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        platform::sync_directory(path)
    }
    fn temporary(&self, parent: &Path) -> io::Result<tempfile::NamedTempFile> {
        tempfile::Builder::new()
            .prefix(".repair-")
            .suffix(".tmp")
            .tempfile_in(parent)
    }
    fn write(&self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
        file.write_all(bytes)
    }
    fn apply_mode(&self, file: &File) -> io::Result<()> {
        platform::apply_replacement_mode(file, 0o600)
    }
    fn verify_mode(&self, file: &File) -> io::Result<()> {
        platform::verify_replacement_mode(file, 0o600)
    }
    fn sync_file(&self, file: &File) -> io::Result<()> {
        file.sync_all()
    }
    fn recheck(&self, root: &Path, relative: &Path) -> Result<PathBuf> {
        read_fs::managed(root, relative, false)
    }
    fn link(&self, source: &Path, target: &Path) -> io::Result<()> {
        fs::hard_link(source, target)
    }
    fn cleanup(&self, temp: tempfile::NamedTempFile) -> io::Result<()> {
        temp.close()
    }
    fn replace(&self, guard: &WorkspaceGuard, raw: &[u8]) -> Result<Delivery> {
        storage::atomic_replace_bytes(guard, Path::new(PROJECTION), raw)
    }
}
struct SystemIo;
impl RepairIo for SystemIo {}

#[cfg(all(test, unix))]
mod tests;

enum BackupFile {
    Tasks,
    Metadata,
}
impl BackupFile {
    fn name(&self) -> &'static str {
        match self {
            Self::Tasks => "tasks.bytes",
            Self::Metadata => "metadata.json",
        }
    }
}

// Raw backups deliberately bypass both JSON profiles and ledger's 8 MiB baseline
// transport. Only our own temporary is cleaned; final/partial backups are retained.
fn publish_backup(
    root: &Path,
    base: &Path,
    role: BackupFile,
    raw: &[u8],
    io: &impl RepairIo,
) -> Result<Delivery> {
    require(raw.len() <= MAX_RAW, "unsupported_recovery_size")?;
    let relative = base.join(role.name());
    let target = io.recheck(root, &relative)?;
    let parent = target.parent().ok_or(Error("invalid_publication_parent"))?;
    require(
        fs::symlink_metadata(parent)?.is_dir(),
        "unsafe_publication_parent",
    )?;
    let mut temp = io.temporary(parent)?;
    let result = (|| {
        io.write(temp.as_file_mut(), raw)?;
        io.apply_mode(temp.as_file())?;
        io.verify_mode(temp.as_file())?;
        io.sync_file(temp.as_file())?;
        io.recheck(root, &relative)?;
        io.link(temp.path(), &target)?;
        Ok(if io.sync_dir(parent).is_ok() {
            Delivery::Committed
        } else {
            Delivery::DurabilityUncertain
        })
    })();
    let _ = io.cleanup(temp);
    result
}

/// Confirm selected authority before even reading the projection. Preserve its
/// exact observed bytes durably before replacement; never adopt a partial backup.
/// No CAS, execution approval, or immunity to hostile concurrent path races.
// The SDK failure contract retains an owned, unboxed Option<View> for recovery.
#[allow(clippy::result_large_err)]
pub fn repair_projection(
    workspace: &Path,
    guard: &WorkspaceGuard,
) -> std::result::Result<RepairReport, RepairFailure> {
    repair_with(workspace, guard, &SystemIo)
}

// Match the public failure contract without discarding captured state.
#[allow(clippy::result_large_err)]
fn repair_with(
    workspace: &Path,
    guard: &WorkspaceGuard,
    io: &impl RepairIo,
) -> std::result::Result<RepairReport, RepairFailure> {
    use RepairDurability::{Confirmed, SynchronizationUncertain};
    use RepairStatus::{AuthorityUnconfirmed, BackupUnconfirmed, Current, Repaired};
    let mut phase = RepairFailurePhase::Authority;
    let mut known_view = None;
    let mut backup_path = None;
    let result = (|| -> Result<(RepairStatus, RepairDurability)> {
        guard.validate_for(workspace)?;
        let root = guard.root();
        known_view = Some(io.reload(root)?.ok_or(Error("no_active_ledger"))?);
        let view = known_view.as_ref().ok_or(Error("no_active_ledger"))?;
        if io.confirm(root, view, guard)? == LedgerConfirmation::SynchronizationUncertain {
            return Ok((AuthorityUnconfirmed, SynchronizationUncertain));
        }
        phase = RepairFailurePhase::ProjectionCapture;
        let expected = view.projection_bytes()?;
        require(expected.len() <= MAX_RAW, "unsupported_replacement_size")?;
        let observed = io.capture(root)?;
        if observed.as_deref() == Some(expected.as_slice()) {
            return Ok((Current, Confirmed));
        }
        // All encoding and size admission precedes any recovery namespace changes.
        let metadata = Value::object(vec![
            ("version".into(), Value::integer("1")?),
            ("missing".into(), Value::Bool(observed.is_none())),
            (
                "revision".into(),
                Value::String(snapshot_revision(observed.as_deref())),
            ),
        ])?
        .storage()?;
        phase = RepairFailurePhase::BackupPreparation;
        let recovery = read_fs::managed(root, Path::new(RECOVERY), true)?;
        match fs::symlink_metadata(&recovery) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => io.mkdir(&recovery)?,
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        // Also sync existing ancestors: their existence alone is not durability.
        for relative in [RECOVERY, "state"] {
            let path = read_fs::managed(root, Path::new(relative), true)?;
            if io.sync_dir(&path).is_err() {
                return Ok((BackupUnconfirmed, SynchronizationUncertain));
            }
        }
        if io.sync_dir(root).is_err() {
            return Ok((BackupUnconfirmed, SynchronizationUncertain));
        }
        let id = io.id()?;
        require(handoff::hex(&id, 32), "invalid_recovery_identity")?;
        let base = Path::new(RECOVERY).join(id);
        let path = read_fs::managed(root, &base, true)?;
        // Exclusive mkdir: even an existing empty directory is never adopted.
        io.mkdir(&path)?;
        backup_path = Some(path.clone());
        for path in [&path, &recovery] {
            if io.sync_dir(path).is_err() {
                return Ok((BackupUnconfirmed, SynchronizationUncertain));
            }
        }
        phase = RepairFailurePhase::BackupPublication;
        if let Some(raw) = &observed {
            if publish_backup(root, &base, BackupFile::Tasks, raw, io)?
                == Delivery::DurabilityUncertain
            {
                return Ok((BackupUnconfirmed, SynchronizationUncertain));
            }
        }
        if publish_backup(root, &base, BackupFile::Metadata, &metadata, io)?
            == Delivery::DurabilityUncertain
        {
            return Ok((BackupUnconfirmed, SynchronizationUncertain));
        }
        phase = RepairFailurePhase::ProjectionRevalidation;
        // Full replay, not just a marker fence. Revision and digest are independent.
        let actual = io.reload(root)?.ok_or(Error("ledger_head_changed"))?;
        require(
            actual.revision == view.revision && actual.head_digest == view.head_digest,
            "ledger_head_changed",
        )?;
        let replacement = actual.projection_bytes()?;
        require(replacement.len() <= MAX_RAW, "unsupported_replacement_size")?;
        known_view = Some(actual);
        require(io.capture(root)? == observed, "ledger_projection_changed")?;
        phase = RepairFailurePhase::ProjectionReplacement;
        let delivery = io.replace(guard, &replacement)?;
        // No fallible post-replacement reads can erase the known visibility result.
        Ok((
            Repaired,
            match delivery {
                Delivery::Committed => Confirmed,
                Delivery::DurabilityUncertain => SynchronizationUncertain,
            },
        ))
    })();
    match result {
        Ok((status, durability)) => match known_view {
            Some(view) => Ok(RepairReport {
                view,
                status,
                durability,
                backup_path,
            }),
            None => Err(RepairFailure {
                cause: Error("no_active_ledger"),
                phase,
                known_view: None,
                backup_path,
            }),
        },
        Err(cause) => Err(RepairFailure {
            cause,
            phase,
            known_view,
            backup_path,
        }),
    }
}
