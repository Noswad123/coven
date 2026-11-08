//! Exclusive native bootstrap ownership. Never adopts or removes partial state.
use super::Delivery;
use crate::{Result, error::require, platform};
use std::{
    fs::File,
    io::Write,
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
    thread::{self, ThreadId},
};

pub(crate) struct Reservation {
    root: PathBuf,
    root_directory: File,
    state_directory: File,
    lock: File,
    owner: u32,
    thread: ThreadId,
    not_send: PhantomData<Rc<()>>,
}

/// Failure preserves partial names, but does not promise their crash durability.
/// The service syncs prepared files/directories before publishing the marker.
pub(crate) fn reserve(root: &Path) -> Result<Reservation> {
    reserve_with(root, &SystemIo)
}

fn reserve_with(root: &Path, io: &impl ReservationIo) -> Result<Reservation> {
    platform::ensure_supported()?;
    require(
        root.is_absolute() && root.to_str().is_some() && root.canonicalize()? == root,
        "invalid_init_root",
    )?;
    let root_directory = io.open_root(root)?;
    io.create_state(&root.join("state"))?;
    let state_directory = io.open_state(&root.join("state"))?;
    let lock = io.create_lock(&root.join("state/.workspace.lock"))?;
    require(io.lock(&lock)?, "workspace_lock_busy")?;
    let reservation = Reservation {
        root: root.to_owned(),
        root_directory,
        state_directory,
        lock,
        owner: std::process::id(),
        thread: thread::current().id(),
        not_send: PhantomData,
    };
    reservation.validate()?;
    Ok(reservation)
}

impl Reservation {
    pub(crate) fn validate(&self) -> Result<()> {
        require(
            self.owner == std::process::id() && self.thread == thread::current().id(),
            "invalid_init_owner",
        )?;
        require(self.root.canonicalize()? == self.root, "invalid_init_root")?;
        platform::validate_init_identity(&self.root, &self.root_directory, true)?;
        platform::validate_init_identity(&self.root.join("state"), &self.state_directory, true)?;
        platform::validate_init_identity(
            &self.root.join("state/.workspace.lock"),
            &self.lock,
            false,
        )
    }

    pub(crate) fn publish_marker(&self, bytes: &[u8]) -> Result<Delivery> {
        self.publish_with(bytes, &SystemIo)
    }

    fn publish_with(&self, bytes: &[u8], io: &impl MarkerIo) -> Result<Delivery> {
        self.validate()?;
        require(bytes.len() <= 8 * 1_048_576, "unsupported_publication_size")?;
        let marker = self.root.join("coven.json");
        match std::fs::symlink_metadata(&marker) {
            Ok(_) => return Err(crate::Error("init_marker_exists")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let temporary = self
            .root
            .join(format!(".coven-init-{}.tmp", platform::random_uuid_hex()?));
        let mut file = io.temporary(&temporary)?;
        let result = (|| {
            io.write(&mut file, bytes)?;
            io.sync_file(&file)?;
            self.validate()?;
            platform::validate_init_identity(&temporary, &file, false)?;
            io.link(&temporary, &marker)?;
            // Publication is known after link: no later failure may erase that fact.
            Ok(if io.sync_root(&self.root_directory).is_ok() {
                Delivery::Committed
            } else {
                Delivery::DurabilityUncertain
            })
        })();
        // A replaced name is not ours to clean up. Never remove marker/state/lock.
        if platform::validate_init_temp(&temporary, &file).is_ok() {
            let _ = io.cleanup(&temporary);
        }
        result
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.owner == std::process::id() && self.thread == thread::current().id() {
            platform::unlock_workspace(&self.lock);
        }
    }
}

trait ReservationIo {
    fn open_root(&self, path: &Path) -> Result<File> {
        platform::open_init_directory(path)
    }
    fn create_state(&self, path: &Path) -> std::io::Result<()> {
        std::fs::create_dir(path)
    }
    fn open_state(&self, path: &Path) -> Result<File> {
        platform::open_init_directory(path)
    }
    fn create_lock(&self, path: &Path) -> Result<File> {
        platform::open_init_lock(path)
    }
    fn lock(&self, file: &File) -> std::io::Result<bool> {
        platform::try_workspace_lock(file)
    }
}

trait MarkerIo {
    fn temporary(&self, path: &Path) -> Result<File> {
        platform::open_generated(path, true)
    }
    fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
        file.write_all(bytes)
    }
    fn sync_file(&self, file: &File) -> std::io::Result<()> {
        file.sync_all()
    }
    fn link(&self, source: &Path, target: &Path) -> std::io::Result<()> {
        std::fs::hard_link(source, target)
    }
    fn sync_root(&self, root: &File) -> std::io::Result<()> {
        root.sync_all()
    }
    fn cleanup(&self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }
}
struct SystemIo;
impl ReservationIo for SystemIo {}
impl MarkerIo for SystemIo {}

#[cfg(all(test, unix))]
mod tests;
