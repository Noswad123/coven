//! Attached Coven bootstrap and read-only Kit reporter route resolution.
//! Receipts fence filesystem generations; they do not authenticate native callers.
use crate::{Error, Result, error::require, task_mutation::TaskOutput};
use std::path::{Path, PathBuf};

#[cfg(unix)]
mod private_fs;
#[cfg(unix)]
use private_fs::Directory;

pub fn default_state_root() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_STATE_HOME") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .ok_or(Error("integration_home_required"))?,
        )
        .join(".local/state"),
    };
    let root = base.join("coven/integration");
    #[cfg(unix)]
    private_fs::valid_path(&root)?;
    require(root.is_absolute(), "integration_invalid_path")?;
    Ok(root)
}

/// Creates only missing directories; never repairs an existing owner/mode/ACL.
pub(crate) fn ensure_private_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        Directory::open(path, true)?.sync()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(Error("integration_privacy_unsupported"))
    }
}

/// Read-only runtime privacy admission. Missing tails are admitted only when the
/// nearest existing parent is safe for creation, including inherited ACL policy.
pub(crate) fn preflight_private_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        private_fs::preflight(path)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(Error("integration_privacy_unsupported"))
    }
}

#[cfg(all(test, unix))]
fn checkpoint(stage: &str) {
    use std::io::{Read, Write};
    thread_local! {
        static SEEN: std::cell::RefCell<std::collections::BTreeSet<String>> = Default::default();
    }
    if !std::env::var("COVEN_RUNTIME_TEST_CHECKPOINTS")
        .unwrap_or_default()
        .split(',')
        .any(|s| s == stage)
        || !SEEN.with(|seen| seen.borrow_mut().insert(stage.into()))
    {
        return;
    }
    let control = std::env::var_os("COVEN_RUNTIME_TEST_CONTROL").expect("checkpoint control");
    let mut stream = std::os::unix::net::UnixStream::connect(control).expect("checkpoint connect");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    stream.write_all(format!("{stage}\n").as_bytes()).unwrap();
    stream.read_exact(&mut [0]).expect("checkpoint release");
}

pub fn runtime_arguments(state_root: Option<&Path>, json: bool) -> Result<TaskOutput> {
    #[cfg(unix)]
    {
        unix::runtime(state_root, json)
    }
    #[cfg(not(unix))]
    {
        let _ = (state_root, json);
        Err(Error("integration_privacy_unsupported"))
    }
}

/// Read-only resolution before any native ID is handed to the existing private client.
/// The returned string cannot fence the Kit API after the later client reply. The
/// private client independently revalidates its receiver, not an atomic API binding.
pub fn reporter_socket(state_root: Option<&Path>) -> Result<String> {
    #[cfg(unix)]
    {
        unix::reporter(state_root)
    }
    #[cfg(not(unix))]
    {
        let _ = state_root;
        Err(Error("integration_privacy_unsupported"))
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::{
        agent_management::RunRequest,
        agent_management_presentation::{self, PresentationRequest},
        observation_socket::{self, Endpoint},
    };
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use std::{
        fs::File,
        os::unix::fs::MetadataExt,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::Duration,
    };

    const RECEIPT: &std::ffi::CStr = c"receipt.json";
    const LOCK: &std::ffi::CStr = c"instance.lock";
    const MARKER: &[u8] = b"{\"name\":\"Coven integration receiver\",\"agents\":[]}";

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Receipt {
        version: u32,
        address: String,
        resolved: String,
        api: (u64, u64),
        nonce: String,
        released: bool,
        receiver: Option<(u64, u64)>,
    }
    impl Receipt {
        fn read(dir: &Directory) -> Result<Self> {
            let bytes = dir.read(RECEIPT)?;
            crate::strict_json::check_depth(&bytes, 8)?;
            let receipt: Self = serde_json::from_slice(&bytes)?;
            require(
                receipt.version == 1
                    && receipt.nonce.len() == 32
                    && receipt.nonce.bytes().all(|b| b.is_ascii_hexdigit()),
                "integration_invalid_receipt",
            )?;
            Ok(receipt)
        }
        fn matches(&self, api: &Endpoint) -> bool {
            self.address == api.selected
                && self.resolved == api.resolved
                && self.api == (api.device, api.inode)
        }
        fn write(&self, dir: &Directory, file: &mut File) -> Result<()> {
            dir.write(RECEIPT, file, &serde_json::to_vec(self)?)
        }
    }
    fn env(name: &str) -> Result<String> {
        std::env::var(name).map_err(|_| Error("integration_context_required"))
    }
    fn api() -> Result<Endpoint> {
        observation_socket::select(&env("KITSUNE_SOCKET_PATH")?)
            .map_err(|_| Error("integration_api_unavailable"))
    }
    fn fence_api(endpoint: &Endpoint) -> Result<()> {
        let current = observation_socket::select(&endpoint.selected)
            .map_err(|_| Error("integration_api_replaced"))?;
        require(current == *endpoint, "integration_api_replaced")
    }
    fn key(address: &str) -> String {
        format!("{:x}", Sha256::digest(address.as_bytes()))[..24].into()
    }
    fn paths(root: &Path, endpoint: &Endpoint) -> Result<(PathBuf, String)> {
        private_fs::valid_path(root)?;
        let instance = root.join("i").join(key(&endpoint.selected));
        let socket = instance
            .join("r.sock")
            .to_str()
            .ok_or(Error("integration_invalid_path"))?
            .to_owned();
        // Reserve the terminating NUL, using the actual platform sockaddr layout.
        let addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        require(
            socket.len() < addr.sun_path.len(),
            "integration_socket_path_too_long",
        )?;
        Ok((instance, socket))
    }

    fn route(argv_json: &str, override_root: Option<&Path>, executable: &Path) -> Result<PathBuf> {
        require(argv_json.len() <= 16384, "integration_invalid_report_route")?;
        let argv: Vec<String> = serde_json::from_str(argv_json)
            .map_err(|_| Error("integration_invalid_report_route"))?;
        require(
            argv.len() == 6
                && argv[1..5] == ["agent", "report-native", "--from-kitsune", "--state-root"],
            "integration_invalid_report_route",
        )?;
        let selected = Path::new(&argv[0]);
        private_fs::valid_path(selected)?;
        // Canonical selection admits an installed executable alias but never executes argv.
        let selected = selected.canonicalize()?;
        let expected = executable.canonicalize()?;
        require(
            selected == expected,
            "integration_report_executable_mismatch",
        )?;
        let m = std::fs::metadata(&selected)?;
        require(
            m.is_file() && m.mode() & 0o111 != 0 && m.mode() & 0o022 == 0,
            "integration_unsafe_executable",
        )?;
        let root = PathBuf::from(&argv[5]);
        private_fs::valid_path(&root)?;
        if let Some(root) = override_root {
            private_fs::valid_path(root)?;
        }
        require(
            override_root.is_none_or(|r| r == root),
            "integration_report_root_mismatch",
        )?;
        Ok(root)
    }

    pub(super) fn reporter(override_root: Option<&Path>) -> Result<String> {
        require(
            env("KITSUNE_AGENT_MANAGEMENT")? == "external"
                && env("KITSUNE_INTEGRATION_SOURCE")? == "coven",
            "integration_context_required",
        )?;
        let root = route(
            &env("KITSUNE_INTEGRATION_REPORT_COMMAND")?,
            override_root,
            &std::env::current_exe()?,
        )?;
        resolve(&root, &api()?)
    }
    fn resolve(root: &Path, endpoint: &Endpoint) -> Result<String> {
        let (instance, socket) = paths(root, endpoint)?;
        let _root = Directory::open(root, false)?;
        let _instances = Directory::open(&root.join("i"), false)?;
        let dir = Directory::open(&instance, false)?;
        // Inspect, but do not create or acquire any locks in reporter mode.
        let lock = dir.file(LOCK, false)?;
        let before = Receipt::read(&dir)?;
        require(
            !before.released && before.matches(endpoint) && before.receiver.is_some(),
            "integration_inactive_generation",
        )?;
        require(
            dir.socket()? == before.receiver,
            "integration_receiver_replaced",
        )?;
        fence_api(endpoint)?;
        require(
            Receipt::read(&dir)? == before,
            "integration_generation_replaced",
        )?;
        require(
            dir.socket()? == before.receiver,
            "integration_receiver_replaced",
        )?;
        dir.fence_file(LOCK, &lock)?;
        _root.fence()?;
        _instances.fence()?;
        fence_api(endpoint)?;
        Ok(socket)
    }

    pub(super) fn runtime(override_root: Option<&Path>, json: bool) -> Result<TaskOutput> {
        // Kit intentionally strips reporter route environment from the generic helper.
        let endpoint = api()?;
        let root = override_root
            .map(Path::to_owned)
            .map(Ok)
            .unwrap_or_else(default_state_root)?;
        let (instance, socket) = paths(&root, &endpoint)?;
        let interrupt = observation_socket::InterruptGuard::install()?;
        let root_dir = Directory::open(&root, true)?;
        let instances = Directory::open(&root.join("i"), true)?;
        // Serializes count admission and initial reservation, not receiver lifetimes.
        let registry = root_dir.registry_lock(&interrupt, Duration::from_secs(3))?;
        #[cfg(test)]
        super::checkpoint("after_registry_lock");
        let name = std::ffi::CString::new(key(&endpoint.selected)).unwrap();
        let new = instances.stat(&name)?.is_none();
        if new {
            require(
                instances.count()? < 64,
                "integration_instance_limit_cleanup_required",
            )?;
        }
        let dir = Directory::open(&instance, new)?;
        let lock = dir.lock(LOCK, new)?;
        require(
            dir.stat(c"r.sock")?.is_none(),
            "integration_socket_conflict",
        )?;
        if !new {
            let previous = Receipt::read(&dir)?;
            require(
                previous.released && previous.address == endpoint.selected,
                "integration_unreleased_state",
            )?;
            // A new API inode is allowed only after a clean release and absent receiver.
            let workspace = Directory::open(&instance.join("w"), false)?;
            require(
                workspace.read(c"coven.json")? == MARKER,
                "integration_invalid_workspace",
            )?;
            Directory::open(&instance.join("w/state"), false)?;
        }
        let mut receipt = Receipt {
            version: 1,
            address: endpoint.selected.clone(),
            resolved: endpoint.resolved.clone(),
            api: (endpoint.device, endpoint.inode),
            nonce: crate::platform::random_uuid_hex()?,
            released: false,
            receiver: None,
        };
        // Retain a writable descriptor; never reopen an existing receipt for truncation.
        let mut file = if new {
            dir.file(RECEIPT, true)?
        } else {
            // Read-only validation above precedes this descriptor-relative writable open.
            dir.writable(RECEIPT)?
        };
        receipt.write(&dir, &mut file)?;
        if new {
            let workspace = Directory::open(&instance.join("w"), true)?;
            let mut marker = workspace.file(c"coven.json", true)?;
            workspace.write(c"coven.json", &mut marker, MARKER)?;
            Directory::open(&instance.join("w/state"), true)?.sync()?;
        }
        dir.sync()?;
        instances.sync()?;
        root_dir.fence_file(c"registry.lock", &registry)?;
        drop(registry);
        fence_api(&endpoint)?;
        drop(interrupt);

        let done = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));
        let failed = cancelled.clone();
        let stop = done.clone();
        let monitor_path = instance.clone();
        let mut active = receipt.clone();
        let monitor = thread::Builder::new()
            .name("coven-integration-receipt".into())
            .spawn(move || -> Result<()> {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let dir = Directory::open(&monitor_path, false)?;
                    loop {
                        if stop.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        dir.fence()?;
                        // bind/chmod and owned cleanup can race this observer. Publish
                        // only a fully admitted node; receiver errors still retain the
                        // active transaction, and successful EOF must verify absence.
                        if let Ok(Some(id)) = dir.socket() {
                            #[cfg(test)]
                            super::checkpoint("before_publish");
                            fence_api(&endpoint)?;
                            active.receiver = Some(id);
                            active.write(&dir, &mut file)?;
                            return Ok(());
                        }
                        thread::sleep(Duration::from_millis(1));
                    }
                }))
                .unwrap_or(Err(Error("integration_receipt_worker_failed")));
                if result.is_err() {
                    failed.store(true, Ordering::Release);
                }
                result
            })?;
        let result = agent_management_presentation::run_arguments_with_cancel(
            &RunRequest {
                workspace: instance.join("w"),
                socket,
            },
            &PresentationRequest {
                socket: receipt.address.clone(),
                source: "coven".into(),
            },
            json,
            Some(&cancelled),
        );
        done.store(true, Ordering::Release);
        let monitored = monitor
            .join()
            .map_err(|_| Error("integration_receipt_worker_failed"))?;
        monitored?;
        let output = result?;
        root_dir.fence()?;
        instances.fence()?;
        dir.fence_file(LOCK, &lock)?;
        require(
            dir.stat(c"r.sock")?.is_none(),
            "integration_receiver_cleanup_unverified",
        )?;
        let current = Receipt::read(&dir)?;
        require(
            current.nonce == receipt.nonce && !current.released,
            "integration_generation_replaced",
        )?;
        receipt = current;
        receipt.released = true;
        let mut file = dir.writable(RECEIPT)?;
        receipt.write(&dir, &mut file)?;
        root_dir.fence()?;
        instances.fence()?;
        Ok(output)
    }

    #[cfg(test)]
    #[path = "tests.rs"]
    mod tests;
}
