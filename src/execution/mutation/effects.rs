//! Standalone effects only; the checked reducers own semantic admission/results.
use super::super::*;
use super::lifecycle::{Metadata, Operation, Request};

pub(super) fn build(
    control: &Value,
    selected: Option<&Validated>,
    request: &Request<'_>,
    source: &Value,
    now: &str,
    metadata: &dyn Metadata,
    work: &mut Work,
) -> Result<Value> {
    require(
        matches!(
            request.operation,
            Operation::Enable | Operation::Enroll | Operation::Prepare
        ),
        "unsupported_execution_operation",
    )?;
    super::super::source(source, get(control, "snapshot_source_id")?, work)?;
    let mut effect = object(vec![("source", source.clone())])?;
    if matches!(request.operation, Operation::Enable) {
        return Ok(effect);
    }
    let task_id = get(&request.parameters, "task_id")?;
    let task_name = identifier(task_id)?;
    let task = get(source, "tasks")?
        .as_array()
        .ok_or(Error("invalid_execution_source_tasks"))?
        .iter()
        .find(|task| get(task, "id").is_ok_and(|id| id.as_str() == Some(task_name)))
        .ok_or(Error("execution_task_not_found"))?;
    let state = &selected.ok_or(Error("execution_disabled"))?.replayed.state;
    let previous = state.enrollments.iter().find(|e| e.task == task_name);
    let mut record = object(vec![
        ("task_id", task_id.clone()),
        ("task_authority_identity", get(source, "identity")?.clone()),
        ("task_source_revision", get(source, "revision")?.clone()),
        ("complete_task_snapshot", task.clone()),
        (
            "task_snapshot_digest",
            string(work.digest(b"coven-task-snapshot-v1\0", task)?),
        ),
    ])?;
    if matches!(request.operation, Operation::Enroll) {
        record.set_field(
            "enrollment_id",
            string(match previous {
                Some(enrollment) => enrollment.id.clone(),
                None => metadata.uuid()?,
            }),
        )?;
        record.set_field(
            "binding_number",
            integer(previous.map_or(1, |e| e.bindings.len() + 1))?,
        )?;
        record.set_field("enrolled_at", string(now))?;
        effect.set_field("enrollment", record)?;
    } else {
        let explicit = get(&request.parameters, "intent")?;
        intent(explicit, work)?;
        let schema = selected.ok_or(Error("execution_disabled"))?.schema;
        let mut older = state.attempts.iter().filter(|a| a.task == task_name);
        let count = older.clone().count();
        let last = older.next_back();
        record.set_field("schema_version", integer(schema)?)?;
        record.set_field("attempt_id", string(metadata.uuid()?))?;
        record.set_field(
            "enrollment_id",
            previous.map_or(Value::Null, |e| string(&e.id)),
        )?;
        record.set_field("attempt_number", integer(count + 1)?)?;
        record.set_field(
            "previous_attempt_id",
            last.map_or(Value::Null, |a| string(&a.id)),
        )?;
        record.set_field(
            "execution_authority_id",
            get(control, "execution_authority_id")?.clone(),
        )?;
        record.set_field("intent", explicit.clone())?;
        record.set_field("prepared_at", string(now))?;
        let mut payload = object(vec![("explicit_intent", explicit.clone())])?;
        for key in [
            "execution_authority_id",
            "enrollment_id",
            "task_authority_identity",
            "task_source_revision",
            "complete_task_snapshot",
        ] {
            payload.set_field(key, get(&record, key)?.clone())?;
        }
        if schema == 2 {
            record.set_field("graph_binding", Value::Null)?;
            payload.set_field("graph_binding", Value::Null)?;
        }
        record.set_field(
            "intent_digest",
            string(work.digest(
                if schema == 2 {
                    b"coven-task-attempt-intent-v2\0"
                } else {
                    b"coven-task-attempt-intent-v1\0"
                },
                &payload,
            )?),
        )?;
        effect.set_field("prepared", record)?;
    }
    Ok(effect)
}

#[cfg(test)]
mod tests;
