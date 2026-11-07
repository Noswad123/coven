//! Bounded snapshot-v1 initialization. Source-tree assets/default wrapper match
//! refresh's preview policy. No runtime, authority activation, or rollback.
use crate::{
    Error, Result,
    error::require,
    platform, read_fs, records,
    refresh::{self, templates},
    storage::WorkspaceGuard,
    task_delivery, task_time,
    values::{self, Profile, Value},
    workspace,
};
use std::{
    collections::{HashSet, VecDeque},
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    time::Duration,
};

mod templates_init;

const MAX_TEXT: usize = 1_048_576;
const PROFILE: &str = "unsupported_init_input_profile";
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

#[derive(Clone, Debug)]
pub struct Options {
    pub name: Option<String>,
    pub agents: String,
    pub opencode_command: String,
    pub multiplexer: String,
    pub example: String,
    pub blank: bool,
    /// Accepted compatibility flag, never authorizes replacement or adoption.
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

pub fn init_argument(path: &Path, options: &Options) -> Result<String> {
    init(&platform::workspace_argument(path)?, options)
}

pub fn init(path: &Path, options: &Options) -> Result<String> {
    platform::ensure_supported()?;
    let root = resolve_root(path)?;
    run(
        &root,
        options,
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &SystemIo,
    )
}

// Unlike existing-workspace readers, init must resolve missing suffixes without
// creating them. Process symlinks before '..', including dangling root aliases.
fn resolve_root(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    fn parts(path: &Path) -> VecDeque<std::ffi::OsString> {
        path.components()
            .map(|part| part.as_os_str().to_owned())
            .collect()
    }
    let mut pending = parts(&path);
    let mut root = PathBuf::new();
    let mut links = 0;
    while let Some(part) = pending.pop_front() {
        match Path::new(&part).components().next().unwrap() {
            Component::RootDir => root.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                root.pop();
            }
            Component::Normal(_) => {
                let next = root.join(&part);
                match fs::symlink_metadata(&next) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        links += 1;
                        require(links <= 40, "unsupported_init_symlink_resolution_profile")?;
                        let target = fs::read_link(next)?;
                        let mut target = parts(&target);
                        target.append(&mut pending);
                        pending = target;
                    }
                    Ok(_) => root.push(part),
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                        ) =>
                    {
                        root.push(part)
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            _ => return Err(Error("unsupported_platform")),
        }
    }
    Ok(root)
}

fn preflight(root: &Path) -> Result<()> {
    match fs::symlink_metadata(root) {
        Ok(meta) => require(meta.is_dir(), "init_workspace_not_directory")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut conflict = false;
    for name in RESERVED {
        match fs::symlink_metadata(root.join(name)) {
            Ok(_) => conflict = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    require(
        !conflict,
        "init_reserved_entry_exists_force_does_not_authorize_reset",
    )
}

fn s(text: impl Into<String>) -> Value {
    Value::String(text.into())
}
fn obj(entries: Vec<(&str, Value)>) -> Result<Value> {
    Value::object(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

// Only init JSON uses Python's default ensure_ascii=True. Expanding the ordered
// storage encoding is safe: raw non-ASCII characters occur only inside strings.
fn initial_json(value: &Value) -> Result<String> {
    let bytes = value.storage()?;
    let mut out = String::new();
    for c in std::str::from_utf8(&bytes)?.chars() {
        if c.is_ascii() && c != '\u{7f}' {
            out.push(c);
        } else {
            for unit in c.encode_utf16(&mut [0; 2]) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
        require(out.len() <= values::MAX_INPUT, PROFILE)?;
    }
    values::parse(out.as_bytes(), Profile::LegacySnapshot)?;
    Ok(out)
}

fn text(source: &Path, directory: Option<&str>, relative: &str, default: &str) -> Result<String> {
    let Some(directory) = directory else {
        return Ok(default.into());
    };
    let raw = workspace::optional_bytes(
        source,
        &Path::new(directory).join(relative),
        MAX_TEXT,
        PROFILE,
    )?;
    match raw {
        Some(raw) => Ok(std::str::from_utf8(&raw)?
            .replace("\r\n", "\n")
            .replace('\r', "\n")),
        None => Ok(default.into()),
    }
}

struct Prepared {
    files: Vec<(String, String)>,
    events: Vec<(&'static str, Value)>,
    success: String,
}

fn admit_file(total: &mut usize, relative: &str, content: &str) -> Result<()> {
    let limit = if relative.ends_with(".json") {
        values::MAX_INPUT
    } else {
        MAX_TEXT
    };
    require(content.len() <= limit, PROFILE)?;
    require(
        content.len() <= values::MAX_OUTPUT.saturating_sub(*total),
        "unsupported_init_output_profile",
    )?;
    *total += content.len();
    Ok(())
}

fn prepare(root: &Path, options: &Options, source: &Path) -> Result<Prepared> {
    let root_text = read_fs::path_text(root)?;
    require(root_text.len() <= MAX_TEXT, PROFILE)?;
    let mut seen = HashSet::new();
    let mut agents = Vec::new();
    let mut names = Vec::new();
    require(options.agents.len() <= values::MAX_INPUT, PROFILE)?;
    for label in options.agents.split(',') {
        let id = templates::slug(label)?;
        require(seen.insert(id.clone()), "duplicate_init_agent")?;
        names.push((id.clone(), s(records::trimmed(label))));
        agents.push(id);
    }
    require(agents.len() <= values::MAX_MEMBERS, PROFILE)?;
    records::multiplexer(&options.multiplexer)?;
    let mux = records::trimmed(&options.multiplexer).to_lowercase();
    let name = options
        .name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            root.file_name()
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("coven")
        });
    let directory = if options.blank {
        None
    } else {
        Some(format!("examples/{}", templates::slug(&options.example)?))
    };
    let mut example_id = None;
    let mut seed_message = None;
    if let Some(directory) = &directory {
        let raw = workspace::optional_bytes(
            source,
            &Path::new(directory).join("example.json"),
            values::MAX_INPUT,
            PROFILE,
        )?
        .ok_or(Error("init_example_assets_unavailable"))?;
        let value = values::parse(&raw, Profile::LegacySnapshot)?;
        let manifest = value.as_object().ok_or(Error("invalid_example_manifest"))?;
        for (key, slot) in [("id", &mut example_id), ("seed_message", &mut seed_message)] {
            if let Some(value) = manifest.get(key) {
                match value {
                    Value::Null | Value::Bool(false) => {}
                    Value::String(value) if value.is_empty() => {}
                    Value::String(value) => *slot = Some(value.clone()),
                    _ => return Err(Error("unsupported_init_example_string_profile")),
                }
            }
        }
        example_id.get_or_insert_with(|| directory.rsplit('/').next().unwrap().into());
    }
    let team = obj(vec![
        ("version", Value::integer("1")?),
        ("name", s(name)),
        ("created_at", s(task_time::now()?)),
        ("source_of_truth", s("workspace-files")),
        ("opencode_command", s(&options.opencode_command)),
        ("multiplexer", s(&mux)),
        ("agents", Value::Array(agents.iter().map(s).collect())),
        ("display_names", Value::object(names)?),
        ("example", example_id.as_ref().map(s).unwrap_or(Value::Null)),
    ])?;
    let effective_mux = match std::env::var("COVEN_MULTIPLEXER") {
        Ok(value) if !value.is_empty() => {
            records::multiplexer(&value)?;
            records::trimmed(&value).to_lowercase()
        }
        Ok(_) | Err(std::env::VarError::NotPresent) => mux.clone(),
        Err(_) => return Err(Error("unsupported_multiplexer")),
    };
    require(
        !records::trimmed(&options.opencode_command).is_empty(),
        "empty_init_opencode_command",
    )?;
    records::TeamRecord::validate(team.clone())?;
    let tasks = seed_tasks(source, directory.as_deref(), &agents)?;
    records::TaskRecords::validate(tasks.clone())?;
    let states = Value::Array(
        agents
            .iter()
            .map(|agent| {
                obj(vec![
                    ("id", s(agent)),
                    ("role", s(templates::role(agent))),
                    ("status", s("idle")),
                ])
            })
            .collect::<Result<_>>()?,
    );
    records::agents(&states)?;
    let mut goal = text(
        source,
        directory.as_deref(),
        "goal.md",
        "# Goal\n\nDescribe the unified coven goal here.\n",
    )?;
    if !goal.ends_with('\n') {
        goal.push('\n');
    }
    let refs = agents.iter().map(String::as_str).collect::<Vec<_>>();
    let mut files: Vec<(String, String)> = vec![
        ("coven.json".into(), initial_json(&team)?),
        ("state/agents.json".into(), initial_json(&states)?),
        ("state/tasks.json".into(), initial_json(&tasks)?),
        ("goal.md".into(), goal.clone()),
        (
            "coven.toml".into(),
            templates_init::team(
                name,
                &agents,
                &options.agents,
                &options.opencode_command,
                &mux,
            ),
        ),
        ("logs/events.jsonl".into(), String::new()),
        ("logs/messages.jsonl".into(), String::new()),
        (
            "tmux/coven.toml".into(),
            templates::manifest(
                name,
                &root_text,
                &refs,
                &options.opencode_command,
                &refresh::launch_cli()?,
            )?,
        ),
    ];
    let mut total = 0;
    for (relative, content) in &files {
        admit_file(&mut total, relative, content)?;
    }
    for (relative, content) in templates_init::structure(source, directory.as_deref())? {
        admit_file(&mut total, &relative, &content)?;
        files.push((relative, content));
    }
    if !options.blank {
        let spec = text(
            source,
            directory.as_deref(),
            "spec.md",
            "# Spec\n\nDescribe the desired behavior here.\n",
        )?;
        admit_file(&mut total, "spec.md", &spec)?;
        files.push(("spec.md".into(), spec));
    }
    let context = text(source, directory.as_deref(), "prompt-context.md", "")?;
    for agent in &agents {
        for (relative, content) in [
            (format!("agents/{agent}.md"), templates_init::agent(agent)),
            (
                format!("prompts/{agent}.md"),
                templates::prompt(agent, &root_text, &context),
            ),
            (
                format!("prompts/{agent}.start.txt"),
                templates::starter(agent, &root_text),
            ),
        ] {
            admit_file(&mut total, &relative, &content)?;
            files.push((relative, content));
        }
    }
    for index in [4, 7] {
        files[index]
            .1
            .parse::<toml::Value>()
            .map_err(|_| Error("invalid_init_toml"))?;
    }
    let mut events = vec![(
        "workspace.created",
        obj(vec![
            ("name", s(name)),
            ("agents", Value::Array(agents.iter().map(s).collect())),
            ("example", s(example_id.as_deref().unwrap_or("blank"))),
        ])?,
    )];
    if let Some(id) = &example_id {
        events.push(("goal.seeded", obj(vec![("example", s(id))])?));
        for task in tasks.as_array().unwrap() {
            events.push(("task.seeded", obj(vec![("task", task.clone())])?));
        }
    }
    // Admit complete wrapped records as well as aggregate log bytes/line count.
    let mut log = Vec::new();
    for (kind, payload) in &events {
        let mut record = obj(vec![
            ("event_id", s("00000000-0000-4000-8000-000000000000")),
            ("ts", s("2000-01-01T00:00:00.000000+00:00")),
            ("type", s(*kind)),
        ])?;
        for (key, value) in payload.as_object().unwrap().entries() {
            record.set_field(key, value.clone())?;
        }
        log.extend(record.compact()?);
        log.push(b'\n');
        require(log.len() <= values::MAX_INPUT, PROFILE)?;
    }
    task_delivery::validate_log(&log)?;
    let dashboard = task_delivery::prepare_initial_dashboard(
        workspace::WorkspaceState {
            team: records::TeamRecord::validate(team)?,
            tasks,
            agents: states,
            lead: Value::Null,
            multiplexer: effective_mux,
        },
        &goal,
    )?;
    require(
        dashboard.len() <= values::MAX_OUTPUT.saturating_sub(total),
        "unsupported_init_output_profile",
    )?;
    let next = if options.blank {
        format!("Next: coven goal {root_text} \"<your goal>\"")
    } else {
        seed_message.unwrap_or_else(|| format!("Seeded example: {}", example_id.unwrap()))
    };
    Ok(Prepared {
        files,
        events,
        success: format!(
            "[INFO] Coven workspace: {root_text}\n[INFO] {next}\n[INFO] Launch ({mux}): coven up {root_text}"
        ),
    })
}

fn seed_tasks(source: &Path, directory: Option<&str>, agents: &[String]) -> Result<Value> {
    let raw = match directory {
        Some(dir) => workspace::optional_bytes(
            source,
            &Path::new(dir).join("tasks.json"),
            values::MAX_INPUT,
            PROFILE,
        )?,
        None => None,
    };
    let raw = raw
        .map(|raw| values::parse(&raw, Profile::LegacySnapshot))
        .transpose()?
        .unwrap_or(Value::Array(vec![]));
    let mut tasks = Vec::new();
    let mut ids = HashSet::new();
    for task in raw.as_array().ok_or(Error("invalid_example_tasks"))? {
        let task = task.as_object().ok_or(Error("invalid_example_task"))?;
        for key in ["id", "title", "owner", "notes"] {
            require(
                task.get(key)
                    .is_none_or(|v| matches!(v, Value::Null | Value::String(_))),
                "invalid_example_task_text",
            )?;
        }
        let get = |key| {
            task.get(key)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
        };
        let id = match get("id") {
            Some(id) => id.into(),
            None => templates::slug(get("title").unwrap_or("task"))?,
        };
        require(
            id.as_bytes()
                .first()
                .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                && id
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_-".contains(&c)),
            "invalid_example_task_id",
        )?;
        require(ids.insert(id.clone()), "duplicate_example_task_id")?;
        let owner = get("owner")
            .map(templates::slug)
            .transpose()?
            .filter(|id| agents.contains(id));
        tasks.push(obj(vec![
            ("id", s(&id)),
            ("title", s(get("title").unwrap_or(&id))),
            (
                "status",
                s(if owner.is_some() {
                    "assigned"
                } else {
                    "pending"
                }),
            ),
            ("owner", owner.map(s).unwrap_or(Value::Null)),
            ("notes", s(get("notes").unwrap_or("Starter example task."))),
            ("created_at", s(task_time::now()?)),
            ("updated_at", s(task_time::now()?)),
        ])?);
    }
    Ok(Value::Array(tasks))
}

trait InitIo {
    fn mkdir_root(&self, root: &Path) -> Result<()> {
        fs::create_dir_all(root)?;
        Ok(())
    }
    fn before_file(&self, _root: &Path, _relative: &str) -> Result<()> {
        Ok(())
    }
    fn write(&self, file: &mut fs::File, bytes: &[u8]) -> std::io::Result<()> {
        file.write_all(bytes)
    }
    fn event(
        &self,
        root: &Path,
        kind: &str,
        payload: &Value,
        guard: &WorkspaceGuard,
    ) -> Result<()> {
        task_delivery::append_event(root, kind, payload, guard)
    }
    fn dashboard(&self, root: &Path, guard: &WorkspaceGuard) -> Result<()> {
        task_delivery::refresh_dashboard(root, guard)
    }
}
struct SystemIo;
impl InitIo for SystemIo {}

fn run(root: &Path, options: &Options, source: &Path, io: &impl InitIo) -> Result<String> {
    preflight(root)?;
    let prepared = prepare(root, options, source)?;
    let stopped = |_| Error("initialization_stopped_partial_files_preserved_for_inspection");
    io.mkdir_root(root).map_err(stopped)?;
    for (relative, content) in &prepared.files {
        io.before_file(root, relative).map_err(stopped)?;
        create_seed(root, relative, content, io).map_err(stopped)?;
    }
    for (kind, payload) in &prepared.events {
        workspace::load(root)?;
        let guard = WorkspaceGuard::acquire(root, Duration::from_secs(5))?;
        io.event(root, kind, payload, &guard)?;
    }
    workspace::load(root)?;
    let guard = WorkspaceGuard::acquire(root, Duration::from_secs(5))?;
    io.dashboard(root, &guard)?;
    Ok(prepared.success)
}

#[cfg(unix)]
fn create_seed(root: &Path, relative: &str, content: &str, io: &impl InitIo) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let path = read_fs::managed(root, Path::new(relative), false)?;
    fs::create_dir_all(path.parent().ok_or(Error("unsafe_init_path"))?)?;
    let path = read_fs::managed(root, Path::new(relative), false)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o666)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    require(file.metadata()?.is_file(), "unsafe_init_output_type")?;
    io.write(&mut file, content.as_bytes())?;
    Ok(())
}
#[cfg(not(unix))]
fn create_seed(_: &Path, _: &str, _: &str, _: &impl InitIo) -> Result<()> {
    Err(Error("unsupported_platform"))
}

#[cfg(test)]
mod tests;
