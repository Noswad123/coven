//! Bounded characterization of Djinn-family hints, never runtime authorization.
use crate::kitsune_observation::{comparison_snapshot, session_processes};
use crate::observation_socket::{
    self as wire, Cancel, Failure, ReadBudget, ReadMethod, WireResult,
};
use crate::observation_time::{Clock, SystemClock};
use crate::{Result, task_mutation::TaskOutput};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub struct Request {
    pub socket: String,
    pub timeout: f64,
    pub max_response_bytes: usize,
}

const LOCATORS: [&str; 4] = ["pane_id", "terminal_id", "workspace_id", "tab_id"];
const CATEGORIES: [&str; 7] = [
    "both_djinn",
    "generic_only",
    "builtin_only",
    "neither",
    "other_builtin_family",
    "conflicting_family_hints",
    "legacy_changed",
];
const LIMITATIONS: [&str; 7] = [
    "Djinn-only comparison: other and unknown built-in families are out of scope, not Coven regressions.",
    "Exact argv[0] basename recognition includes help, maintenance and spoofed executables; it does not establish an active agent or native identity. Missing hints do not establish absence.",
    "Kitsune done means idle and unseen, not a completed task, success or runtime shutdown.",
    "Matching bracketing snapshots are not atomic: intervening mapping changes, process replacement and PID reuse remain possible.",
    "Read-only requests may cause Kitsune metadata expiry, event drainage, title synchronization or default workspace creation. Zero server-side effects are not promised.",
    "Private socket ownership and endpoint device/inode checks are not peer authentication; trusted parent paths and replacement races remain limitations.",
    "No automatic following, adoption, rebinding, native action or built-in disabling is performed or authorized.",
];

trait Transport {
    fn resolve(&self, path: &str) -> WireResult<String>;
    fn select(&self, path: &str) -> WireResult<wire::Endpoint>;
    fn read(
        &self,
        path: &str,
        method: ReadMethod,
        params: &Value,
        deadline: Instant,
        budget: &mut ReadBudget,
        cancel: &dyn Cancel,
    ) -> WireResult<wire::Reply>;
}
struct SocketTransport;
impl Transport for SocketTransport {
    fn resolve(&self, path: &str) -> WireResult<String> {
        wire::resolve(path)
    }
    fn select(&self, path: &str) -> WireResult<wire::Endpoint> {
        wire::select(path)
    }
    fn read(
        &self,
        path: &str,
        method: ReadMethod,
        params: &Value,
        deadline: Instant,
        budget: &mut ReadBudget,
        cancel: &dyn Cancel,
    ) -> WireResult<wire::Reply> {
        wire::read(path, method, params, deadline, budget, cancel)
    }
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

fn cohort(snapshot: &Value) -> BTreeMap<String, Value> {
    snapshot["panes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            (
                p["pane_id"].as_str().unwrap().to_owned(),
                json!([
                    p["pane_id"],
                    p["terminal_id"],
                    p["workspace_id"],
                    p["tab_id"]
                ]),
            )
        })
        .collect()
}

fn agent<'a>(snapshot: &'a Value, pane: &Value) -> Option<&'a Value> {
    snapshot["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["pane_id"] == pane["pane_id"])
}

fn facts(pane: &Value, agent: Option<&Value>) -> Value {
    json!({"listed":agent.is_some(),
        "reported_family":match pane["agent"].as_str() {
            Some("djinn") => json!("djinn"), Some(_) => json!("other"), None => Value::Null },
        "status":pane["agent_status"],
        "native_reference_present":!pane["agent_session"].is_null()})
}

fn category(
    before: &Value,
    after: &Value,
    old_agent: Option<&Value>,
    new_agent: Option<&Value>,
    recognized: bool,
) -> &'static str {
    // Compare private full projections, including reference values and readiness flags.
    if before != after || old_agent != new_agent {
        return "legacy_changed";
    }
    match before["agent"].as_str() {
        Some("djinn") if old_agent.is_some() => {
            if recognized {
                "both_djinn"
            } else {
                "builtin_only"
            }
        }
        Some(family) if family != "djinn" => {
            if recognized {
                "conflicting_family_hints"
            } else {
                "other_builtin_family"
            }
        }
        _ => {
            if recognized {
                "generic_only"
            } else {
                "neither"
            }
        }
    }
}

pub fn inspect(request: &Request) -> Result<Value> {
    inspect_with(request, &SystemClock, &SocketTransport, &wire::NoCancel)
}

fn inspect_with(
    request: &Request,
    clock: &dyn Clock,
    transport: &dyn Transport,
    cancel: &dyn Cancel,
) -> Result<Value> {
    wire::validate_path(&request.socket)?;
    crate::error::require(
        request.timeout.is_finite() && request.timeout > 0.0 && request.timeout <= 30.0,
        "invalid_timeout",
    )?;
    crate::error::require(
        (1024..=8388608).contains(&request.max_response_bytes),
        "invalid_response_budget",
    )?;
    let started = clock.monotonic();
    let deadline = started + Duration::from_secs_f64(request.timeout);
    let mut report = json!({"schema_version":1,"adapter":"kitsune",
        "adapter_contract":"coven.kitsune.agent-comparison.v1",
        "scope":"same_endpoint_pane_family_evidence",
        "endpoint":{"selected":request.socket,"resolved":null,"device":null,"inode":null},
        "started_at":clock.timestamp()?,"authorization":"none","dispatchable":false,
        "native_lifecycle":"unestablished","native_identity":"unestablished",
        "readiness":"unestablished","worker_acceptance":"unestablished","job_completion":"unestablished",
        "replacement_equivalence":"not_established","runtime_fact_freshness":"unknown",
        "timestamp_basis":"local_receipt","limitations":LIMITATIONS,
        "assessment":"unavailable","diagnostics":[],"target_count":null,"rows":[],"counts":null});
    let outcome = (|| -> WireResult<Value> {
        guard(clock, deadline, cancel)?;
        report["endpoint"]["resolved"] = json!(transport.resolve(&request.socket)?);
        let endpoint = transport.select(&request.socket)?;
        report["endpoint"] = json!({"selected":endpoint.selected,"resolved":endpoint.resolved,
            "device":endpoint.device,"inode":endpoint.inode});
        let mut budget = ReadBudget::new(request.max_response_bytes);
        let mut read = |method, params: &Value| -> WireResult<wire::Reply> {
            guard(clock, deadline, cancel)?;
            if transport.select(&endpoint.selected)? != endpoint {
                return Err(Failure::new("endpoint_replaced"));
            }
            let reply = transport.read(
                &endpoint.selected,
                method,
                params,
                deadline,
                &mut budget,
                cancel,
            )?;
            if reply.endpoint != endpoint || transport.select(&endpoint.selected)? != endpoint {
                return Err(Failure::new("endpoint_replaced"));
            }
            guard(clock, deadline, cancel)?;
            Ok(reply)
        };
        let before = comparison_snapshot(&read(ReadMethod::Snapshot, &json!({}))?)?;
        let panes = before["panes"].as_array().unwrap();
        if panes.len() > 64 {
            return Err(Failure::new("pane_limit"));
        }
        let mut evidence = Vec::new();
        for pane in panes {
            let id = pane["pane_id"].as_str().unwrap();
            let mut matches =
                session_processes(&read(ReadMethod::ProcessInfo, &json!({"pane_id":id}))?, id)?;
            matches.sort_by(|a, b| {
                (
                    a["pid"].as_u64(),
                    a["executable_family"].as_str(),
                    a["rule"].as_str(),
                )
                    .cmp(&(
                        b["pid"].as_u64(),
                        b["executable_family"].as_str(),
                        b["rule"].as_str(),
                    ))
            });
            evidence.push(matches);
        }
        let after = comparison_snapshot(&read(ReadMethod::Snapshot, &json!({}))?)?;
        if before["version"] != after["version"] {
            return Err(Failure::new("version_changed"));
        }
        if cohort(&before) != cohort(&after) {
            return Err(Failure::new("pane_mapping_changed"));
        }
        let after_panes: BTreeMap<_, _> = after["panes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p["pane_id"].as_str().unwrap(), p))
            .collect();
        let mut rows = Vec::new();
        let mut counts = json!({});
        for key in CATEGORIES {
            counts[key] = json!(0);
        }
        for (pane, matches) in panes.iter().zip(evidence) {
            let later = after_panes[pane["pane_id"].as_str().unwrap()];
            let old_agent = agent(&before, pane);
            let new_agent = agent(&after, later);
            let comparison = category(pane, later, old_agent, new_agent, !matches.is_empty());
            let mut locators = json!({});
            for key in LOCATORS {
                locators[key] = pane[key].clone();
            }
            counts[comparison] = json!(counts[comparison].as_u64().unwrap() + 1);
            rows.push(json!({"locators":locators,
                "kitsune":{"before":facts(pane,old_agent),"after":facts(later,new_agent)},
                "coven":{"recognition":if matches.is_empty(){"unknown"}else{"djinn_executable_family"},"matches":matches},
                "family_comparison":comparison}));
        }
        let out = json!({"rows":rows,"counts":counts,"target_count":panes.len()});
        guard(clock, deadline, cancel)?;
        Ok(out)
    })();
    // Receipt metadata and projection are inside the same deadline as collection.
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
    let outcome = outcome.and_then(|out| {
        for key in ["rows", "counts", "target_count"] {
            report[key] = out[key].clone();
        }
        guard(clock, deadline, cancel)
    });
    match outcome {
        Ok(()) => {
            report["assessment"] = json!("observed");
        }
        Err(error) => {
            report["rows"] = json!([]);
            report["counts"] = Value::Null;
            report["target_count"] = Value::Null;
            report["assessment"] = json!(if error.code == "unsupported_protocol" {
                "incompatible"
            } else {
                "unavailable"
            });
            report["diagnostics"] = json!([error.code]);
        }
    }
    Ok(report)
}

pub fn arguments(request: &Request, json: bool) -> Result<TaskOutput> {
    let cancel = wire::InterruptGuard::install()?;
    let report = inspect_with(request, &SystemClock, &SocketTransport, &cancel)?;
    let mut out = crate::session_support::output(&report, !json)?;
    out.exit_code = if cancel.interrupted()
        || report["diagnostics"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == "interrupted"))
    {
        130
    } else {
        match report["assessment"].as_str() {
            Some("observed") => 0,
            Some("incompatible") => 2,
            _ => 1,
        }
    };
    Ok(out)
}

#[cfg(test)]
#[path = "kitsune_comparison/tests.rs"]
mod tests;
