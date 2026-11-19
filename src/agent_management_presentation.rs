//! Optional, read-only generic Kit view. Receiver authority never depends on Kit.
use crate::{
    Result,
    agent_management::{self, ServeRequest},
    group_presenter::wire,
    observation_socket::{self as socket, Cancel, Delivery, Failure, WireResult},
    task_mutation::TaskOutput,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

pub struct PresentationRequest {
    pub socket: String,
    pub source: String,
}

pub fn run_arguments(
    core: &agent_management::RunRequest,
    presentation: &PresentationRequest,
    json_output: bool,
) -> Result<TaskOutput> {
    run_arguments_with_cancel(core, presentation, json_output, None)
}

/// Cancel only this owned receiver. The presentation worker is stopped and joined
/// on every receiver return, including cooperative cancellation.
pub fn run_arguments_with_cancel(
    core: &agent_management::RunRequest,
    presentation: &PresentationRequest,
    json_output: bool,
    cancel_receiver: Option<&AtomicBool>,
) -> Result<TaskOutput> {
    socket::validate_path(&presentation.socket)?;
    crate::groups::source(&presentation.source)?;
    let projection = Arc::new(Mutex::new(Value::Null));
    let naming = Arc::new(AtomicU64::new(0));
    let worker_naming = naming.clone();
    let stop = Arc::new(Stop(AtomicBool::new(false)));
    let model = projection.clone();
    let cancel = stop.clone();
    let request = PresentationRequest {
        socket: presentation.socket.clone(),
        source: presentation.source.clone(),
    };
    let worker = thread::Builder::new()
        .name("coven-normal-view".into())
        .spawn(move || worker_report(&request, &model, &cancel, Some(&worker_naming)));
    #[cfg(unix)]
    let result =
        agent_management::unix::run_presentation(core, projection, cancel_receiver, naming);
    #[cfg(not(unix))]
    let result =
        agent_management::run_with_projection_and_cancel(core, projection, cancel_receiver);
    stop.0.store(true, Ordering::Release);
    // All worker IO is cancellation-aware; close has its own 200ms bound.
    // Always join, including receiver startup errors, so no worker is orphaned.
    let presentation = match worker {
        Ok(worker) => worker.join().unwrap_or_else(
            |_| json!({"outcome":"failed","diagnostics":["presentation_worker_panicked"]}),
        ),
        Err(_) => json!({"outcome":"failed","diagnostics":["presentation_worker_unavailable"]}),
    };
    let mut output = result?;
    let mut report: Value = serde_json::from_str(&output.stdout)?;
    report["presentation"] = presentation;
    output.stdout = crate::session_support::output(&report, !json_output)?.stdout;
    Ok(output)
}

struct Stop(AtomicBool);
impl Cancel for Stop {
    fn interrupted(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Start the optional adapter before the receiver; only core readiness admits UI IO.
pub fn serve_arguments(
    core: &ServeRequest,
    presentation: &PresentationRequest,
    json_output: bool,
) -> Result<TaskOutput> {
    socket::validate_path(&presentation.socket)?;
    crate::groups::source(&presentation.source)?;
    let projection = Arc::new(Mutex::new(Value::Null));
    let stop = Arc::new(Stop(AtomicBool::new(false)));
    let (send, receive) = mpsc::sync_channel(1);
    let model = projection.clone();
    let cancel = stop.clone();
    let request = PresentationRequest {
        socket: presentation.socket.clone(),
        source: presentation.source.clone(),
    };
    let worker = thread::Builder::new()
        .name("coven-agent-view".into())
        .spawn(move || {
            let report = worker_report(&request, &model, &cancel, None);
            let _ = send.send(report);
        });
    // Always obtain structured core output, then retain its status and warnings verbatim.
    let core_output = agent_management::serve_with_projection(core, true, Some(projection));
    stop.0.store(true, Ordering::Release);
    let join_end = Instant::now() + Duration::from_secs(1);
    let presentation_report = if worker.is_ok() {
        receive.recv_timeout(Duration::from_secs(1)).unwrap_or_else(
            |_| json!({"outcome":"failed","diagnostics":["presentation_shutdown_timeout"]}),
        )
    } else {
        json!({"outcome":"failed","diagnostics":["presentation_worker_unavailable"]})
    };
    if let Ok(worker) = worker {
        while !worker.is_finished() && Instant::now() < join_end {
            thread::sleep(Duration::from_millis(1));
        }
        // No unbounded join: all normal paths use cancellation-aware IO and bounded close.
        if worker.is_finished() {
            let _ = worker.join();
        }
    }
    let mut output = core_output?;
    let mut report: Value = serde_json::from_str(&output.stdout)?;
    report["presentation"] = presentation_report;
    output.stdout = crate::session_support::output(&report, !json_output)?.stdout;
    Ok(output)
}

fn worker_report(
    request: &PresentationRequest,
    model: &Mutex<Value>,
    stop: &Stop,
    naming: Option<&AtomicU64>,
) -> Value {
    let mut report = json!({"outcome":"completed","authorization":"none",
        "selection":"read_only_reported_record_details","frames":0,"polls":0,
        "cleanup":"not_opened","diagnostics":[]});
    if let Err(error) = run(request, model, stop, &mut report, naming) {
        report["outcome"] = json!(if error.delivery == Delivery::Uncertain {
            "uncertain"
        } else if error.interrupted {
            "stopped"
        } else {
            "failed"
        });
        report["diagnostics"] = json!([error.code]);
    }
    report
}

#[derive(Debug, PartialEq)]
struct Model {
    epoch: String,
    rows: Vec<DisplayRow>,
}

#[derive(Debug, PartialEq)]
struct DisplayRow {
    id: String,
    label: String,
    fallback: &'static str,
    detail: String,
    state: &'static str,
    status: Option<&'static str>,
    current_until: Option<u64>,
    information: Information,
    qualified_bundle: Option<QualifiedBundle>,
}

#[derive(Debug, PartialEq)]
struct QualifiedBundle {
    // Borrowed display metadata has its own lease, independent of row membership.
    epoch: String,
    admission: u64,
    revision: String,
    target: Value,
    until: u64,
}

impl QualifiedBundle {
    fn valid(&self, latest: &Value, id: &str, admission: u64, tick: u64) -> bool {
        if latest["server_epoch"] != self.epoch
            || admission & 1 != 0
            || admission != self.admission
            || tick >= self.until
        {
            return false;
        }
        let Some(hints) = latest["_publisher_names"]
            .as_array()
            .filter(|all| all.len() <= 128)
        else {
            return false;
        };
        let mut matching = hints.iter().filter(|hint| hint["id"] == id);
        let Some(hint) = matching.next() else {
            return false;
        };
        matching.next().is_none()
            && hint["revision"] == self.revision
            && hint["target"] == self.target
            && hint["until"].as_u64().is_some_and(|until| tick < until)
    }
}

impl DisplayRow {
    fn clear_bundle(&mut self) {
        self.label = self.fallback.into();
        self.information.context = vec!["No qualified workspace context.".into()];
        self.qualified_bundle = None;
    }
}

#[derive(Debug, PartialEq, serde::Serialize)]
struct Information {
    attention: Vec<String>,
    activity: Vec<String>,
    context: Vec<String>,
}

impl Information {
    fn unknown() -> Self {
        Self {
            attention: vec!["Attention information unavailable from this report.".into()],
            activity: vec!["Current activity is not available from this report type.".into()],
            context: vec!["No qualified workspace context.".into()],
        }
    }

    fn compact(&self) -> Self {
        let shorten = |line: &str| {
            match line {
                "Attention information unavailable from this report." => {
                    "Report attention information unavailable."
                }
                "Current activity is not available from this report type." => {
                    "Activity unavailable for this report type."
                }
                "Last reported working." => "Last reported working (fresh).",
                "Last reported idle." => "Last reported idle (fresh).",
                "Last reported ended." => "Last reported ended (fresh).",
                other => other,
            }
            .to_owned()
        };
        Self {
            attention: self
                .attention
                .first()
                .map(|s| shorten(s))
                .into_iter()
                .collect(),
            activity: self
                .activity
                .first()
                .map(|s| shorten(s))
                .into_iter()
                .collect(),
            context: Vec::new(),
        }
    }
}

// These are selected-fact ages, not receipt/last-seen times. Bucket here rather
// than adding a UI clock or changing any receiver freshness anchor.
fn reported_information(observation: &Value) -> WireResult<Information> {
    let invalid = || Failure::new("presentation_projection_error");
    let mut info = Information::unknown();
    match (
        observation.get("fresh_pending_count"),
        observation.get("unacknowledged_pending_count"),
    ) {
        (None, None) => {}
        (Some(count), Some(unacked)) => {
            let count = count.as_u64().ok_or_else(invalid)?;
            let unacked = unacked.as_u64().ok_or_else(invalid)?;
            if count > 128 || unacked > count {
                return Err(invalid());
            }
            info.attention = vec![if count == 0 {
                "No pending request in recent reports.".into()
            } else if unacked == 0 {
                format!("{count} reported requests awaiting resolution.")
            } else {
                format!("Reported requests: {count}; unacknowledged: {unacked}.")
            }];
        }
        _ => return Err(invalid()),
    }
    let age = observation
        .get("age_ms")
        .filter(|v| !v.is_null())
        .map(|v| v.as_u64().ok_or_else(invalid))
        .transpose()?;
    if observation["capability"] != "activity" {
        return Ok(info);
    }
    let freshness = observation["freshness"].as_str().ok_or_else(invalid)?;
    let activity = match freshness {
        "expired" => "No recent activity evidence.",
        "historical" => "No activity report in this receiver session.",
        "unknown" if age.is_none() => "No activity report received yet.",
        "unknown" => "Current activity is unknown.",
        "fresh" if observation["origin"] == "djinn_client_attachment" => {
            "No activity report received yet."
        }
        "fresh" => match observation["reported_state"].as_str() {
            Some("working") => "Last reported working.",
            Some("idle") => "Last reported idle.",
            Some("blocked") => "Waiting on a reported request.",
            Some("ended") => "Last reported ended.",
            Some("unknown") => "Current activity is unknown.",
            _ => return Err(invalid()),
        },
        _ => return Err(invalid()),
    };
    info.activity = vec![activity.into()];
    if matches!(freshness, "fresh" | "expired") {
        if let Some(age) = age {
            info.activity
                .push(format!("Evidence age: {}s ({freshness}).", age / 1000));
        }
    }
    Ok(info)
}

// Called only after provenance/state validation. A received event is not an
// explicit job outcome; ended and idle deliberately carry no success status.
fn display_state(state: &str, current: bool) -> (&'static str, Option<&'static str>) {
    match (current, state) {
        (true, "working") => ("Working", Some("running")),
        (true, "blocked") => ("Blocked", Some("attention")),
        (true, "idle") => ("Idle", None),
        (true, "ended") => ("Ended", None),
        _ => ("Unknown", Some("unknown")),
    }
}

fn display_row(
    id: String,
    provider: &str,
    detail: String,
    state: &str,
    current: bool,
) -> DisplayRow {
    let provider = match provider {
        "djinn" => "Djinn",
        "claude" => "Claude",
        "codex" => "Codex",
        "copilot" => "Copilot",
        "pi" => "Pi",
        "opencode" => "OpenCode",
        _ => unreachable!("validated provider"),
    };
    let (state, status) = display_state(state, current);
    DisplayRow {
        id,
        label: provider.into(),
        fallback: provider,
        detail,
        state,
        status,
        current_until: None,
        information: Information::unknown(),
        qualified_bundle: None,
    }
}

impl Model {
    fn expire_current(&mut self, tick: u64) {
        self.rows
            .retain(|row| row.current_until.is_none_or(|until| tick < until));
    }

    fn publication_ceiling(&self) -> u64 {
        self.rows
            .iter()
            .flat_map(|row| {
                [
                    row.current_until,
                    row.qualified_bundle.as_ref().map(|bundle| bundle.until),
                ]
                .into_iter()
                .flatten()
            })
            .fold(
                naming_tick(Instant::now() + Duration::from_secs(5)),
                u64::min,
            )
    }

    fn items(&self, version: u64) -> WireResult<Value> {
        // v3 preserves names even under pressure. Its owner reserve also covers
        // quote/backslash escaping in both bounded 128-byte lease tokens.
        let limit = 96 * 1024 - if version == 3 { 768 } else { 512 };
        let mut items = Vec::with_capacity(self.rows.len());
        let tick = naming_tick(Instant::now());
        let eligible = |row: &&DisplayRow| row.current_until.is_none_or(|until| tick < until);
        for row in self.rows.iter().filter(eligible) {
            if row.id.len() > 128
                || row.label.len() > 160
                || row.detail.len() > 512
                || row.state.is_empty()
                || row.state.len() > 48
                || self.rows.len() > 128
            {
                return Err(Failure::new("integration_view_invalid_frame"));
            }
            let mut item = json!({"id":row.id,"label":row.label,"detail":row.detail});
            if version >= 2 {
                item["presentation"] = json!({"label":""});
                if let Some(status) = row.status {
                    item["presentation"]["status"] = json!(status);
                }
            }
            if version == 3 {
                let info = &row.information;
                let mut total = 0;
                for lines in [&info.attention, &info.activity, &info.context] {
                    if lines.len() > 4 {
                        return Err(Failure::new("integration_view_invalid_frame"));
                    }
                    for line in lines {
                        total += line.len();
                        if !safe_display_text(line, 256) {
                            return Err(Failure::new("integration_view_invalid_frame"));
                        }
                    }
                }
                if total > 1536 {
                    return Err(Failure::new("integration_view_invalid_frame"));
                }
                item["information"] = json!(info);
            }
            items.push(item);
        }
        // Explicit names must not crowd bounded inspector facts out of the
        // frame. Reserve the maximum owner/revision envelope before encoding.
        if serde_json::to_vec(&items)
            .map_err(|_| Failure::new("integration_view_invalid_frame"))?
            .len()
            > limit
        {
            if version == 3 {
                // Diagnostic detail is secondary. Never trade a human pane name
                // or primary evidence for verbose technical provenance.
                for item in &mut items {
                    item["detail"] = json!("Additional detail omitted at view size limit.");
                }
                if serde_json::to_vec(&items)
                    .map_err(|_| Failure::new("integration_view_invalid_frame"))?
                    .len()
                    > limit
                {
                    for (item, row) in items.iter_mut().zip(self.rows.iter().filter(eligible)) {
                        let compact = row.information.compact();
                        // This is Cove's production profile, not a promise to
                        // losslessly pack 128 maximum-size generic v3 objects.
                        if compact
                            .attention
                            .iter()
                            .chain(&compact.activity)
                            .any(|s| !s.is_ascii() || s.len() > 48)
                        {
                            return Err(Failure::new("integration_view_invalid_frame"));
                        }
                        item["information"] = json!(compact);
                        item["detail"] = json!(
                            "Additional detail omitted at view size limit. Context/age omitted."
                        );
                    }
                }
            } else {
                for (item, row) in items.iter_mut().zip(self.rows.iter().filter(eligible)) {
                    item["label"] = json!(row.fallback);
                }
            }
        }
        if version == 3
            && serde_json::to_vec(&items)
                .map_err(|_| Failure::new("integration_view_invalid_frame"))?
                .len()
                > limit
        {
            return Err(Failure::new("integration_view_invalid_frame"));
        }
        Ok(json!(items))
    }
}

// Project only bounded display fields while holding a try-lock. Never copy native
// identifiers or arbitrary future projection fields into the UI worker.
fn snapshot(model: &Mutex<Value>, selected: Option<&str>) -> WireResult<Option<Model>> {
    let value = match model.try_lock() {
        Ok(value) => value,
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Poisoned(_)) => {
            return Err(Failure::new("presentation_projection_error"));
        }
    };
    let Some(epoch) = value["server_epoch"].as_str() else {
        return Ok(None);
    };
    let invalid = || Failure::new("presentation_projection_error");
    if epoch.is_empty() || epoch.len() > 128 || epoch.chars().any(char::is_control) {
        return Err(invalid());
    }
    if value["contract"] == "coven.normal-observations.v1" {
        if value["authorization"] != "none" {
            return Err(invalid());
        }
        let observations = value["observations"].as_array().ok_or_else(invalid)?;
        let omitted = value["omitted_observations"].as_u64().ok_or_else(invalid)?;
        let retired = value["retired_native_observations"]
            .as_u64()
            .ok_or_else(invalid)?;
        let summary = omitted > 0 || retired > 0;
        if observations.len() > if summary { 127 } else { 128 } {
            return Err(invalid());
        }
        let mut rows = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        for observation in observations {
            let id = observation["observation_id"].as_str().ok_or_else(invalid)?;
            if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) || !ids.insert(id) {
                return Err(invalid());
            }
            let current_until = if observation["provider"] == "djinn"
                && value.get("_publisher_current").is_some()
            {
                let item_id = format!("observation:{id}");
                let Some(until) = current_hint_deadline(&value["_publisher_current"], &item_id)
                else {
                    continue;
                };
                if naming_tick(Instant::now()) >= until {
                    continue;
                }
                Some(until)
            } else {
                None
            };
            if observation["origin"] == "provider_native_fact" {
                #[cfg(unix)]
                agent_management::facts::validate_row(observation).map_err(|_| invalid())?;
                #[cfg(not(unix))]
                return Err(invalid());
                let event = observation["last_received_event"]
                    .as_str()
                    .ok_or_else(invalid)?;
                let timing = if observation["source_time"] == "unknown" {
                    "source age unknown".to_owned()
                } else {
                    format!(
                        "caller-supplied source time; {}; unordered, not current state",
                        observation["freshness"].as_str().ok_or_else(invalid)?
                    )
                };
                let item_id = format!("observation:{id}");
                let mut detail = format!(
                    "Coven: last received native hook fact: {event}; {timing}; UNKNOWN; {}; caller unverified; root, native identity and job completion unestablished",
                    observation["profile"].as_str().ok_or_else(invalid)?
                );
                if selected == Some(item_id.as_str()) {
                    detail.push_str("; selected (read-only)");
                }
                rows.push(display_row(
                    item_id,
                    observation["provider"].as_str().ok_or_else(invalid)?,
                    detail,
                    "unknown",
                    false,
                ));
                continue;
            }
            let provider = observation["provider"].as_str().ok_or_else(invalid)?;
            let state = observation["reported_state"].as_str().ok_or_else(invalid)?;
            let origin = observation["origin"].as_str().ok_or_else(invalid)?;
            let freshness = observation["freshness"].as_str().ok_or_else(invalid)?;
            let capability = observation["capability"].as_str().ok_or_else(invalid)?;
            let profile = observation
                .get("profile")
                .map(|v| v.as_str().ok_or_else(invalid))
                .transpose()?;
            let provenance = matches!(
                (provider, origin, profile),
                (
                    "djinn",
                    "djinn_native_event" | "djinn_client_attachment",
                    None
                ) | (
                    "opencode",
                    "provider_native_event",
                    Some("opencode-root.v1")
                ) | ("pi", "provider_native_event", Some("pi-context.v1"))
                    | (
                        "claude" | "codex" | "copilot" | "pi" | "opencode",
                        "v1_receipt",
                        None
                    )
            );
            if !provenance
                || !matches!(state, "unknown" | "working" | "blocked" | "idle" | "ended")
                || !matches!(freshness, "fresh" | "expired" | "historical" | "unknown")
                || !matches!(capability, "activity" | "reference_only")
                || capability
                    != if origin != "v1_receipt" || provider == "claude" {
                        "activity"
                    } else {
                        "reference_only"
                    }
                || (profile == Some("pi-context.v1") && matches!(state, "blocked" | "ended"))
            {
                return Err(invalid());
            }
            let item_id = format!("observation:{id}");
            let mut detail = format!(
                "Coven: last-reported {} ({freshness}); {origin}; {capability}; caller unverified; no pane binding or success claim",
                state.to_ascii_uppercase()
            );
            if let Some(profile) = profile {
                detail.push_str(if profile == "pi-context.v1" {
                    "; pi-context.v1; UI/RPC context only, root unestablished"
                } else {
                    "; opencode-root.v1; producer-qualified root, not receiver authenticated"
                });
            }
            if observation["hosting_state"] == "local_full_tui_root" {
                detail.push_str(
                    "; producer-reported selected local full-TUI root; caller unverified",
                );
            }
            if observation.get("fresh_pending_count").is_some() {
                let count = observation["fresh_pending_count"]
                    .as_u64()
                    .ok_or_else(invalid)?;
                let unacked = observation["unacknowledged_pending_count"]
                    .as_u64()
                    .ok_or_else(invalid)?;
                if count > 128 || unacked > count {
                    return Err(invalid());
                }
                detail.push_str(&format!("; pending {count}, unacknowledged {unacked}"));
            }
            if selected == Some(item_id.as_str()) {
                detail.push_str("; selected (read-only)");
            }
            rows.push(display_row(
                item_id,
                provider,
                detail,
                state,
                freshness == "fresh"
                    && capability == "activity"
                    && origin != "djinn_client_attachment",
            ));
            rows.last_mut().unwrap().current_until = current_until;
            rows.last_mut().unwrap().information = reported_information(observation)?;
        }
        if summary {
            rows.push(DisplayRow { id: "normal:limits".into(), label: "Evidence limits".into(), fallback: "Evidence limits",
                detail: format!("{omitted} observations omitted from this view; {retired} native observations retired; bounded volatile history and replay protection"),
                 state: "Notice", status: None, current_until: None, qualified_bundle: None,
                 information: Information { attention: vec!["Some evidence omitted or retired from this view.".into(), format!("Omitted: {omitted}; retired: {retired}.")],
                     activity: vec!["Bounded evidence history; not a current census.".into()], context: vec![] } });
        }
        return Ok(Some(Model {
            epoch: epoch.into(),
            rows,
        }));
    }
    if value["contract"] != "coven.agent-management.v1" || value["authorization"] != "none" {
        return Err(invalid());
    }
    let records = value["records"].as_array().ok_or_else(invalid)?;
    if records.len() > 128 {
        return Err(Failure::new("integration_view_invalid_frame"));
    }
    let mut rows = Vec::with_capacity(records.len());
    for record in records {
        let id = record["record_id"].as_str().ok_or_else(invalid)?;
        let provider = record["provider"].as_str().ok_or_else(invalid)?;
        let event = record["last_event"].as_str().ok_or_else(invalid)?;
        if id.len() != 64
            || !id.bytes().all(|b| b.is_ascii_hexdigit())
            || agent_management::event(provider, event).is_err()
            || record["reference_trust"] != "caller_supplied_unverified"
            || record["native_identity"] != "unestablished"
            || record["job_completion"] != "unestablished"
        {
            return Err(invalid());
        }
        let state = record["reported_state"].as_str().ok_or_else(invalid)?;
        if !matches!(state, "unknown" | "working" | "idle" | "ended")
            || (matches!(provider, "codex" | "copilot" | "pi" | "opencode") && state != "unknown")
        {
            return Err(invalid());
        }
        let (state, age) = if record["age_ms"].is_null() {
            ("UNKNOWN".to_owned(), "unavailable/unknown".to_owned())
        } else {
            let seconds = record["age_ms"].as_u64().ok_or_else(invalid)? / 1000;
            (state.to_ascii_uppercase(), format!("{seconds}s"))
        };
        let item_id = format!("agent:{id}");
        let mut detail = format!("Last report: {state} \u{b7} age {age}");
        if selected == Some(item_id.as_str()) {
            detail.push_str(" \u{b7} Selected (read-only); reference: caller supplied, unverified; native identity and job completion: unestablished");
        }
        rows.push(display_row(
            item_id,
            provider,
            detail,
            &state.to_ascii_lowercase(),
            !record["age_ms"].is_null(),
        ));
        let information = reported_information(&json!({
            "capability": if provider == "claude" { "activity" } else { "reference_only" },
            "freshness": if record["age_ms"].is_null() { "historical" }
                else if record["age_ms"].as_u64().is_some_and(|age| age >= 30_000) { "expired" }
                else { "fresh" },
            "reported_state": state.to_ascii_lowercase(), "age_ms": record["age_ms"]
        }))?;
        rows.last_mut().unwrap().information = information;
    }
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    if rows.windows(2).any(|pair| pair[0].id == pair[1].id) {
        return Err(invalid());
    }
    Ok(Some(Model {
        epoch: epoch.into(),
        rows,
    }))
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(3)
}

const REFRESH_NEEDED: &str = "presentation_refresh_needed";

fn replace_preflight(
    shared: &Mutex<Value>,
    epoch: &str,
    naming: Option<&AtomicU64>,
    admission: Option<u64>,
    rows: &[DisplayRow],
) -> WireResult<()> {
    let Some(naming) = naming else { return Ok(()) };
    let latest = match shared.try_lock() {
        Ok(latest) => latest,
        Err(TryLockError::WouldBlock) => return Err(Failure::new(REFRESH_NEEDED)),
        Err(TryLockError::Poisoned(_)) => {
            return Err(Failure::new("presentation_projection_error"));
        }
    };
    if latest["server_epoch"] != epoch {
        return Err(Failure::new("presentation_server_epoch_changed"));
    }
    let revision = naming.load(Ordering::Acquire);
    let tick = naming_tick(Instant::now());
    // Empty and partial frames need the same coherent admission as full frames.
    if revision & 1 != 0 || admission != Some(revision) {
        return Err(Failure::new(REFRESH_NEEDED));
    }
    if let Some(until) = rows.iter().filter_map(|row| row.current_until).min() {
        if tick >= until {
            return Err(Failure::new(REFRESH_NEEDED));
        }
    }
    // A borrowed label/context lease can be shorter than current membership,
    // including on rows with no membership deadline. Never renew it here.
    if rows.iter().any(|row| {
        row.qualified_bundle.as_ref().is_some_and(|bundle| {
            admission != Some(revision) || !bundle.valid(&latest, &row.id, revision, tick)
        })
    }) {
        return Err(Failure::new(REFRESH_NEEDED));
    }
    Ok(())
}

fn current_hint_deadline(hints: &Value, id: &str) -> Option<u64> {
    let hints = hints.as_array().filter(|all| all.len() <= 128)?;
    let mut matching = hints.iter().filter(|hint| hint["id"] == id);
    let hint = matching.next()?;
    if matching.next().is_some() || hint.as_object()?.len() != 2 {
        return None;
    }
    hint["until"].as_u64()
}

// Monotonic deadlines belong only to the in-process publisher projection.
pub(crate) fn naming_tick(at: Instant) -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    at.saturating_duration_since(start)
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn explicit_name(value: &Value) -> Option<&str> {
    let text = value.as_str()?;
    // Reject rather than truncate: never split UTF-8, ANSI, or multiline content.
    safe_display_text(text, 160).then_some(text)
}

fn safe_display_text(text: &str, limit: usize) -> bool {
    !text.trim().is_empty() && text.len() <= limit && !text.chars().any(|c|
        c.is_control() || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}'))
}

fn naming_result(reply: &socket::Reply) -> Option<Value> {
    crate::strict_json::check_depth(&reply.payload, 32).ok()?;
    let native = crate::values::parse(&reply.payload, crate::values::Profile::Authority).ok()?;
    let value: Value = serde_json::from_slice(&native.compact().ok()?).ok()?;
    if value.as_object()?.len() != 2 || value["id"] != reply.request_id {
        return None;
    }
    value.get("result").cloned()
}

fn matched_pane<'a>(panes: &'a [Value], target: &Value, inspected: &Value) -> Option<&'a Value> {
    if inspected.as_object()?.len() != 7
        || inspected["type"] != "terminal_inspected"
        || inspected["availability"] != "live"
        || inspected["terminal_id"] != target["terminal_id"]
        || inspected["runtime_generation"] != target["runtime_generation"]
    {
        return None;
    }
    let mut terminals = panes
        .iter()
        .filter(|p| p["terminal_id"] == target["terminal_id"]);
    let pane = terminals.next()?;
    if terminals.next().is_some()
        || pane["pane_id"].as_str()?.is_empty()
        || panes
            .iter()
            .filter(|p| p["pane_id"] == pane["pane_id"])
            .count()
            != 1
        || ["pane_id", "tab_id", "workspace_id"]
            .iter()
            .any(|k| pane[*k].as_str().is_none_or(str::is_empty) || pane[*k] != inspected[*k])
    {
        return None;
    }
    Some(pane)
}

#[cfg(test)]
fn named_pane<'a>(panes: &'a [Value], target: &Value, inspected: &Value) -> Option<&'a str> {
    explicit_name(&matched_pane(panes, target, inspected)?["label"])
}

fn workspace_context(workspaces: &Value, pane: &Value) -> Option<Vec<String>> {
    let workspaces = workspaces.as_array().filter(|all| all.len() <= 4096)?;
    let mut matched = workspaces
        .iter()
        .filter(|w| w["workspace_id"] == pane["workspace_id"]);
    let workspace = matched.next()?;
    if matched.next().is_some() {
        return None;
    }
    // Approved human metadata only. In particular, neither CWD nor a repository
    // path is a substitute for Kit's display metadata or cached branch.
    let project = explicit_name(&workspace["worktree"]["repo_name"])
        .filter(|s| {
            !s.contains(['/', '\\', ':']) && !matches!(*s, "." | "..") && !s.starts_with('~')
        })
        .or_else(|| {
            explicit_name(&workspace["label"]).filter(|s| {
                !s.contains(['/', '\\', ':']) && !matches!(*s, "." | "..") && !s.starts_with('~')
            })
        });
    let branch = explicit_name(&workspace["git_branch"]).filter(|s| {
        !s.starts_with(['/', '\\', '~'])
            && !s.contains(['\\', ':'])
            && !s.split('/').any(|part| matches!(part, "" | "." | ".."))
    });
    let mut context = Vec::new();
    context.push(format!("Kit project: {}", project.unwrap_or("unavailable")));
    context.push(format!("Kit branch: {}", branch.unwrap_or("unavailable")));
    Some(context)
}

fn resolve_names(
    shared: &Mutex<Value>,
    current: &mut Model,
    endpoint: &socket::Endpoint,
    stop: &Stop,
    naming: Option<&AtomicU64>,
) -> WireResult<()> {
    // Refresh the entire bundle, never retain metadata from a failed lookup,
    // moved pane, old association, or contended receiver projection.
    for row in &mut current.rows {
        row.clear_bundle();
    }
    let Some(naming) = naming else { return Ok(()) };
    let revision = naming.load(Ordering::Acquire);
    if revision & 1 != 0 {
        return Err(Failure::new(REFRESH_NEEDED));
    }
    let end = Instant::now() + Duration::from_millis(300);
    let hints = match shared.try_lock() {
        Ok(value) if value["server_epoch"] == current.epoch => value["_publisher_names"].clone(),
        Ok(_) => return Err(Failure::new("presentation_server_epoch_changed")),
        Err(TryLockError::WouldBlock) => return Err(Failure::new(REFRESH_NEEDED)),
        Err(TryLockError::Poisoned(_)) => {
            return Err(Failure::new("presentation_projection_error"));
        }
    };
    if naming.load(Ordering::Acquire) != revision {
        return Err(Failure::new(REFRESH_NEEDED));
    }
    let Some(hints) = hints.as_array().filter(|h| h.len() <= 128) else {
        return Err(Failure::new(REFRESH_NEEDED));
    };
    let eligible = |hint: &&Value| {
        let t = &hint["target"];
        crate::agent_navigation::validate_target(t).is_ok()
            && t["socket"] == endpoint.resolved
            && t["device"] == endpoint.device
            && t["inode"] == endpoint.inode
            && hint["until"]
                .as_u64()
                .is_some_and(|until| naming_tick(Instant::now()) < until)
            && current.rows.iter().any(|r| hint["id"] == r.id)
    };
    let candidates: Vec<_> = hints.iter().filter(eligible).collect();
    if candidates.is_empty() {
        return Ok(());
    }
    let mut budget = socket::ReadBudget::new(1024 * 1024);
    let Some(snapshot) = socket::naming_snapshot(endpoint, end, &mut budget, stop)
        .ok()
        .filter(|r| r.endpoint == *endpoint)
        .and_then(|r| naming_result(&r))
    else {
        return Err(Failure::new(REFRESH_NEEDED));
    };
    if snapshot["type"] != "session_snapshot"
        || snapshot["snapshot"]["terminal_navigation"]["version"] != 1
    {
        return Ok(());
    }
    let Some(panes) = snapshot["snapshot"]["panes"]
        .as_array()
        .filter(|p| p.len() <= 4096)
    else {
        return Ok(());
    };
    let mut names = Vec::new();
    for hint in candidates {
        if socket::check(end, stop).is_err() {
            break;
        }
        let target = &hint["target"];
        let Some(inspected) = socket::naming_inspect(
            endpoint,
            target["terminal_id"].as_str().unwrap(),
            end,
            &mut budget,
            stop,
        )
        .ok()
        .and_then(|r| naming_result(&r)) else {
            return Err(Failure::new(REFRESH_NEEDED));
        };
        if let Some(pane) = matched_pane(panes, target, &inspected) {
            let name = explicit_name(&pane["label"]);
            let Some(context) = workspace_context(&snapshot["snapshot"]["workspaces"], pane) else {
                continue;
            };
            let Some(tabs) = snapshot["snapshot"]["tabs"]
                .as_array()
                .filter(|all| all.len() <= 4096)
            else {
                continue;
            };
            let mut matched = tabs.iter().filter(|t| t["tab_id"] == pane["tab_id"]);
            let Some(tab) = matched.next() else { continue };
            if matched.next().is_some() || tab["workspace_id"] != pane["workspace_id"] {
                continue;
            }
            names.push((hint, name, context));
        }
    }
    if socket::check(end, stop).is_err() {
        return Err(Failure::new(REFRESH_NEEDED));
    }
    if socket::select(&endpoint.selected).ok().as_ref() != Some(endpoint) {
        return Ok(());
    }
    // Requalify against the receiver's latest epoch, association revision/target and
    // actual lease deadline after IO. No successful name survives a failed refresh.
    let latest = match shared.try_lock() {
        Ok(latest) => latest,
        Err(TryLockError::WouldBlock) => return Err(Failure::new(REFRESH_NEEDED)),
        Err(TryLockError::Poisoned(_)) => {
            return Err(Failure::new("presentation_projection_error"));
        }
    };
    if latest["server_epoch"] != current.epoch {
        return Err(Failure::new("presentation_server_epoch_changed"));
    }
    if naming.load(Ordering::Acquire) != revision {
        return Err(Failure::new(REFRESH_NEEDED));
    }
    let tick = naming_tick(Instant::now());
    current.rows.retain(|row| {
        row.current_until.is_none_or(|until| {
            tick < until
                && current_hint_deadline(&latest["_publisher_current"], &row.id)
                    .is_some_and(|end| tick < end)
        })
    });
    for (hint, name, context) in names {
        let (Some(id), Some(hint_revision), Some(until)) = (
            hint["id"].as_str(),
            hint["revision"].as_str(),
            hint["until"].as_u64(),
        ) else {
            continue;
        };
        let bundle = QualifiedBundle {
            epoch: current.epoch.clone(),
            admission: revision,
            revision: hint_revision.into(),
            target: hint["target"].clone(),
            until,
        };
        if bundle.valid(&latest, id, revision, naming_tick(Instant::now())) {
            if let Some(row) = current.rows.iter_mut().find(|r| hint["id"] == r.id) {
                if let Some(name) = name {
                    row.label = name.into();
                }
                row.information.context = context;
                row.qualified_bundle = Some(bundle);
            }
        }
    }
    Ok(())
}
fn pause(stop: &Stop, duration: Duration) {
    let end = Instant::now() + duration;
    while !stop.interrupted() && Instant::now() < end {
        thread::sleep(Duration::from_millis(10).min(end.saturating_duration_since(Instant::now())));
    }
}

fn defer_candidate(stop: &Stop, ceiling: u64) -> WireResult<()> {
    if naming_tick(Instant::now()) >= ceiling {
        return Err(Failure::new(REFRESH_NEEDED));
    }
    pause(stop, Duration::from_millis(20));
    Ok(())
}

fn run(
    request: &PresentationRequest,
    model: &Mutex<Value>,
    stop: &Stop,
    report: &mut Value,
    naming: Option<&AtomicU64>,
) -> WireResult<()> {
    let initial = loop {
        if stop.interrupted() {
            return Ok(());
        }
        if let Some(model) = snapshot(model, None)? {
            break model;
        }
        pause(stop, Duration::from_millis(20));
    };
    socket::validate_path(&request.socket).map_err(|e| Failure::new(e.code()))?;
    crate::groups::source(&request.source).map_err(|e| Failure::new(e.code()))?;
    let endpoint = socket::select(&request.socket)?;
    let reply = socket::read(
        &request.socket,
        socket::ReadMethod::Snapshot,
        &json!({}),
        deadline(),
        &mut socket::ReadBudget::new(1024 * 1024),
        stop,
    )?;
    if reply.endpoint != endpoint {
        return Err(Failure::new("endpoint_replaced"));
    }
    let version = wire::presentation_version(&reply, &request.source)?;
    let opened = socket::view_call(
        &endpoint,
        "integration.view.open",
        &json!({"source":request.source,"version":version}),
        deadline(),
        stop,
        &wire::opened,
    )
    .inspect_err(|error| {
        if error.delivery == Delivery::Uncertain {
            report["cleanup"] = json!("ttl_only_unknown_lease");
        }
    })?;
    let owner = json!({"source":request.source,"lease":opened["lease"],"epoch":opened["epoch"]});
    let epoch = opened["epoch"].as_str().expect("validated epoch");
    report["cleanup"] = json!("ttl_only");
    let result = (|| {
        let mut revision = 0u64;
        let mut shown = Value::Null;
        let mut current = initial;
        let server_epoch = current.epoch.clone();
        let mut selected: Option<String> = None;
        let mut ceiling = naming_tick(Instant::now() + Duration::from_secs(5));
        loop {
            if stop.interrupted() {
                return Ok(());
            }
            // Only a coherent candidate may replace or renew the shown view.
            // A transient race never publishes a cleared/cached candidate.
            let admission = naming.map(|fence| fence.load(Ordering::Acquire));
            if admission.is_some_and(|revision| revision & 1 != 0) {
                defer_candidate(stop, ceiling)?;
                continue;
            }
            let mut next = match snapshot(model, selected.as_deref())? {
                Some(next) => next,
                None if naming.is_none() => std::mem::replace(
                    &mut current,
                    Model {
                        epoch: server_epoch.clone(),
                        rows: Vec::new(),
                    },
                ),
                None => {
                    defer_candidate(stop, ceiling)?;
                    continue;
                }
            };
            if next.epoch != server_epoch {
                return Err(Failure::new("presentation_server_epoch_changed"));
            }
            next.expire_current(naming_tick(Instant::now()));
            let prepared = replace_preflight(model, &server_epoch, naming, admission, &next.rows)
                .and_then(|_| resolve_names(model, &mut next, &endpoint, stop, naming))
                .and_then(|_| {
                    next.expire_current(naming_tick(Instant::now()));
                    replace_preflight(model, &server_epoch, naming, admission, &next.rows)
                });
            match prepared {
                Err(error) if error.code == REFRESH_NEEDED => {
                    defer_candidate(stop, ceiling)?;
                    continue;
                }
                Err(error) => return Err(error),
                Ok(()) => {}
            }
            let items = next.items(version)?;
            if items != shown {
                let next_revision = revision
                    .checked_add(1)
                    .ok_or_else(|| Failure::new("integration_view_invalid_frame"))?;
                let mut frame = owner.clone();
                frame["revision"] = json!(next_revision);
                frame["items"] = items.clone();
                if serde_json::to_vec(&frame)
                    .map_err(|_| Failure::new("integration_view_invalid_frame"))?
                    .len()
                    > 96 * 1024
                {
                    return Err(Failure::new("integration_view_invalid_frame"));
                }
                let replaced = socket::view_call_guarded(
                    &endpoint,
                    "integration.view.replace",
                    &frame,
                    deadline(),
                    stop,
                    &|bytes, id| wire::replaced(bytes, id, epoch, next_revision),
                    &|| replace_preflight(model, &server_epoch, naming, admission, &next.rows),
                );
                match replaced {
                    Err(error)
                        if error.delivery == Delivery::Unsent && error.code == REFRESH_NEEDED =>
                    {
                        // Nothing was written. Rebuild on the same lease and revision;
                        // attempted writes, including zero-byte failures, never enter here.
                        defer_candidate(stop, ceiling)?;
                        continue;
                    }
                    Err(error) => return Err(error),
                    Ok(_) => {}
                }
                revision = next_revision;
                report["frames"] = json!(revision);
                shown = items;
                ceiling = next.publication_ceiling();
            }
            current = next;
            if selected
                .as_ref()
                .is_some_and(|id| !current.rows.iter().any(|row| &row.id == id))
            {
                selected = None;
            }
            pause(stop, Duration::from_millis(250));
            if stop.interrupted() {
                return Ok(());
            }
            // A known clock boundary calls for a new coherent candidate, not a
            // renewal of the old frame. Admission races still use bounded defer.
            let tick = naming_tick(Instant::now());
            if current.rows.iter().any(|row| {
                row.current_until.is_some_and(|until| tick >= until)
                    || row
                        .qualified_bundle
                        .as_ref()
                        .is_some_and(|bundle| tick >= bundle.until)
            }) {
                continue;
            }
            let polled = socket::view_call_guarded(
                &endpoint,
                "integration.view.poll",
                &owner,
                deadline(),
                stop,
                &|bytes, id| wire::polled(bytes, id, epoch, revision),
                &|| replace_preflight(model, &server_epoch, naming, admission, &current.rows),
            );
            let polled = match polled {
                Err(error)
                    if error.delivery == Delivery::Unsent && error.code == REFRESH_NEEDED =>
                {
                    defer_candidate(stop, ceiling)?;
                    continue;
                }
                Err(error) => return Err(error),
                Ok(polled) => polled,
            };
            report["polls"] = json!(report["polls"].as_u64().unwrap_or(0) + 1);
            ceiling = current.publication_ceiling();
            current.expire_current(naming_tick(Instant::now()));
            for intent in polled["intents"].as_array().expect("validated intents") {
                if intent["epoch"] == epoch
                    && intent["revision"] == revision
                    && current
                        .rows
                        .iter()
                        .any(|row| Some(row.id.as_str()) == intent["item_id"].as_str())
                {
                    selected = intent["item_id"].as_str().map(str::to_owned);
                }
            }
        }
    })();
    // An uncertain request is never replayed, refreshed, or closed.
    if result
        .as_ref()
        .err()
        .is_some_and(|e: &Failure| e.delivery == Delivery::Uncertain)
    {
        report["cleanup"] = json!("ttl_only_uncertain");
        return result;
    }
    let closed = socket::view_call(
        &endpoint,
        "integration.view.close",
        &owner,
        Instant::now() + Duration::from_millis(200),
        &socket::NoCancel,
        &wire::closed,
    );
    match closed {
        Ok(_) => report["cleanup"] = json!("closed"),
        Err(error) => {
            report["cleanup"] = json!(if error.delivery == Delivery::Uncertain {
                "ttl_only_uncertain"
            } else {
                "ttl_only"
            });
            if result.is_ok() {
                return Err(error);
            }
        }
    }
    result
}

#[cfg(test)]
#[path = "agent_management_presentation_continuity_tests.rs"]
mod continuity_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v3_information_reports_evidence_not_inventory_or_completion() {
        let mut row = json!({"capability":"activity","origin":"djinn_native_event",
            "freshness":"fresh","reported_state":"working","age_ms":1999});
        let info = reported_information(&row).unwrap();
        assert_eq!(
            info.attention,
            ["Attention information unavailable from this report."]
        );
        assert_eq!(
            info.activity,
            ["Last reported working.", "Evidence age: 1s (fresh)."]
        );
        row["age_ms"] = json!(1000);
        assert_eq!(reported_information(&row).unwrap(), info);
        for (count, unacked, expected) in [
            (0, 0, "No pending request in recent reports."),
            (2, 1, "Reported requests: 2; unacknowledged: 1."),
            (2, 0, "2 reported requests awaiting resolution."),
        ] {
            row["fresh_pending_count"] = json!(count);
            row["unacknowledged_pending_count"] = json!(unacked);
            assert_eq!(reported_information(&row).unwrap().attention, [expected]);
        }
        for (freshness, state, age, expected) in [
            ("fresh", "idle", json!(2000), "Last reported idle."),
            (
                "fresh",
                "blocked",
                json!(2000),
                "Waiting on a reported request.",
            ),
            ("fresh", "ended", json!(2000), "Last reported ended."),
            (
                "fresh",
                "unknown",
                json!(2000),
                "Current activity is unknown.",
            ),
            (
                "expired",
                "unknown",
                json!(32000),
                "No recent activity evidence.",
            ),
            (
                "historical",
                "unknown",
                Value::Null,
                "No activity report in this receiver session.",
            ),
            (
                "unknown",
                "unknown",
                Value::Null,
                "No activity report received yet.",
            ),
        ] {
            row["freshness"] = json!(freshness);
            row["reported_state"] = json!(state);
            row["age_ms"] = age;
            let info = reported_information(&row).unwrap();
            assert_eq!(info.activity[0], expected);
            if freshness == "expired" {
                assert_eq!(info.activity[1], "Evidence age: 32s (expired).");
            }
            assert!(!serde_json::to_string(&info).unwrap().contains("success"));
        }
        row["capability"] = json!("reference_only");
        assert_eq!(
            reported_information(&row).unwrap().activity,
            ["Current activity is not available from this report type."]
        );
        for (key, bad) in [
            ("fresh_pending_count", Value::Null),
            ("unacknowledged_pending_count", json!(3)),
            ("age_ms", json!("0")),
        ] {
            let mut invalid = row.clone();
            invalid[key] = bad;
            assert!(reported_information(&invalid).is_err());
        }
        row.as_object_mut().unwrap().remove("fresh_pending_count");
        assert!(reported_information(&row).is_err());
    }

    #[test]
    fn v3_workspace_context_is_allowlisted_unique_and_honest_about_missing_branch() {
        let pane = json!({"workspace_id":"workspace"});
        let workspace = json!({"workspace_id":"workspace","label":"Fallback",
            "worktree":{"repo_name":"Coven","path":"/PRIVATE"},"git_branch":"feature/ui",
            "cwd":"/PRIVATE","command":"PRIVATE","native_sid":"PRIVATE"});
        assert_eq!(
            workspace_context(&json!([workspace.clone()]), &pane).unwrap(),
            ["Kit project: Coven", "Kit branch: feature/ui"]
        );
        for bad in [
            Value::Null,
            json!("/PRIVATE"),
            json!("unsafe\nbranch"),
            json!("\u{1b}[31mPRIVATE"),
            json!("\u{202e}PRIVATE"),
            json!("x".repeat(161)),
        ] {
            let mut w = workspace.clone();
            w["worktree"]["repo_name"] = bad.clone();
            w["git_branch"] = bad;
            let context = workspace_context(&json!([w]), &pane).unwrap();
            assert_eq!(
                context,
                ["Kit project: Fallback", "Kit branch: unavailable"]
            );
            assert!(!context.join(" ").contains("PRIVATE"));
        }
        assert!(workspace_context(&json!([workspace.clone(), workspace.clone()]), &pane).is_none());
        assert!(workspace_context(&json!([workspace]), &json!({"workspace_id":"moved"})).is_none());
    }

    #[test]
    fn context_contention_clears_the_whole_bundle_without_resetting_facts_or_deadlines() {
        let shared = Mutex::new(json!({"server_epoch":"epoch"}));
        let _held = shared.lock().unwrap();
        let fence = AtomicU64::new(0);
        let mut row = display_row(
            "observation:opaque".into(),
            "djinn",
            "original facts".into(),
            "working",
            true,
        );
        row.label = "Old name".into();
        row.information.context = vec!["Kit project: stale".into(), "Kit branch: stale".into()];
        row.qualified_bundle = Some(QualifiedBundle {
            epoch: "epoch".into(),
            admission: 0,
            revision: "PRIVATE-revision".into(),
            target: json!({"runtime_generation":"PRIVATE-generation"}),
            until: 123,
        });
        row.current_until = Some(123);
        let mut model = Model {
            epoch: "epoch".into(),
            rows: vec![row],
        };
        let resolution = resolve_names(
            &shared,
            &mut model,
            &socket::Endpoint {
                selected: "/not/contacted".into(),
                resolved: "/not/contacted".into(),
                device: 0,
                inode: 0,
            },
            &Stop(AtomicBool::new(false)),
            Some(&fence),
        );
        assert_eq!(resolution.unwrap_err().code, REFRESH_NEEDED);
        assert_eq!(model.rows[0].label, "Djinn");
        assert_eq!(
            model.rows[0].information.context,
            ["No qualified workspace context."]
        );
        assert_eq!(model.rows[0].detail, "original facts");
        assert_eq!(model.rows[0].status, Some("running"));
        assert_eq!(model.rows[0].current_until, Some(123));
        assert!(model.rows[0].qualified_bundle.is_none());
        assert_eq!(fence.load(Ordering::Acquire), 0);
    }

    #[test]
    fn v3_full_production_frames_preserve_names_and_primary_facts_under_pressure() {
        for utf8 in [false, true] {
            let mut model = Model {
                epoch: "epoch".into(),
                rows: vec![],
            };
            for n in 0..128 {
                let freshness = ["fresh", "expired", "historical", "unknown"][n % 4];
                let state = if freshness == "fresh" {
                    ["working", "blocked", "idle", "ended", "unknown"][n % 5]
                } else {
                    "unknown"
                };
                let mut row = display_row(
                    format!("observation:{n:064x}"),
                    "djinn",
                    if utf8 {
                        "\u{754c}".repeat(170) + "xx"
                    } else {
                        "\\\"".repeat(256)
                    },
                    state,
                    freshness == "fresh",
                );
                row.label = if utf8 {
                    "\u{754c}".repeat(52) + "\\\"\\\""
                } else {
                    "\\\"".repeat(80)
                };
                row.information = reported_information(&json!({"origin":"djinn_native_event",
                    "capability":"activity","freshness":freshness,"reported_state":state,
                    "age_ms":u64::MAX,"fresh_pending_count":128,"unacknowledged_pending_count":128})).unwrap();
                row.information.context = vec![
                    format!("Kit project: {}", "\\\"".repeat(80)),
                    format!("Kit branch: {}", "\\\"".repeat(80)),
                ];
                model.rows.push(row);
            }
            let items = model.items(3).unwrap();
            let frame = json!({"source":"s".repeat(64),"lease":"\\\"".repeat(64),
                "epoch":"\\\"".repeat(64),"revision":u64::MAX,"items":items});
            assert!(serde_json::to_vec(&frame).unwrap().len() <= 96 * 1024);
            for (item, row) in frame["items"].as_array().unwrap().iter().zip(&model.rows) {
                assert_eq!(item["label"], row.label);
                assert_eq!(item["id"], row.id);
                assert_eq!(item["presentation"]["label"], "");
                assert_eq!(item["presentation"]["status"].as_str(), row.status);
                assert_eq!(
                    item["information"]["attention"][0],
                    row.information.attention[0]
                );
                assert_eq!(
                    item["information"]["activity"][0],
                    row.information.compact().activity[0]
                );
                assert_eq!(item["information"]["context"], json!([]));
                assert!(
                    item["detail"]
                        .as_str()
                        .unwrap()
                        .contains("Context/age omitted")
                );
            }
            for version in [1, 2] {
                let legacy = model.items(version).unwrap();
                assert_eq!(legacy[0]["detail"], model.rows[0].detail);
                assert!(legacy[0].get("information").is_none());
            }
        }
    }

    #[test]
    fn v3_small_frames_keep_diagnostics_and_information_out_of_legacy_profiles() {
        let mut row = display_row(
            format!("observation:{}", "a".repeat(64)),
            "djinn",
            "Coven: caller unverified; no success claim".into(),
            "working",
            true,
        );
        row.information =
            reported_information(&json!({"capability":"activity","freshness":"fresh",
            "reported_state":"working","age_ms":1200}))
            .unwrap();
        let model = Model {
            epoch: "epoch".into(),
            rows: vec![row],
        };
        for version in [1, 2, 3] {
            let items = model.items(version).unwrap();
            assert_eq!(
                items[0].as_object().unwrap().len(),
                match version {
                    1 => 3,
                    2 => 4,
                    _ => 5,
                }
            );
            assert_eq!(items[0]["detail"], model.rows[0].detail);
            assert_eq!(items[0].get("information").is_some(), version == 3);
        }
    }

    #[test]
    fn v3_information_limits_count_utf8_bytes_and_reject_unsafe_text() {
        let mut model = Model {
            epoch: "epoch".into(),
            rows: vec![display_row(
                "observation:opaque".into(),
                "djinn",
                "diagnostic".into(),
                "unknown",
                false,
            )],
        };
        for bad in [
            vec!["x".to_owned(); 5],
            vec![String::new()],
            vec!["\u{754c}".repeat(86)],
            vec!["line\nline".into()],
            vec!["\u{200f}unsafe".into()],
            vec!["\u{1b}[31munsafe".into()],
        ] {
            model.rows[0].information.attention = bad;
            assert!(model.items(3).is_err());
            assert!(model.items(1).is_ok());
            assert!(model.items(2).is_ok());
        }
        model.rows[0].information = Information {
            attention: vec!["\u{754c}".repeat(85); 4],
            activity: vec!["x".repeat(256); 2],
            context: vec![],
        };
        assert!(model.items(3).is_ok()); // 4 * 255 + 2 * 256 = 1532 bytes.
        model.rows[0].information.context = vec!["extra".into()];
        assert!(model.items(3).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn transport_pre_send_rebuilds_expired_or_invalidated_frames_without_consuming_revision() {
        use std::{
            io::{BufRead, BufReader, Write},
            os::unix::{fs::PermissionsExt, net::UnixListener},
        };
        for (mode, version) in [
            ("expiry", 2),
            ("admission", 2),
            ("stop", 2),
            ("expiry", 3),
            ("admission", 3),
            ("stop", 3),
        ] {
            let root = tempfile::tempdir().unwrap();
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
            let path = root.path().canonicalize().unwrap().join("view.sock");
            let listener = UnixListener::bind(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            listener.set_nonblocking(true).unwrap();
            let endpoint = socket::select(path.to_str().unwrap()).unwrap();
            let until = naming_tick(Instant::now() + Duration::from_secs(1));
            let old_id = format!("observation:{}", "a".repeat(64));
            let new_id = format!("observation:{}", "b".repeat(64));
            let value = |id: &str, end: u64| {
                json!({"contract":"coven.normal-observations.v1",
                "server_epoch":"core","authorization":"none","omitted_observations":0,
                "retired_native_observations":0,"_publisher_current":[{"id":id,"until":end}],
                "observations":[{"observation_id":id.strip_prefix("observation:").unwrap(),
                    "provider":"djinn","origin":"djinn_client_attachment","capability":"activity",
                    "freshness":"unknown","reported_state":"unknown"}]})
            };
            let shared = Mutex::new(value(&old_id, until));
            let naming = AtomicU64::new(0);
            let stop = Stop(AtomicBool::new(false));
            let mut stale = super::snapshot(&shared, None).unwrap().unwrap();
            stale.rows[0].information.context = vec!["Kit branch: stale".into()];
            let owner = json!({"source":"source","lease":"lease","epoch":"view"});
            let mut frame = owner.clone();
            frame["revision"] = json!(1);
            frame["items"] = stale.items(version).unwrap();
            assert_eq!(frame["items"][0]["id"], old_id);
            let (connected, prepared) = mpsc::sync_channel(1);
            thread::scope(|scope| {
                let server = scope.spawn(|| {
                    for attempt in 0..2 {
                        let end = Instant::now() + Duration::from_secs(2);
                        let (mut stream, _) = loop {
                            match listener.accept() {
                                Ok(stream) => break stream,
                                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                    assert!(
                                        Instant::now() < end,
                                        "missing connection: {mode}/{attempt}"
                                    );
                                    thread::sleep(Duration::from_millis(1));
                                }
                                Err(e) => panic!("{e}"),
                            }
                        };
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        if attempt == 0 {
                            connected.send(()).unwrap();
                        }
                        let mut line = String::new();
                        let count = BufReader::new(stream.try_clone().unwrap())
                            .read_line(&mut line)
                            .unwrap();
                        if attempt == 0 {
                            assert_eq!(count, 0, "stale frame reached the socket: {mode}");
                            continue;
                        }
                        let request: Value = serde_json::from_str(&line).unwrap();
                        assert_eq!(request["params"]["lease"], "lease");
                        let result = if mode == "stop" {
                            assert_eq!(request["method"], "integration.view.close");
                            json!({"type":"view_closed"})
                        } else {
                            assert_eq!(request["method"], "integration.view.replace");
                            assert_eq!(request["params"]["revision"], 1);
                            assert_eq!(request["params"]["items"][0]["id"], new_id);
                            assert!(!request["params"]["items"].to_string().contains(&old_id));
                            assert!(
                                !request["params"]["items"]
                                    .to_string()
                                    .contains("Kit branch: stale")
                            );
                            json!({"type":"view_replaced","epoch":"view","revision":1})
                        };
                        writeln!(stream, "{}", json!({"id":request["id"],"result":result}))
                            .unwrap();
                    }
                });
                let failure = socket::view_call_guarded(
                    &endpoint,
                    "integration.view.replace",
                    &frame,
                    deadline(),
                    &stop,
                    &|bytes, id| wire::replaced(bytes, id, "view", 1),
                    &|| {
                        // This barrier is reached only after frame encoding, connection
                        // and endpoint fencing, beyond the former items() check.
                        prepared.recv_timeout(Duration::from_secs(1)).unwrap();
                        match mode {
                            "expiry" => {
                                let end = Instant::now() + Duration::from_secs(2);
                                while naming_tick(Instant::now()) < until {
                                    assert!(Instant::now() < end);
                                    thread::sleep(Duration::from_millis(1));
                                }
                            }
                            "admission" => {
                                naming.store(2, Ordering::Release);
                            }
                            "stop" => {
                                stop.0.store(true, Ordering::Release);
                            }
                            _ => unreachable!(),
                        }
                        if mode != "stop" {
                            *shared.lock().unwrap() = value(
                                &new_id,
                                naming_tick(Instant::now() + Duration::from_secs(2)),
                            );
                        }
                        replace_preflight(&shared, "core", Some(&naming), Some(0), &stale.rows)
                    },
                )
                .unwrap_err();
                assert_eq!(failure.delivery, Delivery::Unsent);
                if mode == "stop" {
                    assert_eq!(failure.code, "interrupted");
                    assert!(failure.interrupted);
                    socket::view_call(
                        &endpoint,
                        "integration.view.close",
                        &owner,
                        Instant::now() + Duration::from_millis(200),
                        &socket::NoCancel,
                        &wire::closed,
                    )
                    .unwrap();
                } else {
                    assert_eq!(failure.code, REFRESH_NEEDED);
                    let fresh = super::snapshot(&shared, None).unwrap().unwrap();
                    frame["items"] = fresh.items(version).unwrap();
                    let admission = naming.load(Ordering::Acquire);
                    socket::view_call_guarded(
                        &endpoint,
                        "integration.view.replace",
                        &frame,
                        deadline(),
                        &stop,
                        &|bytes, id| wire::replaced(bytes, id, "view", 1),
                        &|| {
                            replace_preflight(
                                &shared,
                                "core",
                                Some(&naming),
                                Some(admission),
                                &fresh.rows,
                            )
                        },
                    )
                    .unwrap();
                }
                server.join().unwrap();
            });
        }
    }

    #[test]
    #[cfg(unix)]
    fn transport_pre_send_rebuilds_expired_or_changed_bundles_without_retiring_current_rows() {
        use std::{
            io::{BufRead, BufReader, Write},
            os::unix::{fs::PermissionsExt, net::UnixListener},
        };
        for version in [1, 2, 3] {
            for mode in [
                "expiry",
                "extended_hint",
                "revision",
                "removed",
                "generation",
                "shortened_hint",
                "admission_without_membership",
            ] {
                let root = tempfile::tempdir().unwrap();
                std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
                    .unwrap();
                let path = root.path().canonicalize().unwrap().join("bundle.sock");
                let listener = UnixListener::bind(&path).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
                listener.set_nonblocking(true).unwrap();
                let endpoint = socket::select(path.to_str().unwrap()).unwrap();
                let membership_until = naming_tick(Instant::now() + Duration::from_secs(30));
                // The final 200ms of a five-second borrowed bundle lease.
                let qualified_at = Instant::now() - Duration::from_millis(4800);
                let bundle_until = if matches!(mode, "expiry" | "extended_hint") {
                    naming_tick(qualified_at + Duration::from_secs(5))
                } else {
                    membership_until
                };
                let target = json!({"socket":endpoint.resolved,"device":endpoint.device,"inode":endpoint.inode,
                    "terminal_id":"PRIVATE-terminal","runtime_generation":"PRIVATE-generation"});
                let id = format!("observation:{}", "c".repeat(64));
                let hint = json!({"id":id,"revision":"PRIVATE-revision","target":target,"until":bundle_until});
                let shared = Mutex::new(json!({"server_epoch":"core","_publisher_names":[hint]}));
                let naming = AtomicU64::new(0);
                let stop = Stop(AtomicBool::new(false));
                let other_provider = mode == "admission_without_membership";
                let mut row = display_row(
                    id.clone(),
                    if other_provider { "claude" } else { "djinn" },
                    "Original diagnostic facts".into(),
                    "working",
                    true,
                );
                row.current_until = (!other_provider).then_some(membership_until);
                row.label = "OLD-NAME".into();
                row.information.context = vec![
                    "Kit project: OLD-PROJECT".into(),
                    "Kit branch: OLD-BRANCH".into(),
                ];
                row.qualified_bundle = Some(QualifiedBundle {
                    epoch: "core".into(),
                    admission: 0,
                    revision: "PRIVATE-revision".into(),
                    target,
                    until: bundle_until,
                });
                let mut current = Model {
                    epoch: "core".into(),
                    rows: vec![row],
                };
                let mut frame = json!({"source":"source","lease":"lease","epoch":"view","revision":1,
                    "items":current.items(version).unwrap()});
                let encoded = frame["items"].clone();
                current.rows[0].qualified_bundle.as_mut().unwrap().until += 1;
                assert_eq!(
                    current.items(version).unwrap(),
                    encoded,
                    "private qualification is not wire state"
                );
                current.rows[0].qualified_bundle.as_mut().unwrap().until = bundle_until;
                assert!(!frame.to_string().contains("PRIVATE"));
                let (connected, prepared) = mpsc::sync_channel(1);
                thread::scope(|scope| {
                    let server = scope.spawn(|| {
                        for attempt in 0..2 {
                            let end = Instant::now() + Duration::from_secs(2);
                            let (mut stream, _) = loop {
                                match listener.accept() {
                                    Ok(stream) => break stream,
                                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                        assert!(
                                            Instant::now() < end,
                                            "missing connection: {mode}/{version}/{attempt}"
                                        );
                                        thread::sleep(Duration::from_millis(1));
                                    }
                                    Err(e) => panic!("{e}"),
                                }
                            };
                            stream
                                .set_read_timeout(Some(Duration::from_secs(2)))
                                .unwrap();
                            if attempt == 0 {
                                connected.send(()).unwrap();
                            }
                            let mut line = String::new();
                            let count = BufReader::new(stream.try_clone().unwrap())
                                .read_line(&mut line)
                                .unwrap();
                            if attempt == 0 {
                                assert_eq!(
                                    count, 0,
                                    "stale bundle reached the socket: {mode}/{version}"
                                );
                                continue;
                            }
                            let request: Value = serde_json::from_str(&line).unwrap();
                            assert_eq!(request["method"], "integration.view.replace");
                            assert_eq!(request["params"]["lease"], "lease");
                            assert_eq!(request["params"]["epoch"], "view");
                            assert_eq!(request["params"]["revision"], 1);
                            assert_eq!(request["params"]["items"].as_array().unwrap().len(), 1);
                            assert_eq!(request["params"]["items"][0]["id"], id);
                            assert_eq!(
                                request["params"]["items"][0]["label"],
                                if other_provider { "Claude" } else { "Djinn" }
                            );
                            for stale in ["OLD-NAME", "OLD-PROJECT", "OLD-BRANCH", "PRIVATE"] {
                                assert!(!line.contains(stale), "{stale} leaked: {mode}/{version}");
                            }
                            if version == 3 {
                                assert_eq!(
                                    request["params"]["items"][0]["information"]["context"],
                                    json!(["No qualified workspace context."])
                                );
                            }
                            writeln!(
                                stream,
                                "{}",
                                json!({"id":request["id"],"result":{
                                "type":"view_replaced","epoch":"view","revision":1}})
                            )
                            .unwrap();
                        }
                    });
                    let failure = socket::view_call_guarded(&endpoint, "integration.view.replace", &frame,
                        deadline(), &stop, &|bytes, id| wire::replaced(bytes, id, "view", 1), &|| {
                            // Runs after encoding, connection and endpoint qualification.
                            prepared.recv_timeout(Duration::from_secs(1)).unwrap();
                            match mode {
                                "expiry" | "extended_hint" => {
                                    let end = Instant::now() + Duration::from_secs(1);
                                    while naming_tick(Instant::now()) < bundle_until {
                                        assert!(Instant::now() < end);
                                        thread::sleep(Duration::from_millis(1));
                                    }
                                    if mode == "extended_hint" {
                                        shared.lock().unwrap()["_publisher_names"][0]["until"] = json!(membership_until);
                                    }
                                },
                                "revision" => shared.lock().unwrap()["_publisher_names"][0]["revision"] = json!("new"),
                                "removed" => shared.lock().unwrap()["_publisher_names"] = json!([]),
                                "generation" => shared.lock().unwrap()["_publisher_names"][0]["target"]["runtime_generation"] = json!("new"),
                                "shortened_hint" => shared.lock().unwrap()["_publisher_names"][0]["until"] = json!(0),
                                "admission_without_membership" => naming.store(2, Ordering::Release),
                                _ => unreachable!(),
                            }
                            replace_preflight(&shared, "core", Some(&naming), Some(0), &current.rows)
                        }).unwrap_err();
                    assert_eq!(failure.delivery, Delivery::Unsent);
                    assert_eq!(failure.code, REFRESH_NEEDED);
                    if !other_provider {
                        assert_eq!(naming.load(Ordering::Acquire), 0);
                    }
                    for row in &mut current.rows {
                        row.clear_bundle();
                    }
                    current.expire_current(naming_tick(Instant::now()));
                    assert_eq!(current.rows.len(), 1);
                    assert_eq!(
                        current.rows[0].current_until,
                        (!other_provider).then_some(membership_until)
                    );
                    assert_eq!(current.rows[0].detail, "Original diagnostic facts");
                    assert!(current.rows[0].qualified_bundle.is_none());
                    frame["items"] = current.items(version).unwrap();
                    let admission = naming.load(Ordering::Acquire);
                    socket::view_call_guarded(
                        &endpoint,
                        "integration.view.replace",
                        &frame,
                        deadline(),
                        &stop,
                        &|bytes, id| wire::replaced(bytes, id, "view", 1),
                        &|| {
                            replace_preflight(
                                &shared,
                                "core",
                                Some(&naming),
                                Some(admission),
                                &current.rows,
                            )
                        },
                    )
                    .unwrap();
                    server.join().unwrap();
                });
            }
        }
    }

    #[test]
    fn pre_send_preflight_checks_epoch_and_fails_closed_on_model_contention() {
        let shared = Mutex::new(json!({"server_epoch":"core"}));
        let naming = AtomicU64::new(0);
        let mut row = display_row(
            "observation:opaque".into(),
            "djinn",
            "fact".into(),
            "unknown",
            false,
        );
        row.current_until = Some(naming_tick(Instant::now() + Duration::from_secs(1)));
        let rows = [row];
        let held = shared.lock().unwrap();
        assert_eq!(
            replace_preflight(&shared, "core", Some(&naming), Some(0), &rows)
                .unwrap_err()
                .code,
            REFRESH_NEEDED
        );
        drop(held);
        for revision in [1, 2] {
            naming.store(revision, Ordering::Release);
            assert_eq!(
                replace_preflight(&shared, "core", Some(&naming), Some(0), &[])
                    .unwrap_err()
                    .code,
                REFRESH_NEEDED,
                "empty frames must not bypass admission"
            );
        }
        replace_preflight(&shared, "core", Some(&naming), Some(2), &[]).unwrap();
        shared.lock().unwrap()["server_epoch"] = json!("restarted");
        assert_eq!(
            replace_preflight(&shared, "core", Some(&naming), Some(0), &rows)
                .unwrap_err()
                .code,
            "presentation_server_epoch_changed"
        );
    }

    #[test]
    fn private_current_deadlines_fail_closed_without_hiding_other_providers() {
        let id = format!("observation:{}", "b".repeat(64));
        let mut value = json!({"contract":"coven.normal-observations.v1","server_epoch":"epoch",
            "authorization":"none","omitted_observations":0,"retired_native_observations":0,
            "observations":[
                {"observation_id":"b".repeat(64),"provider":"djinn","origin":"djinn_client_attachment",
                 "reported_state":"unknown","freshness":"unknown","capability":"activity"},
                {"observation_id":"c".repeat(64),"provider":"codex","origin":"v1_receipt",
                 "reported_state":"unknown","freshness":"historical","capability":"reference_only"}]});
        let until = naming_tick(Instant::now() + Duration::from_secs(5));
        for bad in [
            Value::Null,
            json!({}),
            json!([]),
            json!([{"id":id,"until":"invalid"}]),
            json!([{"id":id,"until":0}]),
            json!([{"id":id,"until":until},{"id":id,"until":until}]),
        ] {
            value["_publisher_current"] = bad;
            let model = super::snapshot(&Mutex::new(value.clone()), Some(&id))
                .unwrap()
                .unwrap();
            assert_eq!(model.rows.len(), 1);
            assert_eq!(model.rows[0].label, "Codex");
        }
        value["_publisher_current"] = json!([{"id":id,"until":until}]);
        let shared = Mutex::new(value);
        let mut model = super::snapshot(&shared, Some(&id)).unwrap().unwrap();
        assert_eq!(model.rows.len(), 2);
        assert!(model.rows[0].detail.contains("selected (read-only)"));
        // A contended model cannot refresh or extend this private deadline.
        let _held = shared.lock().unwrap();
        assert!(super::snapshot(&shared, None).unwrap().is_none());
        model.expire_current(until - 1);
        assert_eq!(model.rows.len(), 2);
        model.expire_current(until);
        assert_eq!(model.rows.len(), 1);
        assert_eq!(model.rows[0].label, "Codex");
    }

    #[test]
    fn private_deadline_floor_and_admission_fences_never_extend_membership() {
        let start = Instant::now() + Duration::from_secs(1);
        let end = start + Duration::from_nanos(123_456);
        assert!(naming_tick(end) <= naming_tick(start) + 1);
        let mut model = Model {
            epoch: "epoch".into(),
            rows: vec![
                display_row(
                    "observation:a".into(),
                    "djinn",
                    "fact".into(),
                    "unknown",
                    false,
                ),
                display_row(
                    "observation:b".into(),
                    "codex",
                    "fact".into(),
                    "unknown",
                    false,
                ),
            ],
        };
        model.rows[0].current_until = Some(naming_tick(end));
        model.expire_current(naming_tick(end).saturating_sub(1));
        assert_eq!(model.rows.len(), 2);
        model.expire_current(naming_tick(end));
        assert_eq!(model.rows.len(), 1);
        assert_eq!(model.rows[0].label, "Codex");
    }

    #[test]
    #[cfg(unix)]
    fn naming_refresh_rechecks_epoch_revision_lease_endpoint_and_total_cancel_budget() {
        use std::{
            io::{BufRead, BufReader, Write},
            os::unix::{fs::PermissionsExt, net::UnixListener},
        };
        for mode in [
            "success",
            "generation",
            "wrong_workspace",
            "duplicate_workspace",
            "epoch",
            "revision",
            "removed",
            "expiry",
            "endpoint",
            "admission",
            "current_removed",
            "current_expired",
            "hung",
            "cancel",
        ] {
            let root = tempfile::tempdir().unwrap();
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
            let path = root.path().canonicalize().unwrap().join("kit.sock");
            let listener = UnixListener::bind(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            listener.set_nonblocking(true).unwrap();
            let endpoint = socket::select(path.to_str().unwrap()).unwrap();
            let target = json!({"socket":endpoint.resolved,"device":endpoint.device,"inode":endpoint.inode,
                "terminal_id":"terminal","runtime_generation":"generation"});
            let hint = json!({"id":"observation:opaque","revision":"revision","target":target,
                "until":naming_tick(Instant::now() + Duration::from_secs(2))});
            let count = if mode == "hung" { 128 } else { 1 };
            let shared = Arc::new(Mutex::new(
                json!({"server_epoch":"epoch","_publisher_names":vec![hint; count]}),
            ));
            let fence = Arc::new(AtomicU64::new(0));
            let stop = Arc::new(Stop(AtomicBool::new(false)));
            let mut current = Model {
                epoch: "epoch".into(),
                rows: vec![display_row(
                    "observation:opaque".into(),
                    "djinn",
                    "unchanged fact".into(),
                    "working",
                    true,
                )],
            };
            current.rows[0].label = "Stale name".into();
            current.rows[0].information.context = vec!["Kit branch: stale".into()];
            if matches!(mode, "current_removed" | "current_expired") {
                let until = naming_tick(Instant::now() + Duration::from_secs(2));
                current.rows[0].current_until = Some(until);
                shared.lock().unwrap()["_publisher_current"] =
                    json!([{"id":"observation:opaque","until":until}]);
            }
            let model = shared.clone();
            let revision = fence.clone();
            let cancel = stop.clone();
            let started = Instant::now();
            let worker = thread::spawn(move || {
                let mut replacement = None;
                for step in 0..2 {
                    let end = Instant::now() + Duration::from_secs(1);
                    let mut stream = loop {
                        match listener.accept() {
                            Ok((stream, _)) => break stream,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                assert!(
                                    Instant::now() < end,
                                    "missing naming request: {mode}/{step}"
                                );
                                thread::sleep(Duration::from_millis(1));
                            }
                            Err(e) => panic!("{e}"),
                        }
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut line = String::new();
                    BufReader::new(stream.try_clone().unwrap())
                        .read_line(&mut line)
                        .unwrap();
                    let request: Value = serde_json::from_str(&line).unwrap();
                    let result = if step == 0 {
                        assert_eq!(request["method"], "session.snapshot");
                        let mut snapshot = json!({"type":"session_snapshot","snapshot":{"terminal_navigation":{"version":1},
                            "workspaces":[{"workspace_id":"workspace","label":"Coven","git_branch":"feature/ui"}],
                            "tabs":[{"tab_id":"tab","workspace_id":"workspace"}],
                            "panes":[{"terminal_id":"terminal","pane_id":"pane","tab_id":"tab","workspace_id":"workspace","label":"Manual name"}]}});
                        if mode == "wrong_workspace" {
                            snapshot["snapshot"]["tabs"][0]["workspace_id"] = json!("other");
                        }
                        if mode == "duplicate_workspace" {
                            snapshot["snapshot"]["workspaces"] = json!([
                                {"workspace_id":"workspace","label":"Coven"},
                                {"workspace_id":"workspace","label":"Other"}]);
                        }
                        snapshot
                    } else {
                        assert_eq!(request["method"], "terminal.inspect");
                        // Receiver mutation while the worker is in IO must acquire
                        // the model immediately, not wait on a lookup-held mutex.
                        match mode {
                            "epoch" => model.try_lock().unwrap()["server_epoch"] = json!("new"),
                            "revision" => {
                                model.try_lock().unwrap()["_publisher_names"][0]["revision"] =
                                    json!("new")
                            }
                            "removed" => model.try_lock().unwrap()["_publisher_names"] = json!([]),
                            "expiry" => {
                                model.try_lock().unwrap()["_publisher_names"][0]["until"] =
                                    json!(0);
                            }
                            "admission" => {
                                revision.fetch_or(1, Ordering::AcqRel);
                            }
                            "current_removed" => {
                                model.try_lock().unwrap()["_publisher_current"] = json!([]);
                            }
                            "current_expired" => {
                                model.try_lock().unwrap()["_publisher_current"][0]["until"] =
                                    json!(0);
                            }
                            "endpoint" => {
                                std::fs::remove_file(&path).unwrap();
                                replacement = Some(UnixListener::bind(&path).unwrap());
                                std::fs::set_permissions(
                                    &path,
                                    std::fs::Permissions::from_mode(0o600),
                                )
                                .unwrap();
                            }
                            "hung" => {
                                thread::sleep(Duration::from_millis(400));
                            }
                            "cancel" => cancel.0.store(true, Ordering::Release),
                            _ => {}
                        }
                        json!({"type":"terminal_inspected","terminal_id":"terminal","availability":"live",
                            "runtime_generation":if mode == "generation" {"replaced"} else {"generation"},"pane_id":"pane","tab_id":"tab","workspace_id":"workspace"})
                    };
                    let _ = writeln!(stream, "{}", json!({"id":request["id"],"result":result}));
                }
                drop(replacement);
            });
            let resolution = resolve_names(&shared, &mut current, &endpoint, &stop, Some(&fence));
            if mode == "epoch" {
                assert_eq!(
                    resolution.unwrap_err().code,
                    "presentation_server_epoch_changed"
                );
            } else if matches!(mode, "admission" | "hung" | "cancel" | "endpoint") {
                // Endpoint replacement occurs inside terminal.inspect. Its
                // transport post-receive fence rejects the reply, so no mapping
                // can be qualified and the entire candidate must be deferred.
                assert_eq!(resolution.unwrap_err().code, REFRESH_NEEDED, "mode={mode}");
            } else {
                resolution.unwrap_or_else(|error| {
                    panic!("unexpected lookup outcome: mode={mode}: {error:?}")
                });
            }
            let elapsed = started.elapsed();
            worker.join().unwrap();
            assert!(
                elapsed < Duration::from_millis(500),
                "total naming budget exceeded: {mode}: {elapsed:?}"
            );
            if matches!(mode, "current_removed" | "current_expired") {
                assert!(current.rows.is_empty());
                continue;
            }
            assert_eq!(
                current.rows[0].information.context,
                if mode == "success" {
                    vec!["Kit project: Coven", "Kit branch: feature/ui"]
                } else {
                    vec!["No qualified workspace context."]
                }
            );
            assert_eq!(
                current.rows[0].qualified_bundle.is_some(),
                mode == "success"
            );
            assert_eq!(
                current.rows[0].label,
                if mode == "success" {
                    "Manual name"
                } else {
                    "Djinn"
                },
                "{mode}"
            );
            assert_eq!(current.rows[0].detail, "unchanged fact");
            assert_eq!(current.rows[0].id, "observation:opaque");
        }
    }

    #[test]
    fn explicit_names_reject_unsafe_or_oversize_without_stripping_utf8() {
        for bad in [
            Value::Null,
            json!(""),
            json!(" \t"),
            json!("line\nname"),
            json!("\u{1b}[31mname"),
            json!("name\u{7f}"),
            json!("\u{202e}name"),
            json!("x".repeat(161)),
            json!("\u{754c}".repeat(54)),
        ] {
            assert_eq!(explicit_name(&bad), None);
        }
        for good in ["User / pane", "\u{65e5}\u{672c}\u{8a9e}", " name "] {
            let value = json!(good);
            assert_eq!(explicit_name(&value), Some(good));
        }
        let value = json!("\u{754c}".repeat(53));
        assert_eq!(explicit_name(&value).unwrap().len(), 159);
    }

    #[test]
    fn pane_names_require_unique_current_mapping_and_exact_live_generation() {
        let target = json!({"terminal_id":"terminal","runtime_generation":"generation"});
        let inspected = json!({"type":"terminal_inspected","terminal_id":"terminal",
            "runtime_generation":"generation","availability":"live","pane_id":"moved",
            "tab_id":"tab","workspace_id":"workspace"});
        let pane = json!({"terminal_id":"terminal","pane_id":"moved","tab_id":"tab",
            "workspace_id":"workspace","label":"Manual name","title":"PRIVATE-OSC",
            "agent_name":"PRIVATE-RETIRED","effective_agent_label":"PRIVATE-RETIRED"});
        assert_eq!(
            named_pane(&[pane.clone()], &target, &inspected),
            Some("Manual name")
        );
        for (key, value) in [
            ("runtime_generation", json!("restarted")),
            ("availability", json!("ended")),
            ("terminal_id", json!("other")),
            ("pane_id", json!("old")),
            ("workspace_id", json!("other")),
        ] {
            let mut invalid = inspected.clone();
            invalid[key] = value;
            assert_eq!(named_pane(&[pane.clone()], &target, &invalid), None);
        }
        assert_eq!(
            named_pane(&[pane.clone(), pane.clone()], &target, &inspected),
            None
        );
        let mut alias = pane.clone();
        alias["terminal_id"] = json!("other");
        assert_eq!(
            named_pane(&[pane.clone(), alias], &target, &inspected),
            None
        );
        let mut missing = pane.clone();
        missing.as_object_mut().unwrap().remove("label");
        assert_eq!(named_pane(&[missing], &target, &inspected), None);
    }

    #[derive(Debug, PartialEq)]
    struct EncodedModel {
        rows: Vec<Value>,
    }

    fn snapshot(model: &Mutex<Value>, selected: Option<&str>) -> WireResult<Option<EncodedModel>> {
        super::snapshot(model, selected)?
            .map(|model| {
                Ok(EncodedModel {
                    rows: model.items(1)?.as_array().unwrap().clone(),
                })
            })
            .transpose()
    }

    #[test]
    fn provider_profiles_preserve_receiver_priority_and_strip_private_details() {
        let first = json!({"observation_id":"f".repeat(64),"provider":"opencode","origin":"provider_native_event","profile":"opencode-root.v1","capability":"activity","freshness":"fresh","reported_state":"blocked","fresh_pending_count":2,"unacknowledged_pending_count":1,"request_id":"PRIVATE","cwd":"/PRIVATE"});
        let second = json!({"observation_id":"a".repeat(64),"provider":"pi","origin":"provider_native_event","profile":"pi-context.v1","capability":"activity","freshness":"fresh","reported_state":"working","prompt":"PRIVATE","session_id":"PRIVATE"});
        let value = json!({"contract":"coven.normal-observations.v1","server_epoch":"epoch","authorization":"none","omitted_observations":0,"retired_native_observations":0,"observations":[first,second]});
        let model = snapshot(&Mutex::new(value.clone()), None).unwrap().unwrap();
        assert_eq!(
            model.rows[0]["id"],
            format!("observation:{}", "f".repeat(64))
        );
        let text = serde_json::to_string(&model.rows).unwrap();
        assert!(text.contains("pending 2, unacknowledged 1"));
        assert!(text.contains("UI/RPC context only, root unestablished"));
        assert!(!text.contains("PRIVATE"));
        for (field, bad) in [
            ("provider", json!("pi")),
            ("profile", json!("pi-context.v1")),
            ("profile", Value::Null),
            ("origin", json!("v1_receipt")),
        ] {
            let mut invalid = value.clone();
            invalid["observations"][0][field] = bad;
            assert!(snapshot(&Mutex::new(invalid), None).is_err());
        }
        let mut duplicate = value.clone();
        duplicate["observations"][1]["observation_id"] =
            duplicate["observations"][0]["observation_id"].clone();
        assert!(snapshot(&Mutex::new(duplicate), None).is_err());
    }

    #[test]
    fn attachment_only_ui_is_unknown_source_qualified_and_identity_free() {
        let value = json!({"contract":"coven.normal-observations.v1","server_epoch":"epoch","authorization":"none",
            "omitted_observations":0,"retired_native_observations":0,"observations":[{
                "observation_id":"b".repeat(64),"provider":"djinn","origin":"djinn_client_attachment",
                "reported_state":"unknown","freshness":"unknown","capability":"activity",
                "hosting_state":"local_full_tui_root","association_revision":"PRIVATE-REVISION",
                "client_id":"PRIVATE-CLIENT","instance_id":"PRIVATE-INSTANCE","session_id":"PRIVATE-SID",
                "target":{"socket":"/PRIVATE-SOCKET","terminal_id":"PRIVATE-TERMINAL"}}]});
        let model = snapshot(&Mutex::new(value), None).unwrap().unwrap();
        let text = serde_json::to_string(&model.rows).unwrap();
        assert!(text.contains("UNKNOWN"));
        assert!(text.contains("producer-reported selected local full-TUI root"));
        assert!(!text.contains("PRIVATE"));
        assert_eq!(model.rows[0].as_object().unwrap().len(), 3);
    }

    #[test]
    fn normal_view_is_attributed_and_strips_all_native_details() {
        let value = json!({"contract":"coven.normal-observations.v1","server_epoch":"epoch","authorization":"none","omitted_observations":0,"retired_native_observations":0,"observations":[{
            "observation_id":"b".repeat(64),"provider":"djinn","reported_state":"blocked",
            "origin":"djinn_native_event","freshness":"fresh","capability":"activity",
            "session_id":"PRIVATE-ID","prompt":"PRIVATE-PROMPT","cwd":"/PRIVATE-PATH"}]});
        let model = snapshot(&Mutex::new(value), None).unwrap().unwrap();
        let text = serde_json::to_string(&model.rows).unwrap();
        assert!(text.contains("Coven"));
        assert!(text.contains("Djinn"));
        assert!(text.contains("BLOCKED"));
        assert!(text.contains("caller unverified"));
        assert!(text.contains("no pane binding or success claim"));
        assert!(!text.contains("PRIVATE"));
        assert_eq!(model.rows[0].as_object().unwrap().len(), 3);
    }

    #[test]
    fn normal_view_caps_rows_and_explicitly_displays_omission_and_retirement_counts() {
        let observations: Vec<_> = (0..127).map(|n| json!({"observation_id":format!("{n:064x}"),
            "provider":"djinn","origin":"djinn_native_event","capability":"activity","freshness":"unknown","reported_state":"unknown"})).collect();
        let model = Mutex::new(
            json!({"contract":"coven.normal-observations.v1","server_epoch":"epoch","authorization":"none",
            "omitted_observations":129,"retired_native_observations":300,"observations":observations}),
        );
        let view = snapshot(&model, None).unwrap().unwrap();
        assert_eq!(view.rows.len(), 128);
        let summary = view
            .rows
            .iter()
            .find(|r| r["id"] == "normal:limits")
            .unwrap();
        assert!(
            summary["detail"]
                .as_str()
                .unwrap()
                .contains("129 observations omitted")
        );
        assert!(
            summary["detail"]
                .as_str()
                .unwrap()
                .contains("300 native observations retired")
        );
        model.lock().unwrap()["observations"][0]["provider"] = json!("opencode");
        assert!(snapshot(&model, None).is_err());
    }

    fn projection() -> Value {
        json!({"contract":"coven.agent-management.v1","server_epoch":"server-1",
            "authorization":"none","records":[{"record_id":"a".repeat(64),"provider":"claude",
            "reported_state":"working","last_event":"PreToolUse","age_ms":1999,
            "reference_trust":"caller_supplied_unverified","native_identity":"unestablished",
            "job_completion":"unestablished","native_sid":"DO-NOT-COPY","cwd":"/PRIVATE"}]})
    }

    #[test]
    fn compact_status_is_truthful_versioned_and_private() {
        for (state, _word, status) in [
            ("working", "Working", Some("running")),
            ("blocked", "Blocked", Some("attention")),
            ("idle", "Idle", None),
            ("ended", "Ended", None),
            ("unknown", "Unknown", Some("unknown")),
        ] {
            for freshness in ["fresh", "expired", "historical", "unknown"] {
                let value = Mutex::new(
                    json!({"contract":"coven.normal-observations.v1","server_epoch":"epoch","authorization":"none",
                    "omitted_observations":0,"retired_native_observations":0,"observations":[{
                        "observation_id":"a".repeat(64),"provider":"djinn","origin":"djinn_native_event",
                        "capability":"activity","freshness":freshness,"reported_state":state,
                        "session_id":"PRIVATE","prompt":"PRIVATE","tool":"PRIVATE","error":"PRIVATE","cwd":"/PRIVATE"}]}),
                );
                let model =
                    super::snapshot(&value, Some(&format!("observation:{}", "a".repeat(64))))
                        .unwrap()
                        .unwrap();
                let expected = if freshness == "fresh" {
                    status
                } else {
                    Some("unknown")
                };
                for version in [1, 2] {
                    let items = model.items(version).unwrap();
                    let row = &items[0];
                    assert_eq!(
                        row.as_object().unwrap().len(),
                        if version == 1 { 3 } else { 4 }
                    );
                    assert!(
                        row["detail"]
                            .as_str()
                            .unwrap()
                            .contains("selected (read-only)")
                    );
                    assert!(!items.to_string().contains("PRIVATE"));
                    assert_eq!(row["label"], "Djinn");
                    assert!(
                        row["detail"]
                            .as_str()
                            .unwrap()
                            .contains(&state.to_ascii_uppercase())
                    );
                    if version == 2 {
                        assert_eq!(row["presentation"]["label"], "");
                        let sidebar = format!(
                            "{} {}",
                            row["label"].as_str().unwrap(),
                            row["presentation"]["label"].as_str().unwrap()
                        );
                        assert_eq!(sidebar.matches("Djinn").count(), 1);
                        assert_eq!(sidebar.matches("aaaaaaaa").count(), 0);
                        assert_eq!(row["presentation"]["status"].as_str(), expected);
                        assert_eq!(
                            row["presentation"]
                                .as_object()
                                .unwrap()
                                .contains_key("status"),
                            expected.is_some()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn compact_collisions_limits_and_field_budget_are_bounded() {
        let mut model = Model {
            epoch: "epoch".into(),
            rows: vec![
                display_row(
                    format!("agent:{}", "a".repeat(64)),
                    "djinn",
                    "x".repeat(512),
                    "idle",
                    true,
                ),
                display_row(
                    format!("agent:{}b", "a".repeat(63)),
                    "djinn",
                    "x".repeat(512),
                    "ended",
                    true,
                ),
            ],
        };
        for version in [1, 2] {
            let items = model.items(version).unwrap();
            assert_eq!(items[0]["label"], "Djinn");
            assert_eq!(items[1]["label"], "Djinn");
            assert_ne!(items[0]["id"], items[1]["id"]);
            assert!(serde_json::to_vec(&items).unwrap().len() < 96 * 1024);
        }
        model.rows[0].detail.push('x');
        assert!(model.items(1).is_err());
        assert!(model.items(2).is_err());
    }

    #[test]
    fn full_frames_and_near_cap_profile_details_preserve_budgets() {
        let observations: Vec<_> = (0..127).map(|n| json!({
            "observation_id":format!("{n:064x}"),"provider":"opencode","origin":"provider_native_event",
            "profile":"opencode-root.v1","reported_state":"blocked","capability":"activity","freshness":"fresh",
            "hosting_state":"local_full_tui_root","fresh_pending_count":128,"unacknowledged_pending_count":128,
            "session_id":"PRIVATE","prompt":"PRIVATE","tools":"PRIVATE","cwd":"/PRIVATE","error":"PRIVATE"
        })).collect();
        let value = Mutex::new(
            json!({"contract":"coven.normal-observations.v1","server_epoch":"epoch",
            "authorization":"none","omitted_observations":u64::MAX,"retired_native_observations":u64::MAX,
            "observations":observations}),
        );
        let selected = format!("observation:{:064x}", 0);
        let mut model = super::snapshot(&value, Some(&selected)).unwrap().unwrap();
        for row in &mut model.rows {
            if row.id != "normal:limits" {
                row.label = "\\".repeat(160);
            }
        }
        for version in [1, 2] {
            let named_items: Vec<_> = model
                .rows
                .iter()
                .map(|row| {
                    let mut item = json!({"id":row.id,"label":row.label,"detail":row.detail});
                    if version == 2 {
                        item["presentation"] = json!({"label":""});
                        if let Some(status) = row.status {
                            item["presentation"]["status"] = json!(status);
                        }
                    }
                    item
                })
                .collect();
            let needs_fallback = serde_json::to_vec(&named_items).unwrap().len() > 96 * 1024 - 512;
            let items = model.items(version).unwrap();
            assert_eq!(items.as_array().unwrap().len(), 128);
            for (item, row) in items.as_array().unwrap().iter().zip(&model.rows) {
                assert_eq!(
                    item["label"],
                    if needs_fallback {
                        row.fallback
                    } else {
                        &row.label
                    }
                );
                assert!(item["label"].as_str().unwrap().len() <= 160);
            }
            assert!(
                items[0]["detail"]
                    .as_str()
                    .unwrap()
                    .contains("selected (read-only)")
            );
            for row in items.as_array().unwrap() {
                assert!(row["detail"].as_str().unwrap().len() <= 512);
            }
            let notice = &items[127];
            assert_eq!(notice["id"], "normal:limits");
            assert_eq!(notice["label"], "Evidence limits");
            if version == 2 {
                assert_eq!(notice["presentation"]["label"], "");
            }
            assert!(notice["presentation"].get("status").is_none());
            let frame = json!({"source":"a".repeat(64),"lease":"l".repeat(128),"epoch":"e".repeat(128),
                "revision":u64::MAX,"items":items});
            let bytes = serde_json::to_vec(&frame).unwrap();
            assert!(bytes.len() <= 96 * 1024);
            assert!(!String::from_utf8(bytes).unwrap().contains("PRIVATE"));
        }
    }

    #[test]
    fn fresh_references_and_hosting_metadata_remain_unknown_in_both_profiles() {
        for (provider, origin, capability) in [
            ("djinn", "djinn_client_attachment", "activity"),
            ("codex", "v1_receipt", "reference_only"),
            ("copilot", "v1_receipt", "reference_only"),
            ("pi", "v1_receipt", "reference_only"),
            ("opencode", "v1_receipt", "reference_only"),
        ] {
            let value = Mutex::new(
                json!({"contract":"coven.normal-observations.v1","server_epoch":"epoch",
                "authorization":"none","omitted_observations":0,"retired_native_observations":0,"observations":[{
                    "observation_id":"a".repeat(64),"provider":provider,"origin":origin,"capability":capability,
                    "freshness":"fresh","reported_state":"working","hosting_state":"local_full_tui_root"}]}),
            );
            let model = super::snapshot(&value, None).unwrap().unwrap();
            assert_eq!(
                model.items(2).unwrap()[0]["presentation"]["status"],
                "unknown"
            );
            assert_eq!(
                model.items(1).unwrap()[0]["label"],
                model.items(2).unwrap()[0]["label"]
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn native_fact_event_names_never_become_outcome_or_activity() {
        for provider in ["codex", "copilot"] {
            for event in agent_management::facts::events(provider).unwrap() {
                let row = json!({"observation_id":"b".repeat(64),"provider":provider,
                    "profile":format!("{provider}-hook-facts.v1"),"origin":"provider_native_fact",
                    "capability":"native_hook_fact","reported_state":"unknown","age_ms":0,
                    "freshness":if provider == "codex" {"unknown"} else {"fresh"},
                    "reference_trust":"caller_supplied_unverified","root_eligibility":"unestablished",
                    "native_identity":"unestablished","job_completion":"unestablished","binding":"none",
                    "attribution":"Coven","last_received_event":event,"ordering":"unordered",
                    "time_basis":if provider == "codex" {"local_receipt_only"} else {"source_age_at_receipt_plus_monotonic"},
                    "source_time":if provider == "codex" {"unknown"} else {"caller_supplied_utc"}});
                let value = Mutex::new(
                    json!({"contract":"coven.normal-observations.v1","server_epoch":"epoch",
                    "authorization":"none","omitted_observations":0,"retired_native_observations":0,"observations":[row]}),
                );
                let model = super::snapshot(&value, None).unwrap().unwrap();
                assert_eq!(
                    model.items(2).unwrap()[0]["presentation"]["status"],
                    "unknown"
                );
                assert_eq!(
                    model.items(1).unwrap()[0]["label"],
                    model.items(2).unwrap()[0]["label"]
                );
                let v3 = model.items(3).unwrap();
                assert_eq!(v3[0]["presentation"]["status"], "unknown");
                assert_eq!(
                    v3[0]["information"]["activity"][0],
                    "Current activity is not available from this report type."
                );
                assert_eq!(
                    v3[0]["information"]["attention"][0],
                    "Attention information unavailable from this report."
                );
            }
        }
    }

    #[test]
    fn projection_is_bounded_read_only_and_age_bucketed() {
        let model = Mutex::new(projection());
        let before = snapshot(&model, None).unwrap().unwrap();
        assert_eq!(
            before.rows[0]["detail"],
            "Last report: WORKING \u{b7} age 1s"
        );
        model.lock().unwrap()["records"][0]["age_ms"] = json!(1000);
        assert_eq!(snapshot(&model, None).unwrap().unwrap(), before);
        let selected = snapshot(&model, before.rows[0]["id"].as_str())
            .unwrap()
            .unwrap();
        assert!(
            selected.rows[0]["detail"]
                .as_str()
                .unwrap()
                .contains("Selected (read-only)")
        );
        let serialized = serde_json::to_string(&selected.rows).unwrap();
        assert!(!serialized.contains("DO-NOT-COPY"));
        assert!(!serialized.contains("PRIVATE"));
        assert_eq!(
            model.lock().unwrap()["records"][0]["reported_state"],
            "working"
        );
        model.lock().unwrap()["records"][0]["age_ms"] = Value::Null;
        let old = snapshot(&model, None).unwrap().unwrap();
        assert_eq!(
            old.rows[0]["detail"],
            "Last report: UNKNOWN \u{b7} age unavailable/unknown"
        );
        let guard = model.lock().unwrap();
        assert!(snapshot(&model, None).unwrap().is_none());
        drop(guard);
        model.lock().unwrap()["records"] = json!(vec![projection()["records"][0].clone(); 129]);
        assert_eq!(
            snapshot(&model, None).unwrap_err().code,
            "integration_view_invalid_frame"
        );
    }

    #[test]
    fn codex_rows_are_dynamic_and_never_claim_activity() {
        let mut value = projection();
        value["records"][0]["provider"] = json!("codex");
        value["records"][0]["last_event"] = json!("SessionStart");
        value["records"][0]["reported_state"] = json!("unknown");
        let model = Mutex::new(value);
        let view = snapshot(&model, None).unwrap().unwrap();
        assert!(view.rows[0]["label"].as_str().unwrap().eq("Codex"));
        assert!(
            view.rows[0]["detail"]
                .as_str()
                .unwrap()
                .starts_with("Last report: UNKNOWN")
        );
        for state in ["working", "idle", "ended"] {
            model.lock().unwrap()["records"][0]["reported_state"] = json!(state);
            assert!(snapshot(&model, None).is_err());
        }
        model.lock().unwrap()["records"][0]["reported_state"] = json!("unknown");
        model.lock().unwrap()["records"][0]["last_event"] = json!("Stop");
        assert!(snapshot(&model, None).is_err());
    }

    #[test]
    fn copilot_rows_are_labeled_reference_only_even_when_fresh() {
        let mut value = projection();
        value["records"][0]["provider"] = json!("copilot");
        value["records"][0]["last_event"] = json!("SessionStart");
        value["records"][0]["reported_state"] = json!("unknown");
        let model = Mutex::new(value);
        let view = snapshot(&model, Some(&format!("agent:{}", "a".repeat(64))))
            .unwrap()
            .unwrap();
        assert_eq!(view.rows[0]["label"], "Copilot");
        let detail = view.rows[0]["detail"].as_str().unwrap();
        assert!(detail.starts_with("Last report: UNKNOWN \u{b7} age 1s"));
        assert!(detail.contains("reference: caller supplied, unverified"));
        assert!(detail.contains("native identity and job completion: unestablished"));
        let serialized = serde_json::to_string(&view.rows).unwrap();
        assert!(!serialized.contains("DO-NOT-COPY"));
        assert!(!serialized.contains("PRIVATE"));
        model.lock().unwrap()["records"][0]["age_ms"] = Value::Null;
        assert!(
            snapshot(&model, None).unwrap().unwrap().rows[0]["detail"]
                .as_str()
                .unwrap()
                .contains("unavailable/unknown")
        );
        for state in ["working", "idle", "ended"] {
            model.lock().unwrap()["records"][0]["reported_state"] = json!(state);
            assert!(snapshot(&model, None).is_err());
        }
        model.lock().unwrap()["records"][0]["reported_state"] = json!("unknown");
        for event in ["sessionStart", "Stop", "SubagentStop", "PreToolUse"] {
            model.lock().unwrap()["records"][0]["last_event"] = json!(event);
            assert!(snapshot(&model, None).is_err());
        }
    }

    #[test]
    fn cancelled_before_readiness_does_not_connect() {
        let stop = Arc::new(Stop(AtomicBool::new(false)));
        let cancel = stop.clone();
        let worker = thread::spawn(move || {
            worker_report(
                &PresentationRequest {
                    socket: "not-even-a-path".into(),
                    source: "invalid source".into(),
                },
                &Mutex::new(Value::Null),
                &cancel,
                None,
            )
        });
        thread::sleep(Duration::from_millis(40));
        stop.0.store(true, Ordering::Release);
        assert_eq!(worker.join().unwrap()["outcome"], "completed");
    }

    #[test]
    fn extension_rows_are_labeled_unverified_references_and_never_activity() {
        for (provider, event, label, other_event) in [
            ("pi", "session_start", "Pi", "session.created"),
            ("opencode", "session.created", "OpenCode", "session_start"),
            ("opencode", "session.updated", "OpenCode", "session_start"),
        ] {
            let mut value = projection();
            value["records"][0]["provider"] = json!(provider);
            value["records"][0]["last_event"] = json!(event);
            value["records"][0]["reported_state"] = json!("unknown");
            let model = Mutex::new(value);
            let view = snapshot(&model, Some(&format!("agent:{}", "a".repeat(64))))
                .unwrap()
                .unwrap();
            assert_eq!(view.rows[0]["label"], label);
            let detail = view.rows[0]["detail"].as_str().unwrap();
            assert!(detail.starts_with("Last report: UNKNOWN \u{b7} age 1s"));
            assert!(detail.contains("reference: caller supplied, unverified"));
            assert!(detail.contains("native identity and job completion: unestablished"));
            let serialized = serde_json::to_string(&view.rows).unwrap();
            assert!(!serialized.contains("DO-NOT-COPY"));
            assert!(!serialized.contains("PRIVATE"));
            assert_eq!(view.rows[0].as_object().unwrap().len(), 3);
            assert!(!serialized.contains("pane"));
            for state in ["working", "idle", "ended"] {
                model.lock().unwrap()["records"][0]["reported_state"] = json!(state);
                assert!(snapshot(&model, None).is_err());
            }
            model.lock().unwrap()["records"][0]["reported_state"] = json!("unknown");
            model.lock().unwrap()["records"][0]["age_ms"] = Value::Null;
            assert!(
                snapshot(&model, None).unwrap().unwrap().rows[0]["detail"]
                    .as_str()
                    .unwrap()
                    .contains("unavailable/unknown")
            );
            for event in [
                "SessionStart",
                "Stop",
                "SubagentStop",
                "PreToolUse",
                "other",
                other_event,
            ] {
                model.lock().unwrap()["records"][0]["last_event"] = json!(event);
                assert!(snapshot(&model, None).is_err());
            }
        }
    }
}
