//! Native fixtures only: listing never launches providers or Python.
use coven::{init, ledger, storage::WorkspaceGuard, tasks};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime},
};

fn workspace() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    init::arguments(
        &root,
        &init::Options {
            blank: true,
            agents: "operator".into(),
            ..Default::default()
        },
    )
    .unwrap();
    (temp, root)
}

fn inventory(root: &Path) -> BTreeMap<PathBuf, (bool, SystemTime, Vec<u8>)> {
    fn visit(
        root: &Path,
        path: &Path,
        entries: &mut BTreeMap<PathBuf, (bool, SystemTime, Vec<u8>)>,
    ) {
        let metadata = fs::symlink_metadata(path).unwrap();
        entries.insert(
            path.strip_prefix(root).unwrap().to_owned(),
            (
                metadata.is_dir(),
                metadata.modified().unwrap(),
                if metadata.is_file() {
                    fs::read(path).unwrap()
                } else {
                    vec![]
                },
            ),
        );
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), entries);
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

fn cli(root: &Path, json: bool) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_coven"));
    command.args(["task", "list"]).arg(root);
    if json {
        command.arg("--json");
    }
    let output = command.env("COVEN_MULTIPLEXER", "").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    output
}

fn assert_readonly(root: &Path, expected: &str) -> Value {
    let before = inventory(root);
    assert_eq!(tasks::list_argument(root, false).unwrap(), expected);
    let original = tasks::list_json_argument(root).unwrap();
    assert_eq!(tasks::list_argument(root, true).unwrap(), original);
    assert_eq!(cli(root, false).stdout, format!("{expected}\n").as_bytes());
    assert_eq!(cli(root, true).stdout, format!("{original}\n").as_bytes());
    assert_eq!(inventory(root), before);
    serde_json::from_str(&original).unwrap()
}

#[test]
fn missing_and_empty_snapshot_default_to_human_without_creating_locks() {
    let (_temp, root) = workspace();
    fs::remove_file(root.join("state/.workspace.lock")).unwrap();
    fs::remove_file(root.join("state/tasks.json")).unwrap();
    let report = assert_readonly(&root, "No tasks yet");
    assert_eq!(report, json!({"revision":"tasks-missing:v1", "tasks":[]}));
    fs::write(root.join("state/tasks.json"), "[]\n").unwrap();
    let report = assert_readonly(&root, "No tasks yet");
    assert!(
        report["revision"]
            .as_str()
            .unwrap()
            .starts_with("tasks-sha256:")
    );
    assert_eq!(report["tasks"], json!([]));
    assert!(report.get("authority").is_none());
}

#[test]
fn legacy_extensions_and_raw_strings_are_not_reformatted() {
    let (_temp, root) = workspace();
    let records = json!([
        {"id":"task_9", "title":" \u{96ea}\t\n\r\u{1b}\u{0} ", "status":"custom [state]", "extra":{"legacy":true}},
        {"id":"task_1", "title":"", "status":"", "owner":null},
        {"id":"task_3", "title":"last\n", "status":"assigned", "owner":"operator"}
    ]);
    fs::write(
        root.join("state/tasks.json"),
        serde_json::to_vec(&records).unwrap(),
    )
    .unwrap();
    let report = assert_readonly(
        &root,
        "task_9 [custom [state]] unassigned -  \u{96ea}\t\n\r\u{1b}\0 \ntask_1 [] unassigned - \ntask_3 [assigned] operator - last\n",
    );
    assert_eq!(report["tasks"], records);
}

#[test]
fn owner_empty_keeps_existing_record_admission_policy() {
    let (_temp, root) = workspace();
    fs::write(
        root.join("state/tasks.json"),
        br#"[{"id":"task_1","title":"title","status":"open","owner":""}]"#,
    )
    .unwrap();
    let before = inventory(&root);
    for json in [false, true] {
        assert_eq!(
            tasks::list_argument(&root, json).unwrap_err().code(),
            "invalid_record_identifier"
        );
    }
    assert_eq!(inventory(&root), before);
}

#[test]
#[cfg(unix)]
fn active_ledger_is_authority_even_with_corrupt_or_missing_projection() {
    let (_temp, root) = workspace();
    let records = json!([{"id":"task_1", "title":"ledger\t\u{96ea}", "status":"legacy", "owner":null, "custom":17}]);
    fs::write(
        root.join("state/tasks.json"),
        serde_json::to_vec(&records).unwrap(),
    )
    .unwrap();
    let revision = tasks::read_snapshot(&root).unwrap().revision().to_owned();
    {
        let guard = WorkspaceGuard::acquire(&root, Duration::from_secs(1)).unwrap();
        let _activation = ledger::import_tasks(&root, &revision, &guard).unwrap();
    }
    fs::remove_file(root.join("state/.workspace.lock")).unwrap();
    for missing in [false, true] {
        if missing {
            fs::remove_file(root.join("state/tasks.json")).unwrap();
        } else {
            fs::write(root.join("state/tasks.json"), b"not JSON\xff").unwrap();
        }
        let report = assert_readonly(&root, "task_1 [legacy] unassigned - ledger\t\u{96ea}");
        assert_eq!(report["tasks"], records);
        assert_eq!(report["authority"], "task-ledger-v1");
        assert_eq!(report["sequence"], 0);
        for key in [
            "revision",
            "workspace_id",
            "epoch_id",
            "frame_digest",
            "projection",
        ] {
            assert!(report.get(key).is_some(), "{key}");
        }
    }
}

#[test]
fn unsupported_and_pending_controls_are_not_admitted_by_human_mode() {
    for relative in [
        "state/runtime-launch-control.json",
        "state/actions",
        "state/task-authority.json",
        "state/task-ledger",
    ] {
        let (_temp, root) = workspace();
        let path = root.join(relative);
        if matches!(relative, "state/actions" | "state/task-ledger") {
            fs::create_dir_all(path).unwrap();
        } else {
            fs::write(path, "{}").unwrap();
        }
        let before = inventory(&root);
        let json_error = tasks::list_argument(&root, true).unwrap_err().code();
        assert_eq!(
            tasks::list_argument(&root, false).unwrap_err().code(),
            json_error,
            "{relative}"
        );
        assert_eq!(inventory(&root), before);
    }
}
