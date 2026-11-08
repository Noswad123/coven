#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{
    apply_replacement_mode, ensure_supported, mkdir_private, open_append, open_generated,
    open_readonly, open_workspace_lock, random_uuid_hex, rename_replace, replacement_mode,
    sync_directory, try_workspace_lock, unlock_workspace, validate_generated_target,
    verify_replacement_mode, workspace_argument, workspace_location,
};
#[cfg(unix)]
pub(crate) use unix::{live_artifact, observation_socket};
#[cfg(unix)]
pub(crate) use unix::{runtime_foreground_run, runtime_terminal_ttys};
#[cfg(not(unix))]
pub(crate) use unsupported::{live_artifact, observation_socket};
#[cfg(not(unix))]
pub(crate) use unsupported::{runtime_foreground_run, runtime_terminal_ttys};

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
pub(crate) use unix::{
    open_transport_lock, runtime_command_run, try_transport_lock, unlock_transport,
};
#[cfg(not(unix))]
pub(crate) use unsupported::{
    open_transport_lock, runtime_command_run, try_transport_lock, unlock_transport,
};

/// Executable/descriptor metadata for bounded observation, not authenticated identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileMetadata {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub mtime_ns: i128,
}

#[cfg(unix)]
pub(crate) use unix::{descriptor_metadata, executable_metadata, process_run, supervisor_entry};
#[cfg(not(unix))]
pub(crate) use unsupported::{
    descriptor_metadata, executable_metadata, process_run, supervisor_entry,
};

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
