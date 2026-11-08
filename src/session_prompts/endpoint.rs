use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Endpoint {
    pub socket: crate::observation_socket::Endpoint,
    parent_device: u64,
    parent_inode: u64,
}

impl Endpoint {
    #[cfg(unix)]
    pub(super) fn select(address: &str) -> WireResult<Self> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        let io = |_| Failure::new("prompt_or_endpoint_unavailable");
        path(address).map_err(|e| Failure::new(e.code()))?;
        let (parent, _) = address
            .rsplit_once('/')
            .ok_or_else(|| Failure::new("invalid_prompt_path"))?;
        // A trailing slash makes lstat follow a symlink, bypassing the parent check.
        let parent = parent.trim_end_matches('/');
        let parent = if parent.is_empty() { "/" } else { parent };
        let info = std::fs::symlink_metadata(parent).map_err(io)?;
        if !info.is_dir()
            || info.uid() != unsafe { libc::getuid() }
            || info.mode() & 0o7777 != 0o700
        {
            return Err(Failure::new("unsafe_native_parent"));
        }
        let resolved = crate::platform::observation_socket::resolve(address)?;
        let leaf = std::fs::symlink_metadata(&resolved).map_err(io)?;
        let selected_leaf = std::fs::symlink_metadata(address).map_err(io)?;
        if !leaf.file_type().is_socket()
            || leaf.uid() != unsafe { libc::getuid() }
            || leaf.mode() & 0o7777 != 0o600
            || !selected_leaf.file_type().is_socket()
            || selected_leaf.dev() != leaf.dev()
            || selected_leaf.ino() != leaf.ino()
        {
            return Err(Failure::new("unsafe_native_socket"));
        }
        // Resolving ancestors must not select a different immediate parent.
        let resolved_parent = Path::new(&resolved)
            .parent()
            .ok_or_else(|| Failure::new("unsafe_native_parent"))?;
        let canonical_info = std::fs::symlink_metadata(resolved_parent).map_err(io)?;
        if canonical_info.dev() != info.dev() || canonical_info.ino() != info.ino() {
            return Err(Failure::new("native_endpoint_changed"));
        }
        Ok(Self {
            socket: crate::observation_socket::Endpoint {
                selected: address.into(),
                resolved,
                device: leaf.dev(),
                inode: leaf.ino(),
            },
            parent_device: info.dev(),
            parent_inode: info.ino(),
        })
    }

    #[cfg(not(unix))]
    pub(super) fn select(_: &str) -> WireResult<Self> {
        Err(Failure::new("unsupported_platform"))
    }

    pub(super) fn fence(&self) -> WireResult<()> {
        let current = Self::select(&self.socket.selected)
            .map_err(|_| Failure::new("native_endpoint_changed"))?;
        if current != *self {
            return Err(Failure::new("native_endpoint_changed"));
        }
        Ok(())
    }
}

#[cfg(unix)]
pub(super) fn prompt_file(address: &str) -> WireResult<String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let io = |_| Failure::new("prompt_or_endpoint_unavailable");
    let source = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(address)
        .map_err(io)?;
    if !source.metadata().map_err(io)?.is_file() {
        return Err(Failure::new("invalid_prompt_file"));
    }
    let mut raw = Vec::new();
    source
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(io)?;
    let text = String::from_utf8(raw).map_err(|_| Failure::new("invalid_prompt_text"))?;
    if text.len() > LIMIT
        || text.trim().is_empty()
        || text.chars().any(|c| {
            (c <= '\u{1f}' && !matches!(c, '\t' | '\r' | '\n'))
                || ('\u{7f}'..='\u{9f}').contains(&c)
        })
    {
        return Err(Failure::new("invalid_prompt_text"));
    }
    Ok(text)
}

#[cfg(not(unix))]
pub(super) fn prompt_file(_: &str) -> WireResult<String> {
    Err(Failure::new("unsupported_platform"))
}
