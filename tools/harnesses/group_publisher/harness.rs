//! Standalone test crate; production uses the parent crate and native adapter.
#![allow(dead_code, unused_imports)] // Only the publisher surface is exercised here.
#[path = "../../../src/error.rs"]
mod error;
pub use error::{Error, Result};
// The parent crate supplies this conversion in handoff.rs.
impl From<std::str::Utf8Error> for Error {
    fn from(_: std::str::Utf8Error) -> Self {
        Self("invalid_utf8")
    }
}
#[path = "../../../src/observation_socket.rs"]
mod observation_socket;
#[path = "../../../src/observation_time.rs"]
mod observation_time;
#[path = "../../../src/observation_wire.rs"]
mod observation_wire;
#[path = "../../../src/read_fs.rs"]
mod read_fs;
#[path = "../../../src/strict_json.rs"]
mod strict_json;
#[path = "../../../src/unicode.rs"]
mod unicode;
#[path = "../../../src/values/mod.rs"]
mod values;
mod storage {
    include!(concat!(env!("OUT_DIR"), "/storage.rs"));
}
#[path = "../../../src/task_time.rs"]
mod task_time;
mod groups {
    include!(concat!(env!("OUT_DIR"), "/groups.rs"));
}
#[path = "../../../src/kitsune_observation.rs"]
mod kitsune_observation;
mod djinn_live_observation {
    include!(concat!(env!("OUT_DIR"), "/djinn_live_observation.rs"));
}
#[path = "../../../src/group_live.rs"]
mod group_live;
#[path = "../../../src/platform/unix/live_artifact.rs"]
pub(crate) mod live_artifact;
#[path = "../../../src/platform/unix/observation_socket.rs"]
pub(crate) mod unix_socket;
mod init_platform {
    include!(concat!(env!("OUT_DIR"), "/init_platform.rs"));
}
use platform::workspace_argument;
mod platform {
    use crate::Result;
    pub(crate) use crate::init_platform::*;
    pub(crate) use crate::live_artifact;
    pub(crate) use crate::unix_socket as observation_socket;
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
    pub(crate) fn open_generated(path: &Path, create_new: bool) -> Result<File> {
        Ok(OpenOptions::new()
            .write(true)
            .create(!create_new)
            .create_new(create_new)
            .mode(0o600)
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
    pub(crate) fn random_uuid_hex() -> crate::Result<String> {
        use std::io::Read;
        let mut bytes = [0u8; 16];
        File::open("/dev/urandom")?.read_exact(&mut bytes)?;
        bytes[6] = (bytes[6] & 15) | 64;
        bytes[8] = (bytes[8] & 63) | 128;
        Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }
}
#[path = "../../../src/group_publisher.rs"]
mod group_publisher;
