//! One explicitly pinned legacy heartbeat, never idle or execution authority.
pub(crate) mod schema;
use crate::{
    Error, Result,
    observation_time::{Clock, SystemClock},
};
use schema::{field, integer, object, text};
use serde_json::Value;

pub const ADAPTER_CONTRACT: &str = "coven.djinn.live-observation.v1";
pub const NATIVE_CONTRACT: &str = "djinn.live-observation.v1";
const LIMITATIONS: [&str; 5] = [
    "Freshly sampled legacy runner ownership only, not provider progress, health, global activity or idle. Current/V2 execution, decisions and retry phases are unsupported.",
    "Freshness assumes a suitable local filesystem and a shared same-host wall clock. Rollback can make old samples appear younger; a stateless reader cannot detect all replay or sequence regression. Sequence is ordering metadata, not authentication.",
    "Private ownership/modes and path/inode checks are not peer authentication, a sandbox or protection against malicious same-UID writers or every pathname race.",
    "The timeout is an acceptance deadline, not a way to interrupt blocked filesystem syscalls. Remaining lifetime is receipt-relative and must decrease with elapsed time; reading or importing saved reports never renews source freshness.",
    "No discovery, runtime startup, subprocess, socket, database, config or state write. No group enrollment, readiness upgrade, lifecycle, job completion or execution grant.",
];

pub struct Request {
    pub file: String,
    pub runtime: String,
    pub instance: String,
    pub session: String,
    pub timeout: f64,
}

pub(crate) fn validate(request: &Request) -> Result<()> {
    schema::path(&request.file)?;
    schema::uuid(&request.runtime)?;
    schema::uuid(&request.instance)?;
    schema::session(&request.session)?;
    schema::require(
        request.file.rsplit('/').next() == Some(format!("{}.json", request.runtime).as_str()),
    )?;
    timeout(request.timeout)
}

pub(crate) fn timeout(timeout: f64) -> Result<()> {
    crate::error::require(
        timeout.is_finite() && timeout > 0.0 && timeout <= 1.0,
        "invalid_live_timeout",
    )
}

pub fn inspect(request: &Request) -> Result<Value> {
    inspect_with_clock(request, &SystemClock)
}

pub(crate) fn inspect_with_clock(request: &Request, clock: &dyn Clock) -> Result<Value> {
    validate(request).map_err(|_| Error("invalid_live_arguments"))?;
    let started = clock.monotonic();
    let started_at = clock.timestamp();
    let mut report = object(vec![
        ("schema_version", integer(1)?),
        ("adapter", text("djinn_live")),
        ("adapter_contract", text(ADAPTER_CONTRACT)),
        (
            "file",
            object(vec![
                ("selected_path", text(&request.file)),
                ("resolved_path", Value::Null),
                ("device", Value::Null),
                ("inode", Value::Null),
            ])?,
        ),
        (
            "selection",
            object(vec![
                ("runtime_id", text(&request.runtime)),
                ("instance_id", text(&request.instance)),
                ("session_id", text(&request.session)),
            ])?,
        ),
        (
            "started_at",
            started_at.as_ref().map(|s| text(s)).unwrap_or(Value::Null),
        ),
        ("assessment", text("unavailable")),
        ("diagnostics", Value::Array(vec![])),
        ("activity", text("unknown")),
        ("activity_scope", text("legacy_runner_ownership")),
        ("attention", text("unknown")),
        ("freshness", text("unknown")),
        ("expires_in_ms", integer(0)?),
        ("source", Value::Null),
        ("authorization", text("none")),
        ("dispatchable", Value::Bool(false)),
        ("native_lifecycle", text("unestablished")),
        ("job_completion", text("unestablished")),
        (
            "limitations",
            Value::Array(LIMITATIONS.iter().map(|s| text(s)).collect()),
        ),
    ])?;
    let observed = (|| -> Result<(String, Value)> {
        started_at.map_err(|_| Error("io_error"))?;
        let artifact = crate::platform::live_artifact::open(&request.file)?;
        report["file"] = object(vec![
            ("selected_path", text(&request.file)),
            ("resolved_path", text(artifact.resolved())),
            (
                "device",
                artifact
                    .device()
                    .map(integer)
                    .transpose()?
                    .unwrap_or(Value::Null),
            ),
            (
                "inode",
                artifact
                    .inode()
                    .map(integer)
                    .transpose()?
                    .unwrap_or(Value::Null),
            ),
        ])?;
        let decoded = artifact
            .bytes()
            .and_then(|bytes| schema::decode(bytes, request));
        // Recheck after parsing even if the source schema is rejected.
        artifact.recheck()?;
        decoded
    })();
    // Timestamp generation itself may take time: do it before the final age/deadline guard.
    let observed_at = clock.timestamp();
    report["observed_at"] = observed_at.as_ref().map(|s| text(s)).unwrap_or(Value::Null);
    let wall = clock.wall_ms();
    let elapsed = clock
        .monotonic()
        .saturating_duration_since(started)
        .as_secs_f64()
        * 1000.0;
    let accepted = observed.and_then(|(activity, source)| {
        observed_at.map_err(|_| Error("io_error"))?;
        if elapsed >= request.timeout * 1000.0 {
            return Err(Error("timeout"));
        }
        let age = wall - schema::uint(field(&source, "sampled_at_ms")?, false)? as f64;
        if age < 0.0 {
            return Err(Error("future_sample"));
        }
        let remaining = (3000.0 - age - elapsed).floor();
        if !remaining.is_finite() || remaining <= 0.0 {
            return Err(Error("stale_sample"));
        }
        Ok((activity, source, remaining as u64))
    });
    match accepted {
        Ok((activity, source, remaining)) => {
            report["assessment"] = text("observed");
            report["activity"] = text(&activity);
            report["source"] = source;
            report["freshness"] = text("fresh_sample");
            report["expires_in_ms"] = integer(remaining)?;
        }
        Err(error) => {
            let code = diagnostic(error.code());
            report["assessment"] = text(if code == "unsupported_contract" {
                "incompatible"
            } else {
                "unavailable"
            });
            report["diagnostics"] = Value::Array(vec![text(code)]);
        }
    }
    report["duration_ms"] = Value::from((elapsed * 1000.0).round() / 1000.0);
    Ok(report)
}

pub(crate) fn diagnostic(code: &str) -> &'static str {
    match code {
        "schema_error" => "schema_error",
        "unsupported_contract" => "unsupported_contract",
        "runtime_mismatch" => "runtime_mismatch",
        "instance_not_observed" => "instance_not_observed",
        "session_not_observed" => "session_not_observed",
        "unsafe_directory" => "unsafe_directory",
        "directory_changed" => "directory_changed",
        "unsafe_artifact" => "unsafe_artifact",
        "response_limit" => "response_limit",
        "artifact_changed" => "artifact_changed",
        "timeout" => "timeout",
        "future_sample" => "future_sample",
        "stale_sample" => "stale_sample",
        "unsupported_platform" => "unsupported_platform",
        "invalid_utf8" => "invalid_utf8",
        "duplicate_key" => "duplicate_key",
        "nonfinite_json" => "nonfinite_json",
        "invalid_json" => "invalid_json",
        "nesting_limit" => "nesting_limit",
        _ => "io_error",
    }
}

#[cfg(all(test, unix))]
#[path = "djinn_live_observation/tests.rs"]
mod tests;
