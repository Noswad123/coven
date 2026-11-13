use super::*;
use std::{
    fs::symlink_metadata,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::Component,
};

pub(super) fn text(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or(Error("integration_setup_absolute_path_required"))
}

pub(super) fn absolute(path: &Path) -> Result<()> {
    let s = text(path)?;
    require(
        path.is_absolute()
            && !s.chars().any(char::is_control)
            && (s == "/"
                || s[1..]
                    .split('/')
                    .all(|c| !c.is_empty() && c != "." && c != "..")),
        "integration_setup_absolute_path_required",
    )
}

/// Validate every existing component without resolving aliases. Missing tails
/// are allowed, but no metadata/read errors other than ENOENT are absence.
pub(super) fn physical(path: &Path) -> Result<bool> {
    absolute(path)?;
    let mut walked = PathBuf::from("/");
    for component in path.components() {
        if let Component::Normal(name) = component {
            walked.push(name);
            match symlink_metadata(&walked) {
                Ok(m) => require(m.is_dir(), "integration_setup_unsafe_root")?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(_) => return Err(Error("integration_setup_unsafe_root")),
            }
        }
    }
    Ok(true)
}

pub(super) fn config(selected: Option<&Path>) -> Result<PathBuf> {
    let path = if let Some(path) = selected {
        path.to_owned()
    } else if let Some(path) = std::env::var_os("KITSUNE_CONFIG_PATH") {
        PathBuf::from(path)
    } else if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(path).join("kitsune/config.toml")
    } else {
        PathBuf::from(std::env::var_os("HOME").ok_or(Error("integration_setup_home_required"))?)
            .join(".config/kitsune/config.toml")
    };
    absolute(&path)?;
    require(
        path.file_name().is_some_and(|p| p == CONFIG),
        "integration_setup_config_leaf_required",
    )?;
    Ok(path)
}

pub(super) fn executable(path: &Path) -> Result<()> {
    absolute(path)?;
    require(
        physical(
            path.parent()
                .ok_or(Error("integration_setup_absolute_path_required"))?,
        )?,
        "integration_setup_executable_unavailable",
    )?;
    let m = symlink_metadata(path)?;
    require(
        m.is_file() && m.mode() & 0o111 != 0 && m.mode() & 0o022 == 0,
        "integration_setup_unsafe_executable",
    )
}

pub(super) fn state_root(path: &Path) -> Result<()> {
    absolute(path)?;
    if physical(path)? {
        let m = symlink_metadata(path)?;
        require(
            m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o7777 == 0o700,
            "integration_setup_unsafe_state_root",
        )?;
    }
    // Match integration_runtime's 24-hex address key and i/KEY/r.sock layout
    // without opening/binding a socket. Reserve the terminating NUL.
    let probe = path.join("i").join("0".repeat(24)).join("r.sock");
    let capacity = unsafe { std::mem::zeroed::<libc::sockaddr_un>() }
        .sun_path
        .len();
    require(
        probe.as_os_str().as_bytes().len() < capacity,
        "integration_setup_socket_path_too_long",
    )?;
    // Use runtime's read-only ACL/creation admission before setup can publish
    // external ownership, including when only an ancestor currently exists.
    crate::integration_runtime::preflight_private_directory(path)
}
