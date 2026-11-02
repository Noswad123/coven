//! Frozen workspace assets, ported from runner/coven.py (native asset profile v1).
//! Shell commands here are inert template data, never executed by refresh.
use crate::{Error, Result, error::require, records};

mod init;
pub(crate) use init::{
    InitExample, agent_state_text, checkpoint_files, default_goal, default_output, default_work,
    init_example, team_toml,
};

pub(crate) struct PureAssetContext {
    pub cli_command: String,
    pub example_text: String,
}

pub(crate) fn example_text(example: Option<&str>) -> Result<String> {
    match example {
        None | Some("") => Ok(String::new()),
        Some(value) if slug(value).is_ok_and(|value| value == "interactive-demo") => {
            Ok(include_str!("../examples/interactive-demo/prompt-context.md").to_owned())
        }
        Some(_) => Err(Error("unsupported_refresh_example_profile")),
    }
}

pub(crate) fn slug(value: &str) -> Result<String> {
    let lower = records::trimmed(value).to_lowercase();
    let mut output = String::new();
    let mut separator = false;
    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
            output.push(ch);
            separator = false;
        } else if !separator {
            output.push('-');
            separator = true;
        }
    }
    let output = output.trim_matches('-');
    require(
        output.bytes().any(|c| c.is_ascii_alphanumeric()),
        "invalid_refresh_slug",
    )?;
    Ok(output.to_owned())
}

pub(crate) fn agent_role(agent: &str) -> String {
    match agent.to_lowercase().as_str() {
        "orchestrator" => "Coordinate tasks, checkpoints, messages, and dashboard state.".into(),
        "architect" => "Break goals into plans, identify tradeoffs, and define checkpoints.".into(),
        "implementer" => "Make targeted changes and report validation results.".into(),
        "reviewer" => "Review diffs, test outcomes, risks, and readiness for user approval.".into(),
        "clippy" => "Planner. Turn the user goal into checkpoints, milestones, and clear task slices.".into(),
        "srikanth" => "Tech lead. Own architecture, integration decisions, and final technical direction.".into(),
        "jamal" => "Senior developer. Implement the core terminal application and keep changes shippable.".into(),
        "kris" => "Frontend/UI specialist, aka Dr Oom. Shape the terminal UI, flow, copy, and presentation polish.".into(),
        "eric" => "QA. Verify behavior, run the app, test edge cases, and confirm acceptance criteria.".into(),
        _ => format!("Contribute to the coven as `{agent}`."),
    }
}

pub(crate) fn roleprompt_text(agent: &str, root: &str, context: &PureAssetContext) -> String {
    let role = agent_role(agent);
    let text = records::trimmed(&context.example_text);
    let context_block = if text.is_empty() {
        String::new()
    } else {
        format!("\n{text}\n")
    };
    format!(
        include_str!("workspace_assets/role.txt"),
        agent = agent,
        root = root,
        role = role,
        context_block = context_block
    )
}

pub(crate) fn starter_text(agent: &str, root: &str) -> String {
    format!(
        include_str!("workspace_assets/starter.txt"),
        agent = agent,
        root = root
    )
}

// Python json.dumps defaults to ensure_ascii=True; TOML keeps Unicode scalars.
fn quote(value: &str, ascii: bool) -> String {
    use std::fmt::Write;
    let mut result = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\u{8}' => result.push_str("\\b"),
            '\u{c}' => result.push_str("\\f"),
            ch if ch < ' ' || ch == '\u{7f}' || (ascii && !ch.is_ascii()) => {
                let mut units = [0; 2];
                for unit in ch.encode_utf16(&mut units) {
                    write!(result, "\\u{unit:04x}").expect("writing to String");
                }
            }
            ch => result.push(ch),
        }
    }
    result.push('"');
    result
}
pub(crate) fn q(value: &str) -> String {
    quote(value, true)
}
fn toml_quote(value: &str) -> String {
    quote(value, false)
}

pub(crate) fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&c))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

pub(crate) fn normalize_opencode_command(command: &str) -> String {
    let command = records::trimmed(command);
    if command == "oc" || command.starts_with("oc ") {
        let args = records::trimmed(&command[2..]);
        let suffix = if args.is_empty() {
            String::new()
        } else {
            format!(" {args}")
        };
        format!(
            "zsh -ic {}",
            q(&format!(
                "if command -v oc >/dev/null 2>&1; then exec oc{suffix}; else exec opencode{suffix}; fi"
            ))
        )
    } else {
        command.to_owned()
    }
}

pub(crate) fn tmux_manifest(
    name: &str,
    root: &str,
    agents: &[&str],
    command: &str,
    context: &PureAssetContext,
) -> Result<String> {
    let session = format!("coven-{}", slug(name)?);
    let command = normalize_opencode_command(command);
    let tq = toml_quote;
    let mut output = format!(
        "version = 1\nname = {}\ndefault_session = {}\n\n[vars]\nCOVEN_DIR = {}\nOPENCODE_CMD = {}\n\n[[sessions]]\nname = {}\nif_exists = \"attach\"\n\n[[sessions.windows]]\nname = \"orchestrator\"\n\n[sessions.windows.layout]\nstrategy = \"services-left-playground-right\"\nplayground_pane = \"shell\"\nplayground_percent = 50\n",
        tq(&format!("Coven: {name}")),
        tq(&session),
        tq(root),
        tq(&command),
        tq(&session)
    );
    for (title, command, role) in [
        (
            "dashboard",
            "while true; do clear; cat dashboard.md; sleep 2; done".to_owned(),
            "monitor",
        ),
        (
            "events",
            format!(
                "while true; do clear; {} events; sleep 2; done",
                context.cli_command
            ),
            "monitor",
        ),
        (
            "messages",
            format!(
                "while true; do clear; {} messages; sleep 2; done",
                context.cli_command
            ),
            "monitor",
        ),
        (
            "shell",
            "printf 'coven workspace: '; pwd; exec /bin/zsh".to_owned(),
            "playground",
        ),
    ] {
        output.push_str(&format!("\n[[sessions.windows.panes]]\ntitle = {}\ndir = \"${{COVEN_DIR}}\"\ncommand = {}\nrole = {}\n", tq(title), tq(&command), tq(role)));
    }
    for agent in agents {
        let buffer = q(&format!("{session}-{agent}-starter"));
        let prompt = format!("prompts/{agent}.start.txt");
        // Preserve legacy bytes for ordinary paths, but never interpolate shell
        // quotes or printf format/escape sequences from workspace metadata.
        let label = if root.contains(['\'', '%', '\\']) {
            format!(
                "printf '%s\\n' {} {}",
                shell_quote(&format!("Agent: {agent}")),
                shell_quote(&format!("Prompt: {root}/prompts/{agent}.md"))
            )
        } else {
            format!("printf 'Agent: {agent}\\nPrompt: {root}/prompts/{agent}.md\\n'")
        };
        let launch = format!(
            "{label}; printf 'Auto-bootstrapping OpenCode with {prompt}...\\n\\n'; pane=\"$(tmux display-message -p '#{{pane_id}}')\"; (sleep 5; tmux load-buffer -b {buffer} {}; tmux paste-buffer -b {buffer} -t \"$pane\"; tmux send-keys -t \"$pane\" Enter) & {command}",
            q(&prompt)
        );
        output.push_str(&format!("\n[[sessions.windows]]\nname = {}\n\n[[sessions.windows.panes]]\ntitle = {}\ndir = \"${{COVEN_DIR}}\"\ncommand = {}\n", tq(&format!("agent-{agent}")), tq(agent), tq(&launch)));
        require(
            output.len() <= 1_048_576,
            "unsupported_refresh_template_profile",
        )?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
