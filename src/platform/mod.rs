#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{
    apply_replacement_mode, ensure_supported, mkdir_private, open_append, open_readonly,
    open_workspace_lock, random_uuid_hex, rename_replace, replacement_mode, sync_directory,
    try_workspace_lock, unlock_workspace, verify_replacement_mode, workspace_argument,
    workspace_location,
};

#[cfg(not(unix))]
mod unsupported;
#[cfg(not(unix))]
pub(crate) use unsupported::{
    apply_replacement_mode, ensure_supported, mkdir_private, open_append, open_readonly,
    open_workspace_lock, random_uuid_hex, rename_replace, replacement_mode, sync_directory,
    try_workspace_lock, unlock_workspace, verify_replacement_mode, workspace_argument,
    workspace_location,
};

/// Actual resolved directory identity; persisted authority values are validated separately.
pub(crate) struct WorkspaceLocation {
    pub root: String,
    pub device: u64,
    pub inode: u64,
}
