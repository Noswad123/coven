//! Explicit native prompt admission. No discovery, retries, or terminal input.
use crate::observation_socket::{
    Cancel, Delivery, Failure, InterruptGuard, ReadBudget, WireResult, check,
};
use crate::{Error, Result, task_mutation::TaskOutput};
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod endpoint;
mod protocol;

#[cfg(all(test, unix))]
type AfterAckHook = Box<dyn FnMut(&JsonValue)>;

#[cfg(all(test, unix))]
thread_local! {
    static AFTER_ACK: std::cell::RefCell<Option<AfterAckHook>> = const {
        std::cell::RefCell::new(None)
    };
}

const LIMIT: usize = 65536;
const NATIVE_CONTRACT: &str = "djinn.interactive-prompt.v1";
const LIMITATIONS: [&str; 7] = [
    "Native acceptance acknowledges a message, not worker-policy admission, model execution or job completion.",
    "The selected runtime must explicitly enable its native prompt socket at startup; no discovery or fallback.",
    "Native configuration, plugins, credentials and permissions remain independent; no sandbox or grant transfer.",
    "No automatic retry or durable reconciliation; inspect native state after an uncertain outcome.",
    "Request suppression is process-local and bounded; a changed runtime is not a retry permission.",
    "Socket ownership/identity checks are not authentication or protection against hostile same-user races.",
    "Workspace is control context, not a native cwd/resource scope; filesystem calls are not interruptible by the budget.",
];

pub struct Request {
    pub workspace: PathBuf,
    pub native_socket: String,
    pub expected_runtime: String,
    pub session: String,
    pub request_id: String,
    pub prompt_file: String,
    pub execute: bool,
    pub timeout: f64,
}

fn path(value: &str) -> Result<()> {
    crate::error::require(
        value.starts_with('/')
            && !value.ends_with('/')
            && value.len() <= 4096
            && !value.split('/').any(|part| part == "..")
            && !value
                .chars()
                .any(|c| c <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&c)),
        "invalid_prompt_path",
    )
}

fn arguments(socket: &str, timeout: f64) -> Result<()> {
    path(socket)?;
    crate::error::require(
        timeout.is_finite() && timeout > 0.0 && timeout <= 30.0,
        "invalid_prompt_timeout",
    )
}

fn preview(request: &Request) -> Result<JsonValue> {
    arguments(&request.native_socket, request.timeout)?;
    path(&request.prompt_file)?;
    let workspace = crate::session_support::absolute(&request.workspace)?;
    let workspace = workspace.to_str().ok_or(Error("invalid_prompt_path"))?;
    path(workspace)?;
    crate::error::require(
        protocol::uuid(&request.expected_runtime) && protocol::uuid(&request.request_id),
        "invalid_prompt_uuid",
    )?;
    crate::error::require(
        protocol::identifier(&request.session, "ses_"),
        "invalid_native_session",
    )?;
    Ok(json!({
        "schema_version": 1, "contract": "coven.interactive-prompt.v1",
        "workspace": workspace, "native_socket": request.native_socket,
        "expected_runtime_id": request.expected_runtime, "session_id": request.session,
        "request_id": request.request_id, "prompt_file": request.prompt_file,
        "outcome": "preview", "delivery": "not_attempted", "authorization": "none",
        "native_prompt_acceptance": "unestablished", "native_worker_acceptance": "unestablished",
        "native_lifecycle": "unestablished", "job_completion": "unestablished",
        "prompt_sha256": null, "prompt_bytes": null, "native_message_id": null,
        "diagnostics": [], "limitations": LIMITATIONS,
    }))
}

fn failure(report: &mut JsonValue, error: Failure) {
    let delivery = match error.delivery {
        Delivery::Unsent => "unsent",
        Delivery::Rejected => "rejected",
        Delivery::Uncertain => "uncertain",
    };
    report["outcome"] = json!(if delivery == "unsent" {
        "unavailable"
    } else {
        delivery
    });
    report["delivery"] = json!(delivery);
    report["diagnostics"] = json!([error.code]);
    if error.interrupted {
        report["interrupted"] = json!(true);
    }
}

fn render(report: &JsonValue, json: bool, capabilities: bool) -> Result<TaskOutput> {
    let mut output = crate::session_support::output(report, !json)?;
    output.exit_code = if report.get("interrupted").and_then(JsonValue::as_bool) == Some(true) {
        130
    } else if capabilities {
        u8::from(report["assessment"] != "observed")
    } else {
        match report["outcome"].as_str() {
            Some("preview" | "accepted") => 0,
            Some("uncertain") => 2,
            _ => 1,
        }
    };
    Ok(output)
}

pub fn capabilities_arguments(socket: &str, timeout: f64, json: bool) -> Result<TaskOutput> {
    arguments(socket, timeout)?;
    let cancel = InterruptGuard::install()?;
    let deadline = Instant::now() + Duration::from_secs_f64(timeout);
    let mut report = json!({"schema_version":1, "contract":"coven.native-prompt-capabilities.v1",
        "assessment":"unavailable", "authorization":"none", "runtime_id":null, "diagnostics":[]});
    let result = (|| {
        let endpoint = endpoint::Endpoint::select(socket)?;
        let response = exchange(
            &endpoint,
            protocol::Method::Capabilities,
            &json!({}),
            &fresh_id()?,
            deadline,
            &mut ReadBudget::new(LIMIT),
            &cancel,
        )?;
        protocol::capability(&response)
    })();
    match result {
        Ok(runtime) => {
            report["assessment"] = json!("observed");
            report["runtime_id"] = json!(runtime);
            report["native_contract"] = json!(NATIVE_CONTRACT);
        }
        Err(mut error) => {
            if error.code == "prompt_or_endpoint_unavailable" {
                error.code = "native_io_error".into();
            }
            report["diagnostics"] = json!([error.code]);
            if error.interrupted {
                report["interrupted"] = json!(true);
            }
        }
    }
    if cancel.interrupted() {
        report["interrupted"] = json!(true);
        report["diagnostics"] = json!(["interrupted"]);
    }
    render(&report, json, true)
}

pub fn prompt_arguments(request: &Request, json: bool) -> Result<TaskOutput> {
    let mut report = preview(request)?;
    if !request.execute {
        return render(&report, json, false);
    }
    let cancel = InterruptGuard::install()?;
    let root = Path::new(
        report["workspace"]
            .as_str()
            .ok_or(Error("invalid_prompt_path"))?,
    )
    .to_path_buf();
    let gate = crate::session_support::admit(&root)?;
    crate::session_support::validate_admission(&root, &gate)?;
    report["outcome"] = json!("unavailable");
    report["delivery"] = json!("unsent");
    report["authorization"] = json!("interactive_native_prompt_submission_only");
    let deadline = Instant::now() + Duration::from_secs_f64(request.timeout);
    let result = (|| {
        check(deadline, &cancel)?;
        let text = endpoint::prompt_file(&request.prompt_file)?;
        let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
        report["prompt_sha256"] = json!(digest);
        report["prompt_bytes"] = json!(text.len());
        let endpoint = endpoint::Endpoint::select(&request.native_socket)?;
        let mut budget = ReadBudget::new(LIMIT);
        let response = exchange(
            &endpoint,
            protocol::Method::Capabilities,
            &json!({}),
            &fresh_id()?,
            deadline,
            &mut budget,
            &cancel,
        )?;
        if protocol::capability(&response)? != request.expected_runtime {
            return Err(Failure::new("native_runtime_mismatch"));
        }
        let params = json!({"expected_runtime_id":request.expected_runtime, "session_id":request.session, "text":text});
        let response = exchange(
            &endpoint,
            protocol::Method::Submit,
            &params,
            &request.request_id,
            deadline,
            &mut budget,
            &cancel,
        )?;
        let message = protocol::receipt(&response, request, &digest, text.len())?;
        // Record the accepted fact before observing any subsequent interruption.
        report["outcome"] = json!("accepted");
        report["delivery"] = json!("accepted");
        report["native_prompt_acceptance"] = json!("accepted");
        report["native_message_id"] = json!(message);
        #[cfg(all(test, unix))]
        AFTER_ACK.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut() {
                hook(&report);
            }
        });
        Ok(())
    })();
    if let Err(error) = result {
        failure(&mut report, error);
    }
    if cancel.interrupted() {
        report["interrupted"] = json!(true);
        report["diagnostics"] = json!(["interrupted"]);
    }
    let output = render(&report, json, false);
    drop(gate);
    output
}

fn fresh_id() -> WireResult<String> {
    let raw = crate::platform::random_uuid_hex().map_err(|_| Failure::new("native_io_error"))?;
    if raw.len() != 32 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Failure::new("native_io_error"));
    }
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &raw[..8],
        &raw[8..12],
        &raw[12..16],
        &raw[16..20],
        &raw[20..]
    ))
}

fn exchange(
    endpoint: &endpoint::Endpoint,
    method: protocol::Method,
    params: &JsonValue,
    id: &str,
    deadline: Instant,
    budget: &mut ReadBudget,
    cancel: &dyn Cancel,
) -> WireResult<crate::values::Value> {
    let mut possible_send = false;
    let result = (|| {
        check(deadline, cancel)?;
        if budget.remaining() == 0 {
            return Err(Failure::new("response_limit"));
        }
        endpoint.fence()?;
        let mut frame = serde_json::to_vec(
            &json!({"version":1,"id":id,"method":method.name(),"params":params}),
        )
        .map_err(|_| Failure::new("schema_error"))?;
        frame.push(b'\n');
        let socket = crate::platform::observation_socket::Socket::connect(
            &endpoint.socket,
            deadline,
            cancel,
        )?;
        endpoint.fence()?;
        check(deadline, cancel)?;
        possible_send = true;
        socket.send(&frame, deadline, cancel)?;
        let payload = socket.receive(deadline, budget, cancel)?;
        endpoint.fence()?;
        check(deadline, cancel)?;
        let response = protocol::decode(&payload, id)?;
        check(deadline, cancel)?;
        Ok(response)
    })();
    result.map_err(|mut error: Failure| {
        if possible_send && matches!(method, protocol::Method::Submit) {
            error.delivery = Delivery::Uncertain;
        }
        error.code = match error.code.as_str() {
            "io_error" => "native_io_error".into(),
            "incomplete_response" | "trailing_data" => "invalid_native_frame".into(),
            "endpoint_replaced" | "unsafe_endpoint" => "native_endpoint_changed".into(),
            _ => error.code,
        };
        error
    })
}

#[cfg(test)]
mod tests;
