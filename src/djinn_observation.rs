//! One explicit trusted producer call: persisted evidence, never runtime authority.
//! Native finite-value safety ceilings additionally admit at most 262,144 JSON
//! nodes, 4,300 integer digits and 8,192 bytes per number token. The producer wire
//! profile is depth 32, 4,096 entries per container, 4,096 characters per key/string
//! (8,192 for designated path/value strings). These are not evidence freshness.
mod schema;

use crate::{
    Error, Result,
    process::{NativeRunner, ProbeRequest, ProbeRunner, identity},
    task_mutation::TaskOutput,
    task_time,
    values::Value,
};
use schema::{field, object, text};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Instant,
};

const LIMITATIONS: [&str; 4] = [
    "Persisted evidence only: observation receipt is not event freshness, live activity, native runtime verification, job acceptance or success.",
    "The explicit executable is trusted, not sandboxed or authenticated. Fingerprints and alias checks detect ordinary changes, not every execution race. A selected binary can ignore arguments and access the inherited HOME/PATH.",
    "Executable fingerprint I/O (up to 1 GiB per hash) is outside the subprocess deadline. Provider keys and Djinn/Kitsune configuration overrides are withheld, but this is not native filesystem/network isolation.",
    "Folder/native IDs and file digests describe sourced persisted records, not a live process binding. Per-file stability is not a transactional snapshot. No group membership, worker readiness, policy or execution authority follows.",
];

/// Observe persisted Djinn evidence with explicit selectors and bounded transport.
/// `json` selects compact ASCII JSON; the default is pretty ASCII JSON.
/// The returned body has no terminal newline; the command entry point adds it.
pub fn arguments(
    executable: &str,
    folder: &str,
    timeout: f64,
    max_output_bytes: i64,
    json: bool,
) -> Result<TaskOutput> {
    arguments_with_runner(
        executable,
        folder,
        timeout,
        max_output_bytes,
        json,
        &NativeRunner,
    )
}

fn arguments_with_runner(
    executable: &str,
    folder: &str,
    timeout: f64,
    max_output_bytes: i64,
    json: bool,
    runner: &dyn ProbeRunner,
) -> Result<TaskOutput> {
    let (timeout, max_output_bytes) = identity::validate_limits(timeout, max_output_bytes)?;
    schema::selector(executable).map_err(|_| Error("invalid_observation_selector"))?;
    schema::selector(folder).map_err(|_| Error("invalid_observation_selector"))?;
    let started = Instant::now();
    let mut report = object(vec![
        ("schema_version", Value::integer("1")?),
        ("adapter", text("djinn")),
        ("adapter_contract", text("coven.djinn.observation.v1")),
        (
            "executable",
            object(vec![
                ("selected_path", text(executable)),
                ("resolved_path", Value::Null),
                ("identity", Value::Null),
            ])?,
        ),
        ("folder", text(folder)),
        (
            "request",
            object(vec![("method", text("session.observe"))])?,
        ),
        ("started_at", text(&task_time::now()?)),
        ("assessment", text("unavailable")),
        ("diagnostics", Value::Array(vec![])),
        ("authorization", text("none")),
        ("dispatchable", Value::Bool(false)),
        ("runtime_fact_freshness", text("unknown")),
        ("activity", text("unknown")),
        ("native_lifecycle", text("unestablished")),
        ("job_completion", text("unestablished")),
        ("producer", Value::Null),
        ("observation", Value::Null),
        (
            "limitations",
            Value::Array(LIMITATIONS.iter().map(|s| text(s)).collect()),
        ),
    ])?;
    let observed = (|| -> Result<()> {
        let selected = Path::new(executable);
        let resolved = selected
            .canonicalize()
            .map_err(|_| Error("executable_unavailable"))?;
        let before = identity::fingerprint(&resolved)?;
        let label = resolved.to_str().ok_or(Error("executable_unavailable"))?;
        report.set_field(
            "executable",
            object(vec![
                ("selected_path", text(executable)),
                ("resolved_path", text(label)),
                ("identity", before.value()?),
            ])?,
        )?;
        if !identity::alias_matches(selected, &resolved)
            || identity::metadata(&resolved)? != before.metadata
        {
            return Err(Error("executable_changed"));
        }
        let result = runner.run(&ProbeRequest {
            executable: resolved.clone(),
            arguments: vec![
                OsString::from("session"),
                OsString::from("observe"),
                OsString::from(folder),
                OsString::from("--json"),
            ],
            cwd: PathBuf::from("/"),
            environment: identity::adapter_environment(),
            timeout,
            max_output_bytes,
        });
        // Hash the complete executable again even when the transport failed. An
        // alias/content change dominates both native status and producer payload.
        let after = identity::fingerprint(&resolved)?;
        if before != after || !identity::alias_matches(selected, &resolved) {
            return Err(Error("executable_changed"));
        }
        if result.stdout.len().saturating_add(result.stderr.len()) > max_output_bytes {
            return Err(Error("output_limit"));
        }
        if result.status.as_str() != "ok" || result.returncode != Some(0) {
            return Err(Error(match result.status.as_str() {
                "timeout" => "timeout",
                "output_limit" => "output_limit",
                "spawn_error" => "spawn_error",
                _ => "producer_error",
            }));
        }
        // Stderr is bounded raw bytes, deliberately neither decoded nor exposed.
        let (assessment, facts, producer) = schema::decode(&result.stdout, folder)?;
        report.set_field("assessment", text(assessment))?;
        report.set_field("observation", facts)?;
        report.set_field("producer", producer)?;
        if assessment == "unavailable" {
            report.set_field(
                "diagnostics",
                Value::Array(vec![text("native_unavailable")]),
            )?;
        }
        Ok(())
    })();
    if let Err(error) = observed {
        let code = match error.code() {
            "executable_changed" => "executable_changed",
            "timeout" => "timeout",
            "output_limit" => "output_limit",
            "spawn_error" => "spawn_error",
            "producer_error" => "producer_error",
            "unsupported_contract" => "unsupported_contract",
            "invalid_utf8" => "invalid_utf8",
            "invalid_json" => "invalid_json",
            "duplicate_key" => "duplicate_key",
            "nonfinite_json" => "nonfinite_json",
            "nesting_limit" => "nesting_limit",
            "schema_error" => "schema_error",
            _ => "executable_unavailable",
        };
        report.set_field(
            "assessment",
            text(if code == "unsupported_contract" {
                "incompatible"
            } else {
                "unavailable"
            }),
        )?;
        report.set_field("diagnostics", Value::Array(vec![text(code)]))?;
    }
    report.set_field("observed_at", text(&task_time::now()?))?;
    report.set_field(
        "duration_ms",
        Value::float((started.elapsed().as_secs_f64() * 1_000_000.0).round() / 1000.0)?,
    )?;
    let exit_code = match field(&report, "assessment")?.as_str() {
        Some("observed") => 0,
        Some("incompatible") => 2,
        _ => 1,
    };
    let mut bytes = if json {
        report.compact()?
    } else {
        report.storage()?
    };
    if !json && bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    let stdout = crate::output::ascii_json(
        std::str::from_utf8(&bytes).map_err(|_| Error("report_encoding_failed"))?,
    )?;
    Ok(TaskOutput {
        stdout,
        warnings: vec![],
        exit_code,
    })
}

#[cfg(test)]
#[path = "djinn_observation/tests.rs"]
mod tests;
