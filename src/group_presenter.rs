//! Foreground declaration presentation. Selection has no execution authority.
use crate::{Result, error::require, observation_socket as socket, task_mutation::TaskOutput};
use serde_json::{Value, json};
use socket::{Delivery, Failure, WireResult};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

mod wire;

pub struct Request {
    pub workspace: PathBuf,
    pub group: String,
    pub socket: String,
    pub source: String,
    pub duration: u64,
}

/// Invalid operands return errors. Operational failures return a bounded report.
pub fn arguments(request: &Request, json_output: bool) -> Result<TaskOutput> {
    socket::validate_path(&request.socket)?;
    crate::groups::selector(&request.group)?;
    crate::groups::source(&request.source)?;
    require((1..=3600).contains(&request.duration), "invalid_duration")?;
    let started = Instant::now();
    let end = started + Duration::from_secs(request.duration);
    let mut report = json!({
        "contract":"coven.group-presentation.v1", "schema_version":1, "outcome":"completed",
        "authorization":"none", "dispatchable":false,
        "delivery":"ephemeral_ui_only", "ttl_ms":5000,
        "selection":"read_only_declared_participant_details", "source_routing":"not_authentication",
        "frames":0, "polls":0, "selections":0, "selected_item_id":null,
        "ignored_intents":0, "cleanup":"not_opened", "diagnostics":[],
        "requested_duration_seconds":request.duration
    });
    let result = run(request, end, &mut report);
    if let Err(failure) = result {
        report["outcome"] = json!(if failure.delivery == Delivery::Uncertain {
            "uncertain"
        } else if failure.interrupted {
            "interrupted"
        } else {
            "failed"
        });
        report["diagnostics"] = json!([failure.code]);
    }
    report["duration_ms"] = json!(started.elapsed().as_millis() as u64);
    let mut output = crate::session_support::output(&report, !json_output)?;
    output.exit_code = match report["outcome"].as_str() {
        Some("completed") => 0,
        Some("interrupted") => 130,
        _ => 2,
    };
    Ok(output)
}

fn deadline(end: Instant) -> Instant {
    end.min(Instant::now() + Duration::from_secs(3))
}

fn group(request: &Request) -> WireResult<(String, Vec<Value>)> {
    let view = crate::groups::read_groups(&request.workspace)
        .map_err(|_| Failure::new("group_state_error"))?;
    let declaration = view
        .group(&request.group)
        .map_err(|_| Failure::new("group_not_found"))?;
    // Project declaration text only, never bindings, attachments or native references.
    let field = |value: &crate::values::Value, key: &str| -> WireResult<String> {
        value
            .as_object()
            .and_then(|o| o.get(key))
            .and_then(crate::values::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| Failure::new("group_state_error"))
    };
    let members = declaration
        .as_object()
        .and_then(|o| o.get("members"))
        .and_then(crate::values::Value::as_array)
        .ok_or_else(|| Failure::new("group_state_error"))?;
    if members.len() > 127 {
        return Err(Failure::new("group_size"));
    }
    let mut rows = vec![json!({
        "id":format!("g:{}", field(declaration, "group_id")?),
        "label":field(declaration, "name")?,
        "detail":format!("{} declared participants", members.len())
    })];
    for member in members {
        rows.push(json!({
            "id":format!("p:{}", field(member, "participant_id")?),
            "label":field(member, "key")?,
            "detail":format!("{} \u{b7} declared participant", field(member, "role")?)
        }));
    }
    for row in &rows {
        for (key, max, nonblank) in [
            ("id", 128, true),
            ("label", 160, true),
            ("detail", 512, false),
        ] {
            let text = row[key]
                .as_str()
                .ok_or_else(|| Failure::new("integration_view_invalid_frame"))?;
            if text.len() > max
                || (nonblank && text.trim().is_empty())
                || text.chars().any(char::is_control)
            {
                return Err(Failure::new("integration_view_invalid_frame"));
            }
        }
    }
    Ok((view.revision().into(), rows))
}

fn current(request: &Request, revision: &str) -> WireResult<()> {
    let (now, _) = group(request)?;
    if now != revision {
        return Err(Failure::new("group_revision_changed"));
    }
    Ok(())
}

fn count(report: &mut Value, key: &str) {
    report[key] = json!(report[key].as_u64().unwrap_or(0) + 1);
}

fn run(request: &Request, end: Instant, report: &mut Value) -> WireResult<()> {
    let interrupt =
        socket::InterruptGuard::install().map_err(|_| Failure::new("interrupt_guard_error"))?;
    let (frozen, rows) = group(request)?;
    report["participants"] = json!(rows.len() - 1);
    socket::check(end, &interrupt)?;
    let endpoint = socket::select(&request.socket)?;
    let reply = socket::read(
        &request.socket,
        socket::ReadMethod::Snapshot,
        &json!({}),
        deadline(end),
        &mut socket::ReadBudget::new(1024 * 1024),
        &interrupt,
    )?;
    if reply.endpoint != endpoint {
        return Err(Failure::new("endpoint_replaced"));
    }
    wire::snapshot(&reply, &request.source)?;
    current(request, &frozen)?;
    let opened = socket::view_call(
        &endpoint,
        "integration.view.open",
        &json!({"source":request.source,"version":1}),
        deadline(end),
        &interrupt,
        &|bytes, id| wire::opened(bytes, id),
    );
    let opened = match opened {
        Ok(value) => value,
        Err(error) => {
            if error.delivery == Delivery::Uncertain {
                report["cleanup"] = json!("ttl_only_unknown_lease");
            }
            return Err(error);
        }
    };
    let owner = json!({"source":request.source,"lease":opened["lease"],"epoch":opened["epoch"]});
    let epoch = opened["epoch"].as_str().expect("validated epoch");
    report["cleanup"] = json!("ttl_only");
    // Reserve a small part of the overall duration for a single graceful close.
    let active_end = end - Duration::from_millis(100);
    let result = (|| {
        let mut revision = 1u64;
        let mut shown = rows.clone();
        let mut publish = true;
        loop {
            socket::check(end, &interrupt)?;
            if Instant::now() >= active_end {
                break;
            }
            current(request, &frozen)?;
            if publish {
                let mut frame = owner.clone();
                frame["revision"] = json!(revision);
                frame["items"] = json!(shown);
                socket::view_call(
                    &endpoint,
                    "integration.view.replace",
                    &frame,
                    deadline(active_end),
                    &interrupt,
                    &|bytes, id| wire::replaced(bytes, id, epoch, revision),
                )?;
                count(report, "frames");
                publish = false;
            }
            let wait_until = active_end.min(Instant::now() + Duration::from_millis(250));
            while Instant::now() < wait_until && !interrupt.interrupted() {
                std::thread::sleep(
                    Duration::from_millis(10)
                        .min(wait_until.saturating_duration_since(Instant::now())),
                );
            }
            socket::check(end, &interrupt)?;
            if Instant::now() >= active_end {
                break;
            }
            current(request, &frozen)?;
            let polled = socket::view_call(
                &endpoint,
                "integration.view.poll",
                &owner,
                deadline(active_end),
                &interrupt,
                &|bytes, id| wire::polled(bytes, id, epoch, revision),
            )?;
            count(report, "polls");
            current(request, &frozen)?;
            let mut selected = None;
            for intent in polled["intents"].as_array().expect("validated intents") {
                let found = rows
                    .iter()
                    .skip(1)
                    .position(|row| row["id"] == intent["item_id"]);
                if intent["epoch"] == epoch && intent["revision"] == revision && found.is_some() {
                    selected = found.map(|index| index + 1);
                    count(report, "selections");
                } else {
                    count(report, "ignored_intents");
                }
            }
            if let Some(index) = selected {
                let mut next = rows.clone();
                next[index]["detail"] = json!(format!(
                    "Selected (read-only): {}",
                    rows[index]["detail"].as_str().unwrap()
                ));
                if next[index]["detail"].as_str().unwrap().len() > 512 {
                    return Err(Failure::new("integration_view_invalid_frame"));
                }
                report["selected_item_id"] = rows[index]["id"].clone();
                if next != shown {
                    shown = next;
                    revision += 1;
                    publish = true;
                }
            }
        }
        Ok(())
    })();
    // Unknown delivery may include a drained poll or admitted frame. Never replay
    // or refresh it; let the server's lease TTL clean up this uncertain session.
    if result
        .as_ref()
        .err()
        .is_some_and(|e: &Failure| e.delivery == Delivery::Uncertain)
    {
        report["cleanup"] = json!("ttl_only_uncertain");
        return result;
    }
    if Instant::now() < end {
        match socket::view_call(
            &endpoint,
            "integration.view.close",
            &owner,
            deadline(end),
            &socket::NoCancel,
            &|bytes, id| wire::closed(bytes, id),
        ) {
            Ok(_) => report["cleanup"] = json!("closed"),
            Err(error) => {
                report["cleanup"] = json!(if error.delivery == Delivery::Uncertain {
                    "uncertain"
                } else {
                    "ttl_only"
                });
                if result.is_ok() || error.delivery == Delivery::Uncertain {
                    return Err(error);
                }
            }
        }
    }
    result
}
