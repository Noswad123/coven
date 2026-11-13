use super::*;
use crate::observation_socket::{NoCancel, ReadBudget};
use std::{
    ffi::CString,
    io::{self, Read, Write},
    os::unix::{
        ffi::OsStrExt,
        io::AsRawFd,
        net::{UnixListener, UnixStream},
    },
    thread,
    time::{Duration, Instant},
};

struct Selected {
    path: PathBuf,
    parent: files::Directory,
    leaf: CString,
}

fn selected(path: &str) -> Result<Selected> {
    require(
        crate::private_access::supports_privacy(),
        "agent_unsafe_parent",
    )?;
    crate::observation_socket::validate_path(path)?;
    require(
        !path.split('/').any(|part| part == ".") && !path.contains("//"),
        "agent_invalid_socket",
    )?;
    let path = PathBuf::from(path);
    let parent = files::Directory::open(path.parent().ok_or(Error("agent_invalid_socket"))?)?;
    let leaf = CString::new(
        path.file_name()
            .ok_or(Error("agent_invalid_socket"))?
            .as_bytes(),
    )
    .map_err(|_| Error("agent_invalid_socket"))?;
    let selected = Selected { path, parent, leaf };
    selected.fence()?;
    Ok(selected)
}

impl Selected {
    fn fence(&self) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        // Re-walk the physical name and repeat held-FD identity and OS privacy checks.
        self.parent.fence()?;
        let metadata = self.parent.file().metadata()?;
        require(metadata.mode() & 0o7777 == 0o700, "agent_unsafe_parent")
    }
}

struct Node {
    selected: Selected,
    inode: (u64, u64),
}

// Darwin dev_t is signed and mode_t is u16; Linux uses u64/u32 here.
// Keep the flat policy ABI's conversions explicit across platform widths.
#[allow(clippy::unnecessary_cast)]
fn socket_identity(stat: &libc::stat) -> (u64, u64) {
    (stat.st_dev as u64, stat.st_ino as u64)
}

#[allow(clippy::unnecessary_cast)] // mode_t varies between Darwin u16 and Linux u32.
fn socket_mode(stat: &libc::stat) -> u32 {
    stat.st_mode as u32 & 0o7777
}

impl Node {
    fn capture(selected: Selected) -> Result<(Self, u32)> {
        let stat = selected
            .parent
            .stat(&selected.leaf)?
            .ok_or(Error("agent_unsafe_socket"))?;
        let node = Self {
            selected,
            inode: socket_identity(&stat),
        };
        Ok((node, socket_mode(&stat)))
    }

    fn fence_mode(&self, mode: u32) -> Result<()> {
        self.selected.fence()?;
        let before = self.stat(mode)?;
        crate::private_access::validate_socket_entry(
            self.selected.parent.file(),
            &self.selected.leaf,
            self.inode.0,
            self.inode.1,
            before.st_uid,
            socket_mode(&before),
        )?;
        self.stat(mode)?;
        self.selected.fence()
    }

    fn stat(&self, mode: u32) -> Result<libc::stat> {
        let stat = self
            .selected
            .parent
            .stat(&self.selected.leaf)?
            .ok_or(Error("agent_endpoint_replaced"))?;
        require(
            stat.st_mode & libc::S_IFMT == libc::S_IFSOCK
                && stat.st_nlink == 1
                && stat.st_uid == unsafe { libc::geteuid() }
                && socket_mode(&stat) == mode & 0o7777
                && socket_identity(&stat) == self.inode,
            "agent_unsafe_socket",
        )?;
        Ok(stat)
    }

    fn fence(&self) -> Result<()> {
        self.fence_mode(0o600)
    }

    fn chmod(&self) -> io::Result<()> {
        // No pathname-following fallback, including on older Linux kernels.
        #[cfg(target_os = "linux")]
        let result = unsafe {
            let mode: libc::mode_t = 0o600;
            libc::syscall(
                libc::SYS_fchmodat2,
                self.selected.parent.file().as_raw_fd(),
                self.selected.leaf.as_ptr(),
                mode,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        #[cfg(target_os = "macos")]
        let result = unsafe {
            libc::fchmodat(
                self.selected.parent.file().as_raw_fd(),
                self.selected.leaf.as_ptr(),
                0o600,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let result = -1;
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

struct Bound {
    listener: UnixListener,
    node: Node,
}

impl Bound {
    fn bind(path: &str) -> Result<Self> {
        Self::bind_inner_with_chmod(path, Node::chmod)
    }

    fn bind_inner_with_chmod(
        path: &str,
        chmod: impl FnOnce(&Node) -> io::Result<()>,
    ) -> Result<Self> {
        let selected = selected(path)?;
        selected.fence()?;
        // bind is exclusive and also starts listening. The admitted private parent
        // protects the pre-chmod interval even with umask 000. Never unlink stale leaves.
        let listener =
            UnixListener::bind(&selected.path).map_err(|_| Error("agent_socket_bind_failed"))?;
        let (node, mode) = Node::capture(selected)?;
        let bound = Self { listener, node };
        #[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
        tests::checkpoint("post_bind");
        bound.node.fence_mode(mode)?;
        chmod(&bound.node).map_err(|_| Error("agent_socket_chmod_failed"))?;
        bound.listener.set_nonblocking(true)?;
        bound.fence()?;
        Ok(bound)
    }

    fn fence(&self) -> Result<()> {
        self.node.fence()
    }
}

impl Drop for Bound {
    fn drop(&mut self) {
        // Failed startup may still have the original pre-chmod mode. Cleanup is
        // identity-bound, but never proceeds through a changed/nonprivate parent.
        if self
            .node
            .selected
            .parent
            .stat(&self.node.selected.leaf)
            .ok()
            .flatten()
            .is_some_and(|s| self.node.fence_mode(socket_mode(&s)).is_ok())
        {
            unsafe {
                libc::unlinkat(
                    self.node.selected.parent.file().as_raw_fd(),
                    self.node.selected.leaf.as_ptr(),
                    0,
                );
            }
        }
    }
}

fn publish(projection: &Option<Arc<Mutex<Value>>>, store: &store::Store) {
    // A presentation reader must never hold up receipt processing.
    if let Some(shared) = projection {
        if let Ok(mut latest) = shared.try_lock() {
            *latest = store.projection();
        }
    }
}

pub(super) fn serve(
    request: &ServeRequest,
    json_mode: bool,
    projection: Option<Arc<Mutex<Value>>>,
) -> Result<TaskOutput> {
    // This guard must outlive socket cleanup, including every startup failure path.
    let interrupt = crate::observation_socket::InterruptGuard::install()?;
    let bound = Bound::bind(&request.socket)?;
    let mut store = store::Store::open(&request.workspace)?;
    publish(&projection, &store);
    let deadline = Instant::now() + Duration::from_secs(request.duration);
    while Instant::now() < deadline && !interrupt.interrupted() {
        bound.fence()?;
        store.fence()?;
        match bound.listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(true)?;
                let connection_deadline = deadline.min(Instant::now() + Duration::from_millis(200));
                let reply =
                    receive(&mut stream, connection_deadline, &interrupt).and_then(|bytes| {
                        bound.fence()?;
                        check(connection_deadline, &interrupt)?;
                        handle(&mut store, &bytes)
                    });
                if let Ok(reply) = reply {
                    publish(&projection, &store);
                    if bound.fence().is_ok() {
                        let _ = send(&mut stream, &reply, connection_deadline, &interrupt);
                    }
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::Interrupted =>
            {
                thread::sleep(Duration::from_millis(5))
            }
            Err(e) => return Err(e.into()),
        }
        publish(&projection, &store);
    }
    output(
        json!({"type":"stopped","contract":CONTRACT,"server_epoch":store.epoch,"authorization":"none"}),
        json_mode,
    )
}

fn handle(store: &mut store::Store, bytes: &[u8]) -> Result<Vec<u8>> {
    let request = decode(bytes, 8192)?;
    let id = text(&request, "id")?;
    require(
        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "agent_invalid_schema",
    )?;
    let result = (|| {
        require(
            request.as_object().is_some_and(|o| o.len() == 4)
                && text(&request, "contract")? == CONTRACT,
            "agent_contract_mismatch",
        )?;
        let params = request.get("params").ok_or(Error("agent_invalid_schema"))?;
        match text(&request, "method")? {
            "report" => store.report(params),
            "inventory" => {
                require(
                    params.as_object().is_some_and(|o| o.is_empty()),
                    "agent_invalid_schema",
                )?;
                store.fence()?;
                Ok(store.projection())
            }
            _ => Err(Error("agent_unknown_method")),
        }
    })();
    let value = match result {
        Ok(result) => json!({"id":id,"contract":CONTRACT,"type":"result","result":result}),
        Err(error) => {
            json!({"id":id,"contract":CONTRACT,"type":"error","error":{"code":error.code()}})
        }
    };
    let mut bytes = serde_json::to_vec(&value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn check(deadline: Instant, interrupt: &crate::observation_socket::InterruptGuard) -> Result<()> {
    require(!interrupt.interrupted(), "agent_interrupted")?;
    require(Instant::now() < deadline, "agent_connection_timeout")
}

fn receive(
    stream: &mut UnixStream,
    deadline: Instant,
    interrupt: &crate::observation_socket::InterruptGuard,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        check(deadline, interrupt)?;
        let mut chunk = [0; 1024];
        match stream.read(&mut chunk) {
            Ok(0) => return Err(Error("agent_incomplete_frame")),
            Ok(n) => {
                bytes.extend_from_slice(&chunk[..n]);
                require(bytes.len() <= 8192, "agent_payload_limit")?;
                if let Some(index) = bytes.iter().position(|b| *b == b'\n') {
                    require(index + 1 == bytes.len(), "agent_invalid_frame")?;
                    return Ok(bytes);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2))
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
}

fn send(
    stream: &mut UnixStream,
    mut bytes: &[u8],
    deadline: Instant,
    interrupt: &crate::observation_socket::InterruptGuard,
) -> Result<()> {
    while !bytes.is_empty() {
        check(deadline, interrupt)?;
        match stream.write(bytes) {
            Ok(0) => return Err(Error("agent_connection_closed")),
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2))
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub(super) fn call(path: &str, method: &str, params: Value) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(1);
    let (node, _) = Node::capture(selected(path)?)?;
    node.fence()?;
    let endpoint =
        crate::observation_socket::select(path).map_err(|_| Error("agent_unsafe_socket"))?;
    let id = crate::platform::random_uuid_hex()?;
    let mut frame =
        serde_json::to_vec(&json!({"id":id,"contract":CONTRACT,"method":method,"params":params}))?;
    frame.push(b'\n');
    #[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
    tests::checkpoint("pre_connect");
    node.fence()?;
    crate::platform::observation_socket::fence(&endpoint)
        .map_err(|_| Error("agent_endpoint_replaced"))?;
    crate::observation_socket::check(deadline, &NoCancel)
        .map_err(|_| Error("agent_connection_timeout"))?;
    let socket =
        crate::platform::observation_socket::Socket::connect(&endpoint, deadline, &NoCancel)
            .map_err(|_| Error("agent_connect_failed"))?;
    #[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
    tests::checkpoint("pre_send");
    node.fence()?;
    crate::platform::observation_socket::fence(&endpoint)
        .map_err(|_| Error("agent_endpoint_replaced"))?;
    crate::observation_socket::check(deadline, &NoCancel)
        .map_err(|_| Error("agent_connection_timeout"))?;
    // From the first send onward failures are uncertain, never replayed.
    socket
        .send(&frame, deadline, &NoCancel)
        .map_err(|_| Error("agent_delivery_uncertain"))?;
    #[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
    tests::checkpoint("post_send");
    node.fence()
        .map_err(|_| Error("agent_delivery_uncertain"))?;
    let payload = socket
        .receive(deadline, &mut ReadBudget::new(256 * 1024), &NoCancel)
        .map_err(|_| Error("agent_delivery_uncertain"))?;
    crate::platform::observation_socket::fence(&endpoint)
        .map_err(|_| Error("agent_delivery_uncertain"))?;
    node.fence()
        .map_err(|_| Error("agent_delivery_uncertain"))?;
    crate::observation_socket::check(deadline, &NoCancel)
        .map_err(|_| Error("agent_delivery_uncertain"))?;
    let reply = decode(&payload, 256 * 1024).map_err(|_| Error("agent_delivery_uncertain"))?;
    require(
        reply.as_object().is_some_and(|o| o.len() == 4)
            && reply.get("id").and_then(Value::as_str) == Some(&id)
            && reply.get("contract").and_then(Value::as_str) == Some(CONTRACT),
        "agent_delivery_uncertain",
    )?;
    if reply.get("type").and_then(Value::as_str) == Some("error") {
        let code = reply
            .get("error")
            .and_then(Value::as_object)
            .filter(|error| error.len() == 1)
            .and_then(|error| error.get("code"))
            .and_then(Value::as_str);
        require(
            code.is_some_and(|code| {
                !code.is_empty()
                    && code.len() <= 64
                    && code.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
                    })
            }),
            "agent_delivery_uncertain",
        )?;
        // Never surface peer error bodies, including arbitrary code strings.
        return Err(Error("agent_report_rejected"));
    }
    require(
        reply.get("type").and_then(Value::as_str) == Some("result"),
        "agent_delivery_uncertain",
    )?;
    let result = reply
        .get("result")
        .ok_or(Error("agent_delivery_uncertain"))?;
    validate_result(method, result).map_err(|_| Error("agent_delivery_uncertain"))?;
    crate::observation_socket::check(deadline, &NoCancel)
        .map_err(|_| Error("agent_delivery_uncertain"))?;
    Ok(result.clone())
}

fn validate_result(method: &str, value: &Value) -> Result<()> {
    // Reject extra peer fields before any result can reach stdout.
    if method == "report" {
        require(
            value.as_object().is_some_and(|o| o.len() == 3)
                && value.get("type") == Some(&json!("accepted"))
                && text(value, "record_id")?.len() == 64
                && text(value, "record_id")?
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())
                && matches!(
                    text(value, "delivery")?,
                    "committed" | "durability_uncertain"
                ),
            "agent_delivery_uncertain",
        )?;
    } else {
        require(
            value.as_object().is_some_and(|o| o.len() == 4)
                && value.get("contract") == Some(&json!(CONTRACT))
                && value.get("authorization") == Some(&json!("none")),
            "agent_delivery_uncertain",
        )?;
        let epoch = text(value, "server_epoch")?;
        require(
            epoch.len() == 32 && epoch.bytes().all(|b| b.is_ascii_hexdigit()),
            "agent_delivery_uncertain",
        )?;
        let records = value
            .get("records")
            .and_then(Value::as_array)
            .ok_or(Error("agent_delivery_uncertain"))?;
        require(records.len() <= MAX_RECORDS, "agent_delivery_uncertain")?;
        for record in records {
            require(
                record.as_object().is_some_and(|o| o.len() == 8)
                    && text(record, "record_id")?.len() == 64
                    && text(record, "record_id")?
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit())
                    && event(text(record, "provider")?, text(record, "last_event")?).is_ok()
                    && (!matches!(
                        text(record, "provider")?,
                        "codex" | "copilot" | "pi" | "opencode"
                    ) || text(record, "reported_state")? == "unknown")
                    && matches!(
                        text(record, "reported_state")?,
                        "unknown" | "working" | "idle" | "ended"
                    )
                    && record
                        .get("age_ms")
                        .is_some_and(|v| v.is_null() || v.as_u64().is_some())
                    && text(record, "reference_trust")? == "caller_supplied_unverified"
                    && text(record, "native_identity")? == "unestablished"
                    && text(record, "job_completion")? == "unestablished",
                "agent_delivery_uncertain",
            )?;
        }
    }
    Ok(())
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::{cell::RefCell, fs, os::unix::fs::PermissionsExt};

    type Checkpoint = Box<dyn FnMut(&str)>;

    thread_local! {
        static CHECKPOINT: RefCell<Option<Checkpoint>> = RefCell::new(None);
    }

    pub(super) fn checkpoint(stage: &str) {
        CHECKPOINT.with_borrow_mut(|hook| {
            if let Some(hook) = hook {
                hook(stage);
            }
        });
    }

    struct Hook;

    impl Hook {
        fn new(hook: impl FnMut(&str) + 'static) -> Self {
            CHECKPOINT.with_borrow_mut(|slot| {
                assert!(slot.is_none());
                *slot = Some(Box::new(hook));
            });
            Self
        }
    }

    impl Drop for Hook {
        fn drop(&mut self) {
            CHECKPOINT.with_borrow_mut(|slot| *slot = None);
        }
    }

    fn assert_no_connection(listener: &UnixListener) {
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    fn private_root() -> tempfile::TempDir {
        let root = crate::private_access::test_support::tempdir();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        root
    }

    #[test]
    fn admitted_bind_is_private_and_owned_cleanup_is_descriptor_relative() {
        use std::os::unix::fs::MetadataExt;
        let root = private_root();
        let path = root.path().join("receiver.sock");
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        assert_eq!(fs::symlink_metadata(&path).unwrap().mode() & 0o7777, 0o600);
        bound.fence().unwrap();
        drop(bound);
        assert!(!path.exists());
    }

    fn assert_chmod_startup_failure(errno: i32) {
        use std::cell::Cell;
        let root = private_root();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let marker = workspace.join("coven.json");
        fs::write(&marker, b"unchanged-marker").unwrap();
        let path = root.path().join("receiver.sock");
        let called = Cell::new(false);
        let error = Bound::bind_inner_with_chmod(path.to_str().unwrap(), |node| {
            called.set(true);
            let stat = fs::symlink_metadata(&path).unwrap();
            use std::os::unix::fs::{FileTypeExt, MetadataExt};
            assert!(stat.file_type().is_socket());
            assert_eq!((stat.dev(), stat.ino()), node.inode);
            assert_eq!(node.selected.path, path);
            node.fence_mode(stat.mode() & 0o7777).unwrap();
            Err(io::Error::from_raw_os_error(errno))
        })
        .and_then(|bound| {
            // The same startup ordering as serve: no store may open after chmod refusal.
            let _store = store::Store::open(&workspace)?;
            Ok(bound)
        })
        .err()
        .unwrap();
        assert!(called.get());
        assert_eq!(error.code(), "agent_socket_chmod_failed");
        assert!(fs::symlink_metadata(&path).is_err_and(|e| e.kind() == io::ErrorKind::NotFound));
        assert_eq!(fs::read(&marker).unwrap(), b"unchanged-marker");
        assert_eq!(fs::read_dir(&workspace).unwrap().count(), 1);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn chmod_enosys_refuses_startup_and_cleans_owned_socket_before_store() {
        assert_chmod_startup_failure(libc::ENOSYS);
    }

    #[test]
    fn chmod_eperm_refuses_startup_and_cleans_owned_socket_before_store() {
        assert_chmod_startup_failure(libc::EPERM);
    }

    #[test]
    fn foreign_leaf_refuses_bind_without_invoking_chmod_callback() {
        use std::{cell::Cell, os::unix::fs::MetadataExt};
        let root = private_root();
        let path = root.path().join("receiver.sock");
        fs::write(&path, b"FOREIGN").unwrap();
        let before = fs::symlink_metadata(&path).unwrap();
        let called = Cell::new(false);
        let error = Bound::bind_inner_with_chmod(path.to_str().unwrap(), |_| {
            called.set(true);
            Err(io::Error::from_raw_os_error(libc::EPERM))
        })
        .err()
        .unwrap();
        assert_eq!(error.code(), "agent_socket_bind_failed");
        assert!(!called.get());
        let after = fs::symlink_metadata(&path).unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        assert_eq!(before.mode(), after.mode());
        assert_eq!(fs::read(&path).unwrap(), b"FOREIGN");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn existing_socket_is_not_unlinked_or_rebound() {
        let root = private_root();
        let path = root.path().join("receiver.sock");
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        assert!(Bound::bind(path.to_str().unwrap()).is_err());
        bound.fence().unwrap();
    }

    #[test]
    fn stale_socket_requires_manual_removal() {
        let root = private_root();
        let path = root.path().join("receiver.sock");
        drop(UnixListener::bind(&path).unwrap());
        assert!(Bound::bind(path.to_str().unwrap()).is_err());
        assert!(path.exists());
    }

    #[test]
    fn prechmod_replacement_symlink_is_not_followed_or_cleaned() {
        use std::os::unix::fs::MetadataExt;
        let root = private_root();
        let path = root.path().join("receiver.sock");
        let target = root.path().join("target");
        fs::write(&target, b"foreign").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        let replaced = path.clone();
        let destination = target.clone();
        let _hook = Hook::new(move |stage| {
            if stage == "post_bind" {
                fs::remove_file(&replaced).unwrap();
                std::os::unix::fs::symlink(&destination, &replaced).unwrap();
            }
        });
        assert!(Bound::bind(path.to_str().unwrap()).is_err());
        assert!(fs::symlink_metadata(&path).unwrap().is_symlink());
        assert_eq!(fs::metadata(&target).unwrap().mode() & 0o7777, 0o644);
    }

    #[test]
    fn initial_denials_never_construct_a_client_socket() {
        use crate::platform::observation_socket::SocketConstructionGuard;
        let root = private_root();
        let path = root.path().join("receiver.sock");
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        let guard = SocketConstructionGuard::new(true);
        for mode in [0o000, 0o400, 0o660, 0o666, 0o1600] {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            assert!(call(path.to_str().unwrap(), "inventory", json!({})).is_err());
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        for mode in [0o600, 0o711, 0o750, 0o770, 0o1700] {
            fs::set_permissions(root.path(), fs::Permissions::from_mode(mode)).unwrap();
            assert!(call(path.to_str().unwrap(), "inventory", json!({})).is_err());
        }
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(guard.count(), 0);
        assert_no_connection(&bound.listener);
    }

    #[test]
    fn linked_or_symlinked_socket_and_missing_leaf_never_connect() {
        use crate::platform::observation_socket::SocketConstructionGuard;
        let root = private_root();
        let path = root.path().join("receiver.sock");
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        let guard = SocketConstructionGuard::new(true);
        let link = root.path().join("linked.sock");
        fs::hard_link(&path, &link).unwrap();
        assert!(call(path.to_str().unwrap(), "inventory", json!({})).is_err());
        assert!(call(link.to_str().unwrap(), "inventory", json!({})).is_err());
        fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(call(link.to_str().unwrap(), "inventory", json!({})).is_err());
        assert!(
            call(
                root.path().join("missing.sock").to_str().unwrap(),
                "inventory",
                json!({})
            )
            .is_err()
        );
        assert_eq!(guard.count(), 0);
        assert_no_connection(&bound.listener);
    }

    #[test]
    fn owner_mismatch_never_connects() {
        // Only a privileged fixture can change ownership away from the effective UID.
        if unsafe { libc::geteuid() } != 0 {
            return;
        }
        use crate::platform::observation_socket::SocketConstructionGuard;
        let root = private_root();
        let path = root.path().join("receiver.sock");
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        let name = CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::lchown(name.as_ptr(), 1, !0) }, 0);
        let guard = SocketConstructionGuard::new(true);
        assert!(call(path.to_str().unwrap(), "inventory", json!({})).is_err());
        assert_eq!(guard.count(), 0);
        assert_no_connection(&bound.listener);
    }

    #[test]
    fn preconnect_parent_change_never_connects() {
        use crate::platform::observation_socket::SocketConstructionGuard;
        let root = private_root();
        let path = root.path().join("receiver.sock");
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        let parent = root.path().to_path_buf();
        let _hook = Hook::new(move |stage| {
            if stage == "pre_connect" {
                fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
            }
        });
        let guard = SocketConstructionGuard::new(true);
        assert_ne!(
            call(path.to_str().unwrap(), "inventory", json!({}))
                .unwrap_err()
                .code(),
            "agent_delivery_uncertain"
        );
        assert_eq!(guard.count(), 0);
        assert_no_connection(&bound.listener);
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn preconnect_missing_leaf_race_never_connects() {
        use crate::platform::observation_socket::SocketConstructionGuard;
        let root = private_root();
        let path = root.path().join("receiver.sock");
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        let removed = path.clone();
        let _hook = Hook::new(move |stage| {
            if stage == "pre_connect" {
                fs::remove_file(&removed).unwrap();
            }
        });
        let guard = SocketConstructionGuard::new(true);
        assert!(call(path.to_str().unwrap(), "inventory", json!({})).is_err());
        assert_eq!(guard.count(), 0);
        assert_no_connection(&bound.listener);
    }

    #[test]
    fn preconnect_parent_replacement_never_connects_to_new_private_socket() {
        use crate::platform::observation_socket::SocketConstructionGuard;
        let root = private_root();
        let parent = root.path().join("private");
        fs::create_dir(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        let path = parent.join("receiver.sock");
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        let moved = root.path().join("moved");
        let changed = parent.clone();
        let new_path = path.clone();
        let _hook = Hook::new(move |stage| {
            if stage == "pre_connect" {
                fs::rename(&changed, &moved).unwrap();
                fs::create_dir(&changed).unwrap();
                fs::set_permissions(&changed, fs::Permissions::from_mode(0o700)).unwrap();
                drop(UnixListener::bind(&new_path).unwrap());
                fs::set_permissions(&new_path, fs::Permissions::from_mode(0o600)).unwrap();
            }
        });
        let guard = SocketConstructionGuard::new(true);
        assert!(call(path.to_str().unwrap(), "inventory", json!({})).is_err());
        assert_eq!(guard.count(), 0);
        assert_no_connection(&bound.listener);
        drop(bound);
        assert!(path.exists());
        assert!(root.path().join("moved/receiver.sock").exists());
    }

    #[test]
    fn presend_denial_never_sends_private_session_id() {
        let root = private_root();
        let path = root.path().join("receiver.sock");
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        let changed = path.clone();
        let _hook = Hook::new(move |stage| {
            if stage == "pre_send" {
                fs::set_permissions(&changed, fs::Permissions::from_mode(0o666)).unwrap();
            }
        });
        assert_ne!(
            call(
                path.to_str().unwrap(),
                "report",
                json!({"session_id":"PRIVATE-SID"})
            )
            .unwrap_err()
            .code(),
            "agent_delivery_uncertain"
        );
        let (mut stream, _) = bound.listener.accept().unwrap();
        let mut byte = [0];
        assert_eq!(stream.read(&mut byte).unwrap(), 0);
        assert_no_connection(&bound.listener);
    }

    #[test]
    fn postsend_privacy_failure_is_uncertain_and_never_replayed() {
        let root = private_root();
        let path = root.path().join("receiver.sock");
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        let parent = root.path().to_path_buf();
        let _hook = Hook::new(move |stage| {
            if stage == "post_send" {
                fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
            }
        });
        assert_eq!(
            call(
                path.to_str().unwrap(),
                "report",
                json!({"session_id":"PRIVATE-SID"})
            )
            .unwrap_err()
            .code(),
            "agent_delivery_uncertain"
        );
        let (mut stream, _) = bound.listener.accept().unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.iter().filter(|&&b| b == b'\n').count(), 1);
        assert!(String::from_utf8(bytes).unwrap().contains("PRIVATE-SID"));
        assert_no_connection(&bound.listener);
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn parent_replacement_and_alias_prevent_fences_and_cleanup() {
        let root = private_root();
        let parent = root.path().join("private");
        fs::create_dir(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        let path = parent.join("receiver.sock");
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        let moved = root.path().join("moved");
        fs::rename(&parent, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &parent).unwrap();
        assert!(bound.fence().is_err());
        assert!(selected(path.to_str().unwrap()).is_err());
        drop(bound);
        assert!(moved.join("receiver.sock").exists());
    }

    #[test]
    fn changed_parent_privacy_prevents_cleanup() {
        let root = private_root();
        let path = root.path().join("receiver.sock");
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(bound.fence().is_err());
        drop(bound);
        assert!(path.exists());
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn replacement_socket_is_preserved() {
        let root = private_root();
        let path = root.path().join("receiver.sock");
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        fs::remove_file(&path).unwrap();
        let replacement = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(bound.fence().is_err());
        drop(bound);
        assert!(path.exists());
        drop(replacement);
    }

    #[test]
    fn umask_zero_is_isolated_to_child_process() {
        const CHILD: &str = "COVEN_SOCKET_UMASK_CHILD";
        if std::env::var_os(CHILD).is_some() {
            unsafe {
                libc::umask(0);
            }
            let root = private_root();
            let path = root.path().join("receiver.sock");
            let _hook = Hook::new(move |stage| {
                if stage == "post_bind" {
                    use std::os::unix::fs::MetadataExt;
                    assert_eq!(fs::symlink_metadata(&path).unwrap().mode() & 0o7777, 0o777);
                    assert_eq!(
                        fs::metadata(path.parent().unwrap()).unwrap().mode() & 0o7777,
                        0o700
                    );
                }
            });
            let bound = Bound::bind(root.path().join("receiver.sock").to_str().unwrap()).unwrap();
            bound.fence().unwrap();
            return;
        }
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "agent_management::unix::tests::umask_zero_is_isolated_to_child_process",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[cfg(target_os = "macos")]
    fn add_socket_acl(path: &std::path::Path) {
        assert!(
            std::process::Command::new("/bin/chmod")
                .args(["+a", "everyone allow read,write"])
                .arg(path)
                .status()
                .unwrap()
                .success()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn existing_socket_explicit_acl_denied_under_acl_free_parent() {
        use crate::platform::observation_socket::SocketConstructionGuard;
        let root = private_root();
        let path = root.path().join("receiver.sock");
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        add_socket_acl(&path);
        let guard = SocketConstructionGuard::new(true);
        assert!(call(path.to_str().unwrap(), "inventory", json!({})).is_err());
        assert_eq!(guard.count(), 0);
        assert_no_connection(&bound.listener);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn new_socket_explicit_acl_denied_before_processing() {
        let root = private_root();
        let path = root.path().join("receiver.sock");
        let changed = path.clone();
        let _hook = Hook::new(move |stage| {
            if stage == "post_bind" {
                add_socket_acl(&changed);
            }
        });
        assert!(Bound::bind(path.to_str().unwrap()).is_err());
    }

    #[test]
    fn replacement_leaf_is_never_unlinked_and_parent_alias_is_refused() {
        let root = private_root();
        let path = root.path().join("receiver.sock");
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Bound::bind(path.to_str().unwrap()).is_err());
        assert!(!path.exists());
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let bound = Bound::bind(path.to_str().unwrap()).unwrap();
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"foreign-leaf").unwrap();
        assert!(bound.fence().is_err());
        drop(bound);
        assert_eq!(fs::read(&path).unwrap(), b"foreign-leaf");
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(root.path(), &alias).unwrap();
        assert!(Bound::bind(alias.join("other.sock").to_str().unwrap()).is_err());
    }

    #[test]
    fn private_rpc_correlation_closed_schema_and_no_mutation_on_refusal() {
        let root = private_root();
        fs::write(root.path().join("coven.json"), b"marker").unwrap();
        let mut store = store::Store::open(root.path()).unwrap();
        let id = "a".repeat(32);
        let request = json!({"id":id,"contract":CONTRACT,"method":"report","params":{"provider":"claude","event":"Stop","session_id":"native-ref"}});
        let reply = handle(&mut store, &serde_json::to_vec(&request).unwrap()).unwrap();
        let reply: Value = serde_json::from_slice(&reply).unwrap();
        assert_eq!(reply["id"], id);
        assert_eq!(reply["contract"], CONTRACT);
        assert_eq!(reply["type"], "result");
        assert_eq!(reply["result"]["type"], "accepted");
        let saved = fs::read(root.path().join("state/agent-management.json")).unwrap();
        for (field, value) in [
            (
                "params",
                json!({"provider":"codex","event":"Stop","session_id":"s"}),
            ),
            (
                "params",
                json!({"provider":"codex","event":"SessionStart","session_id":"s","reported_state":"working"}),
            ),
            ("params", json!({"provider":"codex","session_id":"s"})),
            (
                "params",
                json!({"provider":"unknown","event":"SessionStart","session_id":"s"}),
            ),
            ("contract", json!("wrong")),
            ("method", json!("session.snapshot")),
            (
                "params",
                json!({"provider":"claude","event":"Stop","session_id":"s","prompt":"SECRET"}),
            ),
        ] {
            let mut bad = request.clone();
            bad[field] = value;
            let reply = handle(&mut store, &serde_json::to_vec(&bad).unwrap()).unwrap();
            let reply: Value = serde_json::from_slice(&reply).unwrap();
            assert_eq!(reply["id"], id);
            assert_eq!(reply["type"], "error");
            assert!(!reply.to_string().contains("SECRET"));
            assert_eq!(
                saved,
                fs::read(root.path().join("state/agent-management.json")).unwrap()
            );
        }
        let duplicate = format!(
            r#"{{"id":"{id}","id":"{id}","contract":"{CONTRACT}","method":"inventory","params":{{}}}}"#
        );
        assert!(handle(&mut store, duplicate.as_bytes()).is_err());
        assert_eq!(
            saved,
            fs::read(root.path().join("state/agent-management.json")).unwrap()
        );
    }

    #[test]
    fn inventory_validates_codex_event_and_unknown_state_only() {
        let mut value = json!({"contract":CONTRACT,"server_epoch":"a".repeat(32),"authorization":"none","records":[{
            "record_id":"b".repeat(64),"provider":"codex","last_event":"SessionStart","reported_state":"unknown","age_ms":0,
            "reference_trust":"caller_supplied_unverified","native_identity":"unestablished","job_completion":"unestablished"}]});
        assert!(validate_result("inventory", &value).is_ok());
        for state in ["working", "idle", "ended"] {
            value["records"][0]["reported_state"] = json!(state);
            assert!(validate_result("inventory", &value).is_err());
        }
        value["records"][0]["reported_state"] = json!("unknown");
        value["records"][0]["last_event"] = json!("Stop");
        assert!(validate_result("inventory", &value).is_err());
        value["records"][0]["last_event"] = json!("SessionStart");
        value["records"][0]["provider"] = json!("unknown");
        assert!(validate_result("inventory", &value).is_err());
    }

    #[test]
    fn inventory_validates_copilot_closed_schema_and_unknown_state_only() {
        let value = json!({"contract":CONTRACT,"server_epoch":"a".repeat(32),"authorization":"none","records":[{
            "record_id":"b".repeat(64),"provider":"copilot","last_event":"SessionStart","reported_state":"unknown","age_ms":0,
            "reference_trust":"caller_supplied_unverified","native_identity":"unestablished","job_completion":"unestablished"}]});
        assert!(validate_result("inventory", &value).is_ok());
        for (field, invalid) in [
            ("reported_state", json!("working")),
            ("reported_state", json!("idle")),
            ("reported_state", json!("ended")),
            ("last_event", json!("sessionStart")),
            ("last_event", json!("SubagentStop")),
            ("last_event", json!("Stop")),
            ("session_id", json!("SECRET")),
            ("cwd", json!("SECRET")),
        ] {
            let mut bad = value.clone();
            bad["records"][0][field] = invalid;
            assert!(validate_result("inventory", &bad).is_err());
        }
    }

    #[test]
    fn inventory_validates_extension_events_and_unknown_state_only() {
        for (provider, event, other_event) in [
            ("pi", "session_start", "session.created"),
            ("opencode", "session.created", "session_start"),
            ("opencode", "session.updated", "session_start"),
        ] {
            let value = json!({"contract":CONTRACT,"server_epoch":"a".repeat(32),"authorization":"none","records":[{
            "record_id":"b".repeat(64),"provider":provider,"last_event":event,"reported_state":"unknown","age_ms":0,
            "reference_trust":"caller_supplied_unverified","native_identity":"unestablished","job_completion":"unestablished"}]});
            assert!(validate_result("inventory", &value).is_ok());
            for (field, invalid) in [
                ("reported_state", json!("working")),
                ("reported_state", json!("idle")),
                ("reported_state", json!("ended")),
                ("last_event", json!("SessionStart")),
                ("last_event", json!("Stop")),
                ("last_event", json!("SubagentStop")),
                ("last_event", json!("other")),
                ("last_event", json!(other_event)),
                ("origin", json!("SECRET")),
            ] {
                let mut bad = value.clone();
                bad["records"][0][field] = invalid;
                assert!(validate_result("inventory", &bad).is_err());
            }
        }
    }
}
