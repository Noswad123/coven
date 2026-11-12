//! Private, descriptor-relative hook file storage. Provider policy and receipts
//! stay with callers. Supported replacement retains displaced entries until verified;
//! this is not global CAS or protection from later writes through old descriptors.
#![cfg(unix)]
use crate::{Error, Result, error::require};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::{File, Permissions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf},
};

#[path = "hook_files/platform.rs"]
mod platform;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use platform::MoveKind;

pub(crate) fn supports_apply() -> bool {
    platform::supports_apply()
}

#[derive(Clone, Copy)]
pub(crate) struct Spec {
    recovery: &'static str,
    leaves: &'static [&'static str],
    private: &'static [&'static str],
    directories: &'static [&'static str],
    read_limit: usize,
    errors: Errors,
}

#[derive(Clone, Copy)]
struct Errors {
    absolute_path_required: &'static str,
    unsafe_root: &'static str,
    unsafe_leaf: &'static str,
    unsafe_private_file: &'static str,
    conflict: &'static str,
    size_limit: &'static str,
    recovery_required: &'static str,
    apply_unsupported_platform: &'static str,
    write_failed: &'static str,
    durability_uncertain: &'static str,
    lock_failed: &'static str,
    locked: &'static str,
}

macro_rules! errors {
    ($prefix:literal) => {
        Errors {
            absolute_path_required: concat!($prefix, "_absolute_path_required"),
            unsafe_root: concat!($prefix, "_unsafe_root"),
            unsafe_leaf: concat!($prefix, "_unsafe_leaf"),
            unsafe_private_file: concat!($prefix, "_unsafe_private_file"),
            conflict: concat!($prefix, "_conflict"),
            size_limit: concat!($prefix, "_size_limit"),
            recovery_required: concat!($prefix, "_recovery_required"),
            apply_unsupported_platform: concat!($prefix, "_apply_unsupported_platform"),
            write_failed: concat!($prefix, "_write_failed"),
            durability_uncertain: concat!($prefix, "_durability_uncertain"),
            lock_failed: concat!($prefix, "_lock_failed"),
            locked: concat!($prefix, "_locked"),
        }
    };
}

impl Spec {
    pub(crate) const fn opencode() -> Self {
        Self {
            recovery: ".coven-opencode-extension-recovery",
            leaves: &[
                "coven-agent-reference.js",
                "kitsune-agent-state.js",
                "coven-opencode-extension.json",
                ".coven-opencode-extension.lock",
            ],
            private: &[
                "coven-opencode-extension.json",
                ".coven-opencode-extension.lock",
            ],
            directories: &[],
            read_limit: 8 * 1024 * 1024,
            errors: errors!("opencode_extension"),
        }
    }

    pub(crate) const fn pi() -> Self {
        Self {
            recovery: ".coven-pi-extension-recovery",
            leaves: &[
                "coven-agent-reference.ts",
                "kitsune-agent-state.ts",
                "coven-pi-extension.json",
                ".coven-pi-extension.lock",
            ],
            private: &["coven-pi-extension.json", ".coven-pi-extension.lock"],
            directories: &[],
            read_limit: 8 * 1024 * 1024,
            errors: errors!("pi_extension"),
        }
    }

    pub(crate) const fn copilot() -> Self {
        Self {
            recovery: ".coven-copilot-hooks-recovery",
            leaves: &[
                "settings.json",
                "coven-copilot-hooks.json",
                ".coven-copilot-hooks.lock",
                "kitsune-agent-state.sh",
            ],
            private: &["coven-copilot-hooks.json", ".coven-copilot-hooks.lock"],
            directories: &["hooks"],
            read_limit: 8 * 1024 * 1024,
            errors: errors!("copilot_hooks"),
        }
    }

    pub(crate) const fn claude() -> Self {
        Self {
            recovery: ".coven-claude-hooks-recovery",
            leaves: &[
                "settings.json",
                "coven-claude-hooks.json",
                ".coven-claude-hooks.lock",
                "kitsune-agent-state.sh",
            ],
            private: &["coven-claude-hooks.json", ".coven-claude-hooks.lock"],
            directories: &["hooks"],
            read_limit: 8 * 1024 * 1024,
            errors: errors!("claude_hooks"),
        }
    }

    pub(crate) const fn codex() -> Self {
        Self {
            recovery: ".coven-codex-hooks-recovery",
            leaves: &[
                "hooks.json",
                "config.toml",
                "coven-codex-hooks.json",
                ".coven-codex-hooks.lock",
                "kitsune-agent-state.sh",
            ],
            private: &["coven-codex-hooks.json", ".coven-codex-hooks.lock"],
            directories: &[],
            read_limit: 8 * 1024 * 1024,
            errors: errors!("codex_hooks"),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Image {
    pub(crate) bytes: Vec<u8>,
    pub(crate) dev: u64,
    pub(crate) ino: u64,
    pub(crate) mode: u32,
}

pub(crate) struct Root(File, Spec);
pub(crate) struct Lock(File);

impl Drop for Lock {
    fn drop(&mut self) {
        // Explicit unlock also handles a concurrent fork temporarily inheriting
        // this descriptor before its close-on-exec takes effect.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn basename(name: &[u8]) -> bool {
    !name.is_empty() && name != b"." && name != b".." && !name.contains(&b'/') && !name.contains(&0)
}

#[cfg(test)]
#[path = "hook_files/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "hook_files/test_support.rs"]
pub(crate) mod test_support;

impl Root {
    pub(crate) fn open(path: &Path, spec: Spec) -> Result<Self> {
        Self::walk(path, spec, |_| {})
    }

    fn walk(path: &Path, spec: Spec, mut after_open: impl FnMut(&Path)) -> Result<Self> {
        require(
            path.is_absolute()
                && path
                    .components()
                    .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
                && path
                    .to_str()
                    .is_some_and(|s| !s.chars().any(char::is_control)),
            spec.errors.absolute_path_required,
        )?;
        let mut root = Self(File::open("/")?, spec);
        let mut walked = PathBuf::from("/");
        for component in path.components() {
            if let Component::Normal(name) = component {
                use std::os::unix::ffi::OsStrExt;
                root = root.open_directory(name.as_bytes())?;
                walked.push(name);
                after_open(&walked);
            }
        }
        root.validate()?;
        Ok(root)
    }

    fn open_directory(&self, name: &[u8]) -> Result<Self> {
        require(basename(name), self.1.errors.unsafe_root)?;
        let name = CString::new(name).map_err(|_| Error(self.1.errors.unsafe_root))?;
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        require(fd >= 0, self.1.errors.unsafe_root)?;
        Ok(Self(unsafe { File::from_raw_fd(fd) }, self.1))
    }

    pub(crate) fn directory(&self, name: &[u8]) -> Result<Self> {
        require(
            self.1
                .directories
                .iter()
                .any(|allowed| allowed.as_bytes() == name),
            self.1.errors.unsafe_root,
        )?;
        let directory = self.open_directory(name)?;
        directory.validate()?;
        Ok(directory)
    }

    fn validate(&self) -> Result<()> {
        if supports_apply() {
            platform::validate_access(&self.0).map_err(|_| Error(self.1.errors.unsafe_root))?;
        }
        let m = self.0.metadata()?;
        require(
            m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o022 == 0,
            self.1.errors.unsafe_root,
        )
    }

    pub(crate) fn recovery_absent(&self) -> Result<()> {
        let name = CString::new(self.1.recovery).unwrap();
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        let result = unsafe {
            libc::fstatat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        require(
            result != 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound,
            self.1.errors.recovery_required,
        )
    }

    fn open_leaf(&self, name: &str, flags: i32) -> Result<Option<File>> {
        require(
            basename(name.as_bytes()) && self.1.leaves.contains(&name),
            self.1.errors.unsafe_leaf,
        )?;
        let name = CString::new(name).map_err(|_| Error(self.1.errors.unsafe_leaf))?;
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        let exists = unsafe {
            libc::fstatat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0;
        let prior = if exists {
            let stat = unsafe { stat.assume_init() };
            require(
                stat.st_mode & libc::S_IFMT == libc::S_IFREG
                    && stat.st_nlink == 1
                    && stat.st_uid == unsafe { libc::geteuid() }
                    && stat.st_mode & 0o022 == 0,
                self.1.errors.unsafe_leaf,
            )?;
            Some(stat)
        } else {
            require(
                std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound,
                self.1.errors.unsafe_leaf,
            )?;
            None
        };
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
                return Ok(None);
            }
            return Err(Error(self.1.errors.unsafe_leaf));
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if supports_apply() {
            platform::validate_access(&file).map_err(|_| Error(self.1.errors.unsafe_leaf))?;
        }
        let m = file.metadata()?;
        if let Some(prior) = prior {
            // libc's dev_t/ino_t widths and signedness vary across Unix.
            #[allow(clippy::unnecessary_cast)]
            let same_inode = m.dev() == prior.st_dev as u64 && m.ino() == prior.st_ino as u64;
            require(same_inode, self.1.errors.conflict)?;
        }
        require(
            m.is_file()
                && m.nlink() == 1
                && m.uid() == unsafe { libc::geteuid() }
                && m.mode() & 0o022 == 0,
            self.1.errors.unsafe_leaf,
        )?;
        if self
            .1
            .private
            .iter()
            .any(|leaf| leaf.as_bytes() == name.to_bytes())
        {
            require(
                m.mode() & 0o7777 == 0o600,
                self.1.errors.unsafe_private_file,
            )?;
        }
        Ok(Some(file))
    }

    pub(crate) fn lock(&self, name: &str) -> Result<Lock> {
        require(supports_apply(), self.1.errors.apply_unsupported_platform)?;
        self.validate()?;
        require(self.1.private.contains(&name), self.1.errors.unsafe_leaf)?;
        let file = self
            .open_leaf(name, libc::O_RDWR | libc::O_CREAT)?
            .ok_or(Error(self.1.errors.lock_failed))?;
        require(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            self.1.errors.locked,
        )?;
        Ok(Lock(file))
    }

    pub(crate) fn read(&self, name: &str, limit: usize) -> Result<Option<Image>> {
        self.validate()?;
        require(limit <= self.1.read_limit, self.1.errors.size_limit)?;
        let Some(mut file) = self.open_leaf(name, libc::O_RDONLY)? else {
            return Ok(None);
        };
        let m = file.metadata()?;
        require(m.len() <= limit as u64, self.1.errors.size_limit)?;
        let mut bytes = Vec::new();
        (&mut file).take(limit as u64 + 1).read_to_end(&mut bytes)?;
        require(bytes.len() <= limit, self.1.errors.size_limit)?;
        if supports_apply() {
            platform::validate_access(&file).map_err(|_| Error(self.1.errors.unsafe_leaf))?;
        }
        let after = file.metadata()?;
        require(
            m.len() == after.len()
                && m.mtime() == after.mtime()
                && m.mtime_nsec() == after.mtime_nsec()
                && m.ctime() == after.ctime()
                && m.ctime_nsec() == after.ctime_nsec()
                && m.mode() == after.mode()
                && m.uid() == after.uid()
                && after.nlink() == 1,
            self.1.errors.conflict,
        )?;
        Ok(Some(Image {
            bytes,
            dev: m.dev(),
            ino: m.ino(),
            mode: m.mode(),
        }))
    }

    /// Reads only code-approved relative components, never reopening an absolute path.
    pub(crate) fn read_relative(&self, name: &str, limit: usize) -> Result<Option<Image>> {
        if let Some((directory, leaf)) = name.split_once('/') {
            self.directory(directory.as_bytes())?.read(leaf, limit)
        } else {
            self.read(name, limit)
        }
    }

    pub(crate) fn fence(&self, name: &str, expected: &Option<Image>) -> Result<()> {
        require(
            &self.read(name, self.1.read_limit)? == expected,
            self.1.errors.conflict,
        )
    }

    pub(crate) fn replace(
        &self,
        name: &str,
        expected: &Option<Image>,
        bytes: Option<&[u8]>,
        mode: u32,
    ) -> Result<()> {
        require(supports_apply(), self.1.errors.apply_unsupported_platform)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.commit(name, expected, bytes, mode, || {})
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (name, expected, bytes, mode);
            Err(Error(self.1.errors.apply_unsupported_platform))
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn commit(
        &self,
        name: &str,
        expected: &Option<Image>,
        bytes: Option<&[u8]>,
        mode: u32,
        after_fence: impl FnOnce(),
    ) -> Result<()> {
        self.commit_with_fault(
            name,
            expected,
            bytes,
            mode,
            after_fence,
            #[cfg(test)]
            |_, _| Ok(()),
        )
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn commit_with_fault(
        &self,
        name: &str,
        expected: &Option<Image>,
        bytes: Option<&[u8]>,
        mode: u32,
        after_fence: impl FnOnce(),
        #[cfg(test)] mut checkpoint: impl FnMut(Checkpoint, &File) -> std::io::Result<()>,
    ) -> Result<()> {
        self.fence(name, expected)?;
        self.recovery_absent()?;
        let recovery_name = CString::new(self.1.recovery).unwrap();
        require(
            unsafe { libc::mkdirat(self.0.as_raw_fd(), recovery_name.as_ptr(), 0o700) } == 0,
            self.1.errors.recovery_required,
        )?;
        let result = (|| -> Result<()> {
            let mut stash = self.open_directory(self.1.recovery.as_bytes())?;
            #[cfg(test)]
            checkpoint(Checkpoint::StashAccess, &stash.0)?;
            stash.validate()?;
            // Recovery artifacts have their own fixed allowlist, never receipt paths.
            stash.1.leaves = &["expected", "replacement", "displaced", "transaction.json"];
            stash.1.private = &[];
            stash.1.directories = &[];
            // A setgid root propagates setgid; normalize this newly owned stash by FD.
            stash.0.set_permissions(Permissions::from_mode(0o700))?;
            require(
                stash.0.metadata()?.mode() & 0o7777 == 0o700,
                self.1.errors.recovery_required,
            )?;
            #[allow(unused_mut)] // The test checkpoint closure is mutable only in unit builds.
            let mut write = |leaf: &str, data: &[u8], mode: u32| -> Result<()> {
                let mut file = stash
                    .open_leaf(leaf, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?
                    .ok_or(Error(self.1.errors.write_failed))?;
                #[cfg(test)]
                checkpoint(Checkpoint::StageAccess, &file)?;
                platform::validate_access(&file).map_err(|_| Error(self.1.errors.unsafe_leaf))?;
                #[cfg(test)]
                checkpoint(Checkpoint::StageWrite, &file)?;
                file.write_all(data)?;
                file.set_permissions(Permissions::from_mode(mode))?;
                require(
                    file.metadata()?.mode() & 0o7777 == mode,
                    self.1.errors.unsafe_leaf,
                )?;
                #[cfg(test)]
                checkpoint(Checkpoint::StageSync, &file)?;
                platform::sync_file(&file)?;
                platform::validate_access(&file).map_err(|_| Error(self.1.errors.unsafe_leaf))?;
                Ok(())
            };
            if let Some(image) = expected {
                write("expected", &image.bytes, 0o600)?;
            }
            if let Some(data) = bytes {
                write("replacement", data, mode)?;
            }
            let staged = stash.read("replacement", self.1.read_limit)?;
            write(
                "transaction.json",
                &serde_json::to_vec(&json!({
                    "leaf":name,
                    "expected_digest":expected.as_ref().map(|i| digest(&i.bytes)),
                    "expected_mode":expected.as_ref().map(|i| i.mode & 0o7777),
                    "expected_device":expected.as_ref().map(|i| i.dev),
                    "expected_inode":expected.as_ref().map(|i| i.ino),
                    "target_digest":bytes.map(digest),
                    "target_mode":bytes.map(|_| mode),
                    "target_inode":staged.as_ref().map(|i| i.ino)
                }))?,
                0o600,
            )?;
            #[cfg(test)]
            checkpoint(Checkpoint::PreMoveSync, &stash.0)?;
            platform::sync_directory(&stash.0)?;
            platform::sync_directory(&self.0)?;
            self.fence(name, expected)?;
            after_fence();
            #[cfg(test)]
            checkpoint(Checkpoint::Move, &self.0)?;
            let moved = if bytes.is_some() {
                Self::move_entry(
                    &stash,
                    "replacement",
                    self,
                    name,
                    if expected.is_some() {
                        MoveKind::Exchange
                    } else {
                        MoveKind::NoReplace
                    },
                )?;
                if expected.is_some() {
                    Some("replacement")
                } else {
                    None
                }
            } else if expected.is_some() {
                Self::move_entry(self, name, &stash, "displaced", MoveKind::NoReplace)?;
                Some("displaced")
            } else {
                None
            };
            #[cfg(test)]
            checkpoint(Checkpoint::PostMoveSync, &stash.0)?;
            platform::sync_directory(&stash.0)?;
            platform::sync_directory(&self.0)?;
            #[cfg(test)]
            checkpoint(Checkpoint::Verify, &self.0)?;
            if let Some(displaced) = moved {
                if stash.read(displaced, self.1.read_limit).ok().as_ref() != Some(expected) {
                    // Only absence restoration admits a non-clobbering undo.
                    // Otherwise keep both images; never exchange back blindly.
                    if bytes.is_none() {
                        let _ =
                            Self::move_entry(&stash, displaced, self, name, MoveKind::NoReplace);
                        let _ = platform::sync_directory(&self.0);
                        let _ = platform::sync_directory(&stash.0);
                    }
                    return Err(Error(self.1.errors.recovery_required));
                }
            }
            // This observes replacement races, not later writes through externally held old FDs.
            let observed = self.read(name, self.1.read_limit)?;
            require(observed == staged, self.1.errors.recovery_required)?;
            #[cfg(test)]
            checkpoint(Checkpoint::Cleanup, &stash.0)?;
            for leaf in ["expected", "replacement", "displaced", "transaction.json"] {
                let leaf = CString::new(leaf).unwrap();
                if unsafe { libc::unlinkat(stash.0.as_raw_fd(), leaf.as_ptr(), 0) } != 0 {
                    require(
                        std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound,
                        self.1.errors.recovery_required,
                    )?;
                }
            }
            #[cfg(test)]
            checkpoint(Checkpoint::CleanupSync, &stash.0)?;
            platform::sync_directory(&stash.0)?;
            require(
                unsafe {
                    libc::unlinkat(
                        self.0.as_raw_fd(),
                        recovery_name.as_ptr(),
                        libc::AT_REMOVEDIR,
                    )
                } == 0,
                self.1.errors.recovery_required,
            )?;
            #[cfg(test)]
            checkpoint(Checkpoint::FinalSync, &self.0)
                .map_err(|_| Error(self.1.errors.durability_uncertain))?;
            platform::sync_directory(&self.0).map_err(|_| Error(self.1.errors.durability_uncertain))
        })();
        result.map_err(|e| {
            if e.code() == self.1.errors.durability_uncertain {
                e
            } else {
                Error(self.1.errors.recovery_required)
            }
        })
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn move_entry(
        from: &Root,
        source: &str,
        to: &Root,
        target: &str,
        kind: MoveKind,
    ) -> Result<()> {
        require(
            basename(source.as_bytes())
                && from.1.leaves.contains(&source)
                && basename(target.as_bytes())
                && to.1.leaves.contains(&target),
            from.1.errors.unsafe_leaf,
        )?;
        // Darwin EXCL may succeed for the same vnode in one directory. Production
        // moves must cross distinct opened directories, including through aliases.
        let source_dir = from.0.metadata()?;
        let target_dir = to.0.metadata()?;
        require(
            (source_dir.dev(), source_dir.ino()) != (target_dir.dev(), target_dir.ino()),
            from.1.errors.recovery_required,
        )?;
        let source = CString::new(source).unwrap();
        let target = CString::new(target).unwrap();
        platform::move_entry(from.0.as_raw_fd(), &source, to.0.as_raw_fd(), &target, kind)
            .map_err(|_| Error(from.1.errors.recovery_required))
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Checkpoint {
    StashAccess,
    StageAccess,
    StageWrite,
    StageSync,
    PreMoveSync,
    Move,
    PostMoveSync,
    Verify,
    Cleanup,
    CleanupSync,
    FinalSync,
}
