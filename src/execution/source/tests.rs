use super::*;
use crate::{
    canonical,
    values::{self, Profile},
};
use std::{fs, process::Command, time::Duration};

fn fixture() -> (tempfile::TempDir, WorkspaceGuard) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("coven.json"),
        br#"{"name":"test","agents":["b","a"],"unknown":{"preserved":true}}"#,
    )
    .unwrap();
    fs::create_dir(dir.path().join("state")).unwrap();
    fs::write(
        dir.path().join("state/.workspace.lock"),
        b"existing lock metadata",
    )
    .unwrap();
    let guard = WorkspaceGuard::existing(dir.path(), Duration::ZERO).unwrap();
    (dir, guard)
}

fn captured(root: &Path, guard: &WorkspaceGuard) -> Result<Value> {
    capture(root, &string("a".repeat(32)), guard, &mut Work::default())
}

fn write(root: &Path, relative: &str, bytes: impl AsRef<[u8]>) {
    fs::write(root.join(relative), bytes).unwrap();
}

fn oracle(root: &Path) -> Vec<u8> {
    let output = Command::new("python3")
        .args([
            "-B",
            "-c",
            include_str!("oracle.py"),
            root.to_str().unwrap(),
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn exact_source_canonical_extensions_signed_zero_bigints_and_unknown_fields() {
    let (dir, guard) = fixture();
    let raw = br#" [ {"id":"task","title":"Title","status":"custom","owner":"outside","extra":{"z":-0.0,"big":123456789012345678901234567890,"unicode":"\u03bb\ud83e\udd8a","float":1e-7,"nested":[null,true]}} ] "#;
    write(dir.path(), "state/tasks.json", raw);
    let value = captured(dir.path(), &guard).unwrap();
    assert_eq!(value.canonical().unwrap(), oracle(dir.path()));
    assert_eq!(
        get(&value, "roster").unwrap().compact().unwrap(),
        br#"["b","a"]"#
    );
    assert_eq!(
        get(&value, "revision").unwrap().as_str(),
        Some(format!("tasks-sha256:{}", canonical::digest(raw)).as_str())
    );
    assert_eq!(get(&value, "identity").unwrap().canonical().unwrap(), br#"{"kind":"snapshot-v1","path":"state/tasks.json","source_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#);
}

#[test]
fn raw_revision_distinguishes_missing_whitespace_empty_array_and_blank_file() {
    let (dir, guard) = fixture();
    let missing = captured(dir.path(), &guard).unwrap();
    assert_eq!(
        get(&missing, "revision").unwrap().as_str(),
        Some("tasks-missing:v1")
    );
    for raw in [b"[]".as_slice(), b" \n[]\t"] {
        write(dir.path(), "state/tasks.json", raw);
        let value = captured(dir.path(), &guard).unwrap();
        assert_eq!(
            get(&value, "revision").unwrap().as_str(),
            Some(format!("tasks-sha256:{}", canonical::digest(raw)).as_str())
        );
        assert_eq!(value.canonical().unwrap(), oracle(dir.path()));
    }
    for raw in [b"".as_slice(), b" \n\t"] {
        write(dir.path(), "state/tasks.json", raw);
        assert!(captured(dir.path(), &guard).is_err());
    }
}

#[test]
fn independent_snapshot_observation_pairs_fresh_tasks_and_revision() {
    let (dir, guard) = fixture();
    let raw = br#"[{"id":"fresh","title":"Fresh","status":"pending"}]"#;
    let value = capture_inner(
        dir.path(),
        &string("a".repeat(32)),
        &guard,
        &mut Work::default(),
        &mut || {
            write(dir.path(), "state/tasks.json", raw);
        },
    )
    .unwrap();
    assert_eq!(value.canonical().unwrap(), oracle(dir.path()));
    assert_eq!(
        get(&value, "tasks").unwrap().canonical().unwrap(),
        values::parse(raw, Profile::LegacySnapshot)
            .unwrap()
            .canonical()
            .unwrap()
    );
    assert_eq!(value.as_object().unwrap().entries().len(), 4);
}

#[test]
fn independent_ledger_selection_does_not_reuse_workspace_tasks() {
    let (dir, guard) = fixture();
    let value = capture_inner(
        dir.path(),
        &string("a".repeat(32)),
        &guard,
        &mut Work::default(),
        &mut || {
            write(
                dir.path(),
                "state/tasks.json",
                br#"[{"id":"new","title":"New","status":"pending"}]"#,
            );
            let revision = tasks::capture_snapshot(dir.path())
                .unwrap()
                .revision()
                .to_owned();
            let report = ledger::import_tasks(dir.path(), &revision, &guard).unwrap();
            assert_eq!(report.durability(), ledger::ActivationDurability::Confirmed);
            assert_eq!(
                report.view().tasks().canonical().unwrap(),
                br#"[{"id":"new","status":"pending","title":"New"}]"#
            );
        },
    )
    .unwrap();
    assert_eq!(
        get(get(&value, "identity").unwrap(), "kind")
            .unwrap()
            .as_str(),
        Some("task-ledger-v1")
    );
    assert_eq!(value.canonical().unwrap(), oracle(dir.path()));
}

#[test]
fn active_ledger_ignores_corrupt_directory_and_symlink_projection() {
    let (dir, guard) = fixture();
    let report = ledger::import_tasks(dir.path(), "tasks-missing:v1", &guard).unwrap();
    assert_eq!(report.durability(), ledger::ActivationDurability::Confirmed);
    assert!(report.view().tasks().as_array().unwrap().is_empty());
    let expected = captured(dir.path(), &guard).unwrap().canonical().unwrap();
    write(dir.path(), "state/tasks.json", b"not json");
    assert_eq!(
        captured(dir.path(), &guard).unwrap().canonical().unwrap(),
        expected
    );
    fs::remove_file(dir.path().join("state/tasks.json")).unwrap();
    fs::create_dir(dir.path().join("state/tasks.json")).unwrap();
    assert_eq!(
        captured(dir.path(), &guard).unwrap().canonical().unwrap(),
        expected
    );
    fs::remove_dir(dir.path().join("state/tasks.json")).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            "/definitely/not/source",
            dir.path().join("state/tasks.json"),
        )
        .unwrap();
        assert_eq!(
            captured(dir.path(), &guard).unwrap().canonical().unwrap(),
            expected
        );
    }
}

#[test]
fn pending_import_never_falls_back_including_after_workspace_load() {
    let (dir, guard) = fixture();
    let error = capture_inner(
        dir.path(),
        &string("a".repeat(32)),
        &guard,
        &mut Work::default(),
        &mut || {
            fs::create_dir(dir.path().join("state/task-ledger")).unwrap();
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "pending_task_import");
    assert_eq!(
        captured(dir.path(), &guard).unwrap_err().code(),
        "pending_task_import"
    );
}

#[test]
fn full_workspace_preflight_rejects_config_agents_lead_controls_and_checkpoints() {
    for (name, bytes) in [
        (
            "coven.toml",
            b"[runtime]\nmultiplexer='unsupported'".as_slice(),
        ),
        ("coven.toml", b"[runtime]\nopencode_command=''"),
        ("state/agents.json", b"null"),
        ("state/lead.json", b"[]"),
        ("state/runtime-launch-control.json", b"null"),
        ("checkpoints/invalid name.md", b"ok"),
        ("checkpoints/plan-approval.md", b"\xff"),
        ("state/tasks.json", b"null"),
        ("coven.json", b"null"),
    ] {
        let (dir, guard) = fixture();
        fs::create_dir(dir.path().join("checkpoints")).unwrap();
        write(dir.path(), name, bytes);
        assert!(captured(dir.path(), &guard).is_err(), "{name}");
    }
}

#[test]
fn valid_runtime_control_is_not_a_runtime_authority_fence() {
    let (dir, guard) = fixture();
    let location = crate::platform::workspace_location(dir.path()).unwrap();
    let control = serde_json::json!({"version":1,"action":"runtime_launch","workspace_id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","epoch_id":"cccccccccccccccccccccccccccccccc","location":{"root":location.root,"device":location.device,"inode":location.inode},"head":null,"claims":{}});
    write(
        dir.path(),
        "state/runtime-launch-control.json",
        serde_json::to_vec(&control).unwrap(),
    );
    assert!(captured(dir.path(), &guard).is_ok());
}

#[test]
fn uuid_and_matching_guard_precede_workspace_source_io() {
    let (dir, guard) = fixture();
    let other = tempfile::tempdir().unwrap();
    write(dir.path(), "coven.json", b"null");
    for id in ["", "Aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "../invalid"] {
        assert_eq!(
            capture(other.path(), &string(id), &guard, &mut Work::default())
                .unwrap_err()
                .code(),
            "invalid_execution_uuid"
        );
    }
    assert_eq!(
        captured(other.path(), &guard).unwrap_err().code(),
        "invalid_workspace_lock_owner"
    );
    assert!(!other.path().join("state").exists());
}

#[test]
fn source_work_budget_uses_provided_work_without_reset() {
    let (dir, guard) = fixture();
    let size = captured(dir.path(), &guard)
        .unwrap()
        .canonical()
        .unwrap()
        .len();
    let mut work = Work(super::super::MAX_WORK - size);
    capture(dir.path(), &string("a".repeat(32)), &guard, &mut work).unwrap();
    assert_eq!(work.0, super::super::MAX_WORK);
    assert_eq!(
        capture(dir.path(), &string("a".repeat(32)), &guard, &mut work)
            .unwrap_err()
            .code(),
        "unsupported_execution_work_profile"
    );
}

#[test]
fn borrowed_guard_rejects_wrong_thread_before_filesystem_access() {
    let (dir, guard) = fixture();
    let address = &guard as *const WorkspaceGuard as usize;
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                // Test-only bypass of !Send: the scoped thread reads immutable guard
                // fields solely to exercise the dynamic owner check; owner stays alive.
                let guard = unsafe { &*(address as *const WorkspaceGuard) };
                assert_eq!(
                    captured(dir.path(), guard).unwrap_err().code(),
                    "invalid_workspace_lock_owner"
                );
            })
            .join()
            .unwrap();
    });
}

#[cfg(unix)]
#[test]
fn inherited_guard_rejects_wrong_process_before_filesystem_access() {
    let (dir, guard) = fixture();
    let id = string("a".repeat(32));
    // The child only follows the allocation-free UUID/owner refusal path and
    // exits without running destructors or unlocking the parent's descriptor.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        let refused = capture(dir.path(), &id, &guard, &mut Work::default())
            .is_err_and(|error| error.code() == "invalid_workspace_lock_owner");
        unsafe { libc::_exit(if refused { 0 } else { 1 }) };
    }
    let mut status = 0;
    assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 0);
    assert!(captured(dir.path(), &guard).is_ok());
}

#[test]
fn strict_source_rejects_nonfinite_and_surrogate_extensions() {
    for extra in ["NaN", "Infinity", r#""\ud800""#] {
        let (dir, guard) = fixture();
        write(
            dir.path(),
            "state/tasks.json",
            format!(r#"[{{"id":"t","title":"T","status":"pending","extra":{extra}}}]"#),
        );
        assert!(captured(dir.path(), &guard).is_err(), "{extra}");
    }
}

#[test]
fn active_source_digest_also_charges_the_same_command_work() {
    let (dir, guard) = fixture();
    let report = ledger::import_tasks(dir.path(), "tasks-missing:v1", &guard).unwrap();
    assert_eq!(report.durability(), ledger::ActivationDurability::Confirmed);
    assert!(report.view().tasks().as_array().unwrap().is_empty());
    let value = captured(dir.path(), &guard).unwrap();
    let source_bytes = value.canonical().unwrap().len();
    let task_bytes = get(&value, "tasks").unwrap().canonical().unwrap().len();
    let mut work = Work(super::super::MAX_WORK - source_bytes - task_bytes);
    capture(dir.path(), &string("a".repeat(32)), &guard, &mut work).unwrap();
    assert_eq!(work.0, super::super::MAX_WORK);
    let mut work = Work(super::super::MAX_WORK - source_bytes);
    assert_eq!(
        capture(dir.path(), &string("a".repeat(32)), &guard, &mut work)
            .unwrap_err()
            .code(),
        "unsupported_execution_work_profile"
    );
}

#[test]
fn full_workspace_preflight_precedes_pending_refusal_and_keeps_input_limit() {
    let (dir, guard) = fixture();
    fs::create_dir(dir.path().join("state/task-ledger")).unwrap();
    write(
        dir.path(),
        "coven.toml",
        "[runtime]\nmultiplexer='unsupported'",
    );
    assert_eq!(
        captured(dir.path(), &guard).unwrap_err().code(),
        "unsupported_multiplexer"
    );
    fs::remove_file(dir.path().join("coven.toml")).unwrap();
    write(
        dir.path(),
        "state/tasks.json",
        vec![b' '; values::MAX_INPUT + 1],
    );
    assert_eq!(
        captured(dir.path(), &guard).unwrap_err().code(),
        "unsupported_checkpoint_workspace_input_profile"
    );
}

#[test]
fn protected_existing_workspace_observation_preserves_lock_and_control_files() {
    let (dir, guard) = fixture();
    let lock = fs::read(dir.path().join("state/.workspace.lock")).unwrap();
    let team = fs::read(dir.path().join("coven.json")).unwrap();
    let before = fs::metadata(dir.path().join("state/.workspace.lock"))
        .unwrap()
        .modified()
        .unwrap();
    captured(dir.path(), &guard).unwrap();
    assert_eq!(
        fs::read(dir.path().join("state/.workspace.lock")).unwrap(),
        lock
    );
    assert_eq!(
        fs::metadata(dir.path().join("state/.workspace.lock"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
    assert_eq!(fs::read(dir.path().join("coven.json")).unwrap(), team);
    assert!(!dir.path().join("state/execution").exists());
    assert!(!dir.path().join("state/task-ledger").exists());
    assert!(!dir.path().join("state/tasks.json").exists());
}
