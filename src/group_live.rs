//! Explicit source declarations and ephemeral, whole-content-fenced status.
use crate::{
    Error, Result,
    djinn_live_observation::{self as live, Request, schema},
    groups,
    observation_time::{Clock, SystemClock},
    values::Value as AuthorityValue,
};
use serde_json::{Value, json};
use std::{path::Path, time::Instant};

fn authority_field<'a>(value: &'a AuthorityValue, name: &str) -> Result<&'a AuthorityValue> {
    value
        .as_object()
        .and_then(|o| o.get(name))
        .ok_or(Error("invalid_groups"))
}

fn project_authority(value: &AuthorityValue, names: &str) -> Result<Value> {
    // Endpoint metadata is arbitrary precision. Select in the native domain so
    // unused bindings/counters never pass through the bounded report codec.
    let object = value.as_object().ok_or(Error("invalid_groups"))?;
    let projected = AuthorityValue::object(
        names
            .split_whitespace()
            .map(|name| {
                let value = match object.get(name) {
                    Some(value) => value.clone(),
                    None if name == "live_attachment" => AuthorityValue::Null,
                    None => return Err(Error("invalid_groups")),
                };
                Ok((name.to_owned(), value))
            })
            .collect::<Result<_>>()?,
    )?;
    crate::observation_wire::to_json(&projected)
}

pub const STATUS_CONTRACT: &str = "coven.group.status.v1";
const LIMITATIONS: [&str; 5] = [
    "Live attachments are explicit participant declarations, not verified native identity, current-pane association, or terminal/session equivalence.",
    "Busy evidence covers only freshly sampled local legacy runner ownership. Unknown is not idle, stopped, failure, completion, readiness or execution permission.",
    "Sequential source observations are not an atomic native snapshot. Group content is rechecked, but can change after that check. Shared source pins do not prove distinct workers and must not be counted as distinct native executions.",
    "Remaining lifetime is conservatively reduced after all reads and must continue decreasing during delivery/display. Same-host wall clock and local-filesystem assumptions, replay/rollback and same-UID pathname limitations still apply.",
    "Status takes no locks, writes no state, launches no work and publishes no labels. The overall timeout is an acceptance deadline, not a filesystem syscall interrupt.",
];

pub struct AttachRequest<'a> {
    pub group: &'a str,
    pub member: &'a str,
    pub file: &'a str,
    pub runtime: &'a str,
    pub instance: &'a str,
    pub session: &'a str,
    pub reattach: bool,
    pub expected_revision: Option<&'a str>,
    pub timeout: f64,
}

/// Validate the in-process adapter boundary before projection or publication.
pub(crate) fn accepted(report: &Value, attachment: &Value) -> Result<Value> {
    schema::require(
        report.is_object()
            && report["schema_version"].as_u64() == Some(1)
            && report["adapter"] == "djinn_live"
            && report["adapter_contract"] == live::ADAPTER_CONTRACT
            && report["assessment"] == "observed"
            && report["freshness"] == "fresh_sample"
            && matches!(report["activity"].as_str(), Some("busy" | "unknown"))
            && report["activity_scope"] == "legacy_runner_ownership"
            && report["attention"] == "unknown"
            && report["authorization"] == "none"
            && report["dispatchable"] == false
            && report["native_lifecycle"] == "unestablished"
            && report["job_completion"] == "unestablished",
    )?;
    let remaining = schema::uint(schema::field(report, "expires_in_ms")?, true)?;
    schema::require(remaining <= 3000)?;
    schema::require(attachment["source_contract"] == live::NATIVE_CONTRACT)?;
    let pins = schema::project(attachment, "runtime_id instance_id session_id")?;
    schema::uuid(schema::string(&pins["runtime_id"])?)?;
    schema::uuid(schema::string(&pins["instance_id"])?)?;
    schema::session(schema::string(&pins["session_id"])?)?;
    schema::require(report["selection"] == pins)?;
    let source = schema::field(report, "source")?;
    schema::fields(
        source,
        "contract runtime_id instance_id session_id sequence sampled_at_ms valid_for_ms source activity_scope capabilities",
    )?;
    schema::require(
        source["contract"] == live::NATIVE_CONTRACT
            && source["source"] == "legacy_session_run_state"
            && source["activity_scope"] == "legacy_runner_ownership",
    )?;
    for key in ["runtime_id", "instance_id", "session_id"] {
        schema::require(source[key] == pins[key])?;
    }
    schema::uint(schema::field(source, "sequence")?, true)?;
    schema::uint(schema::field(source, "sampled_at_ms")?, false)?;
    schema::require(schema::uint(schema::field(source, "valid_for_ms")?, false)? == 3000)?;
    schema::capabilities(schema::field(source, "capabilities")?)?;
    schema::project(
        source,
        "contract runtime_id instance_id session_id sequence sampled_at_ms valid_for_ms source activity_scope capabilities",
    )
}

fn preflight(
    workspace: &Path,
    group: &str,
    member: &str,
    expected: Option<&str>,
) -> Result<groups::View> {
    groups::selector(group)?;
    groups::selector(member)?;
    let view = groups::read_groups(workspace)?;
    if expected.is_some_and(|token| token != view.revision()) {
        return Err(Error("groups_revision_conflict"));
    }
    view.member(group, member)?;
    Ok(view)
}

pub fn attach(workspace: &Path, request: AttachRequest<'_>) -> Result<AuthorityValue> {
    attach_with_clock(workspace, request, &SystemClock)
}

fn attach_with_clock(
    workspace: &Path,
    request: AttachRequest<'_>,
    clock: &dyn Clock,
) -> Result<AuthorityValue> {
    let source_request = Request {
        file: request.file.to_owned(),
        runtime: request.runtime.to_owned(),
        instance: request.instance.to_owned(),
        session: request.session.to_owned(),
        timeout: request.timeout,
    };
    live::validate(&source_request).map_err(|_| Error("invalid_live_arguments"))?;
    let view = preflight(
        workspace,
        request.group,
        request.member,
        request.expected_revision,
    )?;
    let original_member = view.member(request.group, request.member)?;
    let member = project_authority(original_member, "participant_id live_attachment")?;
    let previous = member.get("live_attachment").filter(|v| !v.is_null());
    let pins = json!({"runtime_id":request.runtime,"instance_id":request.instance,"session_id":request.session});
    if !request.reattach
        && previous.is_some_and(|p| {
            ["runtime_id", "instance_id", "session_id"]
                .iter()
                .any(|k| p[k] != pins[k])
        })
    {
        return Err(Error("live_reattach_required"));
    }
    // No mutation lock is held while opening or validating native data.
    let report = live::inspect_with_clock(&source_request, clock)?;
    if report["assessment"] != "observed" {
        return Err(Error(if report["assessment"] == "incompatible" {
            "live_source_incompatible"
        } else {
            "live_source_unavailable"
        }));
    }
    accepted(
        &report,
        &json!({"source_contract":live::NATIVE_CONTRACT,
        "runtime_id":request.runtime,"instance_id":request.instance,"session_id":request.session}),
    )
    .map_err(|_| Error("invalid_live_report"))?;
    let path = report["file"]["resolved_path"]
        .as_str()
        .ok_or(Error("invalid_live_report"))?;
    let attached_at = clock.timestamp()?;
    // Match Python's insertion order in persisted metadata, not serde map order.
    let mut candidate = AuthorityValue::object(
        [
            ("adapter", "djinn_live"),
            ("source_contract", live::NATIVE_CONTRACT),
            ("artifact_path", path),
            ("runtime_id", request.runtime),
            ("instance_id", request.instance),
            ("session_id", request.session),
            ("attached_at", attached_at.as_str()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), AuthorityValue::String(value.to_owned())))
        .collect(),
    )?;
    groups::validate_attachment(&candidate)?;
    if let Some(previous) = previous {
        let stable = "adapter source_contract artifact_path runtime_id instance_id session_id";
        if schema::project(previous, stable)? == project_authority(&candidate, stable)? {
            candidate = authority_field(original_member, "live_attachment")?.clone();
        } else if !request.reattach {
            return Err(Error("live_reattach_required"));
        }
    }
    groups::attach_live(
        workspace,
        groups::AttachMember {
            group: request.group,
            member: request.member,
            attachment: &candidate,
            expected_revision: view.revision(),
            expected_participant_id: schema::string(schema::field(&member, "participant_id")?)?,
            reattach: request.reattach,
        },
    )
}

pub fn detach(
    workspace: &Path,
    group: &str,
    member: &str,
    expected: Option<&str>,
) -> Result<AuthorityValue> {
    let view = preflight(workspace, group, member, expected)?;
    let participant = authority_field(view.member(group, member)?, "participant_id")?
        .as_str()
        .ok_or(Error("invalid_groups"))?;
    groups::detach_live(workspace, group, member, view.revision(), participant)
}

fn row(member: &Value) -> Value {
    let attachment = member
        .get("live_attachment")
        .cloned()
        .unwrap_or(Value::Null);
    json!({"participant_id":member["participant_id"],"key":member["key"],"role":member["role"],
        "association_basis":if attachment.is_null() {"unattached"} else {"explicit_declaration"},
        "live_attachment":attachment,"assessment":"unattached","activity":"unknown",
        "activity_scope":"legacy_runner_ownership","attention":"unknown","freshness":"unknown",
        "expires_in_ms":0,"source":null,"diagnostics":["live_source_unattached"],
        "authorization":"none","dispatchable":false,"native_lifecycle":"unestablished","job_completion":"unestablished"})
}

fn unknown(row: &mut Value, code: &str, assessment: &str) {
    row["assessment"] = json!(assessment);
    row["diagnostics"] = json!([code]);
    row["activity"] = json!("unknown");
    row["freshness"] = json!("unknown");
    row["expires_in_ms"] = json!(0);
    row["source"] = Value::Null;
}

pub fn status(workspace: &Path, group: &str, member: Option<&str>, timeout: f64) -> Result<Value> {
    status_with_clock(workspace, group, member, timeout, &SystemClock)
}

fn status_with_clock(
    workspace: &Path,
    group: &str,
    member: Option<&str>,
    timeout: f64,
    clock: &dyn Clock,
) -> Result<Value> {
    live::timeout(timeout)?;
    groups::selector(group)?;
    if let Some(member) = member {
        groups::selector(member)?;
    }
    let started = clock.monotonic();
    let started_at = clock.timestamp()?;
    let view = groups::read_groups(workspace)?;
    let selected = view.group(group)?;
    let native_members: Vec<&AuthorityValue> = match member {
        Some(member) => vec![view.member(group, member)?],
        None => authority_field(selected, "members")?
            .as_array()
            .ok_or(Error("invalid_groups"))?
            .iter()
            .collect(),
    };
    schema::require(native_members.len() <= 16)?;
    let members = native_members
        .into_iter()
        .map(|member| project_authority(member, "participant_id key role live_attachment"))
        .collect::<Result<Vec<_>>>()?;
    let state = view.state().ok_or(Error("groups_uninitialized"))?;
    let mut report = json!({"schema_version":1,"contract":STATUS_CONTRACT,"assessment":"unavailable",
        "authority_schema_version":crate::observation_wire::to_json(authority_field(state,"schema_version")?)?,"revision":view.revision(),
        "group":project_authority(selected,"group_id key name")?,"participants":[],
        "started_at":started_at,"diagnostics":[],"authorization":"none","dispatchable":false,
        "native_lifecycle":"unestablished","job_completion":"unestablished","limitations":LIMITATIONS});
    let mut rows: Vec<(Value, Instant)> = Vec::new();
    let mut timed_out = false;
    for member in &members {
        let mut row = row(member);
        let call_started = clock.monotonic();
        let remaining = timeout
            - call_started
                .saturating_duration_since(started)
                .as_secs_f64();
        if remaining <= 0.0 {
            timed_out = true;
            break;
        }
        let attachment = &row["live_attachment"];
        if !attachment.is_null() {
            let request = Request {
                file: schema::string(schema::field(attachment, "artifact_path")?)?.to_owned(),
                runtime: schema::string(schema::field(attachment, "runtime_id")?)?.to_owned(),
                instance: schema::string(schema::field(attachment, "instance_id")?)?.to_owned(),
                session: schema::string(schema::field(attachment, "session_id")?)?.to_owned(),
                timeout: remaining.min(1.0),
            };
            match live::inspect_with_clock(&request, clock) {
                Ok(result) if result["assessment"] == "observed" => {
                    match accepted(&result, attachment) {
                        Ok(source) => {
                            row["assessment"] = json!("observed");
                            row["activity"] = result["activity"].clone();
                            row["freshness"] = json!("fresh_sample");
                            row["expires_in_ms"] = result["expires_in_ms"].clone();
                            row["source"] = source;
                            row["diagnostics"] = json!([]);
                        }
                        Err(_) => unknown(&mut row, "invalid_live_report", "unavailable"),
                    }
                }
                Ok(result) => {
                    let assessment = if result["assessment"] == "incompatible" {
                        "incompatible"
                    } else {
                        "unavailable"
                    };
                    let code = result["diagnostics"]
                        .as_array()
                        .filter(|a| a.len() == 1)
                        .and_then(|a| a[0].as_str())
                        .filter(|c| {
                            matches!(
                                *c,
                                "schema_error"
                                    | "unsupported_contract"
                                    | "runtime_mismatch"
                                    | "instance_not_observed"
                                    | "session_not_observed"
                                    | "unsafe_directory"
                                    | "directory_changed"
                                    | "unsafe_artifact"
                                    | "response_limit"
                                    | "artifact_changed"
                                    | "timeout"
                                    | "future_sample"
                                    | "stale_sample"
                                    | "unsupported_platform"
                                    | "io_error"
                            )
                        })
                        .unwrap_or("live_source_unavailable");
                    unknown(&mut row, code, assessment);
                }
                Err(_) => unknown(&mut row, "live_source_unavailable", "unavailable"),
            }
        }
        rows.push((row, call_started));
    }
    let mut failure = match groups::read_groups(workspace) {
        Ok(current) if current.revision() == view.revision() => None,
        Ok(_) => Some("groups_changed"),
        Err(_) => Some("groups_unavailable"),
    };
    // Project before the final clocks so allocation/delivery preparation cannot
    // renew a lease. Preserve call starts separately for the receipt deduction.
    let calls: Vec<Instant> = rows.iter().map(|(_, call)| *call).collect();
    if failure.is_none() {
        report["participants"] = Value::Array(rows.into_iter().map(|(row, _)| row).collect());
    }
    report["observed_at"] = json!(clock.timestamp()?);
    let wall = clock.wall_ms();
    let finished = clock.monotonic();
    if timed_out || finished.saturating_duration_since(started).as_secs_f64() >= timeout {
        failure = Some("status_timeout");
    }
    if failure.is_none() {
        for (row, call_started) in report["participants"]
            .as_array_mut()
            .ok_or(Error("invalid_live_report"))?
            .iter_mut()
            .zip(calls)
        {
            expire(row, call_started, finished, wall)?;
        }
        report["assessment"] = json!("observed");
    }
    let elapsed = clock
        .monotonic()
        .saturating_duration_since(started)
        .as_secs_f64();
    if elapsed >= timeout {
        failure = Some("status_timeout");
    }
    if let Some(code) = failure {
        report["assessment"] = json!("unavailable");
        report["participants"] = json!([]);
        report["diagnostics"] = json!([code]);
    }
    report["duration_ms"] = json!((elapsed * 1_000_000.0).round() / 1000.0);
    Ok(report)
}

fn expire(row: &mut Value, call_started: Instant, finished: Instant, wall: f64) -> Result<()> {
    if row["source"].is_null() {
        return Ok(());
    }
    let age = wall - schema::uint(schema::field(&row["source"], "sampled_at_ms")?, false)? as f64;
    let expires = (schema::uint(schema::field(row, "expires_in_ms")?, true)? as f64
        - (finished
            .saturating_duration_since(call_started)
            .as_secs_f64()
            * 1000.0)
            .ceil())
    .min((3000.0 - age).floor());
    if age < 0.0 {
        unknown(row, "future_sample", "unavailable");
    } else if !age.is_finite() || !expires.is_finite() || expires <= 0.0 {
        unknown(row, "source_expired", "unavailable");
    } else {
        row["expires_in_ms"] = json!(expires as u64);
    }
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "group_live/tests.rs"]
mod tests;
