//! Bounded, single-attempt observation transport. Endpoint fences are not authentication.
use serde_json::{Value, json};
use std::time::Instant;

pub(crate) use crate::platform::observation_socket::{InterruptGuard, PublisherLease};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Delivery {
    Unsent,
    Rejected,
    Uncertain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Failure {
    pub code: String,
    pub delivery: Delivery,
    pub interrupted: bool,
}

pub(crate) type WireResult<T> = std::result::Result<T, Failure>;

impl Failure {
    pub(crate) fn new(code: &str) -> Self {
        Self {
            code: code.into(),
            delivery: Delivery::Unsent,
            interrupted: code == "interrupted",
        }
    }
}

pub(crate) trait Cancel {
    fn interrupted(&self) -> bool;
}

impl Cancel for InterruptGuard {
    fn interrupted(&self) -> bool {
        InterruptGuard::interrupted(self)
    }
}

pub(crate) struct NoCancel;

impl Cancel for NoCancel {
    fn interrupted(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ReadMethod {
    Snapshot,
    ProcessInfo,
    PaneRead,
}

#[derive(Debug)]
pub(crate) struct ReadBudget {
    remaining: usize,
}

impl ReadBudget {
    pub(crate) fn new(remaining: usize) -> Self {
        Self { remaining }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.remaining
    }

    pub(crate) fn charge(&mut self, bytes: usize) -> WireResult<()> {
        if bytes > self.remaining {
            self.remaining = 0;
            return Err(Failure::new("response_limit"));
        }
        self.remaining -= bytes;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Endpoint {
    pub selected: String,
    pub resolved: String,
    pub device: u64,
    pub inode: u64,
}

impl Endpoint {
    pub(crate) fn identity(&self) -> crate::Result<Value> {
        Ok(json!({"path": self.resolved, "device": self.device, "inode": self.inode}))
    }
}

#[derive(Debug)]
pub(crate) struct Reply {
    pub payload: Vec<u8>,
    pub request_id: String,
    pub endpoint: Endpoint,
}

pub(crate) fn validate_path(path: &str) -> crate::Result<()> {
    crate::error::require(
        path.starts_with('/')
            && !path.ends_with('/')
            && !path.contains('\0')
            && !path.split('/').any(|part| part == "..")
            && path.len() <= 4096,
        "invalid_socket_path",
    )
}

pub(crate) fn resolve(path: &str) -> WireResult<String> {
    validate_path(path).map_err(|error| Failure::new(error.code()))?;
    crate::platform::observation_socket::resolve(path)
}

pub(crate) fn select(path: &str) -> WireResult<Endpoint> {
    validate_path(path).map_err(|error| Failure::new(error.code()))?;
    crate::platform::observation_socket::select(path)
}

pub(crate) fn check(deadline: Instant, cancel: &dyn Cancel) -> WireResult<()> {
    if cancel.interrupted() {
        return Err(Failure::new("interrupted"));
    }
    if Instant::now() >= deadline {
        return Err(Failure::new("timeout"));
    }
    Ok(())
}

fn frame(id: &str, method: &str, params: &Value) -> WireResult<Vec<u8>> {
    let mut bytes = serde_json::to_vec(&json!({"id": id, "method": method, "params": params}))
        .map_err(|_| Failure::new("schema_error"))?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(crate) fn read(
    path: &str,
    method: ReadMethod,
    params: &Value,
    deadline: Instant,
    budget: &mut ReadBudget,
    cancel: &dyn Cancel,
) -> WireResult<Reply> {
    read_method(
        path,
        match method {
            ReadMethod::Snapshot => "session.snapshot",
            ReadMethod::ProcessInfo => "pane.process_info",
            ReadMethod::PaneRead => "pane.read",
        },
        params,
        deadline,
        budget,
        cancel,
    )
}

/// Private, fixed-purpose subscription transport, not a general RPC surface.
pub(crate) struct EventSocket {
    socket: crate::platform::observation_socket::Socket,
    endpoint: Endpoint,
    pending: Vec<u8>,
    partial_since: Option<Instant>,
    fenced: Instant,
}

impl EventSocket {
    pub(crate) const MAX_FRAME_BYTES: usize = 256 * 1024;

    pub(crate) fn subscribe(
        path: &str,
        deadline: Instant,
        cancel: &dyn Cancel,
    ) -> WireResult<(Self, String)> {
        check(deadline, cancel)?;
        let endpoint = select(path)?;
        let id = crate::platform::random_uuid_hex().map_err(|_| Failure::new("io_error"))?;
        let subscriptions: Vec<_> = [
            "pane.created",
            "pane.closed",
            "pane.updated",
            "pane.moved",
            "pane.exited",
            "tab.created",
            "tab.closed",
            "tab.moved",
            "workspace.created",
            "workspace.closed",
            "workspace.moved",
            "layout.updated",
        ]
        .into_iter()
        .map(|name| json!({"type":name}))
        .collect();
        let request = frame(
            &id,
            "events.subscribe",
            &json!({"subscriptions": subscriptions}),
        )?;
        let socket =
            crate::platform::observation_socket::Socket::connect(&endpoint, deadline, cancel)?;
        crate::platform::observation_socket::fence(&endpoint)?;
        socket.send(&request, deadline, cancel)?;
        crate::platform::observation_socket::fence(&endpoint)?;
        Ok((
            Self {
                socket,
                endpoint,
                pending: Vec::new(),
                partial_since: None,
                fenced: Instant::now(),
            },
            id,
        ))
    }

    pub(crate) fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    pub(crate) fn fence(&mut self) -> WireResult<()> {
        crate::platform::observation_socket::fence(&self.endpoint)?;
        self.fenced = Instant::now();
        Ok(())
    }

    /// None is an idle deadline, never EOF. Partial assembly has its own clock.
    pub(crate) fn next_frame(
        &mut self,
        until: Instant,
        cancel: &dyn Cancel,
    ) -> WireResult<Option<Vec<u8>>> {
        use std::time::Duration;
        loop {
            if cancel.interrupted() {
                return Err(Failure::new("interrupted"));
            }
            let now = Instant::now();
            if now.duration_since(self.fenced) >= Duration::from_secs(1) {
                self.fence()?;
            }
            if now >= until {
                if !self.pending.contains(&b'\n')
                    && self
                        .partial_since
                        .is_some_and(|start| now.duration_since(start) >= Duration::from_secs(5))
                {
                    return Err(Failure::new("partial_frame_timeout"));
                }
                return Ok(None);
            }
            if let Some(end) = self.pending.iter().position(|&b| b == b'\n') {
                if end >= Self::MAX_FRAME_BYTES {
                    return Err(Failure::new("response_limit"));
                }
                self.fence()?;
                let rest = self.pending.split_off(end + 1);
                let mut frame = std::mem::replace(&mut self.pending, rest);
                frame.pop();
                if self.pending.is_empty() {
                    self.partial_since = None;
                }
                return Ok(Some(frame));
            }
            if self.pending.len() >= Self::MAX_FRAME_BYTES {
                return Err(Failure::new("response_limit"));
            }
            if self
                .partial_since
                .is_some_and(|start| now.duration_since(start) >= Duration::from_secs(5))
            {
                return Err(Failure::new("partial_frame_timeout"));
            }
            let mut wake = until.min(self.fenced + Duration::from_secs(1));
            if let Some(start) = self.partial_since {
                wake = wake.min(start + Duration::from_secs(5));
            }
            let mut buffer = [0u8; 16384];
            match self.socket.stream_read(&mut buffer, wake, cancel) {
                Ok(Some(0)) => {
                    self.fence()?;
                    return Err(Failure::new(if self.pending.is_empty() {
                        "stream_eof"
                    } else {
                        "incomplete_response"
                    }));
                }
                Ok(Some(count)) => {
                    // A newline ends the old assembly; any new tail begins in this read.
                    if buffer[..count].contains(&b'\n') {
                        self.partial_since = Some(Instant::now());
                    } else {
                        self.partial_since.get_or_insert_with(Instant::now);
                    }
                    self.pending.extend_from_slice(&buffer[..count]);
                }
                Ok(None) => {}
                Err(error) if error.code == "timeout" => {}
                Err(error) => return Err(error),
            }
        }
    }
}

/// Scoped naming read, not an extension of the public observer method set.
pub(crate) fn naming_inspect(
    endpoint: &Endpoint,
    terminal: &str,
    deadline: Instant,
    budget: &mut ReadBudget,
    cancel: &dyn Cancel,
) -> WireResult<Reply> {
    read_endpoint(
        endpoint,
        "terminal.inspect",
        &json!({"terminal_id":terminal}),
        deadline,
        budget,
        cancel,
    )
}

pub(crate) fn naming_snapshot(
    endpoint: &Endpoint,
    deadline: Instant,
    budget: &mut ReadBudget,
    cancel: &dyn Cancel,
) -> WireResult<Reply> {
    read_endpoint(
        endpoint,
        "session.snapshot",
        &json!({}),
        deadline,
        budget,
        cancel,
    )
}

fn read_method(
    path: &str,
    method: &str,
    params: &Value,
    deadline: Instant,
    budget: &mut ReadBudget,
    cancel: &dyn Cancel,
) -> WireResult<Reply> {
    check(deadline, cancel)?;
    if budget.remaining() == 0 {
        return Err(Failure::new("response_limit"));
    }
    read_endpoint(&select(path)?, method, params, deadline, budget, cancel)
}

fn read_endpoint(
    endpoint: &Endpoint,
    method: &str,
    params: &Value,
    deadline: Instant,
    budget: &mut ReadBudget,
    cancel: &dyn Cancel,
) -> WireResult<Reply> {
    check(deadline, cancel)?;
    if budget.remaining() == 0 {
        return Err(Failure::new("response_limit"));
    }
    let request_id = crate::platform::random_uuid_hex().map_err(|_| Failure::new("io_error"))?;
    let request = frame(&request_id, method, params)?;
    let socket = crate::platform::observation_socket::Socket::connect(endpoint, deadline, cancel)?;
    crate::platform::observation_socket::fence(endpoint)?;
    socket.send(&request, deadline, cancel)?;
    crate::platform::observation_socket::fence(endpoint)?;
    let payload = socket.receive(deadline, budget, cancel)?;
    crate::platform::observation_socket::fence(endpoint)?;
    check(deadline, cancel)?;
    Ok(Reply {
        payload,
        request_id,
        endpoint: endpoint.clone(),
    })
}

/// Single-attempt ephemeral view transport; never expands the observer allowlist.
pub(crate) fn view_call(
    endpoint: &Endpoint,
    method: &str,
    params: &Value,
    deadline: Instant,
    cancel: &dyn Cancel,
    validate: &dyn Fn(&[u8], &str) -> WireResult<Value>,
) -> WireResult<Value> {
    view_call_guarded(
        endpoint,
        method,
        params,
        deadline,
        cancel,
        validate,
        &|| Ok(()),
    )
}

/// The preflight runs after serialization, connection and endpoint fencing, while
/// delivery is still Unsent. It is never called again after a send is attempted.
pub(crate) fn view_call_guarded(
    endpoint: &Endpoint,
    method: &str,
    params: &Value,
    deadline: Instant,
    cancel: &dyn Cancel,
    validate: &dyn Fn(&[u8], &str) -> WireResult<Value>,
    pre_send: &dyn Fn() -> WireResult<()>,
) -> WireResult<Value> {
    let mut sent = false;
    let result = (|| {
        check(deadline, cancel)?;
        if !matches!(
            method,
            "integration.view.open"
                | "integration.view.replace"
                | "integration.view.poll"
                | "integration.view.close"
        ) {
            return Err(Failure::new("invalid_view_method"));
        }
        if select(&endpoint.selected)? != *endpoint {
            return Err(Failure::new("endpoint_replaced"));
        }
        let id = crate::platform::random_uuid_hex().map_err(|_| Failure::new("io_error"))?;
        let request = frame(&id, method, params)?;
        let socket =
            crate::platform::observation_socket::Socket::connect(endpoint, deadline, cancel)?;
        crate::platform::observation_socket::fence(endpoint)?;
        check(deadline, cancel)?;
        let qualified = pre_send();
        check(deadline, cancel)?;
        qualified?;
        sent = true;
        socket.send(&request, deadline, cancel)?;
        crate::platform::observation_socket::fence(endpoint)?;
        let payload = socket.receive(deadline, &mut ReadBudget::new(256 * 1024), cancel)?;
        crate::platform::observation_socket::fence(endpoint)?;
        check(deadline, cancel)?;
        let value = validate(&payload, &id)?;
        check(deadline, cancel)?;
        Ok(value)
    })();
    result.map_err(|mut failure: Failure| {
        if sent && failure.delivery != Delivery::Rejected {
            failure.delivery = Delivery::Uncertain;
        }
        failure
    })
}

pub(crate) fn annotate(
    path: &str,
    frozen_endpoint: &Value,
    params: &mut Value,
    deadline: Instant,
    max_response_bytes: usize,
    cancel: &dyn Cancel,
    prepare: &mut dyn FnMut(&mut Value) -> WireResult<()>,
) -> WireResult<()> {
    let mut started_write = false;
    let result = (|| {
        check(deadline, cancel)?;
        if max_response_bytes == 0 {
            return Err(Failure::new("response_limit"));
        }
        let endpoint = select(path)?;
        if endpoint
            .identity()
            .map_err(|error| Failure::new(error.code()))?
            != *frozen_endpoint
        {
            return Err(Failure::new("endpoint_replaced"));
        }
        let request_id =
            crate::platform::random_uuid_hex().map_err(|_| Failure::new("io_error"))?;
        frame(&request_id, "pane.annotate", params)?;
        let socket =
            crate::platform::observation_socket::Socket::connect(&endpoint, deadline, cancel)?;
        // Refresh decreasing evidence ceilings after connection and serialization, never after send.
        check(deadline, cancel)?;
        prepare(params)?;
        frame(&request_id, "pane.annotate", params)?;
        check(deadline, cancel)?;
        prepare(params)?;
        let request = frame(&request_id, "pane.annotate", params)?;
        crate::platform::observation_socket::fence(&endpoint)?;
        check(deadline, cancel)?;
        // A failed first send can still have delivered bytes. No replay is safe from here.
        started_write = true;
        socket.send(&request, deadline, cancel)?;
        crate::platform::observation_socket::fence(&endpoint)?;
        let payload = socket.receive(deadline, &mut ReadBudget::new(max_response_bytes), cancel)?;
        crate::platform::observation_socket::fence(&endpoint)?;
        check(deadline, cancel)?;
        let root = crate::observation_wire::decode_ack(&payload)
            .map_err(|error| Failure::new(error.code()));
        check(deadline, cancel)?;
        let outcome = acknowledgement(&root?, &request_id);
        check(deadline, cancel)?;
        outcome
    })();
    result.map_err(|mut failure: Failure| {
        // Only our validated peer rejection may assert that an attempted write was rejected.
        if started_write && failure.delivery != Delivery::Rejected {
            failure.delivery = Delivery::Uncertain;
        } else if !started_write {
            failure.delivery = Delivery::Unsent;
        }
        failure
    })
}

fn acknowledgement(root: &Value, request_id: &str) -> WireResult<()> {
    let schema = || Failure::new("schema_error");
    let object = root.as_object().ok_or_else(schema)?;
    if object.get("id").and_then(Value::as_str) != Some(request_id)
        || object.contains_key("result") == object.contains_key("error")
    {
        return Err(schema());
    }
    if let Some(error) = object.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_str)
            .ok_or_else(schema)?;
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .ok_or_else(schema)?;
        if code.is_empty()
            || code.len() > 64
            || !code
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
            || message.chars().count() > 4096
        {
            return Err(schema());
        }
        return Err(Failure {
            code: format!("server_error:{code}"),
            delivery: Delivery::Rejected,
            interrupted: false,
        });
    }
    if object.get("result") != Some(&json!({"type": "ok"})) {
        return Err(schema());
    }
    Ok(())
}

#[cfg(test)]
#[path = "observation_socket/tests.rs"]
mod tests;
