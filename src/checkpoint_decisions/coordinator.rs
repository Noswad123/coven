use super::*;
use crate::{
    logs::{self, LogKind},
    read_fs, records,
    storage::{self, Delivery, WorkspaceGuard},
    task_delivery, task_time, workspace,
};
use std::time::Duration;

// Local fault/callback seam only; production always uses shared atomic storage
// and the integrator's real runtime transport, never a notification-free path.
pub(super) trait Io {
    fn phase(&self, _phase: &str, _root: &Path) -> Result<()> {
        Ok(())
    }
    fn replace(&self, held: &WorkspaceGuard, relative: &Path, bytes: &[u8]) -> Result<Delivery> {
        storage::atomic_replace_bytes(held, relative, bytes)
    }
    fn secondary(
        &self,
        label: &str,
        root: &Path,
        request: &DecisionRequest,
        report: &Report,
        held: &WorkspaceGuard,
    ) -> Result<()> {
        match label {
            "event history incomplete" => {
                let payload = Value::object(vec![
                    (
                        "checkpoint".into(),
                        Value::String(request.checkpoint.clone()),
                    ),
                    ("by".into(), Value::String("user".into())),
                    ("source".into(), Value::String(request.source.clone())),
                    (
                        "decision_id".into(),
                        Value::String(report.decision_id.clone()),
                    ),
                    (
                        "scope_revision".into(),
                        Value::String(report.scope_revision.clone()),
                    ),
                    (
                        "reviewed_revision".into(),
                        Value::String(report.before_revision.clone()),
                    ),
                ])?;
                task_delivery::append_event(
                    root,
                    if report.action == "approved" {
                        "checkpoint.approved"
                    } else {
                        "checkpoint.changes.requested"
                    },
                    &payload,
                    held,
                )
            }
            "message history incomplete" => {
                logs::read_log(root, "logs/events.jsonl")?;
                let record = Value::object(vec![
                    ("ts".into(), Value::String(task_time::now()?)),
                    ("from".into(), Value::String("user".into())),
                    ("to".into(), Value::String("coven".into())),
                    ("body".into(), Value::String(request.notes.clone())),
                ])?;
                logs::append_encoded(
                    root,
                    LogKind::Messages,
                    &logs::encode_record(&record)?,
                    held,
                )?;
                task_delivery::append_event(
                    root,
                    "message.sent",
                    &Value::object(vec![
                        ("sender".into(), Value::String("user".into())),
                        ("recipient".into(), Value::String("coven".into())),
                    ])?,
                    held,
                )
            }
            _ => task_delivery::refresh_dashboard(root, held),
        }
    }
    fn targets(&self, root: &Path, selector: &str) -> Result<Vec<String>> {
        crate::runtime::notice_targets(root, selector)
    }
    fn send(&self, root: &Path, agent: &str, text: &str) -> Result<bool> {
        crate::runtime::send_notice(root, agent, text)
    }
    fn notice_history(&self, root: &Path, agent: &str, report: &Report) -> Result<()> {
        let held = WorkspaceGuard::acquire(root, Duration::from_secs(5))?;
        task_delivery::append_event(
            root,
            "checkpoint.notice.sent",
            &Value::object(vec![
                (
                    "checkpoint".into(),
                    Value::String(report.checkpoint.clone()),
                ),
                ("agent".into(), Value::String(agent.into())),
                (
                    "decision_id".into(),
                    Value::String(report.decision_id.clone()),
                ),
                (
                    "scope_revision".into(),
                    Value::String(report.scope_revision.clone()),
                ),
            ])?,
            &held,
        )
    }
}
pub(super) struct SystemIo;
impl Io for SystemIo {}

pub(super) fn validate_selector(root: &Path, selector: &str) -> Result<()> {
    let state = workspace::load(root)?;
    if selector != "all" {
        let agents = state
            .team
            .value()
            .as_object()
            .and_then(|o| o.get("agents"))
            .and_then(Value::as_array)
            .ok_or(Error("invalid_workspace_agents"))?;
        require(
            agents.iter().any(|id| id.as_str() == Some(selector)),
            "unknown_agent",
        )?;
    }
    Ok(())
}
fn validate(root: &Path, request: &DecisionRequest) -> Result<()> {
    validate_selector(root, &request.agent)?;
    relative(&request.checkpoint)?;
    require(
        !matches!(request.action, DecisionAction::Suggest)
            || !records::trimmed(&request.notes).is_empty(),
        "empty_checkpoint_change_request",
    )
}

fn initial(checkpoint: &str) -> Vec<u8> {
    // IDs are ASCII. Python title() starts a new word after digits/underscores.
    let mut boundary = true;
    let title: String = checkpoint
        .replace('-', " ")
        .chars()
        .map(|c| {
            let next = if boundary {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            };
            boundary = !c.is_ascii_alphabetic();
            next
        })
        .collect();
    format!("# Checkpoint: {title}\n\nID: `{checkpoint}`\n\nStatus: pending\n\n## Purpose\n\nUser approval checkpoint.\n\n## User decision\n\n- [ ] Approved\n- [ ] Rejected / needs changes\n\n## Notes\n\n- Agents should append requests, rationale, and decision notes here.\n").into_bytes()
}

fn proposal(root: &Path, request: &DecisionRequest, id: &str) -> Result<(Value, Vec<u8>, Value)> {
    let raw = workspace::optional_bytes(
        root,
        &relative(&request.checkpoint)?,
        1_048_576,
        "unsupported_checkpoint_input_profile",
    )?;
    let before = checkpoints::inspect_bytes(&request.checkpoint, raw.as_deref())?;
    require(
        request
            .expected_revision
            .as_deref()
            .is_none_or(|expected| field(&before, "revision").ok() == Some(expected)),
        "stale_checkpoint_revision",
    )?;
    let (bytes, final_view) = checkpoints::prepare(
        &request.checkpoint,
        raw.as_deref(),
        request.action.status(),
        &request.notes,
        id,
        &task_time::now()?,
        &request.source,
        Some(&initial(&request.checkpoint)),
    )?;
    Ok((before, bytes, final_view))
}

pub(super) fn commit(root: &Path, request: &DecisionRequest, io: &impl Io) -> Result<Report> {
    validate_revision(request.expected_revision.as_deref())?;
    validate(root, request)?;
    let id = task_time::uuid_hex()?;
    proposal(root, request, &id)?; // Pure preflight, including complete output bound.
    io.phase("preflight", root)?;
    let mut report;
    {
        let held = WorkspaceGuard::acquire(root, Duration::from_secs(5))?;
        validate(root, request)?;
        let (before, bytes, final_view) = proposal(root, request, &id)?;
        report = Report {
            outcome: "committed",
            checkpoint: request.checkpoint.clone(),
            action: request.action.status(),
            decision_id: id,
            selection: if request.expected_revision.is_some() {
                "conditional"
            } else {
                "latest"
            },
            before_revision: field(&before, "revision")?.into(),
            after_revision: field(&final_view, "revision")?.into(),
            scope_revision: field(&final_view, "scope_revision")?.into(),
            warnings: Vec::new(),
            sent: 0,
            missing: Vec::new(),
        };
        let directory = read_fs::managed(root, Path::new("checkpoints"), true)?;
        if !directory.exists() {
            storage::prepare_directory(&held, Path::new("checkpoints"))?;
        }
        if io.replace(&held, &relative(&request.checkpoint)?, &bytes)?
            == Delivery::DurabilityUncertain
        {
            report.outcome = "durability_uncertain";
            report.warnings.push(
                "Checkpoint replaced; directory synchronization failed. Inspect before retrying."
                    .into(),
            );
            return Ok(report);
        }
        for label in [
            "event history incomplete",
            "message history incomplete",
            "dashboard stale",
        ] {
            if let Err(error) = io.secondary(label, root, request, &report, &held) {
                report
                    .warnings
                    .push(format!("Checkpoint committed; {label}: {}", error.code()));
            }
        }
    }
    super::notifier::deliver(root, &request.agent, &mut report, io);
    Ok(report)
}

#[cfg(test)]
mod tests;
