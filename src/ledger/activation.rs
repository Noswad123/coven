//! Explicit, guard-scoped snapshot activation. No projection or secondary writes.
use super::publication::{self, ArtifactRole, Publication};
use super::*;
use crate::{execution, platform, storage::WorkspaceGuard};
use std::{fmt, path::PathBuf};

/// Synchronization outcome for this activation operation, not a guarantee of
/// continuing selection or global authority after the operation returns.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationDurability {
    /// This operation's required synchronization calls succeeded.
    Confirmed,
    SynchronizationUncertain,
}

#[must_use]
pub struct ActivationReport {
    view: View,
    durability: ActivationDurability,
}
impl ActivationReport {
    pub fn view(&self) -> &View {
        &self.view
    }
    /// Consume the report, discarding its durability status. Inspect
    /// [`Self::durability`] first if that status matters to the caller.
    pub fn into_view(self) -> View {
        self.view
    }
    pub fn durability(&self) -> ActivationDurability {
        self.durability
    }
}
impl fmt::Debug for ActivationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActivationReport")
            .field("revision", &self.view.revision)
            .field("durability", &self.durability)
            .finish_non_exhaustive()
    }
}

/// Synchronization outcome for this archive operation, not a continuing or
/// global guarantee about authority or the archived path.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveDurability {
    /// This operation's required synchronization calls succeeded.
    Confirmed,
    SynchronizationUncertain,
}

#[must_use]
#[derive(Debug)]
pub struct ArchiveReport {
    path: PathBuf,
    durability: ArchiveDurability,
}
impl ArchiveReport {
    /// Absolute path known to have received the pending directory by rename,
    /// including when subsequent synchronization was uncertain.
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn durability(&self) -> ArchiveDurability {
        self.durability
    }
}

/// Operation-local knowledge, never proof that no earlier artifacts exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportProgress {
    NoNewArtifacts,
    Pending {
        /// The last final file known linked by this invocation, relative to the
        /// guarded workspace root. Not the latest pre-existing artifact.
        last_published: Option<PathBuf>,
        /// Whether this invocation encountered synchronization uncertainty.
        /// `false` does not certify durability of pre-existing artifacts.
        synchronization_uncertain: bool,
    },
}
impl ImportProgress {
    fn pending(&mut self) {
        if matches!(self, Self::NoNewArtifacts) {
            *self = Self::Pending {
                last_published: None,
                synchronization_uncertain: false,
            };
        }
    }
    fn uncertain(&mut self) {
        if let Self::Pending {
            synchronization_uncertain,
            ..
        } = self
        {
            *synchronization_uncertain = true;
        }
    }
    fn linked(&mut self, path: PathBuf) {
        self.pending();
        if let Self::Pending { last_published, .. } = self {
            *last_published = Some(path);
        }
    }
}

#[derive(Debug)]
pub struct ImportFailure {
    cause: Error,
    progress: ImportProgress,
}
impl ImportFailure {
    pub fn code(&self) -> &'static str {
        self.cause.code()
    }
    pub fn progress(&self) -> &ImportProgress {
        &self.progress
    }
}
impl fmt::Display for ImportFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.cause.fmt(f)
    }
}
impl std::error::Error for ImportFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

struct BaselinePlan {
    epoch: String,
    source: Option<Vec<u8>>,
    mode: Option<u32>,
    frame: Vec<u8>,
    manifest: Vec<u8>,
    marker: Vec<u8>,
}

fn encoded(value: &Value) -> Result<Vec<u8>> {
    let mut raw = value.canonical()?;
    raw.push(b'\n');
    require(
        raw.len() <= MAX_FRAME_BYTES,
        "unsupported_ledger_frame_size",
    )?;
    // Encoding alone does not enforce the authority parser's depth/node limits.
    values::parse(&raw, Profile::Authority)?;
    Ok(raw)
}

fn marker_for(manifest: &Value, raw: &[u8]) -> Result<Vec<u8>> {
    let mut fields = Vec::new();
    for key in ["version", "workspace_id", "epoch_id", "baseline_digest"] {
        fields.push((key.to_owned(), get(manifest, key)?.clone()));
    }
    fields.push((
        "manifest_digest".into(),
        Value::String(canonical::digest(raw)),
    ));
    encoded(&Value::object(fields)?)
}

impl BaselinePlan {
    /// Pure fixed-identity constructor, also used for oracle byte vectors.
    fn new(
        source: Option<Vec<u8>>,
        mode: Option<u32>,
        workspace: &str,
        epoch: &str,
    ) -> Result<Self> {
        require(
            handoff::hex(workspace, 32) && handoff::hex(epoch, 32),
            "invalid_ledger_identity",
        )?;
        require(
            source.is_some() == mode.is_some() && mode.is_none_or(|v| v <= 0o7777),
            "invalid_ledger_source_mode",
        )?;
        if let Some(raw) = &source {
            require(
                raw.len() <= MAX_FRAME_BYTES,
                "unsupported_ledger_frame_size",
            )?;
        }
        let tasks = TaskRecords::validate(
            source
                .as_deref()
                .map(|raw| values::parse(raw, Profile::Authority))
                .transpose()?
                .unwrap_or(Value::Array(Vec::new())),
        )?
        .into_value();
        let source_revision = snapshot_revision(source.as_deref());
        let initial = Value::object(vec![
            ("version".into(), Value::integer("1")?),
            ("workspace_id".into(), Value::String(workspace.into())),
            ("epoch_id".into(), Value::String(epoch.into())),
            ("sequence".into(), Value::integer("0")?),
            ("previous_digest".into(), Value::Null),
            (
                "source_revision".into(),
                Value::String(source_revision.clone()),
            ),
            (
                "revision".into(),
                Value::String(revision(workspace, epoch, 0, &tasks)?),
            ),
            ("tasks".into(), tasks),
        ])?;
        let payload_raw = initial.canonical()?;
        let frame = encoded(&Value::object(vec![
            ("version".into(), Value::integer("1")?),
            (
                "length".into(),
                Value::integer(&payload_raw.len().to_string())?,
            ),
            (
                "sha256".into(),
                Value::String(canonical::digest(&payload_raw)),
            ),
            ("payload".into(), initial),
        ])?)?;
        payload(&frame)?;
        let manifest_value = Value::object(vec![
            ("version".into(), Value::integer("1")?),
            ("workspace_id".into(), Value::String(workspace.into())),
            ("epoch_id".into(), Value::String(epoch.into())),
            ("source_revision".into(), Value::String(source_revision)),
            ("source_present".into(), Value::Bool(source.is_some())),
            (
                "source_mode".into(),
                mode.map(|v| Value::integer(&v.to_string()))
                    .transpose()?
                    .unwrap_or(Value::Null),
            ),
            (
                "source_length".into(),
                Value::integer(&source.as_ref().map_or(0, Vec::len).to_string())?,
            ),
            (
                "source_digest".into(),
                Value::String(canonical::digest(source.as_deref().unwrap_or(b""))),
            ),
            (
                "baseline_digest".into(),
                Value::String(canonical::digest(&frame)),
            ),
        ])?;
        let manifest = encoded(&manifest_value)?;
        let marker = marker_for(&manifest_value, &manifest)?;
        Ok(Self {
            epoch: epoch.into(),
            source,
            mode,
            frame,
            manifest,
            marker,
        })
    }
}

// Domain-local I/O seam: no environment switches, CLI, or selected-ledger
// confirmation. Every successful marker publication carries its known candidate.
trait ActivationIo {
    fn source(&self, root: &Path) -> Result<Option<Vec<u8>>> {
        optional_bytes(root, "state/tasks.json")
    }
    fn id(&self) -> Result<String> {
        platform::random_uuid_hex()
    }
    fn mkdir(&self, path: &Path) -> Result<()> {
        Ok(platform::mkdir_private(path)?)
    }
    fn sync_dir(&self, path: &Path) -> Result<()> {
        Ok(platform::sync_directory(path)?)
    }
    fn sync_file(&self, path: &Path) -> Result<()> {
        let file = platform::open_readonly(path)?;
        require(file.metadata()?.is_file(), "unsafe_ledger_activation_type")?;
        Ok(file.sync_all()?)
    }
    fn candidate(&self, root: &Path, marker: Vec<u8>) -> Result<CandidateEpoch> {
        validate_epoch(root, marker)
    }
    fn publish(
        &self,
        guard: &WorkspaceGuard,
        role: ArtifactRole<'_>,
        raw: &[u8],
        mode: Option<u32>,
    ) -> Result<Publication> {
        publication::publish(guard, role, raw, mode)
    }
    fn rename(&self, source: &Path, destination: &Path) -> Result<()> {
        Ok(platform::rename_replace(source, destination)?)
    }
}
struct SystemIo;
impl ActivationIo for SystemIo {}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn observe_pending(root: &Path, progress: &mut ImportProgress) -> Result<bool> {
    let present = exists(&root.join(DIRECTORY))?;
    if present {
        progress.pending();
    }
    Ok(present)
}

fn gate(workspace: &Path, guard: &WorkspaceGuard) -> Result<()> {
    guard.validate_for(workspace)?;
    match execution::guard_authority_change(workspace, guard)? {
        execution::ReservationCheck::DisabledObserved
        | execution::ReservationCheck::ActiveConfirmedClear => Ok(()),
    }
}

fn sync(progress: &mut ImportProgress, action: impl FnOnce() -> Result<()>) -> Result<()> {
    action().inspect_err(|error| {
        if error.code() == "input_or_storage_unavailable" {
            progress.uncertain();
        }
    })
}

fn publish_pending(
    io: &impl ActivationIo,
    guard: &WorkspaceGuard,
    role: ArtifactRole<'_>,
    raw: &[u8],
    mode: Option<u32>,
    progress: &mut ImportProgress,
) -> Result<()> {
    let relative = publication::relative(role)?;
    let result = io.publish(guard, role, raw, mode)?;
    progress.linked(relative);
    match result {
        Publication::Synced => Ok(()),
        Publication::SyncUncertain => {
            progress.uncertain();
            Err(Error("pending_import_synchronization_uncertain"))
        }
    }
}

fn activate(
    io: &impl ActivationIo,
    guard: &WorkspaceGuard,
    marker: &[u8],
    candidate: CandidateEpoch,
) -> Result<ActivationReport> {
    let durability = match io.publish(guard, ArtifactRole::Marker, marker, None)? {
        Publication::Synced => ActivationDurability::Confirmed,
        Publication::SyncUncertain => ActivationDurability::SynchronizationUncertain,
    };
    // No fallible reread may erase knowledge of our marker's successful link.
    Ok(ActivationReport {
        view: candidate.view,
        durability,
    })
}

pub fn import_tasks(
    workspace: &Path,
    expected: &str,
    guard: &WorkspaceGuard,
) -> std::result::Result<ActivationReport, ImportFailure> {
    import_with(workspace, expected, guard, &SystemIo)
}

fn import_with(
    workspace: &Path,
    expected: &str,
    guard: &WorkspaceGuard,
    io: &impl ActivationIo,
) -> std::result::Result<ActivationReport, ImportFailure> {
    let mut progress = ImportProgress::NoNewArtifacts;
    let result = (|| {
        gate(workspace, guard)?;
        let root = guard.root();
        require(
            !exists(&root.join(MARKER))?,
            "active_or_pending_ledger_exists",
        )?;
        let pending = observe_pending(root, &mut progress)?;
        require(!pending, "active_or_pending_ledger_exists")?;
        // Even malformed/null authority is rejected by presence, never parsed.
        read_fs::managed(root, Path::new(MARKER), false)?;
        read_fs::managed(root, Path::new(DIRECTORY), true)?;
        handoff::TaskRevision::parse(expected)?;
        let source = io.source(root)?;
        // Validate records before CAS, matching the snapshot import oracle.
        TaskRecords::validate(
            source
                .as_deref()
                .map(|raw| values::parse(raw, Profile::Authority))
                .transpose()?
                .unwrap_or(Value::Array(Vec::new())),
        )?;
        require(
            snapshot_revision(source.as_deref()) == expected,
            "snapshot_revision_mismatch",
        )?;
        let source_path = read_fs::managed(root, Path::new("state/tasks.json"), false)?;
        let mode = platform::replacement_mode(&source_path)?;
        let plan = BaselinePlan::new(source, mode, &io.id()?, &io.id()?)?;
        let base = format!("{DIRECTORY}/{}", plan.epoch);
        for (relative, parent) in [
            (DIRECTORY.to_owned(), "state".to_owned()),
            (base.clone(), DIRECTORY.to_owned()),
            (format!("{base}/frames"), base.clone()),
        ] {
            let path = read_fs::managed(root, Path::new(&relative), true)?;
            io.mkdir(&path)?;
            // The mkdir is already observable, even when parent synchronization fails.
            progress.pending();
            sync(&mut progress, || {
                let parent = read_fs::managed(root, Path::new(&parent), true)?;
                io.sync_dir(&parent)
            })?;
        }
        if let Some(raw) = &plan.source {
            publish_pending(
                io,
                guard,
                ArtifactRole::Baseline { epoch: &plan.epoch },
                raw,
                plan.mode,
                &mut progress,
            )?;
        }
        publish_pending(
            io,
            guard,
            ArtifactRole::Frame {
                epoch: &plan.epoch,
                sequence: 0,
            },
            &plan.frame,
            None,
            &mut progress,
        )?;
        publish_pending(
            io,
            guard,
            ArtifactRole::Manifest { epoch: &plan.epoch },
            &plan.manifest,
            None,
            &mut progress,
        )?;
        let candidate = io.candidate(root, plan.marker.clone())?;
        require(
            candidate.view.sequence == 0
                && eq_text(get(&candidate.manifest, "source_revision")?, expected),
            "invalid_ledger_baseline",
        )?;
        sync(&mut progress, || io.sync_dir(root))?;
        activate(io, guard, &plan.marker, candidate)
    })();
    result.map_err(|cause| ImportFailure { cause, progress })
}

pub fn resume_import(
    workspace: &Path,
    expected: &str,
    guard: &WorkspaceGuard,
) -> std::result::Result<ActivationReport, ImportFailure> {
    resume_with(workspace, expected, guard, &SystemIo)
}

fn resume_with(
    workspace: &Path,
    expected: &str,
    guard: &WorkspaceGuard,
    io: &impl ActivationIo,
) -> std::result::Result<ActivationReport, ImportFailure> {
    let mut progress = ImportProgress::NoNewArtifacts;
    let result = (|| {
        gate(workspace, guard)?;
        let root = guard.root();
        handoff::TaskRevision::parse(expected)?;
        require(!exists(&root.join(MARKER))?, "no_resumable_pending_import")?;
        let pending = observe_pending(root, &mut progress)?;
        require(pending, "no_resumable_pending_import")?;
        read_fs::managed(root, Path::new(MARKER), false)?;
        let directory = read_fs::managed(root, Path::new(DIRECTORY), true)?;
        // Count every root entry, including inert-looking names, before selecting.
        let mut epochs = Vec::new();
        for (index, entry) in fs::read_dir(directory)?.enumerate() {
            require(
                index < MAX_DIRECTORY_ENTRIES,
                "unsupported_ledger_directory_profile",
            )?;
            let name = entry?
                .file_name()
                .into_string()
                .map_err(|_| Error("invalid_ledger_entry_name"))?;
            require(handoff::hex(&name, 32), "pending_import_requires_one_epoch")?;
            read_fs::managed(root, &Path::new(DIRECTORY).join(&name), true)?;
            epochs.push(name);
        }
        require(epochs.len() == 1, "pending_import_requires_one_epoch")?;
        let epoch = &epochs[0];
        let base = format!("{DIRECTORY}/{epoch}");
        let raw = bytes(root, &format!("{base}/manifest.json"))?;
        let manifest = values::parse(&raw, Profile::Authority)?;
        require(
            eq_text(get(&manifest, "epoch_id")?, epoch),
            "pending_epoch_identity_mismatch",
        )?;
        let marker = marker_for(&manifest, &raw)?;
        let candidate = io.candidate(root, marker.clone())?;
        let source = io.source(root)?;
        let present = matches!(
            get(&candidate.manifest, "source_present")?,
            Value::Bool(true)
        );
        let backup = if present {
            Some(bytes(root, &format!("{base}/baseline.tasks.bytes"))?)
        } else {
            None
        };
        require(
            candidate.view.sequence == 0
                && eq_text(get(&candidate.manifest, "source_revision")?, expected)
                && snapshot_revision(source.as_deref()) == expected
                && source == backup,
            "pending_import_source_changed_or_contains_mutations",
        )?;
        let mut files = vec![
            format!("{base}/manifest.json"),
            format!("{base}/frames/{:020}.frame", 0),
        ];
        if present {
            let relative = format!("{base}/baseline.tasks.bytes");
            let path = read_fs::managed(root, Path::new(&relative), false)?;
            require(
                platform::replacement_mode(&path)?.map(u64::from)
                    == Some(unsigned(get(&candidate.manifest, "source_mode")?)?),
                "pending_source_backup_mode_mismatch",
            )?;
            files.push(relative);
        }
        for relative in files {
            sync(&mut progress, || {
                let path = read_fs::managed(root, Path::new(&relative), false)?;
                io.sync_file(&path)
            })?;
        }
        for relative in [
            format!("{base}/frames"),
            base,
            DIRECTORY.to_owned(),
            "state".to_owned(),
        ] {
            sync(&mut progress, || {
                let path = read_fs::managed(root, Path::new(&relative), true)?;
                io.sync_dir(&path)
            })?;
        }
        sync(&mut progress, || io.sync_dir(root))?;
        activate(io, guard, &marker, candidate)
    })();
    result.map_err(|cause| ImportFailure { cause, progress })
}

pub fn abort_import(workspace: &Path, guard: &WorkspaceGuard) -> Result<ArchiveReport> {
    abort_with(workspace, guard, &SystemIo)
}

fn abort_with(
    workspace: &Path,
    guard: &WorkspaceGuard,
    io: &impl ActivationIo,
) -> Result<ArchiveReport> {
    guard.validate_for(workspace)?;
    let root = guard.root();
    require(!exists(&root.join(MARKER))?, "cannot_abort_active_ledger")?;
    read_fs::managed(root, Path::new(MARKER), false)?;
    let source = read_fs::managed(root, Path::new(DIRECTORY), true)?;
    require(exists(&source)?, "no_pending_import")?;
    let id = io.id()?;
    require(handoff::hex(&id, 32), "invalid_ledger_identity")?;
    let relative = format!("state/task-ledger-aborted-{id}");
    require(
        !exists(&root.join(&relative))?,
        "pending_archive_destination_exists",
    )?;
    let destination = read_fs::managed(root, Path::new(&relative), true)?;
    // Cooperating writers hold this guard. This is not hostile atomic noreplace.
    io.rename(&source, &destination)?;
    let state = root.join("state");
    let durability = if io.sync_dir(&state).is_ok() {
        ArchiveDurability::Confirmed
    } else {
        ArchiveDurability::SynchronizationUncertain
    };
    Ok(ArchiveReport {
        path: destination,
        durability,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        io::{Read, Seek, SeekFrom},
        os::unix::{
            fs::{MetadataExt, PermissionsExt, symlink},
            process::CommandExt,
        },
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    const RAW: &[u8] = b" [ { \"id\":\"t\", \"title\":\"T\", \"status\":\"pending\", \"extension\":[123456789012345678901234567890,-0.0,1e-7,1e20,\"\\u03bb\\ud83e\\udd8a\"] } ]\r\n";

    fn workspace(source: Option<&[u8]>) -> (tempfile::TempDir, WorkspaceGuard) {
        let root = tempfile::tempdir().unwrap();
        // Guard acquisition intentionally needs only a regular control file.
        fs::write(root.path().join("coven.json"), b"not parsed by activation").unwrap();
        let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap();
        if let Some(raw) = source {
            fs::write(root.path().join("state/tasks.json"), raw).unwrap();
            fs::set_permissions(
                root.path().join("state/tasks.json"),
                fs::Permissions::from_mode(0o640),
            )
            .unwrap();
        }
        (root, guard)
    }

    #[derive(Default)]
    struct TestIo {
        calls: RefCell<Vec<String>>,
        fail: Option<usize>,
        uncertain: Option<&'static str>,
        ids: Cell<usize>,
    }
    impl TestIo {
        fn step(&self, name: String) -> Result<()> {
            let index = self.calls.borrow().len();
            self.calls.borrow_mut().push(name);
            if self.fail == Some(index) {
                Err(Error("input_or_storage_unavailable"))
            } else {
                Ok(())
            }
        }
        fn publication_name(role: ArtifactRole<'_>) -> &'static str {
            match role {
                ArtifactRole::Marker => "marker",
                ArtifactRole::Manifest { .. } => "manifest",
                ArtifactRole::Baseline { .. } => "backup",
                ArtifactRole::Frame { .. } => "frame",
            }
        }
    }
    impl ActivationIo for TestIo {
        fn source(&self, root: &Path) -> Result<Option<Vec<u8>>> {
            self.step("source".into())?;
            SystemIo.source(root)
        }
        fn id(&self) -> Result<String> {
            self.step("id".into())?;
            let index = self.ids.get();
            self.ids.set(index + 1);
            Ok(if index == 0 { "a" } else { "b" }.repeat(32))
        }
        fn mkdir(&self, path: &Path) -> Result<()> {
            self.step(format!(
                "mkdir:{}",
                path.file_name().unwrap().to_str().unwrap()
            ))?;
            SystemIo.mkdir(path)
        }
        fn sync_dir(&self, path: &Path) -> Result<()> {
            self.step(format!(
                "sync-dir:{}",
                path.file_name().unwrap().to_str().unwrap()
            ))?;
            SystemIo.sync_dir(path)
        }
        fn sync_file(&self, path: &Path) -> Result<()> {
            self.step(format!(
                "sync-file:{}",
                path.file_name().unwrap().to_str().unwrap()
            ))?;
            SystemIo.sync_file(path)
        }
        fn candidate(&self, root: &Path, marker: Vec<u8>) -> Result<CandidateEpoch> {
            self.step("candidate".into())?;
            SystemIo.candidate(root, marker)
        }
        fn publish(
            &self,
            guard: &WorkspaceGuard,
            role: ArtifactRole<'_>,
            raw: &[u8],
            mode: Option<u32>,
        ) -> Result<Publication> {
            let name = Self::publication_name(role);
            self.step(format!("publish:{name}"))?;
            let result = SystemIo.publish(guard, role, raw, mode)?;
            if self.uncertain == Some(name) {
                Ok(Publication::SyncUncertain)
            } else {
                Ok(result)
            }
        }
        fn rename(&self, source: &Path, destination: &Path) -> Result<()> {
            self.step("rename".into())?;
            SystemIo.rename(source, destination)
        }
    }

    fn pending_fixture(source: Option<&[u8]>) -> (tempfile::TempDir, WorkspaceGuard, String) {
        let (root, guard) = workspace(source);
        let expected = snapshot_revision(source);
        let io = TestIo::default();
        let report = import_with(root.path(), &expected, &guard, &io).unwrap();
        assert_eq!(report.durability(), ActivationDurability::Confirmed);
        fs::remove_file(root.path().join(MARKER)).unwrap();
        (root, guard, expected)
    }

    fn pending_failure(failure: &ImportFailure, uncertain: bool) -> Option<&Path> {
        match failure.progress() {
            ImportProgress::Pending {
                last_published,
                synchronization_uncertain,
            } => {
                assert_eq!(*synchronization_uncertain, uncertain, "{failure:?}");
                if let Some(path) = last_published {
                    assert!(path.is_relative());
                }
                last_published.as_deref()
            }
            other => panic!("expected pending, got {other:?}"),
        }
    }

    struct MutatingCandidateIo {
        mutation: &'static str,
        validated_sequence: Cell<Option<usize>>,
        candidate_called: Cell<bool>,
    }
    impl ActivationIo for MutatingCandidateIo {
        fn candidate(&self, root: &Path, marker: Vec<u8>) -> Result<CandidateEpoch> {
            let marker_value = values::parse(&marker, Profile::Authority).unwrap();
            let epoch = string(get(&marker_value, "epoch_id").unwrap()).unwrap();
            let base = root.join(DIRECTORY).join(epoch);
            match self.mutation {
                "manifest" | "backup" | "frame" => {
                    let path = base.join(match self.mutation {
                        "manifest" => "manifest.json",
                        "backup" => "baseline.tasks.bytes",
                        _ => "frames/00000000000000000000.frame",
                    });
                    let mut raw = fs::read(&path).unwrap();
                    // Still JSON, but no longer the exact committed bytes.
                    raw.push(b' ');
                    fs::write(path, raw).unwrap();
                }
                "unknown" => fs::write(base.join("unexpected"), b"unrecognized").unwrap(),
                "valid-mutation" => {
                    let first = fs::read(base.join("frames/00000000000000000000.frame")).unwrap();
                    let initial = payload(&first).unwrap();
                    let workspace = string(get(&initial, "workspace_id").unwrap()).unwrap();
                    let before = string(get(&initial, "revision").unwrap()).unwrap();
                    let mut tasks = get(&initial, "tasks").unwrap().clone();
                    let task = &mut tasks.array_mut().unwrap()[0];
                    task.set_field("owner", Value::String("worker".into()))
                        .unwrap();
                    task.set_field("status", Value::String("assigned".into()))
                        .unwrap();
                    task.set_field("updated_at", Value::String("then".into()))
                        .unwrap();
                    let task = task.clone();
                    let binding = RequestBinding::assign("t", "worker", Some(before)).unwrap();
                    let result = Value::object(vec![
                        ("outcome".into(), Value::String("committed".into())),
                        ("mutation_id".into(), Value::String("c".repeat(32))),
                        ("task".into(), task),
                        ("before_revision".into(), Value::String(before.into())),
                        (
                            "after_revision".into(),
                            Value::String(revision(workspace, epoch, 1, &tasks).unwrap()),
                        ),
                        ("sequence".into(), Value::integer("1").unwrap()),
                    ])
                    .unwrap();
                    let mutation = Value::object(vec![
                        ("version".into(), Value::integer("1").unwrap()),
                        ("workspace_id".into(), Value::String(workspace.into())),
                        ("epoch_id".into(), Value::String(epoch.into())),
                        ("sequence".into(), Value::integer("1").unwrap()),
                        (
                            "previous_digest".into(),
                            Value::String(canonical::digest(&first)),
                        ),
                        (
                            "request_id".into(),
                            Value::String("injected-assignment".into()),
                        ),
                        (
                            "binding".into(),
                            values::parse(&binding.0, Profile::Authority).unwrap(),
                        ),
                        ("result".into(), result),
                    ])
                    .unwrap();
                    let raw = mutation.canonical().unwrap();
                    let envelope = Value::object(vec![
                        ("version".into(), Value::integer("1").unwrap()),
                        (
                            "length".into(),
                            Value::integer(&raw.len().to_string()).unwrap(),
                        ),
                        ("sha256".into(), Value::String(canonical::digest(&raw))),
                        ("payload".into(), mutation),
                    ])
                    .unwrap();
                    fs::write(
                        base.join("frames/00000000000000000001.frame"),
                        encoded(&envelope).unwrap(),
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
            self.candidate_called.set(true);
            let candidate = SystemIo.candidate(root, marker)?;
            self.validated_sequence.set(Some(candidate.view.sequence));
            Ok(candidate)
        }
    }

    #[test]
    fn real_pre_marker_validation_rejects_changed_artifacts_and_valid_mutation_tail() {
        for (mutation, expected_error) in [
            ("manifest", "ledger_manifest_digest_mismatch"),
            ("backup", "ledger_source_backup_mismatch"),
            ("frame", "ledger_baseline_digest_mismatch"),
            ("unknown", "unexpected_ledger_epoch_entries"),
            ("valid-mutation", "invalid_ledger_baseline"),
        ] {
            let (root, guard) = workspace(Some(RAW));
            let source = root.path().join("state/tasks.json");
            let before = fs::metadata(&source).unwrap();
            let io = MutatingCandidateIo {
                mutation,
                validated_sequence: Cell::new(None),
                candidate_called: Cell::new(false),
            };
            let failure =
                import_with(root.path(), &snapshot_revision(Some(RAW)), &guard, &io).unwrap_err();
            assert!(io.candidate_called.get(), "{mutation}");
            assert_eq!(failure.code(), expected_error, "{mutation}");
            let last = pending_failure(&failure, false).unwrap();
            assert!(last.ends_with("manifest.json"));
            assert!(!root.path().join(MARKER).exists());
            assert!(read(root.path()).unwrap().is_none());
            assert_eq!(fs::read(&source).unwrap(), RAW);
            let after = fs::metadata(source).unwrap();
            assert_eq!(
                (before.dev(), before.ino(), before.mode()),
                (after.dev(), after.ino(), after.mode())
            );
            // A valid mutation passes full replay, but must not be activated by
            // first import's separate sequence-zero admission gate.
            assert_eq!(
                io.validated_sequence.get(),
                if mutation == "valid-mutation" {
                    Some(1)
                } else {
                    None
                }
            );
        }
    }

    #[test]
    fn first_import_preserves_source_and_exposes_only_selected_valid_view() {
        for source in [None, Some(RAW)] {
            let (root, guard) = workspace(source);
            let source_path = root.path().join("state/tasks.json");
            let before = fs::metadata(&source_path).ok();
            let report = import_tasks(root.path(), &snapshot_revision(source), &guard).unwrap();
            assert_eq!(report.durability(), ActivationDurability::Confirmed);
            let view = report.into_view();
            assert_eq!(view.sequence, 0);
            assert_eq!(view.projection_bytes().unwrap(), source.unwrap_or(b"[]\n"));
            assert_eq!(
                read(root.path()).unwrap().unwrap().revision(),
                view.revision()
            );
            match before {
                Some(before) => {
                    let after = fs::metadata(&source_path).unwrap();
                    assert_eq!(
                        (
                            before.ino(),
                            before.mode(),
                            before.mtime(),
                            before.mtime_nsec()
                        ),
                        (after.ino(), after.mode(), after.mtime(), after.mtime_nsec())
                    );
                    assert_eq!(fs::read(source_path).unwrap(), source.unwrap());
                }
                None => assert!(!source_path.exists()),
            }
            for secondary in ["events.jsonl", "dashboard.md", "state/execution"] {
                assert!(!root.path().join(secondary).exists());
            }
        }
    }

    #[test]
    fn every_first_import_io_failure_stops_before_marker_and_tracks_local_progress() {
        for source in [None, Some(RAW)] {
            let (root, guard) = workspace(source);
            let expected = snapshot_revision(source);
            let trace = TestIo::default();
            let _report = import_with(root.path(), &expected, &guard, &trace).unwrap();
            let calls = trace.calls.into_inner();
            assert_eq!(calls.last().unwrap(), "publish:marker");
            for (fail, failed_call) in calls.iter().enumerate() {
                let (root, guard) = workspace(source);
                let io = TestIo {
                    fail: Some(fail),
                    ..Default::default()
                };
                let error = import_with(root.path(), &expected, &guard, &io).unwrap_err();
                assert!(!root.path().join(MARKER).exists(), "{fail}: {calls:?}");
                assert!(read(root.path()).unwrap().is_none());
                assert_eq!(io.calls.borrow().len(), fail + 1);
                let created = calls[..fail].iter().any(|call| call.starts_with("mkdir:"));
                if created {
                    let last = pending_failure(&error, failed_call.starts_with("sync-dir:"));
                    let expected_role = calls[..fail]
                        .iter()
                        .rev()
                        .find(|call| call.starts_with("publish:"));
                    match expected_role.map(String::as_str) {
                        None => assert!(last.is_none()),
                        Some("publish:backup") => {
                            assert!(last.unwrap().ends_with("baseline.tasks.bytes"))
                        }
                        Some("publish:frame") => {
                            assert!(last.unwrap().ends_with("00000000000000000000.frame"))
                        }
                        Some("publish:manifest") => {
                            assert!(last.unwrap().ends_with("manifest.json"))
                        }
                        other => panic!("unexpected publication {other:?}"),
                    }
                } else {
                    assert_eq!(error.progress(), &ImportProgress::NoNewArtifacts);
                }
            }
        }
    }

    #[test]
    fn linked_artifact_uncertainty_stops_but_marker_uncertainty_returns_known_view() {
        for name in ["backup", "frame", "manifest", "marker"] {
            let (root, guard) = workspace(Some(RAW));
            let io = TestIo {
                uncertain: Some(name),
                ..Default::default()
            };
            let result = import_with(root.path(), &snapshot_revision(Some(RAW)), &guard, &io);
            if name == "marker" {
                let report = result.unwrap();
                assert_eq!(
                    report.durability(),
                    ActivationDurability::SynchronizationUncertain
                );
                assert_eq!(io.calls.borrow().last().unwrap(), "publish:marker");
                // Removing selection after return cannot invalidate the known report.
                fs::remove_file(root.path().join(MARKER)).unwrap();
                assert_eq!(report.view().projection_bytes().unwrap(), RAW);
                assert!(!format!("{report:?}").contains("extension"));
            } else {
                let failure = result.unwrap_err();
                assert!(pending_failure(&failure, true).is_some());
                assert!(!root.path().join(MARKER).exists());
                assert_eq!(
                    io.calls.borrow().last().unwrap(),
                    &format!("publish:{name}")
                );
            }
        }
    }

    #[test]
    fn existing_pending_even_incomplete_or_unsafe_is_reported_as_observed() {
        for kind in ["directory", "file", "symlink"] {
            let (root, guard) = workspace(None);
            let path = root.path().join(DIRECTORY);
            match kind {
                "directory" => fs::create_dir(&path).unwrap(),
                "file" => fs::write(&path, b"bad").unwrap(),
                _ => symlink("missing", &path).unwrap(),
            }
            let error = import_tasks(root.path(), "tasks-missing:v1", &guard).unwrap_err();
            pending_failure(&error, false);
            let error = resume_import(root.path(), "tasks-missing:v1", &guard).unwrap_err();
            pending_failure(&error, false);
        }
        for raw in [b"null".as_slice(), b"corrupt"] {
            let (root, guard) = workspace(None);
            fs::write(root.path().join(MARKER), raw).unwrap();
            assert_eq!(
                import_tasks(root.path(), "tasks-missing:v1", &guard)
                    .unwrap_err()
                    .code(),
                "active_or_pending_ledger_exists"
            );
            assert!(abort_import(root.path(), &guard).is_err());
        }
    }

    #[test]
    fn any_existing_marker_is_rejected_without_classifying_its_epoch_as_pending() {
        for with_epoch in [false, true] {
            for kind in ["null", "directory", "symlink"] {
                let (root, guard) = workspace(None);
                if with_epoch {
                    fs::create_dir(root.path().join(DIRECTORY)).unwrap();
                }
                let marker = root.path().join(MARKER);
                match kind {
                    "null" => fs::write(&marker, b"null").unwrap(),
                    "directory" => fs::create_dir(&marker).unwrap(),
                    _ => symlink("missing", &marker).unwrap(),
                }
                let imported = import_tasks(root.path(), "tasks-missing:v1", &guard).unwrap_err();
                assert_eq!(imported.code(), "active_or_pending_ledger_exists");
                assert_eq!(imported.progress(), &ImportProgress::NoNewArtifacts);
                let resumed = resume_import(root.path(), "tasks-missing:v1", &guard).unwrap_err();
                assert_eq!(resumed.code(), "no_resumable_pending_import");
                assert_eq!(resumed.progress(), &ImportProgress::NoNewArtifacts);
                assert!(abort_import(root.path(), &guard).is_err());
            }
        }
    }

    #[test]
    fn every_resume_io_failure_leaves_pending_and_syncs_before_marker() {
        for source in [None, Some(RAW)] {
            let (root, guard, expected) = pending_fixture(source);
            let io = TestIo::default();
            let _report = resume_with(root.path(), &expected, &guard, &io).unwrap();
            let calls = io.calls.into_inner();
            assert_eq!(calls.first().unwrap(), "candidate");
            assert_eq!(calls.last().unwrap(), "publish:marker");
            let syncs = calls
                .iter()
                .filter(|s| s.starts_with("sync-file:"))
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(syncs[0], "sync-file:manifest.json");
            assert_eq!(syncs[1], "sync-file:00000000000000000000.frame");
            assert_eq!(syncs.len(), if source.is_some() { 3 } else { 2 });
            for (fail, failed_call) in calls.iter().enumerate() {
                let (root, guard, expected) = pending_fixture(source);
                let io = TestIo {
                    fail: Some(fail),
                    ..Default::default()
                };
                let failure = resume_with(root.path(), &expected, &guard, &io).unwrap_err();
                assert!(!root.path().join(MARKER).exists());
                assert!(pending_failure(&failure, failed_call.starts_with("sync-")).is_none());
                assert_eq!(io.calls.borrow().len(), fail + 1);
            }
        }
    }

    #[test]
    fn resume_preserves_raw_manifest_and_accepts_only_current_source_chmod() {
        let (root, guard, expected) = pending_fixture(Some(RAW));
        let base = root.path().join(DIRECTORY).join("b".repeat(32));
        let manifest_path = base.join("manifest.json");
        let mut raw = b" \n".to_vec();
        raw.extend(fs::read(&manifest_path).unwrap());
        raw.extend(b" \r\n");
        fs::write(&manifest_path, &raw).unwrap();
        let source_path = root.path().join("state/tasks.json");
        fs::set_permissions(&source_path, fs::Permissions::from_mode(0o600)).unwrap();
        let before = fs::metadata(&source_path).unwrap();
        let io = TestIo {
            uncertain: Some("marker"),
            ..Default::default()
        };
        let report = resume_with(root.path(), &expected, &guard, &io).unwrap();
        assert_eq!(
            report.durability(),
            ActivationDurability::SynchronizationUncertain
        );
        assert_eq!(report.view.manifest_digest, canonical::digest(&raw));
        assert_eq!(fs::read(manifest_path).unwrap(), raw);
        assert_eq!(fs::metadata(source_path).unwrap().ino(), before.ino());
        report.view().fence(root.path()).unwrap();
    }

    #[test]
    fn resume_refuses_incomplete_ambiguous_changed_or_unsafe_pending_without_regeneration() {
        for case in [
            "manifest",
            "frame",
            "backup",
            "mode",
            "source",
            "presence",
            "epoch",
            "unknown",
            "unsafe-temp",
            "mutation",
            "root-temp",
        ] {
            let (root, guard, expected) = pending_fixture(Some(RAW));
            let base = root.path().join(DIRECTORY).join("b".repeat(32));
            match case {
                "manifest" => fs::remove_file(base.join("manifest.json")).unwrap(),
                "frame" => fs::remove_file(base.join("frames/00000000000000000000.frame")).unwrap(),
                "backup" => fs::remove_file(base.join("baseline.tasks.bytes")).unwrap(),
                "mode" => fs::set_permissions(
                    base.join("baseline.tasks.bytes"),
                    fs::Permissions::from_mode(0o600),
                )
                .unwrap(),
                "source" => fs::write(root.path().join("state/tasks.json"), b"[]").unwrap(),
                "presence" => fs::remove_file(root.path().join("state/tasks.json")).unwrap(),
                "epoch" => {
                    fs::create_dir(root.path().join(DIRECTORY).join("c".repeat(32))).unwrap()
                }
                "unknown" => fs::write(base.join("unexpected"), b"x").unwrap(),
                "unsafe-temp" => symlink("missing", base.join(".manifest.json.X.tmp")).unwrap(),
                "mutation" => {
                    fs::write(base.join("frames/00000000000000000001.frame"), b"corrupt").unwrap()
                }
                "root-temp" => fs::write(
                    root.path().join(DIRECTORY).join(".manifest.json.X.tmp"),
                    b"inert",
                )
                .unwrap(),
                _ => unreachable!(),
            }
            let io = TestIo::default();
            let failure = resume_with(root.path(), &expected, &guard, &io).unwrap_err();
            pending_failure(&failure, false);
            assert!(!root.path().join(MARKER).exists(), "{case}");
            assert!(
                !io.calls.borrow().iter().any(|c| c.starts_with("publish:")
                    || c.starts_with("mkdir:")
                    || c.starts_with("sync-")),
                "{case}"
            );
        }
    }

    #[test]
    fn abort_ignores_corrupt_source_execution_and_epoch_and_preserves_known_rename() {
        for fail in [None, Some(0), Some(1), Some(2)] {
            let (root, guard) = workspace(Some(b"corrupt source"));
            let pending = root.path().join(DIRECTORY);
            fs::create_dir(&pending).unwrap();
            fs::write(pending.join("junk"), b"preserve").unwrap();
            fs::set_permissions(pending.join("junk"), fs::Permissions::from_mode(0o671)).unwrap();
            fs::create_dir(root.path().join("state/execution")).unwrap();
            fs::write(root.path().join("state/execution/control.json"), b"corrupt").unwrap();
            let io = TestIo {
                fail,
                ..Default::default()
            };
            let result = abort_with(root.path(), &guard, &io);
            if matches!(fail, Some(0 | 1)) {
                assert!(result.is_err());
                assert!(pending.exists());
            } else {
                let report = result.unwrap();
                assert!(!pending.exists());
                assert_eq!(fs::read(report.path().join("junk")).unwrap(), b"preserve");
                assert_eq!(
                    fs::metadata(report.path().join("junk")).unwrap().mode() & 0o7777,
                    0o671
                );
                assert_eq!(
                    report.durability(),
                    if fail.is_some() {
                        ArchiveDurability::SynchronizationUncertain
                    } else {
                        ArchiveDurability::Confirmed
                    }
                );
            }
            assert_eq!(
                fs::read(root.path().join("state/tasks.json")).unwrap(),
                b"corrupt source"
            );
        }
    }

    #[test]
    fn abort_never_replaces_any_existing_archive_destination() {
        for kind in ["dir", "file", "symlink"] {
            let (root, guard) = workspace(None);
            fs::create_dir(root.path().join(DIRECTORY)).unwrap();
            let destination = root
                .path()
                .join(format!("state/task-ledger-aborted-{}", "a".repeat(32)));
            match kind {
                "dir" => fs::create_dir(&destination).unwrap(),
                "file" => fs::write(&destination, b"existing").unwrap(),
                _ => symlink("missing", &destination).unwrap(),
            }
            assert_eq!(
                abort_with(root.path(), &guard, &TestIo::default())
                    .unwrap_err()
                    .code(),
                "pending_archive_destination_exists"
            );
            assert!(root.path().join(DIRECTORY).exists());
        }
    }

    #[test]
    fn guard_execution_and_pin_failures_precede_all_activation_io() {
        let (root, guard) = workspace(None);
        let (other, _other_guard) = workspace(None);
        for resume in [false, true] {
            let io = TestIo::default();
            let error = if resume {
                resume_with(other.path(), "invalid", &guard, &io)
            } else {
                import_with(other.path(), "invalid", &guard, &io)
            }
            .unwrap_err();
            assert_eq!(error.code(), "invalid_workspace_lock_owner");
            assert!(io.calls.borrow().is_empty());
            let io = TestIo::default();
            let error = if resume {
                resume_with(root.path(), "invalid", &guard, &io)
            } else {
                import_with(root.path(), "invalid", &guard, &io)
            }
            .unwrap_err();
            assert_eq!(error.progress(), &ImportProgress::NoNewArtifacts);
            assert!(io.calls.borrow().is_empty());
        }
        fs::create_dir(root.path().join("state/execution")).unwrap();
        let io = TestIo::default();
        assert_eq!(
            import_with(root.path(), "tasks-missing:v1", &guard, &io)
                .unwrap_err()
                .code(),
            "pending_execution_authority"
        );
        assert_eq!(
            resume_with(root.path(), "tasks-missing:v1", &guard, &io)
                .unwrap_err()
                .code(),
            "pending_execution_authority"
        );
        assert!(io.calls.borrow().is_empty());
        assert!(!root.path().join(DIRECTORY).exists());
    }

    #[test]
    fn wrapped_depth_and_node_limits_are_admitted_before_first_mkdir() {
        let nested = format!(
            "[{{\"id\":\"t\",\"title\":\"T\",\"status\":\"pending\",\"ext\":{}0{}}}]",
            "[".repeat(62),
            "]".repeat(62)
        );
        // Four separately bounded extension arrays exhaust nodes, not item count.
        let dense = format!(
            "[{{\"id\":\"t\",\"title\":\"T\",\"status\":\"pending\",\"ext\":[[{}],[{}],[{}],[{}]]}}]",
            vec!["0"; 65_533].join(","),
            vec!["0"; 65_533].join(","),
            vec!["0"; 65_533].join(","),
            vec!["0"; 65_533].join(",")
        );
        for (raw, expected_code) in [
            (nested.as_bytes(), "unsupported_json_depth_profile"),
            (dense.as_bytes(), "unsupported_json_node_profile"),
        ] {
            let parsed = values::parse(raw, Profile::Authority).unwrap();
            TaskRecords::validate(parsed).unwrap();
            let (root, guard) = workspace(Some(raw));
            let io = TestIo::default();
            let failure =
                import_with(root.path(), &snapshot_revision(Some(raw)), &guard, &io).unwrap_err();
            assert_eq!(failure.code(), expected_code);
            assert_eq!(failure.progress(), &ImportProgress::NoNewArtifacts);
            assert!(!root.path().join(DIRECTORY).exists());
            assert!(!io.calls.borrow().iter().any(|s| s.starts_with("mkdir:")));
        }
    }

    struct FixtureChild(Option<Child>);
    impl Drop for FixtureChild {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                // SAFETY: this child owns a separate process group.
                unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn python(script: &str, args: &[String]) -> String {
        let mut stdout = tempfile::tempfile().unwrap();
        let mut stderr = tempfile::tempfile().unwrap();
        let mut child = FixtureChild(Some(
            Command::new("python3")
                .args(["-c", script])
                .args(args)
                .env("PYTHONDONTWRITEBYTECODE", "1")
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .process_group(0)
                .stdout(Stdio::from(stdout.try_clone().unwrap()))
                .stderr(Stdio::from(stderr.try_clone().unwrap()))
                .spawn()
                .unwrap(),
        ));
        let deadline = Instant::now() + Duration::from_secs(30);
        let status = loop {
            if let Some(status) = child.0.as_mut().unwrap().try_wait().unwrap() {
                child.0.take();
                break Some(status);
            }
            if Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        drop(child);
        stdout.seek(SeekFrom::Start(0)).unwrap();
        stderr.seek(SeekFrom::Start(0)).unwrap();
        let mut out = String::new();
        let mut err = String::new();
        stdout.read_to_string(&mut out).unwrap();
        stderr.read_to_string(&mut err).unwrap();
        assert!(
            status.is_some_and(|s| s.success()),
            "Python fixture timeout/failure: {err}"
        );
        out
    }

    #[test]
    fn live_preparations_still_block_when_source_stale_or_graph_revoked() {
        for state in ["prepared", "stale", "revoked", "clear"] {
            for pending in [false, true] {
                let (root, guard) = if pending {
                    let (root, guard, _) = pending_fixture(Some(RAW));
                    (root, guard)
                } else {
                    workspace(Some(RAW))
                };
                drop(guard);
                python(
                    r#"
import sys
from pathlib import Path
sys.path.insert(0, 'tests')
from _authority_confirmation import execution_call, action, intent
from runner import task_attempts as execution
root, state = Path(sys.argv[1]), sys.argv[2]
execution_call(root, 'enable', {})
execution_call(root, 'enroll', {'task_id':'a','rebind_source':False})
if state == 'revoked':
    execution_call(root, 'upgrade', {})
    node = {k:v for k,v in intent().items() if k in ('task_id','owner','acceptance_criteria')}
    node['depends_on'] = []
    execution_call(root, 'graph_propose', {'document':{'schema_version':1,'kind':'task-graph','nodes':[node]}})
    def decide(decision):
        view = execution.Store(root).read()
        execution_call(root, 'graph_decide', {'graph_revision':view.current_graph_revision,'decision':decision,'reason':'fixture','expected_graph_digest':view.graphs[view.current_graph_revision]['digest']})
    decide('approve')
    execution_call(root, 'prepare_graph', {'task_id':'a','graph_revision':execution.Store(root).read().current_graph_revision})
    decide('changes-requested')
elif state != 'clear':
    action(root, 'prepare')
    if state == 'stale':
        (root / 'state/tasks.json').write_bytes(b'[]\n')
"#,
                    &[root.path().to_str().unwrap().into(), state.into()],
                );
                let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
                let expected = snapshot_revision(Some(RAW));
                let io = TestIo::default();
                let result = if pending {
                    resume_with(root.path(), &expected, &guard, &io)
                } else {
                    import_with(root.path(), &expected, &guard, &io)
                };
                if state == "clear" {
                    assert_eq!(
                        result.unwrap().durability(),
                        ActivationDurability::Confirmed
                    );
                } else {
                    assert_eq!(
                        result.unwrap_err().code(),
                        "prepared_attempt_blocks_mutation"
                    );
                    assert!(io.calls.borrow().is_empty());
                    assert!(!root.path().join(MARKER).exists());
                    assert_eq!(root.path().join(DIRECTORY).exists(), pending);
                }
            }
        }
    }

    #[test]
    fn fixed_identity_plan_matches_python_bytes_including_extensions_and_raw_backup() {
        for source in [None, Some(b" [ ] \r\n".as_slice()), Some(RAW)] {
            let plan = BaselinePlan::new(
                source.map(<[u8]>::to_vec),
                source.map(|_| 0o640),
                &"a".repeat(32),
                &"b".repeat(32),
            )
            .unwrap();
            let out = python(
                r#"
import sys
from runner import task_ledger as l, storage
source = bytes.fromhex(sys.argv[1]) if sys.argv[2] == 'true' else None
tasks = l._tasks(l._json(source) if source is not None else [])
w, e = 'a'*32, 'b'*32
revision = storage.snapshot_revision(source)
frame = l._envelope(dict(version=1, workspace_id=w, epoch_id=e, sequence=0, previous_digest=None, source_revision=revision, tasks=tasks, revision=l._revision(w,e,0,tasks)))
manifest = dict(version=1,workspace_id=w,epoch_id=e,source_revision=revision,source_present=source is not None,source_mode=0o640 if source is not None else None,source_length=len(source or b''),source_digest=l._digest(source or b''),baseline_digest=l._digest(frame))
raw = l._canonical(manifest)+b'\n'
marker = {k:manifest[k] for k in ('version','workspace_id','epoch_id','baseline_digest')}
marker['manifest_digest'] = l._digest(raw)
for item in (frame, raw, l._canonical(marker)+b'\n'):
    print(item.hex())
"#,
                &[
                    source
                        .unwrap_or(b"")
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>(),
                    source.is_some().to_string(),
                ],
            );
            let actual = [&plan.frame, &plan.manifest, &plan.marker]
                .iter()
                .map(|raw| raw.iter().map(|b| format!("{b:02x}")).collect::<String>())
                .collect::<Vec<_>>();
            assert_eq!(out.lines().collect::<Vec<_>>(), actual);
            assert_eq!(plan.source.as_deref(), source);
        }
        assert!(BaselinePlan::new(None, None, "bad", &"b".repeat(32)).is_err());
    }
}
