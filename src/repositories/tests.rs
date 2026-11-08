use super::*;
use crate::{
    process::{ProbeRequest, ProbeResult, ProbeRunner},
    storage::{Delivery, WorkspaceGuard},
};
use std::{cell::Cell, fs, time::Duration};

struct Never;
impl ProbeRunner for Never {
    fn run(&self, _: &ProbeRequest) -> ProbeResult {
        panic!("source-free operation invoked Git")
    }
}
fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("coven.json"),
        b"invalid marker contents\xff",
    )
    .unwrap();
    fs::create_dir(root.path().join("state")).unwrap();
    fs::write(root.path().join("state/tasks.json"), b"broken").unwrap();
    root
}
fn fixture() -> record::Authority {
    let value = json!({"schema_version":1,"kind":"repository-registration","registration_id":"12345678-1234-1234-1234-123456789abc",
        "canonical_root":"/deleted","git_dir":"/deleted/.git","common_dir":"/deleted/.git",
        "filesystem":{"root":{"device":1,"inode":2},"git_dir":{"device":1,"inode":3},"common_dir":{"device":1,"inode":3}},
        "object_format":"sha1","base_commit":"a".repeat(40),"base_tree":"b".repeat(40),"delivery_target":"refs/heads/result",
        "registered_at":"2026-11-02T12:00:00+00:00","original_request":{"path":"/deleted","base":"a".repeat(40),
        "delivery_target":"refs/heads/result","expected_revision":UNREGISTERED}});
    record::Authority::decode(serde_json::to_vec(&value).unwrap()).unwrap()
}
fn parsed(output: TaskOutput) -> Value {
    serde_json::from_str(&output.stdout).unwrap()
}
fn fixture_value(authority: &record::Authority) -> Value {
    serde_json::from_slice(&authority.bytes).unwrap()
}

#[test]
fn pure_inspection_absence_corruption_and_exact_revision() {
    let root = workspace();
    assert_eq!(
        parsed(inspect_with(root.path(), true, true, &Never).unwrap())["revision"],
        UNREGISTERED
    );
    assert!(!root.path().join("state/.workspace.lock").exists());
    let authority = fixture();
    let target = root.path().join(AUTHORITY);
    fs::write(&target, &authority.bytes).unwrap();
    let result = parsed(inspect_with(root.path(), false, true, &Never).unwrap());
    assert_eq!(result["revision"], authority.revision());
    assert_eq!(result["registration"], fixture_value(&authority));
    fs::write(
        &target,
        serde_json::to_vec_pretty(&fixture_value(&authority)).unwrap(),
    )
    .unwrap();
    assert_ne!(
        parsed(inspect_with(root.path(), false, true, &Never).unwrap())["revision"],
        result["revision"]
    );
    for bytes in [
        b"null".to_vec(),
        b"{}".to_vec(),
        b"\xff".to_vec(),
        b"{\"x\":1,\"x\":2}".to_vec(),
        vec![b' '; MAX_RECORD + 1],
    ] {
        fs::write(&target, bytes).unwrap();
        let output = inspect_with(root.path(), true, false, &Never).unwrap();
        assert_eq!(output.exit_code, 1);
        let value = parsed(output);
        assert_eq!(value["status"], "unavailable");
        assert!(value["revision"].is_null());
        assert!(value.get("registration").is_none());
    }
    assert!(!root.path().join("state/.workspace.lock").exists());
}

#[test]
fn record_closed_types_integers_and_original_request() {
    let authority = fixture();
    for (key, value) in [
        ("schema_version", json!(true)),
        ("schema_version", json!(1.0)),
        ("extra", json!(1)),
        ("registered_at", json!("2026-02-30T00:00:00Z")),
        ("canonical_root", json!("/a/../b")),
        ("registration_id", json!("12345678123412341234123456789abc")),
    ] {
        let mut record = fixture_value(&authority);
        record[key] = value;
        assert!(
            record::Authority::decode(serde_json::to_vec(&record).unwrap()).is_err(),
            "{key}"
        );
    }
    for value in [json!(-1), json!(1.0), json!(true), json!(null)] {
        let mut record = fixture_value(&authority);
        record["filesystem"]["root"]["inode"] = value;
        assert!(record::Authority::decode(serde_json::to_vec(&record).unwrap()).is_err());
    }
    let mut raw = authority.bytes.clone();
    raw.extend_from_slice(&vec![b' '; MAX_RECORD - raw.len()]);
    assert!(record::Authority::decode(raw).is_ok());
}

#[test]
fn immutable_source_free_replay_lock_failure_and_confirmation_faults() {
    let root = workspace();
    let authority = fixture();
    fs::write(root.path().join(AUTHORITY), &authority.bytes).unwrap();
    let request = authority.record.original_request.clone();
    let output = registration::register(
        root.path(),
        request.clone(),
        true,
        &Never,
        &registration::SystemIo,
    )
    .unwrap();
    assert_eq!(output.exit_code, 0);
    assert_eq!(parsed(output)["replayed"], true);
    let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap();
    struct Busy;
    impl registration::RegistrationIo for Busy {
        fn lock(&self, root: &Path) -> Result<WorkspaceGuard> {
            WorkspaceGuard::acquire(root, Duration::ZERO)
        }
    }
    let output = registration::register(root.path(), request.clone(), true, &Never, &Busy).unwrap();
    assert_eq!(output.exit_code, 2);
    assert_eq!(parsed(output)["registration"], fixture_value(&authority));
    drop(guard);
    struct Fault {
        reads: Cell<usize>,
    }
    impl registration::RegistrationIo for Fault {
        fn read(&self, root: &Path) -> Result<Option<record::Authority>> {
            self.reads.set(self.reads.get() + 1);
            if self.reads.get() > 1 {
                Err(crate::Error("injected"))
            } else {
                record::read(root)
            }
        }
        fn replace(&self, _: &WorkspaceGuard, _: &[u8]) -> Result<Delivery> {
            panic!("immutable")
        }
    }
    let output = registration::register(
        root.path(),
        request.clone(),
        false,
        &Never,
        &Fault {
            reads: Cell::new(0),
        },
    )
    .unwrap();
    assert_eq!(output.exit_code, 2);
    assert_eq!(parsed(output)["revision"], authority.revision());
    let mut changed = request;
    changed.expected_revision = authority.revision();
    assert!(
        registration::register(root.path(), changed, true, &Never, &registration::SystemIo)
            .is_err()
    );
}

#[test]
fn stale_cas_and_structure_fail_before_git_or_lock() {
    let root = workspace();
    let request = record::Request::new(
        Path::new("/missing"),
        &"a".repeat(40),
        "refs/heads/main",
        &format!("repository-sha256:{}", "0".repeat(64)),
    )
    .unwrap();
    assert!(
        registration::register(root.path(), request, true, &Never, &registration::SystemIo)
            .is_err()
    );
    assert!(!root.path().join("state/.workspace.lock").exists());
    let request = record::Request::new(
        Path::new("./no-checkout-lookup"),
        &"a".repeat(40),
        "refs/heads/main",
        UNREGISTERED,
    )
    .unwrap();
    assert_eq!(
        request.path,
        std::env::current_dir()
            .unwrap()
            .join("no-checkout-lookup")
            .to_str()
            .unwrap()
    );
    for path in ["", "alias/../checkout", "a\0b"] {
        assert!(
            record::Request::new(
                Path::new(path),
                &"a".repeat(40),
                "refs/heads/main",
                UNREGISTERED
            )
            .is_err()
        );
    }
}

#[test]
fn reports_are_ascii_compact_or_pretty_without_parent_newline() {
    let mut value = fixture_value(&fixture());
    value["delivery_target"] = json!("refs/heads/\u{1f98a}");
    value["original_request"]["delivery_target"] = value["delivery_target"].clone();
    let authority = record::Authority::decode(serde_json::to_vec(&value).unwrap()).unwrap();
    let root = workspace();
    fs::write(root.path().join(AUTHORITY), &authority.bytes).unwrap();
    for compact in [false, true] {
        let output = inspect_with(root.path(), false, compact, &Never).unwrap();
        assert!(output.stdout.is_ascii());
        assert!(!output.stdout.ends_with('\n'));
        assert!(output.stdout.contains("\\ud83e\\udd8a"));
        assert_eq!(output.stdout.contains('\n'), !compact);
        assert_eq!(parsed(output)["registration"], value);
    }
}

#[cfg(unix)]
#[test]
fn symlink_authority_and_state_are_unavailable_without_creation() {
    use std::os::unix::fs::symlink;
    let root = workspace();
    symlink("missing", root.path().join(AUTHORITY)).unwrap();
    assert_eq!(
        inspect_with(root.path(), false, true, &Never)
            .unwrap()
            .exit_code,
        1
    );
    fs::remove_file(root.path().join(AUTHORITY)).unwrap();
    fs::remove_file(root.path().join("state/tasks.json")).unwrap();
    fs::remove_dir(root.path().join("state")).unwrap();
    let other = tempfile::tempdir().unwrap();
    symlink(other.path(), root.path().join("state")).unwrap();
    assert_eq!(
        inspect_with(root.path(), false, true, &Never)
            .unwrap()
            .exit_code,
        1
    );
    assert_eq!(fs::read_dir(other.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[path = "real_tests.rs"]
mod real;

#[path = "oracle_tests.rs"]
mod oracle;
