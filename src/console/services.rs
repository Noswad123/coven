use super::*;
use crate::{checkpoint_guidance, lead, runtime, workspace, workspace_commands};

pub(super) trait Services {
    fn initial(&mut self, root: &Path) -> Result<(String, Vec<String>)>;
    fn standup(&mut self, root: &Path, out: &mut impl Write, err: &mut impl Write) -> Result<()>;
    fn status(&mut self, root: &Path, out: &mut impl Write, err: &mut impl Write) -> Result<()>;
    fn next(&mut self, root: &Path, out: &mut impl Write, err: &mut impl Write) -> Result<()>;
    fn agents(&mut self, root: &Path, out: &mut impl Write, err: &mut impl Write) -> Result<()>;
    fn send(
        &mut self,
        root: &Path,
        selector: &str,
        text: &str,
        out: &mut impl Write,
        err: &mut impl Write,
    ) -> Result<()>;
    fn lead(
        &mut self,
        root: &Path,
        tokens: &[&str],
        out: &mut impl Write,
        err: &mut impl Write,
    ) -> Result<()>;
    fn display(&mut self, root: &Path, id: &str, out: &mut impl Write) -> Result<String>;
    fn decide(
        &mut self,
        root: &Path,
        request: checkpoint_decisions::DecisionRequest,
    ) -> Result<checkpoint_decisions::CommandResult>;
    fn review(
        &mut self,
        root: &Path,
        ids: &[String],
        input: &mut impl BufRead,
        out: &mut impl Write,
    ) -> Result<u8>;
}

pub(super) struct NativeServices;

fn task_output(
    result: crate::task_mutation::TaskOutput,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<()> {
    write!(out, "{}", result.stdout)?;
    for warning in result.warnings {
        writeln!(err, "{warning}")?;
    }
    Ok(())
}

impl Services for NativeServices {
    fn initial(&mut self, root: &Path) -> Result<(String, Vec<String>)> {
        let state = workspace::load(root)?;
        let name = state
            .team
            .value()
            .as_object()
            .and_then(|o| o.get("name"))
            .and_then(crate::values::Value::as_str)
            .ok_or(Error("invalid_team_record"))?
            .to_owned();
        Ok((
            name,
            state
                .team
                .agent_ids()?
                .into_iter()
                .map(str::to_owned)
                .collect(),
        ))
    }
    fn standup(&mut self, root: &Path, out: &mut impl Write, _err: &mut impl Write) -> Result<()> {
        writeln!(out, "{}", runtime::standup(root)?.body)?;
        Ok(())
    }
    fn status(&mut self, root: &Path, out: &mut impl Write, err: &mut impl Write) -> Result<()> {
        task_output(workspace_commands::status_arguments(root)?, out, err)
    }
    fn next(&mut self, root: &Path, out: &mut impl Write, err: &mut impl Write) -> Result<()> {
        task_output(checkpoint_guidance::next_arguments(root)?, out, err)
    }
    fn agents(&mut self, root: &Path, out: &mut impl Write, _err: &mut impl Write) -> Result<()> {
        writeln!(out, "{}", runtime::agent_status(root, &[])?.body)?;
        Ok(())
    }
    fn send(
        &mut self,
        root: &Path,
        selector: &str,
        text: &str,
        out: &mut impl Write,
        _err: &mut impl Write,
    ) -> Result<()> {
        // Ordinary command result codes do not contribute to console status.
        // Errors still propagate, and partial-effect diagnostics are printed.
        writeln!(out, "{}", runtime::send(root, selector, text)?.body)?;
        Ok(())
    }
    fn lead(
        &mut self,
        root: &Path,
        tokens: &[&str],
        out: &mut impl Write,
        err: &mut impl Write,
    ) -> Result<()> {
        let operation = match tokens.first().copied() {
            None | Some("status" | "show") => lead::Operation::Show,
            Some("clear" | "remove" | "unset") => lead::Operation::Clear,
            Some("set") if tokens.len() > 1 => lead::Operation::Set(tokens[1].into()),
            Some(agent) => lead::Operation::Set(agent.into()),
        };
        let result = lead::command(root, operation, None)?;
        writeln!(out, "{}", result.body)?;
        for warning in result.warnings {
            writeln!(err, "{warning}")?;
        }
        Ok(())
    }
    fn display(&mut self, root: &Path, id: &str, out: &mut impl Write) -> Result<String> {
        checkpoint_decisions::display_io(root, id, out)
    }
    fn decide(
        &mut self,
        root: &Path,
        request: checkpoint_decisions::DecisionRequest,
    ) -> Result<checkpoint_decisions::CommandResult> {
        checkpoint_decisions::decide(root, request, false)
    }
    fn review(
        &mut self,
        root: &Path,
        ids: &[String],
        input: &mut impl BufRead,
        out: &mut impl Write,
    ) -> Result<u8> {
        checkpoint_decisions::review_io(root, ids, "all", "", input, out)
    }
}
