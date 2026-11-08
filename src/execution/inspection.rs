//! Read-only inspection. Replay reads retain their individual replay budgets;
//! source, projections and final report admission share one assessment budget.
use super::*;
use crate::{storage::WorkspaceGuard, task_mutation::TaskOutput};
use std::{path::PathBuf, time::Duration};

mod commands;
mod projection;
pub use commands::{
    attempt_inspect_arguments, attempt_list_arguments, graph_export_arguments,
    graph_inspect_arguments, inspect_arguments,
};

const NO_LOCK: &str =
    "Coherent source assessment requires the existing workspace lock; none was created";

// Only explicit native bounds/encoding limitations are fatal. In particular,
// unsupported config versions and strict-source JSON extensions are diagnostics.
pub(super) fn profile_failure(error: Error) -> bool {
    matches!(
        error.code(),
        "handoff_size_limit"
            | "execution_cancellation_capacity_exceeded"
            | "execution_intent_size_limit"
            | "execution_source_size_limit"
            | "graph_size_limit"
            | "graph_edge_limit"
            | "unsupported_json_size_profile"
            | "unsupported_json_depth_profile"
            | "unsupported_json_node_profile"
            | "unsupported_json_array_profile"
            | "unsupported_json_object_profile"
            | "unsupported_json_string_profile"
            | "unsupported_json_output_profile"
            | "unsupported_integer_profile"
            | "unsupported_number_token_profile"
            | "unsupported_execution_history_profile"
            | "unsupported_execution_timestamp_profile"
            | "unsupported_execution_work_profile"
            | "unsupported_action_input_profile"
            | "unsupported_action_encoding_profile"
            | "unsupported_action_work_profile"
            | "unsupported_checkpoint_input_profile"
            | "unsupported_checkpoint_scan_profile"
            | "unsupported_checkpoint_toml_scalar_profile"
            | "unsupported_checkpoint_workspace_input_profile"
            | "unsupported_ledger_directory_profile"
            | "unsupported_ledger_frame_size"
            | "unsupported_ledger_history_bytes"
            | "unsupported_ledger_history_profile"
            | "unsupported_ledger_receipt_bytes"
            | "unsupported_ledger_task_count"
            | "unsupported_projection_profile"
            | "unsupported_recovery_size"
            | "unsupported_replacement_size"
            | "unsupported_task_response_profile"
            | "unsupported_platform"
            | "unsupported_named_user_expansion"
            | "non_unicode_path_unsupported"
            | "float_encoding_failed"
            | "report_encoding_failed"
    )
}

trait Io {
    fn read(&self, root: &Path) -> Result<Option<(Value, Validated)>> {
        read_typed(root)
    }
    fn lock(&self, root: &Path) -> Result<Option<WorkspaceGuard>> {
        existing_lock(root, Duration::from_secs(5))
    }
    fn source(
        &self,
        root: &Path,
        snapshot: &Value,
        guard: &WorkspaceGuard,
        work: &mut Work,
    ) -> Result<Value> {
        super::source::capture(root, snapshot, guard, work)
    }
}
struct SystemIo;
impl Io for SystemIo {}

fn existing_lock(root: &Path, timeout: Duration) -> Result<Option<WorkspaceGuard>> {
    let path = read_fs::managed(root, Path::new("state/.workspace.lock"), false)?;
    match fs::symlink_metadata(path) {
        Ok(_) => WorkspaceGuard::existing(root, timeout).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn root_argument(path: &Path) -> Result<PathBuf> {
    crate::platform::ensure_supported()?;
    let root = crate::platform::workspace_argument(path)?.canonicalize()?;
    require(root.is_dir(), "invalid_execution_workspace")?;
    root.to_str().ok_or(Error("non_unicode_path_unsupported"))?;
    Ok(root)
}

fn disabled() -> Result<Value> {
    object(vec![
        ("status", string("disabled")),
        ("revision", string(DISABLED)),
    ])
}

// Returns None only for established active authority, never for invalid control.
fn inspect_control(root: &Path, io: &impl Io) -> Result<Option<Value>> {
    match io.read(root) {
        Ok(Some(_)) => Ok(None),
        Ok(None) => disabled().map(Some),
        Err(error) if profile_failure(error.clone()) => Err(error),
        Err(error) => object(vec![
            (
                "status",
                string(if error.code() == "pending_execution_authority" {
                    "pending"
                } else {
                    "unavailable"
                }),
            ),
            ("error", string(error.code())),
        ])
        .map(Some),
    }
}

fn minimal(execution: Value) -> Result<Value> {
    object(vec![
        ("execution", execution),
        ("authorization", string("none")),
        ("dispatchable", Value::Bool(false)),
        (
            "blockers",
            Value::Array(
                [
                    "PREPARATION_ONLY",
                    "NATIVE_ADMISSION_NOT_IMPLEMENTED",
                    "TASK_EXECUTION_AUTHORIZATION_NOT_IMPLEMENTED",
                ]
                .into_iter()
                .map(string)
                .collect(),
            ),
        ),
    ])
}

fn inspect(root: &Path, io: &impl Io, work: &mut Work) -> Result<Value> {
    if let Some(state) = inspect_control(root, io)? {
        return minimal(state);
    }
    // Deliberately outside the coordination downgrade: the second read is hard.
    let Some(mut view) = io.read(root)? else {
        return minimal(disabled()?);
    };
    let mut source = None;
    let mut reason = Some(NO_LOCK.to_owned());
    let coordinated = (|| -> Result<Option<Value>> {
        let Some(guard) = io.lock(root)? else {
            return Ok(None);
        };
        let Some(locked) = io.read(root)? else {
            return disabled().map(Some);
        };
        view = locked;
        match io.source(root, get(&view.0, "snapshot_source_id")?, &guard, work) {
            Ok(captured) => {
                source = Some(captured);
                reason = None;
            }
            Err(error) if profile_failure(error.clone()) => return Err(error),
            Err(error) => {
                reason = Some(format!(
                    "Current source could not be assessed: {}",
                    error.code()
                ));
            }
        }
        Ok(None)
    })();
    match coordinated {
        Ok(Some(state)) => return minimal(state),
        Ok(None) => {}
        Err(error) if profile_failure(error.clone()) => return Err(error),
        Err(error) => {
            if let Some(state) = inspect_control(root, io)? {
                return minimal(state);
            }
            // The fallback read classifies authority only. It cannot replace the
            // retained view with a revision that was never paired with source.
            reason = Some(format!(
                "Coordinated source assessment unavailable: {}",
                error.code()
            ));
        }
    }
    projection::active(&view.0, &view.1, source.as_ref(), reason.as_deref(), work)
}

fn output(value: &Value, json: bool, work: &mut Work) -> Result<TaskOutput> {
    let bytes = if json {
        value.compact()?
    } else {
        value.storage()?
    };
    let text = std::str::from_utf8(&bytes)?;
    let stdout = crate::output::ascii_json(text.strip_suffix('\n').unwrap_or(text))?;
    work.charge(stdout.len())?;
    Ok(TaskOutput {
        stdout,
        warnings: Vec::new(),
        exit_code: 0,
    })
}

#[cfg(test)]
mod tests;
