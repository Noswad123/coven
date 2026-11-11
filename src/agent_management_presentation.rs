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
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

pub struct PresentationRequest {
    pub socket: String,
    pub source: String,
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
            let report = worker_report(&request, &model, &cancel);
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

fn worker_report(request: &PresentationRequest, model: &Mutex<Value>, stop: &Stop) -> Value {
    let mut report = json!({"outcome":"completed","authorization":"none",
        "selection":"read_only_reported_record_details","frames":0,"polls":0,
        "cleanup":"not_opened","diagnostics":[]});
    if let Err(error) = run(request, model, stop, &mut report) {
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
    rows: Vec<Value>,
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
    if epoch.is_empty()
        || epoch.len() > 128
        || epoch.chars().any(char::is_control)
        || value["contract"] != "coven.agent-management.v1"
        || value["authorization"] != "none"
    {
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
        let label = match provider {
            "pi" => "Pi",
            "opencode" => "OpenCode",
            _ => provider,
        };
        rows.push(
            json!({"id":item_id,"label":format!("{label} \u{b7} {}", &id[..12]),"detail":detail}),
        );
    }
    rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    if rows.windows(2).any(|pair| pair[0]["id"] == pair[1]["id"]) {
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
fn pause(stop: &Stop, duration: Duration) {
    let end = Instant::now() + duration;
    while !stop.interrupted() && Instant::now() < end {
        thread::sleep(Duration::from_millis(10).min(end.saturating_duration_since(Instant::now())));
    }
}

fn run(
    request: &PresentationRequest,
    model: &Mutex<Value>,
    stop: &Stop,
    report: &mut Value,
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
    wire::snapshot(&reply, &request.source)?;
    let opened = socket::view_call(
        &endpoint,
        "integration.view.open",
        &json!({"source":request.source,"version":1}),
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
        loop {
            if stop.interrupted() {
                return Ok(());
            }
            if let Some(next) = snapshot(model, selected.as_deref())? {
                if next.epoch != server_epoch {
                    return Err(Failure::new("presentation_server_epoch_changed"));
                }
                current = next;
            }
            let items = json!(current.rows);
            if items != shown {
                revision = revision
                    .checked_add(1)
                    .ok_or_else(|| Failure::new("integration_view_invalid_frame"))?;
                let mut frame = owner.clone();
                frame["revision"] = json!(revision);
                frame["items"] = items.clone();
                if serde_json::to_vec(&frame)
                    .map_err(|_| Failure::new("integration_view_invalid_frame"))?
                    .len()
                    > 96 * 1024
                {
                    return Err(Failure::new("integration_view_invalid_frame"));
                }
                socket::view_call(
                    &endpoint,
                    "integration.view.replace",
                    &frame,
                    deadline(),
                    stop,
                    &|bytes, id| wire::replaced(bytes, id, epoch, revision),
                )?;
                report["frames"] = json!(revision);
                shown = items;
            }
            pause(stop, Duration::from_millis(250));
            if stop.interrupted() {
                return Ok(());
            }
            let polled = socket::view_call(
                &endpoint,
                "integration.view.poll",
                &owner,
                deadline(),
                stop,
                &|bytes, id| wire::polled(bytes, id, epoch, revision),
            )?;
            report["polls"] = json!(report["polls"].as_u64().unwrap_or(0) + 1);
            for intent in polled["intents"].as_array().expect("validated intents") {
                if intent["epoch"] == epoch
                    && intent["revision"] == revision
                    && current
                        .rows
                        .iter()
                        .any(|row| row["id"] == intent["item_id"])
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
mod tests {
    use super::*;

    fn projection() -> Value {
        json!({"contract":"coven.agent-management.v1","server_epoch":"server-1",
            "authorization":"none","records":[{"record_id":"a".repeat(64),"provider":"claude",
            "reported_state":"working","last_event":"PreToolUse","age_ms":1999,
            "reference_trust":"caller_supplied_unverified","native_identity":"unestablished",
            "job_completion":"unestablished","native_sid":"DO-NOT-COPY","cwd":"/PRIVATE"}]})
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
        assert!(
            view.rows[0]["label"]
                .as_str()
                .unwrap()
                .starts_with("codex ")
        );
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
        assert_eq!(view.rows[0]["label"], "copilot \u{b7} aaaaaaaaaaaa");
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
            assert_eq!(
                view.rows[0]["label"],
                format!("{label} \u{b7} aaaaaaaaaaaa")
            );
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
