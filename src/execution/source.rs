//! Current source observations under a caller-owned lock, never mutation authority.
use super::{Work, get, object, string, uuid};
use crate::{
    Result, error::require, ledger, storage::WorkspaceGuard, tasks, values::Value, workspace,
};
use std::path::Path;

pub(super) fn capture(
    root: &Path,
    snapshot_source_id: &Value,
    guard: &WorkspaceGuard,
    work: &mut Work,
) -> Result<Value> {
    capture_inner(
        root,
        snapshot_source_id,
        guard,
        work,
        #[cfg(test)]
        &mut || {},
    )
}

fn capture_inner(
    root: &Path,
    snapshot_source_id: &Value,
    guard: &WorkspaceGuard,
    work: &mut Work,
    #[cfg(test)] after_workspace: &mut dyn FnMut(),
) -> Result<Value> {
    uuid(snapshot_source_id)?;
    guard.validate_for(root)?;
    let state = workspace::load(root)?;
    let roster = get(state.team.value(), "agents")?.clone();
    // Preflight tasks are deliberately discarded: revision and tasks must come
    // from the same independent observation below.
    drop(state);
    #[cfg(test)]
    after_workspace();
    let mut value = match ledger::read(root)? {
        Some(view) => object(vec![
            (
                "identity",
                object(vec![
                    ("kind", string("task-ledger-v1")),
                    ("workspace_id", string(view.workspace_id())),
                    ("epoch_id", string(view.epoch_id())),
                    ("path", string("state/task-ledger")),
                ])?,
            ),
            ("revision", string(view.revision())),
            ("tasks", view.tasks().clone()),
        ])?,
        None => {
            require(!ledger::pending(root)?, "pending_task_import")?;
            let mut snapshot = tasks::capture_snapshot(root)?.report()?;
            snapshot.set_field(
                "identity",
                object(vec![
                    ("kind", string("snapshot-v1")),
                    ("source_id", snapshot_source_id.clone()),
                    ("path", string("state/tasks.json")),
                ])?,
            )?;
            snapshot
        }
    };
    value.set_field("roster", roster)?;
    super::source(&value, snapshot_source_id, work)?;
    Ok(value)
}

#[cfg(test)]
mod tests;
