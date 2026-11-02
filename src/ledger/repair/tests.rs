use super::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
    os::unix::{
        fs::{MetadataExt, PermissionsExt, symlink},
        process::CommandExt,
    },
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct FixtureChild(Option<Child>);
impl Drop for FixtureChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            // SAFETY: this owned fixture has its own process group.
            unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let mut out = tempfile::tempfile().unwrap();
    let mut err = tempfile::tempfile().unwrap();
    let mut child = FixtureChild(Some(Command::new("python3").args(["-c", r#"
import contextlib, io, sys
from pathlib import Path
from runner import coven
root = Path(sys.argv[1])
def invoke(*args):
    with contextlib.redirect_stdout(io.StringIO()):
        status = coven.main(list(map(str, args)))
    assert status == 0, (args, status)
invoke('init', root, '--blank', '--agents', 'operator')
invoke('state', 'import-tasks', root, '--expect-revision', coven.read_task_snapshot(root)[0], '--json')
invoke('task', 'add', root, 'Secret task title', '--request-id', 'task-1', '--json')
"#]).arg(root.path()).env("PYTHONDONTWRITEBYTECODE", "1").env("COVEN_MULTIPLEXER", "")
        .current_dir(env!("CARGO_MANIFEST_DIR")).process_group(0)
        .stdout(Stdio::from(out.try_clone().unwrap())).stderr(Stdio::from(err.try_clone().unwrap())).spawn().unwrap()));
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.0.as_mut().unwrap().try_wait().unwrap() {
            child.0.take();
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    drop(child);
    out.seek(SeekFrom::Start(0)).unwrap();
    err.seek(SeekFrom::Start(0)).unwrap();
    let mut stdout = String::new();
    let mut stderr = String::new();
    out.read_to_string(&mut stdout).unwrap();
    err.read_to_string(&mut stderr).unwrap();
    assert!(
        status.is_some(),
        "Python fixture timeout\n{stdout}\n{stderr}"
    );
    assert!(status.unwrap().success(), "{stdout}\n{stderr}");
    root
}

// Ignore atime only: even read-only confirmation may update it.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    identity: (u32, u64, u64, u32, u32),
    namespace: (u64, u64, i64, i64, i64, i64),
    bytes: Vec<u8>,
    symlink: Option<PathBuf>,
}
fn inventory(root: &Path) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, path: &Path, entries: &mut BTreeMap<PathBuf, Entry>) {
        let m = fs::symlink_metadata(path).unwrap();
        entries.insert(
            path.strip_prefix(root).unwrap().to_owned(),
            Entry {
                identity: (m.mode(), m.dev(), m.ino(), m.uid(), m.gid()),
                namespace: (
                    m.nlink(),
                    m.len(),
                    m.mtime(),
                    m.mtime_nsec(),
                    m.ctime(),
                    m.ctime_nsec(),
                ),
                bytes: if m.is_file() {
                    fs::read(path).unwrap()
                } else {
                    Vec::new()
                },
                symlink: m.is_symlink().then(|| fs::read_link(path).unwrap()),
            },
        );
        if m.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), entries);
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

const ID: &str = "0123456789abcdef0123456789abcdef";
struct Spy {
    trace: RefCell<Vec<&'static str>>,
    fault: Option<usize>,
    reloads: Cell<usize>,
    change: &'static str,
}
impl Spy {
    fn new(fault: Option<usize>, change: &'static str) -> Self {
        Self {
            trace: RefCell::new(Vec::new()),
            fault,
            reloads: Cell::new(0),
            change,
        }
    }
    fn hit(&self, event: &'static str) -> io::Result<()> {
        let mut trace = self.trace.borrow_mut();
        let index = trace.len();
        trace.push(event);
        if self.fault == Some(index) {
            Err(io::Error::other(event))
        } else {
            Ok(())
        }
    }
}
impl RepairIo for Spy {
    fn reload(&self, root: &Path) -> Result<Option<View>> {
        self.hit("reload")?;
        let mut view = SystemIo.reload(root)?.unwrap();
        let count = self.reloads.get();
        self.reloads.set(count + 1);
        if count == 1 {
            match self.change {
                "revision" => view.revision.push('x'),
                "digest" => view.head_digest.push('x'),
                "raw" => fs::write(root.join(PROJECTION), b"unbacked edit").unwrap(),
                "missing" => fs::remove_file(root.join(PROJECTION)).unwrap(),
                "symlink" => {
                    fs::remove_file(root.join(PROJECTION)).unwrap();
                    symlink(root.join("coven.json"), root.join(PROJECTION)).unwrap();
                }
                _ => {}
            }
        }
        Ok(Some(view))
    }
    fn confirm(
        &self,
        root: &Path,
        view: &View,
        guard: &WorkspaceGuard,
    ) -> Result<LedgerConfirmation> {
        self.hit("confirm")?;
        if self.change == "authority_uncertain" {
            return Ok(LedgerConfirmation::SynchronizationUncertain);
        }
        SystemIo.confirm(root, view, guard)
    }
    fn capture(&self, root: &Path) -> Result<Option<Vec<u8>>> {
        self.hit("capture")?;
        SystemIo.capture(root)
    }
    fn id(&self) -> Result<String> {
        self.hit("id")?;
        Ok(ID.into())
    }
    fn mkdir(&self, path: &Path) -> io::Result<()> {
        self.hit("mkdir")?;
        SystemIo.mkdir(path)
    }
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        self.hit("dirsync")?;
        SystemIo.sync_dir(path)
    }
    fn temporary(&self, parent: &Path) -> io::Result<tempfile::NamedTempFile> {
        self.hit("temporary")?;
        SystemIo.temporary(parent)
    }
    fn write(&self, file: &mut File, raw: &[u8]) -> io::Result<()> {
        file.write_all(&raw[..raw.len() / 2])?;
        self.hit("write")?;
        file.write_all(&raw[raw.len() / 2..])
    }
    fn apply_mode(&self, file: &File) -> io::Result<()> {
        self.hit("mode")?;
        SystemIo.apply_mode(file)
    }
    fn verify_mode(&self, file: &File) -> io::Result<()> {
        self.hit("verify_mode")?;
        SystemIo.verify_mode(file)
    }
    fn sync_file(&self, file: &File) -> io::Result<()> {
        self.hit("filesync")?;
        SystemIo.sync_file(file)
    }
    fn recheck(&self, root: &Path, relative: &Path) -> Result<PathBuf> {
        self.hit("recheck")?;
        SystemIo.recheck(root, relative)
    }
    fn link(&self, source: &Path, target: &Path) -> io::Result<()> {
        self.hit("link")?;
        SystemIo.link(source, target)
    }
    fn cleanup(&self, temp: tempfile::NamedTempFile) -> io::Result<()> {
        if self.hit("cleanup").is_err() {
            temp.keep().map_err(|e| e.error)?;
            return Err(io::Error::other("cleanup"));
        }
        SystemIo.cleanup(temp)
    }
    fn replace(&self, guard: &WorkspaceGuard, raw: &[u8]) -> Result<Delivery> {
        self.hit("replace")?;
        let result = SystemIo.replace(guard, raw)?;
        if self.change == "replacement_uncertain" {
            Ok(Delivery::DurabilityUncertain)
        } else {
            Ok(result)
        }
    }
}

#[test]
fn current_still_confirms_and_authority_uncertain_never_touches_projection() {
    let root = fixture();
    let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
    let spy = Spy::new(None, "");
    let report = repair_with(root.path(), &guard, &spy).unwrap();
    assert_eq!(report.status(), RepairStatus::Current);
    assert_eq!(report.durability(), RepairDurability::Confirmed);
    assert!(report.backup_path().is_none());
    assert_eq!(*spy.trace.borrow(), ["reload", "confirm", "capture"]);
    assert!(!root.path().join(RECOVERY).exists());
    assert!(!format!("{report:?}").contains("Secret task title"));
    fs::remove_file(root.path().join(PROJECTION)).unwrap();
    fs::create_dir(root.path().join(PROJECTION)).unwrap();
    let spy = Spy::new(None, "authority_uncertain");
    let report = repair_with(root.path(), &guard, &spy).unwrap();
    assert_eq!(report.status(), RepairStatus::AuthorityUnconfirmed);
    assert_eq!(
        report.durability(),
        RepairDurability::SynchronizationUncertain
    );
    assert!(report.backup_path().is_none());
    assert_eq!(*spy.trace.borrow(), ["reload", "confirm"]);
    let spy = Spy::new(Some(1), "");
    let failure = repair_with(root.path(), &guard, &spy).unwrap_err();
    assert_eq!(failure.phase(), RepairFailurePhase::Authority);
    assert!(failure.known_view().is_some());
    assert!(failure.backup_path().is_none());
    assert!(!format!("{failure:?}").contains("Secret task title"));
    assert_eq!(failure.to_string(), failure.code());
    assert!(std::error::Error::source(&failure).is_some());
}

#[test]
fn raw_backups_missing_empty_invalid_utf8_and_full_32m_preserve_exact_bytes_and_modes() {
    let root = fixture();
    let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
    let expected = read(root.path())
        .unwrap()
        .unwrap()
        .projection_bytes()
        .unwrap();
    // Current configuration and execution policy are deliberately not SDK gates.
    fs::write(root.path().join("coven.json"), b"invalid config").unwrap();
    fs::create_dir_all(root.path().join("state/execution")).unwrap();
    fs::write(
        root.path().join("state/execution/control.json"),
        b"invalid policy",
    )
    .unwrap();
    let excess_nodes = format!("[{}0]", "0,".repeat(values::MAX_NODES)).into_bytes();
    for raw in [
        None,
        Some(Vec::new()),
        Some(vec![0xff, 0, 0xfe]),
        Some(excess_nodes),
        Some(vec![0x80; MAX_RAW]),
    ] {
        fs::remove_file(root.path().join(PROJECTION)).unwrap();
        if let Some(raw) = &raw {
            fs::write(root.path().join(PROJECTION), raw).unwrap();
            fs::set_permissions(
                root.path().join(PROJECTION),
                fs::Permissions::from_mode(0o640),
            )
            .unwrap();
        }
        let report = repair_projection(root.path(), &guard).unwrap();
        assert_eq!(report.status(), RepairStatus::Repaired);
        assert_eq!(report.durability(), RepairDurability::Confirmed);
        let backup = report.backup_path().unwrap();
        assert!(backup.is_absolute());
        assert_eq!(
            fs::metadata(backup).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(fs::read(root.path().join(PROJECTION)).unwrap(), expected);
        assert_eq!(
            fs::metadata(root.path().join(PROJECTION))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            if raw.is_some() { 0o640 } else { 0o600 }
        );
        assert_eq!(backup.join("tasks.bytes").exists(), raw.is_some());
        if let Some(raw) = &raw {
            assert_eq!(fs::read(backup.join("tasks.bytes")).unwrap(), *raw);
        }
        let metadata = Value::object(vec![
            ("version".into(), Value::integer("1").unwrap()),
            ("missing".into(), Value::Bool(raw.is_none())),
            (
                "revision".into(),
                Value::String(snapshot_revision(raw.as_deref())),
            ),
        ])
        .unwrap()
        .storage()
        .unwrap();
        assert_eq!(fs::read(backup.join("metadata.json")).unwrap(), metadata);
        for entry in fs::read_dir(backup).unwrap() {
            assert_eq!(
                entry.unwrap().metadata().unwrap().permissions().mode() & 0o7777,
                0o600
            );
        }
        assert_eq!(report.into_view().projection_bytes().unwrap(), expected);
    }
}

#[test]
fn over_32m_raw_projection_is_a_recovery_profile_refusal_without_any_changes() {
    let root = fixture();
    let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
    let oversized = vec![0; MAX_RAW + 1];
    fs::write(root.path().join(PROJECTION), &oversized).unwrap();
    let before = inventory(root.path());
    let failure = repair_projection(root.path(), &guard).unwrap_err();
    assert_eq!(failure.phase(), RepairFailurePhase::ProjectionCapture);
    assert_eq!(failure.code(), "unsupported_recovery_size");
    assert!(failure.backup_path().is_none());
    assert!(!root.path().join(RECOVERY).exists());
    assert_eq!(fs::read(root.path().join(PROJECTION)).unwrap(), oversized);
    assert_eq!(inventory(root.path()), before);
}

#[test]
fn repair_changes_only_projection_and_new_recovery_artifacts() {
    let root = fixture();
    let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
    for relative in [
        "state/execution/control.json",
        "state/handoff.json",
        "state/handoffs/sentinel.json",
        "logs/events.jsonl",
        "logs/messages.jsonl",
        "dashboard.md",
    ] {
        let path = root.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"\xff\0sentinel\xfe\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    }
    fs::write(root.path().join("coven.json"), b"\xff\0config sentinel").unwrap();
    symlink("handoff.json", root.path().join("state/unrelated-link")).unwrap();
    let raw = b"\xff\0raw projection\xfe\n";
    fs::write(root.path().join(PROJECTION), raw).unwrap();
    let mut before = inventory(root.path());
    let report = repair_projection(root.path(), &guard).unwrap();
    assert_eq!(report.status(), RepairStatus::Repaired);
    assert_eq!(report.durability(), RepairDurability::Confirmed);
    let backup = report.backup_path().unwrap();
    assert_eq!(fs::read(backup.join("tasks.bytes")).unwrap(), raw);
    let base = backup.strip_prefix(root.path()).unwrap();
    let mut expected_new = [
        PathBuf::from(RECOVERY),
        base.to_owned(),
        base.join("tasks.bytes"),
        base.join("metadata.json"),
    ];
    expected_new.sort();
    let mut after = inventory(root.path());
    let actual_new: Vec<_> = after
        .keys()
        .filter(|path| !before.contains_key(*path))
        .cloned()
        .collect();
    assert_eq!(actual_new, expected_new);
    for path in expected_new {
        after.remove(&path).unwrap();
    }
    let replacement = after.remove(Path::new(PROJECTION)).unwrap();
    let original = before.remove(Path::new(PROJECTION)).unwrap();
    assert_eq!(replacement.bytes, report.view().projection_bytes().unwrap());
    assert_eq!(original.bytes, raw);
    assert_eq!(replacement.identity.0, original.identity.0);
    // Adding recovery and replacing tasks necessarily changes state directory
    // size/link count/timestamps, but not its inode, mode, ownership or contents.
    after.get_mut(Path::new("state")).unwrap().namespace = before[Path::new("state")].namespace;
    assert_eq!(after, before);
    let stable = inventory(root.path());
    assert_eq!(
        repair_projection(root.path(), &guard).unwrap().status(),
        RepairStatus::Current
    );
    assert_eq!(inventory(root.path()), stable);
}

#[test]
fn actual_valid_frame_appended_before_final_reload_retains_backup_and_projection() {
    struct Append {
        frame: PathBuf,
        raw: Vec<u8>,
        reloads: Cell<usize>,
    }
    impl RepairIo for Append {
        fn reload(&self, root: &Path) -> Result<Option<View>> {
            let count = self.reloads.get();
            self.reloads.set(count + 1);
            if count == 1 {
                fs::write(&self.frame, &self.raw).unwrap();
            }
            SystemIo.reload(root)
        }
    }
    let root = fixture();
    let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
    let view = read(root.path()).unwrap().unwrap();
    let mut task = view.tasks().as_array().unwrap()[0].clone();
    let task_id = get(&task, "id").unwrap().as_str().unwrap().to_owned();
    task.set_field("owner", Value::String("operator".into()))
        .unwrap();
    task.set_field("status", Value::String("assigned".into()))
        .unwrap();
    task.set_field("updated_at", Value::String("later".into()))
        .unwrap();
    let committed = crate::ledger::commit_task(
        root.path(),
        &view,
        &RequestKey::parse("append-during-repair").unwrap(),
        &RequestBinding::assign(&task_id, "operator", None).unwrap(),
        task,
        &guard,
    )
    .unwrap();
    assert_eq!(committed.delivery, Delivery::Committed);
    let frame = root
        .path()
        .join(DIRECTORY)
        .join(committed.view.epoch_id())
        .join(format!("frames/{:020}.frame", committed.view.sequence()));
    let raw = fs::read(&frame).unwrap();
    fs::remove_file(&frame).unwrap();
    fs::write(root.path().join(PROJECTION), b"\xff\0captured raw").unwrap();
    let hook = Append {
        frame,
        raw,
        reloads: Cell::new(0),
    };
    let failure = repair_with(root.path(), &guard, &hook).unwrap_err();
    assert_eq!(failure.phase(), RepairFailurePhase::ProjectionRevalidation);
    assert_eq!(failure.code(), "ledger_head_changed");
    assert_eq!(failure.known_view().unwrap().revision(), view.revision());
    assert_eq!(
        fs::read(failure.backup_path().unwrap().join("tasks.bytes")).unwrap(),
        b"\xff\0captured raw"
    );
    assert_eq!(
        fs::read(root.path().join(PROJECTION)).unwrap(),
        b"\xff\0captured raw"
    );
    let actual = read(root.path()).unwrap().unwrap();
    assert_eq!(actual.revision(), committed.view.revision());
    assert_eq!(actual.head_digest(), committed.view.head_digest());
}

#[test]
fn every_transport_failure_has_phase_path_and_visibility_evidence() {
    let root = fixture();
    let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
    fs::write(root.path().join(PROJECTION), b"old raw").unwrap();
    let spy = Spy::new(None, "");
    assert_eq!(
        repair_with(root.path(), &guard, &spy).unwrap().status(),
        RepairStatus::Repaired
    );
    let trace = spy.trace.into_inner();
    let child_mkdir = trace
        .iter()
        .enumerate()
        .filter(|(_, e)| **e == "mkdir")
        .nth(1)
        .unwrap()
        .0;
    let publication_start = trace.iter().position(|e| *e == "recheck").unwrap();
    let revalidation_start = trace.iter().rposition(|e| *e == "reload").unwrap();
    let links: Vec<_> = trace
        .iter()
        .enumerate()
        .filter_map(|(i, e)| (*e == "link").then_some(i))
        .collect();
    for (index, event) in trace.iter().enumerate() {
        if root.path().join(RECOVERY).exists() {
            fs::remove_dir_all(root.path().join(RECOVERY)).unwrap();
        }
        fs::write(root.path().join(PROJECTION), b"old raw").unwrap();
        let spy = Spy::new(Some(index), "");
        let result = repair_with(root.path(), &guard, &spy);
        let known_path = root.path().join(RECOVERY).join(ID);
        if *event == "dirsync" {
            let report = result.unwrap();
            assert_eq!(report.status(), RepairStatus::BackupUnconfirmed, "{index}");
            assert_eq!(
                report.durability(),
                RepairDurability::SynchronizationUncertain
            );
            assert_eq!(
                report.backup_path(),
                (index > child_mkdir).then_some(known_path.as_path())
            );
        } else if *event == "cleanup" {
            assert_eq!(result.unwrap().status(), RepairStatus::Repaired);
            assert_ne!(fs::read(root.path().join(PROJECTION)).unwrap(), b"old raw");
            continue;
        } else {
            let failure = result.unwrap_err();
            if *event == "capture" {
                assert_eq!(failure.code(), "input_or_storage_unavailable");
            }
            assert_eq!(
                failure.backup_path(),
                (index > child_mkdir).then_some(known_path.as_path()),
                "{index} {event}"
            );
            assert_eq!(failure.known_view().is_some(), index > 0);
            let phase = if index <= 1 {
                RepairFailurePhase::Authority
            } else if index == 2 {
                RepairFailurePhase::ProjectionCapture
            } else if index < publication_start {
                RepairFailurePhase::BackupPreparation
            } else if index < revalidation_start {
                RepairFailurePhase::BackupPublication
            } else if *event == "replace" {
                RepairFailurePhase::ProjectionReplacement
            } else {
                RepairFailurePhase::ProjectionRevalidation
            };
            assert_eq!(failure.phase(), phase, "{index} {event}");
        }
        assert_eq!(
            fs::read(root.path().join(PROJECTION)).unwrap(),
            b"old raw",
            "{index} {event}"
        );
        assert_eq!(known_path.join("tasks.bytes").exists(), index > links[0]);
        assert_eq!(known_path.join("metadata.json").exists(), index > links[1]);
    }
}

#[test]
fn revalidation_never_overwrites_unbacked_edits_or_changed_heads() {
    for change in [
        "raw",
        "missing",
        "symlink",
        "revision",
        "digest",
        "replacement_uncertain",
    ] {
        let root = fixture();
        let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
        fs::write(root.path().join(PROJECTION), b"captured raw").unwrap();
        let spy = Spy::new(None, change);
        let result = repair_with(root.path(), &guard, &spy);
        let backup = root.path().join(RECOVERY).join(ID);
        assert_eq!(
            fs::read(backup.join("tasks.bytes")).unwrap(),
            b"captured raw"
        );
        if change == "replacement_uncertain" {
            let report = result.unwrap();
            assert_eq!(report.status(), RepairStatus::Repaired);
            assert_eq!(
                report.durability(),
                RepairDurability::SynchronizationUncertain
            );
            assert_eq!(report.backup_path(), Some(backup.as_path()));
            assert_eq!(
                fs::read(root.path().join(PROJECTION)).unwrap(),
                report.view().projection_bytes().unwrap()
            );
        } else {
            let failure = result.unwrap_err();
            assert_eq!(failure.phase(), RepairFailurePhase::ProjectionRevalidation);
            assert_eq!(failure.backup_path(), Some(backup.as_path()));
            assert!(!spy.trace.borrow().contains(&"replace"));
            match change {
                "raw" => assert_eq!(
                    fs::read(root.path().join(PROJECTION)).unwrap(),
                    b"unbacked edit"
                ),
                "missing" => assert!(!root.path().join(PROJECTION).exists()),
                "symlink" => assert!(
                    fs::symlink_metadata(root.path().join(PROJECTION))
                        .unwrap()
                        .is_symlink()
                ),
                _ => {
                    assert_eq!(failure.code(), "ledger_head_changed");
                    assert_eq!(
                        fs::read(root.path().join(PROJECTION)).unwrap(),
                        b"captured raw"
                    );
                }
            }
        }
    }
}

#[test]
fn guard_unsafe_types_and_collisions_are_rejected_without_adoption() {
    let root = fixture();
    let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
    let other = tempfile::tempdir().unwrap();
    let spy = Spy::new(None, "");
    let failure = repair_with(other.path(), &guard, &spy).unwrap_err();
    assert_eq!(failure.code(), "invalid_workspace_lock_owner");
    assert_eq!(failure.phase(), RepairFailurePhase::Authority);
    assert!(failure.known_view().is_none());
    assert!(spy.trace.borrow().is_empty());
    let projection = root.path().join(PROJECTION);
    fs::remove_file(&projection).unwrap();
    for kind in ["directory", "symlink", "fifo"] {
        match kind {
            "directory" => fs::create_dir(&projection).unwrap(),
            "symlink" => symlink(root.path().join("coven.json"), &projection).unwrap(),
            _ => {
                use std::os::unix::ffi::OsStrExt;
                let path = std::ffi::CString::new(projection.as_os_str().as_bytes()).unwrap();
                // SAFETY: valid NUL-terminated path; no borrowed pointer escapes.
                assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
            }
        }
        let failure = repair_projection(root.path(), &guard).unwrap_err();
        assert_eq!(failure.phase(), RepairFailurePhase::ProjectionCapture);
        assert_eq!(failure.code(), "unsafe_archive_path");
        assert!(!root.path().join(RECOVERY).exists());
        if kind == "directory" {
            fs::remove_dir(&projection).unwrap();
        } else {
            fs::remove_file(&projection).unwrap();
        }
    }
    fs::write(&projection, b"old").unwrap();
    let backup = root.path().join(RECOVERY).join(ID);
    fs::create_dir_all(&backup).unwrap();
    let orphan = backup.join(".repair-existing.tmp");
    fs::write(&orphan, b"never adopt").unwrap();
    let spy = Spy::new(None, "");
    let failure = repair_with(root.path(), &guard, &spy).unwrap_err();
    assert_eq!(failure.phase(), RepairFailurePhase::BackupPreparation);
    assert!(failure.backup_path().is_none());
    assert_eq!(fs::read(&orphan).unwrap(), b"never adopt");
    assert_eq!(fs::read_dir(&backup).unwrap().count(), 1);
    assert_eq!(fs::read(&projection).unwrap(), b"old");
}

#[test]
fn pure_report_helper_preserves_shape_and_getters_without_late_io() {
    let root = fixture();
    let view = read(root.path()).unwrap().unwrap();
    let report = view.report(root.path()).unwrap();
    let projection = get(&report, "projection").unwrap().clone();
    assert_eq!(
        get(&report, "workspace_id").unwrap().as_str(),
        Some(view.workspace_id())
    );
    assert_eq!(
        get(&report, "epoch_id").unwrap().as_str(),
        Some(view.epoch_id())
    );
    assert!(exact_integer(
        get(&report, "sequence").unwrap(),
        view.sequence()
    ));
    assert_eq!(
        get(&report, "frame_digest").unwrap().as_str(),
        Some(view.head_digest())
    );
    fs::remove_dir_all(root.path().join("state")).unwrap();
    assert_eq!(
        view.report_with_projection(projection)
            .unwrap()
            .storage()
            .unwrap(),
        report.storage().unwrap()
    );
}

#[test]
fn expected_size_admission_precedes_capture_and_all_backup_io() {
    struct Oversized;
    impl RepairIo for Oversized {
        fn reload(&self, root: &Path) -> Result<Option<View>> {
            let mut view = read(root)?.unwrap();
            view.baseline_projection = Some(vec![b' '; MAX_RAW + 1]);
            Ok(Some(view))
        }
        fn confirm(&self, _: &Path, _: &View, _: &WorkspaceGuard) -> Result<LedgerConfirmation> {
            Ok(LedgerConfirmation::Confirmed)
        }
        fn capture(&self, _: &Path) -> Result<Option<Vec<u8>>> {
            panic!("expected output cap must precede raw capture")
        }
    }
    let root = fixture();
    let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
    let failure = repair_with(root.path(), &guard, &Oversized).unwrap_err();
    assert_eq!(failure.phase(), RepairFailurePhase::ProjectionCapture);
    assert_eq!(failure.code(), "unsupported_replacement_size");
    assert!(failure.known_view().is_some());
    assert!(failure.backup_path().is_none());
    assert!(!root.path().join(RECOVERY).exists());
}

#[test]
fn recovery_path_policy_and_exclusive_file_publication_never_adopt_existing_bytes() {
    struct Collision;
    impl RepairIo for Collision {
        fn temporary(&self, parent: &Path) -> io::Result<tempfile::NamedTempFile> {
            tempfile::Builder::new()
                .prefix(".repair-old")
                .suffix(".tmp")
                .rand_bytes(0)
                .tempfile_in(parent)
        }
    }
    let root = fixture();
    let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
    fs::write(root.path().join(PROJECTION), b"old projection").unwrap();
    let recovery = root.path().join(RECOVERY);
    for directory in [false, true] {
        if directory {
            symlink(root.path().join("state"), &recovery).unwrap();
        } else {
            fs::write(&recovery, b"not a directory").unwrap();
        }
        let failure = repair_projection(root.path(), &guard).unwrap_err();
        assert_eq!(failure.phase(), RepairFailurePhase::BackupPreparation);
        assert_eq!(failure.code(), "unsafe_archive_path");
        assert!(failure.backup_path().is_none());
        fs::remove_file(&recovery).unwrap();
    }
    let base = Path::new(RECOVERY).join(ID);
    fs::create_dir_all(root.path().join(&base)).unwrap();
    let target = root.path().join(&base).join("tasks.bytes");
    fs::write(&target, b"existing backup").unwrap();
    let orphan = root.path().join(&base).join(".repair-old.tmp");
    fs::write(&orphan, b"orphan bytes").unwrap();
    assert!(publish_backup(guard.root(), &base, BackupFile::Tasks, b"new", &SystemIo).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"existing backup");
    assert_eq!(fs::read(&orphan).unwrap(), b"orphan bytes");
    assert_eq!(fs::read_dir(root.path().join(&base)).unwrap().count(), 2);
    fs::remove_file(&target).unwrap();
    assert!(publish_backup(guard.root(), &base, BackupFile::Tasks, b"new", &Collision).is_err());
    assert!(!target.exists());
    assert_eq!(fs::read(&orphan).unwrap(), b"orphan bytes");
    publish_backup(guard.root(), &base, BackupFile::Tasks, b"fresh", &SystemIo).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"fresh");
    assert_eq!(fs::read(&orphan).unwrap(), b"orphan bytes");
    assert_eq!(
        fs::read(root.path().join(PROJECTION)).unwrap(),
        b"old projection"
    );
}

#[test]
fn absent_or_corrupt_authority_is_not_snapshot_recovery() {
    let root = fixture();
    let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
    let original = fs::read(root.path().join(PROJECTION)).unwrap();
    fs::write(root.path().join(MARKER), b"corrupt marker").unwrap();
    let failure = repair_projection(root.path(), &guard).unwrap_err();
    assert_eq!(failure.phase(), RepairFailurePhase::Authority);
    assert!(failure.known_view().is_none());
    fs::remove_file(root.path().join(MARKER)).unwrap();
    let failure = repair_projection(root.path(), &guard).unwrap_err();
    assert_eq!(failure.phase(), RepairFailurePhase::Authority);
    assert_eq!(failure.code(), "no_active_ledger");
    assert!(failure.known_view().is_none());
    assert!(!root.path().join(RECOVERY).exists());
    assert_eq!(fs::read(root.path().join(PROJECTION)).unwrap(), original);
}
