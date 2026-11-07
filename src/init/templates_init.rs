use super::{Result, templates, text};
use std::path::Path;

pub(super) fn team(
    name: &str,
    agents: &[String],
    labels: &str,
    command: &str,
    mux: &str,
) -> String {
    let q = templates::toml_string;
    let blocks = agents
        .iter()
        .zip(labels.split(','))
        .map(|(id, label)| {
            format!(
                "[[agents]]\nid = {}\ndisplay_name = {}\nrole = {}\nprompt = {}\n",
                q(id),
                q(crate::records::trimmed(label)),
                q(&templates::role(id)),
                q(&format!("prompts/{id}.md"))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "version = 1\nname = {}\n\n[runtime]\nmultiplexer = {}\nopencode_command = {}\nsource_of_truth = \"workspace-files\"\ndashboard = \"dashboard.md\"\n\n{blocks}\n[[checkpoints]]\nid = \"plan-approval\"\ndescription = \"Consult the user after the coven proposes a plan and before implementation.\"\nfile = \"checkpoints/plan-approval.md\"\nconsult_user = true\n\n[[checkpoints]]\nid = \"pre-delivery-review\"\ndescription = \"Consult the user after review/validation and before declaring the task complete.\"\nfile = \"checkpoints/pre-delivery-review.md\"\nconsult_user = true\n",
        q(name),
        q(mux),
        q(command)
    )
}

pub(super) fn agent(id: &str) -> String {
    format!(
        "# Agent: {id}\n\n## Role\n\n{}\n\n## Current assignment\n\n- None.\n\n## Notes\n\n- Append agent-local notes here.\n",
        templates::role(id)
    )
}

fn checkpoint(id: &str, title: &str, description: &str) -> String {
    format!(
        "# Checkpoint: {title}\n\nID: `{id}`\n\nStatus: pending\n\n## Purpose\n\n{description}\n\n## User decision\n\n- [ ] Approved\n- [ ] Rejected / needs changes\n\n## Notes\n\n- Agents should append requests, rationale, and decision notes here.\n"
    )
}

pub(super) fn structure(source: &Path, directory: Option<&str>) -> Result<Vec<(String, String)>> {
    Ok(vec![
        (
            "work/README.md".into(),
            text(
                source,
                directory,
                "work/README.md",
                "# Work Directory\n\nAgents should put in-progress implementation files here.\n\nFor the starter interactive demo example, this is where draft source and design\nhandoffs should be created before the final runnable artifact lands in\n`../output/demo.py`.\n",
            )?,
        ),
        (
            "output/README.md".into(),
            text(
                source,
                directory,
                "output/README.md",
                "# Output Directory\n\nAgents should put final runnable artifacts, demos, summaries, and delivery notes\nhere.\n\nFor the starter example, this should include `demo.py`, an interactive\nworkspace-aware terminal demo agent runnable with:\n\n```bash\npython3 output/demo.py\n```\n\nThe demo should answer questions about what was built, who did what, why\ndecisions were made, which checkpoints were approved, and what QA verified.\n",
            )?,
        ),
        (
            "checkpoints/plan-approval.md".into(),
            checkpoint(
                "plan-approval",
                "Plan Approval",
                "Approve the coven plan, acceptance criteria, and proposed implementation sequence before implementation begins.",
            ),
        ),
        (
            "checkpoints/pre-delivery-review.md".into(),
            checkpoint(
                "pre-delivery-review",
                "Pre-delivery Review",
                "Approve final validation and review notes before the coven declares the work complete.",
            ),
        ),
    ])
}
