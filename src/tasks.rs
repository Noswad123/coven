//! Native snapshot preview. Refuse all unported authority rather than treating a
//! task projection as the source, and preserve existing command preflight guards.
use crate::{
    Error, Result, canonical,
    error::require,
    read_fs,
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

fn preflight(root: &Path) -> Result<()> {
    for relative in DIRECTORIES {
        read_fs::managed(root, Path::new(relative), true)?;
    }
    for relative in FILES {
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
    for agent in team.agent_ids()? {
        for relative in [
            format!("agents/{agent}.md"),
            format!("prompts/{agent}.md"),
            format!("prompts/{agent}.start.txt"),
        ] {
            read_fs::managed(root, Path::new(&relative), false)?;
        }
    }
    records::agents(&optional_json(
        root,
        "state/agents.json",
        Value::Array(Vec::new()),
    )?)?;
    records::lead(&optional_json(root, "state/lead.json", Value::Null)?)?;
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
    preflight(&root)?;
    let raw = optional_bytes(&root, "state/tasks.json", values::MAX_INPUT)?;
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
    authority_fence(&root)?;
    Ok(Snapshot { revision, records })
}

pub fn list_json(workspace: &Path) -> Result<String> {
    let bytes = read_snapshot(workspace)?.report()?.compact()?;
    crate::output::ascii_json(std::str::from_utf8(&bytes)?)
}

/// Normalize only CLI spelling; the library's Path APIs remain literal.
pub fn list_json_argument(workspace: &Path) -> Result<String> {
    list_json(&crate::platform::workspace_argument(workspace)?)
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
