#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{
    ensure_supported, mkdir_private, open_readonly, open_workspace_lock, sync_directory,
    try_workspace_lock, unlock_workspace, workspace_argument,
};

#[cfg(not(unix))]
mod unsupported;
#[cfg(not(unix))]
pub(crate) use unsupported::{
    ensure_supported, mkdir_private, open_readonly, open_workspace_lock, sync_directory,
    try_workspace_lock, unlock_workspace, workspace_argument,
};
