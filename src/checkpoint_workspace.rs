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

/// Complete safely escaped pretty or compact presentation before stdout.
/// The existing compact library API and its byte representation stay unchanged.
pub fn inspect_text_arguments(items: &[PathBuf], json: bool) -> Result<String> {
    if json {
        return inspect_json_arguments(items);
    }
    let pretty = inspect_arguments(items)?.storage()?;
    let text = std::str::from_utf8(&pretty)?;
    crate::output::ascii_json(text.strip_suffix('\n').unwrap_or(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::values::{self, Profile};
    use std::fs;

    #[test]
    fn pretty_and_compact_preserve_values_ascii_and_compact_library_bytes() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("coven.json"),
            br#"{"name":"test","agents":["a"]}"#,
        )
        .unwrap();
        fs::create_dir(root.path().join("checkpoints")).unwrap();
        let raw = "# Plan\nStatus: pending\nSnow: \u{2603}\u{1f98a}\n";
        fs::write(root.path().join("checkpoints/plan-approval.md"), raw).unwrap();
        let items = [root.path().to_owned()];
        let compact = inspect_json_arguments(&items).unwrap();
        assert_eq!(inspect_text_arguments(&items, true).unwrap(), compact);
        let pretty = inspect_text_arguments(&items, false).unwrap();
        assert!(pretty.starts_with("{\n  \"checkpoint\": \"plan-approval\",\n"));
        assert!(!pretty.ends_with('\n'));
        assert!(!compact.contains('\n'));
        assert!(pretty.is_ascii() && compact.is_ascii());
        assert!(pretty.contains("\\u2603\\ud83e\\udd8a"));
        assert_eq!(
            values::parse(pretty.as_bytes(), Profile::LegacySnapshot)
                .unwrap()
                .compact()
                .unwrap(),
            values::parse(compact.as_bytes(), Profile::LegacySnapshot)
                .unwrap()
                .compact()
                .unwrap(),
        );
        assert_eq!(
            fs::read_to_string(root.path().join("checkpoints/plan-approval.md")).unwrap(),
            raw
        );
        assert!(!root.path().join("state").exists());
    }

    #[test]
    fn pretty_inspection_keeps_preflight_before_arity_and_identifier_errors() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("coven.json");
        fs::write(&path, b"{}").unwrap();
        let items = [
            root.path().to_owned(),
            PathBuf::from("../bad"),
            PathBuf::from("extra"),
        ];
        assert_eq!(
            inspect_text_arguments(&items, false).unwrap_err().code(),
            "missing_workspace_record_field"
        );
        fs::write(path, br#"{"name":"test","agents":["a"]}"#).unwrap();
        assert_eq!(
            inspect_text_arguments(&items, false).unwrap_err().code(),
            "invalid_checkpoint_operands"
        );
        assert_eq!(
            inspect_text_arguments(&items[..2], false)
                .unwrap_err()
                .code(),
            "invalid_checkpoint_identifier"
        );
        assert!(!root.path().join("state").exists());
    }
}
