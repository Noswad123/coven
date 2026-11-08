//! Workspace templates and the shared relocatable example asset service.
//! Shell commands here are inert template data, never executed by refresh.
use crate::{Result, error::require, records};

#[path = "example_assets.rs"]
pub(crate) mod example_assets;

#[path = "workspace_assets/init.rs"]
mod init;
pub(crate) use init::{
    InitExample, agent_state_text, checkpoint_files, default_goal, default_output, default_work,
    init_example, team_toml,
};

pub(crate) struct PureAssetContext {
    pub cli_command: String,
    pub example_text: String,
}

pub(crate) fn command_environment() -> std::result::Result<Option<String>, std::env::VarError> {
    for key in ["COVEN_COMMAND", "ARCANA_PUBLIC_COVEN"] {
        match std::env::var(key) {
            Ok(value) if !value.is_empty() => return Ok(Some(value)),
            Ok(_) | Err(std::env::VarError::NotPresent) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

pub(crate) fn example_text(example: Option<&str>) -> Result<String> {
    if example.is_none() {
        return Ok(String::new());
    }
    example_assets::AssetBundle::environment()?.context(example)
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

pub(crate) fn agent_launch_command(
    name: &str,
    root: &str,
    agent: &str,
    command: &str,
    multiplexer: &str,
) -> Result<String> {
    // Reject original bytes before oc normalization can JSON-escape the NUL.
    require(!command.contains('\0'), "invalid_runtime_command")?;
    require(
        command.len() <= 1_048_576,
        "unsupported_refresh_template_profile",
    )?;
    normalized_agent_launch_command(
        name,
        root,
        agent,
        &normalize_opencode_command(command),
        multiplexer,
    )
}

fn normalized_agent_launch_command(
    name: &str,
    root: &str,
    agent: &str,
    command: &str,
    multiplexer: &str,
) -> Result<String> {
    require(
        agent.len() <= 255 - ".start.txt".len()
            && root.len() <= 1_048_576
            && !root.contains('\0')
            && command.len() <= 1_048_576
            && name.len() <= 1_048_576,
        "unsupported_refresh_template_profile",
    )?;
    // Reuse the record identifier contract; path components then cannot escape
    // prompts/, and the longest generated basename fits the filesystem bound.
    records::lead(&crate::values::parse(
        format!("{{\"agent\":{}}}", q(agent)).as_bytes(),
        crate::values::Profile::LegacySnapshot,
    )?)?;
    records::multiplexer(multiplexer)?;
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
    let launch = if records::trimmed(multiplexer).eq_ignore_ascii_case("herdr") {
        format!(
            "{label}; printf 'Auto-bootstrapping OpenCode with {prompt}...\\n\\n'; pane=\"${{HERDR_PANE_ID:-}}\"; herdr_bin=\"${{HERDR_BIN_PATH:-herdr}}\"; (sleep 5; if [ -n \"$pane\" ]; then prompt=\"$(cat {})\"; \"$herdr_bin\" pane send-text \"$pane\" \"$prompt\"; \"$herdr_bin\" pane send-keys \"$pane\" enter; fi) & {command}",
            shell_quote(&prompt)
        )
    } else {
        let buffer = q(&format!("coven-{}-{agent}-starter", slug(name)?));
        format!(
            "{label}; printf 'Auto-bootstrapping OpenCode with {prompt}...\\n\\n'; pane=\"$(tmux display-message -p '#{{pane_id}}')\"; (sleep 5; tmux load-buffer -b {buffer} {}; tmux paste-buffer -b {buffer} -t \"$pane\"; tmux send-keys -t \"$pane\" Enter) & {command}",
            q(&prompt)
        )
    };
    require(!launch.contains('\0'), "invalid_runtime_command")?;
    require(
        launch.len() <= 1_048_576,
        "unsupported_refresh_template_profile",
    )?;
    Ok(launch)
}

pub(crate) fn tmux_manifest(
    name: &str,
    root: &str,
    agents: &[&str],
    command: &str,
    context: &PureAssetContext,
) -> Result<String> {
    require(!command.contains('\0'), "invalid_runtime_command")?;
    let session = format!("coven-{}", slug(name)?);
    let normalized_command = normalize_opencode_command(command);
    let tq = toml_quote;
    let mut output = format!(
        "version = 1\nname = {}\ndefault_session = {}\n\n[vars]\nCOVEN_DIR = {}\nOPENCODE_CMD = {}\n\n[[sessions]]\nname = {}\nif_exists = \"attach\"\n\n[[sessions.windows]]\nname = \"orchestrator\"\n\n[sessions.windows.layout]\nstrategy = \"services-left-playground-right\"\nplayground_pane = \"shell\"\nplayground_percent = 50\n",
        tq(&format!("Coven: {name}")),
        tq(&session),
        tq(root),
        tq(&normalized_command),
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
        let launch =
            normalized_agent_launch_command(name, root, agent, &normalized_command, "tmux")?;
        output.push_str(&format!("\n[[sessions.windows]]\nname = {}\n\n[[sessions.windows.panes]]\ntitle = {}\ndir = \"${{COVEN_DIR}}\"\ncommand = {}\n", tq(&format!("agent-{agent}")), tq(agent), tq(&launch)));
        require(
            output.len() <= 1_048_576,
            "unsupported_refresh_template_profile",
        )?;
    }
    Ok(output)
}

#[cfg(test)]
#[path = "workspace_assets/tests.rs"]
mod tests;
