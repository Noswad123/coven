use super::*;

fn vectors() -> Value {
    values::parse(include_bytes!("vectors.json"), Profile::Authority).unwrap()
}

#[test]
fn frozen_python_full_reports_status_precedence_order_and_membership() {
    for vector in vectors().as_array().unwrap() {
        let control = get(vector, "control").unwrap();
        let retained = validated(control).unwrap();
        for case in get(vector, "cases").unwrap().as_array().unwrap() {
            let source = get(case, "source").unwrap();
            let source = (!matches!(source, Value::Null)).then_some(source);
            let error = get(case, "error").unwrap().as_str();
            let actual = active(control, &retained, source, error, &mut Work::default()).unwrap();
            assert_eq!(
                String::from_utf8(actual.canonical().unwrap()).unwrap(),
                String::from_utf8(get(case, "expected").unwrap().canonical().unwrap()).unwrap(),
                "{} / {}",
                text(get(vector, "name").unwrap()).unwrap(),
                text(get(case, "name").unwrap()).unwrap()
            );
        }
    }
}

#[test]
fn aggregate_work_exact_boundary_is_fatal_including_graph_matching() {
    let vectors = vectors();
    let vector = vectors.as_array().unwrap().last().unwrap();
    let control = get(vector, "control").unwrap();
    let retained = validated(control).unwrap();
    let case = &get(vector, "cases").unwrap().as_array().unwrap()[0];
    let source = Some(get(case, "source").unwrap());
    let mut measured = Work::default();
    let expected = active(control, &retained, source, None, &mut measured).unwrap();
    let mut exact = Work(MAX_WORK - measured.0);
    assert_eq!(
        active(control, &retained, source, None, &mut exact)
            .unwrap()
            .canonical()
            .unwrap(),
        expected.canonical().unwrap()
    );
    assert_eq!(exact.0, MAX_WORK);
    let mut short = Work(MAX_WORK - measured.0 + 1);
    assert_eq!(
        active(control, &retained, source, None, &mut short)
            .unwrap_err()
            .0,
        "unsupported_execution_work_profile"
    );
    let metadata = retained.graph.as_ref().unwrap();
    let head = get(metadata, "current_graph_revision").unwrap();
    let graph = get(get(metadata, "graphs").unwrap(), text(head).unwrap()).unwrap();
    let graph_admission = graph.canonical().unwrap().len();
    let mut exhausted = Work(MAX_WORK - graph_admission);
    assert_eq!(
        graph_projection(
            head,
            graph,
            head,
            &retained.replayed.state,
            source,
            None,
            &mut exhausted
        )
        .unwrap_err()
        .0,
        "unsupported_execution_work_profile"
    );
}

#[test]
fn graph_match_ordinary_error_is_diagnostic_not_permission() {
    let vectors = vectors();
    let vector = vectors.as_array().unwrap().last().unwrap();
    let retained = validated(get(vector, "control").unwrap()).unwrap();
    let metadata = retained.graph.as_ref().unwrap();
    let head = get(metadata, "current_graph_revision").unwrap();
    let graph = get(get(metadata, "graphs").unwrap(), text(head).unwrap()).unwrap();
    let actual = graph_projection(
        head,
        graph,
        head,
        &retained.replayed.state,
        Some(&Value::Null),
        None,
        &mut Work::default(),
    )
    .unwrap();
    assert_eq!(
        text(get(get(&actual, "source_binding").unwrap(), "status").unwrap()).unwrap(),
        "source_unavailable"
    );
    assert!(matches!(
        get(&actual, "preparation_eligible").unwrap(),
        Value::Bool(false)
    ));
}

#[test]
fn frozen_owner_absent_or_null_and_exact_task_selection() {
    let vectors = vectors();
    let vector = &vectors.as_array().unwrap()[0];
    let control = get(vector, "control").unwrap();
    let retained = validated(control).unwrap();
    let original = &retained.replayed.state.enrollments[0].bindings[0];
    let case = &get(vector, "cases").unwrap().as_array().unwrap()[0];
    let mut source = get(case, "source").unwrap().clone();
    let mut tasks = get(&source, "tasks").unwrap().as_array().unwrap().to_vec();
    // An earlier unrelated task must never substitute for the exact selected ID.
    tasks.reverse();
    source
        .set_field("tasks", Value::Array(tasks.clone()))
        .unwrap();
    assert_eq!(
        text(
            get(
                &binding_status(original, Some(&source), None, &mut Work::default()).unwrap(),
                "status"
            )
            .unwrap()
        )
        .unwrap(),
        "current"
    );
    for owner in [None, Some(Value::Null)] {
        let snapshot = get(original, "complete_task_snapshot").unwrap();
        let mut task = Value::object(
            snapshot
                .as_object()
                .unwrap()
                .entries()
                .iter()
                .filter(|(key, _)| key != "owner")
                .cloned()
                .collect(),
        )
        .unwrap();
        if let Some(owner) = owner {
            task.set_field("owner", owner).unwrap();
        }
        let mut record = original.clone();
        record
            .set_field("complete_task_snapshot", task.clone())
            .unwrap();
        record
            .set_field(
                "task_snapshot_digest",
                string(
                    Work::default()
                        .digest(b"coven-task-snapshot-v1\0", &task)
                        .unwrap(),
                ),
            )
            .unwrap();
        let mut current_tasks = tasks.clone();
        let selected = current_tasks
            .iter_mut()
            .find(|t| get(t, "id").unwrap().as_str() == Some("z"))
            .unwrap();
        *selected = task;
        source
            .set_field("tasks", Value::Array(current_tasks))
            .unwrap();
        source.set_field("roster", Value::Array(vec![])).unwrap();
        assert_eq!(
            text(
                get(
                    &binding_status(&record, Some(&source), None, &mut Work::default()).unwrap(),
                    "status"
                )
                .unwrap()
            )
            .unwrap(),
            "current"
        );
    }
    assert_eq!(
        text(
            get(
                &binding_status(original, None, Some(""), &mut Work::default()).unwrap(),
                "reason"
            )
            .unwrap()
        )
        .unwrap(),
        "Source not assessed"
    );
}
