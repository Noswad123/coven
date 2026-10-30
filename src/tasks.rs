//! Native snapshot preview. Refuse all unported authority rather than treating a
//! task projection as the source, and preserve existing command preflight guards.
use crate::{
    Error, Result, canonical,
    error::require,
    ledger, read_fs,
    records::{self, TaskRecords, TeamRecord},
    values::{self, Profile, Value},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

const DIRECTORIES: &[&str] = &[
    "agents",
    "prompts",
    "state",
    "logs",
    "checkpoints",
    "work",
    "output",
    "tmux",
    "state/task-ledger",
    "state/task-recovery",
    "state/actions",
];
const FILES: &[&str] = &[
    "coven.json",
    "coven.toml",
    "goal.md",
    "spec.md",
    "dashboard.md",
    "state/agents.json",
    "state/tasks.json",
    "state/lead.json",
    "state/.workspace.lock",
    "state/task-authority.json",
    "logs/events.jsonl",
    "logs/messages.jsonl",
    "tmux/coven.toml",
    "work/README.md",
    "output/README.md",
    "state/runtime-launch-control.json",
    "state/.runtime-transport.lock",
];

fn authority_fence(root: &Path) -> Result<()> {
    read_fs::managed(root, Path::new("state"), true)?;
    for relative in [
        "state/task-authority.json",
        "state/task-ledger",
        "state/runtime-launch-control.json",
        "state/actions",
    ] {
        match fs::symlink_metadata(root.join(relative)) {
            Ok(_) => return Err(Error("unsupported_native_authority")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn runtime_fence(root: &Path) -> Result<()> {
    read_fs::managed(root, Path::new("state"), true)?;
    for relative in ["state/runtime-launch-control.json", "state/actions"] {
        match fs::symlink_metadata(root.join(relative)) {
            Ok(_) => return Err(Error("unsupported_native_authority")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn optional_bytes(root: &Path, relative: &str, limit: usize) -> Result<Option<Vec<u8>>> {
    let path = read_fs::managed(root, Path::new(relative), false)?;
    match fs::symlink_metadata(&path) {
        Ok(_) => Ok(Some(read_fs::read_regular(&path, limit)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn optional_json(root: &Path, relative: &str, default: Value) -> Result<Value> {
    match optional_bytes(root, relative, values::MAX_INPUT)? {
        Some(raw) => values::parse(&raw, Profile::LegacySnapshot),
        None => Ok(default),
    }
}
fn checkpoint(root: &Path, relative: &str) -> Result<()> {
    if let Some(raw) = optional_bytes(root, relative, 1_048_576)? {
        std::str::from_utf8(&raw)?;
    }
    Ok(())
}

fn preflight(root: &Path, active_ledger: bool, full: bool) -> Result<()> {
    for relative in DIRECTORIES {
        read_fs::managed(root, Path::new(relative), true)?;
    }
    for relative in FILES {
        if active_ledger && *relative == "state/tasks.json" {
            continue;
        }
        read_fs::managed(root, Path::new(relative), false)?;
    }
    checkpoint(root, "checkpoints/plan-approval.md")?;
    checkpoint(root, "checkpoints/pre-delivery-review.md")?;
    let directory = root.join("checkpoints");
    if directory.exists() {
        for (index, entry) in fs::read_dir(directory)?.enumerate() {
            require(index < 4096, "unsupported_checkpoint_scan_profile")?;
            let path = entry?.path();
            if path
                .extension()
                .and_then(|text| text.to_str())
                .is_some_and(|text| text.eq_ignore_ascii_case("md"))
            {
                let stem = path
                    .file_stem()
                    .and_then(|text| text.to_str())
                    .ok_or(Error("invalid_checkpoint_id"))?;
                require(
                    !stem.is_empty()
                        && stem
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
                    "invalid_checkpoint_id",
                )?;
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| Error("unsafe_archive_path"))?;
                checkpoint(
                    root,
                    relative
                        .to_str()
                        .ok_or(Error("non_unicode_path_unsupported"))?,
                )?;
            }
        }
    }
    let team = TeamRecord::validate(optional_json(root, "coven.json", Value::Null)?)?;
    let mut configured = team.configured_multiplexer().map(str::to_owned);
    if let Some(raw) = optional_bytes(root, "coven.toml", 1_048_576)? {
        let config: toml::Value = std::str::from_utf8(&raw)?
            .parse()
            .map_err(|_| Error("invalid_workspace_toml"))?;
        if let Some(runtime) = config.get("runtime") {
            let runtime = runtime
                .as_table()
                .ok_or(Error("invalid_runtime_configuration"))?;
            for key in ["multiplexer", "opencode_command"] {
                if let Some(value) = runtime.get(key) {
                    let value = value
                        .as_str()
                        .ok_or(Error("invalid_runtime_configuration"))?;
                    require(
                        !records::trimmed(value).is_empty(),
                        "invalid_runtime_configuration",
                    )?;
                    if key == "multiplexer" {
                        configured = Some(records::trimmed(value).to_owned());
                    }
                }
            }
        }
    }
    if let Some(value) = &configured {
        records::multiplexer(value)?;
    }
    let environment = match std::env::var("COVEN_MULTIPLEXER") {
        Ok(value) if !value.is_empty() => Some(value),
        Ok(_) | Err(std::env::VarError::NotPresent) => None,
        Err(_) => return Err(Error("unsupported_multiplexer")),
    };
    records::multiplexer(
        environment
            .as_deref()
            .or(configured.as_deref())
            .unwrap_or("herdr"),
    )?;
    for agent in if full { team.agent_ids()? } else { Vec::new() } {
        for relative in [
            format!("agents/{agent}.md"),
            format!("prompts/{agent}.md"),
            format!("prompts/{agent}.start.txt"),
        ] {
            read_fs::managed(root, Path::new(&relative), false)?;
        }
    }
    if full {
        records::agents(&optional_json(
            root,
            "state/agents.json",
            Value::Array(Vec::new()),
        )?)?;
        records::lead(&optional_json(root, "state/lead.json", Value::Null)?)?;
    }
    Ok(())
}

pub struct Snapshot {
    revision: String,
    records: TaskRecords,
}
impl Snapshot {
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn records(&self) -> &TaskRecords {
        &self.records
    }
    pub fn report(self) -> Result<Value> {
        Value::object(vec![
            ("revision".to_owned(), Value::String(self.revision)),
            ("tasks".to_owned(), self.records.into_value()),
        ])
    }
}

pub fn read_snapshot(workspace: &Path) -> Result<Snapshot> {
    let root: PathBuf = read_fs::workspace(workspace)?;
    authority_fence(&root)?; // Before any projection/snapshot preflight or read.
    preflight(&root, false, true)?;
    let snapshot = capture_snapshot(&root)?;
    authority_fence(&root)?;
    Ok(snapshot)
}

fn capture_snapshot(root: &Path) -> Result<Snapshot> {
    let raw = optional_bytes(root, "state/tasks.json", values::MAX_INPUT)?;
    let revision = raw
        .as_deref()
        .map(|raw| format!("tasks-sha256:{}", canonical::digest(raw)))
        .unwrap_or_else(|| "tasks-missing:v1".to_owned());
    let value = raw
        .as_deref()
        .map(|raw| values::parse(raw, Profile::LegacySnapshot))
        .transpose()?
        .unwrap_or(Value::Array(Vec::new()));
    let records = TaskRecords::validate(value)?;
    Ok(Snapshot { revision, records })
}

struct CapturedReport {
    root: PathBuf,
    view: Option<ledger::View>,
    report: Value,
    pending: bool,
}
impl CapturedReport {
    fn finish(self) -> Result<Value> {
        runtime_fence(&self.root)?;
        if let Some(view) = &self.view {
            view.fence(&self.root)?;
        } else {
            require(
                ledger::pending(&self.root)? == self.pending,
                "task_authority_changed_during_read",
            )?;
        }
        Ok(self.report)
    }
}

fn capture_listing(root: PathBuf) -> Result<CapturedReport> {
    runtime_fence(&root)?;
    let view = ledger::read(&root)?;
    if view.is_none() {
        require(!ledger::pending(&root)?, "unsupported_native_authority")?;
    }
    preflight(&root, view.is_some(), true)?;
    let report = if let Some(view) = &view {
        view.report(&root)?
    } else {
        capture_snapshot(&root)?.report()?
    };
    Ok(CapturedReport {
        root,
        view,
        report,
        pending: false,
    })
}

fn capture_inspection(root: PathBuf) -> Result<CapturedReport> {
    runtime_fence(&root)?;
    let view = ledger::read(&root)?;
    preflight(&root, view.is_some(), false)?;
    let mut observed_pending = false;
    let report = if let Some(view) = &view {
        view.report(&root)?
    } else {
        observed_pending = ledger::pending(&root)?;
        match capture_snapshot(&root) {
            Ok(snapshot) => Value::object(vec![
                (
                    "authority".to_owned(),
                    Value::String("snapshot-v1".to_owned()),
                ),
                ("revision".to_owned(), Value::String(snapshot.revision)),
                ("tasks".to_owned(), snapshot.records.into_value()),
                ("import_pending".to_owned(), Value::Bool(observed_pending)),
            ])?,
            Err(error) if observed_pending => Value::object(vec![
                (
                    "authority".to_owned(),
                    Value::String("snapshot-v1".to_owned()),
                ),
                ("import_pending".to_owned(), Value::Bool(true)),
                (
                    "source_error".to_owned(),
                    Value::String(error.code().to_owned()),
                ),
            ])?,
            Err(error) => return Err(error),
        }
    };
    Ok(CapturedReport {
        root,
        view,
        report,
        pending: observed_pending,
    })
}

pub fn list_json(workspace: &Path) -> Result<String> {
    let report = capture_listing(read_fs::workspace(workspace)?)?
        .finish()?
        .compact()?;
    crate::output::ascii_json(std::str::from_utf8(&report)?)
}

pub fn inspect_state(workspace: &Path) -> Result<Value> {
    capture_inspection(read_fs::workspace(workspace)?)?.finish()
}

pub fn inspect_json_argument(workspace: &Path) -> Result<String> {
    let report = inspect_state(&crate::platform::workspace_argument(workspace)?)?.compact()?;
    crate::output::ascii_json(std::str::from_utf8(&report)?)
}

/// Normalize only CLI spelling; the library's Path APIs remain literal.
pub fn list_json_argument(workspace: &Path) -> Result<String> {
    list_json(&crate::platform::workspace_argument(workspace)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn minimal() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("coven.json"),
            r#"{"name":"test","agents":["operator"]}"#,
        )
        .unwrap();
        root
    }
    #[test]
    fn authority_fence_is_rechecked_not_remembered_as_absence() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("state")).unwrap();
        authority_fence(root.path()).unwrap();
        fs::write(
            root.path().join("state/task-authority.json"),
            "invalid control",
        )
        .unwrap();
        assert_eq!(
            authority_fence(root.path()).unwrap_err().code(),
            "unsupported_native_authority"
        );
    }
    #[test]
    fn captured_snapshot_never_returns_after_marker_or_runtime_publication() {
        for runtime in [false, true] {
            let root = minimal();
            let captured = capture_listing(root.path().to_owned()).unwrap();
            fs::create_dir(root.path().join("state")).unwrap();
            let relative = if runtime {
                "state/runtime-launch-control.json"
            } else {
                ledger::MARKER
            };
            fs::write(root.path().join(relative), "appeared after capture").unwrap();
            assert!(captured.finish().is_err());
        }
    }
    #[test]
    fn pending_inspection_transition_is_checked_before_report_return() {
        let root = minimal();
        fs::create_dir_all(root.path().join(ledger::DIRECTORY)).unwrap();
        let captured = capture_inspection(root.path().to_owned()).unwrap();
        fs::remove_dir(root.path().join(ledger::DIRECTORY)).unwrap();
        assert_eq!(
            captured.finish().unwrap_err().code(),
            "task_authority_changed_during_read"
        );
    }
    #[test]
    #[cfg(unix)]
    fn captured_active_listing_and_inspection_reject_changed_marker() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("control");
        let code = "from runner import coven;import sys;w=sys.argv[1];coven.main(['init',w,'--blank','--agents','operator']);r=coven.read_task_snapshot(__import__('pathlib').Path(w))[0];coven.main(['state','import-tasks',w,'--expect-revision',r,'--json'])";
        let output = std::process::Command::new("python3")
            .arg("-c")
            .arg(code)
            .arg(&workspace)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("COVEN_MULTIPLEXER", "")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let listing = capture_listing(workspace.clone()).unwrap();
        let inspection = capture_inspection(workspace.clone()).unwrap();
        fs::remove_file(workspace.join(ledger::MARKER)).unwrap();
        assert_eq!(
            listing.finish().unwrap_err().code(),
            "task_authority_changed_during_read"
        );
        assert_eq!(
            inspection.finish().unwrap_err().code(),
            "task_authority_changed_during_read"
        );
    }
}
