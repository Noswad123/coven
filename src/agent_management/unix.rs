use super::*;
use crate::observation_socket::{NoCancel, ReadBudget};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::Path,
    thread,
    time::{Duration, Instant},
};

fn identity(meta: &fs::Metadata) -> (u64, u64) {
    (meta.dev(), meta.ino())
}
fn owner(meta: &fs::Metadata) -> bool {
    meta.uid() == unsafe { libc::geteuid() }
}

pub(super) fn private_directory(path: &Path) -> Result<(u64, u64)> {
    let meta = fs::symlink_metadata(path)?;
    require(
        meta.is_dir() && owner(&meta) && meta.mode() & 0o022 == 0,
        "agent_unsafe_parent",
    )?;
    Ok(identity(&meta))
}

pub(super) fn private_file(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    require(
        meta.is_file() && owner(&meta) && meta.nlink() == 1 && meta.mode() & 0o7777 == 0o600,
        "agent_unsafe_store",
    )
}

pub(super) struct ReceiverLease {
    file: fs::File,
    path: PathBuf,
    inode: (u64, u64),
}

impl ReceiverLease {
    pub(super) fn acquire(root: &Path) -> Result<Self> {
        let path = crate::read_fs::managed(
            root,
            Path::new("state/.agent-management.receiver.lock"),
            false,
        )?;
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)?;
        let metadata = file.metadata()?;
        require(
            metadata.is_file()
                && owner(&metadata)
                && metadata.nlink() == 1
                && metadata.mode() & 0o7777 == 0o600,
            "agent_unsafe_receiver_lease",
        )?;
        require(
            crate::platform::try_workspace_lock(&file)?,
            "agent_receiver_busy",
        )?;
        let lease = Self {
            file,
            path,
            inode: identity(&metadata),
        };
        lease.fence()?;
        Ok(lease)
    }

    pub(super) fn fence(&self) -> Result<()> {
        for metadata in [self.file.metadata()?, fs::symlink_metadata(&self.path)?] {
            require(
                metadata.is_file()
                    && owner(&metadata)
                    && metadata.nlink() == 1
                    && metadata.mode() & 0o7777 == 0o600
                    && identity(&metadata) == self.inode,
                "agent_receiver_lease_replaced",
            )?;
        }
        Ok(())
    }
    // Closing our descriptor releases flock. The persistent lease inode is never removed.
}

fn socket_parent(path: &Path) -> Result<(u64, u64)> {
    let inode = private_directory(path)?;
    require(
        fs::symlink_metadata(path)?.mode() & 0o7777 == 0o700,
        "agent_unsafe_parent",
    )?;
    Ok(inode)
}

pub(super) fn workspace_paths(
    root: &Path,
    allow_missing: bool,
) -> Result<((u64, u64), (u64, u64))> {
    let state = crate::read_fs::managed(root, Path::new("state"), true)?;
    if allow_missing && !state.exists() {
        return Ok(((0, 0), (0, 0)));
    }
    let directory = private_directory(&state)?;
    let lock = crate::read_fs::managed(root, Path::new("state/.workspace.lock"), false)?;
    let metadata = match fs::symlink_metadata(lock) {
        Err(e) if allow_missing && e.kind() == std::io::ErrorKind::NotFound => {
            return Ok((directory, (0, 0)));
        }
        result => result?,
    };
    require(
        metadata.is_file()
            && owner(&metadata)
            && metadata.nlink() == 1
            && metadata.mode() & 0o022 == 0,
        "agent_unsafe_workspace_lock",
    )?;
    Ok((directory, identity(&metadata)))
}

fn selected(path: &str) -> Result<PathBuf> {
    crate::observation_socket::validate_path(path)?;
    let selected = PathBuf::from(path);
    require(
        selected.file_name().is_some() && !path.ends_with("/.") && !path.contains("//"),
        "agent_invalid_socket",
    )?;
    let parent = selected.parent().ok_or(Error("agent_invalid_socket"))?;
    require(parent.canonicalize()? == parent, "agent_unsafe_parent")?;
    socket_parent(parent)?;
    Ok(selected)
}

struct Bound {
    listener: UnixListener,
    path: PathBuf,
    inode: (u64, u64),
    parent: (u64, u64),
}

impl Bound {
    fn bind(path: &str) -> Result<Self> {
        let path = selected(path)?;
        let parent = socket_parent(path.parent().unwrap())?;
        // bind itself is exclusive. Never remove an existing leaf, including stale sockets.
        let listener = UnixListener::bind(&path).map_err(|_| Error("agent_socket_bind_failed"))?;
        let inode = identity(&fs::symlink_metadata(&path)?);
        let bound = Self {
            listener,
            path,
            inode,
            parent,
        };
        fs::set_permissions(&bound.path, fs::Permissions::from_mode(0o600))?;
        bound.listener.set_nonblocking(true)?;
        bound.fence()?;
        Ok(bound)
    }

    fn fence(&self) -> Result<()> {
        require(
            socket_parent(self.path.parent().unwrap())? == self.parent,
            "agent_endpoint_replaced",
        )?;
        let meta = fs::symlink_metadata(&self.path)?;
        require(
            meta.file_type().is_socket()
                && owner(&meta)
                && meta.mode() & 0o7777 == 0o600
                && identity(&meta) == self.inode,
            "agent_endpoint_replaced",
        )
    }
}

impl Drop for Bound {
    fn drop(&mut self) {
        if private_directory(self.path.parent().unwrap()).ok() == Some(self.parent)
            && fs::symlink_metadata(&self.path)
                .is_ok_and(|m| m.file_type().is_socket() && identity(&m) == self.inode)
        {
            let _ = fs::remove_file(&self.path);
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
                        handle(&mut store, &bytes)
                    });
                if let Ok(reply) = reply {
                    publish(&projection, &store);
                    let _ = send(&mut stream, &reply, connection_deadline, &interrupt);
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
    selected(path)?;
    let meta = fs::symlink_metadata(path)?;
    require(
        meta.file_type().is_socket() && owner(&meta) && meta.mode() & 0o7777 == 0o600,
        "agent_unsafe_socket",
    )?;
    let endpoint =
        crate::observation_socket::select(path).map_err(|_| Error("agent_unsafe_socket"))?;
    let deadline = Instant::now() + Duration::from_secs(1);
    let id = crate::platform::random_uuid_hex()?;
    let mut frame =
        serde_json::to_vec(&json!({"id":id,"contract":CONTRACT,"method":method,"params":params}))?;
    frame.push(b'\n');
    let socket =
        crate::platform::observation_socket::Socket::connect(&endpoint, deadline, &NoCancel)
            .map_err(|_| Error("agent_connect_failed"))?;
    crate::platform::observation_socket::fence(&endpoint)
        .map_err(|_| Error("agent_endpoint_replaced"))?;
    // From the first send onward failures are uncertain, never replayed.
    socket
        .send(&frame, deadline, &NoCancel)
        .map_err(|_| Error("agent_delivery_uncertain"))?;
    let payload = socket
        .receive(deadline, &mut ReadBudget::new(256 * 1024), &NoCancel)
        .map_err(|_| Error("agent_delivery_uncertain"))?;
    crate::platform::observation_socket::fence(&endpoint)
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
                    && (text(record, "provider")? != "codex"
                        || text(record, "reported_state")? == "unknown")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_leaf_is_never_unlinked_and_parent_alias_is_refused() {
        let root = tempfile::tempdir().unwrap();
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
        let root = tempfile::tempdir().unwrap();
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
}
