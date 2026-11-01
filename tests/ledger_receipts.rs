//! Python generates authority; native receipt tests use only the library API.
#![cfg(unix)]

use coven::{
    ledger::{self, RequestBinding, RequestKey},
    values::{self, Profile, Value},
};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom},
    os::unix::{fs::MetadataExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct CorpusChild(Option<Child>);

impl Drop for CorpusChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            // SAFETY: process_group(0) gives this owned child a new group whose
            // ID is its PID. Never signal the harness's or user's process group.
            unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Entry {
    mode: u32,
    inode: u64,
    device: u64,
    mtime: (i64, i64),
    bytes: Vec<u8>,
    link: Option<PathBuf>,
}

fn inventory(root: &Path) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, path: &Path, entries: &mut BTreeMap<PathBuf, Entry>) {
        let metadata = fs::symlink_metadata(path).unwrap();
        let kind = metadata.file_type();
        entries.insert(
            path.strip_prefix(root).unwrap().to_owned(),
            Entry {
                mode: metadata.mode(),
                inode: metadata.ino(),
                device: metadata.dev(),
                mtime: (metadata.mtime(), metadata.mtime_nsec()),
                // Never open FIFOs, follow links, or inspect projection contents
                // through the ledger reader merely to prove it did no writes.
                bytes: if kind.is_file() {
                    fs::read(path).unwrap()
                } else {
                    Vec::new()
                },
                link: kind.is_symlink().then(|| fs::read_link(path).unwrap()),
            },
        );
        if kind.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), entries);
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

fn field<'a>(value: &'a Value, name: &str) -> &'a Value {
    value.as_object().unwrap().get(name).unwrap()
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    field(value, name).as_str().unwrap()
}

#[test]
fn python_history_receipts_full_replay_validation_and_read_only_lookup() {
    const CHILD_ROOT: &str = "COVEN_RECEIPT_TEST_CHILD_ROOT";
    let root = match std::env::var_os(CHILD_ROOT) {
        Some(root) => PathBuf::from(root),
        None => {
            // Parent ownership ensures even a killed FIFO reader leaves no fixtures.
            let temporary = tempfile::tempdir().unwrap();
            let mut stdout = tempfile::tempfile().unwrap();
            let mut stderr = tempfile::tempfile().unwrap();
            let mut child = CorpusChild(Some(
                Command::new(std::env::current_exe().unwrap())
                    .arg("--exact")
                    .arg("python_history_receipts_full_replay_validation_and_read_only_lookup")
                    .arg("--nocapture")
                    .env(CHILD_ROOT, temporary.path())
                    .process_group(0)
                    // Files cannot saturate a pipe if an assertion dumps a large
                    // inventory. The successful child prints only test counts.
                    .stdout(Stdio::from(stdout.try_clone().unwrap()))
                    .stderr(Stdio::from(stderr.try_clone().unwrap()))
                    .spawn()
                    .unwrap(),
            ));
            let deadline = Instant::now() + Duration::from_secs(60);
            let status = loop {
                if child.0.as_mut().unwrap().try_wait().unwrap().is_some() {
                    break Some(child.0.take().unwrap().wait_with_output().unwrap().status);
                }
                if Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(25));
            };
            // On timeout, kill the isolated group (including Python), then reap
            // before reading diagnostics or letting the parent drop the root.
            drop(child);
            stdout.seek(SeekFrom::Start(0)).unwrap();
            stderr.seek(SeekFrom::Start(0)).unwrap();
            let mut out = String::new();
            let mut err = String::new();
            stdout.read_to_string(&mut out).unwrap();
            stderr.read_to_string(&mut err).unwrap();
            print!("{out}");
            eprint!("{err}");
            assert!(
                status.is_some(),
                "receipt corpus exceeded 60-second timeout"
            );
            assert!(status.unwrap().success(), "receipt corpus child failed");
            return;
        }
    };
    let project = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("python3")
        .arg(project.join("tests/_receipt_parity.py"))
        .arg("--root")
        .arg(&root)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONINTMAXSTRDIGITS", "4300")
        .env("COVEN_MULTIPLEXER", "")
        .current_dir(project)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut counts = BTreeMap::<String, usize>::new();
    let mut summary = None;
    for line in output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        // No serde_json::Value: extension integers and signed floats are authority.
        let case = values::parse(line, Profile::Authority).unwrap();
        if text(&case, "kind") == "summary" {
            assert!(summary.replace(case).is_none());
            continue;
        }
        assert!(summary.is_none(), "cases must precede summary");
        assert_eq!(text(&case, "kind"), "case");
        let path = Path::new(text(&case, "path"));
        assert!(path.starts_with(&root));
        let status = text(&case, "status");
        let label = text(&case, "label");
        let before = inventory(path);
        let read = ledger::read(path);
        if status == "read_error" {
            assert!(read.is_err(), "{label}: corrupt tail returned a view");
        } else if status == "no_authority" {
            assert!(read.unwrap().is_none(), "{label}");
        } else {
            let view = read
                .unwrap_or_else(|error| panic!("{label}: {error:?}"))
                .unwrap_or_else(|| panic!("{label}: authority missing"));
            let key = RequestKey::parse(text(&case, "key"));
            if status == "invalid_key" {
                assert!(key.is_err(), "{label}");
            } else {
                let key = key.unwrap();
                let binding = RequestBinding::parse(field(&case, "binding").clone());
                if status == "invalid_binding" {
                    assert!(binding.is_err(), "{label}: {:?}", field(&case, "binding"));
                } else {
                    let binding = binding.unwrap_or_else(|error| panic!("{label}: {error:?}"));
                    let raw = field(&case, "binding");
                    let args = field(raw, "args");
                    let expected_revision = field(raw, "expected_revision").as_str();
                    let constructed = match text(raw, "operation") {
                        "add" => RequestBinding::add(
                            text(args, "title"),
                            field(args, "owner").as_str(),
                            text(args, "notes"),
                            expected_revision,
                        ),
                        "assign" => RequestBinding::assign(
                            text(args, "task_id"),
                            text(args, "agent"),
                            expected_revision,
                        ),
                        _ => panic!("unexpected valid operation"),
                    };
                    assert_eq!(
                        constructed.unwrap(),
                        binding,
                        "constructor identity: {label}"
                    );
                    let receipt = view.lookup(&key, &binding);
                    if status == "conflict" {
                        match receipt {
                            Err(error) => {
                                assert_eq!(error.code(), "request_key_binding_conflict", "{label}")
                            }
                            Ok(_) => panic!("{label}: changed binding accepted"),
                        }
                    } else {
                        assert_eq!(status, "result");
                        let receipt = receipt.unwrap_or_else(|error| panic!("{label}: {error:?}"));
                        let actual = receipt.map_or(Value::Null, |receipt| receipt.to_value());
                        let expected = field(&case, "expected").canonical().unwrap();
                        assert_eq!(
                            expected,
                            text(&case, "canonical").as_bytes(),
                            "Python canonical oracle: {label}"
                        );
                        assert_eq!(actual.canonical().unwrap(), expected, "{label}");
                        if let Some(receipt) = receipt {
                            assert_eq!(receipt.value().canonical().unwrap(), expected, "{label}");
                            let mut owned = receipt.to_value();
                            assert_eq!(owned.canonical().unwrap(), expected);
                            // Public Object mutation is private to the crate. Replace
                            // the owned value, then re-read the retained record.
                            assert!(matches!(owned, Value::Object(_)));
                            owned = Value::Null;
                            assert!(matches!(owned, Value::Null));
                            assert_eq!(
                                view.lookup(&key, &binding)
                                    .unwrap()
                                    .unwrap()
                                    .value()
                                    .canonical()
                                    .unwrap(),
                                expected
                            );
                        }
                        if matches!(field(&case, "fence"), Value::Bool(true)) {
                            assert_eq!(inventory(path), before, "{label}: read/lookup wrote files");
                            fs::write(path.join(ledger::MARKER), b"changed marker\n").unwrap();
                            let changed = inventory(path);
                            assert_eq!(
                                view.lookup(&key, &binding)
                                    .unwrap()
                                    .unwrap()
                                    .value()
                                    .canonical()
                                    .unwrap(),
                                expected
                            );
                            assert_eq!(
                                view.fence(path).unwrap_err().code(),
                                "task_authority_changed_during_read"
                            );
                            assert_eq!(
                                inventory(path),
                                changed,
                                "{label}: snapshot lookup or fence wrote files"
                            );
                            *counts.entry(status.to_owned()).or_default() += 1;
                            continue;
                        }
                    }
                }
            }
        }
        assert_eq!(
            inventory(path),
            before,
            "{label}: namespace/bytes/mode/inode/mtime changed"
        );
        *counts.entry(status.to_owned()).or_default() += 1;
    }
    let summary = summary.expect("Python must emit case counts");
    for (status, count) in &counts {
        assert_eq!(
            field(field(&summary, "counts"), status)
                .canonical()
                .unwrap(),
            count.to_string().as_bytes()
        );
    }
    assert_eq!(
        field(&summary, "counts")
            .as_object()
            .unwrap()
            .entries()
            .len(),
        counts.len()
    );
    let total = counts.values().sum::<usize>();
    assert_eq!(
        field(&summary, "total").canonical().unwrap(),
        total.to_string().as_bytes()
    );
    assert!(
        (50..=100).contains(&total),
        "keep the parity corpus bounded: {total}"
    );
    for status in [
        "result",
        "conflict",
        "invalid_key",
        "invalid_binding",
        "no_authority",
        "read_error",
    ] {
        assert!(counts.contains_key(status), "missing category: {status}");
    }
    println!("receipt parity: {total} cases, {counts:?}");
}

#[test]
fn typed_constructors_reject_invalid_arguments_without_a_receipt_map() {
    for title in ["", " \t", "\u{85}\u{a0}"] {
        assert!(RequestBinding::add(title, None, "", None).is_err());
    }
    for owner in ["", " \t", "\u{85}\u{a0}"] {
        assert!(RequestBinding::add("title", Some(owner), "", None).is_err());
    }
    for selector in ["", " \t", "\u{85}\u{a0}"] {
        assert!(RequestBinding::assign(selector, "agent", None).is_err());
        assert!(RequestBinding::assign("task", selector, None).is_err());
    }
    for revision in ["", "tasks-missing:v1\n", "tasks-ledger:v2:x"] {
        assert!(RequestBinding::add("title", None, "", Some(revision)).is_err());
        assert!(RequestBinding::assign("task", "agent", Some(revision)).is_err());
    }
    // Schema validity does not depend on whether any task/agent/key exists.
    assert!(RequestBinding::add("!!!", Some("unknown"), " \t", Some("tasks-missing:v1")).is_ok());
    assert!(RequestBinding::assign("unknown task", "unknown agent", None).is_ok());
}
