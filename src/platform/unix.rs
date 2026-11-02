use crate::Result;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::{ffi::OsString, path::PathBuf};
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

pub(crate) fn ensure_supported() -> Result<()> {
    Ok(())
}

pub(crate) fn random_uuid_hex() -> Result<String> {
    uuid_hex_from_reader(File::open("/dev/urandom")?)
}

fn uuid_hex_from_reader(mut entropy: impl std::io::Read) -> Result<String> {
    let mut bytes = [0_u8; 16];
    entropy.read_exact(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(crate) fn workspace_location(root: &Path) -> Result<super::WorkspaceLocation> {
    let metadata = root.metadata()?;
    crate::error::require(metadata.is_dir(), "invalid_action_workspace")?;
    Ok(super::WorkspaceLocation {
        root: root
            .to_str()
            .ok_or(crate::Error("non_unicode_path_unsupported"))?
            .to_owned(),
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

pub(crate) fn open_readonly(path: &Path) -> Result<File> {
    // Match Python's descriptor-level regular-file check without FIFO blocking.
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?)
}

pub(crate) fn open_append(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    crate::error::require(file.metadata()?.is_file(), "unsafe_event_log_type")?;
    Ok(file)
}

pub(crate) fn open_generated(path: &Path, exclusive: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .mode(0o666)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    if exclusive {
        options.create_new(true);
    } else {
        options.create(true);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    crate::error::require(metadata.is_file(), "unsafe_generated_file_type")?;
    crate::error::require(metadata.nlink() == 1, "unsafe_generated_hardlink")?;
    // Verify the opened descriptor before truncation, not after O_TRUNC acted.
    if !exclusive {
        file.set_len(0)?;
    }
    Ok(file)
}

pub(crate) fn validate_generated_target(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            crate::error::require(metadata.is_file(), "unsafe_generated_file_type")?;
            crate::error::require(metadata.nlink() == 1, "unsafe_generated_hardlink")
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn replacement_mode(path: &Path) -> std::io::Result<Option<u32>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(metadata.mode() & 0o7777)),
        Ok(_) => Err(std::io::Error::other("unsafe replacement target type")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn apply_replacement_mode(file: &File, mode: u32) -> std::io::Result<()> {
    file.set_permissions(std::fs::Permissions::from_mode(mode & 0o7777))
}

pub(crate) fn verify_replacement_mode(file: &File, mode: u32) -> std::io::Result<()> {
    // fchmod may succeed while silently clearing SGID on a nonmember group.
    // Refuse before rename if the filesystem cannot retain the requested bits.
    if file.metadata()?.mode() & 0o7777 == mode & 0o7777 {
        Ok(())
    } else {
        Err(std::io::Error::other("replacement mode was not retained"))
    }
}

pub(crate) fn rename_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(source, target)
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
    // flock coordinates with the existing Python kernel lock on the same inode.
    let status = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if status == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if matches!(
        error.raw_os_error(),
        Some(libc::EAGAIN) | Some(libc::EACCES)
    ) {
        Ok(false)
    } else {
        Err(error)
    }
}

pub(crate) fn unlock_workspace(file: &File) {
    // Failure cannot safely be repaired by unlinking/replacing a persistent lock.
    unsafe {
        libc::flock(file.as_raw_fd(), libc::LOCK_UN);
    }
}

pub(crate) fn workspace_argument(path: &Path) -> Result<PathBuf> {
    let raw = path.as_os_str().as_bytes();
    if raw.is_empty() {
        return Ok(PathBuf::from("."));
    }
    if raw[0] != b'~' {
        return Ok(path.to_owned());
    }
    let end = raw
        .iter()
        .position(|byte| *byte == b'/')
        .unwrap_or(raw.len());
    // Avoid selecting a literal '~user' directory or implicit NSS/network lookup.
    if end != 1 {
        return Err(crate::Error("unsupported_named_user_expansion"));
    }
    let home = std::env::var_os("HOME").ok_or(crate::Error("home_expansion_unavailable"))?;
    let home = home.as_os_str().as_bytes();
    let end_home = home
        .iter()
        .rposition(|byte| *byte != b'/')
        .map_or(0, |index| index + 1);
    let mut expanded = home[..end_home].to_vec();
    expanded.extend_from_slice(&raw[end..]);
    if expanded.is_empty() {
        expanded.push(b'/');
    }
    Ok(PathBuf::from(OsString::from_vec(expanded)))
}

pub(crate) fn mkdir_private(path: &Path) -> std::io::Result<()> {
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

pub(crate) fn sync_directory(path: &Path) -> std::io::Result<()> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?
        .sync_all()
}

#[cfg(test)]
mod tests {
    #[test]
    fn os_identifiers_use_lowercase_uuid_v4_shape() {
        let id = super::random_uuid_hex().unwrap();
        assert_eq!(id.len(), 32);
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert_eq!(id.as_bytes()[12], b'4');
        assert!(matches!(id.as_bytes()[16], b'8' | b'9' | b'a' | b'b'));
    }

    #[test]
    fn identifiers_require_complete_successful_entropy_reads() {
        assert_eq!(
            super::uuid_hex_from_reader(&[0_u8; 16][..]).unwrap(),
            "00000000000040008000000000000000"
        );
        for bytes in [&[][..], &[0_u8; 15][..]] {
            assert_eq!(
                super::uuid_hex_from_reader(bytes).unwrap_err().code(),
                "input_or_storage_unavailable"
            );
        }
        struct Failing;
        impl std::io::Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("entropy unavailable"))
            }
        }
        assert_eq!(
            super::uuid_hex_from_reader(Failing).unwrap_err().code(),
            "input_or_storage_unavailable"
        );
    }

    #[test]
    fn event_append_descriptors_are_private_append_nonblocking_regular_files() {
        use std::io::Write;
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("event.jsonl");
        let mut file = super::open_append(&path).unwrap();
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0);
        assert_ne!(flags & libc::O_APPEND, 0);
        assert_ne!(flags & libc::O_NONBLOCK, 0);
        let fd_flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
        assert_ne!(fd_flags & libc::FD_CLOEXEC, 0);
        assert_eq!(
            file.metadata().unwrap().permissions().mode() & !0o600 & 0o7777,
            0
        );
        file.write_all(b"first\n").unwrap();
        drop(file);
        super::open_append(&path)
            .unwrap()
            .write_all(b"second\n")
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first\nsecond\n");
        let link = root.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(super::open_append(&link).is_err());
        let fifo = root.path().join("fifo");
        let c_path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        assert!(super::open_append(&fifo).is_err()); // no reader, must not block
        let _reader = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
            .unwrap();
        assert_eq!(
            super::open_append(&fifo).unwrap_err().code(),
            "unsafe_event_log_type"
        );
        assert!(super::open_append(std::path::Path::new("/dev/null")).is_err());
    }

    #[test]
    fn generated_outputs_verify_descriptor_before_truncation_and_deny_hardlinks() {
        use std::io::Write;
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{MetadataExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let authored = root.path().join("goal.md");
        let generated = root.path().join("agent.start.txt");
        std::fs::write(&authored, b"authored bytes").unwrap();
        std::fs::hard_link(&authored, &generated).unwrap();
        let before = std::fs::metadata(&authored).unwrap();
        assert_eq!(
            super::validate_generated_target(&generated)
                .unwrap_err()
                .code(),
            "unsafe_generated_hardlink"
        );
        assert_eq!(
            super::open_generated(&generated, false).unwrap_err().code(),
            "unsafe_generated_hardlink"
        );
        assert_eq!(std::fs::read(&authored).unwrap(), b"authored bytes");
        assert_eq!(std::fs::metadata(&authored).unwrap().ino(), before.ino());
        std::fs::remove_file(&generated).unwrap();
        let mut file = super::open_generated(&generated, true).unwrap();
        let inode = file.metadata().unwrap().ino();
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
        assert_ne!(flags & libc::O_NONBLOCK, 0);
        let fd_flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
        assert_ne!(fd_flags & libc::FD_CLOEXEC, 0);
        file.write_all(b"generated bytes").unwrap();
        drop(file);
        assert!(super::open_generated(&generated, true).is_err());
        assert_eq!(std::fs::read(&generated).unwrap(), b"generated bytes");
        let mut file = super::open_generated(&generated, false).unwrap();
        assert_eq!(file.metadata().unwrap().ino(), inode);
        assert_eq!(file.metadata().unwrap().len(), 0);
        file.write_all(b"regenerated").unwrap();
        assert_eq!(std::fs::read(&generated).unwrap(), b"regenerated");
        let link = root.path().join("link");
        symlink(&authored, &link).unwrap();
        assert!(super::open_generated(&link, false).is_err());
        assert_eq!(std::fs::read(&authored).unwrap(), b"authored bytes");
        assert_eq!(
            super::open_generated(std::path::Path::new("/dev/null"), false)
                .unwrap_err()
                .code(),
            "unsafe_generated_file_type"
        );
    }
}
