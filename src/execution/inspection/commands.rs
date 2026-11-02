use super::*;

pub fn inspect_arguments(workspace: &Path, json: bool) -> Result<TaskOutput> {
    let root = root_argument(workspace)?;
    let mut work = Work::default();
    output(&inspect(&root, &SystemIo, &mut work)?, json, &mut work)
}

fn without(value: &Value, names: &[&str]) -> Result<Value> {
    Value::object(
        value
            .as_object()
            .ok_or(Error("invalid_execution_fields"))?
            .entries()
            .iter()
            .filter(|(key, _)| !names.contains(&key.as_str()))
            .cloned()
            .collect(),
    )
}

pub fn attempt_list_arguments(
    workspace: &Path,
    task: Option<&str>,
    json: bool,
) -> Result<TaskOutput> {
    let root = root_argument(workspace)?;
    let mut work = Work::default();
    let mut value = inspect(&root, &SystemIo, &mut work)?;
    if let Some(task) = task {
        // A filter is not retained source data: validate the borrowed text
        // without allocating an otherwise unused Value::String for API callers.
        require(
            !task.is_empty()
                && task
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')),
            "invalid_execution_identifier",
        )?;
        if let Some(attempts) = value.as_object().and_then(|v| v.get("attempts")) {
            let selected = attempts
                .as_array()
                .ok_or(Error("invalid_execution_attempts"))?
                .iter()
                .filter_map(|item| {
                    match get(item, "prepared")
                        .and_then(|v| get(v, "task_id"))
                        .and_then(text)
                    {
                        Ok(id) if id == task => Some(Ok(item.clone())),
                        Ok(_) => None,
                        Err(error) => Some(Err(error)),
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            value.set_field("attempts", Value::Array(selected))?;
        }
    }
    output(&without(&value, &["enrollments"])?, json, &mut work)
}

// Arity, not workspace markers, disambiguates a required selector.
fn selector(items: &[PathBuf]) -> Result<(&Path, &Path)> {
    match items {
        [selected] => Ok((Path::new("."), selected)),
        [workspace, selected] => Ok((workspace, selected)),
        _ => Err(Error("invalid_execution_selector_arguments")),
    }
}

pub fn attempt_inspect_arguments(items: &[PathBuf], json: bool) -> Result<TaskOutput> {
    let (workspace, selected) = selector(items)?;
    let id = selected
        .to_str()
        .ok_or(Error("non_unicode_path_unsupported"))?;
    require(hex(id, 32), "invalid_execution_attempt_id")?;
    let root = root_argument(workspace)?;
    let mut work = Work::default();
    let mut value = inspect(&root, &SystemIo, &mut work)?;
    if let Some(attempts) = value.as_object().and_then(|v| v.get("attempts")) {
        let selected = attempts
            .as_array()
            .ok_or(Error("invalid_execution_attempts"))?
            .iter()
            .find(|item| {
                get(item, "prepared")
                    .and_then(|v| get(v, "attempt_id"))
                    .and_then(text)
                    == Ok(id)
            })
            .ok_or(Error("unknown_execution_attempt_id"))?
            .clone();
        value.set_field("attempt", selected)?;
    }
    output(
        &without(&value, &["attempts", "enrollments"])?,
        json,
        &mut work,
    )
}

pub fn graph_inspect_arguments(items: &[PathBuf], json: bool) -> Result<TaskOutput> {
    let (root, rest) = crate::workspace::split_optional_workspace(items)?;
    require(rest.len() <= 1, "invalid_execution_graph_inspect_arguments")?;
    root.to_str().ok_or(Error("non_unicode_path_unsupported"))?;
    let mut work = Work::default();
    let value = inspect(&root, &SystemIo, &mut work)?;
    let execution = get(&value, "execution")?;
    if text(get(execution, "status")?)? != "active" {
        return output(&value, json, &mut work);
    }
    require(
        exact_integer(get(execution, "schema_version")?, 2),
        "execution_upgrade_required",
    )?;
    let revision = match rest.first() {
        Some(path) => {
            let raw = path.to_str().ok_or(Error("non_unicode_path_unsupported"))?;
            if raw.is_empty() {
                get(&value, "current_graph_revision")?.as_str()
            } else {
                Some(raw)
            }
        }
        None => get(&value, "current_graph_revision")?.as_str(),
    };
    let graph = if let Some(revision) = revision {
        require(hex(revision, 32), "invalid_execution_graph_revision")?;
        get(&value, "graphs")?
            .as_array()
            .ok_or(Error("invalid_execution_graphs"))?
            .iter()
            .find(|item| get(item, "graph_revision").and_then(text) == Ok(revision))
            .ok_or(Error("unknown_execution_graph_revision"))?
            .clone()
    } else {
        Value::Null
    };
    output(
        &object(vec![
            ("execution", execution.clone()),
            ("graph", graph),
            ("authorization", string("none")),
            ("dispatchable", Value::Bool(false)),
        ])?,
        json,
        &mut work,
    )
}

pub fn graph_export_arguments(items: &[PathBuf], json: bool) -> Result<TaskOutput> {
    let (workspace, selected) = selector(items)?;
    let revision = selected
        .to_str()
        .ok_or(Error("non_unicode_path_unsupported"))?;
    require(hex(revision, 32), "invalid_execution_graph_revision")?;
    let root = root_argument(workspace)?;
    let (_, validated) = read_typed(&root)?.ok_or(Error("execution_upgrade_required"))?;
    require(validated.schema == 2, "execution_upgrade_required")?;
    let graphs = get(
        validated
            .graph
            .as_ref()
            .ok_or(Error("execution_upgrade_required"))?,
        "graphs",
    )?;
    let graph = graphs
        .as_object()
        .and_then(|v| v.get(revision))
        .ok_or(Error("unknown_execution_graph_revision"))?;
    let value = object(vec![
        ("manifest", get(graph, "manifest")?.clone()),
        ("graph_digest", get(graph, "digest")?.clone()),
        ("execution_revision", string(validated.replayed.revision)),
        ("authorization", string("none")),
        ("dispatchable", Value::Bool(false)),
    ])?;
    output(&value, json, &mut Work::default())
}
