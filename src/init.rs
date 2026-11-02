//! Native bootstrap stages everything before effects and publishes coven.json last.
//! Partial outputs are evidence: never adopt, overwrite, clean up, or reset them.
use crate::{
    Error, Result,
    error::require,
    output, platform, read_fs,
    records::{self, TaskRecords, TeamRecord},
    storage::{Delivery, bootstrap},
    task_delivery,
    task_mutation::TaskOutput,
    task_time,
    values::{self, Profile, Value},
    workspace::{self, WorkspaceState},
    workspace_assets::{self as assets, PureAssetContext},
};
use std::{collections::HashSet, fs, io::Write, path::Path};

const MAX_TEMPLATE: usize = 1_048_576;
const MAX_JSON: usize = 8 * MAX_TEMPLATE;
const MAX_STAGE: usize = 32 * MAX_TEMPLATE;
const MAX_FILES: usize = 4096;
const MAX_AGENT_ID: usize = 255 - ".start.txt".len();
const RESERVED: &[&str] = &[
    "coven.json",
    "coven.toml",
    "goal.md",
    "spec.md",
    "dashboard.md",
    "agents",
    "prompts",
    "state",
    "logs",
    "checkpoints",
    "work",
    "output",
    "tmux",
];
const DIRECTORIES: &[&str] = &[
    "agents",
    "prompts",
    "logs",
    "checkpoints",
    "work",
    "output",
    "tmux",
];

pub struct Options {
    pub name: Option<String>,
    pub agents: String,
    pub opencode_command: String,
    pub multiplexer: String,
    pub example: String,
    pub blank: bool,
    pub force: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            name: None,
            agents: "Clippy,Srikanth,Jamal,Kris,Eric".into(),
            opencode_command: "oc".into(),
            multiplexer: "herdr".into(),
            example: "interactive-demo".into(),
            blank: false,
            force: false,
        }
    }
}

struct Generated {
    relative: String,
    bytes: Vec<u8>,
}
struct Prepared {
    files: Vec<Generated>,
    size: usize,
    stdout: String,
    diagnostic_root: String,
}
impl Prepared {
    fn new() -> Self {
        Self {
            files: Vec::new(),
            size: 0,
            stdout: String::new(),
            diagnostic_root: String::new(),
        }
    }
    fn admit(&self, size: usize, limit: usize) -> Result<()> {
        require(size <= limit, "unsupported_init_file_profile")?;
        require(
            self.files.len() < MAX_FILES && size <= MAX_STAGE.saturating_sub(self.size),
            "unsupported_init_stage_profile",
        )
    }
    fn push(&mut self, relative: String, bytes: Vec<u8>, limit: usize) -> Result<()> {
        self.admit(bytes.len(), limit)?;
        self.size += bytes.len();
        self.files.push(Generated { relative, bytes });
        Ok(())
    }
    fn text(&mut self, relative: &str, text: &str) -> Result<()> {
        self.admit(text.len(), MAX_TEMPLATE)?;
        self.push(relative.into(), text.as_bytes().to_vec(), MAX_TEMPLATE)
    }
    fn json(&mut self, relative: &str, value: &Value) -> Result<()> {
        let bytes = value.storage()?;
        let text = std::str::from_utf8(&bytes)?;
        let mut size = 0;
        for ch in text.chars() {
            size += if ch < '\u{7f}' { 1 } else { 6 * ch.len_utf16() };
            require(size <= MAX_JSON, "unsupported_init_json_profile")?;
        }
        self.admit(size, MAX_JSON)?;
        let bytes = output::ascii_json(text)?.into_bytes();
        // Check the encoded reader profile, not just the in-memory value.
        let decoded = values::parse(&bytes, Profile::LegacySnapshot)?;
        require(
            decoded.compact()? == value.compact()?,
            "invalid_init_json_roundtrip",
        )?;
        self.push(relative.into(), bytes, MAX_JSON)
    }
}

// Private injection only: production cannot select a clock or a publication fault.
trait InitIo {
    fn now(&self) -> Result<String> {
        task_time::now()
    }
    fn uuid(&self) -> Result<String> {
        task_time::uuid_hyphenated()
    }
    fn boundary(&self, _operation: &str, _root: &Path) -> Result<()> {
        Ok(())
    }
    fn write(&self, file: &mut fs::File, bytes: &[u8]) -> Result<()> {
        Ok(file.write_all(bytes)?)
    }
    fn sync_file(&self, file: &fs::File) -> Result<()> {
        Ok(file.sync_all()?)
    }
    fn sync_directory(&self, path: &Path) -> Result<()> {
        Ok(platform::sync_directory(path)?)
    }
    fn marker(&self, reservation: &bootstrap::Reservation, bytes: &[u8]) -> Result<Delivery> {
        reservation.publish_marker(bytes)
    }
}
struct SystemInitIo;
impl InitIo for SystemInitIo {}

fn s(text: &str) -> Value {
    Value::String(text.into())
}
fn object<const N: usize>(fields: [(&str, Value); N]) -> Result<Value> {
    Value::object(
        fields
            .into_iter()
            .map(|(key, value)| (key.into(), value))
            .collect(),
    )
}

fn check_reserved(root: &Path, owned_state: bool) -> Result<()> {
    match fs::symlink_metadata(root) {
        Ok(metadata) => require(metadata.is_dir(), "init_root_not_directory")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    for name in RESERVED {
        if owned_state && *name == "state" {
            continue;
        }
        match fs::symlink_metadata(root.join(name)) {
            Ok(_) => return Err(Error("init_reserved_name_exists")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn event(log: &mut Vec<u8>, kind: &str, payload: Value, io: &impl InitIo) -> Result<()> {
    let mut value = object([
        ("event_id", s(&io.uuid()?)),
        ("ts", s(&io.now()?)),
        ("type", s(kind)),
    ])?;
    for (key, value_part) in payload
        .as_object()
        .ok_or(Error("invalid_init_event"))?
        .entries()
    {
        value.set_field(key, value_part.clone())?;
    }
    let mut bytes = value.compact()?;
    bytes.push(b'\n');
    require(
        bytes.len() <= MAX_JSON.saturating_sub(log.len()),
        "unsupported_init_log_profile",
    )?;
    log.extend(bytes);
    Ok(())
}

fn human(text: &str) -> Result<String> {
    let mut size = 0;
    for ch in text.chars() {
        size += match ch {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
            ch if ch < ' ' => 6,
            ch if ch >= '\u{7f}' => 6 * ch.len_utf16(),
            _ => 1,
        };
        require(
            size <= values::MAX_OUTPUT,
            "unsupported_json_output_profile",
        )?;
    }
    let encoded = serde_json::to_string(text)?;
    output::ascii_json(&encoded[1..encoded.len() - 1])
}

fn prepare(root: &Path, options: &Options, io: &impl InitIo) -> Result<Prepared> {
    let root_text = root.to_str().ok_or(Error("non_unicode_path_unsupported"))?;
    require(
        root_text.len() <= MAX_TEMPLATE,
        "unsupported_init_input_profile",
    )?;
    require(
        options.agents.len() <= MAX_JSON,
        "unsupported_init_input_profile",
    )?;
    let mut ids = Vec::new();
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for label in options.agents.split(',') {
        require(
            label.len() <= values::MAX_STRING,
            "unsupported_init_input_profile",
        )?;
        require(ids.len() < MAX_FILES / 3, "unsupported_init_stage_profile")?;
        let id = assets::slug(label)?;
        require(
            id.len() <= MAX_AGENT_ID,
            "unsupported_init_filename_profile",
        )?;
        require(seen.insert(id.clone()), "duplicate_init_agent")?;
        ids.push(id.clone());
        names.push((id, s(records::trimmed(label))));
    }
    require(
        options.multiplexer.len() <= MAX_TEMPLATE,
        "unsupported_init_input_profile",
    )?;
    let runtime = records::trimmed(&options.multiplexer).to_lowercase();
    records::multiplexer(&runtime)?;
    let name = options
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            root.file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("coven")
        });
    let example: Option<assets::InitExample> = if options.blank {
        None
    } else {
        require(
            options.example.len() <= MAX_TEMPLATE,
            "unsupported_init_input_profile",
        )?;
        Some(assets::init_example(&options.example)?)
    };
    let display_names = Value::object(names)?;
    let roster = Value::Array(ids.iter().map(|id| s(id)).collect());
    let version = values::parse(b"1", Profile::LegacySnapshot)?;
    let created_at = io.now()?;
    let effective_runtime = workspace::select_multiplexer(Some(&runtime))?;
    require(
        !records::trimmed(&options.opencode_command).is_empty(),
        "invalid_init_command",
    )?;
    require(
        name.len() <= values::MAX_STRING && options.opencode_command.len() <= values::MAX_STRING,
        "unsupported_init_input_profile",
    )?;
    let team = TeamRecord::validate(object([
        ("version", version),
        ("name", s(name)),
        ("created_at", s(&created_at)),
        ("source_of_truth", s("workspace-files")),
        ("opencode_command", s(&options.opencode_command)),
        ("multiplexer", s(&runtime)),
        ("agents", roster.clone()),
        ("display_names", display_names.clone()),
        (
            "example",
            example
                .as_ref()
                .map_or(Value::Null, |example| s(example.id)),
        ),
    ])?)?;
    let cli_command = match std::env::var("COVEN_COMMAND") {
        Ok(value) if !value.is_empty() => {
            require(
                value.len() <= MAX_TEMPLATE,
                "unsupported_init_input_profile",
            )?;
            assets::shell_quote(&value)
        }
        Ok(_) | Err(std::env::VarError::NotPresent) => "coven".into(),
        Err(_) => return Err(Error("unsupported_init_command_environment_profile")),
    };
    let context = PureAssetContext {
        cli_command,
        example_text: example
            .as_ref()
            .map_or("", |example| example.context)
            .into(),
    };
    let mut tasks = Vec::new();
    if let Some(example) = &example {
        for raw in example
            .tasks
            .as_array()
            .ok_or(Error("invalid_init_tasks"))?
        {
            let raw = raw.as_object().ok_or(Error("invalid_init_task"))?;
            let field = |key| raw.get(key).and_then(Value::as_str);
            let id = field("id").ok_or(Error("invalid_init_task"))?;
            let owner = field("owner")
                .filter(|owner| !owner.is_empty())
                .map(assets::slug)
                .transpose()?;
            let owner = owner.filter(|owner| seen.contains(owner));
            tasks.push(object([
                ("id", s(id)),
                (
                    "title",
                    s(field("title")
                        .filter(|title| !title.is_empty())
                        .unwrap_or(id)),
                ),
                (
                    "status",
                    s(if owner.is_some() {
                        "assigned"
                    } else {
                        "pending"
                    }),
                ),
                ("owner", owner.as_deref().map_or(Value::Null, s)),
                (
                    "notes",
                    s(field("notes")
                        .filter(|notes| !notes.is_empty())
                        .unwrap_or("Starter example task.")),
                ),
                ("created_at", s(&io.now()?)),
                ("updated_at", s(&io.now()?)),
            ])?);
        }
    }
    let tasks = TaskRecords::validate(Value::Array(tasks))?.into_value();
    let agents = Value::Array(
        ids.iter()
            .map(|id| {
                object([
                    ("id", s(id)),
                    ("role", s(&assets::agent_role(id))),
                    ("status", s("idle")),
                ])
            })
            .collect::<Result<Vec<_>>>()?,
    );
    records::agents(&agents)?;
    let mut result = Prepared::new();
    result.json("coven.json", team.value())?;
    result.json("state/agents.json", &agents)?;
    result.json("state/tasks.json", &tasks)?;
    let goal = example
        .as_ref()
        .map_or_else(assets::default_goal, |example| example.goal);
    result.text("goal.md", goal)?;
    let toml = assets::team_toml(
        name,
        &ids,
        &options.opencode_command,
        &runtime,
        &display_names,
    )?;
    require(toml.len() <= MAX_TEMPLATE, "unsupported_init_file_profile")?;
    toml.parse::<toml::Value>()
        .map_err(|_| Error("invalid_init_toml"))?;
    result.text("coven.toml", &toml)?;
    let agent_refs = ids.iter().map(String::as_str).collect::<Vec<_>>();
    let manifest = assets::tmux_manifest(
        name,
        root_text,
        &agent_refs,
        &options.opencode_command,
        &context,
    )?;
    require(
        manifest.len() <= MAX_TEMPLATE,
        "unsupported_init_file_profile",
    )?;
    manifest
        .parse::<toml::Value>()
        .map_err(|_| Error("invalid_init_toml"))?;
    result.text("tmux/coven.toml", &manifest)?;
    result.text(
        "work/README.md",
        example
            .as_ref()
            .map_or_else(assets::default_work, |example| example.work),
    )?;
    result.text(
        "output/README.md",
        example
            .as_ref()
            .map_or_else(assets::default_output, |example| example.output),
    )?;
    if let Some(example) = &example {
        result.text("spec.md", example.spec)?;
    }
    for (relative, text) in assets::checkpoint_files() {
        result.text(&relative, &text)?;
    }
    for id in &ids {
        result.text(&format!("agents/{id}.md"), &assets::agent_state_text(id))?;
        result.text(
            &format!("prompts/{id}.md"),
            &assets::roleprompt_text(id, root_text, &context),
        )?;
        result.text(
            &format!("prompts/{id}.start.txt"),
            &assets::starter_text(id, root_text),
        )?;
    }
    let mut log = Vec::new();
    let example_id = example.as_ref().map_or("blank", |example| example.id);
    event(
        &mut log,
        "workspace.created",
        object([
            ("name", s(name)),
            ("agents", roster),
            ("example", s(example_id)),
        ])?,
        io,
    )?;
    if example.is_some() {
        event(
            &mut log,
            "goal.seeded",
            object([("example", s(example_id))])?,
            io,
        )?;
        for task in tasks.as_array().ok_or(Error("invalid_init_tasks"))? {
            event(
                &mut log,
                "task.seeded",
                object([("task", task.clone())])?,
                io,
            )?;
        }
    }
    // Complete JSONL reader validation, with compact literal-Unicode records.
    require(
        log.len() <= MAX_JSON && log.last() == Some(&b'\n'),
        "unsupported_init_log_profile",
    )?;
    for (index, line) in log.split_inclusive(|byte| *byte == b'\n').enumerate() {
        require(index < 65_536, "unsupported_init_log_profile")?;
        let value = values::parse(&line[..line.len() - 1], Profile::LegacySnapshot)?;
        require(value.as_object().is_some(), "invalid_init_event")?;
    }
    result.push("logs/events.jsonl".into(), log, MAX_JSON)?;
    result.text("logs/messages.jsonl", "")?;
    let dashboard = task_delivery::prepare_initial_dashboard(
        WorkspaceState {
            team,
            tasks,
            agents,
            lead: Value::Null,
            multiplexer: effective_runtime,
        },
        goal,
    )?;
    result.push("dashboard.md".into(), dashboard, MAX_TEMPLATE)?;
    let root_display = human(root_text)?;
    result.diagnostic_root = root_display.chars().take(2048).collect();
    if result.diagnostic_root.len() < root_display.len() {
        result.diagnostic_root.push_str("...");
    }
    let detail = match example {
        Some(example) => human(example.seed_message)?,
        None => format!("Next: coven goal {root_display} \"<your goal>\""),
    };
    result.stdout = format!(
        "[INFO] Coven workspace: {root_display}\n[INFO] {detail}\n[INFO] Launch ({runtime}): coven up {root_display}"
    );
    require(
        result.stdout.len() <= values::MAX_OUTPUT,
        "unsupported_init_output_profile",
    )?;
    Ok(result)
}

struct Progress<'a> {
    root: &'a str,
    phase: &'static str,
    current: Option<&'a str>,
    last_completed: Option<&'a str>,
    partial_file: bool,
}

fn publish<'a>(
    root: &Path,
    prepared: &'a Prepared,
    io: &impl InitIo,
    progress: &mut Progress<'a>,
) -> Result<(Delivery, bootstrap::Reservation)> {
    progress.phase = "inspect-root";
    let mut missing = Vec::new();
    for ancestor in root.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                require(metadata.is_dir(), "init_root_not_directory")?;
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing.push(ancestor),
            Err(error) => return Err(error.into()),
        }
    }
    let containing_parent = missing.last().and_then(|path| path.parent());
    for directory in missing.iter().rev() {
        progress.phase = "create-root";
        io.boundary("mkdir-root", root)?;
        // Concurrent init may create a shared missing ancestor, but never adopt
        // managed directories: state/ and every managed output stay exclusive.
        match fs::create_dir(directory) {
            Ok(()) => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::AlreadyExists && directory.is_dir() => {}
            Err(error) => return Err(error.into()),
        }
    }
    progress.phase = "reserve-state";
    io.boundary("reserve", root)?;
    let reservation = bootstrap::reserve(root)?;
    progress.phase = "recheck-reserved";
    io.boundary("recheck-reserved", root)?;
    reservation.validate()?;
    check_reserved(root, true)?;
    for directory in DIRECTORIES {
        progress.phase = "create-directory";
        progress.current = Some(directory);
        io.boundary("mkdir-managed", root)?;
        reservation.validate()?;
        fs::create_dir(root.join(directory))?;
    }
    for generated in &prepared.files {
        if generated.relative == "coven.json" {
            continue;
        }
        progress.phase = "open-file";
        progress.current = Some(&generated.relative);
        progress.partial_file = false;
        io.boundary("open", root)?;
        reservation.validate()?;
        let path = read_fs::managed(root, Path::new(&generated.relative), false)?;
        let mut file = platform::open_generated(&path, true)?;
        progress.partial_file = true;
        if generated.relative == "dashboard.md" {
            progress.phase = "dashboard-mode";
            io.boundary("dashboard-mode", root)?;
            platform::apply_replacement_mode(&file, 0o600)?;
            progress.phase = "verify-dashboard-mode";
            io.boundary("verify-dashboard-mode", root)?;
            platform::verify_replacement_mode(&file, 0o600)?;
        }
        progress.phase = "write-file";
        io.boundary("write", root)?;
        reservation.validate()?;
        io.write(&mut file, &generated.bytes)?;
        progress.phase = "sync-file";
        io.boundary("sync-file", root)?;
        io.sync_file(&file)?;
        progress.last_completed = Some(&generated.relative);
        progress.partial_file = false;
    }
    for directory in DIRECTORIES.iter().copied().chain(std::iter::once("state")) {
        progress.phase = "sync-managed-directory";
        progress.current = Some(directory);
        io.boundary("sync-managed-directory", root)?;
        reservation.validate()?;
        io.sync_directory(&root.join(directory))?;
    }
    // Sync the chain observed missing by this invocation, through its existing
    // containing parent. Existing ancestor entries are assumed durable, even if
    // another initializer just created them; Committed is operation-scoped.
    for ancestor in std::iter::once(root)
        .chain(missing.iter().copied().filter(|path| *path != root))
        .chain(containing_parent)
    {
        progress.phase = "sync-creation-chain";
        progress.current = None;
        io.boundary("sync-ancestor", root)?;
        reservation.validate()?;
        io.sync_directory(ancestor)?;
    }
    let marker = prepared
        .files
        .iter()
        .find(|file| file.relative == "coven.json")
        .ok_or(Error("missing_init_marker"))?;
    progress.phase = "publish-marker";
    progress.current = Some("coven.json");
    io.boundary("publish-marker", root)?;
    reservation.validate()?;
    // No fallible work follows a known linked marker. Bootstrap reports final
    // directory-sync uncertainty as Delivery, not an ordinary failure.
    let delivery = io.marker(&reservation, &marker.bytes)?;
    Ok((delivery, reservation))
}

pub fn arguments(workspace: &Path, options: &Options) -> Result<TaskOutput> {
    arguments_with(workspace, options, &SystemInitIo)
}
fn arguments_with(workspace: &Path, options: &Options, io: &impl InitIo) -> Result<TaskOutput> {
    platform::ensure_supported()?;
    let root = platform::resolve_init_target(workspace)?;
    check_reserved(&root, false)?;
    let prepared = prepare(&root, options, io)?;
    let mut progress = Progress {
        root: &prepared.diagnostic_root,
        phase: "prepared",
        current: None,
        last_completed: None,
        partial_file: false,
    };
    // --force is deliberately not consulted: it never authorizes a reset.
    match publish(&root, &prepared, io, &mut progress) {
        Err(error) => Ok(TaskOutput {
            stdout: String::new(),
            warnings: vec![format!(
                "[ERROR] Initialization stopped; partial files have been preserved for inspection: {} (root={} phase={} target={} last_completed={} partial_file={})",
                error.code(),
                progress.root,
                progress.phase,
                progress.current.unwrap_or("none"),
                progress.last_completed.unwrap_or("none"),
                progress.partial_file
            )],
            exit_code: 1,
        }),
        Ok((delivery, _reservation)) => Ok(TaskOutput {
            stdout: prepared.stdout,
            warnings: if matches!(delivery, Delivery::DurabilityUncertain) {
                vec!["[WARN] init_marker_durability_uncertain: workspace initialized; inspect before retrying".into()]
            } else {
                Vec::new()
            },
            exit_code: if matches!(delivery, Delivery::DurabilityUncertain) {
                2
            } else {
                0
            },
        }),
    }
}

#[cfg(test)]
mod tests;
