#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{
    apply_replacement_mode, ensure_supported, mkdir_private, open_append, open_generated,
    open_readonly, open_workspace_lock, random_uuid_hex, rename_replace, replacement_mode,
    sync_directory, try_workspace_lock, unlock_workspace, validate_generated_target,
    verify_replacement_mode, workspace_argument, workspace_location,
};

#[cfg(not(unix))]
mod unsupported;
#[cfg(not(unix))]
pub(crate) use unsupported::{
    apply_replacement_mode, ensure_supported, mkdir_private, open_append, open_generated,
    open_readonly, open_workspace_lock, random_uuid_hex, rename_replace, replacement_mode,
    sync_directory, try_workspace_lock, unlock_workspace, validate_generated_target,
    verify_replacement_mode, workspace_argument, workspace_location,
};

/// Actual resolved directory identity; persisted authority values are validated separately.
pub(crate) struct WorkspaceLocation {
    pub root: String,
    pub device: u64,
    pub inode: u64,
}

#[cfg(unix)]
pub(crate) use unix::init::{
    open_init_directory, open_init_lock, resolve_init_target, validate_init_identity,
    validate_init_temp,
};
#[cfg(not(unix))]
pub(crate) use unsupported::{
    open_init_directory, open_init_lock, resolve_init_target, validate_init_identity,
    validate_init_temp,
};
