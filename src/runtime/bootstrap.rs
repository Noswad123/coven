//! Immediate bootstrap is not public send: native ordinary nonzero delivery
//! statuses are ignored, attempts are counted, and no messages/dashboard is written.
//! Prompt drafts retain at most 8 MiB of text, agent-ID and relative-path bytes
//! per batch, using the same native staging profile as lifecycle launch drafts.
use super::{CommandResult, TIMEOUT, legacy, namespace, object, root, transport};
use crate::{
    Error, Result,
    error::require,
    platform, read_fs,
    runtime_command::{NativeRunner, Runner},
    storage::WorkspaceGuard,
    task_delivery,
    transport_gate::TransportGuard,
    workspace, workspace_assets,
};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[cfg(test)]
mod tests;

pub fn bootstrap(workspace: &Path) -> Result<CommandResult> {
    bootstrap_with(&root(workspace)?, &NativeRunner)
}

fn bootstrap_with(root: &Path, runner: &dyn Runner) -> Result<CommandResult> {
    bootstrap_using(root, |_, multiplexer| {
        transport::Backend::new(multiplexer, runner)
    })
}

pub(super) fn bootstrap_using<'a>(
    root: &Path,
    backend: impl FnOnce(&Path, &str) -> Result<transport::Backend<'a>>,
) -> Result<CommandResult> {
    bootstrap_admitting(root, backend, |root| TransportGuard::shared(root, TIMEOUT))
}

fn bootstrap_admitting<'a>(
    root: &Path,
    backend: impl FnOnce(&Path, &str) -> Result<transport::Backend<'a>>,
    admit: impl FnOnce(&Path) -> Result<TransportGuard>,
) -> Result<CommandResult> {
    preview(root)?;
    let gate = admit(root)?;
    let root = gate.root();
    legacy(root, &gate)?;
    preview(root)?;
    let guard = WorkspaceGuard::acquire(root, TIMEOUT)?;
    legacy(root, &gate)?;
    let state = workspace::load(root)?;
    let ns = namespace(&state)?;
    // Read the entire existing UTF-8 batch before creating a single missing file.
    let prompts = prompts(root, &state.team.agent_ids()?)?;
    let backend = backend(root, &state.multiplexer)?;
    drop(guard);
    let mut sent = 0;
    let mut missing = Vec::new();
    for (agent, relative, _) in prompts {
        // Re-read ALL prompts under fresh exclusion. A late malformed prompt must
        // not let an earlier missing starter be created using stale preflight.
        let text = {
            let guard = WorkspaceGuard::acquire(root, TIMEOUT)?;
            legacy(root, &gate)?;
            let fresh = workspace::load(root)?;
            require(
                namespace(&fresh)? == ns && fresh.multiplexer == state.multiplexer,
                "runtime_configuration_changed",
            )?;
            let fresh_prompts = self::prompts(root, &fresh.team.agent_ids()?)?;
            let (_, _, text) = fresh_prompts
                .into_iter()
                .find(|(name, _, _)| name == &agent)
                .ok_or(Error("unknown_agent"))?;
            let path = read_fs::managed(root, &relative, false)?;
            if workspace::optional_bytes(
                root,
                &relative,
                workspace::MAX_TEXT,
                "unsupported_runtime_prompt_profile",
            )?
            .is_none()
            {
                let mut file = platform::open_generated(&path, true)?;
                file.write_all(text.as_bytes())?;
                file.sync_all()?;
            }
            drop(guard);
            text
        };
        let Some(target) = backend.pane(&ns, &agent)? else {
            missing.push(agent);
            continue;
        };
        if state.multiplexer == "tmux" {
            let buffer = format!("{ns}-{agent}-starter");
            let path = read_fs::path_text(&root.join(&relative))?;
            backend.capture(&["load-buffer", "-b", &buffer, &path])?;
            backend.capture(&["paste-buffer", "-b", &buffer, "-t", &target])?;
            backend.capture(&["send-keys", "-t", &target, "Enter"])?;
        } else {
            backend.capture(&["pane", "send-text", &target, &text])?;
            backend.capture(&["pane", "send-keys", &target, "enter"])?;
        }
        sent += 1;
        let history = (|| {
            let guard = WorkspaceGuard::acquire(root, TIMEOUT)?;
            legacy(root, &gate)?;
            let mut fields = vec![("agent", agent.as_str()), ("target", target.as_str())];
            if state.multiplexer == "herdr" {
                fields.push(("multiplexer", "herdr"));
            }
            task_delivery::append_event(root, "agent.bootstrap.sent", &object(fields)?, &guard)
        })();
        if let Err(error) = history {
            return Ok(super::lifecycle::progress_error(
                &format!("Bootstrap delivery attempted for {sent} agent(s)"),
                error,
            ));
        }
    }
    let mut body = String::new();
    if !missing.is_empty() {
        body.push_str(&format!("[INFO] No running agent pane found for: {}\n[INFO] If the coven session is partial or stale, run: coven up --replace <workspace>\n", missing.join(", ")));
    }
    body.push_str(&format!("[INFO] Bootstrap prompts sent: {sent}"));
    Ok(CommandResult { body, exit_code: 0 })
}

fn preview(root: &Path) -> Result<()> {
    let state = super::lifecycle::preview_workspace(root)?;
    namespace(&state)?;
    prompts(root, &state.team.agent_ids()?)?;
    Ok(())
}

fn prompts(root: &Path, agents: &[&str]) -> Result<Vec<(String, PathBuf, String)>> {
    use super::lifecycle::staging::{MAX_STAGING, Staged};
    let mut draft = Staged::new(MAX_STAGING);
    for agent in agents {
        let relative = PathBuf::from(format!("prompts/{agent}.start.txt"));
        let text = match workspace::optional_bytes(
            root,
            &relative,
            workspace::MAX_TEXT,
            "unsupported_runtime_prompt_profile",
        )? {
            Some(bytes) => std::str::from_utf8(&bytes)?
                .replace("\r\n", "\n")
                .replace('\r', "\n"),
            None => workspace_assets::starter_text(agent, &read_fs::path_text(root)?),
        };
        require(!text.contains('\0'), "invalid_runtime_text")?;
        let sizes = [agent.len(), relative.as_os_str().len(), text.len()];
        draft.push(((*agent).to_owned(), relative, text), &sizes)?;
    }
    Ok(draft.finish())
}
