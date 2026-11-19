use super::{
    config::EndpointConfig,
    source::{self, Request},
};
use crate::{Error, Result};
use serde::Deserialize;
use serde_json::Value;
use std::time::Instant;

pub(super) const HELPER_LIMIT: usize = 1024 * 1024;

pub(super) fn probe_code(code: &str) -> Option<&'static str> {
    Some(match code {
        "unsupported_protocol" => "unsupported_protocol",
        "timeout" => "timeout",
        "response_limit" => "response_limit",
        "request_limit" => "request_limit",
        "invalid_probe_request" => "invalid_probe_request",
        "invalid_reference" => "invalid_reference",
        "invalid_socket_path" => "invalid_socket_path",
        "terminal_replaced" => "terminal_replaced",
        "schema_error" => "schema_error",
        "process_discovery_unavailable" => "process_discovery_unavailable",
        "output_unsupported" => "output_unsupported",
        "upstream_unavailable" | "upstream_rejected" => "upstream_unavailable",
        _ => return None,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum HelperReply<T> {
    #[serde(rename = "result")]
    Result(T),
    #[serde(rename = "error")]
    Error(HelperError),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HelperError {
    code: String,
}

pub(super) fn decode_helper(payload: &[u8], request: &Request) -> Result<Value> {
    fn decode<T: serde::de::DeserializeOwned + serde::Serialize>(payload: &[u8]) -> Result<Value> {
        match serde_json::from_slice::<HelperReply<T>>(payload)
            .map_err(|_| Error("schema_error"))?
        {
            HelperReply::Result(result) => {
                serde_json::to_value(result).map_err(|_| Error("schema_error"))
            }
            HelperReply::Error(error) => {
                let code = probe_code(&error.code)
                    .filter(|code| *code == error.code)
                    .ok_or(Error("schema_error"))?;
                Err(Error(code))
            }
        }
    }
    if payload.len() > HELPER_LIMIT {
        return Err(Error("response_limit"));
    }
    match request {
        Request::Inventory {} => {
            let value = decode::<super::model::Inventory>(payload)?;
            serde_json::to_value(super::parse_inventory(value)?).map_err(|_| Error("schema_error"))
        }
        Request::Output { .. } => decode::<super::model::Output>(payload),
    }
}

pub(super) trait Transport: Send + Sync {
    fn call(
        &self,
        endpoint: &EndpointConfig,
        request: &Request,
        deadline: Instant,
    ) -> Result<Value>;
}

pub(super) struct NativeTransport;

impl Transport for NativeTransport {
    fn call(
        &self,
        endpoint: &EndpointConfig,
        request: &Request,
        deadline: Instant,
    ) -> Result<Value> {
        if endpoint.ssh.is_none() {
            return source::execute(&endpoint.socket, request, deadline);
        }
        remote(endpoint, request, deadline)
    }
}

#[cfg(not(unix))]
fn remote(_: &EndpointConfig, _: &Request, _: Instant) -> Result<Value> {
    Err(Error("upstream_unavailable"))
}

#[cfg(unix)]
fn remote(endpoint: &EndpointConfig, request: &Request, deadline: Instant) -> Result<Value> {
    let ssh = endpoint.ssh.as_ref().ok_or(Error("invalid_ssh_config"))?;
    let input = serde_json::to_vec(request).map_err(|_| Error("schema_error"))?;
    // Paths/target were validated against a shell-metacharacter-free grammar. Remote
    // shell parsing is unavoidable in SSH; no browser-controlled value reaches argv.
    // SSH config is disabled: target must resolve directly, not through a Host alias.
    let mut command = std::process::Command::new("/usr/bin/ssh");
    command.args([
        "-F",
        "/dev/null",
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "IdentitiesOnly=yes",
        "-i",
        "~/.ssh/id_ed25519",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "PasswordAuthentication=no",
        "-o",
        "KbdInteractiveAuthentication=no",
        "-o",
        "NumberOfPasswordPrompts=0",
        "-o",
        "ForwardAgent=no",
        "-o",
        "ClearAllForwardings=yes",
        "-o",
        "ForwardX11=no",
        "-o",
        "PermitLocalCommand=no",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
        "-o",
        "ControlPersist=no",
        "-o",
        "ConnectTimeout=5",
        "-o",
        "ConnectionAttempts=1",
        "--",
        &ssh.target,
        &ssh.helper,
        "observer",
        "probe",
        "--socket",
        &endpoint.socket,
    ]);
    let out = run_helper(&mut command, &input, deadline)?;
    // Closed envelopes preserve only whitelisted structured helper failures.
    decode_helper(&out, request)
}

#[cfg(unix)]
pub(super) fn run_helper(
    command: &mut std::process::Command,
    input: &[u8],
    deadline: Instant,
) -> Result<Vec<u8>> {
    use std::{
        io::{Read, Write},
        os::fd::AsRawFd,
        process::{Child, Stdio},
        thread,
        time::Duration,
    };
    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            // SSH has no local proxy/command children: config files are disabled.
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn nonblocking(fd: &impl AsRawFd) -> Result<()> {
        // Pipes belong exclusively to this bounded operation; no reader threads can leak.
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            return Err(Error("upstream_unavailable"));
        }
        Ok(())
    }
    fn drain(reader: &mut impl Read, out: &mut Vec<u8>, max: usize) -> Result<bool> {
        let mut bytes = [0; 8192];
        // Bound work per turn as well as total bytes so stderr cannot starve deadlines.
        for _ in 0..16 {
            match reader.read(&mut bytes) {
                Ok(0) => return Ok(true),
                Ok(n) => {
                    if n > max.saturating_sub(out.len()) {
                        return Err(Error("response_limit"));
                    }
                    out.extend_from_slice(&bytes[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(Error("upstream_unavailable")),
            }
        }
        Ok(false)
    }
    if input.len() > 4096 {
        return Err(Error("request_limit"));
    }
    if Instant::now() >= deadline {
        return Err(Error("timeout"));
    }
    let mut child = ChildGuard(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| Error("upstream_unavailable"))?,
    );
    let mut stdin = child.0.stdin.take().ok_or(Error("upstream_unavailable"))?;
    let mut stdout = child.0.stdout.take().ok_or(Error("upstream_unavailable"))?;
    let mut stderr = child.0.stderr.take().ok_or(Error("upstream_unavailable"))?;
    nonblocking(&stdin)?;
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    match stdin.write(input) {
        Ok(n) if n == input.len() => (),
        _ => return Err(Error("upstream_unavailable")),
    }
    drop(stdin);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    loop {
        if Instant::now() >= deadline {
            return Err(Error("timeout"));
        }
        let out_done = drain(&mut stdout, &mut out, HELPER_LIMIT)?;
        let err_done = drain(&mut stderr, &mut err, 16 * 1024)?;
        if let Some(status) = child
            .0
            .try_wait()
            .map_err(|_| Error("upstream_unavailable"))?
        {
            if !status.success() {
                return Err(Error("upstream_unavailable"));
            }
            if out_done && err_done {
                return Ok(out);
            }
        }
        thread::sleep(
            Duration::from_millis(5).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}
