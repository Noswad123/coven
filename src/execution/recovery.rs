//! Explicit maintenance without source loading, orphan parsing, or promotion.
use super::*;
use crate::{platform, storage::WorkspaceGuard, task_mutation::TaskOutput};
use std::{io, time::Duration};

#[derive(Clone, Copy, Debug)]
pub enum RecoveryMode {
    ConfirmAuthority,
    ArchivePending,
}

trait Io {
    fn lock(&self, root: &Path) -> Result<WorkspaceGuard> {
        WorkspaceGuard::execution_maintenance(root, Duration::from_secs(5))
    }
    fn uuid(&self) -> Result<String> {
        platform::random_uuid_hex()
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }
    fn sync_state(&self, state: &Path) -> io::Result<()> {
        platform::sync_directory(state)
    }
}
struct SystemIo;
impl Io for SystemIo {}

pub fn recover_arguments(
    workspace: &Path,
    mode: RecoveryMode,
    expected: Option<&str>,
    json: bool,
) -> Result<TaskOutput> {
    recover_with(workspace, mode, expected, json, &SystemIo)
}

fn active_revision(revision: &str) -> bool {
    // This is an unstored API selector: inspect the fixed shape without allocating
    // a vector proportional to an arbitrarily long malformed caller string.
    let mut parts = revision.split(':');
    match (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) {
        (Some(kind), Some(authority), Some(generation), Some(digest), None) => {
            matches!(kind, "execution-control-v1" | "execution-control-v2")
                && hex(authority, 32)
                && decimal(generation, false)
                && hex(digest, 64)
        }
        _ => false,
    }
}

fn render(value: &Value, json: bool) -> Result<TaskOutput> {
    let bytes = if json {
        value.compact()?
    } else {
        value.storage()?
    };
    let text = std::str::from_utf8(&bytes)?;
    Ok(TaskOutput {
        stdout: crate::output::ascii_json(text.strip_suffix('\n').unwrap_or(text))?,
        warnings: Vec::new(),
        exit_code: 0,
    })
}

fn recover_with(
    workspace: &Path,
    mode: RecoveryMode,
    expected: Option<&str>,
    json: bool,
    io: &impl Io,
) -> Result<TaskOutput> {
    if matches!(mode, RecoveryMode::ConfirmAuthority) {
        require(
            expected.is_some_and(active_revision),
            "confirmation_requires_active_execution_revision",
        )?;
    }
    platform::ensure_supported()?;
    let root = platform::workspace_argument(workspace)?.canonicalize()?;
    require(root.is_dir(), "invalid_execution_workspace")?;
    let guard = io.lock(&root)?;
    guard.validate_for(&root)?;
    match mode {
        RecoveryMode::ConfirmAuthority => {
            let (_, selected) = read_typed(&root)?.ok_or(Error("execution_revision_conflict"))?;
            require(
                expected == Some(selected.replayed.revision.as_str()),
                "execution_revision_conflict",
            )?;
            let output = render(
                &object(vec![
                    ("outcome", string("confirmed")),
                    ("revision", string(&selected.replayed.revision)),
                    ("authorization", string("none")),
                    ("dispatchable", Value::Bool(false)),
                ])?,
                json,
            )?;
            super::confirmation::confirm(&root, &selected, &guard)?;
            Ok(output)
        }
        RecoveryMode::ArchivePending => {
            require(
                expected.is_none(),
                "archive_does_not_accept_execution_revision",
            )?;
            let pending = read_fs::managed(&root, Path::new("state/execution"), true)?;
            let control =
                read_fs::managed(&root, Path::new("state/execution/control.json"), false)?;
            absent(&control, "cannot_archive_present_execution_control")?;
            require(pending.is_dir(), "no_pending_execution_directory")?;
            let relative = format!("state/execution-pending-archive-{}", io.uuid()?);
            let target = root.join(&relative);
            absent(&target, "execution_archive_target_exists")?;
            let state = read_fs::managed(&root, Path::new("state"), true)?;
            let path = read_fs::path_text(&target)?;
            let output = render(
                &object(vec![
                    ("outcome", string("archived_pending")),
                    ("archive_path", string(&path)),
                    ("revision", string(DISABLED)),
                    ("authorization", string("none")),
                    ("dispatchable", Value::Bool(false)),
                ])?,
                json,
            )?;
            // Prepare both deliveries before the rename. No late formatting or
            // reread may erase the known destination or suggest a rollback.
            let quoted = crate::output::ascii_json(std::str::from_utf8(&string(path).compact()?)?)?;
            let diagnostic = format!(
                "[ERROR] pending_archive_durability_uncertain: pending directory may already have been renamed to {quoted}; no rollback performed"
            );
            require(
                diagnostic.len() <= values::MAX_OUTPUT,
                "unsupported_json_output_profile",
            )?;
            let failure = TaskOutput {
                stdout: String::new(),
                warnings: vec![diagnostic],
                exit_code: 1,
            };
            io.rename(&pending, &target)?;
            if io.sync_state(&state).is_err() {
                return Ok(failure);
            }
            Ok(output)
        }
    }
}

fn absent(path: &Path, present: &'static str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(Error(present)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests;
