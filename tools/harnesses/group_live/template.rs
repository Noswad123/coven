//! Focused verification template, rebased into OUT_DIR by build.rs.
#![allow(dead_code)] // This harness intentionally links only a subset of domain callers.
#[path = "../../error.rs"]
pub mod error;
pub use error::{Error, Result};
// Supplied by handoff.rs in the complete application crate.
impl From<std::str::Utf8Error> for Error {
    fn from(_: std::str::Utf8Error) -> Self {
        Self("invalid_utf8")
    }
}
#[path = "../../djinn_live_observation.rs"]
pub mod djinn_live_observation;
#[path = "../../group_live.rs"]
pub mod group_live;
#[path = "../../groups.rs"]
pub mod groups;
#[path = "../../platform/unix/live_artifact.rs"]
pub(crate) mod live_artifact;
#[path = "../../observation_time.rs"]
mod observation_time;
#[path = "../../observation_wire.rs"]
mod observation_wire;
#[path = "../../read_fs.rs"]
mod read_fs;
#[path = "../../storage.rs"]
mod storage;
#[path = "../../strict_json.rs"]
mod strict_json;
#[path = "../../task_time.rs"]
mod task_time;
#[path = "../../unicode.rs"]
mod unicode;
#[path = "../../values/mod.rs"]
pub mod values;

// Keep artifact policy in its actual platform file. Test-only storage support
// uses real files and advisory locks; the application uses its existing platform.
mod platform {
    use crate::Result;
    pub(crate) use crate::live_artifact;
    use std::{
        fs::{File, OpenOptions},
        os::{
            fd::AsRawFd,
            unix::fs::{OpenOptionsExt, PermissionsExt},
        },
        path::{Path, PathBuf},
    };
    pub(crate) fn open_readonly(path: &Path) -> Result<File> {
        Ok(OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?)
    }
    pub(crate) fn open_workspace_lock(path: &Path, create: bool) -> Result<File> {
        Ok(OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?)
    }
    pub(crate) fn try_workspace_lock(file: &File) -> std::io::Result<bool> {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(true);
        }
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::EAGAIN) {
            Ok(false)
        } else {
            Err(e)
        }
    }
    pub(crate) fn unlock_workspace(file: &File) {
        unsafe {
            libc::flock(file.as_raw_fd(), libc::LOCK_UN);
        }
    }
    pub(crate) fn sync_directory(path: &Path) -> std::io::Result<()> {
        File::open(path)?.sync_all()
    }
    pub(crate) fn random_uuid_hex() -> Result<String> {
        use std::io::Read;
        let mut bytes = [0u8; 16];
        File::open("/dev/urandom")?.read_exact(&mut bytes)?;
        bytes[6] = (bytes[6] & 15) | 64;
        bytes[8] = (bytes[8] & 63) | 128;
        Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }
    pub(crate) fn workspace_argument(path: &Path) -> Result<PathBuf> {
        Ok(path.to_owned())
    }
    pub(crate) fn replacement_mode(path: &Path) -> std::io::Result<Option<u32>> {
        match std::fs::symlink_metadata(path) {
            Ok(m) if m.is_file() => Ok(Some(m.permissions().mode() & 0o7777)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            _ => Err(std::io::Error::other("unsafe")),
        }
    }
    pub(crate) fn apply_replacement_mode(file: &File, mode: u32) -> std::io::Result<()> {
        file.set_permissions(std::fs::Permissions::from_mode(mode))
    }
    pub(crate) fn verify_replacement_mode(file: &File, mode: u32) -> std::io::Result<()> {
        if file.metadata()?.permissions().mode() & 0o7777 == mode {
            Ok(())
        } else {
            Err(std::io::Error::other("mode"))
        }
    }
    pub(crate) fn rename_replace(a: &Path, b: &Path) -> std::io::Result<()> {
        std::fs::rename(a, b)
    }
    pub(crate) fn mkdir_private(path: &Path) -> std::io::Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(path)
    }
    pub(crate) fn ensure_supported() -> Result<()> {
        Ok(())
    }
}
