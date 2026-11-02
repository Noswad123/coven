//! Pure graph validation and refreezing from retained source and replay state.
use super::{
    Error, Number, Replay, Result, TaskRecords, Value, Work, exact_integer, fields, get,
    identifier, integer, intent, object, require, source, string, task_revision, text, timestamp,
    uuid,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};

const MAX_BYTES: usize = 1_048_576;
const DOCUMENT_NODE: &str = "task_id owner acceptance_criteria depends_on";
const FROZEN_NODE: &str = "task_id owner acceptance_criteria depends_on enrollment_id binding_number complete_task_snapshot task_snapshot_digest node_digest";

pub(super) struct FreezeIds<'a> {
    pub authority: &'a Value,
    pub snapshot: &'a Value,
    pub graph_id: &'a Value,
    pub revision: &'a Value,
    pub predecessor: &'a Value,
    pub proposed_at: &'a Value,
}

pub(super) fn digest(manifest: &Value, work: &mut Work) -> Result<String> {
    Ok(format!(
        "task-graph-manifest-v1:{}",
        work.digest(b"coven-task-graph-manifest-v1\0", manifest)?
    ))
}

pub(super) fn node_digest(node: &Value, work: &mut Work) -> Result<String> {
    let entries = node
        .as_object()
        .ok_or(Error("invalid_graph_node"))?
        .entries()
        .iter()
        .filter(|(key, _)| key != "node_digest")
        .cloned()
        .collect();
    Ok(format!(
        "task-graph-node-v1:{}",
        work.digest(b"coven-task-graph-node-v1\0", &Value::object(entries)?)?
    ))
}

fn header(value: &Value, kind: &str, names: &str, work: &mut Work) -> Result<()> {
    require(work.encode(value)?.len() <= MAX_BYTES, "graph_size_limit")?;
    fields(value, names)?;
    require(
        exact_integer(get(value, "schema_version")?, 1),
        "unsupported_graph_version",
    )?;
    require(text(get(value, "kind")?)? == kind, "invalid_graph_kind")
}

// This order is only for validation; persisted node and dependency arrays stay intact.
pub(super) fn order(nodes: &[Value]) -> Result<Vec<&str>> {
    let mut pending = BTreeMap::new();
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for node in nodes {
        let id = text(get(node, "task_id")?)?;
        let deps = get(node, "depends_on")?
            .as_array()
            .ok_or(Error("invalid_graph_dependencies"))?;
        pending.insert(id, deps.len());
        children.insert(id, vec![]);
    }
    for node in nodes {
        let id = text(get(node, "task_id")?)?;
        for dep in get(node, "depends_on")?
            .as_array()
            .ok_or(Error("invalid_graph_dependencies"))?
        {
            let dep = text(dep)?;
            require(dep != id, "graph_self_dependency")?;
            children
                .get_mut(dep)
                .ok_or(Error("graph_dangling_dependency"))?
                .push(id);
        }
    }
    let mut ready: BTreeSet<_> = pending
        .iter()
        .filter_map(|(&id, &count)| (count == 0).then_some(id))
        .collect();
    let mut result = Vec::with_capacity(nodes.len());
    while let Some(id) = ready.pop_first() {
        result.push(id);
        for child in &children[id] {
            let count = pending
                .get_mut(child)
                .ok_or(Error("graph_dangling_dependency"))?;
            *count -= 1;
            if *count == 0 {
                ready.insert(child);
            }
        }
    }
    require(result.len() == nodes.len(), "graph_cycle")?;
    Ok(result)
}

fn nodes<'a>(value: &'a Value, frozen: bool, work: &mut Work) -> Result<&'a [Value]> {
    let nodes = value.as_array().ok_or(Error("invalid_graph_nodes"))?;
    require((1..=64).contains(&nodes.len()), "invalid_graph_node_count")?;
    let mut seen = HashSet::new();
    let mut edges = 0;
    for node in nodes {
        fields(node, if frozen { FROZEN_NODE } else { DOCUMENT_NODE })?;
        intent(
            &object(vec![
                ("schema_version", integer(1)?),
                ("kind", string("task-intent")),
                ("task_id", get(node, "task_id")?.clone()),
                ("owner", get(node, "owner")?.clone()),
                (
                    "acceptance_criteria",
                    get(node, "acceptance_criteria")?.clone(),
                ),
            ])?,
            work,
        )?;
        require(
            seen.insert(text(get(node, "task_id")?)?),
            "duplicate_graph_task_id",
        )?;
        let deps = get(node, "depends_on")?
            .as_array()
            .ok_or(Error("invalid_graph_dependencies"))?;
        let mut unique = HashSet::new();
        for dep in deps {
            require(
                unique.insert(identifier(dep)?),
                "duplicate_graph_dependency",
            )?;
        }
        edges += deps.len();
    }
    require(edges <= 256, "graph_edge_limit")?;
    order(nodes)?;
    Ok(nodes)
}

pub(super) fn validate_document(value: &Value, work: &mut Work) -> Result<()> {
    header(value, "task-graph", "schema_version kind nodes", work)?;
    nodes(get(value, "nodes")?, false, work)?;
    Ok(())
}

fn identity(value: &Value, revision: &Value) -> Result<()> {
    task_revision(revision)?;
    let revision = text(revision)?;
    match text(get(value, "kind")?)? {
        "snapshot-v1" => {
            fields(value, "kind source_id path")?;
            uuid(get(value, "source_id")?)?;
            require(
                text(get(value, "path")?)? == "state/tasks.json"
                    && (revision == "tasks-missing:v1" || revision.starts_with("tasks-sha256:")),
                "graph_snapshot_identity_mismatch",
            )
        }
        "task-ledger-v1" => {
            fields(value, "kind workspace_id epoch_id path")?;
            let workspace = uuid(get(value, "workspace_id")?)?;
            let epoch = uuid(get(value, "epoch_id")?)?;
            // A selected graph cannot reconstruct a whole-ledger digest.
            require(
                text(get(value, "path")?)? == "state/task-ledger"
                    && revision.starts_with(&format!("tasks-ledger:v1:{workspace}:{epoch}:")),
                "graph_ledger_identity_mismatch",
            )
        }
        _ => Err(Error("unsupported_graph_source_identity")),
    }
}

fn positive(value: &Value) -> Result<()> {
    require(
        matches!(value, Value::Number(Number::Integer(n))
            if n.as_str().as_bytes().first().is_some_and(|b| matches!(b, b'1'..=b'9'))),
        "invalid_graph_binding_number",
    )
}

fn task_snapshot(task: &Value, id: &Value) -> Result<()> {
    TaskRecords::validate(Value::Array(vec![task.clone()]))?;
    require(
        text(get(task, "id")?)? == text(id)?,
        "graph_task_id_mismatch",
    )
}

pub(super) fn validate_manifest(value: &Value, work: &mut Work) -> Result<()> {
    header(
        value,
        "task-graph-manifest",
        "schema_version kind execution_authority_id graph_id graph_revision predecessor task_authority_identity task_source_revision nodes proposed_at",
        work,
    )?;
    for key in ["execution_authority_id", "graph_id", "graph_revision"] {
        uuid(get(value, key)?)?;
    }
    if !matches!(get(value, "predecessor")?, Value::Null) {
        uuid(get(value, "predecessor")?)?;
    }
    timestamp(get(value, "proposed_at")?)?;
    identity(
        get(value, "task_authority_identity")?,
        get(value, "task_source_revision")?,
    )?;
    require(
        text(get(value, "task_source_revision")?)? != "tasks-missing:v1",
        "graph_missing_task_source",
    )?;
    let nodes = nodes(get(value, "nodes")?, true, work)?;
    let mut enrollments = HashSet::new();
    for node in nodes {
        require(
            enrollments.insert(uuid(get(node, "enrollment_id")?)?),
            "duplicate_graph_enrollment_id",
        )?;
        positive(get(node, "binding_number")?)?;
        let task = get(node, "complete_task_snapshot")?;
        task_snapshot(task, get(node, "task_id")?)?;
        require(
            task.as_object()
                .and_then(|o| o.get("owner"))
                .and_then(Value::as_str)
                == Some(text(get(node, "owner")?)?),
            "graph_frozen_owner_mismatch",
        )?;
        require(
            text(get(node, "task_snapshot_digest")?)?
                == work.digest(b"coven-task-snapshot-v1\0", task)?,
            "graph_task_snapshot_digest_mismatch",
        )?;
        require(
            text(get(node, "node_digest")?)? == node_digest(node, work)?,
            "graph_node_digest_mismatch",
        )?;
    }
    Ok(())
}

fn enrollment<'a>(state: &'a Replay, task_id: &str, work: &mut Work) -> Result<Option<&'a Value>> {
    let Some(enrollment) = state.enrollments.iter().find(|e| e.task == task_id) else {
        return Ok(None);
    };
    uuid(&string(&enrollment.id))?;
    let binding = enrollment
        .bindings
        .last()
        .ok_or(Error("graph_enrollment_missing_binding"))?;
    fields(
        binding,
        "enrollment_id task_id binding_number task_authority_identity task_source_revision complete_task_snapshot task_snapshot_digest enrolled_at",
    )?;
    positive(get(binding, "binding_number")?)?;
    require(
        text(get(binding, "task_id")?)? == task_id
            && text(get(binding, "enrollment_id")?)? == enrollment.id,
        "graph_enrollment_identity_mismatch",
    )?;
    identity(
        get(binding, "task_authority_identity")?,
        get(binding, "task_source_revision")?,
    )?;
    timestamp(get(binding, "enrolled_at")?)?;
    let task = get(binding, "complete_task_snapshot")?;
    task_snapshot(task, get(binding, "task_id")?)?;
    require(
        text(get(binding, "task_snapshot_digest")?)?
            == work.digest(b"coven-task-snapshot-v1\0", task)?,
        "graph_enrollment_snapshot_digest_mismatch",
    )?;
    Ok(Some(binding))
}

fn account_enrollments(state: &Replay, work: &mut Work) -> Result<()> {
    let map = Value::object(
        state
            .enrollments
            .iter()
            .map(|e| {
                Ok((
                    e.task.clone(),
                    object(vec![
                        ("enrollment_id", string(&e.id)),
                        ("task_id", string(&e.task)),
                        ("bindings", Value::Array(e.bindings.clone())),
                    ])?,
                ))
            })
            .collect::<Result<_>>()?,
    )?;
    work.encode(&map)?;
    Ok(())
}

pub(super) fn freeze(
    document: &Value,
    retained_source: &Value,
    state: &Replay,
    ids: FreezeIds<'_>,
    work: &mut Work,
) -> Result<Value> {
    validate_document(document, work)?;
    uuid(ids.snapshot)?;
    source(retained_source, ids.snapshot, work)?;
    account_enrollments(state, work)?;
    let tasks = get(retained_source, "tasks")?
        .as_array()
        .ok_or(Error("invalid_graph_source_tasks"))?;
    let roster = get(retained_source, "roster")?
        .as_array()
        .ok_or(Error("invalid_graph_roster"))?;
    let identity = get(retained_source, "identity")?;
    let mut frozen = Vec::new();
    for node in get(document, "nodes")?
        .as_array()
        .ok_or(Error("invalid_graph_nodes"))?
    {
        let task_id = text(get(node, "task_id")?)?;
        let task = tasks
            .iter()
            .find(|t| {
                t.as_object()
                    .and_then(|o| o.get("id"))
                    .and_then(Value::as_str)
                    == Some(task_id)
            })
            .ok_or(Error("graph_selected_task_not_found"))?;
        let owner = text(get(node, "owner")?)?;
        require(
            task.as_object()
                .and_then(|o| o.get("owner"))
                .and_then(Value::as_str)
                == Some(owner)
                && roster.iter().any(|v| v.as_str() == Some(owner)),
            "graph_owner_assignment_mismatch",
        )?;
        let binding = enrollment(state, task_id, work)?.ok_or(Error("graph_task_not_enrolled"))?;
        require(
            work.equal(get(binding, "task_authority_identity")?, identity)?,
            "graph_enrollment_source_mismatch",
        )?;
        let mut node = node.clone();
        node.set_field("enrollment_id", get(binding, "enrollment_id")?.clone())?;
        node.set_field("binding_number", get(binding, "binding_number")?.clone())?;
        node.set_field("complete_task_snapshot", task.clone())?;
        node.set_field(
            "task_snapshot_digest",
            string(work.digest(b"coven-task-snapshot-v1\0", task)?),
        )?;
        node.set_field("node_digest", string(node_digest(&node, work)?))?;
        frozen.push(node);
    }
    let manifest = object(vec![
        ("schema_version", integer(1)?),
        ("kind", string("task-graph-manifest")),
        ("execution_authority_id", ids.authority.clone()),
        ("graph_id", ids.graph_id.clone()),
        ("graph_revision", ids.revision.clone()),
        ("predecessor", ids.predecessor.clone()),
        ("task_authority_identity", identity.clone()),
        (
            "task_source_revision",
            get(retained_source, "revision")?.clone(),
        ),
        ("nodes", Value::Array(frozen)),
        ("proposed_at", ids.proposed_at.clone()),
    ])?;
    validate_manifest(&manifest, work)?;
    Ok(manifest)
}

pub(super) fn match_source(
    manifest: &Value,
    retained_source: &Value,
    state: &Replay,
    work: &mut Work,
) -> Result<Value> {
    validate_manifest(manifest, work)?;
    work.encode(retained_source)?;
    fields(retained_source, "identity revision tasks roster")?;
    let source_identity = get(retained_source, "identity")?;
    identity(source_identity, get(retained_source, "revision")?)?;
    let zero = string("00000000000000000000000000000000");
    let snapshot = source_identity
        .as_object()
        .and_then(|o| o.get("source_id"))
        .unwrap_or(&zero);
    source(retained_source, snapshot, work)?;
    account_enrollments(state, work)?;
    let tasks = get(retained_source, "tasks")?
        .as_array()
        .ok_or(Error("invalid_graph_source_tasks"))?;
    let roster = get(retained_source, "roster")?
        .as_array()
        .ok_or(Error("invalid_graph_roster"))?;
    let changed_authority =
        !work.equal(source_identity, get(manifest, "task_authority_identity")?)?;
    let mut nodes: Vec<_> = get(manifest, "nodes")?
        .as_array()
        .ok_or(Error("invalid_graph_nodes"))?
        .iter()
        .map(|node| Ok((text(get(node, "task_id")?)?, node)))
        .collect::<Result<_>>()?;
    nodes.sort_by_key(|&(id, _)| id);
    let priority = [
        "source_authority_changed",
        "selected_inputs_changed",
        "enrollment_changed",
        "owner_missing",
    ];
    let mut statuses = [false; 4];
    let mut reasons = Vec::new();
    for (task_id, node) in nodes {
        let task = tasks.iter().find(|t| {
            t.as_object()
                .and_then(|o| o.get("id"))
                .and_then(Value::as_str)
                == Some(task_id)
        });
        let binding = enrollment(state, task_id, work)?;
        let changed_inputs = match task {
            Some(task) => !work.equal(task, get(node, "complete_task_snapshot")?)?,
            None => true,
        };
        let changed_enrollment = match binding {
            Some(binding) => {
                !work.equal(get(binding, "enrollment_id")?, get(node, "enrollment_id")?)?
                    || !work.equal(
                        get(binding, "binding_number")?,
                        get(node, "binding_number")?,
                    )?
                    || !work.equal(get(binding, "task_authority_identity")?, source_identity)?
            }
            None => true,
        };
        let owner = text(get(node, "owner")?)?;
        let missing_owner = !roster.iter().any(|v| v.as_str() == Some(owner));
        for (index, changed) in [
            changed_authority,
            changed_inputs,
            changed_enrollment,
            missing_owner,
        ]
        .into_iter()
        .enumerate()
        {
            if changed {
                statuses[index] = true;
                reasons.push(object(vec![
                    ("task_id", string(task_id)),
                    ("reason", string(priority[index])),
                ])?);
            }
        }
    }
    let status = statuses
        .iter()
        .position(|&changed| changed)
        .map_or("current", |i| priority[i]);
    object(vec![
        ("status", string(status)),
        (
            "whole_revision_changed",
            Value::Bool(!work.equal(
                get(retained_source, "revision")?,
                get(manifest, "task_source_revision")?,
            )?),
        ),
        ("reasons", Value::Array(reasons)),
    ])
}

#[cfg(test)]
mod tests {
    use super::super::{Enrollment, Profile, values};
    use super::*;

    fn parse(s: &str) -> Value {
        values::parse(s.as_bytes(), Profile::Authority).unwrap()
    }

    fn document(count: usize) -> Value {
        object(vec![
            ("schema_version", integer(1).unwrap()),
            ("kind", string("task-graph")),
            (
                "nodes",
                Value::Array(
                    (0..count)
                        .map(|i| {
                            object(vec![
                                ("task_id", string(format!("n{i:03}"))),
                                ("owner", string("worker")),
                                (
                                    "acceptance_criteria",
                                    parse(r#"[{"id":"proof","text":"Explicit evidence"}]"#),
                                ),
                                ("depends_on", Value::Array(vec![])),
                            ])
                            .unwrap()
                        })
                        .collect(),
                ),
            ),
        ])
        .unwrap()
    }

    fn fixture(count: usize) -> (Value, Value, Replay) {
        let doc = document(count);
        let mut source = parse(
            r#"{"identity":{"kind":"snapshot-v1","source_id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","path":"state/tasks.json"},"revision":"tasks-sha256:0000000000000000000000000000000000000000000000000000000000000000","tasks":[],"roster":["worker"]}"#,
        );
        let mut state = Replay::default();
        let mut tasks = Vec::new();
        for i in 0..count {
            let id = format!("n{i:03}");
            let eid = format!("{i:032x}");
            let mut task = parse(
                r#"{"id":"placeholder","title":"Task","status":"open","owner":"worker","extension":[1.0,123456789012345678901234567890,-0.0,"\u03bb"]}"#,
            );
            task.set_field("id", string(&id)).unwrap();
            let binding = object(vec![
                ("enrollment_id", string(&eid)),
                ("task_id", string(&id)),
                ("binding_number", integer(1).unwrap()),
                (
                    "task_authority_identity",
                    get(&source, "identity").unwrap().clone(),
                ),
                (
                    "task_source_revision",
                    get(&source, "revision").unwrap().clone(),
                ),
                ("complete_task_snapshot", task.clone()),
                (
                    "task_snapshot_digest",
                    string(
                        Work::default()
                            .digest(b"coven-task-snapshot-v1\0", &task)
                            .unwrap(),
                    ),
                ),
                ("enrolled_at", string("2024-02-29T00:00:00Z")),
            ])
            .unwrap();
            state.enrollments.push(Enrollment {
                task: id,
                id: eid,
                bindings: vec![binding],
            });
            tasks.push(task);
        }
        source.set_field("tasks", Value::Array(tasks)).unwrap();
        (doc, source, state)
    }

    fn freeze_fixture(doc: &Value, source: &Value, state: &Replay) -> Result<Value> {
        freeze(
            doc,
            source,
            state,
            FreezeIds {
                authority: &string("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                snapshot: &string("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
                graph_id: &string("cccccccccccccccccccccccccccccccc"),
                revision: &string("dddddddddddddddddddddddddddddddd"),
                predecessor: &Value::Null,
                proposed_at: &string("2024-02-29T00:00:00Z"),
            },
            &mut Work::default(),
        )
    }

    fn replace_node(value: &mut Value, index: usize, key: &str, field: Value) {
        let mut nodes = get(value, "nodes").unwrap().as_array().unwrap().to_vec();
        nodes[index].set_field(key, field).unwrap();
        value.set_field("nodes", Value::Array(nodes)).unwrap();
    }

    fn rehash_nodes(manifest: &mut Value) {
        let mut nodes = get(manifest, "nodes").unwrap().as_array().unwrap().to_vec();
        for node in &mut nodes {
            let task = get(node, "complete_task_snapshot").unwrap();
            node.set_field(
                "task_snapshot_digest",
                string(
                    Work::default()
                        .digest(b"coven-task-snapshot-v1\0", task)
                        .unwrap(),
                ),
            )
            .unwrap();
            node.set_field(
                "node_digest",
                string(node_digest(node, &mut Work::default()).unwrap()),
            )
            .unwrap();
        }
        manifest.set_field("nodes", Value::Array(nodes)).unwrap();
    }

    #[test]
    fn graph_counts_fields_types_and_lexical_topology() {
        for count in [1, 64] {
            validate_document(&document(count), &mut Work::default()).unwrap();
            let (doc, source, state) = fixture(count);
            validate_manifest(
                &freeze_fixture(&doc, &source, &state).unwrap(),
                &mut Work::default(),
            )
            .unwrap();
        }
        for count in [0, 65] {
            assert!(validate_document(&document(count), &mut Work::default()).is_err());
        }
        let mut doc = document(3);
        replace_node(&mut doc, 0, "depends_on", parse(r#"["n002"]"#));
        let before = doc.canonical().unwrap();
        validate_document(&doc, &mut Work::default()).unwrap();
        assert_eq!(
            order(get(&doc, "nodes").unwrap().as_array().unwrap()).unwrap(),
            ["n001", "n002", "n000"]
        );
        assert_eq!(before, doc.canonical().unwrap());
        for bad in [parse("true"), parse("1.0"), parse("2")] {
            let mut bad_doc = doc.clone();
            bad_doc.set_field("schema_version", bad).unwrap();
            assert!(validate_document(&bad_doc, &mut Work::default()).is_err());
        }
        for (key, bad) in [
            ("extra", Value::Null),
            ("owner", string("bad owner")),
            ("acceptance_criteria", parse("[]")),
            ("acceptance_criteria", parse(r#"[{"id":"a","text":" "}]"#)),
            (
                "acceptance_criteria",
                parse(r#"[{"id":"a","text":"x","extra":1}]"#),
            ),
            (
                "acceptance_criteria",
                parse(r#"[{"id":"a","text":"x"},{"id":"a","text":"y"}]"#),
            ),
            ("depends_on", Value::Null),
            ("depends_on", parse(r#"["n002","n002"]"#)),
            ("depends_on", parse(r#"["bad dependency"]"#)),
            ("depends_on", parse(r#"["missing"]"#)),
            ("depends_on", parse(r#"["n000"]"#)),
            ("task_id", string("n001")),
        ] {
            let mut bad_doc = doc.clone();
            replace_node(&mut bad_doc, 0, key, bad);
            assert!(
                validate_document(&bad_doc, &mut Work::default()).is_err(),
                "{key}"
            );
        }
        replace_node(&mut doc, 2, "depends_on", parse(r#"["n000"]"#));
        assert_eq!(
            validate_document(&doc, &mut Work::default()).unwrap_err().0,
            "graph_cycle"
        );
    }

    #[test]
    fn edges_256_inclusive_257_rejected() {
        let mut doc = document(64);
        let mut left = 257;
        let mut last = 0;
        for i in 0..64 {
            let count = i.min(left);
            replace_node(
                &mut doc,
                i,
                "depends_on",
                Value::Array((0..count).map(|j| string(format!("n{j:03}"))).collect()),
            );
            left -= count;
            if left == 0 {
                last = i;
                break;
            }
        }
        assert_eq!(
            validate_document(&doc, &mut Work::default()).unwrap_err().0,
            "graph_edge_limit"
        );
        let node = &get(&doc, "nodes").unwrap().as_array().unwrap()[last];
        let mut deps = get(node, "depends_on")
            .unwrap()
            .as_array()
            .unwrap()
            .to_vec();
        deps.pop();
        replace_node(&mut doc, last, "depends_on", Value::Array(deps));
        validate_document(&doc, &mut Work::default()).unwrap();
    }

    #[test]
    fn python_digest_vectors_omit_only_node_digest_and_charge_work() {
        let payload = parse(r#"{"a":1}"#);
        assert_eq!(
            node_digest(&payload, &mut Work::default()).unwrap(),
            "task-graph-node-v1:1b8028602b8f845687f8274dd5e9f2c9f2a879d2ddf9661e331ed80dd4aec3f0"
        );
        assert_eq!(
            digest(&payload, &mut Work::default()).unwrap(),
            "task-graph-manifest-v1:b1dae5d396d50b71d2207c32ba9fdd55a1d75856355cf05a426d8bd79f77b70e"
        );
        let mut with_digest = payload.clone();
        with_digest.set_field("node_digest", Value::Null).unwrap();
        assert_eq!(
            node_digest(&with_digest, &mut Work::default()).unwrap(),
            node_digest(&payload, &mut Work::default()).unwrap()
        );
        with_digest.set_field("a", parse("1.0")).unwrap();
        assert_ne!(
            node_digest(&with_digest, &mut Work::default()).unwrap(),
            node_digest(&payload, &mut Work::default()).unwrap()
        );
        assert!(node_digest(&Value::Null, &mut Work::default()).is_err());
        let mut work = Work(super::super::MAX_WORK - 7);
        digest(&payload, &mut work).unwrap();
        assert!(digest(&payload, &mut work).is_err());
    }

    #[test]
    fn document_and_manifest_exact_canonical_size_limit() {
        let mut doc = document(1);
        let length = doc.canonical().unwrap().len();
        replace_node(
            &mut doc,
            0,
            "task_id",
            string("x".repeat(MAX_BYTES - length + 4)),
        );
        assert_eq!(doc.canonical().unwrap().len(), MAX_BYTES);
        validate_document(&doc, &mut Work::default()).unwrap();
        replace_node(
            &mut doc,
            0,
            "task_id",
            string("x".repeat(MAX_BYTES - length + 5)),
        );
        assert_eq!(
            validate_document(&doc, &mut Work::default()).unwrap_err().0,
            "graph_size_limit"
        );

        let (doc, source, state) = fixture(1);
        let mut manifest = freeze_fixture(&doc, &source, &state).unwrap();
        let mut task = get(
            &get(&manifest, "nodes").unwrap().as_array().unwrap()[0],
            "complete_task_snapshot",
        )
        .unwrap()
        .clone();
        task.set_field("padding", string("")).unwrap();
        replace_node(&mut manifest, 0, "complete_task_snapshot", task.clone());
        rehash_nodes(&mut manifest);
        let padding = MAX_BYTES - manifest.canonical().unwrap().len();
        task.set_field("padding", string("p".repeat(padding)))
            .unwrap();
        replace_node(&mut manifest, 0, "complete_task_snapshot", task.clone());
        rehash_nodes(&mut manifest);
        assert_eq!(manifest.canonical().unwrap().len(), MAX_BYTES);
        validate_manifest(&manifest, &mut Work::default()).unwrap();
        task.set_field("padding", string("p".repeat(padding + 1)))
            .unwrap();
        replace_node(&mut manifest, 0, "complete_task_snapshot", task);
        rehash_nodes(&mut manifest);
        assert_eq!(manifest.canonical().unwrap().len(), MAX_BYTES + 1);
        assert_eq!(
            validate_manifest(&manifest, &mut Work::default())
                .unwrap_err()
                .0,
            "graph_size_limit"
        );
    }

    #[test]
    fn manifest_closed_shapes_identity_and_frozen_integrity() {
        let (doc, source, state) = fixture(2);
        let manifest = freeze_fixture(&doc, &source, &state).unwrap();
        for (key, bad) in [
            ("extra", Value::Null),
            ("predecessor", string("bad")),
            ("graph_id", string("A".repeat(32))),
            ("graph_revision", Value::Null),
            ("proposed_at", string("2023-02-29T00:00:00Z")),
            ("task_source_revision", string("tasks-missing:v1")),
            (
                "task_source_revision",
                string(format!(
                    "tasks-ledger:v1:{}:{}:0:{}",
                    "a".repeat(32),
                    "b".repeat(32),
                    "0".repeat(64)
                )),
            ),
        ] {
            let mut bad_manifest = manifest.clone();
            bad_manifest.set_field(key, bad).unwrap();
            assert!(
                validate_manifest(&bad_manifest, &mut Work::default()).is_err(),
                "{key}"
            );
        }
        for (key, bad) in [
            ("binding_number", parse("true")),
            ("binding_number", parse("1.0")),
            ("binding_number", parse("0")),
            ("binding_number", parse("-1")),
            ("enrollment_id", string("00000000000000000000000000000001")),
            ("owner", string("other")),
            ("task_snapshot_digest", string("0".repeat(64))),
            ("node_digest", string("task-graph-node-v1:bad")),
            (
                "complete_task_snapshot",
                parse(r#"{"id":"wrong","title":"x","status":"open","owner":"worker"}"#),
            ),
        ] {
            let mut bad_manifest = manifest.clone();
            replace_node(&mut bad_manifest, 0, key, bad);
            assert!(
                validate_manifest(&bad_manifest, &mut Work::default()).is_err(),
                "{key}"
            );
        }
        let mut ledger = manifest.clone();
        ledger.set_field("task_authority_identity", parse(r#"{"kind":"task-ledger-v1","workspace_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","epoch_id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","path":"state/task-ledger"}"#)).unwrap();
        ledger
            .set_field(
                "task_source_revision",
                string(format!(
                    "tasks-ledger:v1:{}:{}:0:{}",
                    "a".repeat(32),
                    "b".repeat(32),
                    "0".repeat(64)
                )),
            )
            .unwrap();
        // Standalone validation does not hash selected tasks as a complete ledger.
        validate_manifest(&ledger, &mut Work::default()).unwrap();
        let mut bad_identity = get(&ledger, "task_authority_identity").unwrap().clone();
        bad_identity.set_field("path", string("other")).unwrap();
        ledger
            .set_field("task_authority_identity", bad_identity)
            .unwrap();
        assert!(validate_manifest(&ledger, &mut Work::default()).is_err());
    }

    #[test]
    fn refreeze_uses_retained_source_not_enrollment_snapshot() {
        let (doc, mut source, mut state) = fixture(1);
        let original = freeze_fixture(&doc, &source, &state).unwrap();
        // An older enrollment need not have an owner at all.
        let binding = &mut state.enrollments[0].bindings[0];
        let mut old_task = get(binding, "complete_task_snapshot").unwrap().clone();
        old_task.set_field("owner", Value::Null).unwrap();
        binding
            .set_field("complete_task_snapshot", old_task.clone())
            .unwrap();
        binding
            .set_field(
                "task_snapshot_digest",
                string(
                    Work::default()
                        .digest(b"coven-task-snapshot-v1\0", &old_task)
                        .unwrap(),
                ),
            )
            .unwrap();
        let mut tasks = get(&source, "tasks").unwrap().as_array().unwrap().to_vec();
        tasks[0]
            .set_field("title", string("Retained newer title"))
            .unwrap();
        source
            .set_field("tasks", Value::Array(tasks.clone()))
            .unwrap();
        let frozen = freeze_fixture(&doc, &source, &state).unwrap();
        let node = &get(&frozen, "nodes").unwrap().as_array().unwrap()[0];
        assert_eq!(
            get(node, "complete_task_snapshot")
                .unwrap()
                .canonical()
                .unwrap(),
            tasks[0].canonical().unwrap()
        );
        assert_ne!(frozen.canonical().unwrap(), original.canonical().unwrap());
        assert_eq!(
            text(
                get(
                    &match_source(&frozen, &source, &state, &mut Work::default()).unwrap(),
                    "status"
                )
                .unwrap()
            )
            .unwrap(),
            "current"
        );
        source.set_field("roster", parse(r#"["other"]"#)).unwrap();
        assert!(freeze_fixture(&doc, &source, &state).is_err());
        state.enrollments[0].bindings[0]
            .set_field("enrolled_at", string("bad"))
            .unwrap();
        assert!(match_source(&frozen, &source, &state, &mut Work::default()).is_err());
    }

    #[test]
    fn match_reason_order_global_priority_and_independent_revision() {
        let (mut doc, mut source, state) = fixture(2);
        let mut nodes = get(&doc, "nodes").unwrap().as_array().unwrap().to_vec();
        nodes.reverse();
        doc.set_field("nodes", Value::Array(nodes)).unwrap();
        let manifest = freeze_fixture(&doc, &source, &state).unwrap();
        source
            .set_field(
                "revision",
                string(format!("tasks-sha256:{}", "1".repeat(64))),
            )
            .unwrap();
        let current = match_source(&manifest, &source, &state, &mut Work::default()).unwrap();
        assert_eq!(
            current.canonical().unwrap(),
            br#"{"reasons":[],"status":"current","whole_revision_changed":true}"#
        );
        let mut tasks = get(&source, "tasks").unwrap().as_array().unwrap().to_vec();
        tasks[0]
            .set_field(
                "extension",
                parse(r#"[1,123456789012345678901234567890,-0.0,"\u03bb"]"#),
            )
            .unwrap();
        source.set_field("tasks", Value::Array(tasks)).unwrap();
        let changed = match_source(&manifest, &source, &state, &mut Work::default()).unwrap();
        assert_eq!(
            text(get(&changed, "status").unwrap()).unwrap(),
            "selected_inputs_changed"
        );
        let mut changed_identity = get(&source, "identity").unwrap().clone();
        changed_identity
            .set_field("source_id", string("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"))
            .unwrap();
        source.set_field("identity", changed_identity).unwrap();
        source.set_field("tasks", Value::Array(vec![])).unwrap();
        source.set_field("roster", parse(r#"["other"]"#)).unwrap();
        let result = match_source(&manifest, &source, &state, &mut Work::default()).unwrap();
        assert_eq!(
            text(get(&result, "status").unwrap()).unwrap(),
            "source_authority_changed"
        );
        let reasons = get(&result, "reasons").unwrap().as_array().unwrap();
        assert_eq!(reasons.len(), 8);
        for (i, reason) in reasons.iter().enumerate() {
            assert_eq!(
                text(get(reason, "task_id").unwrap()).unwrap(),
                if i < 4 { "n000" } else { "n001" }
            );
            assert_eq!(
                text(get(reason, "reason").unwrap()).unwrap(),
                [
                    "source_authority_changed",
                    "selected_inputs_changed",
                    "enrollment_changed",
                    "owner_missing"
                ][i % 4]
            );
        }
        // Highest priority can occur on a later lexical node.
        let (doc, mut source, mut state) = fixture(2);
        let manifest = freeze_fixture(&doc, &source, &state).unwrap();
        state.enrollments.remove(0);
        let mut tasks = get(&source, "tasks").unwrap().as_array().unwrap().to_vec();
        tasks[1].set_field("title", string("changed")).unwrap();
        source.set_field("tasks", Value::Array(tasks)).unwrap();
        let result = match_source(&manifest, &source, &state, &mut Work::default()).unwrap();
        assert_eq!(
            text(get(&result, "status").unwrap()).unwrap(),
            "selected_inputs_changed"
        );
        assert!(matches!(
            get(&result, "whole_revision_changed").unwrap(),
            Value::Bool(false)
        ));
    }

    #[test]
    fn full_retained_source_checks_ledger_digest_and_snapshot_id() {
        let (doc, mut source, mut state) = fixture(2);
        let identity = parse(
            r#"{"kind":"task-ledger-v1","workspace_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","epoch_id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","path":"state/task-ledger"}"#,
        );
        let hash = Work::default()
            .digest(b"", get(&source, "tasks").unwrap())
            .unwrap();
        let revision = string(format!(
            "tasks-ledger:v1:{}:{}:0:{hash}",
            "a".repeat(32),
            "b".repeat(32)
        ));
        source.set_field("identity", identity.clone()).unwrap();
        source.set_field("revision", revision.clone()).unwrap();
        for e in &mut state.enrollments {
            e.bindings[0]
                .set_field("task_authority_identity", identity.clone())
                .unwrap();
            e.bindings[0]
                .set_field("task_source_revision", revision.clone())
                .unwrap();
        }
        let manifest = freeze_fixture(&doc, &source, &state).unwrap();
        let result = match_source(&manifest, &source, &state, &mut Work::default()).unwrap();
        assert_eq!(text(get(&result, "status").unwrap()).unwrap(), "current");
        // Full source validation hashes all tasks; manifest identity checks do not.
        source.set_field("tasks", Value::Array(vec![])).unwrap();
        assert_eq!(
            match_source(&manifest, &source, &state, &mut Work::default())
                .unwrap_err()
                .0,
            "execution_ledger_digest_mismatch"
        );
        assert!(freeze_fixture(&doc, &source, &state).is_err());

        let (doc, mut source, state) = fixture(1);
        let manifest = freeze_fixture(&doc, &source, &state).unwrap();
        let mut identity = get(&source, "identity").unwrap().clone();
        identity
            .set_field("source_id", string("not-a-uuid"))
            .unwrap();
        source.set_field("identity", identity).unwrap();
        assert!(match_source(&manifest, &source, &state, &mut Work::default()).is_err());
        assert!(freeze_fixture(&doc, &source, &state).is_err());
    }
}
