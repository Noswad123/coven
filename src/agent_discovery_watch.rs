//! Bounded foreground sampling. Optional UI delivery never grants execution authority.
use crate::{
    Result,
    agent_discovery::{self, Request},
    error::require,
    observation_socket::{self as socket, Cancel},
    task_mutation::TaskOutput,
};
use serde_json::{Value, json};
use std::{
    io::Write,
    time::{Duration, Instant},
};

mod presentation;

pub struct WatchRequest {
    pub discovery: Request,
    pub duration: f64,
    pub interval: f64,
    pub source: Option<String>,
}

fn validate(request: &WatchRequest) -> Result<()> {
    agent_discovery::validate(&request.discovery)?;
    require(
        request.duration.is_finite() && (0.1..=3600.0).contains(&request.duration),
        "invalid_duration",
    )?;
    require(
        request.interval.is_finite() && (0.1..=60.0).contains(&request.interval),
        "invalid_interval",
    )?;
    if let Some(source) = &request.source {
        crate::groups::source(source)?;
        require(
            request.interval <= 2.0 && request.discovery.timeout <= 3.0,
            "invalid_presentation_timing",
        )?;
    }
    Ok(())
}

/// Samples are streamed once; the returned summary is rendered once by the caller.
pub fn arguments(request: &WatchRequest, json: bool) -> Result<TaskOutput> {
    validate(request)?;
    let interrupt = socket::InterruptGuard::install()?;
    let stdout = std::io::stdout();
    run(
        request,
        json,
        &interrupt,
        &mut stdout.lock(),
        &mut agent_discovery::inspect_cancel,
    )
}

fn unavailable(sequence: u64, code: &str) -> Value {
    let scan_id =
        crate::platform::random_uuid_hex().unwrap_or_else(|_| format!("watch-{sequence}"));
    json!({"contract":"coven.agent-discovery.v1", "scan_id":scan_id,
        "schema_version":1,"assessment":"unavailable", "diagnostics":[code], "targets":[],
        "target_count":null,"recognized_target_count":null,"authorization":"none","dispatchable":false,
        "activity":"unknown","attention":"unknown","native_identity":"unestablished",
        "native_lifecycle":"unestablished","job_completion":"unestablished","native_catalog_complete":false,"acquisition_completeness":"unknown"})
}

fn run(
    request: &WatchRequest,
    json_output: bool,
    cancel: &dyn Cancel,
    writer: &mut dyn Write,
    collect: &mut dyn FnMut(&Request, &dyn Cancel) -> Result<Value>,
) -> Result<TaskOutput> {
    let started = Instant::now();
    let end = started + Duration::from_secs_f64(request.duration);
    let interval = Duration::from_secs_f64(request.interval);
    // Never adopt a replacement endpoint, including after an initial selection failure.
    let endpoint = socket::select(&request.discovery.socket);
    let mut ui = presentation::Presenter::new(request.source.as_deref(), endpoint.clone());
    let mut summary = json!({"contract":"coven.agent-discovery-watch.v1", "outcome":"completed",
        "samples":0,"observed":0,"unavailable":0,"incompatible":0,
        "max_targets":0,"max_sample_ms":0,"skipped_ticks":0,"diagnostics":[],
        "authorization":"none","dispatchable":false,"native_catalog_complete":false,
        "limitations":["Observation only, not a complete native catalog or execution authority. Unknown is not absence, idle or completion.",
            "Kit read request housekeeping may expire metadata, drain events, synchronize titles and ensure a default workspace or shell; zero server effects are not promised.",
            "Endpoint fences are not authentication or atomic attribution. UI delivery is ephemeral, not lossless; uncertain operations are never replayed."],
        "requested_duration_seconds":request.duration,"interval_seconds":request.interval});
    let mut next = started;
    let mut exit_code = 0;
    let mut sequence = 0u64;
    loop {
        while Instant::now() < next.min(end) && !cancel.interrupted() {
            std::thread::sleep(
                Duration::from_millis(10)
                    .min(next.min(end).saturating_duration_since(Instant::now())),
            );
        }
        if cancel.interrupted() {
            summary["outcome"] = json!("interrupted");
            exit_code = 130;
            break;
        }
        if Instant::now() >= end {
            break;
        }
        sequence += 1;
        let sample_start = Instant::now();
        let mut bounded = Request {
            socket: request.discovery.socket.clone(),
            pane: request.discovery.pane.clone(),
            expected_terminal: request.discovery.expected_terminal.clone(),
            timeout: request
                .discovery
                .timeout
                .min(end.saturating_duration_since(sample_start).as_secs_f64()),
            max_response_bytes: request.discovery.max_response_bytes,
        };
        let fenced = || -> socket::WireResult<()> {
            let frozen = endpoint.as_ref().map_err(Clone::clone)?;
            if socket::select(&request.discovery.socket)? != *frozen {
                return Err(socket::Failure::new("endpoint_replaced"));
            }
            Ok(())
        };
        let mut sample = match fenced() {
            Ok(()) => {
                let remaining = end.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                bounded.timeout = request.discovery.timeout.min(remaining.as_secs_f64());
                collect(&bounded, cancel)
                    .unwrap_or_else(|_| unavailable(sequence, "collection_error"))
            }
            Err(error) => unavailable(sequence, &error.code),
        };
        if let Err(error) = fenced() {
            sample = unavailable(sequence, &error.code);
        }
        // A failed collection is a full empty observation, never a stale positive.
        if sample["assessment"] != "observed" {
            sample["targets"] = json!([]);
        }
        let assessment = match sample["assessment"].as_str() {
            Some("observed") => "observed",
            Some("incompatible") => "incompatible",
            _ => "unavailable",
        };
        count(&mut summary, assessment);
        count(&mut summary, "samples");
        let targets = sample["targets"].as_array().map_or(0, Vec::len) as u64;
        summary["max_targets"] = json!(summary["max_targets"].as_u64().unwrap().max(targets));
        summary["max_sample_ms"] = json!(
            summary["max_sample_ms"]
                .as_u64()
                .unwrap()
                .max(sample_start.elapsed().as_millis() as u64)
        );
        let text = if json_output {
            crate::session_support::output(&sample, false)?.stdout
        } else {
            format!(
                "Discovery sample {sequence}: {assessment}; {targets} observed targets (activity and attention unknown)"
            )
        };
        if writer
            .write_all(text.as_bytes())
            .and_then(|_| writer.write_all(b"\n"))
            .and_then(|_| writer.flush())
            .is_err()
        {
            summary["outcome"] = json!("output_failure");
            summary["diagnostics"] = json!(["output_failure"]);
            exit_code = 1;
            break;
        }
        if !cancel.interrupted() && Instant::now() < end {
            ui.sample(&sample, sample_start, end, cancel);
        }
        next += interval;
        // Skip elapsed slots rather than admitting a late catch-up collection.
        let now = Instant::now();
        if next < now {
            let skipped = ((now - next).as_nanos() / interval.as_nanos() + 1) as u64;
            next += interval.mul_f64(skipped as f64);
            summary["skipped_ticks"] = json!(summary["skipped_ticks"].as_u64().unwrap() + skipped);
        }
    }
    ui.finish();
    summary["presentation"] = ui.report;
    summary["duration_ms"] = json!(started.elapsed().as_millis() as u64);
    let mut output = crate::session_support::output(&summary, !json_output)?;
    output.exit_code = exit_code;
    Ok(output)
}

fn count(report: &mut Value, key: &str) {
    report[key] = json!(report[key].as_u64().unwrap_or(0) + 1);
}

#[cfg(test)]
mod tests;
