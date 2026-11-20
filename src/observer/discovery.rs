//! Local hint registry. Callers must authorize the source before invoking scan.
use super::*;
use crate::session_prompts::{endpoint::Endpoint, protocol::uuid};
use crate::shared_native::{Connection, Grant};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    contract: String,
    version: u8,
    native_socket: String,
    expected_runtime: String,
}

#[derive(Clone)]
pub(super) struct Selected {
    pub endpoint: Endpoint,
    pub runtime: String,
    pub grant: Grant,
    pub expires: Instant,
}

#[cfg(unix)]
pub(super) fn scan(directory: &str, deadline: Instant) -> Result<Vec<Selected>> {
    use std::{
        fs::{File, OpenOptions},
        io::Read,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
    };
    let invalid = || Error("discovery_unavailable");
    if Instant::now() >= deadline {
        return Err(invalid());
    }
    let root = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(directory)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(invalid()),
    };
    let info = root.metadata().map_err(|_| invalid())?;
    let private = |m: &std::fs::Metadata, mode| {
        m.uid() == unsafe { libc::getuid() } && m.mode() & 0o7777 == mode
    };
    if !info.is_dir()
        || !private(&info, 0o700)
        || std::fs::canonicalize(directory).map_err(|_| invalid())?
            != std::path::Path::new(directory)
    {
        return Err(invalid());
    }
    let mut descriptors = Vec::new();
    let mut runtimes = HashSet::new();
    let mut endpoints = HashSet::new();
    // Count all entries, not just matching filenames. Enumeration is never unbounded.
    for (count, entry) in std::fs::read_dir(directory)
        .map_err(|_| invalid())?
        .enumerate()
    {
        if count >= 64 || Instant::now() >= deadline {
            return Err(invalid());
        }
        let entry = entry.map_err(|_| invalid())?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(invalid)?;
        if !name.ends_with(".json") {
            continue;
        }
        if descriptors.len() >= 32 {
            return Err(invalid());
        }
        let cname = std::ffi::CString::new(name).map_err(|_| invalid())?;
        let fd = unsafe {
            libc::openat(
                root.as_raw_fd(),
                cname.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(invalid());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let metadata = file.metadata().map_err(|_| invalid())?;
        if !metadata.is_file()
            || !private(&metadata, 0o600)
            || metadata.len() > 8192
            || metadata.nlink() != 1
        {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        file.take(8193)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        if bytes.len() > 8192 {
            return Err(invalid());
        }
        // Authority parsing rejects duplicate fields before serde's closed schema.
        let parsed = crate::values::parse(&bytes, crate::values::Profile::Authority)
            .map_err(|_| invalid())?;
        let descriptor: Descriptor = serde_json::from_value(
            crate::observation_wire::to_json(&parsed).map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?;
        if descriptor.contract != "djinn.web-discovery.v1"
            || descriptor.version != 1
            || !uuid(&descriptor.expected_runtime)
            || name != format!("{}.json", descriptor.expected_runtime)
            || descriptor.native_socket.len() > 100
            || descriptor
                .native_socket
                .split('/')
                .any(|s| matches!(s, "." | ".."))
            || !runtimes.insert(descriptor.expected_runtime.clone())
            || !endpoints.insert(descriptor.native_socket.clone())
        {
            return Err(invalid());
        }
        descriptors.push(descriptor);
    }
    let current = std::fs::symlink_metadata(directory).map_err(|_| invalid())?;
    if !current.is_dir()
        || current.dev() != info.dev()
        || current.ino() != info.ino()
        || !private(&current, 0o700)
    {
        return Err(invalid());
    }
    let mut selected = Vec::new();
    let mut identities = HashSet::new();
    for descriptor in descriptors {
        if Instant::now() >= deadline {
            return Err(invalid());
        }
        let Ok(endpoint) = Endpoint::select(&descriptor.native_socket) else {
            continue;
        };
        if endpoint.socket.resolved != descriptor.native_socket {
            return Err(invalid());
        }
        if !identities.insert((endpoint.socket.device, endpoint.socket.inode)) {
            return Err(invalid());
        }
        let mut connection = Connection::new(&endpoint, &descriptor.expected_runtime, deadline);
        if connection.capabilities().is_err() {
            continue;
        }
        // Start the lease before the request, not after the network round trip.
        let started = Instant::now();
        if let Ok(Some(grant)) = connection.status() {
            let expires = started + Duration::from_millis(grant.lease_ms);
            if Instant::now() < expires {
                selected.push(Selected {
                    endpoint,
                    runtime: descriptor.expected_runtime,
                    grant,
                    expires,
                });
            }
        }
    }
    Ok(selected)
}

#[cfg(not(unix))]
pub(super) fn scan(_: &str, _: Instant) -> Result<Vec<Selected>> {
    Err(Error("discovery_unavailable"))
}
