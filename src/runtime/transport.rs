use super::CommandResult;
use crate::{
    Error, Result,
    error::require,
    launch::executable,
    records,
    runtime_command::{Runner, Status},
    values::{self, Profile, Value},
};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

#[cfg(test)]
use std::path::Path;

pub(super) struct Backend<'a> {
    cwd: PathBuf,
    executable: String,
    tmux: bool,
    env: BTreeMap<String, String>,
    runner: &'a dyn Runner,
    select: &'a executable::Selector<'a>,
}

#[derive(Debug)]
pub(super) struct Captured {
    pub code: i32,
    pub stdout: String,
}

#[cfg(test)]
impl<'a> Backend<'a> {
    pub(super) fn injected(root: &Path, tmux: bool, runner: &'a dyn Runner) -> Self {
        Self {
            cwd: std::env::current_dir().unwrap(),
            executable: "/injected/backend".into(),
            tmux,
            env: std::env::vars()
                .chain([("HOME".into(), root.to_string_lossy().into_owned())])
                .collect(),
            runner,
            select: &|_, _, _, _| Box::new(std::iter::once(Ok("/injected/backend".into()))),
        }
    }
}

impl<'a> Backend<'a> {
    pub(super) fn new(multiplexer: &str, runner: &'a dyn Runner) -> Result<Self> {
        // Python legacy commands inherit ALL ambient environment, including
        // backend socket/session overrides. Frozen launch's six-key set is wrong here.
        let env = std::env::vars().collect();
        Ok(Self {
            // Legacy Python inherits the caller cwd, not the workspace. Freeze
            // it once for both relative PATH selection and native execution.
            cwd: std::env::current_dir()?,
            executable: multiplexer.to_owned(),
            tmux: multiplexer == "tmux",
            env,
            runner,
            select: &executable::candidates,
        })
    }

    pub(super) fn capture(&self, args: &[&str]) -> Result<Captured> {
        let mut argv = vec![self.executable.clone()];
        argv.extend(args.iter().map(|s| (*s).to_owned()));
        // Use the same execvpe policy as frozen launch, but with the captured
        // complete legacy environment. Only proven pre-image failures advance
        // PATH; lookup and all spawn attempts share one command deadline.
        let output = executable::run(
            argv,
            &self.cwd,
            &self.env,
            self.runner,
            self.select,
            Duration::from_secs(60),
        )
        .map_err(|(kind, _)| {
            Error(match kind {
                "TimeoutExpired" => "runtime_timeout_before_execution",
                _ => "runtime_executable_unavailable",
            })
        })?;
        require(
            output.stdout.len().saturating_add(output.stderr.len()) <= 1_048_576,
            "runtime_output_limit_effects_unknown",
        )?;
        let code = match output.status {
            Status::Exited(code) => code,
            Status::Timeout => return Err(Error("runtime_timeout_effects_unknown")),
            Status::OutputLimit => return Err(Error("runtime_output_limit_effects_unknown")),
            Status::SpawnError => return Err(Error("runtime_command_effects_unknown")),
            Status::ExecFailed => return Err(Error("runtime_command_exec_failed")),
        };
        let universal_newlines = |bytes: &[u8]| -> Result<String> {
            Ok(std::str::from_utf8(bytes)?
                .replace("\r\n", "\n")
                .replace('\r', "\n"))
        };
        // Validate/decode both streams for capture parity, but do not retain
        // peer diagnostics: native errors intentionally expose fixed codes only.
        universal_newlines(&output.stderr)?;
        Ok(Captured {
            code,
            stdout: universal_newlines(&output.stdout)?,
        })
    }

    fn run(&self, args: &[&str]) -> Result<(i32, String)> {
        let output = self.capture(args)?;
        Ok((output.code, output.stdout))
    }

    pub(super) fn foreground(
        &self,
        args: &[&str],
        runner: &dyn crate::runtime_foreground::Runner,
    ) -> Result<i32> {
        // Interactive execution has inherited stdio and no command deadline.
        // Selection still uses the same frozen cwd/environment and PATH policy.
        for candidate in executable::candidates_unbounded(&self.executable, &self.cwd, &self.env) {
            let candidate = candidate.map_err(|_| Error("runtime_executable_unavailable"))?;
            let mut argv = vec![candidate];
            argv.extend(args.iter().map(|arg| (*arg).to_owned()));
            match runner.run(&crate::runtime_foreground::Request {
                argv,
                cwd: self.cwd.clone(),
                env: self.env.clone(),
            }) {
                Status::ExecFailed => continue,
                Status::Exited(code) => return Ok(code),
                Status::SpawnError => return Err(Error("runtime_command_effects_unknown")),
                Status::Timeout => return Err(Error("runtime_timeout_effects_unknown")),
                Status::OutputLimit => return Err(Error("runtime_output_limit_effects_unknown")),
            }
        }
        Err(Error("runtime_executable_unavailable"))
    }

    fn list(&self, args: &[&str], key: &str) -> Result<Vec<Value>> {
        let (code, stdout) = self.run(args)?;
        if code != 0 {
            return Ok(Vec::new());
        }
        let payload = match values::parse(stdout.as_bytes(), Profile::LegacySnapshot) {
            Ok(payload) => payload,
            Err(_) => return Ok(Vec::new()),
        };
        Ok(payload
            .as_object()
            .and_then(|v| v.get("result"))
            .and_then(Value::as_object)
            .and_then(|v| v.get(key))
            .and_then(Value::as_array)
            .map(<[Value]>::to_vec)
            .unwrap_or_default())
    }

    pub(super) fn workspace(&self, namespace: &str) -> Result<Option<Value>> {
        Ok(self
            .list(&["workspace", "list"], "workspaces")?
            .into_iter()
            .find(|v| field(v, "label") == Some(namespace)))
    }

    pub(super) fn tab(&self, namespace: &str, agent: &str) -> Result<Option<String>> {
        if self.tmux {
            let target = format!("{namespace}:agent-{agent}");
            return Ok(self.pane(namespace, agent)?.map(|_| target));
        }
        Ok(self.native_target(namespace, agent)?.map(|(_, tab)| tab))
    }

    fn tab_record(&self, namespace: &str, agent: &str) -> Result<Option<Value>> {
        let Some(workspace) = self.workspace(namespace)? else {
            return Ok(None);
        };
        let Some(workspace_id) = handle(&workspace, "workspace_id")? else {
            return Ok(None);
        };
        let label = format!("agent-{agent}");
        Ok(self
            .list(&["tab", "list", "--workspace", &workspace_id], "tabs")?
            .into_iter()
            .find(|v| field(v, "label") == Some(label.as_str())))
    }

    pub(super) fn pane(&self, namespace: &str, agent: &str) -> Result<Option<String>> {
        if self.tmux {
            let (code, stdout) = self.run(&[
                "list-panes",
                "-t",
                &format!("{namespace}:agent-{agent}"),
                "-F",
                "#{pane_id}",
            ])?;
            if code != 0 || records::trimmed(&stdout).is_empty() {
                return Ok(None);
            }
            let pane = records::trimmed(&stdout)
                .lines()
                .next()
                .ok_or(Error("invalid_runtime_handle"))?;
            require(
                pane.starts_with('%')
                    && pane.len() > 1
                    && pane[1..].bytes().all(|b| b.is_ascii_digit()),
                "invalid_runtime_handle",
            )?;
            return Ok(Some(pane.into()));
        }
        Ok(self.native_target(namespace, agent)?.map(|(pane, _)| pane))
    }

    fn native_target(&self, namespace: &str, agent: &str) -> Result<Option<(String, String)>> {
        let Some(tab) = self.tab_record(namespace, agent)? else {
            return Ok(None);
        };
        let Some(workspace_id) = handle(&tab, "workspace_id")? else {
            return Ok(None);
        };
        let Some(tab_id) = handle(&tab, "tab_id")? else {
            return Ok(None);
        };
        for pane in self.list(&["pane", "list", "--workspace", &workspace_id], "panes")? {
            if field(&pane, "tab_id") != Some(tab_id.as_str()) {
                continue;
            }
            if let Some(id) = handle(&pane, "pane_id")? {
                return Ok(Some((id, tab_id)));
            }
        }
        Ok(None)
    }

    pub(super) fn send(&self, namespace: &str, agent: &str, text: &str) -> Result<bool> {
        let Some(pane) = self.pane(namespace, agent)? else {
            return Ok(false);
        };
        let commands = if self.tmux {
            vec![vec!["send-keys", "-t", &pane, text, "Enter"]]
        } else {
            vec![
                vec!["pane", "send-text", &pane, text],
                vec!["pane", "send-keys", &pane, "enter"],
            ]
        };
        for args in commands {
            require(self.run(&args)?.0 == 0, "agent_pane_delivery_failed")?;
        }
        Ok(true)
    }

    pub(super) fn down(&self, namespace: &str) -> Result<CommandResult> {
        if self.tmux {
            if self.run(&["has-session", "-t", namespace])?.0 != 0 {
                return Ok(CommandResult {
                    body: format!("[INFO] Coven tmux session is not running: {namespace}"),
                    exit_code: 0,
                });
            }
            let (code, body) = self.run(&["kill-session", "-t", namespace])?;
            return Ok(CommandResult {
                body: body.trim_end_matches('\n').to_owned(),
                exit_code: u8::try_from(code).unwrap_or(1),
            });
        }
        let Some(workspace) = self.workspace(namespace)? else {
            return Ok(CommandResult {
                body: format!("[INFO] Coven Herdr workspace is not running: {namespace}"),
                exit_code: 0,
            });
        };
        if let Some(id) = handle(&workspace, "workspace_id")? {
            require(
                self.run(&["workspace", "close", &id])?.0 == 0,
                "runtime_workspace_close_failed",
            )?;
        }
        Ok(CommandResult {
            body: String::new(),
            exit_code: 0,
        })
    }
}

#[cfg(all(test, unix))]
#[path = "transport/tests.rs"]
mod tests;

pub(super) fn field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.as_object()?.get(key)?.as_str()
}

pub(super) fn handle(value: &Value, key: &str) -> Result<Option<String>> {
    let Some(raw) = value.as_object().and_then(|v| v.get(key)) else {
        return Ok(None);
    };
    if matches!(raw, Value::Null) || raw.as_str() == Some("") {
        return Ok(None);
    }
    let id = raw.as_str().ok_or(Error("invalid_runtime_handle"))?;
    require(
        (1..=128).contains(&id.len())
            && !id.starts_with('-')
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "invalid_runtime_handle",
    )?;
    Ok(Some(id.into()))
}
