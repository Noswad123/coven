//! Pure current-source observations over already validated execution authority.
use super::super::*;

fn binding_status(
    record: &Value,
    source: Option<&Value>,
    error: Option<&str>,
    work: &mut Work,
) -> Result<Value> {
    let Some(source) = source else {
        return object(vec![
            ("status", string("source_unavailable")),
            (
                "reason",
                string(
                    error
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Source not assessed"),
                ),
            ),
        ]);
    };
    let (status, reason) = if !work.equal(
        get(record, "task_authority_identity")?,
        get(source, "identity")?,
    )? {
        (
            "source_authority_changed",
            Some("Explicit source rebind is required before new preparation"),
        )
    } else {
        let task_id = text(get(record, "task_id")?)?;
        let task = get(source, "tasks")?
            .as_array()
            .ok_or(Error("invalid_execution_tasks"))?
            .iter()
            .find(|task| get(task, "id").ok().and_then(Value::as_str) == Some(task_id));
        match task {
            None => ("task_missing", None),
            Some(task)
                if work.digest(b"coven-task-snapshot-v1\0", task)?
                    != text(get(record, "task_snapshot_digest")?)? =>
            {
                ("task_changed", None)
            }
            Some(_) => {
                let owner = get(record, "complete_task_snapshot")?
                    .as_object()
                    .and_then(|o| o.get("owner"));
                let roster = get(source, "roster")?
                    .as_array()
                    .ok_or(Error("invalid_execution_roster"))?;
                if owner.is_some_and(|owner| {
                    !matches!(owner, Value::Null)
                        && !roster.iter().any(|v| v.as_str() == owner.as_str())
                }) {
                    ("owner_missing", None)
                } else if !work.equal(
                    get(record, "task_source_revision")?,
                    get(source, "revision")?,
                )? {
                    (
                        "source_revision_changed",
                        Some(
                            "Whole-task authority revision differs, even if selected task content is unchanged",
                        ),
                    )
                } else {
                    ("current", None)
                }
            }
        }
    };
    let mut result = object(vec![("status", string(status))])?;
    if let Some(reason) = reason {
        result.set_field("reason", string(reason))?;
    }
    Ok(result)
}

fn graph_projection(
    revision: &Value,
    graph: &Value,
    head: &Value,
    state: &Replay,
    source: Option<&Value>,
    error: Option<&str>,
    work: &mut Work,
) -> Result<Value> {
    // Admit retained payloads before copying them into the presentation.
    work.encode(graph)?;
    let manifest = get(graph, "manifest")?;
    let matched = match source {
        Some(source) => match graphs::match_source(manifest, source, state, work) {
            Ok(matched) => matched,
            Err(error) if super::profile_failure(error.clone()) => return Err(error),
            Err(error) => object(vec![
                ("status", string("source_unavailable")),
                ("reason", string(error.code())),
            ])?,
        },
        None => object(vec![
            ("status", string("source_unavailable")),
            ("reason", error.map_or(Value::Null, string)),
        ])?,
    };
    let current = text(revision).ok() == head.as_str();
    let eligible = current
        && text(get(graph, "current_review")?)? == "approved_for_preparation"
        && text(get(&matched, "status")?)? == "current";
    let frozen_nodes = get(manifest, "nodes")?
        .as_array()
        .ok_or(Error("invalid_execution_graph_nodes"))?;
    let mut nodes = Vec::with_capacity(frozen_nodes.len());
    for node in frozen_nodes {
        let id = text(get(node, "task_id")?)?;
        let dependencies = get(node, "depends_on")?;
        let has_dependencies = !dependencies
            .as_array()
            .ok_or(Error("invalid_graph_dependencies"))?
            .is_empty();
        nodes.push(object(vec![
            ("task_id", string(id)),
            ("depends_on", dependencies.clone()),
            (
                "preparation_eligible",
                Value::Bool(eligible && !state.attempts.iter().any(|a| a.active() && a.task == id)),
            ),
            (
                "dependency_acceptance",
                string(if has_dependencies {
                    "unestablished"
                } else {
                    "not_required"
                }),
            ),
            ("authorization", string("none")),
            ("dispatchable", Value::Bool(false)),
        ])?);
    }
    let affected = state
        .attempts
        .iter()
        .filter(|a| {
            a.prepared
                .as_object()
                .and_then(|o| o.get("graph_binding"))
                .and_then(Value::as_object)
                .and_then(|o| o.get("graph_revision"))
                .and_then(Value::as_str)
                == revision.as_str()
        })
        .map(|a| string(&a.id))
        .collect();
    object(vec![
        ("graph_revision", revision.clone()),
        ("graph_digest", get(graph, "digest")?.clone()),
        ("current", Value::Bool(current)),
        ("manifest", manifest.clone()),
        ("planning_review", get(graph, "current_review")?.clone()),
        ("decisions", get(graph, "decisions")?.clone()),
        ("source_binding", matched),
        (
            "dependency_order",
            Value::Array(
                graphs::order(frozen_nodes)?
                    .into_iter()
                    .map(string)
                    .collect(),
            ),
        ),
        ("preparation_eligible", Value::Bool(eligible)),
        ("nodes", Value::Array(nodes)),
        ("affected_preparations", Value::Array(affected)),
        ("authorization", string("none")),
        ("dispatchable", Value::Bool(false)),
    ])
}

pub(super) fn active(
    control: &Value,
    retained: &super::super::Validated,
    source: Option<&Value>,
    error: Option<&str>,
    work: &mut super::super::Work,
) -> Result<Value> {
    if let Some(source) = source {
        work.encode(source)?;
    }
    let state = &retained.replayed.state;
    let mut enrollments = Vec::with_capacity(state.enrollments.len());
    for enrollment in &state.enrollments {
        let mut bindings = Vec::with_capacity(enrollment.bindings.len());
        for binding in &enrollment.bindings {
            work.encode(binding)?;
            bindings.push(binding.clone());
        }
        let latest = enrollment
            .bindings
            .last()
            .ok_or(Error("invalid_execution_enrollment"))?;
        enrollments.push(object(vec![
            ("enrollment_id", string(&enrollment.id)),
            ("task_id", string(&enrollment.task)),
            ("bindings", Value::Array(bindings)),
            (
                "source_binding",
                binding_status(latest, source, error, work)?,
            ),
        ])?);
    }
    let mut projected_graphs = Vec::new();
    if let Some(metadata) = &retained.graph {
        let head = get(metadata, "current_graph_revision")?;
        let graphs = get(metadata, "graphs")?;
        // History has already been validated. Scan only proposal identities, not
        // receipts or the old historical report's lexically sorted graph map.
        for entry in get(control, "history")?
            .as_array()
            .ok_or(Error("invalid_execution_history"))?
        {
            if text(get(get(entry, "binding")?, "operation")?)? == "graph_propose" {
                let revision = get(get(get(entry, "effect")?, "manifest")?, "graph_revision")?;
                projected_graphs.push(graph_projection(
                    revision,
                    get(graphs, text(revision)?)?,
                    head,
                    state,
                    source,
                    error,
                    work,
                )?);
            }
        }
    }
    let mut attempts = Vec::with_capacity(state.attempts.len());
    for attempt in &state.attempts {
        work.encode(&attempt.prepared)?;
        let mut transitions = Vec::with_capacity(attempt.transitions.len());
        for transition in &attempt.transitions {
            work.encode(transition)?;
            transitions.push(transition.clone());
        }
        let mut projected = object(vec![
            ("prepared", attempt.prepared.clone()),
            ("state", string(attempt.state())),
            ("transitions", Value::Array(transitions)),
            (
                "source_binding",
                binding_status(&attempt.prepared, source, error, work)?,
            ),
        ])?;
        if let Some(metadata) = &retained.graph {
            let binding = attempt
                .prepared
                .as_object()
                .and_then(|o| o.get("graph_binding"))
                .filter(|v| !matches!(v, Value::Null));
            let status = match binding {
                None => "not_graph_bound",
                Some(binding) => {
                    let revision = get(binding, "graph_revision")?;
                    if revision.as_str() != get(metadata, "current_graph_revision")?.as_str() {
                        "superseded_revision"
                    } else {
                        let graph = get(get(metadata, "graphs")?, text(revision)?)?;
                        let latest = get(graph, "decisions")?.as_array().and_then(|d| d.last());
                        if text(get(graph, "current_review")?)? != "approved_for_preparation"
                            || latest
                                .and_then(|d| get(d, "decision_id").ok())
                                .and_then(Value::as_str)
                                != get(binding, "decision_id")?.as_str()
                        {
                            "planning_review_changed"
                        } else {
                            let report = projected_graphs
                                .iter()
                                .find(|g| {
                                    get(g, "graph_revision").ok().and_then(Value::as_str)
                                        == revision.as_str()
                                })
                                .ok_or(Error("execution_graph_not_head"))?;
                            if text(get(get(report, "source_binding")?, "status")?)? == "current" {
                                "current"
                            } else {
                                "source_unavailable_or_stale"
                            }
                        }
                    }
                }
            };
            projected.set_field("graph_binding_status", string(status))?;
        }
        attempts.push(projected);
    }
    let mut result = object(vec![
        (
            "execution",
            object(vec![
                ("status", string("active")),
                ("revision", string(&retained.replayed.revision)),
                ("execution_authority_id", string(&retained.authority)),
                ("generation", get(control, "generation")?.clone()),
                ("schema_version", integer(retained.schema)?),
            ])?,
        ),
        ("authorization", string("none")),
        ("dispatchable", Value::Bool(false)),
        (
            "blockers",
            Value::Array(
                [
                    "PREPARATION_ONLY",
                    "NATIVE_ADMISSION_NOT_IMPLEMENTED",
                    "TASK_EXECUTION_AUTHORIZATION_NOT_IMPLEMENTED",
                ]
                .into_iter()
                .map(string)
                .collect(),
            ),
        ),
        (
            "source_assessment",
            object(vec![
                ("coordinated", Value::Bool(source.is_some())),
                ("error", error.map_or(Value::Null, string)),
                (
                    "revision",
                    source
                        .map(|s| get(s, "revision").cloned())
                        .transpose()?
                        .unwrap_or(Value::Null),
                ),
            ])?,
        ),
        ("enrollments", Value::Array(enrollments)),
        ("attempts", Value::Array(attempts)),
    ])?;
    if let Some(metadata) = &retained.graph {
        result.set_field("graphs", Value::Array(projected_graphs))?;
        result.set_field(
            "current_graph_revision",
            get(metadata, "current_graph_revision")?.clone(),
        )?;
        work.encode(get(metadata, "graph_managed_tasks")?)?;
        result.set_field(
            "graph_managed_tasks",
            get(metadata, "graph_managed_tasks")?.clone(),
        )?;
    }
    work.encode(&result)?;
    Ok(result)
}

#[cfg(test)]
mod tests;
