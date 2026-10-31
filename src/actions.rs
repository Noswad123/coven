//! No-effects historical action inspection. This snapshot is not linearizable,
//! latest-state authority, runtime eligibility, or proof of a durable claim.
pub(crate) mod authority;
mod plans;

use crate::{
    Error, Result,
    error::require,
    platform, read_fs,
    values::{self, Profile, Value},
};
use authority::{Budget, Lookup, ValidatedControl};
use std::{fs, io::Read, path::Path};

const CONTROL: &str = "state/runtime-launch-control.json";
const MAX_CHECKPOINT: usize = 1_048_576;

fn parse_authority(raw: &[u8]) -> Result<Value> {
    // Match Python json.detect_encoding's BOM and implicit UTF-16/32 probes,
    // but explicitly refuse these encodings rather than silently converting.
    let non_native = raw.starts_with(b"\xef\xbb\xbf")
        || raw.starts_with(b"\xff\xfe")
        || raw.starts_with(b"\xfe\xff")
        || raw.starts_with(b"\0\0\xfe\xff")
        || (raw.len() >= 4 && (raw[0] == 0 || raw[1] == 0))
        || (raw.len() == 2 && (raw[0] == 0 || raw[1] == 0));
    require(!non_native, "unsupported_action_encoding_profile")?;
    values::parse(raw, Profile::Authority)
}

fn string(value: impl Into<String>) -> Value {
    Value::String(value.into())
}
fn object(entries: Vec<(&str, Value)>) -> Result<Value> {
    Value::object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

// Opening stays no-follow/nonblocking. Missing is distinct from unreadable and
// unsafe, and size refusals must survive the optional-artifact catch boundary.
fn read(
    root: &Path,
    relative: &Path,
    limit: usize,
    profile: &'static str,
) -> Result<Option<Vec<u8>>> {
    let path = read_fs::managed(root, relative, false)?;
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let mut input = platform::open_readonly(&path)?;
    require(input.metadata()?.is_file(), "unsafe_archive_path")?;
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    require(bytes.len() <= limit, profile)?;
    Ok(Some(bytes))
}

fn optional_error(error: Error, checkpoint: bool) -> Result<Value> {
    if authority::fatal(&error) {
        return Err(error);
    }
    Ok(string(match (checkpoint, error.code()) {
        (false, "unsafe_archive_path") => "action_bundle_unsafe_path",
        (false, "input_or_storage_unavailable") => "action_bundle_unavailable",
        (false, "action_bundle_missing") => "action_bundle_missing",
        (false, _) => "action_bundle_invalid",
        (true, "unsafe_archive_path") => "action_checkpoint_unsafe_path",
        (true, "input_or_storage_unavailable") => "action_checkpoint_unavailable",
        (true, _) => "action_checkpoint_invalid",
    }))
}

fn inspect_with_budget(
    workspace: &Path,
    action_id: &str,
    budget: &mut Budget,
) -> Result<(Value, Vec<u8>)> {
    platform::ensure_supported()?;
    authority::action_id(action_id)?;
    let root = workspace.canonicalize()?;
    require(root.is_dir(), "invalid_action_workspace")?;
    let actual = platform::workspace_location(&root)?;
    let control = match read(
        &root,
        Path::new(CONTROL),
        values::MAX_INPUT,
        "unsupported_action_input_profile",
    )? {
        Some(raw) => Some(ValidatedControl::validate(
            parse_authority(&raw)?,
            &actual,
            budget,
        )?),
        None => {
            // Python rejects orphan authority, without enumerating any bundles.
            let directory = read_fs::managed(&root, Path::new("state/actions"), true)?;
            match fs::symlink_metadata(directory) {
                Ok(_) => return Err(Error("action_orphan_bundles")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            None
        }
    };
    let mut claim = Value::Null;
    let mut workspace_id = Value::Null;
    let mut epoch_id = Value::Null;
    let mut revision = Value::Null;
    if let Some(control) = &control {
        claim = match control.lookup(action_id, None)? {
            Lookup::Absent => Value::Null,
            Lookup::Existing(value) | Lookup::Match(value) | Lookup::Conflict(value) => {
                value.clone()
            }
        };
        workspace_id = authority::get(control.value(), "workspace_id")?.clone();
        epoch_id = authority::get(control.value(), "epoch_id")?.clone();
        revision = string(authority::control_revision(control.value(), budget)?);
    }
    let mut bundle = Value::Null;
    let mut bundle_error = Value::Null;
    let mut manifest = Value::Null;
    let read_bundle = (|| -> Result<(Value, String)> {
        let raw = read(
            &root,
            Path::new(&format!("state/actions/{action_id}.json")),
            values::MAX_INPUT,
            "unsupported_action_input_profile",
        )?
        .ok_or(Error("action_bundle_missing"))?;
        let value = parse_authority(&raw)?;
        let digest = authority::bundle(&value, action_id, budget)?;
        Ok((value, digest))
    })();
    match read_bundle {
        Ok((value, digest)) => {
            bundle = value;
            manifest = string(digest);
        }
        Err(error) => bundle_error = optional_error(error, false)?,
    }
    let checkpoint_id = format!("runtime-launch-{action_id}");
    let checkpoint = (|| {
        let raw = read(
            &root,
            Path::new(&format!("checkpoints/{checkpoint_id}.md")),
            MAX_CHECKPOINT,
            "unsupported_checkpoint_input_profile",
        )?;
        crate::checkpoints::inspect_bytes(&checkpoint_id, raw.as_deref())
    })();
    let (checkpoint_key, checkpoint_value) = match checkpoint {
        Ok(value) => ("checkpoint", value),
        Err(error) => ("checkpoint_error", optional_error(error, true)?),
    };
    let value = object(vec![
        (
            "control",
            control.map_or(Value::Null, |c| c.value().clone()),
        ),
        ("workspace_id", workspace_id),
        ("epoch_id", epoch_id),
        (
            "location",
            object(vec![
                ("root", string(actual.root)),
                ("device", Value::integer(&actual.device.to_string())?),
                ("inode", Value::integer(&actual.inode.to_string())?),
            ])?,
        ),
        ("control_revision", revision),
        ("bundle", bundle),
        ("bundle_error", bundle_error),
        ("manifest_digest", manifest),
        ("checkpoint_id", string(checkpoint_id)),
        ("claim", claim),
        (checkpoint_key, checkpoint_value),
        ("outcome", string("inspected")),
    ])?;
    let compact = value.compact()?;
    budget.charge(compact.len())?;
    Ok((value, compact))
}

/// Inspect only persisted historical authority and its dedicated checkpoint.
/// Library output is compact-bounded; unlike CLI output, it is not ASCII-escaped
/// and can therefore accept a larger Unicode report within the same work limit.
pub fn inspect(workspace: &Path, action_id: &str) -> Result<Value> {
    inspect_with_budget(workspace, action_id, &mut Budget::default()).map(|(value, _)| value)
}

/// Expand the CLI workspace argument and produce bounded ASCII JSON.
pub fn inspect_json_argument(workspace: &Path, id: &str) -> Result<String> {
    inspect_json_with_budget(workspace, id, &mut Budget::default())
}

fn inspect_json_with_budget(workspace: &Path, id: &str, budget: &mut Budget) -> Result<String> {
    platform::ensure_supported()?;
    // Keep the compact serialization and ASCII presentation on one meter.
    // inspect_with_budget already charges the compact report.
    let (_, compact) = inspect_with_budget(&platform::workspace_argument(workspace)?, id, budget)?;
    let json = std::str::from_utf8(&compact).map_err(|_| Error("report_encoding_failed"))?;
    let output = crate::output::ascii_json(json)?;
    budget.charge(output.len())?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    fn control(root: &Path) -> Value {
        let actual = platform::workspace_location(root).unwrap();
        object(vec![
            ("version", Value::integer("1").unwrap()),
            ("action", string("runtime_launch")),
            ("workspace_id", string("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")),
            ("epoch_id", string("cccccccccccccccccccccccccccccccc")),
            (
                "location",
                object(vec![
                    ("root", string(actual.root)),
                    (
                        "device",
                        Value::integer(&actual.device.to_string()).unwrap(),
                    ),
                    ("inode", Value::integer(&actual.inode.to_string()).unwrap()),
                ])
                .unwrap(),
            ),
            ("head", Value::Null),
            ("claims", object(vec![]).unwrap()),
        ])
        .unwrap()
    }
    #[test]
    fn missing_authority_needs_no_config_and_does_not_write() {
        let temp = tempfile::tempdir().unwrap();
        let value = inspect(temp.path(), ID).unwrap();
        assert_eq!(
            authority::get(&value, "outcome").unwrap().as_str(),
            Some("inspected")
        );
        assert_eq!(
            authority::get(&value, "bundle_error").unwrap().as_str(),
            Some("action_bundle_missing")
        );
        assert!(authority::get(&value, "checkpoint").is_ok());
        assert!(authority::get(&value, "checkpoint_error").is_err());
        assert_eq!(value.as_object().unwrap().entries().len(), 12);
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
        assert!(inspect(temp.path(), "../bad").is_err());
    }
    #[test]
    fn optional_invalid_artifacts_are_diagnostics_but_resource_errors_are_fatal() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("state/actions")).unwrap();
        fs::write(
            temp.path().join(CONTROL),
            control(&temp.path().canonicalize().unwrap())
                .canonical()
                .unwrap(),
        )
        .unwrap();
        let path = temp.path().join(format!("state/actions/{ID}.json"));
        for raw in [
            b"null".as_slice(),
            b"{\"version\":1,\"version\":1}",
            b"{\"x\":NaN}",
            b"{\"x\":\"\\ud800\"}",
        ] {
            fs::write(&path, raw).unwrap();
            let value = inspect(temp.path(), ID).unwrap();
            assert_eq!(
                authority::get(&value, "bundle_error").unwrap().as_str(),
                Some("action_bundle_invalid")
            );
            assert_eq!(
                authority::get(&value, "outcome").unwrap().as_str(),
                Some("inspected")
            );
        }
        fs::write(&path, vec![b' '; values::MAX_INPUT + 1]).unwrap();
        assert_eq!(
            inspect(temp.path(), ID).unwrap_err().code(),
            "unsupported_action_input_profile"
        );
        fs::write(&path, b"null").unwrap();
        fs::create_dir(temp.path().join("checkpoints")).unwrap();
        fs::write(
            temp.path()
                .join(format!("checkpoints/runtime-launch-{ID}.md")),
            vec![b'x'; MAX_CHECKPOINT + 1],
        )
        .unwrap();
        assert_eq!(
            inspect(temp.path(), ID).unwrap_err().code(),
            "unsupported_checkpoint_input_profile"
        );
    }
    #[test]
    fn invalid_control_is_never_an_optional_diagnostic() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("state")).unwrap();
        fs::write(temp.path().join(CONTROL), b"null").unwrap();
        assert!(inspect(temp.path(), ID).is_err());
        fs::remove_file(temp.path().join(CONTROL)).unwrap();
        fs::create_dir(temp.path().join("state/actions")).unwrap();
        assert_eq!(
            inspect(temp.path(), ID).unwrap_err().code(),
            "action_orphan_bundles"
        );
    }
    #[test]
    fn python_non_native_authority_encodings_are_explicit_fatal_profile_refusals() {
        // Match Python's detection for BOM and implicit UTF-16/32, both endian
        // orders. These bytes must never become optional bundle diagnostics.
        let raw = b"{\"version\":1}";
        let mut encodings = vec![[b"\xef\xbb\xbf".as_slice(), raw].concat()];
        for width in [2, 4] {
            for little in [false, true] {
                let encoded: Vec<u8> = raw
                    .iter()
                    .flat_map(|b| {
                        let mut bytes = vec![0; width];
                        bytes[if little { 0 } else { width - 1 }] = *b;
                        bytes
                    })
                    .collect();
                encodings.push(encoded.clone());
                let bom: &[u8] = match (width, little) {
                    (2, false) => b"\xfe\xff",
                    (2, true) => b"\xff\xfe",
                    (4, false) => b"\0\0\xfe\xff",
                    (4, true) => b"\xff\xfe\0\0",
                    _ => unreachable!(),
                };
                encodings.push([bom, &encoded].concat());
            }
        }
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("state/actions")).unwrap();
        let valid_control = control(&temp.path().canonicalize().unwrap())
            .canonical()
            .unwrap();
        let bundle_path = temp.path().join(format!("state/actions/{ID}.json"));
        for encoded in encodings {
            fs::write(temp.path().join(CONTROL), &encoded).unwrap();
            assert_eq!(
                inspect(temp.path(), ID).unwrap_err().code(),
                "unsupported_action_encoding_profile"
            );
            assert_eq!(
                inspect_json_argument(temp.path(), ID).unwrap_err().code(),
                "unsupported_action_encoding_profile"
            );
            fs::write(temp.path().join(CONTROL), &valid_control).unwrap();
            fs::write(&bundle_path, encoded).unwrap();
            assert_eq!(
                inspect(temp.path(), ID).unwrap_err().code(),
                "unsupported_action_encoding_profile"
            );
            assert_eq!(
                inspect_json_argument(temp.path(), ID).unwrap_err().code(),
                "unsupported_action_encoding_profile"
            );
        }
        assert!(parse_authority(b"{}").is_ok());
        assert!(parse_authority(b"\"\xe9\x9b\xaa\"").is_ok());
        assert!(parse_authority(b"{bad}").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn managed_symlinks_and_wrong_types_stay_unsafe() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("state/actions")).unwrap();
        fs::write(
            temp.path().join(CONTROL),
            control(&temp.path().canonicalize().unwrap())
                .canonical()
                .unwrap(),
        )
        .unwrap();
        let path = temp.path().join(format!("state/actions/{ID}.json"));
        symlink("/does-not-exist", &path).unwrap();
        let value = inspect(temp.path(), ID).unwrap();
        assert_eq!(
            authority::get(&value, "bundle_error").unwrap().as_str(),
            Some("action_bundle_unsafe_path")
        );
        fs::create_dir(temp.path().join("checkpoints")).unwrap();
        fs::create_dir(
            temp.path()
                .join(format!("checkpoints/runtime-launch-{ID}.md")),
        )
        .unwrap();
        let value = inspect(temp.path(), ID).unwrap();
        assert_eq!(
            authority::get(&value, "checkpoint_error").unwrap().as_str(),
            Some("action_checkpoint_unsafe_path")
        );
        fs::remove_file(&path).unwrap();
        fs::remove_file(temp.path().join(CONTROL)).unwrap();
        symlink("/does-not-exist", temp.path().join(CONTROL)).unwrap();
        assert!(inspect(temp.path(), ID).is_err());
    }
}
