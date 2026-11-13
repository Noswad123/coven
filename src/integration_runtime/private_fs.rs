//! Descriptor-relative private state. Fences detect replacement, not hostile same-UID CAS.
use crate::{Error, Result, error::require};
use std::{
    ffi::{CStr, CString},
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path, PathBuf},
};

fn openat(parent: &File, name: &CStr, flags: i32) -> std::io::Result<File> {
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600 as libc::c_uint,
        )
    };
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

fn access(file: &File) -> Result<()> {
    crate::private_access::validate_fd_access(file)?;
    #[cfg(target_os = "linux")]
    for name in [c"system.posix_acl_access", c"system.posix_acl_default"] {
        let n =
            unsafe { libc::fgetxattr(file.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0) };
        if n >= 0 {
            return Err(Error("integration_acl_denied"));
        }
        require(
            matches!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ENODATA) | Some(libc::ENOTSUP)
            ),
            "integration_acl_unavailable",
        )?;
    }
    Ok(())
}

pub(super) fn valid_path(path: &Path) -> Result<()> {
    let text = path.to_str().ok_or(Error("integration_invalid_path"))?;
    require(
        path.is_absolute()
            && text != "/"
            && !text.ends_with('/')
            && !text.chars().any(char::is_control)
            && !text
                .split('/')
                .skip(1)
                .any(|s| matches!(s, "" | "." | "..")),
        "integration_invalid_path",
    )
}

pub(super) fn preflight(path: &Path) -> Result<()> {
    walk(path, false, true).map(|_| ())
}

fn creation_parent(file: &File) -> Result<()> {
    let m = file.metadata()?;
    require(
        m.is_dir()
            && (m.uid() == unsafe { libc::geteuid() } || m.uid() == 0)
            && m.mode() & 0o022 == 0,
        "integration_unsafe_creation_parent",
    )?;
    access(file)?;
    // Ask the OS using effective credentials, including root/capability semantics.
    // The retained directory anchors lookup without probing by creating a child.
    require(
        unsafe {
            libc::faccessat(
                file.as_raw_fd(),
                c".".as_ptr(),
                libc::W_OK | libc::X_OK,
                libc::AT_EACCESS,
            )
        } == 0,
        "integration_creation_parent_access_denied",
    )
}

fn walk(path: &Path, create: bool, preview: bool) -> Result<File> {
    valid_path(path)?;
    require(
        crate::private_access::supports_privacy(),
        "integration_privacy_unsupported",
    )?;
    let fd = unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    for component in path.components() {
        if let Component::Normal(name) = component {
            let name =
                CString::new(name.as_bytes()).map_err(|_| Error("integration_invalid_path"))?;
            let next = match openat(&file, &name, libc::O_RDONLY | libc::O_DIRECTORY) {
                Ok(next) => next,
                Err(e) if preview && e.kind() == std::io::ErrorKind::NotFound => {
                    creation_parent(&file)?;
                    return Ok(file);
                }
                Err(e) if create && e.kind() == std::io::ErrorKind::NotFound => {
                    creation_parent(&file)?;
                    #[cfg(test)]
                    super::checkpoint("before_mkdir");
                    let created =
                        unsafe { libc::mkdirat(file.as_raw_fd(), name.as_ptr(), 0o700) } == 0;
                    if !created {
                        let error = std::io::Error::last_os_error();
                        if error.kind() != std::io::ErrorKind::AlreadyExists {
                            return Err(error.into());
                        }
                    }
                    let next = openat(&file, &name, libc::O_RDONLY | libc::O_DIRECTORY)?;
                    admit_dir(&next)?;
                    if created {
                        file.sync_all()?;
                    }
                    next
                }
                Err(e) => return Err(e.into()),
            };
            file = next;
        }
    }
    admit_dir(&file)?;
    Ok(file)
}

fn admit_dir(file: &File) -> Result<()> {
    let m = file.metadata()?;
    require(
        m.is_dir() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o7777 == 0o700,
        "integration_unsafe_directory",
    )?;
    access(file)
}

pub(super) struct Directory {
    file: File,
    path: PathBuf,
    id: (u64, u64),
}
impl Directory {
    pub(super) fn open(path: &Path, create: bool) -> Result<Self> {
        if create {
            preflight(path)?;
        }
        let file = walk(path, create, false)?;
        let m = file.metadata()?;
        Ok(Self {
            file,
            path: path.into(),
            id: (m.dev(), m.ino()),
        })
    }
    pub(super) fn fence(&self) -> Result<()> {
        admit_dir(&self.file)?;
        let current = walk(&self.path, false, false)?.metadata()?;
        require(
            (current.dev(), current.ino()) == self.id,
            "integration_state_replaced",
        )
    }
    pub(super) fn sync(&self) -> Result<()> {
        self.file.sync_all()?;
        self.fence()
    }
    pub(super) fn stat(&self, name: &CStr) -> Result<Option<libc::stat>> {
        self.fence()?;
        let mut stat = std::mem::MaybeUninit::uninit();
        if unsafe {
            libc::fstatat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0
        {
            Ok(Some(unsafe { stat.assume_init() }))
        } else {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(e.into())
            }
        }
    }
    pub(super) fn file(&self, name: &CStr, create: bool) -> Result<File> {
        self.fence()?;
        let file = openat(
            &self.file,
            name,
            if create {
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL
            } else {
                libc::O_RDONLY
            },
        )?;
        self.fence_file(name, &file)?;
        Ok(file)
    }
    pub(super) fn writable(&self, name: &CStr) -> Result<File> {
        self.fence()?;
        let file = openat(&self.file, name, libc::O_RDWR)?;
        self.fence_file(name, &file)?;
        Ok(file)
    }
    pub(super) fn fence_file(&self, name: &CStr, file: &File) -> Result<()> {
        self.fence()?;
        let m = file.metadata()?;
        require(
            m.is_file()
                && m.uid() == unsafe { libc::geteuid() }
                && m.nlink() == 1
                && m.mode() & 0o7777 == 0o600,
            "integration_unsafe_file",
        )?;
        access(file)?;
        let s = self
            .stat(name)?
            .ok_or(Error("integration_state_replaced"))?;
        #[allow(clippy::unnecessary_cast)]
        let matches = (s.st_dev as u64, s.st_ino as u64) == (m.dev(), m.ino());
        require(matches, "integration_state_replaced")
    }
    pub(super) fn lock(&self, name: &CStr, new: bool) -> Result<File> {
        let file = self.file(name, new)?;
        require(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "integration_instance_busy",
        )?;
        self.fence_file(name, &file)?;
        file.sync_all()?;
        self.sync()?;
        Ok(file)
    }
    pub(super) fn registry_lock(
        &self,
        cancel: &dyn crate::observation_socket::Cancel,
        timeout: std::time::Duration,
    ) -> Result<File> {
        use std::time::{Duration, Instant};
        require(!cancel.interrupted(), "integration_startup_cancelled")?;
        self.fence()?;
        #[cfg(test)]
        super::checkpoint("before_registry_create");
        let file = match openat(
            &self.file,
            c"registry.lock",
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
        ) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                openat(&self.file, c"registry.lock", libc::O_RDWR)?
            }
            Err(error) => return Err(error.into()),
        };
        self.fence_file(c"registry.lock", &file)?;
        let deadline = Instant::now() + timeout;
        loop {
            require(!cancel.interrupted(), "integration_startup_cancelled")?;
            self.fence_file(c"registry.lock", &file)?;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            require(
                matches!(
                    error.raw_os_error(),
                    Some(libc::EWOULDBLOCK) | Some(libc::EINTR)
                ),
                "integration_registry_lock_failed",
            )?;
            #[cfg(test)]
            super::checkpoint("before_registry_wait");
            let remaining = deadline.saturating_duration_since(Instant::now());
            require(!remaining.is_zero(), "integration_registry_busy")?;
            std::thread::sleep(remaining.min(Duration::from_millis(5)));
        }
        self.fence_file(c"registry.lock", &file)?;
        file.sync_all()?;
        self.sync()?;
        Ok(file)
    }
    pub(super) fn count(&self) -> Result<usize> {
        self.fence()?;
        // A separate open description avoids sharing a directory iteration offset.
        let file = openat(&self.file, c".", libc::O_RDONLY | libc::O_DIRECTORY)?;
        use std::os::fd::IntoRawFd;
        let fd = file.into_raw_fd();
        let dir = unsafe { libc::fdopendir(fd) };
        if dir.is_null() {
            unsafe {
                libc::close(fd);
            }
            return Err(std::io::Error::last_os_error().into());
        }
        let mut count = 0;
        let mut failed = false;
        loop {
            #[cfg(target_os = "linux")]
            unsafe {
                *libc::__errno_location() = 0;
            }
            #[cfg(target_os = "macos")]
            unsafe {
                *libc::__error() = 0;
            }
            let entry = unsafe { libc::readdir(dir) };
            if entry.is_null() {
                failed = std::io::Error::last_os_error().raw_os_error() != Some(0);
                break;
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if name != c"." && name != c".." {
                count += 1;
            }
            if count > 64 {
                break;
            }
        }
        unsafe {
            libc::closedir(dir);
        }
        require(!failed, "integration_directory_read_failed")?;
        self.fence()?;
        Ok(count)
    }
    pub(super) fn socket(&self) -> Result<Option<(u64, u64)>> {
        let Some(s) = self.stat(c"r.sock")? else {
            return Ok(None);
        };
        require(
            s.st_mode & libc::S_IFMT == libc::S_IFSOCK
                && s.st_mode & 0o7777 == 0o600
                && s.st_uid == unsafe { libc::geteuid() }
                && s.st_nlink == 1,
            "integration_unsafe_socket",
        )?;
        #[allow(clippy::unnecessary_cast)]
        let id = (s.st_dev as u64, s.st_ino as u64);
        crate::private_access::validate_socket_entry(
            &self.file, c"r.sock", id.0, id.1, s.st_uid, 0o600,
        )?;
        #[cfg(target_os = "linux")]
        {
            // Linux cannot fgetxattr an O_PATH socket FD. Resolve only the retained
            // parent through procfs, and lgetxattr never follows the leaf. Absence
            // of procfs is a refusal, not a pathname-following fallback.
            let path =
                CString::new(format!("/proc/self/fd/{}/r.sock", self.file.as_raw_fd())).unwrap();
            let n = unsafe {
                libc::lgetxattr(
                    path.as_ptr(),
                    c"system.posix_acl_access".as_ptr(),
                    std::ptr::null_mut(),
                    0,
                )
            };
            require(
                n < 0
                    && matches!(
                        std::io::Error::last_os_error().raw_os_error(),
                        Some(libc::ENODATA) | Some(libc::ENOTSUP)
                    ),
                "integration_socket_acl_denied",
            )?;
            let after = self
                .stat(c"r.sock")?
                .ok_or(Error("integration_receiver_replaced"))?;
            #[allow(clippy::unnecessary_cast)]
            let same = (after.st_dev as u64, after.st_ino as u64) == id
                && after.st_mode == s.st_mode
                && after.st_uid == s.st_uid
                && after.st_nlink == 1;
            require(same, "integration_receiver_replaced")?;
        }
        Ok(Some(id))
    }
    pub(super) fn read(&self, name: &CStr) -> Result<Vec<u8>> {
        let mut file = self.file(name, false)?;
        let before = file.metadata()?;
        require(before.len() <= 8192, "integration_receipt_limit")?;
        let mut bytes = Vec::new();
        (&mut file).take(8193).read_to_end(&mut bytes)?;
        let after = file.metadata()?;
        require(
            bytes.len() <= 8192
                && before.len() == after.len()
                && before.mtime() == after.mtime()
                && before.mtime_nsec() == after.mtime_nsec()
                && before.ctime() == after.ctime()
                && before.ctime_nsec() == after.ctime_nsec(),
            "integration_state_replaced",
        )?;
        self.fence_file(name, &file)?;
        Ok(bytes)
    }
    pub(super) fn write(&self, name: &CStr, file: &mut File, bytes: &[u8]) -> Result<()> {
        use std::io::{Seek, SeekFrom};
        require(bytes.len() <= 8192, "integration_receipt_limit")?;
        self.fence_file(name, file)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(bytes)?;
        file.set_len(bytes.len() as u64)?;
        file.sync_all()?;
        self.fence_file(name, file)?;
        self.sync()
    }
}
