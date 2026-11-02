//! Source-free durability confirmation, not reservation or mutation admission.
use super::*;
use crate::{platform, storage::WorkspaceGuard};
use std::{fs::File, io};

trait Io {
    fn read(&self, root: &Path) -> Result<Option<Validated>> {
        Ok(read_typed(root)?.map(|(_, state)| state))
    }
    fn open(&self, path: &Path) -> Result<File> {
        platform::open_readonly(path)
    }
    fn regular(&self, file: &File) -> io::Result<bool> {
        Ok(file.metadata()?.is_file())
    }
    fn sync_file(&self, file: &File) -> io::Result<()> {
        file.sync_all()
    }
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        platform::sync_directory(path)
    }
}
struct SystemIo;
impl Io for SystemIo {}

pub(super) fn confirm(root: &Path, selected: &Validated, guard: &WorkspaceGuard) -> Result<()> {
    confirm_with(root, selected, guard, &SystemIo)
}

fn confirm_with(
    root: &Path,
    selected: &Validated,
    guard: &WorkspaceGuard,
    io: &impl Io,
) -> Result<()> {
    guard.validate_for(root)?;
    let current = io
        .read(root)?
        .ok_or(Error("execution_authority_changed_before_confirmation"))?;
    require(
        selected.authority == current.authority
            && selected.replayed.revision == current.replayed.revision,
        "execution_authority_changed_before_confirmation",
    )?;
    let control = read_fs::managed(root, Path::new("state/execution/control.json"), false)?;
    let file = io.open(&control)?;
    require(io.regular(&file)?, "unsafe_archive_path")?;
    io.sync_file(&file)?;
    for relative in ["state/execution", "state", ""] {
        let directory = if relative.is_empty() {
            root.to_owned()
        } else {
            read_fs::managed(root, Path::new(relative), true)?
        };
        io.sync_dir(&directory)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
