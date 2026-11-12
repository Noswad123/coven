//! Agent-only, descriptor-relative private persistence. Fences detect replacement;
//! they are not a CAS against a malicious same-UID writer or an already-open FD.
use crate::{Error, Result, error::require, storage::Delivery};
use std::{
    cell::Cell,
    ffi::{CStr, CString},
    fs::{File, Metadata},
    io::{Read, Write},
    marker::PhantomData,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path, PathBuf},
    rc::Rc,
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

const STATE: &CStr = c"state";
const LOCK: &CStr = c".workspace.lock";
const LEASE: &CStr = c".agent-management.receiver.lock";
const STORE: &CStr = c"agent-management.json";
const LIMIT: usize = 256 * 1024;

fn identity(m: &Metadata) -> (u64, u64) {
    (m.dev(), m.ino())
}
fn stat_identity(s: &libc::stat) -> (u64, u64) {
    (s.st_dev as u64, s.st_ino as u64)
}
fn basename(leaf: &CStr) -> Result<()> {
    require(
        !leaf.to_bytes().is_empty()
            && !leaf.to_bytes().contains(&b'/')
            && leaf != c"."
            && leaf != c"..",
        "agent_invalid_basename",
    )
}
fn openat(parent: &File, leaf: &CStr, flags: i32, mode: libc::mode_t) -> std::io::Result<File> {
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            leaf.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            mode,
        )
    };
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

// Tests inject extra-access denials and races at the actual descriptor boundary.
#[cfg(test)]
thread_local! {
    static CHECKPOINT: std::cell::RefCell<Option<Box<dyn Fn(&str, &File) -> std::io::Result<()>>>> = const { std::cell::RefCell::new(None) };
}
fn access(role: &str, file: &File) -> Result<()> {
    #[cfg(test)]
    CHECKPOINT
        .with(|hook| {
            if let Some(hook) = hook.borrow().as_ref() {
                hook(role, file)?;
            }
            Ok::<_, std::io::Error>(())
        })
        .map_err(|_| Error("agent_private_access_denied"))?;
    let _ = role;
    crate::private_access::validate_fd_access(file)
        .map_err(|_| Error("agent_private_access_denied"))
}
fn admit_directory(file: &File, role: &str) -> Result<Metadata> {
    let m = file.metadata()?;
    require(
        m.is_dir() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o022 == 0,
        "agent_unsafe_parent",
    )?;
    access(role, file)?;
    Ok(m)
}
fn admit_file(file: &File, private: bool, role: &str) -> Result<Metadata> {
    let m = file.metadata()?;
    require(
        m.is_file()
            && m.uid() == unsafe { libc::geteuid() }
            && m.nlink() == 1
            && if private {
                m.mode() & 0o7777 == 0o600
            } else {
                m.mode() & 0o022 == 0
            },
        "agent_unsafe_store",
    )?;
    access(role, file)?;
    Ok(m)
}

/// Absolute physical names only. System ancestors are pinned, not privacy-admitted.
pub(super) struct Directory {
    file: File,
    path: PathBuf,
    inode: (u64, u64),
}
impl Directory {
    pub fn open(path: &Path) -> Result<Self> {
        require(
            crate::private_access::supports_privacy(),
            "agent_privacy_unsupported",
        )?;
        let file = Self::walk(path)?;
        let inode = identity(&admit_directory(&file, "directory")?);
        Ok(Self {
            file,
            path: path.to_owned(),
            inode,
        })
    }
    fn walk(path: &Path) -> Result<File> {
        require(path.is_absolute(), "agent_unsafe_parent")?;
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
            match component {
                Component::RootDir => {}
                Component::Normal(name) => {
                    let name =
                        CString::new(name.as_bytes()).map_err(|_| Error("agent_unsafe_parent"))?;
                    file = openat(&file, &name, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
                }
                _ => return Err(Error("agent_unsafe_parent")),
            }
        }
        Ok(file)
    }
    pub fn file(&self) -> &File {
        &self.file
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn identity(&self) -> (u64, u64) {
        self.inode
    }
    pub fn fence(&self) -> Result<()> {
        require(
            identity(&admit_directory(&self.file, "directory")?) == self.inode,
            "agent_store_replaced",
        )?;
        let current = Self::walk(&self.path)?;
        require(
            identity(&admit_directory(&current, "directory")?) == self.inode,
            "agent_store_replaced",
        )
    }
    pub fn stat(&self, leaf: &CStr) -> Result<Option<libc::stat>> {
        basename(leaf)?;
        let mut stat = std::mem::MaybeUninit::uninit();
        if unsafe {
            libc::fstatat(
                self.file.as_raw_fd(),
                leaf.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0
        {
            Ok(Some(unsafe { stat.assume_init() }))
        } else {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(error.into())
            }
        }
    }
    fn matches(&self, leaf: &CStr, inode: (u64, u64)) -> Result<()> {
        require(
            self.stat(leaf)?.is_some_and(|s| stat_identity(&s) == inode),
            "agent_store_replaced",
        )
    }
}

struct Held {
    file: File,
    inode: (u64, u64),
}
impl Held {
    fn open(state: &Directory, leaf: &CStr, private: bool, role: &str) -> Result<Self> {
        let file = openat(state.file(), leaf, libc::O_RDWR | libc::O_CREAT, 0o600)?;
        let inode = identity(&admit_file(&file, private, role)?);
        let held = Self { file, inode };
        held.fence(state, leaf, private, role)?;
        Ok(held)
    }
    fn fence(&self, state: &Directory, leaf: &CStr, private: bool, role: &str) -> Result<()> {
        require(
            identity(&admit_file(&self.file, private, role)?) == self.inode,
            "agent_store_replaced",
        )?;
        state.matches(leaf, self.inode)
    }
}

pub(super) struct Files {
    root: Directory,
    state: Directory,
    lock: Held,
    lease: Held,
    guarded: Cell<bool>,
}
pub(super) struct Guard<'a> {
    files: &'a Files,
    pid: u32,
    thread: ThreadId,
    not_send: PhantomData<Rc<()>>,
}
impl Guard<'_> {
    fn validate(&self, files: &Files) -> Result<()> {
        require(
            std::ptr::eq(self.files, files)
                && self.pid == std::process::id()
                && self.thread == thread::current().id(),
            "invalid_workspace_lock_owner",
        )?;
        files.fence()
    }
}
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if self.pid == std::process::id() && self.thread == thread::current().id() {
            unsafe {
                libc::flock(self.files.lock.file.as_raw_fd(), libc::LOCK_UN);
            }
            self.files.guarded.set(false);
        }
    }
}
fn flock(file: &File, timeout: Duration, code: &'static str) -> Result<()> {
    let started = Instant::now();
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if !matches!(
            error.raw_os_error(),
            Some(libc::EWOULDBLOCK) | Some(libc::EINTR)
        ) {
            return Err(error.into());
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        require(!remaining.is_zero(), code)?;
        thread::sleep(remaining.min(Duration::from_millis(5)));
    }
}

/// A load observation keeps the actual admitted target descriptor until publication.
pub(super) struct Image {
    pub bytes: Option<Vec<u8>>,
    held: Option<(File, Metadata)>,
}
fn unchanged(a: &Metadata, b: &Metadata) -> bool {
    identity(a) == identity(b)
        && a.len() == b.len()
        && a.mode() == b.mode()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}
impl Files {
    pub fn open(workspace: &Path) -> Result<Self> {
        // current_dir uses physical getcwd, not the shell's potentially aliased PWD.
        let absolute = if workspace.is_absolute() {
            workspace.to_owned()
        } else {
            std::env::current_dir()?.join(workspace)
        };
        require(
            !absolute
                .components()
                .any(|c| matches!(c, Component::ParentDir)),
            "agent_unsafe_parent",
        )?;
        let root = Directory::open(&absolute)?;
        access("root", root.file())?;
        let marker = openat(root.file(), c"coven.json", libc::O_RDONLY, 0)?;
        require(marker.metadata()?.is_file(), "invalid_workspace")?;
        root.fence()?;
        if unsafe { libc::mkdirat(root.file().as_raw_fd(), STATE.as_ptr(), 0o700) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(error.into());
            }
        }
        let file = openat(root.file(), STATE, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        let inode = identity(&admit_directory(&file, "state")?);
        let state = Directory {
            file,
            path: root.path().join("state"),
            inode,
        };
        root.matches(STATE, inode)?;
        state.fence()?;
        let lock = Held::open(&state, LOCK, false, "workspace_lock")?;
        // Use this same open file description for every subsequent guard.
        flock(
            &lock.file,
            Duration::from_millis(100),
            "workspace_lock_busy",
        )?;
        let lease_result: Result<Held> = (|| {
            root.fence()?;
            state.fence()?;
            lock.fence(&state, LOCK, false, "workspace_lock")?;
            let lease = Held::open(&state, LEASE, true, "receiver_lease")?;
            flock(&lease.file, Duration::ZERO, "agent_receiver_busy")?;
            lease.fence(&state, LEASE, true, "receiver_lease")?;
            Ok(lease)
        })();
        unsafe {
            libc::flock(lock.file.as_raw_fd(), libc::LOCK_UN);
        }
        let files = Self {
            root,
            state,
            lock,
            lease: lease_result?,
            guarded: Cell::new(false),
        };
        files.fence()?;
        Ok(files)
    }
    pub fn root(&self) -> &Path {
        self.root.path()
    }
    pub fn sync_startup(&self) -> Result<()> {
        self.fence()?;
        self.state
            .file()
            .sync_all()
            .map_err(|_| Error("agent_startup_sync_failed"))?;
        self.root
            .file()
            .sync_all()
            .map_err(|_| Error("agent_startup_sync_failed"))
    }
    pub fn fence(&self) -> Result<()> {
        (|| {
            self.root.fence()?;
            access("root", self.root.file())?;
            self.state.fence()?;
            access("state", self.state.file())?;
            self.root.matches(STATE, self.state.identity())?;
            self.lock.fence(&self.state, LOCK, false, "workspace_lock")
        })()
        .map_err(|_| Error("agent_store_replaced"))?;
        self.lease
            .fence(&self.state, LEASE, true, "receiver_lease")
            .map_err(|_| Error("agent_receiver_lease_replaced"))
    }
    pub fn guard(&self, timeout: Duration) -> Result<Guard<'_>> {
        self.fence()?;
        // flock on the same open description succeeds recursively; reject that
        // case so a nested guard can never prematurely unlock its outer guard.
        require(!self.guarded.get(), "workspace_lock_busy")?;
        flock(&self.lock.file, timeout, "workspace_lock_busy")?;
        self.guarded.set(true);
        let guard = Guard {
            files: self,
            pid: std::process::id(),
            thread: thread::current().id(),
            not_send: PhantomData,
        };
        guard.validate(self)?;
        Ok(guard)
    }
    pub fn load(&self, guard: &Guard<'_>) -> Result<Image> {
        guard.validate(self)?;
        let file = match openat(self.state.file(), STORE, libc::O_RDONLY, 0) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                require(self.state.stat(STORE)?.is_none(), "agent_store_replaced")?;
                return Ok(Image {
                    bytes: None,
                    held: None,
                });
            }
            Err(e) => return Err(e.into()),
        };
        let before = admit_file(&file, true, "existing_store")?;
        self.state.matches(STORE, identity(&before))?;
        require(before.len() <= LIMIT as u64, "agent_store_limit")?;
        let mut bytes = Vec::new();
        (&file).take(LIMIT as u64 + 1).read_to_end(&mut bytes)?;
        require(bytes.len() <= LIMIT, "agent_store_limit")?;
        let after = admit_file(&file, true, "existing_store")?;
        require(unchanged(&before, &after), "agent_store_replaced")?;
        self.state.matches(STORE, identity(&after))?;
        guard.validate(self)?;
        Ok(Image {
            bytes: Some(bytes),
            held: Some((file, after)),
        })
    }
    fn image_fence(&self, image: &Image) -> Result<()> {
        match &image.held {
            None => require(self.state.stat(STORE)?.is_none(), "agent_store_replaced"),
            Some((file, before)) => {
                let after = admit_file(file, true, "existing_store")?;
                require(unchanged(before, &after), "agent_store_replaced")?;
                self.state.matches(STORE, identity(&after))
            }
        }
    }
    pub fn replace(
        &self,
        guard: &Guard<'_>,
        image: &Image,
        nonce: &str,
        bytes: &[u8],
    ) -> Result<Delivery> {
        require(
            nonce.len() == 32 && nonce.bytes().all(|b| b.is_ascii_hexdigit()),
            "agent_invalid_nonce",
        )?;
        require(bytes.len() <= LIMIT, "agent_store_limit")?;
        guard.validate(self)?;
        self.image_fence(image)?;
        let name = CString::new(format!(".agent-management-{nonce}.tmp")).unwrap();
        let mut file = openat(
            self.state.file(),
            &name,
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        let inode = identity(&file.metadata()?);
        let result = (|| {
            // No secret bytes reach an inode before exact-mode and ACL admission.
            admit_file(&file, true, "pre_temp_write")?;
            require(file.metadata()?.len() == 0, "agent_unsafe_store")?;
            self.state.matches(&name, inode)?;
            guard.validate(self)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            admit_file(&file, true, "after_write")?;
            access("prepublication", &file)?;
            guard.validate(self)?;
            self.image_fence(image)?;
            admit_file(&file, true, "temporary")?;
            require(identity(&file.metadata()?) == inode, "agent_store_replaced")?;
            self.state.matches(&name, inode)?;
            if unsafe {
                libc::renameat(
                    self.state.file().as_raw_fd(),
                    name.as_ptr(),
                    self.state.file().as_raw_fd(),
                    STORE.as_ptr(),
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            // Publication has happened. Only the following directory sync can
            // affect delivery, and failure must never trigger rollback or resend.
            Ok(
                if access("postrename_sync", self.state.file())
                    .and_then(|_| self.state.file().sync_all().map_err(Into::into))
                    .is_ok()
                {
                    Delivery::Committed
                } else {
                    Delivery::DurabilityUncertain
                },
            )
        })();
        // Never unlink a foreign replacement, even when initial admission failed.
        if self
            .state
            .stat(&name)
            .ok()
            .flatten()
            .is_some_and(|s| stat_identity(&s) == inode)
        {
            unsafe {
                libc::unlinkat(self.state.file().as_raw_fd(), name.as_ptr(), 0);
            }
        }
        result
    }
}

#[cfg(test)]
#[path = "files/tests.rs"]
mod tests;
