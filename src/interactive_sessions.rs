//! Explicit interactive input submission, not native execution or worker admission.
use crate::observation_socket::{
    self as wire, Cancel, Delivery, Endpoint, Failure, ReadBudget, Reply, WireResult,
};
use crate::platform::observation_socket::{Socket, fence};
use crate::transport_gate::TransportGuard;
use crate::values::Value as Native;
use crate::{Error, Result, error::require, task_mutation::TaskOutput};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const LIMITATIONS: [&str; 6] = [
    "Input queued is not native startup, session identity, readiness, worker acceptance or completion.",
    "Select a quiescent empty shell prompt; other writers and post-check foreground changes remain possible.",
    "The native UI uses its own configuration, credentials and permissions; this is not a resource/tool sandbox.",
    "Executable/socket metadata checks are not authentication or protection against hostile same-user races.",
    "No durable submission receipt or automatic retry: inspect native state after an uncertain outcome.",
    "Workspace is control context, not a cwd change. Read requests may trigger existing server housekeeping.",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Start,
    Resume,
}

pub struct Request {
    pub workspace: PathBuf,
    pub operation: Operation,
    pub runtime: String,
    pub executable: String,
    pub socket: String,
    pub pane: String,
    pub expected_terminal: String,
    pub session: Option<String>,
    pub timeout: f64,
    pub execute: bool,
}

// No deserialized or caller-mutable plan can add native arguments or authorization.
struct Plan {
    report: Value,
    workspace: PathBuf,
    timeout: f64,
}

fn text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 4096
        && !value
            .chars()
            .any(|c| c <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&c))
}

fn absolute(value: &str) -> bool {
    text(value)
        && value.starts_with('/')
        && !value.ends_with('/')
        && !value.split('/').any(|part| part == "..")
}

impl Plan {
    fn new(request: &Request) -> Result<Self> {
        require(
            request.timeout.is_finite() && request.timeout > 0.0 && request.timeout <= 30.0,
            "invalid_timeout",
        )?;
        // absolute() is lexical only: absent workspaces and paths are valid previews.
        let workspace = crate::session_support::absolute(&request.workspace)?;
        let root = workspace.to_str().ok_or(Error("invalid_workspace_path"))?;
        require(absolute(root), "invalid_workspace_path")?;
        require(
            request.runtime == "djinn",
            "unsupported_interactive_runtime",
        )?;
        require(absolute(&request.executable), "invalid_executable_path")?;
        require(absolute(&request.socket), "invalid_socket_path")?;
        for value in [&request.pane, &request.expected_terminal] {
            require(
                text(value) && crate::kitsune_observation::session_opaque(value),
                "invalid_interactive_identifier",
            )?;
        }
        let (operation, argv) = match request.operation {
            Operation::Start => {
                require(
                    request.session.is_none(),
                    "start_session_selector_forbidden",
                )?;
                ("start", vec![request.executable.clone(), "--ui".into()])
            }
            Operation::Resume => {
                let selector = request
                    .session
                    .as_deref()
                    .ok_or(Error("missing_session_selector"))?;
                require(
                    text(selector) && !selector.starts_with('-'),
                    "invalid_session_selector",
                )?;
                (
                    "resume",
                    vec![
                        request.executable.clone(),
                        "--session".into(),
                        selector.into(),
                    ],
                )
            }
        };
        Ok(Self {
            report: json!({"schema_version":1,"contract":"coven.interactive-session.v1",
                "workspace":root,"operation":operation,"runtime":"djinn",
                "executable":request.executable,"socket":request.socket,"pane_id":request.pane,
                "expected_terminal_id":request.expected_terminal,"session_selector":request.session,
                "argv":argv,"outcome":"preview","delivery":"not_attempted","authorization":"none",
                "native_lifecycle":"unestablished","native_worker_acceptance":"unestablished",
                "job_completion":"unestablished","diagnostics":[],"limitations":LIMITATIONS}),
            workspace,
            timeout: request.timeout,
        })
    }
}

pub fn arguments(request: &Request, json_output: bool) -> Result<TaskOutput> {
    let plan = Plan::new(request)?;
    let report = if request.execute {
        let gate = crate::session_support::admit(&plan.workspace)?;
        let interrupt = wire::InterruptGuard::install()?;
        let mut report = execute(&plan, &gate, &interrupt)?;
        // A late interrupt must not erase an already recorded queue acknowledgement.
        if interrupt.interrupted() {
            report["interrupted"] = json!(true);
            if report["diagnostics"] == json!([]) {
                report["diagnostics"] = json!(["interrupted"]);
            }
        }
        report
    } else {
        plan.report
    };
    render(&report, json_output)
}

fn render(report: &Value, json_output: bool) -> Result<TaskOutput> {
    let mut output = if json_output {
        crate::session_support::output(report, false)?
    } else {
        let argv = report["argv"]
            .as_array()
            .ok_or(Error("invalid_interactive_report"))?;
        let command = argv
            .iter()
            .map(|arg| {
                arg.as_str()
                    .map(crate::workspace_assets::shell_quote)
                    .ok_or(Error("invalid_interactive_report"))
            })
            .collect::<Result<Vec<_>>>()?
            .join(" ");
        let mut stdout = format!(
            "{}: {} Djinn UI in {}\nCommand: {}\n{}",
            report["outcome"].as_str().unwrap_or_default(),
            report["operation"].as_str().unwrap_or_default(),
            report["pane_id"].as_str().unwrap_or_default(),
            command,
            if report["outcome"] == "preview" {
                "Preview only; --execute explicitly submits to a quiescent empty shell prompt."
            } else {
                "Input acknowledgement is not proof of native startup, resume or readiness."
            }
        );
        for diagnostic in report["diagnostics"].as_array().into_iter().flatten() {
            stdout.push_str("\nDiagnostic: ");
            stdout.push_str(diagnostic.as_str().unwrap_or_default());
        }
        TaskOutput {
            stdout,
            warnings: vec![],
            exit_code: 0,
        }
    };
    output.exit_code = if report["interrupted"] == true {
        130
    } else if matches!(report["outcome"].as_str(), Some("preview" | "input_queued")) {
        0
    } else if report["delivery"] == "uncertain" {
        2
    } else {
        1
    };
    Ok(output)
}

#[cfg(unix)]
fn executable(path: &str) -> WireResult<(u64, u64, u64, i64, i64)> {
    use std::os::unix::fs::MetadataExt;
    let unavailable = || Failure::new("executable_unavailable");
    // Follow symlinks, as stat/X_OK do. Do not impose binary-hash or ownership policy.
    let info = std::fs::metadata(path).map_err(|_| unavailable())?;
    let path = std::ffi::CString::new(path).map_err(|_| unavailable())?;
    if !info.is_file() || unsafe { libc::access(path.as_ptr(), libc::X_OK) } != 0 {
        return Err(unavailable());
    }
    Ok((
        info.dev(),
        info.ino(),
        info.size(),
        info.mtime(),
        info.mtime_nsec(),
    ))
}

#[cfg(not(unix))]
fn executable(_path: &str) -> WireResult<(u64, u64, u64, i64, i64)> {
    Err(Failure::new("unsupported_platform"))
}

fn snapshot(
    plan: &Plan,
    endpoint: &Endpoint,
    deadline: Instant,
    budget: &mut ReadBudget,
    cancel: &dyn Cancel,
) -> WireResult<Value> {
    wire::check(deadline, cancel)?;
    if budget.remaining() == 0 {
        return Err(Failure::new("response_limit"));
    }
    fence(endpoint)?;
    let reply = wire::read(
        &endpoint.selected,
        wire::ReadMethod::Snapshot,
        &json!({}),
        deadline,
        budget,
        cancel,
    )?;
    if reply.endpoint != *endpoint {
        return Err(Failure::new("endpoint_replaced"));
    }
    fence(endpoint)?;
    selected_mapping(plan, &reply)
}

fn selected_mapping(plan: &Plan, reply: &Reply) -> WireResult<Value> {
    let snapshot = crate::kitsune_observation::command_snapshot(reply)?;
    let pane = snapshot["panes"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["pane_id"] == plan.report["pane_id"])
        .ok_or_else(|| Failure::new("pane_not_found"))?;
    if pane["terminal_id"] != plan.report["expected_terminal_id"] {
        return Err(Failure::new("terminal_mismatch"));
    }
    Ok(pane.clone())
}

fn execute(plan: &Plan, gate: &TransportGuard, cancel: &dyn Cancel) -> Result<Value> {
    crate::session_support::validate_admission(&plan.workspace, gate)?;
    let mut report = plan.report.clone();
    report["authorization"] = json!("interactive_terminal_submission_only");
    report["outcome"] = json!("unavailable");
    report["delivery"] = json!("unsent");
    let deadline = Instant::now() + Duration::from_secs_f64(plan.timeout);
    let outcome = (|| {
        wire::check(deadline, cancel)?;
        let path = plan.report["executable"].as_str().unwrap();
        let original = executable(path)?;
        let endpoint = wire::select(plan.report["socket"].as_str().unwrap())?;
        let mut budget = ReadBudget::new(1_048_576);
        let before = snapshot(plan, &endpoint, deadline, &mut budget, cancel)?;
        let after = snapshot(plan, &endpoint, deadline, &mut budget, cancel)?;
        if before != after {
            return Err(Failure::new("pane_mapping_changed"));
        }
        if executable(path)? != original {
            return Err(Failure::new("executable_changed"));
        }
        submit(plan, &endpoint, deadline, &mut budget, cancel)
    })();
    match outcome {
        Ok(()) => {
            report["outcome"] = json!("input_queued");
            report["delivery"] = json!("queued");
        }
        Err(failure) => {
            let (outcome, delivery) = match failure.delivery {
                Delivery::Unsent => ("unavailable", "unsent"),
                Delivery::Rejected => ("rejected", "rejected"),
                Delivery::Uncertain => ("uncertain", "uncertain"),
            };
            report["outcome"] = json!(outcome);
            report["delivery"] = json!(delivery);
            report["diagnostics"] = json!([failure.code]);
            if failure.interrupted {
                report["interrupted"] = json!(true);
            }
        }
    }
    if cancel.interrupted() {
        report["interrupted"] = json!(true);
    }
    Ok(report)
}

fn submit(
    plan: &Plan,
    endpoint: &Endpoint,
    deadline: Instant,
    budget: &mut ReadBudget,
    cancel: &dyn Cancel,
) -> WireResult<()> {
    let mut possible_send = false;
    let outcome = (|| {
        wire::check(deadline, cancel)?;
        if budget.remaining() == 0 {
            return Err(Failure::new("response_limit"));
        }
        fence(endpoint)?;
        let id = crate::platform::random_uuid_hex().map_err(|_| Failure::new("io_error"))?;
        let params = json!({"pane_id":plan.report["pane_id"],"expected_terminal_id":plan.report["expected_terminal_id"],"argv":plan.report["argv"]});
        let mut frame =
            serde_json::to_vec(&json!({"id":id,"method":"pane.submit_command","params":params}))
                .map_err(|_| Failure::new("schema_error"))?;
        frame.push(b'\n');
        let socket = Socket::connect(endpoint, deadline, cancel)?;
        fence(endpoint)?;
        wire::check(deadline, cancel)?;
        // The first send can partially deliver even when it returns an error.
        possible_send = true;
        socket.send(&frame, deadline, cancel)?;
        fence(endpoint)?;
        let payload = socket.receive(deadline, budget, cancel)?;
        fence(endpoint)?;
        wire::check(deadline, cancel)?;
        let acknowledgement = acknowledgement(&payload, &id, &plan.report);
        wire::check(deadline, cancel)?;
        acknowledgement
    })();
    outcome.map_err(|mut failure: Failure| {
        if possible_send && failure.delivery != Delivery::Rejected {
            failure.delivery = Delivery::Uncertain;
        }
        failure
    })
}

fn acknowledgement(payload: &[u8], id: &str, plan: &Value) -> WireResult<()> {
    let schema = || Failure::new("schema_error");
    // Validate the entire native tree, including ignored finite, large-integer extensions.
    let root =
        crate::observation_wire::decode_native(payload).map_err(|e| Failure::new(e.code()))?;
    let root = root.as_object().ok_or_else(schema)?;
    if root.get("id").and_then(Native::as_str) != Some(id)
        || root.get("result").is_some() == root.get("error").is_some()
    {
        return Err(schema());
    }
    if let Some(error) = root.get("error") {
        let error = error.as_object().ok_or_else(schema)?;
        let code = error
            .get("code")
            .and_then(Native::as_str)
            .ok_or_else(schema)?;
        error
            .get("message")
            .and_then(Native::as_str)
            .ok_or_else(schema)?;
        if code.is_empty()
            || code.len() > 64
            || !code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        {
            return Err(schema());
        }
        return Err(Failure {
            code: format!("server_error:{code}"),
            delivery: Delivery::Rejected,
            interrupted: false,
        });
    }
    let result = root
        .get("result")
        .and_then(Native::as_object)
        .ok_or_else(schema)?;
    for (key, expected) in [
        ("type", "pane_command_submitted"),
        ("pane_id", plan["pane_id"].as_str().unwrap()),
        (
            "terminal_id",
            plan["expected_terminal_id"].as_str().unwrap(),
        ),
    ] {
        if result.get(key).and_then(Native::as_str) != Some(expected) {
            return Err(schema());
        }
    }
    let argv = result
        .get("argv")
        .and_then(Native::as_array)
        .ok_or_else(schema)?;
    let expected = plan["argv"].as_array().unwrap();
    if argv.len() != expected.len()
        || !argv
            .iter()
            .zip(expected)
            .all(|(a, b)| a.as_str().is_some() && a.as_str() == b.as_str())
    {
        return Err(schema());
    }
    Ok(())
}

#[cfg(test)]
#[path = "interactive_sessions/tests.rs"]
mod tests;
