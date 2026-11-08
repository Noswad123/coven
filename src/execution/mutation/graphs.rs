//! Reviewed graph effects. The retained reducers remain the authority for admission.
use super::super::{
    Error, Result, Validated, Value, Work, get, graphs as frozen, integer, object, require, string,
    text,
};
use super::lifecycle::{Metadata, Operation, Request};

pub(super) fn build(
    control: &Value,
    selected: &Validated,
    request: &Request<'_>,
    source: Option<&Value>,
    now: &str,
    metadata: &dyn Metadata,
    work: &mut Work,
) -> Result<Value> {
    let params = &request.parameters;
    match request.operation {
        Operation::GraphPropose => {
            let source = source.ok_or(Error("execution_source_required"))?;
            let graphs = selected
                .graph_state
                .as_ref()
                .ok_or(Error("unsupported_execution_version"))?;
            let id = if matches!(graphs.id, Value::Null) {
                string(metadata.uuid()?)
            } else {
                graphs.id.clone()
            };
            let manifest = frozen::freeze(
                get(params, "document")?,
                source,
                &selected.replayed.state,
                frozen::FreezeIds {
                    authority: get(control, "execution_authority_id")?,
                    snapshot: get(control, "snapshot_source_id")?,
                    graph_id: &id,
                    revision: &string(metadata.uuid()?),
                    predecessor: &graphs.head,
                    proposed_at: &string(now),
                },
                work,
            )?;
            object(vec![("source", source.clone()), ("manifest", manifest)])
        }
        Operation::GraphDecide => {
            let approve = text(get(params, "decision")?)? == "approve";
            let decision = object(vec![
                ("decision_id", string(metadata.uuid()?)),
                ("graph_revision", get(params, "graph_revision")?.clone()),
                ("decision", get(params, "decision")?.clone()),
                ("reason", get(params, "reason")?.clone()),
                (
                    "state",
                    string(if approve {
                        "approved_for_preparation"
                    } else {
                        "changes_requested"
                    }),
                ),
                ("decided_at", string(now)),
            ])?;
            let mut effect = object(vec![("decision", decision)])?;
            if approve {
                effect.set_field(
                    "source",
                    source.ok_or(Error("execution_source_required"))?.clone(),
                )?;
            }
            Ok(effect)
        }
        Operation::PrepareGraph => {
            let source = source.ok_or(Error("execution_source_required"))?;
            let graphs = selected
                .graph_state
                .as_ref()
                .ok_or(Error("unsupported_execution_version"))?;
            let graph = graphs.head(get(params, "graph_revision")?)?;
            require(
                text(get(graph, "current_review")?)? == "approved_for_preparation",
                "execution_graph_not_approved",
            )?;
            let task_id = text(get(params, "task_id")?)?;
            let node = get(get(graph, "manifest")?, "nodes")?
                .as_array()
                .ok_or(Error("invalid_execution_graph_nodes"))?
                .iter()
                .find(|node| get(node, "task_id").ok().and_then(Value::as_str) == Some(task_id))
                .ok_or(Error("execution_task_not_in_graph"))?;
            let intent = object(vec![
                ("schema_version", integer(1)?),
                ("kind", string("task-intent")),
                ("task_id", get(node, "task_id")?.clone()),
                ("owner", get(node, "owner")?.clone()),
                (
                    "acceptance_criteria",
                    get(node, "acceptance_criteria")?.clone(),
                ),
            ])?;
            let latest = get(graph, "decisions")?
                .as_array()
                .and_then(|decisions| decisions.last())
                .ok_or(Error("execution_graph_not_approved"))?;
            let graph_binding = object(vec![
                ("graph_id", graphs.id.clone()),
                ("graph_revision", graphs.head.clone()),
                ("graph_digest", get(graph, "digest")?.clone()),
                ("decision_id", get(latest, "decision_id")?.clone()),
                ("node_digest", get(node, "node_digest")?.clone()),
            ])?;
            let state = &selected.replayed.state;
            let task = get(source, "tasks")?
                .as_array()
                .ok_or(Error("invalid_execution_source_tasks"))?
                .iter()
                .find(|task| get(task, "id").ok().and_then(Value::as_str) == Some(task_id))
                .ok_or(Error("execution_task_not_found"))?;
            let enrollment = state.enrollments.iter().find(|e| e.task == task_id);
            let mut older = state.attempts.iter().filter(|a| a.task == task_id);
            let count = older.clone().count();
            let last = older.next_back();
            let mut record = object(vec![
                ("schema_version", integer(2)?),
                ("attempt_id", string(metadata.uuid()?)),
                ("task_id", string(task_id)),
                (
                    "enrollment_id",
                    enrollment.map_or(Value::Null, |e| string(&e.id)),
                ),
                ("attempt_number", integer(count + 1)?),
                (
                    "previous_attempt_id",
                    last.map_or(Value::Null, |a| string(&a.id)),
                ),
                (
                    "execution_authority_id",
                    get(control, "execution_authority_id")?.clone(),
                ),
                ("task_authority_identity", get(source, "identity")?.clone()),
                ("task_source_revision", get(source, "revision")?.clone()),
                ("complete_task_snapshot", task.clone()),
                (
                    "task_snapshot_digest",
                    string(work.digest(b"coven-task-snapshot-v1\0", task)?),
                ),
                ("intent", intent.clone()),
                ("graph_binding", graph_binding.clone()),
                ("prepared_at", string(now)),
            ])?;
            let mut payload = object(vec![
                ("explicit_intent", intent),
                ("graph_binding", graph_binding),
            ])?;
            for key in [
                "execution_authority_id",
                "enrollment_id",
                "task_authority_identity",
                "task_source_revision",
                "complete_task_snapshot",
            ] {
                payload.set_field(key, get(&record, key)?.clone())?;
            }
            record.set_field(
                "intent_digest",
                string(work.digest(b"coven-task-attempt-intent-v2\0", &payload)?),
            )?;
            object(vec![("source", source.clone()), ("prepared", record)])
        }
        _ => Err(Error("unsupported_execution_operation")),
    }
}

#[cfg(test)]
mod tests;
