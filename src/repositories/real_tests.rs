//! Disposable real-Git domain tests. Supervisor lifecycle has separate acceptance tests.
use super::*;
use crate::process::ProbeStatus;
use std::{
    os::unix::{
        ffi::OsStringExt,
        fs::{MetadataExt, symlink},
    },
    path::PathBuf,
    process::Command,
};

struct GitRunner {
    calls: Cell<usize>,
}
impl ProbeRunner for GitRunner {
    fn run(&self, request: &ProbeRequest) -> ProbeResult {
        self.calls.set(self.calls.get() + 1);
        let output = Command::new(&request.executable)
            .args(&request.arguments)
            .current_dir(&request.cwd)
            .env_clear()
            .envs(&request.environment)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        ProbeResult {
            status: if output.status.success() {
                ProbeStatus::Ok
            } else {
                ProbeStatus::Nonzero
            },
            returncode: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
            duration_ms: 0.0,
        }
    }
}
struct Fixture {
    _temp: tempfile::TempDir,
    workspace: PathBuf,
    checkout: PathBuf,
    base: String,
}
impl Fixture {
    fn new(format: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let checkout = temp.path().join("checkout");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(workspace.join("state")).unwrap();
        fs::write(workspace.join("coven.json"), b"corrupt\xff").unwrap();
        fs::create_dir(&checkout).unwrap();
        let mut f = Self {
            _temp: temp,
            workspace,
            checkout,
            base: String::new(),
        };
        f.git(&["init", "--quiet", &format!("--object-format={format}")]);
        fs::write(f.checkout.join("file"), b"original\n").unwrap();
        f.git(&["add", "file"]);
        f.git(&["commit", "--quiet", "-m", "fixture"]);
        f.base = String::from_utf8(f.git(&["rev-parse", "HEAD"]))
            .unwrap()
            .trim()
            .to_owned();
        f
    }
    fn git(&self, args: &[&str]) -> Vec<u8> {
        let git = crate::process::identity::find_executable("git")
            .unwrap()
            .canonicalize()
            .unwrap();
        let output = Command::new(git)
            // Fixture setup must be quiescent before exact source snapshots;
            // a detached maintenance child can otherwise remove its own lock
            // after setup returns and resemble a write by the code under test.
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "maintenance.auto=false",
                "-c",
                "gc.auto=0",
            ])
            .args(args)
            .current_dir(&self.checkout)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {:?}", output.stderr);
        output.stdout
    }
    fn request(&self) -> record::Request {
        record::Request::new(
            &self.checkout,
            &self.base,
            "refs/heads/delivery",
            UNREGISTERED,
        )
        .unwrap()
    }
    fn register(&self, runner: &impl ProbeRunner) -> Result<TaskOutput> {
        registration::register(
            &self.workspace,
            self.request(),
            true,
            runner,
            &registration::SystemIo,
        )
    }
}
fn snapshot(root: &Path) -> BTreeSnapshot {
    fn walk(root: &Path, current: &Path, result: &mut BTreeSnapshot) {
        for entry in fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            let m = fs::symlink_metadata(&path).unwrap();
            if m.is_dir() {
                walk(root, &path, result);
            } else if m.is_file() {
                result.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    (
                        fs::read(&path).unwrap(),
                        m.ino(),
                        m.mode(),
                        m.mtime(),
                        m.mtime_nsec(),
                        m.ctime(),
                        m.ctime_nsec(),
                    ),
                );
            }
        }
    }
    let mut result = BTreeSnapshot::new();
    walk(root, root, &mut result);
    result
}
type BTreeSnapshot = std::collections::BTreeMap<PathBuf, (Vec<u8>, u64, u32, i64, i64, i64, i64)>;

#[test]
fn git_pin_detects_replacement_retarget_and_final_same_metadata_rewrite() {
    use std::{
        cell::RefCell,
        collections::VecDeque,
        io::{Read, Seek, SeekFrom, Write},
        os::unix::fs::PermissionsExt,
    };
    const MODE: &str = "COVEN_REPOSITORY_GIT_PIN_TEST";
    let Ok(mode) = std::env::var(MODE) else {
        let git = crate::process::identity::find_executable("git")
            .unwrap()
            .canonicalize()
            .unwrap();
        for mode in ["replace", "retarget", "finalhash", "verify"] {
            let temp = tempfile::tempdir().unwrap();
            let bin = temp.path().join("bin");
            fs::create_dir(&bin).unwrap();
            for name in ["target", "other"] {
                let path = temp.path().join(name);
                fs::copy(&git, &path).unwrap();
                let mut permissions = fs::metadata(&path).unwrap().permissions();
                permissions.set_mode(permissions.mode() | 0o200);
                fs::set_permissions(&path, permissions).unwrap();
            }
            symlink(temp.path().join("target"), bin.join("git")).unwrap();
            let mut paths = vec![PathBuf::from("bin")];
            paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
            let test = format!(
                "{}::git_pin_detects_replacement_retarget_and_final_same_metadata_rewrite",
                module_path!().split_once("::").unwrap().1
            );
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &test, "--nocapture"])
                .current_dir(temp.path())
                .env(MODE, mode)
                .env("PATH", std::env::join_paths(paths).unwrap())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{mode}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
        return;
    };
    fn copy_result(result: &ProbeResult) -> ProbeResult {
        ProbeResult {
            status: result.status,
            returncode: result.returncode,
            stdout: result.stdout.clone(),
            stderr: result.stderr.clone(),
            duration_ms: result.duration_ms,
        }
    }
    struct RecordProbes {
        runner: GitRunner,
        results: RefCell<VecDeque<ProbeResult>>,
    }
    impl ProbeRunner for RecordProbes {
        fn run(&self, r: &ProbeRequest) -> ProbeResult {
            let result = self.runner.run(r);
            self.results.borrow_mut().push_back(copy_result(&result));
            result
        }
    }
    let f = Fixture::new("sha1");
    let recorder = RecordProbes {
        runner: GitRunner {
            calls: Cell::new(0),
        },
        results: RefCell::new(VecDeque::new()),
    };
    super::super::git::observe(&f.request(), &recorder).unwrap();
    let known = if mode == "verify" {
        Some(parsed(
            f.register(&GitRunner {
                calls: Cell::new(0),
            })
            .unwrap(),
        ))
    } else {
        None
    };
    struct Replay {
        mode: String,
        selected: PathBuf,
        results: RefCell<VecDeque<ProbeResult>>,
        calls: Cell<usize>,
        mutate_at: usize,
    }
    impl ProbeRunner for Replay {
        fn run(&self, r: &ProbeRequest) -> ProbeResult {
            self.calls.set(self.calls.get() + 1);
            if self.mode != "finalhash" {
                assert!(!r.arguments.iter().any(|a| a == "status"));
            }
            let result = self.results.borrow_mut().pop_front().unwrap();
            if self.calls.get() == self.mutate_at {
                if self.mode == "retarget" {
                    let other = r.executable.with_file_name("other");
                    fs::remove_file(&self.selected).unwrap();
                    symlink(other, &self.selected).unwrap();
                } else if self.mode == "finalhash" {
                    let before = crate::process::identity::metadata(&r.executable).unwrap();
                    let mut file = fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&r.executable)
                        .unwrap();
                    let modified = file.metadata().unwrap().modified().unwrap();
                    let mut byte = [0];
                    file.read_exact(&mut byte).unwrap();
                    byte[0] ^= 1;
                    file.seek(SeekFrom::Start(0)).unwrap();
                    file.write_all(&byte).unwrap();
                    file.set_modified(modified).unwrap();
                    assert_eq!(
                        crate::process::identity::metadata(&r.executable).unwrap(),
                        before
                    );
                } else {
                    let replacement = r.executable.with_file_name("replacement");
                    fs::copy(&r.executable, &replacement).unwrap();
                    fs::rename(replacement, &r.executable).unwrap();
                }
            }
            result
        }
    }
    let count = recorder.results.borrow().len();
    let replay = Replay {
        mode: mode.clone(),
        selected: crate::process::identity::find_executable("git").unwrap(),
        results: recorder.results,
        calls: Cell::new(0),
        mutate_at: if mode == "finalhash" { count } else { 1 },
    };
    let before = snapshot(&f.checkout);
    if let Some(known) = known {
        let output = inspect_with(&f.workspace, true, true, &replay).unwrap();
        assert_eq!(output.exit_code, 1);
        let value = parsed(output);
        assert_eq!(value["registration"], known["registration"]);
        assert_eq!(value["revision"], known["revision"]);
        assert_eq!(
            parsed(f.register(&Never).unwrap())["revision"],
            known["revision"]
        );
    } else {
        assert_eq!(
            f.register(&replay).err().unwrap().code(),
            "repository_git_changed"
        );
        assert!(!f.workspace.join(AUTHORITY).exists());
        assert!(!f.workspace.join("state/.workspace.lock").exists());
    }
    assert_eq!(
        replay.calls.get(),
        if mode == "finalhash" { count } else { 1 }
    );
    assert_eq!(snapshot(&f.checkout), before);
}

#[test]
fn real_sha1_sha256_dirty_preserves_every_source_byte_and_metadata() {
    for format in ["sha1", "sha256"] {
        let f = Fixture::new(format);
        fs::write(f.checkout.join("file"), b"staged\n").unwrap();
        f.git(&["add", "file"]);
        fs::write(f.checkout.join("file"), b"unstaged\n").unwrap();
        fs::write(
            f.checkout
                .join(std::ffi::OsString::from_vec(b"nonutf8-\xff\n".to_vec())),
            b"untracked",
        )
        .unwrap();
        let before = snapshot(&f.checkout);
        let runner = GitRunner {
            calls: Cell::new(0),
        };
        let output = f.register(&runner).unwrap();
        assert_eq!(output.exit_code, 0);
        let result = parsed(output);
        assert_eq!(result["registration"]["object_format"], format);
        assert_eq!(result["observations"]["tracked_changes"], 1);
        assert_eq!(result["observations"]["untracked"], 1);
        assert_eq!(result["delivery"]["outcome"], "durably_committed");
        assert_eq!(snapshot(&f.checkout), before);
        let verified = parsed(inspect_with(&f.workspace, true, true, &runner).unwrap());
        assert_eq!(verified["observations"]["available"], true);
        assert_eq!(snapshot(&f.checkout), before);
        assert_eq!(
            fs::metadata(f.workspace.join(AUTHORITY)).unwrap().mode() & 0o7777,
            0o600
        );
        let count = runner.calls.get();
        fs::remove_dir_all(&f.checkout).unwrap();
        assert_eq!(
            parsed(f.register(&runner).unwrap())["revision"],
            result["revision"]
        );
        assert_eq!(runner.calls.get(), count);
    }
}

#[test]
fn linked_worktree_and_retargeted_original_alias_retry() {
    let f = Fixture::new("sha1");
    let linked = f._temp.path().join("linked");
    f.git(&[
        "worktree",
        "add",
        "--quiet",
        "--detach",
        linked.to_str().unwrap(),
        &f.base,
    ]);
    let alias = f._temp.path().join("alias");
    symlink(&linked, &alias).unwrap();
    let request =
        record::Request::new(&alias, &f.base, "refs/heads/delivery", UNREGISTERED).unwrap();
    let runner = GitRunner {
        calls: Cell::new(0),
    };
    let first = parsed(
        registration::register(
            &f.workspace,
            request.clone(),
            true,
            &runner,
            &registration::SystemIo,
        )
        .unwrap(),
    );
    assert_ne!(
        first["registration"]["git_dir"],
        first["registration"]["common_dir"]
    );
    fs::remove_file(&alias).unwrap();
    symlink("missing", &alias).unwrap();
    let count = runner.calls.get();
    let replay = parsed(
        registration::register(
            &f.workspace,
            request,
            true,
            &runner,
            &registration::SystemIo,
        )
        .unwrap(),
    );
    assert_eq!(replay["registration"], first["registration"]);
    assert_eq!(runner.calls.get(), count);
    assert_eq!(
        inspect_with(&f.workspace, true, true, &runner)
            .unwrap()
            .exit_code,
        1
    );
}

#[test]
fn exclusions_before_status_and_no_helper_or_source_writes() {
    for exclusion in [
        "split",
        "shared",
        "promisor",
        "include",
        "filter",
        "partial",
        "alternates",
    ] {
        let f = Fixture::new("sha1");
        match exclusion {
            "split" => {
                f.git(&["config", "core.splitIndex", "true"]);
            }
            "shared" => {
                fs::write(f.checkout.join(".git/ShArEdInDeX.stale"), b"stale").unwrap();
            }
            "promisor" => {
                fs::write(f.checkout.join(".git/objects/pack/fake.PrOmIsOr"), b"").unwrap();
            }
            "include" => {
                f.git(&["config", "include.path", "/missing"]);
            }
            "filter" => {
                f.git(&["config", "filter.evil.clean", "touch /never"]);
            }
            "partial" => {
                f.git(&["config", "remote.origin.promisor", "true"]);
                f.git(&["config", "remote.origin.url", "evil::example"]);
            }
            "alternates" => {
                fs::write(
                    f.checkout.join(".git/objects/info/alternates"),
                    b"/missing\n",
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        struct NoStatus(GitRunner);
        impl ProbeRunner for NoStatus {
            fn run(&self, r: &ProbeRequest) -> ProbeResult {
                assert!(!r.arguments.iter().any(|a| a == "status"));
                self.0.run(r)
            }
        }
        let before = snapshot(&f.checkout);
        assert!(
            f.register(&NoStatus(GitRunner {
                calls: Cell::new(0)
            }))
            .is_err(),
            "{exclusion}"
        );
        assert_eq!(snapshot(&f.checkout), before);
        assert!(!f.workspace.join(AUTHORITY).exists());
    }
}

#[test]
fn prepublication_refusal_and_known_postpublication_faults() {
    struct Fault {
        phase: &'static str,
    }
    impl registration::RegistrationIo for Fault {
        fn replace(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
            if self.phase == "before" {
                return Err(crate::Error("injected_prepublication"));
            }
            let delivery =
                crate::storage::atomic_replace_bytes(guard, Path::new(AUTHORITY), bytes)?;
            if self.phase == "after" {
                return Ok(Delivery::DurabilityUncertain);
            }
            if self.phase == "delete" {
                fs::remove_file(guard.root().join(AUTHORITY))?;
            }
            if self.phase == "corrupt" {
                fs::write(guard.root().join(AUTHORITY), b"null")?;
            }
            Ok(delivery)
        }
    }
    for phase in ["before", "after", "delete", "corrupt"] {
        let f = Fixture::new("sha1");
        let runner = GitRunner {
            calls: Cell::new(0),
        };
        let result =
            registration::register(&f.workspace, f.request(), true, &runner, &Fault { phase });
        if phase == "before" {
            assert!(result.is_err());
            assert!(!f.workspace.join(AUTHORITY).exists());
        } else {
            let output = result.unwrap();
            assert_eq!(output.exit_code, 2);
            let value = parsed(output);
            assert_eq!(value["registration"]["base_commit"], f.base);
            assert_eq!(value["delivery"]["outcome"], "durability_uncertain");
            assert_eq!(value["replayed"], false);
        }
    }
}

#[test]
fn under_lock_concurrent_exact_replay_or_immutable_conflict() {
    struct Concurrent {
        different: bool,
        request: record::Request,
    }
    impl registration::RegistrationIo for Concurrent {
        fn lock(&self, root: &Path) -> Result<WorkspaceGuard> {
            let guard = WorkspaceGuard::acquire(root, Duration::ZERO)?;
            let mut value = fixture_value(&fixture());
            value["original_request"] = serde_json::to_value(&self.request)?;
            value["base_commit"] = json!(self.request.base);
            value["delivery_target"] = json!(self.request.delivery_target);
            if self.different {
                value["delivery_target"] = json!("refs/heads/concurrent");
                value["original_request"]["delivery_target"] = json!("refs/heads/concurrent");
            }
            let bytes = serde_json::to_vec(&value)?;
            record::Authority::decode(bytes.clone())?;
            crate::storage::atomic_replace_bytes(&guard, Path::new(AUTHORITY), &bytes)?;
            Ok(guard)
        }
        fn replace(&self, _: &WorkspaceGuard, _: &[u8]) -> Result<Delivery> {
            panic!("must not replace concurrent authority")
        }
    }
    for different in [false, true] {
        let f = Fixture::new("sha1");
        let runner = GitRunner {
            calls: Cell::new(0),
        };
        let result = registration::register(
            &f.workspace,
            f.request(),
            true,
            &runner,
            &Concurrent {
                different,
                request: f.request(),
            },
        );
        if different {
            assert_eq!(
                result.err().unwrap().code(),
                "repository_immutable_request_conflict"
            );
        } else {
            let value = parsed(result.unwrap());
            assert_eq!(value["replayed"], true);
            assert_eq!(value["observations"]["status"], "not_assessed");
        }
    }
}

#[test]
fn selector_retarget_between_observation_passes_cannot_publish() {
    struct Retarget {
        checkout: PathBuf,
        old: PathBuf,
    }
    impl registration::RegistrationIo for Retarget {
        fn lock(&self, root: &Path) -> Result<WorkspaceGuard> {
            let guard = WorkspaceGuard::acquire(root, Duration::ZERO)?;
            fs::rename(&self.checkout, &self.old)?;
            symlink(&self.old, &self.checkout)?;
            Ok(guard)
        }
        fn replace(&self, _: &WorkspaceGuard, _: &[u8]) -> Result<Delivery> {
            panic!("changed frozen facts")
        }
    }
    let f = Fixture::new("sha1");
    let runner = GitRunner {
        calls: Cell::new(0),
    };
    let result = registration::register(
        &f.workspace,
        f.request(),
        true,
        &runner,
        &Retarget {
            checkout: f.checkout.clone(),
            old: f._temp.path().join("moved"),
        },
    );
    assert!(result.is_err());
    assert!(!f.workspace.join(AUTHORITY).exists());
}

#[test]
fn independent_cooperating_registrations_have_one_immutable_fact() {
    for same in [false, true] {
        let f = Fixture::new("sha1");
        let results = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..2)
                .map(|index| {
                    let workspace = &f.workspace;
                    let mut request = f.request();
                    if !same && index == 1 {
                        request.delivery_target = "refs/heads/other".to_owned();
                    }
                    scope.spawn(move || {
                        registration::register(
                            workspace,
                            request,
                            true,
                            &GitRunner {
                                calls: Cell::new(0),
                            },
                            &registration::SystemIo,
                        )
                    })
                })
                .collect();
            threads
                .into_iter()
                .map(|t| t.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(
            results.iter().filter(|r| r.is_ok()).count(),
            if same { 2 } else { 1 }
        );
        let authority = record::read(&f.workspace).unwrap().unwrap();
        for output in results.into_iter().flatten() {
            assert_eq!(output.exit_code, 0);
            assert_eq!(parsed(output)["revision"], authority.revision());
        }
    }
}

#[test]
fn unborn_head_and_missing_historical_blob_have_limited_availability() {
    let f = Fixture::new("sha1");
    let blob = String::from_utf8(f.git(&["rev-parse", "HEAD:file"]))
        .unwrap()
        .trim()
        .to_owned();
    fs::write(f.checkout.join("file"), b"new HEAD content\n").unwrap();
    f.git(&["add", "file"]);
    f.git(&["commit", "--quiet", "-m", "advance"]);
    fs::remove_file(
        f.checkout
            .join(".git/objects")
            .join(&blob[..2])
            .join(&blob[2..]),
    )
    .unwrap();
    let runner = GitRunner {
        calls: Cell::new(0),
    };
    let value = parsed(f.register(&runner).unwrap());
    assert_eq!(
        value["observations"]["base_object_validation"],
        "commit_and_root_tree_only"
    );
    assert_eq!(
        value["observations"]["complete_checkout_availability"],
        "unestablished"
    );
    f.git(&["symbolic-ref", "HEAD", "refs/heads/unborn-fixture"]);
    let value = parsed(inspect_with(&f.workspace, true, true, &runner).unwrap());
    assert_eq!(
        value["observations"]["head"],
        json!({"status":"unborn","ref":"refs/heads/unborn-fixture"})
    );
}

#[test]
fn linked_and_common_split_index_artifacts_casefold_before_index_reads() {
    for linked_case in [false, true] {
        let mut f = Fixture::new("sha1");
        let common = f.checkout.join(".git");
        if linked_case {
            let linked = f._temp.path().join("linked");
            f.git(&[
                "worktree",
                "add",
                "--quiet",
                "--detach",
                linked.to_str().unwrap(),
                &f.base,
            ]);
            f.checkout = linked;
        }
        let git_dir = PathBuf::from(
            String::from_utf8(f.git(&["rev-parse", "--absolute-git-dir"]))
                .unwrap()
                .trim(),
        );
        f.git(&["update-index", "--split-index"]);
        let shared: Vec<_> = fs::read_dir(&git_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("sharedindex.")
            })
            .collect();
        assert!(!shared.is_empty());
        for path in shared {
            let renamed =
                path.with_file_name(path.file_name().unwrap().to_string_lossy().to_uppercase());
            fs::rename(path, renamed).unwrap();
        }
        struct NoIndex(GitRunner);
        impl ProbeRunner for NoIndex {
            fn run(&self, r: &ProbeRequest) -> ProbeResult {
                assert!(!r.arguments.iter().any(|a| a == "status"));
                self.0.run(r)
            }
        }
        let before = snapshot(f._temp.path());
        assert!(
            f.register(&NoIndex(GitRunner {
                calls: Cell::new(0)
            }))
            .is_err()
        );
        assert_eq!(snapshot(f._temp.path()), before);
        // A Unicode casefold sentinel in the common dir is also excluded.
        for directory in [&git_dir, &common] {
            for entry in fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("sharedindex.")
                {
                    fs::remove_file(path).unwrap();
                }
            }
        }
        fs::write(common.join("\u{17f}haredindex.stale"), b"stale").unwrap();
        let before = snapshot(f._temp.path());
        assert!(
            f.register(&NoIndex(GitRunner {
                calls: Cell::new(0)
            }))
            .is_err()
        );
        assert_eq!(snapshot(f._temp.path()), before);
    }
}

#[test]
fn raw_commit_and_root_tree_integrity_are_checked_independently() {
    struct Corrupt {
        kind: &'static str,
        runner: GitRunner,
    }
    impl ProbeRunner for Corrupt {
        fn run(&self, r: &ProbeRequest) -> ProbeResult {
            let mut result = self.runner.run(r);
            let args = &r.arguments;
            if args.len() >= 3
                && args[args.len() - 3] == "cat-file"
                && args[args.len() - 2] == self.kind
            {
                result.stdout.push(b'x');
            }
            result
        }
    }
    for kind in ["commit", "tree"] {
        let f = Fixture::new("sha1");
        let before = snapshot(&f.checkout);
        let result = f.register(&Corrupt {
            kind,
            runner: GitRunner {
                calls: Cell::new(0),
            },
        });
        assert_eq!(
            result.err().unwrap().code(),
            if kind == "commit" {
                "repository_commit_hash_mismatch"
            } else {
                "repository_tree_hash_mismatch"
            }
        );
        assert!(!f.workspace.join(AUTHORITY).exists());
        assert_eq!(snapshot(&f.checkout), before);
    }
}

#[test]
fn native_observation_rejects_bare_subdirectory_old_git_and_noncommit_bases() {
    let f = Fixture::new("sha1");
    let runner = GitRunner {
        calls: Cell::new(0),
    };
    let tree = String::from_utf8(f.git(&["rev-parse", "HEAD^{tree}"])).unwrap();
    let blob = String::from_utf8(f.git(&["rev-parse", "HEAD:file"])).unwrap();
    for base in [tree.trim(), blob.trim(), &"f".repeat(40)] {
        let mut request = f.request();
        request.base = base.to_owned();
        assert!(
            registration::register(
                &f.workspace,
                request,
                true,
                &runner,
                &registration::SystemIo
            )
            .is_err()
        );
    }
    let sub = f.checkout.join("subdirectory");
    fs::create_dir(&sub).unwrap();
    let mut request = f.request();
    request.path = sub.to_str().unwrap().to_owned();
    assert!(
        registration::register(
            &f.workspace,
            request,
            true,
            &runner,
            &registration::SystemIo
        )
        .is_err()
    );
    let bare = f._temp.path().join("bare.git");
    f.git(&["init", "--bare", "--quiet", bare.to_str().unwrap()]);
    let mut request = f.request();
    request.path = bare.to_str().unwrap().to_owned();
    assert!(
        registration::register(
            &f.workspace,
            request,
            true,
            &runner,
            &registration::SystemIo
        )
        .is_err()
    );
    struct Old;
    impl ProbeRunner for Old {
        fn run(&self, _: &ProbeRequest) -> ProbeResult {
            ProbeResult {
                status: ProbeStatus::Ok,
                returncode: Some(0),
                stdout: b"git version 2.47.9\n".to_vec(),
                stderr: Vec::new(),
                duration_ms: 0.0,
            }
        }
    }
    assert_eq!(
        f.register(&Old).err().unwrap().code(),
        "repository_git_version_unsupported"
    );
    assert!(!f.workspace.join("state/.workspace.lock").exists());
    assert!(!f.workspace.join(AUTHORITY).exists());
}
