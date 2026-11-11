//! Ephemeral, unclassified foreground-cohort observation, never execution authority.
mod schema;

use crate::observation_socket::{self, Cancel, Failure, ReadBudget, ReadMethod, WireResult};
use crate::{Result, task_mutation::TaskOutput};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

pub const CONTRACT: &str = "coven.agent-discovery.v1";

#[derive(Clone)]
pub struct Request {
    pub socket: String,
    pub pane: Option<String>,
    pub expected_terminal: Option<String>,
    pub timeout: f64,
    pub max_response_bytes: usize,
}

pub(crate) fn validate(request: &Request) -> Result<()> {
    observation_socket::validate_path(&request.socket)?;
    crate::error::require(
        request.timeout.is_finite() && request.timeout > 0.0 && request.timeout <= 30.0,
        "invalid_timeout",
    )?;
    crate::error::require(
        (1024..=8 * 1024 * 1024).contains(&request.max_response_bytes),
        "invalid_response_budget",
    )?;
    for (value, code) in [
        (&request.pane, "invalid_pane_id"),
        (&request.expected_terminal, "invalid_terminal_id"),
    ] {
        if let Some(value) = value {
            crate::error::require(crate::kitsune_observation::session_opaque(value), code)?;
        }
    }
    crate::error::require(
        request.expected_terminal.is_none() || request.pane.is_some(),
        "expected_terminal_requires_pane",
    )
}

pub fn inspect(request: &Request) -> Result<Value> {
    validate(request)?;
    let cancel = observation_socket::InterruptGuard::install()?;
    inspect_cancel(request, &cancel)
}

pub(crate) fn inspect_cancel(request: &Request, cancel: &dyn Cancel) -> Result<Value> {
    validate(request)?;
    let started = Instant::now();
    let deadline = started + Duration::from_secs_f64(request.timeout);
    let scan_id = crate::platform::random_uuid_hex()?;
    let mut report = json!({
        "schema_version":1,"contract":CONTRACT,"scan_id":scan_id,
        "assessment":"unavailable","diagnostics":[],"targets":[],
        "target_count":null,"recognized_target_count":null,
        "endpoint":{"selected":request.socket,"resolved":null,"device":null,"inode":null},
        "process_observation":null,"authorization":"none","dispatchable":false,
        "native_catalog_complete":false,"activity":"unknown","attention":"unknown",
        "native_identity":"unestablished","native_lifecycle":"unestablished","job_completion":"unestablished",
        "limitations":[
            "Read-only requests are not zero-effect server operations: Kit pipeline housekeeping may expire metadata, drain events, synchronize titles, and ensure a default workspace or shell.",
            "Reported Linux foreground-cohort capability only. Unix transport on macOS is unvalidated; Windows acquisition is unsupported. Endpoint fences are not authentication and cannot eliminate pathname races.",
            "Matching inventory brackets are not atomic attribution or freshness attestation. Processes may exit, PIDs may be reused, and mappings may change between reads. Unknown does not establish absence.",
            "Provider-family recognition is not native identity, activity, lifecycle, job completion, authorization, or a complete native catalog. No persistence, adoption, roles, or terminal associations are created."
        ]
    });
    let outcome = (|| -> WireResult<Vec<Value>> {
        observation_socket::check(deadline, cancel)?;
        if cfg!(windows) {
            return Err(Failure::new("unsupported_acquisition"));
        }
        report["endpoint"]["resolved"] = json!(observation_socket::resolve(&request.socket)?);
        let endpoint = observation_socket::select(&request.socket)?;
        report["endpoint"] = json!({"selected":endpoint.selected,"resolved":endpoint.resolved,
            "device":endpoint.device,"inode":endpoint.inode});
        let mut budget = ReadBudget::new(request.max_response_bytes);
        let mut read = |method, params: &Value| {
            observation_socket::check(deadline, cancel)?;
            if observation_socket::select(&request.socket)? != endpoint {
                return Err(Failure::new("endpoint_replaced"));
            }
            let reply = observation_socket::read(
                &request.socket,
                method,
                params,
                deadline,
                &mut budget,
                cancel,
            )?;
            if reply.endpoint != endpoint
                || observation_socket::select(&request.socket)? != endpoint
            {
                return Err(Failure::new("endpoint_replaced"));
            }
            Ok(reply)
        };
        let before = schema::snapshot(&read(ReadMethod::Snapshot, &json!({}))?)?;
        report["process_observation"] = before.descriptor.clone();
        let selected = before.selected(request.pane.as_deref())?;
        if request
            .expected_terminal
            .as_deref()
            .is_some_and(|expected| {
                selected.first().and_then(|p| p["terminal_id"].as_str()) != Some(expected)
            })
        {
            return Err(Failure::new("pane_mapping_changed"));
        }
        let mut targets = Vec::new();
        for locators in &selected {
            let pane = locators["pane_id"]
                .as_str()
                .ok_or_else(|| Failure::new("schema_error"))?;
            let facts = schema::processes(
                &read(ReadMethod::ProcessInfo, &json!({"pane_id":pane}))?,
                pane,
            )?;
            let sample_received_at =
                crate::task_time::now().map_err(|_| Failure::new("io_error"))?;
            let matches: Vec<_> = facts
                .iter()
                .flat_map(crate::agent_processes::classify)
                .collect();
            let mut providers: Vec<_> = matches.iter().map(|m| m.provider).collect();
            providers.sort_unstable();
            providers.dedup();
            let recognition = match providers.len() {
                0 => "unknown",
                1 => "provider_family",
                _ => "multiple_provider_families",
            };
            // JSON tuple encoding avoids ambiguous concatenation of opaque locators.
            let seed = serde_json::to_vec(&json!([scan_id, locators["terminal_id"]]))
                .map_err(|_| Failure::new("io_error"))?;
            let observation_id = format!("{:x}", Sha256::digest(seed));
            let mut target = locators.clone();
            target["observation_id"] = json!(observation_id);
            target["recognition"] = json!(recognition);
            target["matches"] =
                serde_json::to_value(matches).map_err(|_| Failure::new("io_error"))?;
            target["provider_candidates"] = json!(providers);
            target["sample_received_at"] = json!(sample_received_at);
            target["mapping_evidence"] = json!("matching_inventory_brackets");
            target["activity"] = json!("unknown");
            target["attention"] = json!("unknown");
            for key in ["native_identity", "native_lifecycle", "job_completion"] {
                target[key] = json!("unestablished");
            }
            targets.push(target);
        }
        let after = schema::snapshot(&read(ReadMethod::Snapshot, &json!({}))?)?;
        if before.version != after.version || before.protocol != after.protocol {
            return Err(Failure::new("server_changed"));
        }
        if before.descriptor != after.descriptor {
            return Err(Failure::new("process_observation_changed"));
        }
        if selected != after.selected(request.pane.as_deref())? {
            return Err(Failure::new("pane_mapping_changed"));
        }
        observation_socket::check(deadline, cancel)?;
        Ok(targets)
    })();
    match outcome {
        Ok(targets) => {
            report["assessment"] = json!("observed");
            report["target_count"] = json!(targets.len());
            report["recognized_target_count"] = json!(
                targets
                    .iter()
                    .filter(|t| t["recognition"] != "unknown")
                    .count()
            );
            report["targets"] = json!(targets);
        }
        Err(failure) => {
            report["assessment"] = json!(if matches!(
                failure.code.as_str(),
                "unsupported_protocol"
                    | "process_observation_unsupported"
                    | "unsupported_acquisition"
            ) {
                "incompatible"
            } else {
                "unavailable"
            });
            report["diagnostics"] = json!([failure.code]);
        }
    }
    report["duration_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
    Ok(report)
}

pub fn arguments(request: &Request, json_output: bool) -> Result<TaskOutput> {
    let report = inspect(request)?;
    let exit_code = if report["diagnostics"]
        .as_array()
        .is_some_and(|d| d.iter().any(|v| v == "interrupted"))
    {
        130
    } else {
        match report["assessment"].as_str() {
            Some("observed") => 0,
            Some("incompatible") => 2,
            _ => 1,
        }
    };
    let stdout = if json_output {
        crate::output::ascii_json(&report.to_string())?
    } else {
        human(&report)
    };
    Ok(TaskOutput {
        stdout,
        warnings: Vec::new(),
        exit_code,
    })
}

fn human(report: &Value) -> String {
    let mut lines = vec![format!(
        "Agent discovery: {} — activity and attention unknown",
        report["assessment"].as_str().unwrap_or("unavailable")
    )];
    for target in report["targets"].as_array().into_iter().flatten() {
        // Escape opaque peer locators, not just C0 controls: no terminal escape
        // sequences or directional formatting from native strings are emitted.
        let pane = target["pane_id"].as_str().unwrap_or("").escape_default();
        let terminal = target["terminal_id"]
            .as_str()
            .unwrap_or("")
            .escape_default();
        let matches = target["matches"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut hints: Vec<_> = matches
            .iter()
            .take(8)
            .map(|m| {
                format!(
                    "{} pid {} ({})",
                    m["provider"].as_str().unwrap_or("unknown"),
                    m["pid"],
                    m["rule"].as_str().unwrap_or("unknown")
                )
            })
            .collect();
        if matches.len() > 8 {
            hints.push(format!("+{} more matches; use --json", matches.len() - 8));
        }
        if hints.is_empty() {
            hints.push("unrecognized evidence (not absence)".into());
        }
        lines.push(format!(
            "Pane {pane} / terminal {terminal}: {}",
            hints.join(", ")
        ));
    }
    lines.push(format!(
        "Targets: {} (recognized: {})",
        report["target_count"], report["recognized_target_count"]
    ));
    lines.push(format!("Diagnostics: {}", report["diagnostics"]));
    lines.join("\n")
}

#[cfg(test)]
mod tests;
