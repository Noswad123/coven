//! Python-compatible interactive console. Services own current-state validation;
//! the initial roster is only a shortcut vocabulary, never mutation authority.
use crate::{Error, Result, checkpoint_decisions, error::require, records};
use std::{
    io::{self, BufRead, Write},
    path::Path,
};

mod services;
use services::{NativeServices, Services};

const HELP: &str = "Commands:
  help                         Show this help
  standup                      Show Scrum-style coven summary
  status                       Show dashboard
  next                         Show recommended next action
  agents                       Show agent runtime status
  send <agent|all> <message>   Send a multiplexer-backed message
  approve [checkpoint]         Approve a checkpoint (default: plan-approval)
  suggest <checkpoint> <text>  Save/send requested checkpoint changes
  review [checkpoint]          Interactive checkpoint review
  lead [agent|status|clear]    Set/show/clear the coven lead
  quit                         Exit

Shortcut: `<agent|all> <message>` also sends a message.
";

pub fn run(workspace: &Path) -> Result<u8> {
    let root = crate::platform::workspace_argument(workspace)?.canonicalize()?;
    let (stdin, stdout, stderr) = (io::stdin(), io::stdout(), io::stderr());
    run_with(
        &root,
        &mut stdin.lock(),
        &mut stdout.lock(),
        &mut stderr.lock(),
        &mut NativeServices,
    )
}

// Per-line bound, not a session transcript bound: arbitrarily long sessions can
// stream through constant space. Python's full whitespace set is used below.
fn read_line(input: &mut impl BufRead) -> Result<Option<String>> {
    let mut bytes = Vec::new();
    std::io::Read::take(input, 1_048_579).read_until(b'\n', &mut bytes)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    require(
        bytes.len() <= 1_048_576,
        "unsupported_console_input_profile",
    )?;
    Ok(Some(
        String::from_utf8(bytes).map_err(|_| Error("invalid_console_utf8"))?,
    ))
}

// Unlike split_whitespace, the unsplit remainder retains interior whitespace.
fn split_once(raw: &str) -> (&str, Option<&str>) {
    match raw.find(records::python_space) {
        Some(index) => (
            &raw[..index],
            Some(raw[index..].trim_start_matches(records::python_space)),
        ),
        None => (raw, None),
    }
}

fn run_with(
    root: &Path,
    input: &mut impl BufRead,
    output: &mut impl Write,
    errors: &mut impl Write,
    services: &mut impl Services,
) -> Result<u8> {
    let (name, agents) = services.initial(root)?;
    writeln!(output, "coven console: {name} ({})", root.display())?;
    writeln!(output, "{HELP}")?;
    let mut status = 0;
    loop {
        write!(output, "coven> ")?;
        output.flush()?;
        let Some(line) = read_line(input)? else {
            writeln!(output)?;
            return Ok(status);
        };
        let raw = records::trimmed(&line);
        match raw {
            "" => continue,
            "q" | "quit" | "exit" => return Ok(status),
            "h" | "help" | "?" => {
                writeln!(output, "{HELP}")?;
                continue;
            }
            "standup" => {
                services.standup(root, output, errors)?;
                continue;
            }
            "status" => {
                services.status(root, output, errors)?;
                continue;
            }
            "next" => {
                services.next(root, output, errors)?;
                continue;
            }
            "agents" | "agent status" => {
                services.agents(root, output, errors)?;
                continue;
            }
            _ => {}
        }
        let (command, rest) = split_once(raw);
        let (argument, text) = rest.map(split_once).unwrap_or(("", None));
        match command {
            "review" => {
                let ids = rest.map(|_| vec![argument.to_owned()]).unwrap_or_default();
                status = status.max(services.review(root, &ids, input, output)?);
            }
            "lead" => {
                // Console tokens are not CLI flags; --clear is an agent name.
                let tokens: Vec<_> = rest
                    .unwrap_or("")
                    .split(records::python_space)
                    .filter(|s| !s.is_empty())
                    .collect();
                services.lead(root, &tokens, output, errors)?;
            }
            "approve" | "suggest" => {
                if command == "suggest" && text.is_none() {
                    writeln!(output, "Usage: suggest <checkpoint> <requested changes>")?;
                    continue;
                }
                let id = if rest.is_none() {
                    "plan-approval"
                } else {
                    argument
                };
                let (action, notes, label) = if command == "approve" {
                    (
                        checkpoint_decisions::DecisionAction::Approve,
                        "Approved displayed checkpoint content from console.",
                        "approved",
                    )
                } else {
                    (
                        checkpoint_decisions::DecisionAction::Suggest,
                        text.unwrap_or_default(),
                        "changes_requested",
                    )
                };
                let revision = services.display(root, id, output)?;
                write!(
                    output,
                    "Record {label} for this displayed snapshot? [y/N] > "
                )?;
                output.flush()?;
                let Some(answer) = read_line(input)? else {
                    continue;
                };
                if !matches!(
                    records::trimmed(&answer).to_lowercase().as_str(),
                    "y" | "yes"
                ) {
                    writeln!(output, "[INFO] No checkpoint decision saved")?;
                    continue;
                }
                let request = checkpoint_decisions::DecisionRequest {
                    checkpoint: id.into(),
                    action,
                    notes: notes.into(),
                    source: "console".into(),
                    agent: "all".into(),
                    expected_revision: Some(revision),
                };
                match services.decide(root, request) {
                    Ok(result) => {
                        writeln!(output, "{}", result.body)?;
                        status = status.max(result.exit_code);
                    }
                    Err(error) if error.code() == "stale_checkpoint_revision" => {
                        writeln!(
                            errors,
                            "[WARN] Stale checkpoint revision. Reinspect and confirm explicitly; no retry was made."
                        )?;
                        errors.flush()?;
                        status = status.max(1);
                    }
                    Err(error) => return Err(error),
                }
            }
            "send" => {
                if let Some(text) = text {
                    services.send(root, argument, text, output, errors)?;
                } else {
                    writeln!(output, "Usage: send <agent|all> <message>")?;
                }
            }
            command
                if rest.is_some() && (command == "all" || agents.iter().any(|a| a == command)) =>
            {
                services.send(root, command, rest.unwrap_or_default(), output, errors)?;
            }
            _ => writeln!(output, "Unknown command. Type `help`.")?,
        }
    }
}

#[cfg(test)]
mod tests;
