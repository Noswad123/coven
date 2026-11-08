//! Help-only native worker readiness. Observations never admit work or policy.
mod help;

use crate::{
    Error, Result,
    error::require,
    process::{NativeRunner, ProbeRequest, ProbeRunner, ProbeStatus, identity},
    task_mutation::TaskOutput,
    values::Value,
};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct ReadinessOptions<'a> {
    pub adapter: &'a str,
    pub profile: &'a str,
    pub executable: Option<&'a Path>,
    pub timeout: f64,
    pub max_output_bytes: i64,
    pub json: bool,
}

const SURFACES: [&str; 8] = [
    "root",
    "session",
    "session run",
    "session status",
    "session watch",
    "ask",
    "agents",
    "version",
];
const CAPABILITIES: [(&str, &str, &str); 11] = [
    (
        "worker_contract",
        "worker_contract_unverified",
        "Require a documented versioned native worker contract and a matching Coven adapter; do not invent commands from help.",
    ),
    (
        "native_execution_identity",
        "native_identity_unverified",
        "Require stable native execution/session references in submission or reconciliation, not pane IDs or PIDs.",
    ),
    (
        "durable_attempt_correlation",
        "attempt_correlation_unverified",
        "Require persisted caller-supplied attempt identity and a supported lookup for that identity.",
    ),
    (
        "submission_acknowledgement",
        "submission_ack_unverified",
        "Require attempt-bound acceptance/rejection; a spawned process is not accepted work.",
    ),
    (
        "lost_ack_reconciliation",
        "reconcile_unverified",
        "Require recoverable lookup or idempotent admission before any automatic resubmission.",
    ),
    (
        "lifecycle_including_permission_pause",
        "lifecycle_unverified",
        "Require typed native lifecycle facts that distinguish permission pauses, completion, failure, and unknown state.",
    ),
    (
        "side_effect_free_inspection",
        "inspection_purity_unverified",
        "Require a documented non-mutating inspection route; command names such as status do not prove purity.",
    ),
    (
        "result_transcript_references",
        "result_references_unverified",
        "Require result/transcript pointers bound to the admitted attempt, not exit-code or stdout success alone.",
    ),
    (
        "cancellation",
        "cancellation_unverified",
        "Require cancellation acknowledgement and separate termination observation; deletion is not cancellation.",
    ),
    (
        "immutable_policy_admission",
        "readonly_policy_unverified",
        "Require accepted immutable per-attempt tool policy and digest; role names or read-only prompts are not evidence.",
    ),
    (
        "resource_scoped_readonly_enforcement",
        "resource_scope_unverified",
        "Require native enforcement of the attempt's exact resource/path limits, not unrestricted default reads or shell access.",
    ),
];

fn s(text: &str) -> Value {
    Value::String(text.to_owned())
}
fn object<const N: usize>(entries: [(&str, Value); N]) -> Result<Value> {
    Value::object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}
fn strings(items: &[String]) -> Value {
    Value::Array(items.iter().map(|item| s(item)).collect())
}
fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn unverified() -> Result<Value> {
    object([
        ("state", s("unknown")),
        ("evidence_source", s("unverified")),
    ])
}
fn path_text(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or(Error("unsupported_worker_path_encoding"))
}

struct Report {
    cwd: PathBuf,
    assessment: &'static str,
    native_version: Option<String>,
    surfaces: Vec<Value>,
    executable: Value,
    probes: Vec<Value>,
    blockers: Vec<(&'static str, &'static str)>,
    failed: bool,
    changed: bool,
}
impl Report {
    fn new(cwd: PathBuf) -> Result<Self> {
        Ok(Self {
            cwd,
            assessment: "unknown",
            native_version: None,
            surfaces: SURFACES
                .iter()
                .map(|_| unverified())
                .collect::<Result<_>>()?,
            executable: object([
                ("selected_path", Value::Null),
                ("resolved_path", Value::Null),
                ("identity", Value::Null),
            ])?,
            probes: Vec::new(),
            blockers: CAPABILITIES
                .iter()
                .map(|(_, code, guidance)| (*code, *guidance))
                .collect(),
            failed: false,
            changed: false,
        })
    }
    fn block(&mut self, code: &'static str, guidance: &'static str) {
        if !self.blockers.iter().any(|(existing, _)| *existing == code) {
            self.blockers.push((code, guidance));
        }
    }
    fn not_advertised(&mut self, index: usize, probe_id: &str) -> Result<()> {
        self.surfaces[index] = object([
            ("state", s("not_advertised")),
            ("evidence_source", s("help_only")),
            ("probe_id", s(probe_id)),
        ])?;
        Ok(())
    }
    fn value(&self) -> Result<Value> {
        object([
            ("schema_version", Value::integer("1")?),
            ("adapter", s("djinn")),
            ("adapter_contract", s("djinn-help-v1")),
            ("profile", s("research-readonly")),
            ("assessment", s(self.assessment)),
            ("authorization", s("none")),
            (
                "native_version",
                self.native_version.as_deref().map_or(Value::Null, s),
            ),
            (
                "context",
                object([
                    ("workspace", s(path_text(&self.cwd)?)),
                    ("configuration_inspected", Value::Bool(false)),
                    ("policy_inspected", Value::Bool(false)),
                    ("sessions_inspected", Value::Bool(false)),
                ])?,
            ),
            (
                "notes",
                Value::Array(vec![
                    s(
                        "No supported versioned native worker contract interrogated; help does not attest policy.",
                    ),
                    s("Version output, when observed, is not capability attestation."),
                ]),
            ),
            (
                "capabilities",
                Value::object(
                    CAPABILITIES
                        .iter()
                        .map(|(name, _, _)| Ok(((*name).to_owned(), unverified()?)))
                        .collect::<Result<_>>()?,
                )?,
            ),
            (
                "cli_surfaces",
                Value::object(
                    SURFACES
                        .iter()
                        .zip(&self.surfaces)
                        .map(|(name, value)| ((*name).to_owned(), value.clone()))
                        .collect(),
                )?,
            ),
            ("executable", self.executable.clone()),
            ("probes", Value::Array(self.probes.clone())),
            (
                "blockers",
                Value::Array(
                    self.blockers
                        .iter()
                        .map(|(code, guidance)| {
                            object([("code", s(code)), ("next_step", s(guidance))])
                        })
                        .collect::<Result<_>>()?,
                ),
            ),
        ])
    }
    fn output(&self, json: bool) -> Result<TaskOutput> {
        let stdout = if json {
            String::from_utf8(self.value()?.compact()?)
                .map_err(|_| Error("report_encoding_failed"))?
        } else {
            let executable = self
                .executable
                .as_object()
                .ok_or(Error("invalid_worker_report"))?;
            let path = executable
                .get("resolved_path")
                .and_then(Value::as_str)
                .or_else(|| executable.get("selected_path").and_then(Value::as_str))
                .unwrap_or("not found");
            let mut lines = vec![
                format!(
                    "djinn adapter compatibility: {}",
                    self.assessment.to_uppercase()
                ),
                "Profile: research-readonly; adapter contract: djinn-help-v1".to_owned(),
                format!("Executable: {}", visible_text(path)),
            ];
            if let Some(hash) = executable
                .get("identity")
                .and_then(Value::as_object)
                .and_then(|v| v.get("sha256"))
                .and_then(Value::as_str)
            {
                lines.push(format!("Executable SHA256: {hash}"));
            }
            lines.push(format!(
                "Native version: {}",
                self.native_version.as_deref().unwrap_or("not observed")
            ));
            lines.push("Authorization: none. No worker created; help surfaces do not attest effective policy or execution readiness.".to_owned());
            for (name, surface) in SURFACES.iter().zip(&self.surfaces) {
                let state = surface
                    .as_object()
                    .and_then(|v| v.get("state"))
                    .and_then(Value::as_str)
                    .ok_or(Error("invalid_worker_report"))?;
                lines.push(format!("- CLI {name}: {state}"));
            }
            lines.push("Blockers:".to_owned());
            lines.extend(
                self.blockers
                    .iter()
                    .map(|(code, guidance)| format!("- {code}: {guidance}")),
            );
            lines.join("\n")
        };
        Ok(TaskOutput {
            stdout,
            warnings: Vec::new(),
            exit_code: if self.assessment == "blocked" { 1 } else { 2 },
        })
    }
}

fn visible_text(text: &str) -> String {
    use std::fmt::Write;
    let mut output = String::new();
    for character in text.chars() {
        if crate::unicode::printable(character) {
            output.push(character);
        } else {
            let _ = write!(output, "\\u{:04x}", u32::from(character));
        }
    }
    output
}

struct Prober<'a> {
    selected: &'a Path,
    resolved: &'a Path,
    pinned: &'a identity::Identity,
    timeout: Duration,
    limit: usize,
    environment: std::collections::BTreeMap<OsString, OsString>,
    runner: &'a dyn ProbeRunner,
}
impl Prober<'_> {
    fn probe(
        &self,
        report: &mut Report,
        arguments: &[&str],
        route: &[&str],
        index: usize,
    ) -> Result<Option<(help::Help, String)>> {
        // The domain, not the generic transport, owns this closed observation set.
        require(
            matches!(
                arguments,
                ["--help"]
                    | ["--version"]
                    | ["session", "--help"]
                    | ["session", "run", "--help"]
                    | ["session", "status", "--help"]
                    | ["session", "watch", "--help"]
                    | ["ask", "--help"]
                    | ["agents", "--help"]
            ),
            "unsupported_worker_probe",
        )?;
        if !identity::alias_matches(self.selected, self.resolved)
            || identity::metadata(self.resolved).ok().as_ref() != Some(&self.pinned.metadata)
        {
            report.changed = true;
            return Ok(None);
        }
        let observed = crate::task_time::now()?;
        let mut result = self.runner.run(&ProbeRequest {
            executable: self.resolved.to_owned(),
            arguments: arguments.iter().map(OsString::from).collect(),
            cwd: report.cwd.clone(),
            environment: self.environment.clone(),
            timeout: self.timeout,
            max_output_bytes: self.limit,
        });
        if !result.duration_ms.is_finite() || result.duration_ms < 0.0 {
            result.status = ProbeStatus::SpawnError;
            result.returncode = None;
            result.stdout.clear();
            result.stderr.clear();
            result.duration_ms = 0.0;
        }
        if result.stdout.len().saturating_add(result.stderr.len()) > self.limit {
            result.stdout.truncate(self.limit);
            result.stderr.truncate(self.limit - result.stdout.len());
            result.status = ProbeStatus::OutputLimit;
        }
        let id = format!("probe-{}", report.probes.len() + 1);
        let mut combined = Sha256::new();
        combined.update(&result.stdout);
        combined.update(&result.stderr);
        let mut provenance = object([
            ("id", s(&id)),
            (
                "argv",
                Value::Array(
                    std::iter::once(s(path_text(self.resolved)?))
                        .chain(arguments.iter().map(|arg| s(arg)))
                        .collect(),
                ),
            ),
            ("observed_at", s(&observed)),
            ("status", s(result.status.as_str())),
            (
                "returncode",
                result
                    .returncode
                    .map(|code| Value::integer(&code.to_string()))
                    .transpose()?
                    .unwrap_or(Value::Null),
            ),
            (
                "output_size",
                Value::integer(&(result.stdout.len() + result.stderr.len()).to_string())?,
            ),
            (
                "stdout_size",
                Value::integer(&result.stdout.len().to_string())?,
            ),
            (
                "stderr_size",
                Value::integer(&result.stderr.len().to_string())?,
            ),
            ("stdout_sha256", s(&digest(&result.stdout))),
            ("stderr_sha256", s(&digest(&result.stderr))),
            ("output_sha256", s(&format!("{:x}", combined.finalize()))),
            (
                "duration_ms",
                Value::float((result.duration_ms * 1000.0).round() / 1000.0)?,
            ),
        ])?;
        let mut parsed = None;
        if result.status != ProbeStatus::Ok || result.returncode != Some(0) {
            report.failed = true;
            provenance.set_field(
                "diagnostic_excerpt",
                s(&format!(
                    "Probe failed: {}. Native output withheld.",
                    result.status.as_str()
                )),
            )?;
            report.block(
                "probe_failure",
                "Check the selected executable and probe bounds; retry help only.",
            );
        } else {
            let surface = (|| -> Result<Value> {
                std::str::from_utf8(&result.stderr)?;
                if arguments == ["--version"] {
                    report.native_version = Some(help::version(&result.stdout)?);
                    object([
                        ("state", s("observed")),
                        ("evidence_source", s("help_only")),
                        ("attestation", Value::Bool(false)),
                    ])
                } else {
                    let help = help::parse(&result.stdout, route)?;
                    let surface = object([
                        ("state", s("advertised")),
                        ("evidence_source", s("help_only")),
                        ("commands", strings(&help.commands)),
                        ("flags", strings(&help.flags)),
                        ("help_sha256", s(&digest(&result.stdout))),
                    ])?;
                    parsed = Some((help, id.clone()));
                    Ok(surface)
                }
            })();
            match surface {
                Ok(mut surface) => {
                    surface.set_field("probe_id", s(&id))?;
                    report.surfaces[index] = surface;
                }
                Err(_) => {
                    report.failed = true;
                    provenance.set_field("parse_status", s("error"))?;
                    provenance.set_field(
                        "diagnostic_excerpt",
                        s("Unrecognized or incomplete help/version text; output withheld."),
                    )?;
                    report.block(
                        "probe_parse_error",
                        "Verify complete UTF-8 Clap-style help for this adapter route.",
                    );
                }
            }
        }
        report.probes.push(provenance);
        Ok(parsed)
    }
}

pub fn readiness_arguments(workspace: &Path, options: ReadinessOptions<'_>) -> Result<TaskOutput> {
    readiness_with_runner(workspace, options, &NativeRunner)
}

fn readiness_with_runner(
    workspace: &Path,
    options: ReadinessOptions<'_>,
    runner: &dyn ProbeRunner,
) -> Result<TaskOutput> {
    let (timeout, limit) = identity::validate_limits(options.timeout, options.max_output_bytes)?;
    require(
        options.adapter == "djinn" && options.profile == "research-readonly",
        "unsupported_worker_adapter_or_profile",
    )?;
    require(workspace.is_dir(), "invalid_worker_workspace")?;
    let mut report = Report::new(workspace.canonicalize()?)?;
    let selected = match options.executable {
        Some(path) => {
            let text = path_text(path)?;
            require(
                !text.is_empty() && !text.contains('\0'),
                "invalid_worker_executable",
            )?;
            let path = if text == "~" || text.starts_with("~/") {
                if let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) {
                    PathBuf::from(home).join(text.strip_prefix("~/").unwrap_or(""))
                } else {
                    report.block("executable_unverified", "Select an explicit trusted executable path with a resolvable home directory.");
                    return report.output(options.json);
                }
            } else {
                path.to_owned()
            };
            if path.is_absolute() {
                Some(path)
            } else {
                Some(std::env::current_dir()?.join(path))
            }
        }
        None => identity::find_executable("djinn"),
    };
    let Some(selected) = selected else {
        report.block(
            "executable_missing",
            "Install Djinn or select an existing executable path.",
        );
        return report.output(options.json);
    };
    let absolute = if selected.is_absolute() {
        selected
    } else {
        std::env::current_dir()?.join(selected)
    };
    let mut selected = PathBuf::new();
    // Preserve parents until filesystem resolution: link/.. need not refer to
    // the lexical parent of link. Components removes only redundant dots here.
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            other => selected.push(other),
        }
    }
    report
        .executable
        .set_field("selected_path", s(path_text(&selected)?))?;
    let pinned = (|| -> Result<_> {
        let resolved = selected.canonicalize()?;
        report
            .executable
            .set_field("resolved_path", s(path_text(&resolved)?))?;
        let pinned = identity::fingerprint(&resolved)?;
        report.executable.set_field("identity", pinned.value()?)?;
        Ok((resolved, pinned))
    })();
    let (resolved, pinned) = match pinned {
        Ok(value) => value,
        Err(_) => {
            report.block(
                if selected.exists() {
                    "executable_unverified"
                } else {
                    "executable_missing"
                },
                "Select a stable, regular executable readable within the 1 GiB identity limit.",
            );
            return report.output(options.json);
        }
    };
    let prober = Prober {
        selected: &selected,
        resolved: &resolved,
        pinned: &pinned,
        timeout,
        limit,
        environment: identity::adapter_environment(),
        runner,
    };
    let mut missing = false;
    if let Some((root, root_id)) = prober.probe(&mut report, &["--help"], &[], 0)? {
        for (command, index) in [("session", 1), ("ask", 5), ("agents", 6)] {
            if !root.commands.iter().any(|name| name == command) {
                report.not_advertised(index, &root_id)?;
                if command == "session" {
                    missing = true;
                }
                continue;
            }
            let surface = prober.probe(&mut report, &[command, "--help"], &[command], index)?;
            if command == "session" {
                if let Some((session, session_id)) = surface {
                    for (child, index) in [("run", 2), ("status", 3), ("watch", 4)] {
                        if session.commands.iter().any(|name| name == child) {
                            prober.probe(
                                &mut report,
                                &["session", child, "--help"],
                                &["session", child],
                                index,
                            )?;
                        } else {
                            report.not_advertised(index, &session_id)?;
                            if child == "run" {
                                missing = true;
                            }
                        }
                    }
                }
            }
        }
        if root.flags.iter().any(|flag| flag == "--version") {
            prober.probe(&mut report, &["--version"], &[], 7)?;
        } else {
            report.not_advertised(7, &root_id)?;
        }
    }
    // Always hash again, including missing routes and failed probes. Metadata
    // alone cannot detect an in-place rewrite with a restored modification time.
    match identity::fingerprint(&resolved) {
        Ok(after) => {
            report
                .executable
                .set_field("identity_after", after.value()?)?;
            if after != pinned || !identity::alias_matches(&selected, &resolved) {
                report.changed = true;
            }
        }
        Err(_) => report.changed = true,
    }
    if report.changed {
        report.block(
            "executable_changed",
            "Keep the selected executable and alias stable, then retry.",
        );
    }
    if missing {
        report.block("unsupported_advertised_session_route", "Select an adapter/executable advertising session run; do not try bare fallback commands.");
    }
    if missing && !report.failed && !report.changed {
        report.assessment = "blocked";
    }
    report.output(options.json)
}

#[cfg(test)]
mod tests;
