//! Pure initial workspace templates and the portable embedded seed.
use super::{agent_role, toml_quote};
use crate::{Error, Result, error::require, values};

pub(crate) struct InitExample {
    pub id: String,
    pub seed_message: String,
    pub goal: String,
    pub spec: String,
    pub work: String,
    pub output: String,
    pub context: String,
    pub tasks: values::Value,
}

pub(crate) fn init_example(selector: &str) -> Result<InitExample> {
    super::example_assets::AssetBundle::environment()?.load(selector)
}

pub(super) fn embedded_example() -> Result<InitExample> {
    #[derive(serde::Deserialize)]
    struct Manifest<'a> {
        id: &'a str,
        seed_message: &'a str,
    }
    let manifest: Manifest<'static> =
        serde_json::from_str(include_str!("../../examples/interactive-demo/example.json"))
            .map_err(|_| Error("unsupported_init_example_profile"))?;
    Ok(InitExample {
        id: manifest.id.into(),
        seed_message: manifest.seed_message.into(),
        goal: include_str!("../../examples/interactive-demo/goal.md").into(),
        spec: include_str!("../../examples/interactive-demo/spec.md").into(),
        work: include_str!("../../examples/interactive-demo/work/README.md").into(),
        output: include_str!("../../examples/interactive-demo/output/README.md").into(),
        context: include_str!("../../examples/interactive-demo/prompt-context.md").into(),
        tasks: values::parse(
            include_bytes!("../../examples/interactive-demo/tasks.json"),
            values::Profile::LegacySnapshot,
        )?,
    })
}

pub(crate) fn team_toml(
    name: &str,
    agents: &[String],
    command: &str,
    multiplexer: &str,
    display_names: &values::Value,
) -> Result<String> {
    const LIMIT: usize = 1_048_576;
    // Check escaped lengths before allocating quoted strings or growing output.
    fn append(output: &mut String, text: &str, quoted: bool) -> Result<()> {
        let mut size = if quoted { 2 } else { text.len() };
        if quoted {
            for ch in text.chars() {
                size += match ch {
                    '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
                    ch if ch < ' ' || ch == '\u{7f}' => 6,
                    ch => ch.len_utf8(),
                };
                require(size <= LIMIT, "unsupported_init_template_profile")?;
            }
        }
        require(
            size <= LIMIT.saturating_sub(output.len()),
            "unsupported_init_template_profile",
        )?;
        if quoted {
            output.push_str(&toml_quote(text));
        } else {
            output.push_str(text);
        }
        Ok(())
    }
    let names = display_names
        .as_object()
        .ok_or(Error("unsupported_init_template_profile"))?;
    let mut output = String::new();
    append(&mut output, "version = 1\nname = ", false)?;
    append(&mut output, name, true)?;
    append(&mut output, "\n\n[runtime]\nmultiplexer = ", false)?;
    append(&mut output, multiplexer, true)?;
    append(&mut output, "\nopencode_command = ", false)?;
    append(&mut output, command, true)?;
    append(
        &mut output,
        "\nsource_of_truth = \"workspace-files\"\ndashboard = \"dashboard.md\"\n\n",
        false,
    )?;
    for (index, agent) in agents.iter().enumerate() {
        require(agent.len() <= LIMIT, "unsupported_init_template_profile")?;
        if index != 0 {
            append(&mut output, "\n", false)?;
        }
        append(&mut output, "[[agents]]\nid = ", false)?;
        append(&mut output, agent, true)?;
        append(&mut output, "\ndisplay_name = ", false)?;
        let display_name = match names.get(agent) {
            Some(value) => value
                .as_str()
                .ok_or(Error("unsupported_init_template_profile"))?,
            None => agent,
        };
        append(&mut output, display_name, true)?;
        append(&mut output, "\nrole = ", false)?;
        append(&mut output, &agent_role(agent), true)?;
        append(&mut output, "\nprompt = ", false)?;
        append(&mut output, &format!("prompts/{agent}.md"), true)?;
        append(&mut output, "\n", false)?;
    }
    append(
        &mut output,
        "\n[[checkpoints]]\nid = \"plan-approval\"\ndescription = \"Consult the user after the coven proposes a plan and before implementation.\"\nfile = \"checkpoints/plan-approval.md\"\nconsult_user = true\n\n[[checkpoints]]\nid = \"pre-delivery-review\"\ndescription = \"Consult the user after review/validation and before declaring the task complete.\"\nfile = \"checkpoints/pre-delivery-review.md\"\nconsult_user = true\n",
        false,
    )?;
    Ok(output)
}

pub(crate) fn agent_state_text(agent: &str) -> String {
    format!(
        "# Agent: {agent}\n\n## Role\n\n{}\n\n## Current assignment\n\n- None.\n\n## Notes\n\n- Append agent-local notes here.\n",
        agent_role(agent)
    )
}

pub(crate) fn checkpoint_files() -> Vec<(String, String)> {
    [
        (
            "plan-approval",
            "Plan Approval",
            "Approve the coven plan, acceptance criteria, and proposed implementation sequence before implementation begins.",
        ),
        (
            "pre-delivery-review",
            "Pre-delivery Review",
            "Approve final validation and review notes before the coven declares the work complete.",
        ),
    ]
    .into_iter()
    .map(|(id, title, description)| {
        (
            format!("checkpoints/{id}.md"),
            format!("# Checkpoint: {title}\n\nID: `{id}`\n\nStatus: pending\n\n## Purpose\n\n{description}\n\n## User decision\n\n- [ ] Approved\n- [ ] Rejected / needs changes\n\n## Notes\n\n- Agents should append requests, rationale, and decision notes here.\n"),
        )
    })
    .collect()
}

pub(crate) fn default_goal() -> &'static str {
    "# Goal\n\nDescribe the unified coven goal here.\n"
}

pub(crate) fn default_work() -> &'static str {
    "# Work Directory\n\nAgents should put in-progress implementation files here.\n\nFor the starter interactive demo example, this is where draft source and design\nhandoffs should be created before the final runnable artifact lands in\n`../output/demo.py`.\n"
}

pub(crate) fn default_output() -> &'static str {
    "# Output Directory\n\nAgents should put final runnable artifacts, demos, summaries, and delivery notes\nhere.\n\nFor the starter example, this should include `demo.py`, an interactive\nworkspace-aware terminal demo agent runnable with:\n\n```bash\npython3 output/demo.py\n```\n\nThe demo should answer questions about what was built, who did what, why\ndecisions were made, which checkpoints were approved, and what QA verified.\n"
}

#[cfg(test)]
#[path = "init_tests.rs"]
mod tests;
