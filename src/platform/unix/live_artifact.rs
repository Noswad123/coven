//! Private, bounded artifact reads. Identity checks are not same-UID authentication.
use crate::{Error, Result, error::require};
use std::{
    fs::{File, Metadata, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

const LIMIT: u64 = 65_536;

#[derive(Debug, PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    size: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}

fn stamp(info: Metadata) -> Result<Stamp> {
    require(
        info.is_file()
            && info.uid() == unsafe { libc::getuid() }
            && info.mode() & 0o7777 == 0o600
            && info.nlink() == 1,
        "unsafe_artifact",
    )?;
    require(info.len() <= LIMIT, "response_limit")?;
    Ok(Stamp {
        device: info.dev(),
        inode: info.ino(),
        size: info.len(),
        mtime: (info.mtime(), info.mtime_nsec()),
        ctime: (info.ctime(), info.ctime_nsec()),
    })
}

#[derive(Debug, PartialEq, Eq)]
struct Directory {
    resolved: PathBuf,
    device: u64,
    inode: u64,
}

fn private(info: &Metadata) -> bool {
    info.is_dir() && info.uid() == unsafe { libc::getuid() } && info.mode() & 0o7777 == 0o700
}

fn directory(path: &Path) -> Result<Directory> {
    // A trailing slash or dot must not make a symlink leaf appear to be a directory.
    let info = std::fs::symlink_metadata(path).map_err(|_| Error("io_error"))?;
    require(private(&info), "unsafe_directory")?;
    let resolved = path.canonicalize().map_err(|_| Error("io_error"))?;
    let actual = std::fs::symlink_metadata(&resolved).map_err(|_| Error("io_error"))?;
    require(
        private(&actual) && (info.dev(), info.ino()) == (actual.dev(), actual.ino()),
        "directory_changed",
    )?;
    Ok(Directory {
        resolved,
        device: info.dev(),
        inode: info.ino(),
    })
}

#[derive(Debug)]
pub(crate) struct Artifact {
    selected_parent: PathBuf,
    parent: Directory,
    path: String,
    device: Option<u64>,
    inode: Option<u64>,
    read: Result<Contents>,
}

#[derive(Debug)]
struct Contents {
    file: File,
    before: Stamp,
    bytes: Vec<u8>,
}

pub(crate) fn open(path: &str) -> Result<Artifact> {
    open_with(path, || {})
}

fn open_with(path: &str, after_stamp: impl FnOnce()) -> Result<Artifact> {
    // Keep ancestor alias spelling for fencing, but reject a symlink parent leaf.
    let (parent, leaf) = path.rsplit_once('/').ok_or(Error("io_error"))?;
    let selected_parent = PathBuf::from(if parent.is_empty() { "/" } else { parent });
    // lstat("alias/.") follows alias. Strip redundant trailing separators/dots so
    // the actual selected directory leaf must itself be nonsymlink.
    let selected_parent = lexical_parent(&selected_parent);
    let parent = directory(&selected_parent)?;
    let resolved = parent.resolved.join(leaf);
    let path = resolved.to_str().ok_or(Error("io_error"))?.to_owned();
    let (mut device, mut inode) = (None, None);
    // Preserve only checked metadata on a failed read, without a second pathname
    // lookup in the observer. bytes/recheck never expose or accept failed contents.
    let read = (|| {
        let before = stamp(std::fs::symlink_metadata(&resolved).map_err(|_| Error("io_error"))?)?;
        device = Some(before.device);
        inode = Some(before.inode);
        after_stamp();
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&resolved)
            .map_err(|_| Error("io_error"))?;
        require(
            stamp(file.metadata().map_err(|_| Error("io_error"))?)? == before
                && directory(&selected_parent)? == parent,
            "artifact_changed",
        )?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error("io_error"))?;
        require(bytes.len() <= LIMIT as usize, "response_limit")?;
        Ok(Contents {
            file,
            before,
            bytes,
        })
    })();
    let mut artifact = Artifact {
        selected_parent,
        parent,
        path,
        device,
        inode,
        read,
    };
    if let Err(error) = artifact.recheck() {
        artifact.read = Err(error);
    }
    Ok(artifact)
}

fn lexical_parent(path: &Path) -> PathBuf {
    // Components removes trailing slash/dot without resolving ancestor aliases.
    path.components().collect()
}

impl Artifact {
    pub(crate) fn bytes(&self) -> Result<&[u8]> {
        self.read
            .as_ref()
            .map(|read| read.bytes.as_slice())
            .map_err(Clone::clone)
    }
    pub(crate) fn resolved(&self) -> &str {
        &self.path
    }
    pub(crate) fn device(&self) -> Option<u64> {
        self.device
    }
    pub(crate) fn inode(&self) -> Option<u64> {
        self.inode
    }
    pub(crate) fn recheck(&self) -> Result<()> {
        let read = self.read.as_ref().map_err(Clone::clone)?;
        require(
            stamp(read.file.metadata().map_err(|_| Error("io_error"))?)? == read.before
                && stamp(std::fs::symlink_metadata(&self.path).map_err(|_| Error("io_error"))?)?
                    == read.before
                && directory(&self.selected_parent)? == self.parent,
            "artifact_changed",
        )
    }
}

#[cfg(test)]
#[path = "live_artifact/tests.rs"]
mod tests;
