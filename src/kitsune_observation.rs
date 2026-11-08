//! Explicit, read-only Kitsune observation. No discovery, execution or persistence.
use crate::observation_socket::{
    self, Cancel, Failure, NoCancel, ReadBudget, ReadMethod, WireResult,
};
use crate::observation_time::{Clock, SystemClock};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
#[path = "kitsune_observation/bindings.rs"]
mod bindings;
#[path = "kitsune_observation/schema.rs"]
mod schema;
pub(crate) use bindings::{
    ProcessRequest, binding_from_process, binding_from_snapshot, read_publication_snapshot,
};

const CONTRACT: &str = "coven.kitsune.observation.v1";
pub(crate) fn session_snapshot(reply: &observation_socket::Reply) -> WireResult<Value> {
    Ok((*schema::snapshot(reply, true)?).clone())
}

pub(crate) fn comparison_snapshot(reply: &observation_socket::Reply) -> WireResult<Value> {
    schema::comparison_snapshot(reply)
}

pub(crate) fn session_processes(
    reply: &observation_socket::Reply,
    pane: &str,
) -> WireResult<Vec<Value>> {
    schema::processes(reply, pane)
}

pub(crate) fn session_opaque(value: &str) -> bool {
    schema::opaque(value)
}

pub(crate) fn command_snapshot(reply: &observation_socket::Reply) -> WireResult<Value> {
    schema::command_snapshot(reply)
}

const PROCESS_CONTRACT: &str = "coven.kitsune.djinn-process.v1";
const LIMITATIONS: [&str; 3] = [
    "Observation only: no execution authorization or UI return channel. Kitsune's request pipeline performs metadata expiry, event drainage and title synchronization, and may ensure a default workspace (potentially spawning a shell) in an empty existing session. Zero server-side effects are not promised.",
    "Socket ownership, permissions and connection are not peer authentication. Parent path aliases are resolved but parent directories are trusted; stat checks cannot eliminate path replacement races. Device/inode identify the observed endpoint, not a globally stable server identity.",
    "Snapshot receipt is not runtime freshness attestation. Native references may be persisted; pane presence, done status and empty inventories do not establish native lifecycle, runtime shutdown, job completion or success.",
];
const PROCESS_LIMITATIONS: [&str; 3] = [
    "Exact argv[0] basename matching identifies a reported executable family, not an authenticated binary, interactive agent, native session or activity. All Djinn modes (including help and maintenance) share executable names.",
    "Only one selected pane's foreground process group is sampled. Missing argv, Bun/Node launchers, renamed binaries, background and headless work remain unrecognized. Unknown does not establish absence.",
    "Matching before/after pane mappings are not atomic attribution: intervening mapping changes, process exit/replacement and PID reuse remain possible. Process info supplies no terminal identity or runtime generation.",
];

pub struct Request {
    pub socket: String,
    pub process_pane: Option<String>,
    pub timeout: f64,
    pub max_response_bytes: usize,
}

// The seam is private: production always uses the bounded native socket reader.
trait Transport {
    fn resolve(&self, path: &str) -> WireResult<String>;
    fn select(&self, path: &str) -> WireResult<observation_socket::Endpoint>;
    fn read(
        &self,
        path: &str,
        method: ReadMethod,
        params: &Value,
        deadline: Instant,
        budget: &mut ReadBudget,
        cancel: &dyn Cancel,
    ) -> WireResult<observation_socket::Reply>;
}
struct SocketTransport;
impl Transport for SocketTransport {
    fn resolve(&self, path: &str) -> WireResult<String> {
        observation_socket::resolve(path)
    }
    fn select(&self, path: &str) -> WireResult<observation_socket::Endpoint> {
        observation_socket::select(path)
    }
    fn read(
        &self,
        path: &str,
        method: ReadMethod,
        params: &Value,
        deadline: Instant,
        budget: &mut ReadBudget,
        cancel: &dyn Cancel,
    ) -> WireResult<observation_socket::Reply> {
        observation_socket::read(path, method, params, deadline, budget, cancel)
    }
}
fn endpoint_report(e: &observation_socket::Endpoint) -> Value {
    json!({"selected":e.selected,"resolved":e.resolved,"device":e.device,"inode":e.inode})
}
fn identity(e: &observation_socket::Endpoint) -> WireResult<Value> {
    e.identity().map_err(|e| Failure::new(e.code()))
}
fn guard(clock: &dyn Clock, deadline: Instant, cancel: &dyn Cancel) -> WireResult<()> {
    if cancel.interrupted() {
        return Err(Failure::new("interrupted"));
    }
    if clock.monotonic() >= deadline {
        return Err(Failure::new("timeout"));
    }
    Ok(())
}
fn read_pinned(
    transport: &dyn Transport,
    endpoint: &observation_socket::Endpoint,
    method: ReadMethod,
    params: &Value,
    deadline: Instant,
    budget: &mut ReadBudget,
    cancel: &dyn Cancel,
) -> WireResult<observation_socket::Reply> {
    // Reject already visible replacements before reading; the returned endpoint
    // must also agree. Stat fences cannot eliminate all path replacement races.
    if transport.select(&endpoint.selected)? != *endpoint {
        return Err(Failure::new("endpoint_replaced"));
    }
    let r = transport.read(&endpoint.selected, method, params, deadline, budget, cancel)?;
    if r.endpoint != *endpoint {
        return Err(Failure::new("endpoint_replaced"));
    }
    Ok(r)
}
fn pane(snapshot: &Value, id: &str) -> WireResult<Value> {
    snapshot["panes"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["pane_id"].as_str() == Some(id))
        .cloned()
        .ok_or_else(|| Failure::new("pane_not_found"))
}
fn evidence(
    request: &ProcessRequest<'_>,
    endpoint: &observation_socket::Endpoint,
    budget: &mut ReadBudget,
    cancel: &dyn Cancel,
    clock: &dyn Clock,
    transport: &dyn Transport,
) -> WireResult<Value> {
    let r = read_pinned(
        transport,
        endpoint,
        ReadMethod::Snapshot,
        &json!({}),
        request.deadline,
        budget,
        cancel,
    )?;
    let snapshot = schema::snapshot(&r, true)?;
    let before = pane(&snapshot, request.pane)?;
    if request
        .expected_terminal
        .is_some_and(|t| before["terminal_id"].as_str() != Some(t))
    {
        return Err(Failure::new("pane_mapping_changed"));
    }
    let r = read_pinned(
        transport,
        endpoint,
        ReadMethod::ProcessInfo,
        &json!({"pane_id":request.pane}),
        request.deadline,
        budget,
        cancel,
    )?;
    let matches = schema::processes(&r, request.pane)?;
    let stamp = clock.timestamp().map_err(|e| Failure::new(e.code()))?;
    let r = read_pinned(
        transport,
        endpoint,
        ReadMethod::Snapshot,
        &json!({}),
        request.deadline,
        budget,
        cancel,
    )?;
    let snapshot = schema::snapshot(&r, true)?;
    let after = pane(&snapshot, request.pane)?;
    if before != after {
        return Err(Failure::new("pane_mapping_changed"));
    }
    let mut locators = before;
    locators["endpoint"] = endpoint_report(endpoint);
    let out = json!({"provenance":"kitsune.pane.process_info","interpreter":PROCESS_CONTRACT,
        "locators":locators,"sample_received_at":stamp,"mapping_evidence":"matching_bracketing_samples",
        "recognition":if matches.is_empty(){"unknown"}else{"djinn_executable_family"},"matches":matches,
        "activity":"unknown","attention":"unknown","native_lifecycle":"unestablished","job_completion":"unestablished"});
    guard(clock, request.deadline, cancel)?;
    Ok(out)
}
fn agents(snapshot: &Value, endpoint: &Value) -> Vec<Value> {
    snapshot["agents"].as_array().into_iter().flatten().map(|a| {
        let status = a["agent_status"].as_str().unwrap_or("unknown");
        let mut locators = json!({"endpoint":endpoint});
        for k in schema::LOCATORS {locators[k] = a[k].clone();}
        let mut out = json!({"provenance":"kitsune.session.snapshot","locators":locators,
            "raw_status":status,"activity":if matches!(status,"done"|"idle"){"idle"}else{status},
            "attention":match status {"done"=>"unacknowledged","blocked"=>"needs_attention",_=>"unknown"},
            "native_lifecycle":"unestablished","job_completion":"unestablished"});
        for k in ["revision","focused","state_change_seq","launch_pending","interactive_ready","screen_detection_skipped"] {out[k]=a[k].clone();}
        for (k,target) in [("agent","agent_kind"),("name","name"),("agent_session","native_session")] {
            if !a[k].is_null() {out[target]=a[k].clone();}
        }
        out
    }).collect()
}

pub fn inspect(request: &Request) -> crate::Result<Value> {
    inspect_with(request, &SystemClock, &SocketTransport)
}
fn inspect_with(
    request: &Request,
    clock: &dyn Clock,
    transport: &dyn Transport,
) -> crate::Result<Value> {
    observation_socket::validate_path(&request.socket)?;
    crate::error::require(
        request.timeout.is_finite() && request.timeout > 0.0 && request.timeout <= 30.0,
        "invalid_timeout",
    )?;
    crate::error::require(
        (1024..=8388608).contains(&request.max_response_bytes),
        "invalid_response_budget",
    )?;
    if let Some(p) = &request.process_pane {
        crate::error::require(schema::opaque(p), "invalid_pane_id")?;
    }
    let started = clock.monotonic();
    let deadline = started + Duration::from_secs_f64(request.timeout);
    let mut limitations = LIMITATIONS.to_vec();
    if request.process_pane.is_some() {
        limitations.extend(PROCESS_LIMITATIONS);
    }
    let mut report = json!({"schema_version":1,"adapter":"kitsune",
        "adapter_contract":if request.process_pane.is_some(){PROCESS_CONTRACT}else{CONTRACT},
        "endpoint":{"selected":request.socket,"resolved":null,"device":null,"inode":null},
        "request":if let Some(p)=&request.process_pane {json!({"mode":"djinn_process","pane_id":p})}else{json!({"method":"session.snapshot"})},
        "started_at":clock.timestamp()?,"authorization":"none","dispatchable":false,
        "runtime_fact_freshness":"unknown","server_snapshot_timestamp":null,"limitations":limitations,
        "assessment":"unavailable","diagnostics":[]});
    if request.process_pane.is_some() {
        report["observations"] = json!([]);
    } else {
        report["agents"] = json!([]);
        report["pane_count"] = Value::Null;
    }
    let outcome = (|| -> WireResult<Value> {
        report["endpoint"]["resolved"] = json!(transport.resolve(&request.socket)?);
        let endpoint = transport.select(&request.socket)?;
        report["endpoint"] = endpoint_report(&endpoint);
        let mut budget = ReadBudget::new(request.max_response_bytes);
        if let Some(p) = &request.process_pane {
            evidence(
                &ProcessRequest {
                    socket: &request.socket,
                    pane: p,
                    deadline,
                    frozen_endpoint: None,
                    expected_terminal: None,
                },
                &endpoint,
                &mut budget,
                &NoCancel,
                clock,
                transport,
            )
        } else {
            let r = read_pinned(
                transport,
                &endpoint,
                ReadMethod::Snapshot,
                &json!({}),
                deadline,
                &mut budget,
                &NoCancel,
            )?;
            let s = schema::snapshot(&r, false)?;
            let out = json!({"agents":agents(&s,&endpoint_report(&endpoint)),"pane_count":s["panes"].as_array().map(Vec::len)});
            guard(clock, deadline, &NoCancel)?;
            Ok(out)
        }
    })();
    match outcome {
        Ok(v) => {
            report["assessment"] = json!("observed");
            if request.process_pane.is_some() {
                report["observations"] = json!([v]);
            } else {
                report["agents"] = v["agents"].clone();
                report["pane_count"] = v["pane_count"].clone();
            }
        }
        Err(e) => {
            report["assessment"] = json!(if e.code == "unsupported_protocol" {
                "incompatible"
            } else {
                "unavailable"
            });
            report["diagnostics"] = json!([e.code]);
        }
    }
    report["observed_at"] = json!(clock.timestamp()?);
    report["duration_ms"] = json!(
        (clock
            .monotonic()
            .saturating_duration_since(started)
            .as_secs_f64()
            * 1_000_000.0)
            .round()
            / 1000.0
    );
    Ok(report)
}

#[cfg(test)]
#[path = "kitsune_observation/tests.rs"]
mod tests;
