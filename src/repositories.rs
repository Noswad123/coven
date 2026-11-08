//! Immutable checkout observations, never Git mutation or delivery authorization.
mod git;
mod record;
mod registration;
mod timestamp;

use crate::{Result, task_mutation::TaskOutput};
use serde_json::{Value, json};
use std::path::Path;

const UNREGISTERED: &str = "repository-unregistered:v1";
const MAX_RECORD: usize = 65_536;
const AUTHORITY: &str = "state/repository.json";

pub fn inspect_arguments(workspace: &Path, verify: bool, json: bool) -> Result<TaskOutput> {
    inspect_with(workspace, verify, json, &crate::process::NativeRunner)
}

fn inspect_with(
    workspace: &Path,
    verify: bool,
    compact: bool,
    runner: &impl crate::process::ProbeRunner,
) -> Result<TaskOutput> {
    let authority = record::workspace(workspace).and_then(|root| record::read(&root));
    match authority {
        Ok(authority) => {
            let mut result = envelope(authority.as_ref());
            if verify {
                result["observations"] = match authority.as_ref() {
                    None => json!({"available":false,"reason":"unregistered"}),
                    Some(known) => match git::observe(&known.record.original_request, runner) {
                        Ok((frozen, observations)) if frozen == known.record.frozen => observations,
                        _ => {
                            result["status"] = json!("unavailable");
                            json!({"available":false,"reason":"repository_unavailable_or_changed"})
                        }
                    },
                };
            }
            render(&mut result, compact, authority.as_ref())
        }
        Err(_) => {
            let mut result = envelope(None);
            result["status"] = json!("unavailable");
            result["revision"] = Value::Null;
            result["diagnostics"] = json!([{"code":"repository_authority_unavailable"}]);
            if verify {
                result["observations"] =
                    json!({"available":false,"reason":"authority_unavailable"});
            }
            render(&mut result, compact, None)
        }
    }
}

pub fn register_arguments(
    workspace: &Path,
    path: &Path,
    base: &str,
    target: &str,
    expected: &str,
    json: bool,
) -> Result<TaskOutput> {
    registration::register(
        workspace,
        record::Request::new(path, base, target, expected)?,
        json,
        &crate::process::NativeRunner,
        &registration::SystemIo,
    )
}

fn envelope(authority: Option<&record::Authority>) -> Value {
    json!({"schema_version":1,"status":if authority.is_some() {"registered"} else {"unregistered"},
        "revision":authority.map_or_else(||UNREGISTERED.to_owned(), |a|a.revision()),
        "authorization":"none","dispatchable":false})
}

fn render(
    value: &mut Value,
    compact: bool,
    authority: Option<&record::Authority>,
) -> Result<TaskOutput> {
    let exit_code = if value["delivery"]["outcome"] == "durability_uncertain" {
        2
    } else if value["status"] == "unavailable" {
        1
    } else {
        0
    };
    let mut stored = crate::values::parse(
        &serde_json::to_vec(value)?,
        crate::values::Profile::Authority,
    )?;
    if let Some(authority) = authority {
        stored.set_field("registration", authority.value.clone())?;
    }
    let text = if compact {
        // Match Python json.dumps' compact (non-indented), spaced separators.
        let mut buffer = Vec::new();
        // The compact serializer below adds spaces only outside quoted strings.
        let raw = stored.compact()?;
        let (mut quoted, mut escaped) = (false, false);
        for byte in raw {
            buffer.push(byte);
            if quoted {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    quoted = false;
                }
            } else if byte == b'"' {
                quoted = true;
            } else if matches!(byte, b',' | b':') {
                buffer.push(b' ');
            }
        }
        String::from_utf8(buffer).map_err(|_| crate::Error("repository_report_encoding"))?
    } else {
        let bytes = stored.storage()?;
        String::from_utf8(bytes[..bytes.len() - 1].to_vec())
            .map_err(|_| crate::Error("repository_report_encoding"))?
    };
    Ok(TaskOutput {
        stdout: crate::output::ascii_json(&text)?,
        warnings: Vec::new(),
        exit_code,
    })
}

#[cfg(test)]
#[path = "repositories/tests.rs"]
mod tests;
