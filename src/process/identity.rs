//! Ordinary executable change detection, not authentication or an execution sandbox.
use crate::{Result, error::require, platform, values::Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_EXECUTABLE_BYTES: u64 = 1 << 30;
const ENV_KEYS: [&str; 6] = ["PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TERM"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pub metadata: platform::FileMetadata,
    pub sha256: String,
}

impl Identity {
    pub(crate) fn value(&self) -> Result<Value> {
        Value::object(vec![
            (
                "device".into(),
                Value::integer(&self.metadata.device.to_string())?,
            ),
            (
                "inode".into(),
                Value::integer(&self.metadata.inode.to_string())?,
            ),
            (
                "size".into(),
                Value::integer(&self.metadata.size.to_string())?,
            ),
            (
                "mtime_ns".into(),
                Value::integer(&self.metadata.mtime_ns.to_string())?,
            ),
            ("sha256".into(), Value::String(self.sha256.clone())),
        ])
    }
}

pub(crate) fn metadata(path: &Path) -> Result<platform::FileMetadata> {
    platform::executable_metadata(path)
}

/// Hash I/O is intentionally outside the subprocess deadline, as in the legacy adapter.
pub(crate) fn fingerprint(path: &Path) -> Result<Identity> {
    fingerprint_checked(path, MAX_EXECUTABLE_BYTES, |_| Ok(()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Checkpoint {
    BeforeOpen,
    AfterOpen,
    AfterRead,
}

fn fingerprint_checked(
    path: &Path,
    limit: u64,
    mut checkpoint: impl FnMut(Checkpoint) -> Result<()>,
) -> Result<Identity> {
    let before = metadata(path)?;
    require(before.size <= limit, "executable_too_large")?;
    checkpoint(Checkpoint::BeforeOpen)?;
    let mut file = platform::open_readonly(path)?;
    require(
        platform::descriptor_metadata(&file)? == before,
        "executable_changed",
    )?;
    checkpoint(Checkpoint::AfterOpen)?;
    let mut digest = Sha256::new();
    let mut count = 0_u64;
    let mut buffer = [0_u8; 65536];
    // The extra byte detects growth without reading an unbounded changing file.
    let mut reader = (&mut file).take(limit + 1);
    loop {
        let length = match reader.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if length == 0 {
            break;
        }
        count += length as u64;
        require(count <= limit, "executable_too_large")?;
        digest.update(&buffer[..length]);
        checkpoint(Checkpoint::AfterRead)?;
    }
    require(
        count == before.size
            && platform::descriptor_metadata(&file)? == before
            && metadata(path)? == before,
        "executable_changed",
    )?;
    Ok(Identity {
        metadata: before,
        sha256: format!("{:x}", digest.finalize()),
    })
}

/// Caller PATH discovery only; finding an executable does not establish trust.
pub(crate) fn find_executable(name: &str) -> Option<PathBuf> {
    find_in_path(name, std::env::var_os("PATH").as_deref())
}

fn find_in_path(name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    let candidate = Path::new(name);
    let executable = |candidate: &Path| {
        // shutil.which follows aliases; fingerprinting the resolved selection is separate.
        let resolved = candidate.canonicalize().ok()?;
        metadata(&resolved).ok()?;
        Some(candidate.to_path_buf())
    };
    if candidate
        .parent()
        .is_some_and(|parent| !parent.as_os_str().is_empty())
    {
        return executable(candidate);
    }
    let path = path.unwrap_or_else(|| OsStr::new("/bin:/usr/bin"));
    if path.is_empty() {
        return None;
    }
    std::env::split_paths(path).find_map(|directory| executable(&directory.join(name)))
}

pub(crate) fn adapter_environment() -> BTreeMap<OsString, OsString> {
    filtered_environment(std::env::vars_os())
}

fn filtered_environment(
    entries: impl IntoIterator<Item = (OsString, OsString)>,
) -> BTreeMap<OsString, OsString> {
    entries
        .into_iter()
        .filter(|(key, _)| ENV_KEYS.iter().any(|allowed| key == OsStr::new(allowed)))
        .collect()
}

pub(crate) fn validate_limits(timeout: f64, max_output_bytes: i64) -> Result<(Duration, usize)> {
    require(
        timeout.is_finite() && timeout > 0.0 && timeout <= 30.0,
        "invalid_probe_timeout",
    )?;
    require(
        (1024..=1048576).contains(&max_output_bytes),
        "invalid_probe_output_limit",
    )?;
    Ok((Duration::from_secs_f64(timeout), max_output_bytes as usize))
}

pub(crate) fn alias_matches(selected: &Path, resolved: &Path) -> bool {
    selected
        .canonicalize()
        .is_ok_and(|current| current == resolved)
}

#[cfg(test)]
mod tests;
