//! Read-only composition for Python's optional-workspace checkpoint command.
use crate::{
    Error, Result, checkpoints,
    error::require,
    values::Value,
    workspace::{self, MAX_TEXT, optional_bytes},
};
use std::path::{Path, PathBuf};

pub(crate) fn inspect_root(
    root: &Path,
    remaining: &[PathBuf],
    #[cfg(test)] observed: &mut dyn FnMut(usize),
) -> Result<Value> {
    #[cfg(test)]
    workspace::load_observed(root, observed)?;
    #[cfg(not(test))]
    workspace::load(root)?;
    require(remaining.len() <= 1, "invalid_checkpoint_operands")?;
    let id = remaining
        .first()
        .map(|item| item.to_str().ok_or(Error("non_unicode_path_unsupported")))
        .transpose()?
        .unwrap_or("plan-approval");
    checkpoints::inspect_bytes(id, None)?;
    let raw = optional_bytes(
        root,
        &Path::new("checkpoints").join(format!("{id}.md")),
        MAX_TEXT,
        "unsupported_checkpoint_input_profile",
    )?;
    checkpoints::inspect_bytes(id, raw.as_deref())
}

/// Raw variadic CLI operands: only a marked first candidate selects a workspace.
/// No ancestor search, arity inference, or expansion of the checkpoint selector.
pub fn inspect_arguments(items: &[PathBuf]) -> Result<Value> {
    let (root, remaining) = workspace::split_optional_workspace(items)?;
    let value = inspect_root(
        &root,
        remaining,
        #[cfg(test)]
        &mut |_| {},
    )?;
    // Both public APIs enforce the native compact-output profile.
    value.compact()?;
    Ok(value)
}

/// Complete compact and ASCII serialization before the caller writes stdout.
pub fn inspect_json_arguments(items: &[PathBuf]) -> Result<String> {
    let compact = inspect_arguments(items)?.compact()?;
    crate::output::ascii_json(std::str::from_utf8(&compact)?)
}
