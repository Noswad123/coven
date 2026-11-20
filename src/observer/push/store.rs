//! Descriptor-relative private storage; never follow aliases or adopt public files.
use crate::{Error, Result};
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::Path,
};

pub(super) const LIMIT: usize = 1024 * 1024;
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Commit {
    Durable,
    Uncertain,
}
type Access = fn(&File) -> std::io::Result<()>;
pub(super) struct Store {
    dir: File,
    _lock: File,
    access: Access,
    #[cfg(test)]
    pub fault: std::sync::atomic::AtomicU8,
}
fn err<T>(_: T) -> Error {
    Error("push_state_unavailable")
}
fn name(s: &str) -> CString {
    CString::new(s).unwrap()
}
fn open(dir: &File, path: &str, flags: i32, mode: u32) -> Result<File> {
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name(path).as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode,
        )
    };
    if fd < 0 {
        return Err(Error("push_state_unavailable"));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn private(file: &File, access: Access) -> Result<()> {
    access(file).map_err(|_| Error("push_unsafe_acl"))?;
    let m = file.metadata().map_err(err)?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o7777 != 0o600
        || m.nlink() != 1
        || m.len() > LIMIT as u64
    {
        return Err(Error("push_unsafe_state"));
    }
    Ok(())
}
impl Store {
    pub fn new(path: &str) -> Result<Self> {
        Self::with_access(path, crate::private_access::validate_fd_access)
    }
    fn with_access(path: &str, access: Access) -> Result<Self> {
        let mut dir = File::open("/").map_err(err)?;
        let parts: Vec<_> = Path::new(path).components().skip(1).collect();
        for (i, part) in parts.iter().enumerate() {
            access(&dir).map_err(|_| Error("push_unsafe_acl"))?;
            let m = dir.metadata().map_err(err)?;
            if (m.uid() != 0 && m.uid() != unsafe { libc::geteuid() }) || m.mode() & 0o022 != 0 {
                return Err(Error("push_unsafe_parent"));
            }
            let part = part
                .as_os_str()
                .to_str()
                .ok_or(Error("push_unsafe_state"))?;
            if i + 1 == parts.len() {
                let rc = unsafe { libc::mkdirat(dir.as_raw_fd(), name(part).as_ptr(), 0o700) };
                if rc != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                    return Err(Error("push_state_unavailable"));
                }
            }
            dir = open(&dir, part, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        }
        let m = dir.metadata().map_err(err)?;
        access(&dir).map_err(|_| Error("push_unsafe_acl"))?;
        if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o7777 != 0o700 {
            return Err(Error("push_unsafe_state"));
        }
        let lock = open(
            &dir,
            "lock",
            libc::O_RDWR | libc::O_CREAT | libc::O_NONBLOCK,
            0o600,
        )?;
        private(&lock, access)?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(Error("push_state_busy"));
        }
        Ok(Self {
            dir,
            _lock: lock,
            access,
            #[cfg(test)]
            fault: std::sync::atomic::AtomicU8::new(0),
        })
    }
    pub fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        (self.access)(&self.dir).map_err(|_| Error("push_unsafe_acl"))?;
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                self.dir.as_raw_fd(),
                name(path).as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
                Ok(None)
            } else {
                Err(Error("push_state_unavailable"))
            };
        }
        let f = open(&self.dir, path, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        private(&f, self.access)?;
        let mut bytes = Vec::new();
        f.take(LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        if bytes.len() > LIMIT {
            return Err(Error("push_state_limit"));
        }
        Ok(Some(bytes))
    }
    pub fn write(&self, path: &str, bytes: &[u8]) -> Result<Commit> {
        if bytes.len() > LIMIT {
            return Err(Error("push_state_limit"));
        }
        // Validate existing destination before replacement as well as when loading.
        self.read(path)?;
        let temp = format!(".{}", crate::platform::random_uuid_hex().map_err(err)?);
        let result = (|| {
            let mut f = open(
                &self.dir,
                &temp,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )?;
            private(&f, self.access)?;
            f.write_all(bytes).map_err(err)?;
            f.sync_all().map_err(err)?;
            #[cfg(test)]
            if self.fault.load(std::sync::atomic::Ordering::SeqCst) == 1 {
                return Err(Error("push_state_unavailable"));
            }
            if unsafe {
                libc::renameat(
                    self.dir.as_raw_fd(),
                    name(&temp).as_ptr(),
                    self.dir.as_raw_fd(),
                    name(path).as_ptr(),
                )
            } != 0
            {
                return Err(Error("push_state_unavailable"));
            }
            #[cfg(test)]
            if self.fault.load(std::sync::atomic::Ordering::SeqCst) == 2 {
                return Ok(Commit::Uncertain);
            }
            Ok(if self.dir.sync_all().is_ok() {
                Commit::Durable
            } else {
                Commit::Uncertain
            })
        })();
        unsafe {
            libc::unlinkat(self.dir.as_raw_fd(), name(&temp).as_ptr(), 0);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn deny(_: &File) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
    }
    #[test]
    fn acl_denial_precedes_directory_adoption_read_and_secret_write() {
        let root = crate::private_access::test_support::tempdir();
        let path = root.path().join("push");
        assert!(Store::with_access(path.to_str().unwrap(), deny).is_err());
        assert!(!path.exists());
        let mut store = Store::new(path.to_str().unwrap()).unwrap();
        store.write("secret", b"original").unwrap();
        store.access = deny;
        assert!(store.read("secret").is_err());
        assert!(store.write("secret", b"replacement").is_err());
        assert_eq!(std::fs::read(path.join("secret")).unwrap(), b"original");
        store.access = |f| {
            if unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETFL) } & libc::O_ACCMODE
                == libc::O_WRONLY
            {
                deny(f)
            } else {
                crate::private_access::validate_fd_access(f)
            }
        };
        assert!(store.write("new-secret", b"must-not-be-written").is_err());
        assert!(!path.join("new-secret").exists());
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 2);
        fn deny_files(f: &File) -> std::io::Result<()> {
            if f.metadata()?.is_file() {
                deny(f)
            } else {
                crate::private_access::validate_fd_access(f)
            }
        }
        store.access = deny_files;
        assert!(store.read("secret").is_err());
        drop(store);
        // The same policy also rejects an ACL-bearing lock before locking it.
        assert!(Store::with_access(path.to_str().unwrap(), deny_files).is_err());
    }
}
