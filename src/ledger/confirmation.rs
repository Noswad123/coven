//! Confirmation is not execution permission, a latest-head guarantee, or a durable
//! receipt acknowledgement. Future task pipelines must explicitly match the outcome
//! under the same guard; execution has its own, different delivery protocol.
use super::*;
use crate::{platform, storage::WorkspaceGuard};
use std::{fs::File, io, path::PathBuf};

/// Synchronization outcome, not execution permission, a latest-head guarantee,
/// or a durable receipt acknowledgement. Callers must explicitly match this
/// under the same guard; execution uses a separate delivery protocol.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerConfirmation {
    Confirmed,
    SynchronizationUncertain,
}

// Domain-local phase seams; production always performs full replay and a bounded
// raw manifest reread. No environment failpoints or publication policy is shared.
trait ConfirmationIo {
    fn reload(&self, root: &Path) -> Result<Option<View>> {
        super::read(root)
    }
    fn read_manifest(&self, root: &Path, relative: &str) -> Result<Vec<u8>> {
        bytes(root, relative)
    }
    fn managed(&self, root: &Path, relative: &Path, directory: bool) -> Result<PathBuf> {
        read_fs::managed(root, relative, directory)
    }
    fn open_readonly(&self, path: &Path) -> Result<File> {
        platform::open_readonly(path)
    }
    fn regular(&self, file: &File) -> io::Result<bool> {
        Ok(file.metadata()?.is_file())
    }
    fn sync_file(&self, file: &File) -> io::Result<()> {
        file.sync_all()
    }
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        platform::sync_directory(path)
    }
}
struct SystemIo;
impl ConfirmationIo for SystemIo {}

/// Revalidate the entire selected history, then synchronize only its authority
/// files and containing directories. Does not acquire a lock, write, or reread
/// after synchronization. `FrozenResult` remains historical, not delivery state.
/// Validation and manifest reread failures are hard errors. Only managed-path
/// metadata, storage-open, descriptor metadata, file sync and directory sync I/O
/// failures during synchronization return `SynchronizationUncertain`; unsafe
/// types/paths and other open policy errors remain hard errors.
pub fn confirm_selected(
    workspace: &Path,
    expected: &View,
    guard: &WorkspaceGuard,
) -> Result<LedgerConfirmation> {
    confirm_with(workspace, expected, guard, &SystemIo)
}

fn confirm_with(
    workspace: &Path,
    expected: &View,
    guard: &WorkspaceGuard,
    io: &impl ConfirmationIo,
) -> Result<LedgerConfirmation> {
    guard.validate_for(workspace)?;
    let root = guard.root();
    let actual = io.reload(root)?.ok_or(Error("ledger_head_changed"))?;
    require(
        actual.revision == expected.revision && actual.head_digest == expected.head_digest,
        "ledger_head_changed",
    )?;
    let base = format!("{DIRECTORY}/{}", actual.epoch_id);
    let manifest = format!("{base}/manifest.json");
    // Preserve Python's extra pre-sync read/hard-failure phase, but conservatively
    // bind raw bytes and require a bool instead of Python's tamper-time truthiness.
    let raw = io.read_manifest(root, &manifest)?;
    require(
        canonical::digest(&raw) == actual.manifest_digest,
        "ledger_manifest_digest_mismatch",
    )?;
    let manifest_value = values::parse(&raw, Profile::Authority)?;
    let present = match get(&manifest_value, "source_present")? {
        Value::Bool(present) => *present,
        _ => return Err(Error("invalid_ledger_source_metadata")),
    };
    let mut files = vec![MARKER.to_owned(), manifest];
    if present {
        files.push(format!("{base}/baseline.tasks.bytes"));
    }
    files.extend(
        (0..=actual.sequence).map(|sequence| format!("{base}/frames/{sequence:020}.frame")),
    );
    for relative in files {
        let path = match io.managed(root, Path::new(&relative), false) {
            Ok(path) => path,
            Err(error) if error.code() == "input_or_storage_unavailable" => {
                return Ok(LedgerConfirmation::SynchronizationUncertain);
            }
            Err(error) => return Err(error),
        };
        let file = match io.open_readonly(&path) {
            Ok(file) => file,
            Err(error) if error.code() == "input_or_storage_unavailable" => {
                return Ok(LedgerConfirmation::SynchronizationUncertain);
            }
            Err(error) => return Err(error),
        };
        match io.regular(&file) {
            Ok(true) => {}
            Ok(false) => return Err(Error("unsafe_ledger_confirmation_type")),
            Err(_) => return Ok(LedgerConfirmation::SynchronizationUncertain),
        }
        if io.sync_file(&file).is_err() {
            return Ok(LedgerConfirmation::SynchronizationUncertain);
        }
    }
    for relative in [
        format!("{base}/frames"),
        base,
        DIRECTORY.to_owned(),
        "state".to_owned(),
    ] {
        let path = match io.managed(root, Path::new(&relative), true) {
            Ok(path) => path,
            Err(error) if error.code() == "input_or_storage_unavailable" => {
                return Ok(LedgerConfirmation::SynchronizationUncertain);
            }
            Err(error) => return Err(error),
        };
        if io.sync_dir(&path).is_err() {
            return Ok(LedgerConfirmation::SynchronizationUncertain);
        }
    }
    // The canonical root is already validated by the existing guard.
    if io.sync_dir(root).is_err() {
        return Ok(LedgerConfirmation::SynchronizationUncertain);
    }
    Ok(LedgerConfirmation::Confirmed)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        cell::RefCell,
        collections::BTreeMap,
        io::{Read, Seek, SeekFrom},
        os::unix::{
            fs::{MetadataExt, symlink},
            process::CommandExt,
        },
        path::PathBuf,
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    struct FixtureChild(Option<Child>);
    impl Drop for FixtureChild {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                // SAFETY: process_group(0) isolates this owned child's group.
                // Kill descendants too, then reap before fixture cleanup.
                unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn fixture(present: bool, sequence: usize) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        // Anonymous files outside the workspace cannot fill a diagnostic pipe.
        let mut stdout = tempfile::tempfile().unwrap();
        let mut stderr = tempfile::tempfile().unwrap();
        let mut child = FixtureChild(Some(Command::new("python3")
            .args(["-c", r#"
import contextlib, io, sys
from pathlib import Path
from runner import coven
root = Path(sys.argv[1])
def invoke(*args):
    with contextlib.redirect_stdout(io.StringIO()):
        status = coven.main(list(map(str, args)))
    assert status == 0, (args, status)
invoke('init', root, '--blank', '--agents', 'operator')
if sys.argv[2] == 'false':
    (root / 'state/tasks.json').unlink()
invoke('state', 'import-tasks', root, '--expect-revision', coven.read_task_snapshot(root)[0], '--json')
for i in range(int(sys.argv[3])):
    invoke('task', 'add', root, f'Task {i}', '--request-id', f'add-{i}', '--json')
"#])
            .arg(root.path())
            .arg(present.to_string())
            .arg(sequence.to_string())
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("COVEN_MULTIPLEXER", "")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .process_group(0)
            .stdout(Stdio::from(stdout.try_clone().unwrap()))
            .stderr(Stdio::from(stderr.try_clone().unwrap()))
            .spawn()
            .unwrap()));
        let deadline = Instant::now() + Duration::from_secs(60);
        let status = loop {
            if let Some(status) = child.0.as_mut().unwrap().try_wait().unwrap() {
                // try_wait already reaped the completed child.
                child.0.take();
                break Some(status);
            }
            if Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        drop(child);
        stdout.seek(SeekFrom::Start(0)).unwrap();
        stderr.seek(SeekFrom::Start(0)).unwrap();
        let mut out = String::new();
        let mut err = String::new();
        stdout.read_to_string(&mut out).unwrap();
        stderr.read_to_string(&mut err).unwrap();
        assert!(
            status.is_some(),
            "Python fixture exceeded 60-second deadline\n{out}\n{err}"
        );
        assert!(status.unwrap().success(), "{out}\n{err}");
        root
    }

    #[derive(Clone, Copy)]
    enum Fault {
        Io,
        OpenPolicy,
        Nonregular,
    }
    struct Spy {
        root: PathBuf,
        trace: RefCell<Vec<String>>,
        current: RefCell<String>,
        fault: Option<(usize, Fault)>,
    }
    impl Spy {
        fn new(root: &Path, fault: Option<(usize, Fault)>) -> Self {
            Self {
                root: root.canonicalize().unwrap(),
                trace: RefCell::new(Vec::new()),
                current: RefCell::new(String::new()),
                fault,
            }
        }
        fn hit(&self, event: String) -> Option<Fault> {
            let mut trace = self.trace.borrow_mut();
            let index = trace.len();
            trace.push(event);
            self.fault
                .filter(|(at, _)| *at == index)
                .map(|(_, fault)| fault)
        }
        fn name(&self, path: &Path) -> String {
            let relative = path.strip_prefix(&self.root).unwrap();
            if relative.as_os_str().is_empty() {
                ".".into()
            } else {
                relative.to_str().unwrap().into()
            }
        }
    }
    impl ConfirmationIo for Spy {
        fn reload(&self, root: &Path) -> Result<Option<View>> {
            assert_eq!(root, self.root);
            if self.hit("reload".into()).is_some() {
                return Err(Error("input_or_storage_unavailable"));
            }
            SystemIo.reload(root)
        }
        fn read_manifest(&self, root: &Path, relative: &str) -> Result<Vec<u8>> {
            if self.hit(format!("reread:{relative}")).is_some() {
                return Err(Error("input_or_storage_unavailable"));
            }
            SystemIo.read_manifest(root, relative)
        }
        fn managed(&self, root: &Path, relative: &Path, directory: bool) -> Result<PathBuf> {
            let kind = if directory { "dirpath" } else { "filepath" };
            match self.hit(format!("{kind}:{}", relative.display())) {
                Some(Fault::OpenPolicy) => Err(Error("unsafe_archive_path")),
                Some(_) => Err(Error("input_or_storage_unavailable")),
                None => SystemIo.managed(root, relative, directory),
            }
        }
        fn open_readonly(&self, path: &Path) -> Result<File> {
            let name = self.name(path);
            *self.current.borrow_mut() = name.clone();
            match self.hit(format!("open:{name}")) {
                Some(Fault::OpenPolicy) => Err(Error("unsafe_archive_path")),
                Some(_) => Err(Error("input_or_storage_unavailable")),
                None => SystemIo.open_readonly(path),
            }
        }
        fn regular(&self, file: &File) -> io::Result<bool> {
            match self.hit(format!("metadata:{}", self.current.borrow())) {
                Some(Fault::Nonregular) => Ok(false),
                Some(_) => Err(io::Error::other("metadata fault")),
                None => SystemIo.regular(file),
            }
        }
        fn sync_file(&self, file: &File) -> io::Result<()> {
            if self
                .hit(format!("fsync:{}", self.current.borrow()))
                .is_some()
            {
                return Err(io::Error::other("file sync fault"));
            }
            SystemIo.sync_file(file)
        }
        fn sync_dir(&self, path: &Path) -> io::Result<()> {
            if self.hit(format!("dirsync:{}", self.name(path))).is_some() {
                return Err(io::Error::other("directory sync fault"));
            }
            SystemIo.sync_dir(path)
        }
    }

    // Ignore atime: read-only confirmation may update it. Compare namespace,
    // contents, modes, timestamps and inode identity, including the owned lock.
    #[derive(Debug, PartialEq, Eq)]
    struct Entry(u32, u64, u64, i64, i64, Vec<u8>, Option<PathBuf>);
    fn inventory(root: &Path) -> BTreeMap<PathBuf, Entry> {
        fn visit(root: &Path, path: &Path, output: &mut BTreeMap<PathBuf, Entry>) {
            let m = fs::symlink_metadata(path).unwrap();
            output.insert(
                path.strip_prefix(root).unwrap().to_owned(),
                Entry(
                    m.mode(),
                    m.dev(),
                    m.ino(),
                    m.mtime(),
                    m.mtime_nsec(),
                    if m.is_file() {
                        fs::read(path).unwrap()
                    } else {
                        Vec::new()
                    },
                    m.is_symlink().then(|| fs::read_link(path).unwrap()),
                ),
            );
            if m.is_dir() {
                for entry in fs::read_dir(path).unwrap() {
                    visit(root, &entry.unwrap().path(), output);
                }
            }
        }
        let mut output = BTreeMap::new();
        visit(root, root, &mut output);
        output
    }

    #[test]
    fn python_fixtures_exact_trace_and_every_sync_fault_stops() {
        for present in [false, true] {
            for sequence in [0, 3] {
                let root = fixture(present, sequence);
                let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
                let view = read(root.path()).unwrap().unwrap();
                let base = format!("{DIRECTORY}/{}", view.epoch_id);
                let manifest = format!("{base}/manifest.json");
                let mut expected = vec!["reload".into(), format!("reread:{manifest}")];
                let mut files = vec![MARKER.to_owned(), manifest];
                if present {
                    files.push(format!("{base}/baseline.tasks.bytes"));
                }
                files.extend((0..=sequence).map(|s| format!("{base}/frames/{s:020}.frame")));
                for file in files {
                    for operation in ["filepath", "open", "metadata", "fsync"] {
                        expected.push(format!("{operation}:{file}"));
                    }
                }
                for dir in [
                    format!("{base}/frames"),
                    base,
                    DIRECTORY.into(),
                    "state".into(),
                    ".".into(),
                ] {
                    if dir != "." {
                        expected.push(format!("dirpath:{dir}"));
                    }
                    expected.push(format!("dirsync:{dir}"));
                }
                let before = inventory(root.path());
                let spy = Spy::new(root.path(), None);
                assert_eq!(
                    confirm_with(root.path(), &view, &guard, &spy).unwrap(),
                    LedgerConfirmation::Confirmed
                );
                assert_eq!(*spy.trace.borrow(), expected);
                assert_eq!(
                    confirm_selected(root.path(), &view, &guard).unwrap(),
                    LedgerConfirmation::Confirmed
                );
                for (index, event) in expected.iter().enumerate() {
                    let spy = Spy::new(root.path(), Some((index, Fault::Io)));
                    let result = confirm_with(root.path(), &view, &guard, &spy);
                    if index < 2 {
                        assert_eq!(result.unwrap_err().code(), "input_or_storage_unavailable");
                    } else {
                        assert_eq!(
                            result.unwrap(),
                            LedgerConfirmation::SynchronizationUncertain
                        );
                    }
                    assert_eq!(*spy.trace.borrow(), expected[..=index]);
                    let fault = if event.starts_with("open:")
                        || event.starts_with("filepath:")
                        || event.starts_with("dirpath:")
                    {
                        Some(Fault::OpenPolicy)
                    } else if event.starts_with("metadata:") {
                        Some(Fault::Nonregular)
                    } else {
                        None
                    };
                    if let Some(fault) = fault {
                        let spy = Spy::new(root.path(), Some((index, fault)));
                        let error = confirm_with(root.path(), &view, &guard, &spy).unwrap_err();
                        assert_eq!(
                            error.code(),
                            if matches!(fault, Fault::Nonregular) {
                                "unsafe_ledger_confirmation_type"
                            } else {
                                "unsafe_archive_path"
                            }
                        );
                        assert_eq!(*spy.trace.borrow(), expected[..=index]);
                    }
                }
                assert_eq!(inventory(root.path()), before);
            }
        }
    }

    #[test]
    fn guard_precedes_authority_io_and_head_components_are_independent() {
        let root = fixture(false, 0);
        let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
        let mut view = read(root.path()).unwrap().unwrap();
        let other = tempfile::tempdir().unwrap();
        let spy = Spy::new(root.path(), None);
        assert_eq!(
            confirm_with(other.path(), &view, &guard, &spy)
                .unwrap_err()
                .code(),
            "invalid_workspace_lock_owner"
        );
        assert!(spy.trace.borrow().is_empty());
        for digest in [false, true] {
            let original = if digest {
                view.head_digest.clone()
            } else {
                view.revision.clone()
            };
            if digest {
                view.head_digest.push('x');
            } else {
                view.revision.push('x');
            }
            let spy = Spy::new(root.path(), None);
            assert_eq!(
                confirm_with(root.path(), &view, &guard, &spy)
                    .unwrap_err()
                    .code(),
                "ledger_head_changed"
            );
            assert_eq!(*spy.trace.borrow(), ["reload"]);
            if digest {
                view.head_digest = original;
            } else {
                view.revision = original;
            }
        }
        let tail = root
            .path()
            .join(DIRECTORY)
            .join(&view.epoch_id)
            .join("frames/00000000000000000001.frame");
        fs::write(&tail, b"corrupt appended tail").unwrap();
        let spy = Spy::new(root.path(), None);
        assert!(confirm_with(root.path(), &view, &guard, &spy).is_err());
        assert_eq!(*spy.trace.borrow(), ["reload"]);
        fs::remove_file(tail).unwrap();
        fs::remove_file(root.path().join(MARKER)).unwrap();
        assert_eq!(
            confirm_with(root.path(), &view, &guard, &spy)
                .unwrap_err()
                .code(),
            "ledger_head_changed"
        );
        assert!(pending(root.path()).unwrap());
    }

    struct ManifestTransition<'a> {
        spy: &'a Spy,
        change: &'a str,
    }
    impl ConfirmationIo for ManifestTransition<'_> {
        fn reload(&self, root: &Path) -> Result<Option<View>> {
            let mut actual = self.spy.reload(root)?.unwrap();
            let path = root
                .join(DIRECTORY)
                .join(&actual.epoch_id)
                .join("manifest.json");
            match self.change {
                "missing" => fs::remove_file(&path).unwrap(),
                "directory" => {
                    fs::remove_file(&path).unwrap();
                    fs::create_dir(&path).unwrap();
                }
                "symlink" => {
                    fs::remove_file(&path).unwrap();
                    symlink(root.join("coven.json"), &path).unwrap();
                }
                "digest" => fs::write(&path, b"{}").unwrap(),
                // Bind these synthetic seam values solely to exercise Authority
                // parsing and strict bool validation beyond the digest gate.
                "truthy" | "profile" | "authority_profile" => {
                    let raw = if self.change == "truthy" {
                        b"{\"source_present\":1}".to_vec()
                    } else if self.change == "authority_profile" {
                        format!(
                            "{{\"source_present\":true,\"extra\":\"{}\"}}",
                            "x".repeat(values::MAX_STRING + 1)
                        )
                        .into_bytes()
                    } else {
                        vec![b' '; MAX_FRAME_BYTES + 1]
                    };
                    actual.manifest_digest = canonical::digest(&raw);
                    fs::write(&path, raw).unwrap();
                }
                "parse" => {
                    actual.manifest_digest = canonical::digest(b"invalid");
                    fs::write(&path, b"invalid").unwrap();
                }
                _ => panic!("unknown transition"),
            }
            Ok(Some(actual))
        }
        fn read_manifest(&self, root: &Path, relative: &str) -> Result<Vec<u8>> {
            self.spy.read_manifest(root, relative)
        }
        fn open_readonly(&self, _: &Path) -> Result<File> {
            panic!("manifest failures must precede sync")
        }
        fn sync_dir(&self, _: &Path) -> io::Result<()> {
            panic!("manifest failures must precede sync")
        }
    }
    #[test]
    fn extra_manifest_read_failures_are_always_hard_before_sync() {
        for change in [
            "missing",
            "directory",
            "symlink",
            "digest",
            "truthy",
            "profile",
            "authority_profile",
            "parse",
        ] {
            let root = fixture(true, 0);
            let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
            let view = read(root.path()).unwrap().unwrap();
            let spy = Spy::new(root.path(), None);
            let transition = ManifestTransition { spy: &spy, change };
            let error = confirm_with(root.path(), &view, &guard, &transition).unwrap_err();
            let expected = match change {
                "missing" => "required_ledger_file_missing",
                "directory" | "symlink" => "unsafe_archive_path",
                "digest" => "ledger_manifest_digest_mismatch",
                "truthy" => "invalid_ledger_source_metadata",
                "profile" => "handoff_size_limit",
                "authority_profile" => "unsupported_json_string_profile",
                "parse" => "invalid_compat_json",
                _ => unreachable!(),
            };
            assert_eq!(error.code(), expected, "{change}");
            assert_eq!(spy.trace.borrow().len(), 2);
        }
    }

    struct PathTransition<'a> {
        spy: &'a Spy,
        trigger: String,
        target: PathBuf,
    }
    impl PathTransition<'_> {
        fn change_after(&self, event: &str) {
            if event == self.trigger {
                let moved = self.target.with_extension("moved");
                fs::rename(&self.target, &moved).unwrap();
                symlink(moved, &self.target).unwrap();
            }
        }
    }
    impl ConfirmationIo for PathTransition<'_> {
        fn reload(&self, root: &Path) -> Result<Option<View>> {
            self.spy.reload(root)
        }
        fn read_manifest(&self, root: &Path, relative: &str) -> Result<Vec<u8>> {
            let raw = self.spy.read_manifest(root, relative)?;
            self.change_after("reread");
            Ok(raw)
        }
        fn open_readonly(&self, path: &Path) -> Result<File> {
            self.spy.open_readonly(path)
        }
        fn regular(&self, file: &File) -> io::Result<bool> {
            self.spy.regular(file)
        }
        fn sync_file(&self, file: &File) -> io::Result<()> {
            self.spy.sync_file(file)?;
            self.change_after(&format!("fsync:{}", self.spy.current.borrow()));
            Ok(())
        }
        fn sync_dir(&self, path: &Path) -> io::Result<()> {
            self.spy.sync_dir(path)?;
            self.change_after(&format!("dirsync:{}", self.spy.name(path)));
            Ok(())
        }
    }
    #[test]
    fn every_managed_sync_path_is_rechecked_and_unsafe_paths_fail_hard() {
        for target in [
            "marker", "manifest", "baseline", "frame", "frames", "epoch", "ledger", "state",
        ] {
            let root = fixture(true, 0);
            let guard = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
            let view = read(root.path()).unwrap().unwrap();
            let base = format!("{DIRECTORY}/{}", view.epoch_id);
            let frame = format!("{base}/frames/00000000000000000000.frame");
            let (relative, trigger, stopped_before) = match target {
                "marker" => (MARKER.into(), "reread".into(), format!("open:{MARKER}")),
                "manifest" => (
                    format!("{base}/manifest.json"),
                    "reread".into(),
                    format!("open:{base}/manifest.json"),
                ),
                "baseline" => (
                    format!("{base}/baseline.tasks.bytes"),
                    "reread".into(),
                    format!("open:{base}/baseline.tasks.bytes"),
                ),
                "frame" => (frame.clone(), "reread".into(), format!("open:{frame}")),
                "frames" => (
                    format!("{base}/frames"),
                    format!("fsync:{frame}"),
                    format!("dirsync:{base}/frames"),
                ),
                "epoch" => (
                    base.clone(),
                    format!("dirsync:{base}/frames"),
                    format!("dirsync:{base}"),
                ),
                "ledger" => (
                    DIRECTORY.into(),
                    format!("dirsync:{base}"),
                    format!("dirsync:{DIRECTORY}"),
                ),
                "state" => (
                    "state".into(),
                    format!("dirsync:{DIRECTORY}"),
                    "dirsync:state".into(),
                ),
                _ => unreachable!(),
            };
            let spy = Spy::new(root.path(), None);
            let transition = PathTransition {
                spy: &spy,
                trigger,
                target: root.path().join(relative),
            };
            assert_eq!(
                confirm_with(root.path(), &view, &guard, &transition)
                    .unwrap_err()
                    .code(),
                "unsafe_archive_path",
                "{target}"
            );
            assert!(!spy.trace.borrow().contains(&stopped_before), "{target}");
        }
    }
}
