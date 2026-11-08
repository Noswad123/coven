//! Serial foreground observations. Timer completion is not native job completion.
use crate::observation_socket::{Cancel, Failure, InterruptGuard, WireResult};
use crate::session_inspection::{self, Request};
use crate::task_mutation::TaskOutput;
use crate::{Result, error::require};
use serde_json::{Value, json};
use std::io::Write;
use std::time::{Duration, Instant};

fn validate(duration: f64, interval: f64) -> Result<()> {
    require(
        duration.is_finite() && (0.1..=3600.0).contains(&duration),
        "invalid_watch_duration",
    )?;
    require(
        interval.is_finite() && (0.1..=60.0).contains(&interval),
        "invalid_watch_interval",
    )
}

fn observation(report: &Value) -> bool {
    if report["schema_version"].as_u64() != Some(1)
        || report["contract"] != "coven.session-inspection.v1"
        || !matches!(
            report["assessment"].as_str(),
            Some("observed" | "unavailable" | "incompatible")
        )
        || !report["diagnostics"]
            .as_array()
            .is_some_and(|a| a.iter().all(Value::is_string))
        || report["authorization"] != "none"
        || report["dispatchable"] != false
        || report["native_catalog_complete"] != false
        || report["runtime_fact_freshness"] != "unknown"
        || [
            "native_identity",
            "native_lifecycle",
            "native_worker_acceptance",
            "job_completion",
        ]
        .iter()
        .any(|k| report[k] != "unestablished")
    {
        return false;
    }
    let Some(targets) = report["targets"].as_array() else {
        return false;
    };
    let declarations = &report["declarations"];
    if !declarations["unmatched"].is_array()
        || report.get("target_count").is_none()
        || report.get("recognized_target_count").is_none()
    {
        return false;
    }
    if report["assessment"] == "observed" {
        report["target_count"].as_u64() == Some(targets.len() as u64)
            && report["recognized_target_count"].as_u64()
                == Some(
                    targets
                        .iter()
                        .filter(|t| t["recognition"] == "djinn_executable_family")
                        .count() as u64,
                )
            && matches!(
                declarations["assessment"].as_str(),
                Some("observed" | "uninitialized")
            )
            && targets.iter().all(|t| {
                t.is_object()
                    && t["associations"].is_array()
                    && matches!(
                        t["recognition"].as_str(),
                        Some("unknown" | "djinn_executable_family")
                    )
                    && t["activity"] == "unknown"
                    && t["attention"] == "unknown"
                    && ["native_identity", "native_lifecycle", "job_completion"]
                        .iter()
                        .all(|k| t[k] == "unestablished")
            })
    } else {
        targets.is_empty()
            && report["target_count"].is_null()
            && report["recognized_target_count"].is_null()
            && declarations["assessment"] == "unavailable"
            && declarations["unmatched"]
                .as_array()
                .is_some_and(Vec::is_empty)
            && [
                "revision",
                "authority_schema_version",
                "other_endpoint_count",
                "outside_inspection_count",
            ]
            .iter()
            .all(|k| declarations[k].is_null())
    }
}

trait Timer {
    fn now(&self) -> WireResult<f64>;
    fn sleep(&self, seconds: f64) -> WireResult<()>;
}
struct SystemTimer(Instant);
impl Timer for SystemTimer {
    fn now(&self) -> WireResult<f64> {
        Ok(self.0.elapsed().as_secs_f64())
    }
    fn sleep(&self, seconds: f64) -> WireResult<()> {
        // Short sleeps let SIGINT stop a watch without waiting a full interval.
        std::thread::sleep(Duration::from_secs_f64(seconds.min(0.05)));
        Ok(())
    }
}
fn now(timer: &dyn Timer, previous: &mut Option<f64>, cancel: &dyn Cancel) -> WireResult<f64> {
    if cancel.interrupted() {
        return Err(Failure::new("interrupted"));
    }
    let value = timer.now().map_err(|e| {
        if e.interrupted {
            e
        } else {
            Failure::new("clock_failed")
        }
    })?;
    if !value.is_finite() || previous.is_some_and(|p| value < p) {
        return Err(Failure::new("clock_failed"));
    }
    *previous = Some(value);
    Ok(value)
}
fn fail(summary: &mut Value, outcome: &str, diagnostic: &str) {
    summary["outcome"] = json!(outcome);
    summary["diagnostics"] = json!([diagnostic]);
}
fn increment(summary: &mut Value, key: &str) {
    summary[key] = json!(summary[key].as_u64().unwrap() + 1);
}

fn watch(
    collect: &mut dyn FnMut(f64) -> WireResult<Value>,
    emit: &mut dyn FnMut(&Value) -> WireResult<()>,
    duration: f64,
    interval: f64,
    timer: &dyn Timer,
    cancel: &dyn Cancel,
) -> Value {
    let mut summary = json!({"schema_version":1,"contract":"coven.session-watch-summary.v1","outcome":"completed",
        "poll_attempts":0,"emitted":0,"observed_samples":0,"unavailable_samples":0,"incompatible_samples":0,"diagnostics":[],
        "authorization":"none","dispatchable":false,"native_catalog_complete":false,
        "native_identity":"unestablished","native_lifecycle":"unestablished","native_worker_acceptance":"unestablished","job_completion":"unestablished"});
    let outcome = (|| -> WireResult<()> {
        let mut previous = None;
        let started = now(timer, &mut previous, cancel)?;
        let deadline = started + duration;
        if !deadline.is_finite() || deadline <= started {
            return Err(Failure::new("clock_failed"));
        }
        let mut current = started;
        while current < deadline {
            if cancel.interrupted() {
                return Err(Failure::new("interrupted"));
            }
            increment(&mut summary, "poll_attempts");
            let collected = collect(deadline - current);
            if cancel.interrupted() || collected.as_ref().err().is_some_and(|e| e.interrupted) {
                return Err(Failure::new("interrupted"));
            }
            let observation = collected.ok().filter(observation);
            let failed = observation.is_none();
            current = now(timer, &mut previous, cancel)?;
            let assessment = observation
                .as_ref()
                .and_then(|r| r["assessment"].as_str())
                .unwrap_or("unavailable");
            let elapsed = (current - started) * 1000.0;
            if !elapsed.is_finite() || !(elapsed * 1000.0).is_finite() {
                return Err(Failure::new("clock_failed"));
            }
            let event = json!({"schema_version":1,"contract":"coven.session-watch.v1","sequence":summary["emitted"].as_u64().unwrap()+1,
                "elapsed_ms":(elapsed*1000.0).round()/1000.0,"assessment":assessment,
                "diagnostics":if failed {json!(["collection_failed"])}else{observation.as_ref().unwrap()["diagnostics"].clone()},
                "observation":observation});
            if let Err(e) = emit(&event) {
                if e.interrupted || cancel.interrupted() {
                    return Err(Failure::new("interrupted"));
                }
                fail(&mut summary, "output_failed", "output_failed");
                return Ok(());
            }
            increment(&mut summary, "emitted");
            increment(&mut summary, &format!("{assessment}_samples"));
            if cancel.interrupted() {
                return Err(Failure::new("interrupted"));
            }
            if failed {
                fail(&mut summary, "collector_failed", "collection_failed");
                return Ok(());
            }
            current = now(timer, &mut previous, cancel)?;
            if current >= deadline {
                break;
            }
            let tick = ((current - started) / interval).floor() + 1.0;
            let mut next = started + tick * interval;
            if next <= current {
                next = started + (tick + 1.0) * interval;
            }
            if !next.is_finite() || next <= current {
                return Err(Failure::new("clock_failed"));
            }
            let target = next.min(deadline);
            while current < target {
                if cancel.interrupted() {
                    return Err(Failure::new("interrupted"));
                }
                timer.sleep(target - current).map_err(|e| {
                    if e.interrupted {
                        e
                    } else {
                        Failure::new("clock_failed")
                    }
                })?;
                let after = now(timer, &mut previous, cancel)?;
                if after <= current {
                    return Err(Failure::new("clock_failed"));
                }
                current = after;
            }
        }
        Ok(())
    })();
    if let Err(e) = outcome {
        let code = if e.interrupted {
            "interrupted"
        } else {
            "clock_failed"
        };
        fail(&mut summary, code, code);
    }
    summary
}

pub fn exit_code(summary: &Value) -> u8 {
    match summary["outcome"].as_str() {
        Some("completed") => 0,
        Some("interrupted") => 130,
        _ => 1,
    }
}

fn emit(writer: &mut dyn Write, value: &Value, json_output: bool, summary: bool) -> WireResult<()> {
    let text = if json_output {
        crate::session_support::output(value, false)
            .map_err(|_| Failure::new("output_failed"))?
            .stdout
    } else if summary {
        format!(
            "Watch {}; {} observation(s) emitted. Native lifecycle remains unestablished.",
            value["outcome"].as_str().unwrap_or("output_failed"),
            value["emitted"]
        )
    } else {
        let body = if value["observation"].is_null() {
            "Observation unavailable; no previous targets are current.".into()
        } else {
            session_inspection::human(&value["observation"])
        };
        format!(
            "Watch sample {}: {}\n{body}",
            value["sequence"],
            value["assessment"].as_str().unwrap_or("unavailable")
        )
    };
    writer
        .write_all(text.as_bytes())
        .and_then(|_| writer.write_all(b"\n"))
        .and_then(|_| writer.flush())
        .map_err(|error| {
            Failure::new(if error.kind() == std::io::ErrorKind::Interrupted {
                "interrupted"
            } else {
                "output_failed"
            })
        })
}

/// Streams and flushes every event and summary. Returned stdout is always empty.
pub fn arguments(
    request: &Request,
    duration: f64,
    interval: f64,
    json_output: bool,
    writer: &mut dyn Write,
) -> Result<TaskOutput> {
    session_inspection::validate(request)?;
    validate(duration, interval)?;
    let guard = InterruptGuard::install()?;
    let summary = watch(
        &mut |remaining| {
            let mut bounded = request.clone();
            bounded.timeout = bounded.timeout.min(remaining);
            session_inspection::inspect_cancel(&bounded, &guard)
                .map_err(|_| Failure::new("collection_failed"))
        },
        &mut |event| emit(writer, event, json_output, false),
        duration,
        interval,
        &SystemTimer(Instant::now()),
        &guard,
    );
    let mut code = exit_code(&summary);
    if summary["outcome"] != "output_failed" {
        match emit(writer, &summary, json_output, true) {
            Err(error) => {
                code = if error.interrupted || guard.interrupted() {
                    130
                } else {
                    1
                };
            }
            Ok(()) if guard.interrupted() => code = 130,
            Ok(()) => {}
        }
    }
    Ok(TaskOutput {
        stdout: String::new(),
        warnings: vec![],
        exit_code: code,
    })
}

#[cfg(test)]
#[path = "session_watch/tests.rs"]
mod tests;
