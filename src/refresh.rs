//! Existing-workspace refresh. Only generated assets are published; no runtime
//! is contacted and authored state/history is never repaired or replayed.
//! Examples use the shared relocatable asset bundle;
//! COVEN_COMMAND defaults intentionally to `coven`, not a checkout wrapper.
use crate::{
    Error, Result,
    error::require,
    platform, read_fs,
    storage::{self, Delivery, WorkspaceGuard},
    task_delivery,
    task_mutation::TaskOutput,
    values::Value,
    workspace,
    workspace_assets::{self as assets, PureAssetContext},
};
use std::{fs, fs::File, io::Write, path::Path, time::Duration};

const MAX_TEMPLATE: usize = 1_048_576;
const MAX_STAGE: usize = 32 * MAX_TEMPLATE;
const MAX_FILES: usize = 4096;

struct Generated {
    relative: String,
    bytes: Vec<u8>,
    exclusive: bool,
}
struct Prepared {
    files: Vec<Generated>,
    dashboard: Vec<u8>,
    runtime: String,
    size: usize,
}
impl Prepared {
    fn push(&mut self, relative: String, text: String, exclusive: bool) -> Result<()> {
        require(
            text.len() <= MAX_TEMPLATE,
            "unsupported_refresh_template_profile",
        )?;
        require(
            self.files.len() < MAX_FILES && text.len() <= MAX_STAGE - self.size,
            "unsupported_refresh_stage_profile",
        )?;
        self.size += text.len();
        self.files.push(Generated {
            relative,
            bytes: text.into_bytes(),
            exclusive,
        });
        Ok(())
    }
}

fn prepare(root: &Path) -> Result<Prepared> {
    let state = workspace::load(root)?;
    let agents = state.team.agent_ids()?;
    let team = state
        .team
        .value()
        .as_object()
        .ok_or(Error("invalid_refresh_team"))?;
    let name = team
        .get("name")
        .and_then(Value::as_str)
        .ok_or(Error("invalid_refresh_team"))?;
    let command = team
        .get("opencode_command")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .unwrap_or("oc");
    let example = match team.get("example") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(Error("unsupported_refresh_example_profile"))?,
        ),
    };
    let cli_command = match assets::command_environment() {
        Ok(Some(value)) => assets::shell_quote(&value),
        Ok(None) => "coven".to_owned(),
        Err(_) => return Err(Error("unsupported_refresh_command_environment_profile")),
    };
    let context = PureAssetContext {
        cli_command,
        example_text: assets::example_text(example)?,
    };
    task_delivery::validate_checkpoint_history(root)?;
    let root_text = read_fs::path_text(root)?;
    let manifest = assets::tmux_manifest(name, &root_text, &agents, command, &context)?;
    require(
        manifest.len() <= MAX_TEMPLATE,
        "unsupported_refresh_template_profile",
    )?;
    manifest
        .parse::<toml::Value>()
        .map_err(|_| Error("invalid_refresh_manifest"))?;
    let mut result = Prepared {
        files: Vec::new(),
        dashboard: Vec::new(),
        runtime: state.multiplexer,
        size: 0,
    };
    result.push("tmux/coven.toml".into(), manifest, false)?;
    for agent in &agents {
        let relative = format!("prompts/{agent}.md");
        let path = read_fs::managed(root, Path::new(&relative), false)?;
        match fs::symlink_metadata(path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                result.push(
                    relative,
                    assets::roleprompt_text(agent, &root_text, &context),
                    true,
                )?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    for agent in &agents {
        result.push(
            format!("prompts/{agent}.start.txt"),
            assets::starter_text(agent, &root_text),
            false,
        )?;
    }
    // In-place writes must not alias any other name, including authored files
    // outside the workspace. Exclusive role creation and atomic dashboard
    // replacement do not modify an existing target inode.
    for generated in &result.files {
        if !generated.exclusive {
            let path = read_fs::managed(root, Path::new(&generated.relative), false)?;
            platform::validate_generated_target(&path)?;
        }
    }
    result.dashboard = task_delivery::prepare_dashboard_bytes(root)?;
    require(
        result.dashboard.len() <= MAX_STAGE,
        "unsupported_refresh_dashboard_profile",
    )?;
    Ok(result)
}

trait RefreshIo {
    fn prepared(&self, _root: &Path, _guarded: bool) -> Result<()> {
        Ok(())
    }
    fn open(&self, path: &Path, exclusive: bool) -> Result<File> {
        platform::open_generated(path, exclusive)
    }
    fn write(&self, file: &mut File, bytes: &[u8]) -> Result<()> {
        Ok(file.write_all(bytes)?)
    }
    fn dashboard(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        storage::atomic_replace_bytes(guard, Path::new("dashboard.md"), bytes)
    }
}
struct SystemRefreshIo;
impl RefreshIo for SystemRefreshIo {}

fn publish(
    root: &Path,
    guard: &WorkspaceGuard,
    prepared: &Prepared,
    io: &impl RefreshIo,
) -> Result<()> {
    guard.validate_for(root)?;
    for generated in &prepared.files {
        let relative = Path::new(&generated.relative);
        let parent = relative.parent().ok_or(Error("invalid_refresh_parent"))?;
        let directory = read_fs::managed(root, parent, true)?;
        match fs::create_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                read_fs::managed(root, parent, true)?;
            }
            Err(error) => return Err(error.into()),
        }
        let path = read_fs::managed(root, relative, false)?;
        if generated.exclusive && path.try_exists()? {
            continue;
        }
        let mut file = match io.open(&path, generated.exclusive) {
            Ok(file) => file,
            Err(error) if generated.exclusive => {
                // Only a newly colliding regular role file is preservable.
                let path = read_fs::managed(root, relative, false)?;
                if path.try_exists()? {
                    continue;
                }
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        io.write(&mut file, &generated.bytes)?;
    }
    match io.dashboard(guard, &prepared.dashboard)? {
        Delivery::Committed => Ok(()),
        Delivery::DurabilityUncertain => Err(Error("refresh_dashboard_durability_uncertain")),
    }
}

/// Prepare without creating the first lock, then discard and prepare fresh under
/// one five-second workspace guard. Publication errors are prepared stderr lines.
pub fn arguments(workspace: &Path) -> Result<TaskOutput> {
    arguments_with(workspace, &SystemRefreshIo)
}
fn arguments_with(workspace: &Path, io: &impl RefreshIo) -> Result<TaskOutput> {
    platform::ensure_supported()?;
    let argument = platform::workspace_argument(workspace)?;
    let root = argument
        .canonicalize()
        .map_err(|_| Error("invalid_checkpoint_workspace"))?;
    require(root.is_dir(), "invalid_checkpoint_workspace")?;
    read_fs::path_text(&root)?;
    drop(prepare(&root)?);
    io.prepared(&root, false)?;
    let guard = WorkspaceGuard::acquire(&root, Duration::from_secs(5))?;
    let prepared = prepare(&root)?;
    io.prepared(&root, true)?;
    if let Err(error) = publish(&root, &guard, &prepared, io) {
        return Ok(TaskOutput {
            stdout: String::new(),
            warnings: vec![format!(
                "[ERROR] refresh_outputs_may_be_partial: {}",
                error.code()
            )],
            exit_code: 1,
        });
    }
    Ok(TaskOutput {
        stdout: format!(
            "[INFO] Regenerated tmux/coven.toml and starter prompt files (runtime={})",
            prepared.runtime
        ),
        warnings: Vec::new(),
        exit_code: 0,
    })
}

#[cfg(test)]
mod tests;
