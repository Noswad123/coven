use super::{field, label, python, text};
use crate::{
    Error, Result, checkpoints,
    error::require,
    values::{self, Value},
    workspace::{self, WorkspaceState},
};
use std::path::Path;

#[derive(Default)]
struct RowBudget {
    bytes: usize,
}

impl RowBudget {
    fn push(&mut self, rows: &mut Vec<String>, row: String) -> Result<()> {
        let bytes = self
            .bytes
            .checked_add(row.len())
            .ok_or(Error("unsupported_history_output_profile"))?;
        require(
            bytes <= values::MAX_OUTPUT,
            "unsupported_history_output_profile",
        )?;
        self.bytes = bytes;
        rows.push(row);
        Ok(())
    }
}

fn checkpoint(root: &Path, id: &str) -> Result<String> {
    let raw = workspace::optional_bytes(
        root,
        Path::new(&format!("checkpoints/{id}.md")),
        workspace::MAX_TEXT,
        "unsupported_checkpoint_input_profile",
    )?;
    let view = checkpoints::inspect_bytes(id, raw.as_deref())?;
    Ok(text(&view, "effective_status", "pending").into())
}
pub(super) fn render(
    root: &Path,
    state: &WorkspaceState,
    mut observe: impl FnMut(&str) -> Result<(String, Option<String>)>,
) -> Result<String> {
    // All sections share one retention limit, before final output is assembled.
    let mut budget = RowBudget::default();
    let mut buckets: [Vec<String>; 4] = Default::default();
    for task in state
        .tasks
        .as_array()
        .ok_or(Error("invalid_workspace_record_list"))?
    {
        let status = text(task, "status", "");
        let lower = status.to_lowercase();
        let line = format!(
            "- `{}` \u{2014} {} ({})",
            text(task, "id", ""),
            text(task, "title", ""),
            field(task, "owner")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("unassigned")
        );
        let bucket = if matches!(lower.as_str(), "completed" | "done") {
            0
        } else if lower.contains("block") || lower.contains("wait") {
            2
        } else if matches!(
            lower.as_str(),
            "in_progress" | "assigned" | "active" | "review"
        ) {
            1
        } else {
            3
        };
        budget.push(
            &mut buckets[bucket],
            if bucket == 0 {
                line
            } else {
                format!(
                    "{line} \u{2014} {}",
                    if bucket == 3 && status.is_empty() {
                        "pending"
                    } else {
                        status
                    }
                )
            },
        )?;
    }
    let mut agents = Vec::new();
    for saved in state
        .agents
        .as_array()
        .ok_or(Error("invalid_workspace_record_list"))?
    {
        let agent = text(saved, "id", "");
        let (runtime, pane) = observe(agent)?;
        let mut out = python::Output::default();
        out.push(&format!(
            "- `{agent}` ({}) \u{2014} state=",
            label(state.team.value(), agent, true)
        ))?;
        out.value(field(saved, "status").unwrap_or(&Value::String("unknown".into())))?;
        out.push(&format!(
            ", {}={runtime}{}",
            state.multiplexer,
            pane.map(|p| format!(" at {p}")).unwrap_or_default()
        ))?;
        budget.push(&mut agents, out.finish())?;
    }
    let mut checkpoints = Vec::new();
    for id in ["plan-approval", "pre-delivery-review"] {
        budget.push(
            &mut checkpoints,
            format!(
                "- `{id}` \u{2014} {} (`checkpoints/{id}.md`)",
                checkpoint(root, id)?
            ),
        )?;
    }
    let mut lead = Vec::new();
    if matches!(state.lead, Value::Null) {
        budget.push(&mut lead, "- None set.".into())?;
    } else {
        let agent = text(&state.lead, "agent", "");
        let (runtime, pane) = observe(agent)?;
        budget.push(
            &mut lead,
            format!(
                "- `{agent}` \u{2014} {}={runtime}{}",
                state.multiplexer,
                pane.map(|p| format!(" at {p}")).unwrap_or_default()
            ),
        )?;
        budget.push(
            &mut lead,
            format!("- Set: {}", text(&state.lead, "set_at", "unknown time")),
        )?;
        budget.push(
            &mut lead,
            format!(
                "- Stop condition: {}",
                text(&state.lead, "until", "goal achieved or critical blocker")
            ),
        )?;
    }
    let mut suggestions = Vec::new();
    if !buckets[2].is_empty() {
        budget.push(
            &mut suggestions,
            "- Clear the blocked task(s) or message the owner for a handoff.".into(),
        )?;
    }
    if checkpoint(root, "pre-delivery-review")? != "approved"
        && !buckets[0].is_empty()
        && buckets[2].is_empty()
    {
        budget.push(
            &mut suggestions,
            "- Prepare or review the pre-delivery checkpoint when QA/review evidence is ready."
                .into(),
        )?;
    }
    if suggestions.is_empty() {
        budget.push(
            &mut suggestions,
            "- Continue from the dashboard and current task assignments.".into(),
        )?;
    }
    let mut out = python::Output::default();
    out.push(&format!(
        "# Coven Standup: {}",
        text(state.team.value(), "name", "")
    ))?;
    for (title, lines, empty) in [
        ("Coven Lead", &lead, "- None set."),
        ("Done", &buckets[0], "- Nothing completed yet."),
        ("In Progress", &buckets[1], "- Nothing in progress."),
        ("Blocked / Waiting", &buckets[2], "- No blockers recorded."),
        ("Pending", &buckets[3], "- No pending tasks."),
        ("Agents", &agents, "- No agents recorded."),
        ("Checkpoints", &checkpoints, "- No checkpoints recorded."),
        (
            "Suggested Next Action",
            &suggestions,
            "- Inspect `coven status`.",
        ),
    ] {
        out.push(&format!("\n## {title}\n\n"))?;
        if lines.is_empty() {
            out.push(empty)?;
        } else {
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    out.push("\n")?;
                }
                out.push(line)?;
            }
        }
        out.push("\n")?;
    }
    Ok(out.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_row_budget_accepts_exact_limit_and_rejects_before_push_or_overflow() {
        // Precharge prior rows without allocating a full 32 MiB test fixture.
        let mut budget = RowBudget {
            bytes: values::MAX_OUTPUT - 4,
        };
        let mut first = Vec::new();
        let mut second = Vec::new();
        budget.push(&mut first, "abc".into()).unwrap();
        budget.push(&mut second, "d".into()).unwrap();
        assert_eq!(budget.bytes, values::MAX_OUTPUT);
        assert_eq!(
            budget.push(&mut second, "e".into()).unwrap_err().code(),
            "unsupported_history_output_profile"
        );
        assert_eq!(budget.bytes, values::MAX_OUTPUT);
        assert_eq!(first, ["abc"]);
        assert_eq!(second, ["d"]);

        budget.bytes = usize::MAX;
        assert_eq!(
            budget
                .push(&mut first, "overflow".into())
                .unwrap_err()
                .code(),
            "unsupported_history_output_profile"
        );
        assert_eq!(budget.bytes, usize::MAX);
        assert_eq!(first, ["abc"]);
    }
}
