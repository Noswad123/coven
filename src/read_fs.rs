use crate::{Error, Result, error::require, platform};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

pub(crate) fn read_regular(path: &Path, limit: usize) -> Result<Vec<u8>> {
    platform::ensure_supported()?;
    let mut input = platform::open_readonly(path)?;
    require(input.metadata()?.is_file(), "handoff_file_unavailable")?;
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    require(bytes.len() <= limit, "handoff_size_limit")?;
    Ok(bytes)
}

pub(crate) fn managed(root: &Path, relative: &Path, directory: bool) -> Result<PathBuf> {
    let mut selected = root.to_owned();
    let parts = relative.components().collect::<Vec<_>>();
    require(!parts.is_empty(), "unsafe_archive_path")?;
    for (index, part) in parts.iter().enumerate() {
        let Component::Normal(name) = part else {
            return Err(Error("unsafe_archive_path"));
        };
        selected.push(name);
        let metadata = match fs::symlink_metadata(&selected) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        require(
            if directory || index + 1 < parts.len() {
                metadata.is_dir()
            } else {
                metadata.is_file()
            },
            "unsafe_archive_path",
        )?;
    }
    Ok(selected)
}

pub(crate) fn workspace(path: &Path) -> Result<PathBuf> {
    platform::ensure_supported()?;
    let root = path.canonicalize()?;
    require(root.is_dir(), "invalid_handoff_workspace")?;
    let marker = managed(&root, Path::new("coven.json"), false)?;
    require(marker.is_file(), "invalid_handoff_workspace")?;
    Ok(root)
}

pub(crate) fn path_text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(Error("non_unicode_path_unsupported"))
}
