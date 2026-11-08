//! Read-only existing-pane inventory and explicit historical declaration joins.
use crate::observation_socket::{
    self as wire, Cancel, Endpoint, Failure, ReadBudget, ReadMethod, WireResult,
};
use crate::observation_time::{Clock, SystemClock};
use crate::task_mutation::TaskOutput;
use crate::values::{Number, Value as Native};
use crate::{Result, error::require};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Request {
    pub workspace: PathBuf,
    pub socket: String,
    pub pane: Option<String>,
    pub expected_terminal: Option<String>,
    pub runtime: String,
    pub timeout: f64,
    pub max_response_bytes: usize,
    pub group: Option<String>,
    pub member: Option<String>,
    pub recognized_only: bool,
}

const LIMITATIONS: [&str; 14] = [
    "Observation only: no execution authorization or UI return channel. Kitsune's request pipeline performs metadata expiry, event drainage and title synchronization, and may ensure a default workspace (potentially spawning a shell) in an empty existing session. Zero server-side effects are not promised.",
    "Socket ownership, permissions and connection are not peer authentication. Parent path aliases are resolved but parent directories are trusted; stat checks cannot eliminate path replacement races. Device/inode identify the observed endpoint, not a globally stable server identity.",
    "Snapshot receipt is not runtime freshness attestation. Native references may be persisted; pane presence, done status and empty inventories do not establish native lifecycle, runtime shutdown, job completion or success.",
    "Exact argv[0] basename matching identifies a reported executable family, not an authenticated binary, interactive agent, native session or activity. All Djinn modes (including help and maintenance) share executable names.",
    "Only one selected pane's foreground process group is sampled. Missing argv, Bun/Node launchers, renamed binaries, background and headless work remain unrecognized. Unknown does not establish absence.",
    "Matching before/after pane mappings are not atomic attribution: intervening mapping changes, process exit/replacement and PID reuse remain possible. Process info supplies no terminal identity or runtime generation.",
    "Scope is existing panes on one selected Kitsune endpoint, not a complete native session/worker catalog.",
    "Executable-family matches are process hints, not authenticated binaries, interactive mode or native session identity.",
    "Matching inventory brackets are not atomic attribution; PID reuse, ABA and post-read changes remain possible.",
    "Activity/attention are not assessed. Unknown is not idle, stopped, absent, ready, successful or complete.",
    "Group/role/native references are explicit historical declarations, not permissions or verified native identity.",
    "Live attachments are not opened; use the separate group status contract for scoped busy/unknown evidence.",
    "Reports are uncached observations, not authorization for subsequent input, launch, resume or stop.",
    "Deadline is an acceptance budget, not a filesystem syscall interrupt or hard native freshness bound.",
];

pub(crate) fn validate(request: &Request) -> Result<()> {
    wire::validate_path(&request.socket)?;
    require(
        request.timeout.is_finite() && request.timeout > 0.0 && request.timeout <= 30.0,
        "invalid_timeout",
    )?;
    require(
        (1024..=8388608).contains(&request.max_response_bytes),
        "invalid_response_budget",
    )?;
    require(request.runtime == "djinn", "unsupported_runtime")?;
    for pane in [&request.pane, &request.expected_terminal]
        .into_iter()
        .flatten()
    {
        require(
            crate::kitsune_observation::session_opaque(pane),
            "invalid_pane_id",
        )?;
    }
    require(
        request.expected_terminal.is_none() || request.pane.is_some(),
        "expected_terminal_requires_pane",
    )?;
    crate::session_views::validate_selection(request.group.as_deref(), request.member.as_deref())
}

fn field<'a>(value: &'a Native, key: &str) -> &'a Native {
    value
        .as_object()
        .and_then(|o| o.get(key))
        .unwrap_or(&Native::Null)
}
fn projection(value: &Native) -> WireResult<Value> {
    crate::observation_wire::to_json(value).map_err(|_| Failure::new("groups_unavailable"))
}
fn schema(view: &crate::groups::View) -> WireResult<Value> {
    projection(
        view.state()
            .map(|s| field(s, "schema_version"))
            .unwrap_or(&Native::Null),
    )
}
fn integer_matches(value: &Native, expected: u64) -> bool {
    matches!(value, Native::Number(Number::Integer(n)) if n.as_str() == expected.to_string())
}
fn mapping(snapshot: &Value) -> BTreeMap<&str, &Value> {
    snapshot["panes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p["pane_id"].as_str()?, p)))
        .collect()
}

fn join(
    view: &crate::groups::View,
    endpoint: &Endpoint,
    panes: &BTreeMap<&str, &Value>,
    targets: &mut [Value],
) -> WireResult<Value> {
    let mut unmatched = Vec::new();
    let (mut elsewhere, mut outside) = (0, 0);
    for group in view
        .state()
        .map(|s| field(s, "groups"))
        .and_then(Native::as_array)
        .into_iter()
        .flatten()
    {
        for member in field(group, "members").as_array().into_iter().flatten() {
            let binding = field(member, "binding");
            let p = field(binding, "presentation");
            let evidence = field(binding, "evidence");
            let declared = field(p, "endpoint");
            if field(declared, "path").as_str() != Some(&endpoint.resolved) {
                elsewhere += 1;
                continue;
            }
            let mut association = json!({"association_basis":"explicit_declaration",
                "native_identity":"unestablished", "live_attachment_declared":!matches!(field(member,"live_attachment"), Native::Null)});
            for (source, key, dest) in [
                (group, "group_id", "group_id"),
                (group, "key", "group_key"),
                (group, "name", "group_name"),
                (member, "participant_id", "participant_id"),
                (member, "key", "member_key"),
                (member, "role", "role"),
                (evidence, "provenance", "binding_provenance"),
                (evidence, "native_reference", "declared_native_reference"),
                (p, "pane_id_at_binding", "declared_pane_id"),
                (p, "terminal_id", "declared_terminal_id"),
            ] {
                association[dest] = projection(field(source, key))?;
            }
            // Authority endpoints are arbitrary-precision integers. Compare natively;
            // never serialize the entire authority through serde's bounded integers.
            let reason = if !integer_matches(field(declared, "device"), endpoint.device)
                || !integer_matches(field(declared, "inode"), endpoint.inode)
            {
                Some("endpoint_identity_changed")
            } else if !panes
                .values()
                .any(|pane| pane["terminal_id"].as_str() == field(p, "terminal_id").as_str())
            {
                Some("terminal_not_observed")
            } else {
                None
            };
            if let Some(reason) = reason {
                association["binding_check"] = json!(reason);
                unmatched.push(association);
                continue;
            }
            let current = panes
                .values()
                .find(|pane| pane["terminal_id"].as_str() == field(p, "terminal_id").as_str())
                .unwrap();
            let Some(target) = targets
                .iter_mut()
                .find(|t| t["pane_id"] == current["pane_id"])
            else {
                outside += 1;
                continue;
            };
            association["binding_check"] = json!(if field(evidence, "provenance").as_str()
                == Some("kitsune.pane.process_info")
            {
                if projection(field(field(evidence, "process_evidence"), "matches"))?
                    == target["matches"]
                {
                    "matching_process_evidence"
                } else {
                    "process_evidence_changed"
                }
            } else {
                "legacy_evidence_not_revalidated"
            });
            target["associations"]
                .as_array_mut()
                .unwrap()
                .push(association);
        }
    }
    Ok(
        json!({"assessment":if view.state().is_some(){"observed"}else{"uninitialized"},
        "revision":view.revision(),"authority_schema_version":schema(view)?,"unmatched":unmatched,
        "other_endpoint_count":elsewhere,"outside_inspection_count":outside}),
    )
}

fn read(
    endpoint: &Endpoint,
    method: ReadMethod,
    params: &Value,
    deadline: Instant,
    budget: &mut ReadBudget,
    cancel: &dyn Cancel,
) -> WireResult<wire::Reply> {
    if wire::select(&endpoint.selected)? != *endpoint {
        return Err(Failure::new("endpoint_replaced"));
    }
    let reply = wire::read(&endpoint.selected, method, params, deadline, budget, cancel)?;
    if reply.endpoint != *endpoint {
        return Err(Failure::new("endpoint_replaced"));
    }
    Ok(reply)
}

/// Collect one fresh report. Invalid options are errors; observation failures are reports.
pub fn inspect(request: &Request) -> Result<Value> {
    validate(request)?;
    let guard = wire::InterruptGuard::install()?;
    inspect_cancel(request, &guard)
}

pub(crate) fn inspect_cancel(request: &Request, cancel: &dyn Cancel) -> Result<Value> {
    validate(request)?;
    let started = Instant::now();
    let deadline = started + Duration::from_secs_f64(request.timeout);
    let workspace = crate::session_support::absolute(&request.workspace)?;
    let workspace_text = workspace
        .to_str()
        .ok_or(crate::Error("non_unicode_path_unsupported"))?;
    let mut report = json!({"schema_version":1,"contract":"coven.session-inspection.v1",
        "scope":"existing_panes_on_selected_kitsune_endpoint","runtime":request.runtime,
        "workspace":workspace_text,
        "request":{"mode":if request.pane.is_some(){"inspect"}else{"list"},"pane_id":request.pane,"expected_terminal_id":request.expected_terminal},
        "endpoint":{"selected":request.socket,"resolved":null,"device":null,"inode":null},
        "started_at":SystemClock.timestamp()?,"assessment":"unavailable","diagnostics":[],"targets":[],
        "target_count":null,"recognized_target_count":null,
        "declarations":{"assessment":"unavailable","revision":null,"authority_schema_version":null,"unmatched":[],"other_endpoint_count":null,"outside_inspection_count":null},
        "authorization":"none","dispatchable":false,"native_catalog_complete":false,
        "native_identity":"unestablished","native_lifecycle":"unestablished","native_worker_acceptance":"unestablished","job_completion":"unestablished",
        "runtime_fact_freshness":"unknown","server_snapshot_timestamp":null,"limitations":LIMITATIONS});
    let outcome = (|| -> WireResult<(Vec<Value>, Value, Value)> {
        let before_groups = crate::groups::read_groups(&workspace)
            .map_err(|_| Failure::new("groups_unavailable"))?;
        wire::check(deadline, cancel)?;
        let _ = wire::resolve(&request.socket)?;
        let endpoint = wire::select(&request.socket)?;
        report["endpoint"] = json!({"selected":endpoint.selected,"resolved":endpoint.resolved,"device":endpoint.device,"inode":endpoint.inode});
        let mut budget = ReadBudget::new(request.max_response_bytes);
        let before = crate::kitsune_observation::session_snapshot(&read(
            &endpoint,
            ReadMethod::Snapshot,
            &json!({}),
            deadline,
            &mut budget,
            cancel,
        )?)?;
        let before_map = mapping(&before);
        let selected = if let Some(id) = &request.pane {
            let pane = before_map
                .get(id.as_str())
                .ok_or_else(|| Failure::new("pane_not_found"))?;
            if request
                .expected_terminal
                .as_deref()
                .is_some_and(|t| pane["terminal_id"].as_str() != Some(t))
            {
                return Err(Failure::new("terminal_mismatch"));
            }
            vec![*pane]
        } else {
            if before_map.len() > 64 {
                return Err(Failure::new("pane_limit"));
            }
            before["panes"].as_array().unwrap().iter().collect()
        };
        let mut targets = Vec::new();
        for pane in selected {
            let id = pane["pane_id"].as_str().unwrap();
            let reply = read(
                &endpoint,
                ReadMethod::ProcessInfo,
                &json!({"pane_id":id}),
                deadline,
                &mut budget,
                cancel,
            )?;
            let mut matches = crate::kitsune_observation::session_processes(&reply, id)?;
            matches.sort_by(|a, b| {
                a["pid"]
                    .as_u64()
                    .cmp(&b["pid"].as_u64())
                    .then_with(|| {
                        a["executable_family"]
                            .as_str()
                            .cmp(&b["executable_family"].as_str())
                    })
                    .then_with(|| a["rule"].as_str().cmp(&b["rule"].as_str()))
            });
            let mut target = pane.clone();
            for (k,v) in json!({"provenance":"kitsune.pane.process_info","interpreter":"coven.kitsune.djinn-process.v1",
                "sample_received_at":SystemClock.timestamp().map_err(|_| Failure::new("io_error"))?,
                "mapping_evidence":"matching_inventory_brackets","recognition":if matches.is_empty(){"unknown"}else{"djinn_executable_family"},
                "matches":matches,"activity":"unknown","attention":"unknown","native_identity":"unestablished",
                "native_lifecycle":"unestablished","job_completion":"unestablished","associations":[]}).as_object().unwrap() {
                target[k] = v.clone();
            }
            targets.push(target);
        }
        let after = crate::kitsune_observation::session_snapshot(&read(
            &endpoint,
            ReadMethod::Snapshot,
            &json!({}),
            deadline,
            &mut budget,
            cancel,
        )?)?;
        if before["version"] != after["version"] || before["protocol"] != after["protocol"] {
            return Err(Failure::new("server_context_changed"));
        }
        let after_map = mapping(&after);
        if if let Some(id) = &request.pane {
            before_map.get(id.as_str()) != after_map.get(id.as_str())
        } else {
            before_map != after_map
        } {
            return Err(Failure::new("pane_mapping_changed"));
        }
        let after_groups = crate::groups::read_groups(&workspace)
            .map_err(|_| Failure::new("groups_unavailable"))?;
        if before_groups.revision() != after_groups.revision()
            || schema(&before_groups)? != schema(&after_groups)?
        {
            return Err(Failure::new("groups_changed"));
        }
        if wire::select(&request.socket)? != endpoint {
            return Err(Failure::new("endpoint_replaced"));
        }
        let declarations = join(&before_groups, &endpoint, &after_map, &mut targets)?;
        wire::check(deadline, cancel)?;
        Ok((targets, declarations, before))
    })();
    match outcome {
        Ok((targets, declarations, snapshot)) => {
            report["assessment"] = json!("observed");
            report["target_count"] = json!(targets.len());
            report["recognized_target_count"] = json!(
                targets
                    .iter()
                    .filter(|t| t["recognition"] == "djinn_executable_family")
                    .count()
            );
            report["targets"] = json!(targets);
            report["declarations"] = declarations;
            report["server_version"] = snapshot["version"].clone();
            report["protocol"] = snapshot["protocol"].clone();
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
    report["observed_at"] = json!(SystemClock.timestamp()?);
    report["duration_ms"] = json!((started.elapsed().as_secs_f64() * 1_000_000.0).round() / 1000.0);
    crate::session_views::select_report(
        &report,
        request.group.as_deref(),
        request.member.as_deref(),
        request.recognized_only,
    )
}

pub fn exit_code(report: &Value) -> u8 {
    if report["diagnostics"]
        .as_array()
        .is_some_and(|a| a.iter().any(|v| v == "interrupted"))
    {
        130
    } else if report["assessment"] == "observed" {
        0
    } else if report["assessment"] == "incompatible" {
        2
    } else {
        1
    }
}

pub fn arguments(request: &Request, json: bool, pretty: bool) -> Result<TaskOutput> {
    render(&inspect(request)?, json, pretty)
}

pub(crate) fn render(report: &Value, json_output: bool, pretty: bool) -> Result<TaskOutput> {
    let mut out = if json_output || pretty {
        crate::session_support::output(report, pretty && !json_output)?
    } else {
        TaskOutput {
            stdout: human(report),
            warnings: vec![],
            exit_code: 0,
        }
    };
    out.exit_code = exit_code(report);
    Ok(out)
}

fn visible(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            if crate::unicode::printable(c) {
                c.to_string().chars().collect::<Vec<_>>()
            } else {
                c.escape_default().collect()
            }
        })
        .collect()
}
fn display(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        s.into()
    } else if value.is_null() {
        "None".into()
    } else if value == true {
        "True".into()
    } else if value == false {
        "False".into()
    } else {
        value.to_string()
    }
}
pub(crate) fn human(report: &Value) -> String {
    let mut lines = vec![format!("Session target inspection: {}",display(&report["assessment"])),
        "Scope: existing panes on the selected endpoint; native activity/readiness remains unknown.".into()];
    if report.get("view").is_some() {
        let v = &report["view"];
        lines.push(visible(&format!(
            "View: group={} member={} recognized_only={}; selected {} of {} observed pane targets.",
            display(&v["group_key"]),
            display(&v["member_key"]),
            display(&v["recognized_only"]),
            display(&report["target_count"]),
            display(&v["observed_target_count"])
        )));
        lines.push("Declaration aggregate counts remain scoped to the full observation.".into());
    }
    for t in report["targets"].as_array().into_iter().flatten() {
        let associations = t["associations"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| {
                format!(
                    "{}/{}: {} [{}]",
                    display(&a["group_key"]),
                    display(&a["member_key"]),
                    display(&a["role"]),
                    display(&a["binding_check"])
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        lines.push(visible(&format!(
            "{}  {}  {}  {}",
            display(&t["pane_id"]),
            display(&t["terminal_id"]),
            display(&t["recognition"]),
            if associations.is_empty() {
                "no matching declaration"
            } else {
                &associations
            }
        )));
    }
    if report["assessment"] == "observed" {
        lines.push(format!(
            "Unmatched declarations: {}",
            report["declarations"]["unmatched"]
                .as_array()
                .map_or(0, Vec::len)
        ));
    }
    for d in report["diagnostics"].as_array().into_iter().flatten() {
        lines.push(format!("Diagnostic: {}", display(d)));
    }
    lines.join("\n")
}

#[cfg(test)]
#[path = "session_inspection/tests.rs"]
mod tests;
