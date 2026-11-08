//! CLI adapters bind selected document values, never their pathnames.
use super::lifecycle::{self, Operation, Request};
use super::*;

#[derive(Clone, Copy)]
pub struct WriteOptions<'a> {
    pub expected_execution_revision: &'a str,
    pub expected_tasks_revision: Option<&'a str>,
    pub request_id: &'a str,
    pub json: bool,
}

pub enum PreparationInput<'a> {
    Intent(&'a Path),
    GraphRevision(&'a str),
}

fn bounded_text(value: &str) -> Result<Value> {
    require(
        value.len() <= values::MAX_STRING,
        "unsupported_json_string_profile",
    )?;
    Ok(string(value))
}

fn selector(items: &[PathBuf]) -> Result<(&Path, &str)> {
    let (workspace, selected) = match items {
        [selected] => (Path::new("."), selected),
        [workspace, selected] => (workspace.as_path(), selected),
        _ => return Err(Error("invalid_execution_selector_arguments")),
    };
    Ok((
        workspace,
        selected
            .to_str()
            .ok_or(Error("non_unicode_path_unsupported"))?,
    ))
}

fn document(path: &Path, graph: bool) -> Result<Value> {
    let selected = crate::platform::workspace_argument(path)?.canonicalize()?;
    let value = values::parse(
        &read_fs::read_regular(&selected, 1_048_576)?,
        Profile::Authority,
    )?;
    let mut work = Work::default();
    if graph {
        super::super::graphs::validate_document(&value, &mut work)?;
    } else {
        intent(&value, &mut work)?;
    }
    Ok(value)
}

fn run(
    workspace: &Path,
    operation: Operation,
    parameters: Value,
    options: WriteOptions<'_>,
) -> Result<TaskOutput> {
    // Upgrade's wrapper binding precedes even workspace resolution.
    let request = Request {
        operation,
        parameters,
        expected_execution: options.expected_execution_revision,
        expected_tasks: options.expected_tasks_revision,
        key: options.request_id,
    };
    if operation == Operation::Upgrade {
        request.binding(2, &mut Work::default())?;
    }
    crate::platform::ensure_supported()?;
    let root = crate::platform::workspace_argument(workspace)?.canonicalize()?;
    require(root.is_dir(), "invalid_execution_workspace")?;
    root.to_str().ok_or(Error("non_unicode_path_unsupported"))?;
    lifecycle::execute(&root, &request, options.json)
}

pub fn enable_arguments(workspace: &Path, options: WriteOptions<'_>) -> Result<TaskOutput> {
    run(workspace, Operation::Enable, object(vec![])?, options)
}

pub fn enroll_arguments(
    items: &[PathBuf],
    rebind: bool,
    options: WriteOptions<'_>,
) -> Result<TaskOutput> {
    let (workspace, task) = selector(items)?;
    run(
        workspace,
        Operation::Enroll,
        object(vec![
            ("task_id", bounded_text(task)?),
            ("rebind_source", Value::Bool(rebind)),
        ])?,
        options,
    )
}

pub fn upgrade_arguments(workspace: &Path, options: WriteOptions<'_>) -> Result<TaskOutput> {
    run(workspace, Operation::Upgrade, object(vec![])?, options)
}

pub fn prepare_arguments(
    items: &[PathBuf],
    input: PreparationInput<'_>,
    options: WriteOptions<'_>,
) -> Result<TaskOutput> {
    // Read and strictly validate the selected file before authority/root preflight.
    let (operation, field, value) = match input {
        PreparationInput::Intent(path) => (Operation::Prepare, "intent", document(path, false)?),
        PreparationInput::GraphRevision(revision) => (
            Operation::PrepareGraph,
            "graph_revision",
            bounded_text(revision)?,
        ),
    };
    let (workspace, task) = selector(items)?;
    run(
        workspace,
        operation,
        object(vec![("task_id", bounded_text(task)?), (field, value)])?,
        options,
    )
}

pub fn propose_arguments(
    workspace: &Path,
    path: &Path,
    options: WriteOptions<'_>,
) -> Result<TaskOutput> {
    let value = document(path, true)?;
    run(
        workspace,
        Operation::GraphPropose,
        object(vec![("document", value)])?,
        options,
    )
}

pub fn decide_arguments(
    items: &[PathBuf],
    decision: &str,
    reason: &str,
    digest: &str,
    options: WriteOptions<'_>,
) -> Result<TaskOutput> {
    let (workspace, revision) = selector(items)?;
    run(
        workspace,
        Operation::GraphDecide,
        object(vec![
            ("graph_revision", bounded_text(revision)?),
            ("decision", bounded_text(decision)?),
            ("reason", bounded_text(reason)?),
            ("expected_graph_digest", bounded_text(digest)?),
        ])?,
        options,
    )
}

#[cfg(test)]
mod tests;
