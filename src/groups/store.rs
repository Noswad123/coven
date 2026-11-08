use super::schema::*;
use crate::{
    Error, Result,
    error::require,
    platform, read_fs,
    storage::{self, Delivery, WorkspaceGuard},
    values::{self, Profile, Value},
};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
pub(super) const LIMIT: usize = 1_048_576;
const RELATIVE: &str = "state/groups.json";

pub(super) fn workspace(path: &Path) -> Result<PathBuf> {
    let check = || {
        let root = path.canonicalize()?;
        require(root.is_dir(), "groups_invalid_workspace")?;
        require(
            std::fs::symlink_metadata(root.join("coven.json"))?.is_file(),
            "groups_invalid_workspace",
        )?;
        require(
            std::fs::symlink_metadata(root.join("state"))?.is_dir(),
            "groups_invalid_workspace",
        )?;
        read_fs::managed(&root, Path::new(RELATIVE), false)?;
        Ok(root)
    };
    check().map_err(|_: Error| Error("groups_invalid_workspace"))
}
pub(super) fn decode(bytes: &[u8]) -> Result<Value> {
    require(bytes.len() <= LIMIT, "groups_authority_size_limit")?;
    crate::strict_json::check_depth(bytes, 16).map_err(|_| Error("groups_nesting_limit"))?;
    let v = values::parse(bytes, Profile::Authority).map_err(|_| Error("groups_invalid_json"))?;
    validate(&v)?;
    Ok(v)
}
pub(super) fn read(root: &Path) -> Result<(Option<Value>, Option<Vec<u8>>)> {
    let path = read_fs::managed(root, Path::new(RELATIVE), false)
        .map_err(|_| Error("groups_unsafe_authority"))?;
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((None, None)),
        Err(_) => return Err(Error("groups_authority_unavailable")),
        Ok(m) => require(m.is_file(), "groups_unsafe_authority")?,
    }
    let mut file =
        platform::open_readonly(&path).map_err(|_| Error("groups_authority_unavailable"))?;
    require(file.metadata()?.is_file(), "groups_unsafe_authority")?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)?;
    Ok((Some(decode(&bytes)?), Some(bytes)))
}

/// Domain fault boundary. A replacement returning Ok has a known visible result,
/// even when its directory sync failed. Every subsequent failure is uncertainty.
pub(super) trait Io {
    fn uuid(&self) -> Result<String> {
        crate::task_time::uuid_hyphenated()
    }
    fn replace(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        storage::atomic_replace_bytes(guard, Path::new(RELATIVE), bytes)
    }
    fn reread(&self, root: &Path) -> Result<(Option<Value>, Option<Vec<u8>>)> {
        read(root)
    }
    fn open(&self, path: &Path) -> Result<File> {
        platform::open_readonly(path)
    }
    fn sync_file(&self, file: &File) -> Result<()> {
        Ok(file.sync_all()?)
    }
    fn sync_dir(&self, path: &Path) -> Result<()> {
        Ok(platform::sync_directory(path)?)
    }
}
pub(super) struct SystemIo;
impl Io for SystemIo {}

pub(super) fn finish(
    guard: &WorkspaceGuard,
    state: &Value,
    original: Option<&[u8]>,
    changed: bool,
    io: &impl Io,
) -> Result<Value> {
    let encoded;
    let bytes = if changed {
        validate(state)?;
        encoded = state
            .storage()
            .map_err(|_| Error("groups_writer_profile"))?;
        // Admit exact serialized bytes through the native reader before rename.
        // This enforces its depth/node profile too, with no speculative headroom.
        let admitted = decode(&encoded)?;
        require(equal(state, &admitted)?, "groups_writer_profile")?;
        &encoded[..]
    } else {
        original.ok_or(Error("groups_missing_observation"))?
    };
    if changed && io.replace(guard, bytes)? == Delivery::DurabilityUncertain {
        return obj(&[("outcome", string("durability_uncertain"))]);
    }
    let confirmation = || -> Result<()> {
        let (current, current_bytes) = io.reread(guard.root())?;
        require(
            current
                .as_ref()
                .is_some_and(|v| equal(v, state).unwrap_or(false))
                && current_bytes.as_deref() == Some(bytes),
            "groups_changed_after_publication",
        )?;
        let path = read_fs::managed(guard.root(), Path::new(RELATIVE), false)?;
        let file = io.open(&path)?;
        require(file.metadata()?.is_file(), "groups_unsafe_authority")?;
        io.sync_file(&file)?;
        io.sync_dir(&guard.root().join("state"))
    };
    let uncertain = confirmation().is_err();
    obj(&[(
        "outcome",
        string(if uncertain {
            "durability_uncertain"
        } else {
            "durably_committed"
        }),
    )])
}
