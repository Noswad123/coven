//! Frozen runtime transport only: not readiness, task dispatch, or native lifetime ownership.
use crate::{
    Result,
    actions::authority as plans,
    runtime_command::{NativeRunner, Runner, Status},
    values::{self, Profile, Value},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

#[path = "launch/executable.rs"]
pub(crate) mod executable;

#[cfg(test)]
#[path = "launch/tests.rs"]
mod tests;

pub(crate) fn build_plan(
    root: &str,
    namespace: &str,
    multiplexer: &str,
    agents: &Value,
    environment: &Value,
) -> Result<Value> {
    plans::build_plan(root, namespace, multiplexer, agents, environment)
}

/// Return an independent snapshot; no config, path normalization, or ambient reads.
pub(crate) fn validate_plan(plan: &Value) -> Result<Value> {
    plans::validate_plan(plan)
}

pub(crate) fn execute(plan: &Value) -> Value {
    run_with(
        plan,
        &NativeRunner,
        &mut |delay| {
            std::thread::sleep(delay);
            Ok(())
        },
        false,
    )
}

pub(crate) fn stop(plan: &Value) -> Value {
    run_with(plan, &NativeRunner, &mut |_| Ok(()), true)
}

const MAX_OUTPUT: usize = 1024 * 1024;
type Failure = (&'static str, String);
type TransportResult<T> = std::result::Result<T, Failure>;

fn string(text: impl Into<String>) -> Value {
    Value::String(text.into())
}
fn object(entries: impl IntoIterator<Item = (String, Value)>) -> Value {
    // All report keys are fixed, or come from the validated canonical output map.
    // Infallible construction is important: never replace an observed report on an encoding error.
    Value::object(entries.into_iter().collect())
        .expect("transport report keys are unique by construction")
}
fn field<'a>(value: &'a Value, key: &str) -> TransportResult<&'a Value> {
    if let Some(object) = value.as_object() {
        return object
            .get(key)
            .ok_or_else(|| ("KeyError", format!("'{key}'")));
    }
    let message = match value {
        Value::Array(_) => "list indices must be integers or slices, not str",
        Value::String(_) => "string indices must be integers, not 'str'",
        Value::Null => "'NoneType' object is not subscriptable",
        Value::Bool(_) => "'bool' object is not subscriptable",
        Value::Number(values::Number::Integer(_)) => "'int' object is not subscriptable",
        _ => "'float' object is not subscriptable",
    };
    Err(("TypeError", message.into()))
}
fn text(value: &Value) -> TransportResult<&str> {
    value
        .as_str()
        .ok_or_else(|| ("TypeError", "expected text".into()))
}
fn invalid(message: impl Into<String>) -> Failure {
    ("ValueError", message.into())
}
fn truncated(text: &str) -> String {
    text.chars().take(2048).collect()
}
fn trim_python(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}
fn handle(value: &Value) -> TransportResult<String> {
    let id = value.as_str().filter(|id| {
        (1..=128).contains(&id.len())
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    });
    id.map(str::to_owned)
        .ok_or_else(|| invalid("missing or unsafe native handle"))
}

fn json_output(raw: &[u8]) -> TransportResult<Value> {
    // The shared finite parser rejects -Infinity as a malformed number; Python's
    // parse_constant hook classifies it alongside NaN and positive Infinity.
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in raw.iter().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                quoted = false;
            }
        } else if *byte == b'"' {
            quoted = true;
        } else if raw[index..].starts_with(b"-Infinity") {
            return Err(invalid("nonfinite JSON number"));
        }
    }
    values::parse(raw, Profile::Authority).map_err(|error| {
        if error.code().ends_with("_profile") {
            return ("UnsupportedOutputProfile", error.code().to_owned());
        }
        let message = match error.code() {
            "duplicate_json_key" => "duplicate JSON key",
            "unsupported_nonfinite_json" => "nonfinite JSON number",
            other => other,
        };
        (
            if message == "duplicate JSON key" || message == "nonfinite JSON number" {
                "ValueError"
            } else {
                "JSONDecodeError"
            },
            message.to_owned(),
        )
    })
}

fn tmux_present(code: i32, stderr: &str, namespace: &str) -> TransportResult<bool> {
    if code == 0 {
        return Ok(true);
    }
    let detail = trim_python(stderr);
    let lower = detail.to_lowercase();
    let absent = detail == format!("can't find session: {namespace}")
        || detail == format!("can't find session: ={namespace}")
        || lower.starts_with("no server running on ")
        || ((lower.starts_with("error connecting to ")
            || lower.starts_with("failed to connect to "))
            && lower.contains("no such file or directory"));
    if code == 1 && absent {
        return Ok(false);
    }
    Err(invalid(format!(
        "tmux presence query failed ({code}): {}",
        if detail.is_empty() {
            "no established absence response".to_owned()
        } else {
            truncated(detail)
        }
    )))
}

struct Report {
    stopping: bool,
    completed: usize,
    handles: BTreeMap<String, String>,
    diagnostics: Vec<Value>,
    unknown: bool,
    attempted: bool,
    succeeded: bool,
}
impl Report {
    fn value(self, completed: bool) -> Value {
        let mut entries = vec![
            (
                "operation".into(),
                string(if self.stopping {
                    "runtime_stop"
                } else {
                    "runtime_launch"
                }),
            ),
            (
                "status".into(),
                string(if completed {
                    "transport_completed"
                } else {
                    "transport_failed_or_partial"
                }),
            ),
            ("completed_steps".into(), count(self.completed)),
            (
                "handles".into(),
                object(self.handles.into_iter().map(|(k, v)| (k, string(v)))),
            ),
            ("diagnostics".into(), Value::Array(self.diagnostics)),
            (
                "outcome_unknown".into(),
                Value::Bool(self.unknown || (self.stopping && self.attempted && !self.succeeded)),
            ),
        ];
        if self.stopping {
            entries.push(("stop_command_attempted".into(), Value::Bool(self.attempted)));
            entries.push(("stop_command_succeeded".into(), Value::Bool(self.succeeded)));
        }
        object(entries)
    }
}

fn count(value: usize) -> Value {
    // Report counters are bounded by the canonical plan's 163 steps.
    Value::integer(&value.to_string()).expect("bounded transport step counter is a JSON integer")
}

/// Injectable command and delay boundaries; commands own only their direct CLI child.
pub(crate) fn run_with(
    plan: &Value,
    runner: &dyn Runner,
    sleeper: &mut dyn FnMut(Duration) -> TransportResult<()>,
    stopping: bool,
) -> Value {
    run_resolving_with(plan, runner, sleeper, stopping, &executable::candidates)
}

fn run_resolving_with(
    plan: &Value,
    runner: &dyn Runner,
    sleeper: &mut dyn FnMut(Duration) -> TransportResult<()>,
    stopping: bool,
    select: &executable::Selector<'_>,
) -> Value {
    let mut report = Report {
        stopping,
        completed: 0,
        handles: BTreeMap::new(),
        diagnostics: Vec::new(),
        unknown: false,
        attempted: false,
        succeeded: false,
    };
    let mut current = None;
    let result: TransportResult<()> = (|| {
        let frozen = validate_plan(plan).map_err(|error| {
            invalid(if error.code() == "invalid_runtime_launch_plan" {
                if plan.as_object().is_some_and(|object| {
                    object.entries().len() == 9
                        && [
                            "version",
                            "operation",
                            "root",
                            "cwd",
                            "namespace",
                            "multiplexer",
                            "agents",
                            "env",
                            "steps",
                        ]
                        .iter()
                        .all(|key| object.get(key).is_some())
                }) {
                    "plan differs from deterministic runtime_launch v1 plan"
                } else {
                    "invalid plan fields"
                }
            } else {
                error.code()
            })
        })?;
        let namespace = text(field(&frozen, "namespace")?)?;
        let tmux = text(field(&frozen, "multiplexer")?)? == "tmux";
        let cwd = PathBuf::from(text(field(&frozen, "cwd")?)?);
        let env: BTreeMap<_, _> = field(&frozen, "env")?
            .as_object()
            .ok_or_else(|| invalid("invalid environment"))?
            .entries()
            .iter()
            .map(|(k, v)| Ok((k.clone(), text(v)?.to_owned())))
            .collect::<TransportResult<_>>()?;
        let steps = field(&frozen, "steps")?
            .as_array()
            .ok_or_else(|| invalid("invalid steps"))?;
        let stop_steps;
        let steps = if stopping {
            let mut presence = steps[0].clone();
            presence
                .set_field("operation", string("ensure_present"))
                .map_err(|e| invalid(e.code()))?;
            let argv = if tmux {
                vec![
                    string("tmux"),
                    string("kill-session"),
                    string("-t"),
                    string(format!("={namespace}")),
                ]
            } else {
                vec![
                    string("herdr"),
                    string("workspace"),
                    string("close"),
                    object([("handle".into(), string("workspace"))]),
                ]
            };
            stop_steps = [
                presence,
                object([
                    ("operation".into(), string("run")),
                    ("argv".into(), Value::Array(argv)),
                ]),
            ];
            stop_steps.as_slice()
        } else {
            steps
        };
        for (index, step) in steps.iter().enumerate() {
            current = Some(index);
            let operation = text(field(step, "operation")?)?;
            if operation == "sleep" {
                sleeper(Duration::from_secs(5))?;
            } else {
                let argv = field(step, "argv")?
                    .as_array()
                    .ok_or_else(|| invalid("invalid argv"))?
                    .iter()
                    .map(|arg| {
                        if arg.as_object().is_some() {
                            let name = text(field(arg, "handle")?)?;
                            report
                                .handles
                                .get(name)
                                .cloned()
                                .ok_or_else(|| ("KeyError", format!("'{name}'")))
                        } else {
                            Ok(text(arg)?.to_owned())
                        }
                    })
                    .collect::<TransportResult<Vec<_>>>()?;
                let output =
                    executable::run(argv, &cwd, &env, runner, select, Duration::from_secs(60))?;
                if stopping && operation == "run" {
                    report.attempted = true;
                }
                let code = match output.status {
                    Status::Exited(code) => code,
                    Status::ExecFailed => {
                        report.attempted = false;
                        return Err((
                            "OSError",
                            "transport failed before backend image execution".into(),
                        ));
                    }
                    Status::Timeout => {
                        report.unknown = true;
                        return Err((
                            "TimeoutExpired",
                            "transport failed; native effects unknown".into(),
                        ));
                    }
                    Status::SpawnError => {
                        report.unknown = true;
                        return Err(("OSError", "transport failed; native effects unknown".into()));
                    }
                    Status::OutputLimit => {
                        report.unknown = true;
                        return Err((
                            "OutputLimit",
                            "transport output exceeds 1 MiB; native effects unknown".into(),
                        ));
                    }
                };
                if output.stdout.len().saturating_add(output.stderr.len()) > MAX_OUTPUT {
                    report.unknown = true;
                    return Err((
                        "OutputLimit",
                        "transport output exceeds 1 MiB; native effects unknown".into(),
                    ));
                }
                // Python text=True decodes strictly and performs universal newline translation.
                let decode = |raw: &[u8]| -> TransportResult<String> {
                    String::from_utf8(raw.to_vec())
                        .map(|s| s.replace("\r\n", "\n").replace('\r', "\n"))
                        .map_err(|_| {
                            (
                                "UnicodeDecodeError",
                                "transport output is not valid UTF-8".into(),
                            )
                        })
                };
                let stdout = decode(&output.stdout)?;
                let stderr = decode(&output.stderr)?;
                if operation == "ensure_absent" || operation == "ensure_present" {
                    let present = if tmux {
                        tmux_present(code, &stderr, namespace)?
                    } else {
                        if code != 0 {
                            return Err(invalid(format!("workspace list returned {code}")));
                        }
                        let payload = json_output(stdout.as_bytes())?;
                        let workspaces = field(field(&payload, "result")?, "workspaces")?
                            .as_array()
                            .filter(|items| items.iter().all(|v| v.as_object().is_some()))
                            .ok_or_else(|| invalid("workspaces must be a list of objects"))?;
                        let matches = workspaces
                            .iter()
                            .filter(|w| {
                                field(w, "label").ok().and_then(Value::as_str) == Some(namespace)
                            })
                            .collect::<Vec<_>>();
                        if matches.len() > 1 {
                            return Err(invalid("ambiguous scoped workspace label"));
                        }
                        if stopping && !matches.is_empty() {
                            report.handles.insert(
                                "workspace".into(),
                                handle(field(matches[0], "workspace_id").unwrap_or(&Value::Null))?,
                            );
                        }
                        !matches.is_empty()
                    };
                    if !stopping && present {
                        return Err(invalid(
                            "scoped namespace already exists; no adoption or replacement",
                        ));
                    }
                    if stopping && !present {
                        report.completed += 1;
                        break;
                    }
                } else {
                    if code != 0 {
                        return Err(invalid(format!(
                            "command returned {code}: {}",
                            truncated(&stderr)
                        )));
                    }
                    if stopping {
                        report.succeeded = true;
                    }
                    if let Some(outputs) = step.as_object().and_then(|v| v.get("outputs")) {
                        let payload = json_output(stdout.as_bytes())?;
                        // The rebuilt map observes workspace before orchestrator, like Python.
                        for (name, path) in outputs
                            .as_object()
                            .ok_or_else(|| invalid("invalid outputs"))?
                            .entries()
                        {
                            let mut value = &payload;
                            for key in path
                                .as_array()
                                .ok_or_else(|| invalid("invalid output path"))?
                            {
                                value = field(value, text(key)?)?;
                            }
                            report.handles.insert(name.clone(), handle(value)?);
                        }
                    }
                }
            }
            report.completed += 1;
        }
        Ok(())
    })();
    let completed = result.is_ok();
    if let Err((error, message)) = result {
        if error == "UnsupportedOutputProfile" {
            report.unknown = true;
        }
        report.diagnostics.push(object([
            ("step".into(), current.map(count).unwrap_or(Value::Null)),
            ("error".into(), string(error)),
            ("message".into(), string(truncated(&message))),
        ]));
    }
    report.value(completed)
}
