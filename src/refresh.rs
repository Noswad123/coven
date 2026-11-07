//! Native full refresh, not initialization or authority repair.
//!
//! Preview assets come from CARGO_MANIFEST_DIR/examples and the default launch
//! command is CARGO_MANIFEST_DIR/bin/coven (the retained Python entrypoint).
//! Installed asset/entrypoint cutover is deliberately deferred. No interpreter,
//! runtime, or subprocess is called here. Explicit COVEN_COMMAND is one shell word.
//! In addition to shared workspace/value/log limits, text inputs are <=1 MiB,
//! generated outputs together <=32 MiB. Missing configured assets refuse.
//! Consumed checkpoint-reference text is capped at 256 MiB per prepare pass.
use crate::{
    Error, Result,
    error::require,
    platform, read_fs, records,
    storage::{self, Delivery, WorkspaceGuard},
    task_delivery,
    values::{self, Profile, Value},
    workspace,
};
use std::{fs, io::Write, path::Path, time::Duration};

mod templates;

const MAX_TEXT: usize = 1_048_576;
const MAX_OUTPUT: usize = 32 * MAX_TEXT;
const PROFILE: &str = "unsupported_refresh_input_profile";

struct Prepared {
    manifest: String,
    missing: Vec<(String, String)>,
    starters: Vec<(String, String)>,
    dashboard: String,
    multiplexer: String,
}

/// CLI path expansion is kept separate from the literal-path library API.
pub fn refresh_argument(path: &Path) -> Result<String> {
    refresh(&platform::workspace_argument(path)?)
}

pub fn refresh(path: &Path) -> Result<String> {
    let root = read_fs::workspace(path)?;
    run(&root, &SystemIo)
}

trait RefreshIo {
    fn observe(&self, _phase: &str, _root: &Path) -> Result<()> {
        Ok(())
    }
    fn write(&self, root: &Path, relative: &str, text: &str, missing: bool) -> Result<()> {
        direct_write(root, relative, text, missing)
    }
    fn dashboard(&self, guard: &WorkspaceGuard, text: &str) -> Result<Delivery> {
        storage::atomic_replace_bytes(guard, Path::new("dashboard.md"), text.as_bytes())
    }
}
struct SystemIo;
impl RefreshIo for SystemIo {}

fn run(root: &Path, io: &impl RefreshIo) -> Result<String> {
    // Discard every output, including dashboard. Deterministic failures must not
    // create even a legacy workspace's first coordination inode.
    drop(prepare(root)?);
    io.observe("draft", root)?;
    workspace::load(root)?; // outer workspace_mutation pre-context preflight
    io.observe("outer", root)?;
    let guard = WorkspaceGuard::acquire(root, Duration::from_secs(5))?;
    io.observe("locked", root)?;
    workspace::load(root)?; // outer held-context preflight
    io.observe("outer-held", root)?;
    workspace::load(root)?; // nested workspace_mutation pre-context preflight
    io.observe("nested-before", root)?;
    guard.validate_for(root)?;
    workspace::load(root)?; // nested borrowed-context preflight
    io.observe("nested", root)?;
    let prepared = prepare(root)?;
    io.observe("prepared", root)?;
    guard.validate_for(root)?;
    io.write(root, "tmux/coven.toml", &prepared.manifest, false)?;
    for (path, text) in &prepared.missing {
        io.write(root, path, text, true)?;
    }
    for (path, text) in &prepared.starters {
        io.write(root, path, text, false)?;
    }
    match io.dashboard(&guard, &prepared.dashboard)? {
        Delivery::Committed => {}
        Delivery::DurabilityUncertain => return Err(Error("dashboard_synchronization_uncertain")),
    }
    Ok(format!(
        "[INFO] Regenerated tmux/coven.toml and starter prompt files (runtime={})",
        prepared.multiplexer
    ))
}

fn optional_text(root: &Path, relative: &str) -> Result<String> {
    let raw = workspace::optional_bytes(root, Path::new(relative), MAX_TEXT, PROFILE)?
        .unwrap_or_default();
    Ok(std::str::from_utf8(&raw)?
        .replace("\r\n", "\n")
        .replace('\r', "\n"))
}

fn checkpoint_logs(root: &Path) -> Result<()> {
    let mut consumed = 0usize;
    // Messages precede events. Validate every checkpoint field, even on a
    // non-request record or a falsey/null value. History never repairs decisions.
    for relative in ["logs/messages.jsonl", "logs/events.jsonl"] {
        let raw = workspace::optional_bytes(
            root,
            Path::new(relative),
            8 * MAX_TEXT,
            "unsupported_event_log_size_profile",
        )?
        .unwrap_or_default();
        // Python reads/parses the entire log before following any references.
        task_delivery::validate_log(&raw)?;
        task_delivery::visit_log_records(&raw, &mut |record| {
            if let Some(checkpoint) = record.as_object().unwrap().get("checkpoint") {
                let id = checkpoint
                    .as_str()
                    .ok_or(Error("invalid_checkpoint_identifier"))?;
                require(
                    !id.is_empty()
                        && id
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-')),
                    "invalid_checkpoint_identifier",
                )?;
                let text = optional_text(root, &format!("checkpoints/{id}.md"))?;
                consumed += text.len();
                require(
                    consumed <= 256 * MAX_TEXT,
                    "unsupported_refresh_checkpoint_work_profile",
                )?;
            }
            Ok(())
        })?;
    }
    Ok(())
}

fn example_directory(source: &Path, example: Option<&str>) -> Result<Option<String>> {
    let Some(example) = example.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let relative = format!("examples/{}/example.json", templates::slug(example)?);
    let raw = workspace::optional_bytes(source, Path::new(&relative), values::MAX_INPUT, PROFILE)?
        .ok_or(Error("refresh_example_assets_unavailable"))?;
    require(
        values::parse(&raw, Profile::LegacySnapshot)?
            .as_object()
            .is_some(),
        "invalid_example_manifest",
    )?;
    let directory = Path::new(&relative).parent().unwrap();
    Ok(Some(directory.to_str().unwrap().to_owned()))
}

fn launch_cli() -> Result<String> {
    let command = match std::env::var("COVEN_COMMAND") {
        Ok(command) if !command.is_empty() => command,
        Ok(_) | Err(std::env::VarError::NotPresent) => {
            let wrapper = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/coven");
            require(wrapper.is_file(), "refresh_python_wrapper_unavailable")?;
            read_fs::path_text(&wrapper)?
        }
        Err(_) => return Err(Error("unsupported_refresh_command_profile")),
    };
    require(
        command.len() <= MAX_TEXT && !command.contains('\0'),
        "unsupported_refresh_command_profile",
    )?;
    Ok(templates::shell_quote(&command))
}

fn prepare(root: &Path) -> Result<Prepared> {
    prepare_with_assets(root, Path::new(env!("CARGO_MANIFEST_DIR")))
}

fn prepare_with_assets(root: &Path, source: &Path) -> Result<Prepared> {
    let state = workspace::load(root)?;
    let team = state.team.value().as_object().unwrap();
    let agents = state.team.agent_ids()?;
    let name = team.get("name").and_then(Value::as_str).unwrap();
    let command = team
        .get("opencode_command")
        .and_then(Value::as_str)
        .unwrap_or("oc");
    let example = example_directory(source, team.get("example").and_then(Value::as_str))?;
    checkpoint_logs(root)?;
    let root_text = read_fs::path_text(root)?;
    require(root_text.len() <= MAX_TEXT, PROFILE)?;
    let manifest = templates::manifest(name, &root_text, &agents, command, &launch_cli()?)?;
    manifest
        .parse::<toml::Value>()
        .map_err(|_| Error("invalid_refresh_manifest"))?;
    // Read context even when every role prompt already exists.
    let context = match example {
        Some(directory) => optional_text(source, &format!("{directory}/prompt-context.md"))?,
        None => String::new(),
    };
    let mut total = manifest.len();
    let mut missing = Vec::new();
    let mut starters = Vec::new();
    for agent in agents {
        let starter = templates::starter(agent, &root_text);
        admit(&mut total, starter.len())?;
        starters.push((format!("prompts/{agent}.start.txt"), starter));
        let relative = format!("prompts/{agent}.md");
        if !read_fs::managed(root, Path::new(&relative), false)?.exists() {
            let prompt = templates::prompt(agent, &root_text, &context);
            admit(&mut total, prompt.len())?;
            missing.push((relative, prompt));
        }
    }
    let dashboard = task_delivery::prepare_dashboard(root)?;
    admit(&mut total, dashboard.len())?;
    Ok(Prepared {
        manifest,
        missing,
        starters,
        dashboard,
        multiplexer: state.multiplexer,
    })
}

fn admit(total: &mut usize, len: usize) -> Result<()> {
    require(
        len <= MAX_OUTPUT.saturating_sub(*total),
        "unsupported_refresh_output_profile",
    )?;
    *total += len;
    Ok(())
}

#[cfg(unix)]
fn direct_write(root: &Path, relative: &str, text: &str, missing: bool) -> Result<()> {
    direct_with(root, relative, text, missing, &SystemDirectIo)
}

#[cfg(unix)]
trait DirectIo {
    fn write(&self, file: &mut fs::File, bytes: &[u8]) -> std::io::Result<()> {
        file.write_all(bytes)
    }
}
#[cfg(unix)]
struct SystemDirectIo;
#[cfg(unix)]
impl DirectIo for SystemDirectIo {}

#[cfg(unix)]
fn direct_with(
    root: &Path,
    relative: &str,
    text: &str,
    missing: bool,
    io: &impl DirectIo,
) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let path = read_fs::managed(root, Path::new(relative), false)?;
    let parent = path.parent().ok_or(Error("unsafe_archive_path"))?;
    fs::create_dir_all(parent)?;
    let path = read_fs::managed(root, Path::new(relative), false)?;
    if missing && path.exists() {
        return Ok(());
    }
    // Open without truncation: verify the actual descriptor before altering it.
    // New files use Python's 0666/umask policy; old inode/mode survive. No fsync,
    // replacement, or rollback: an operational failure may leave partial bytes.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .mode(0o666)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    require(file.metadata()?.is_file(), "unsafe_refresh_output_type")?;
    file.set_len(0)?;
    io.write(&mut file, text.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn direct_write(_: &Path, _: &str, _: &str, _: bool) -> Result<()> {
    Err(Error("unsupported_platform"))
}

#[cfg(test)]
mod tests;
