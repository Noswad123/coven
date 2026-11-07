use super::{MAX_OUTPUT, Result, admit, records};
use crate::error::require;

pub(super) fn slug(text: &str) -> Result<String> {
    let mut out = String::new();
    let mut separator = false;
    for c in records::trimmed(text).to_lowercase().chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') {
            out.push(c);
            separator = false;
        } else if !separator {
            out.push('-');
            separator = true;
        }
    }
    let out = out.trim_matches('-');
    require(
        out.bytes().any(|c| c.is_ascii_alphanumeric()),
        "invalid_refresh_slug",
    )?;
    Ok(out.to_owned())
}

pub(super) fn shell_quote(text: &str) -> String {
    if !text.is_empty()
        && text
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&c))
    {
        text.into()
    } else {
        format!("'{}'", text.replace('\'', "'\"'\"'"))
    }
}

// Python json.dumps defaults to ensure_ascii=True, including surrogate pairs.
fn q(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if !((' '..='~').contains(&c)) => {
                for unit in c.encode_utf16(&mut [0; 2]) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn toml_string(text: &str) -> String {
    serde_json::to_string(text)
        .unwrap()
        .replace('\u{7f}', "\\u007f")
}

fn normalize(command: &str) -> String {
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
        command.into()
    }
}

pub(super) fn manifest(
    name: &str,
    root: &str,
    agents: &[&str],
    command: &str,
    cli: &str,
) -> Result<String> {
    let session = format!("coven-{}", slug(name)?);
    let command = normalize(command);
    let mut out = format!(
        "version = 1\nname = {}\ndefault_session = {}\n\n[vars]\nCOVEN_DIR = {}\nOPENCODE_CMD = {}\n\n[[sessions]]\nname = {}\nif_exists = \"attach\"\n\n[[sessions.windows]]\nname = \"orchestrator\"\n\n[sessions.windows.layout]\nstrategy = \"services-left-playground-right\"\nplayground_pane = \"shell\"\nplayground_percent = 50\n\n",
        toml_string(&format!("Coven: {name}")),
        toml_string(&session),
        toml_string(root),
        toml_string(&command),
        toml_string(&session)
    );
    for (title, command, role) in [
        (
            "dashboard",
            "while true; do clear; cat dashboard.md; sleep 2; done".into(),
            "monitor",
        ),
        (
            "events",
            format!("while true; do clear; {cli} events; sleep 2; done"),
            "monitor",
        ),
        (
            "messages",
            format!("while true; do clear; {cli} messages; sleep 2; done"),
            "monitor",
        ),
        (
            "shell",
            "printf 'coven workspace: '; pwd; exec /bin/zsh".into(),
            "playground",
        ),
    ] {
        push(
            &mut out,
            &format!(
                "[[sessions.windows.panes]]\ntitle = {}\ndir = \"${{COVEN_DIR}}\"\ncommand = {}\nrole = {}\n\n",
                toml_string(title),
                toml_string(&command),
                toml_string(role)
            ),
        )?;
    }
    for agent in agents {
        let buffer = q(&format!("{session}-{agent}-starter"));
        let prompt_path = format!("prompts/{agent}.start.txt");
        let launch = format!(
            "printf 'Agent: {agent}\\nPrompt: {root}/prompts/{agent}.md\\n'; printf 'Auto-bootstrapping OpenCode with {prompt_path}...\\n\\n'; pane=\"$(tmux display-message -p '#{{pane_id}}')\"; (sleep 5; tmux load-buffer -b {buffer} {}; tmux paste-buffer -b {buffer} -t \"$pane\"; tmux send-keys -t \"$pane\" Enter) & {command}",
            q(&prompt_path)
        );
        push(
            &mut out,
            &format!(
                "[[sessions.windows]]\nname = {}\n\n[[sessions.windows.panes]]\ntitle = {}\ndir = \"${{COVEN_DIR}}\"\ncommand = {}\n\n",
                toml_string(&format!("agent-{agent}")),
                toml_string(agent),
                toml_string(&launch)
            ),
        )?;
    }
    // Python joins a final empty line, not an extra blank line.
    out.pop();
    require(
        out.len() <= MAX_OUTPUT,
        "unsupported_refresh_output_profile",
    )?;
    Ok(out)
}

fn push(out: &mut String, text: &str) -> Result<()> {
    let mut total = out.len();
    admit(&mut total, text.len())?;
    out.push_str(text);
    Ok(())
}

fn role(agent: &str) -> String {
    match agent.to_lowercase().as_str() {
        "orchestrator" => "Coordinate tasks, checkpoints, messages, and dashboard state.",
        "architect" => "Break goals into plans, identify tradeoffs, and define checkpoints.",
        "implementer" => "Make targeted changes and report validation results.",
        "reviewer" => "Review diffs, test outcomes, risks, and readiness for user approval.",
        "clippy" => "Planner. Turn the user goal into checkpoints, milestones, and clear task slices.",
        "srikanth" => "Tech lead. Own architecture, integration decisions, and final technical direction.",
        "jamal" => "Senior developer. Implement the core terminal application and keep changes shippable.",
        "kris" => "Frontend/UI specialist, aka Dr Oom. Shape the terminal UI, flow, copy, and presentation polish.",
        "eric" => "QA. Verify behavior, run the app, test edge cases, and confirm acceptance criteria.",
        _ => return format!("Contribute to the coven as `{agent}`."),
    }.into()
}

pub(super) fn prompt(agent: &str, root: &str, context: &str) -> String {
    let context = records::trimmed(context);
    let context = if context.is_empty() {
        String::new()
    } else {
        format!("\n{context}\n")
    };
    format!("You are `{agent}` in a coven workspace.

Workspace: `{root}`
Role: {}

Read first:
1. `goal.md`
2. `spec.md` if present
3. `dashboard.md`
4. `coven.toml`
5. `agents/{agent}.md`
6. Output of `coven task list --json` (after import, `state/tasks.json` is only a projection)
7. `logs/messages.jsonl`
8. `checkpoints/*.md`

Rules:
- Treat task state as authoritative and the logs as history, not a replay ledger.
- Do not edit shared task JSON, logs, or dashboard files directly. Use `coven task list --json`, `coven task add`, `coven task assign`, `coven message`, and `coven refresh`.
- Use `--expect-revision` from `task list --json` when a task change depends on the state you reviewed. A stale revision requires reinspection, not an automatic retry.
- If task authority is an imported ledger, add/assign require `--request-id`. On a lost/uncertain response, retry the same key with the original exact request and original expected revision, not a new key or edited arguments.
- Completion/blocker lifecycle updates are not yet a task API: report them with `coven message {agent} coven \"<summary>\"` rather than changing task JSON manually.
- Consult the user at configured checkpoints before proceeding past them.
- Do not overwrite another agent's notes; append corrections or send messages.
- Keep work scoped to your current assignment.
- Put in-progress implementation files under `work/`.
- Put final runnable/demo artifacts and handoff output under `output/`.
{context}
", role(agent))
}

pub(super) fn starter(agent: &str, root: &str) -> String {
    format!("Read `{root}/prompts/{agent}.md` and begin as `{agent}`.

Your first actions:
1. Read `goal.md`, `spec.md` if present, `dashboard.md`, `coven.toml`, `agents/{agent}.md`, and `coven task list --json` output.
2. Find your assigned task(s).
3. Record your start through `coven message {agent} coven \"Starting assigned work\"`; do not append logs directly.
4. Inspect `checkpoints/*.md`, `work/`, and `output/`.
5. Work only within your role and current assignment.
6. If you reach a checkpoint requiring user approval, stop and send the request through `coven message {agent} user \"Checkpoint <id> needs approval: <summary>\"`.

Shared task JSON, logs, and dashboard files are not editing surfaces. Use the CLI
for task changes and messages. Existing role prompts may be customized; these
coordination rules still apply even if an older prompt asks for direct edits.
Inspect task authority through `coven state inspect --json`. In imported ledger
mode, add/assign require `--request-id`; retry a lost response with the same key
and original arguments/revision. Task JSON may be stale and is not authority.

Do not wait for another prompt unless blocked. Start now.
")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_json_shell_and_toml_quoting_are_distinct() {
        assert_eq!(
            q("\u{7f}\u{1f600}\n\"\\"),
            "\"\\u007f\\ud83d\\ude00\\n\\\"\\\\\""
        );
        assert_eq!(toml_string("\u{7f}\u{1f600}"), "\"\\u007f\u{1f600}\"");
        assert_eq!(
            normalize(" oc \t--one "),
            "zsh -ic \"if command -v oc >/dev/null 2>&1; then exec oc --one; else exec opencode --one; fi\""
        );
        assert_eq!(normalize("oc\t--one"), "oc\t--one");
        assert_eq!(normalize(" custom --one "), "custom --one");
    }
}
