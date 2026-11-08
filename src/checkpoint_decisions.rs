//! Single-file decision authority. Runtime delivery is observational and unlocked.
use crate::{Error, Result, checkpoints, error::require, values::Value};
use std::path::{Path, PathBuf};

mod coordinator;
mod notifier;
mod review;
pub use review::review;
#[cfg(test)]
pub(crate) use review::review_with as review_with_for_test;
pub(crate) use review::{display_io, review_io};

#[derive(Clone, Copy, Debug)]
pub enum DecisionAction {
    Approve,
    Suggest,
}
impl DecisionAction {
    fn status(self) -> &'static str {
        match self {
            Self::Approve => "approved",
            Self::Suggest => "changes_requested",
        }
    }
}

pub struct DecisionRequest {
    pub checkpoint: String,
    pub action: DecisionAction,
    pub notes: String,
    pub source: String,
    pub agent: String,
    pub expected_revision: Option<String>,
}

#[derive(Debug)]
pub struct CommandResult {
    pub body: String,
    pub exit_code: u8,
}

pub fn decide(workspace: &Path, request: DecisionRequest, json: bool) -> Result<CommandResult> {
    let report = coordinator::commit(workspace, &request, &coordinator::SystemIo)?;
    render(&report, json)
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value
        .as_object()
        .and_then(|o| o.get(name))
        .and_then(Value::as_str)
        .ok_or(Error("invalid_checkpoint_result"))
}
fn relative(checkpoint: &str) -> Result<PathBuf> {
    checkpoints::inspect_bytes(checkpoint, None)?;
    Ok(Path::new("checkpoints").join(format!("{checkpoint}.md")))
}
fn view(root: &Path, checkpoint: &str) -> Result<Value> {
    let raw = crate::workspace::optional_bytes(
        root,
        &relative(checkpoint)?,
        1_048_576,
        "unsupported_checkpoint_input_profile",
    )?;
    checkpoints::inspect_bytes(checkpoint, raw.as_deref())
}
fn validate_revision(expected: Option<&str>) -> Result<()> {
    if let Some(expected) = expected {
        let valid = expected
            .strip_prefix("checkpoint-snapshot-v1:")
            .is_some_and(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            })
            || expected
                .strip_prefix("checkpoint-missing-v1:")
                .is_some_and(|s| checkpoints::inspect_bytes(s, None).is_ok());
        require(valid, "invalid_expected_checkpoint_revision")?;
    }
    Ok(())
}

struct Report {
    outcome: &'static str,
    checkpoint: String,
    action: &'static str,
    decision_id: String,
    selection: &'static str,
    before_revision: String,
    after_revision: String,
    scope_revision: String,
    warnings: Vec<String>,
    sent: usize,
    missing: Vec<String>,
}
impl Report {
    fn value(&self) -> Result<Value> {
        let mut entries = Vec::new();
        for (name, text) in [
            ("outcome", self.outcome),
            ("checkpoint", &self.checkpoint),
            ("action", self.action),
            ("decision_id", &self.decision_id),
            ("selection", self.selection),
            ("before_revision", &self.before_revision),
            ("after_revision", &self.after_revision),
            ("scope_revision", &self.scope_revision),
        ] {
            entries.push((name.into(), Value::String(text.into())));
        }
        entries.push((
            "warnings".into(),
            Value::Array(self.warnings.iter().cloned().map(Value::String).collect()),
        ));
        entries.push(("sent".into(), Value::integer(&self.sent.to_string())?));
        entries.push((
            "missing".into(),
            Value::Array(self.missing.iter().cloned().map(Value::String).collect()),
        ));
        Value::object(entries)
    }
}
fn render(report: &Report, json: bool) -> Result<CommandResult> {
    let body = if json {
        crate::output::ascii_json(std::str::from_utf8(&report.value()?.compact()?)?)?
    } else {
        let label = if report.outcome == "committed" {
            "Recorded"
        } else {
            "Replaced; durability uncertain for"
        };
        let mut body = format!(
            "[INFO] {label} {} decision {}: {} (selection={})",
            report.checkpoint, report.decision_id, report.action, report.selection
        );
        if !report.missing.is_empty() {
            body.push_str(&format!(
                "\nNo running pane found for: {}",
                report.missing.join(", ")
            ));
        }
        for warning in &report.warnings {
            body.push_str(&format!("\n[WARN] {warning}"));
        }
        body
    };
    Ok(CommandResult {
        body,
        exit_code: if report.outcome == "committed" { 0 } else { 2 },
    })
}
