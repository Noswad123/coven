use super::*;
use sha2::{Digest, Sha256};

pub(super) struct Captured {
    pub inventory: Value,
    agents: Value,
}
impl Captured {
    pub fn plan(&self, control: &Value, id: &str) -> Result<Value> {
        let runtime = get(&self.inventory, "effective_runtime")?;
        let plan = crate::launch::build_plan(
            text(get(&self.inventory, "location")?, "root")?,
            &format!("coven-{}-{id}", text(control, "workspace_id")?),
            text(runtime, "multiplexer")?,
            &self.agents,
            get(get(&self.inventory, "environment")?, "transport")?,
        )?;
        crate::launch::validate_plan(&plan)
    }
}

pub(super) fn capture(root: &Path) -> Result<Captured> {
    let state = crate::workspace::load(root)?;
    let task_authority = if let Some(view) = crate::ledger::read(root)? {
        let report = view.report(root)?;
        require(
            text(get(&report, "projection")?, "status")? == "current",
            "action_task_projection_unhealthy",
        )?;
        object(
            [
                "authority",
                "revision",
                "tasks",
                "workspace_id",
                "epoch_id",
                "sequence",
            ]
            .into_iter()
            .map(|key| Ok((key, get(&report, key)?.clone())))
            .collect::<Result<Vec<_>>>()?,
        )?
    } else {
        require(!crate::ledger::pending(root)?, "action_task_import_pending")?;
        let raw = read(
            root,
            Path::new("state/tasks.json"),
            values::MAX_INPUT,
            "unsupported_action_input_profile",
        )?;
        object(vec![
            ("authority", string("snapshot-v1")),
            (
                "revision",
                string(crate::ledger::snapshot_revision(raw.as_deref())),
            ),
            ("tasks", state.tasks),
        ])?
    };
    let roster = state.team.agent_ids()?;
    let command = crate::workspace_assets::normalize_opencode_command(
        state
            .team
            .value()
            .as_object()
            .and_then(|v| v.get("opencode_command"))
            .and_then(Value::as_str)
            .unwrap_or("oc"),
    );
    let mut paths = [
        "coven.json",
        "coven.toml",
        "goal.md",
        "spec.md",
        "state/tasks.json",
        "state/agents.json",
        "state/lead.json",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    for id in &roster {
        paths.extend([
            format!("agents/{id}.md"),
            format!("prompts/{id}.md"),
            format!("prompts/{id}.start.txt"),
        ]);
    }
    let mut files = Vec::new();
    let mut captured = std::collections::HashMap::new();
    let mut total = 0usize;
    for path in paths {
        let raw = read(
            root,
            Path::new(&path),
            768 * 1024,
            "unsupported_action_writer_capacity",
        )?;
        total += raw.as_ref().map_or(0, Vec::len);
        require(total <= 2 * 1_048_576, "unsupported_action_writer_capacity")?;
        files.push(object(vec![
            ("path", string(&path)),
            (
                "bytes",
                raw.as_deref()
                    .map_or(Value::Null, |raw| string(base64(raw))),
            ),
            (
                "sha256",
                raw.as_deref().map_or(Value::Null, |raw| {
                    string(format!("{:x}", Sha256::digest(raw)))
                }),
            ),
        ])?);
        captured.insert(path, raw);
    }
    let mut agents = Vec::new();
    for id in &roster {
        let mut starter = String::new();
        for path in [
            format!("prompts/{id}.md"),
            format!("prompts/{id}.start.txt"),
        ] {
            let raw = captured
                .get(&path)
                .and_then(Option::as_deref)
                .ok_or(Error("action_prompt_missing_or_blank"))?;
            let text = std::str::from_utf8(raw)?;
            require(
                !crate::records::trimmed(text).is_empty(),
                "action_prompt_missing_or_blank",
            )?;
            starter = text.to_owned();
        }
        agents.push(object(vec![
            ("id", string(*id)),
            ("command", string(&command)),
            ("starter", string(starter)),
        ])?);
    }
    let selection = env("COVEN_MULTIPLEXER")?;
    let mut environment = Vec::new();
    for key in ["HOME", "LANG", "LC_ALL", "PATH", "SHELL", "TERM"] {
        if let Some(value) = env(key)? {
            environment.push((key, string(value)));
        }
    }
    Ok(Captured {
        agents: Value::Array(agents),
        inventory: object(vec![
            ("version", Value::integer("1")?),
            ("location", location(root)?),
            ("files", Value::Array(files)),
            ("task_authority", task_authority),
            (
                "roster",
                Value::Array(roster.into_iter().map(string).collect()),
            ),
            (
                "effective_runtime",
                object(vec![
                    ("multiplexer", string(state.multiplexer)),
                    ("command", string(command)),
                ])?,
            ),
            (
                "environment",
                object(vec![
                    (
                        "selection",
                        object(vec![(
                            "COVEN_MULTIPLEXER",
                            selection.map_or(Value::Null, string),
                        )])?,
                    ),
                    ("transport", object(environment)?),
                ])?,
            ),
        ])?,
    })
}
fn env(key: &str) -> Result<Option<String>> {
    match std::env::var(key) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(Error("unsupported_action_environment_profile")),
    }
}
fn base64(raw: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(raw.len().div_ceil(3) * 4);
    for chunk in raw.chunks(3) {
        let a = chunk[0] as usize;
        let b = chunk.get(1).copied().unwrap_or(0) as usize;
        let c = chunk.get(2).copied().unwrap_or(0) as usize;
        output.push(ALPHABET[a >> 2] as char);
        output.push(ALPHABET[((a & 3) << 4) | (b >> 4)] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[((b & 15) << 2) | (c >> 6)] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[c & 63] as char
        } else {
            '='
        });
    }
    output
}
