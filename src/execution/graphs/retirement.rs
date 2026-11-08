//! Frozen historical calls only; no interpreter, copied validator or live oracle.
use super::super::{Enrollment, Profile, values};
use super::*;
use serde_json::Value as Json;
use sha2::{Digest, Sha256};

const CORPUS: &str = include_str!("../../../tests/retirement/fixtures/task_graphs.json");
const PROVENANCE: &str =
    include_str!("../../../tests/retirement/fixtures/task_graphs_provenance.json");
const REVIEWS: &str = include_str!("../../../tests/retirement/reviews/task_graphs.json");

fn sha(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}

struct Corpus {
    metadata: Json,
    documents: Vec<Value>,
}

impl Corpus {
    fn load() -> Self {
        let metadata: Json = serde_json::from_str(CORPUS).unwrap();
        let provenance: Json = serde_json::from_str(PROVENANCE).unwrap();
        assert_eq!(sha(CORPUS.as_bytes()), provenance["corpus_sha256"]);
        let documents = metadata["documents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|document| {
                let mut raw = String::new();
                for part in document["parts"].as_array().unwrap() {
                    if let Some(literal) = part.as_str() {
                        raw.push_str(literal);
                    } else {
                        let count = part["count"].as_u64().unwrap() as usize;
                        assert!(count <= 2 * MAX_BYTES);
                        let character = part["repeat"].as_str().unwrap();
                        assert_eq!(character.chars().count(), 1);
                        raw.push_str(&character.repeat(count));
                    }
                }
                assert_eq!(raw.len() as u64, document["bytes"].as_u64().unwrap());
                assert_eq!(sha(raw.as_bytes()), document["sha256"]);
                // Only the metadata goes through serde. Native Authority parsing
                // sees the original raw numeric lexemes and unknown Unicode.
                if raw.starts_with('"') {
                    // Captured canonical byte outputs can exceed the native
                    // input string limit. Decode only their JSON string wrapper;
                    // never parse embedded JSON through serde.
                    string(serde_json::from_str::<String>(&raw).unwrap())
                } else {
                    values::parse(raw.as_bytes(), Profile::Authority).unwrap()
                }
            })
            .collect();
        Self {
            metadata,
            documents,
        }
    }

    fn value(&self, index: &Json) -> &Value {
        &self.documents[index.as_u64().unwrap() as usize]
    }

    fn case(&self, id: &str) -> &Json {
        self.metadata["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"] == id)
            .unwrap()
    }

    fn run(&self, case: &Json) -> Option<Result<Value>> {
        let args = self.value(&case["args"]).as_array().unwrap();
        let options = self.value(&case["options"]);
        let mut work = Work::default();
        Some(match case["operation"].as_str().unwrap() {
            "canonical" => work
                .encode(&args[0])
                .map(|bytes| string(String::from_utf8(bytes).unwrap())),
            "digest" => digest(&args[0], &mut work).map(string),
            "node_digest" => node_digest(&args[0], &mut work).map(string),
            "validate_document" => validate_document(&args[0], &mut work).map(|()| args[0].clone()),
            "validate_manifest" => validate_manifest(&args[0], &mut work).map(|()| args[0].clone()),
            "topology" => {
                let result = if text(get(&args[0], "kind").unwrap()).unwrap() == "task-graph" {
                    validate_document(&args[0], &mut work)
                } else {
                    validate_manifest(&args[0], &mut work)
                };
                result.and_then(|()| {
                    order(get(&args[0], "nodes")?.as_array().unwrap())
                        .map(|ids| Value::Array(ids.into_iter().map(string).collect()))
                })
            }
            "freeze" => {
                let state = replay(&args[2])?;
                freeze(
                    &args[0],
                    &args[1],
                    &state,
                    FreezeIds {
                        authority: get(options, "authority_id").unwrap(),
                        snapshot: get(options, "snapshot_source_id").unwrap(),
                        graph_id: get(options, "graph_id").unwrap(),
                        revision: get(options, "revision_id").unwrap(),
                        predecessor: get(options, "predecessor").unwrap(),
                        proposed_at: get(options, "proposed_at").unwrap(),
                    },
                    &mut work,
                )
            }
            "match_source" => {
                let state = replay(&args[2])?;
                match_source(&args[0], &args[1], &state, &mut work)
            }
            other => panic!("unknown operation {other}"),
        })
    }
}

fn replay(map: &Value) -> Option<Replay> {
    let mut state = Replay::default();
    for (key, entry) in map.as_object().unwrap().entries() {
        let task = text(get(entry, "task_id").unwrap()).unwrap();
        // Replay has no independent Python map key. Retain these two calls as
        // unresolved typed-boundary obligations, not an adapter-side refusal.
        if key != task {
            return None;
        }
        state.enrollments.push(Enrollment {
            task: task.to_owned(),
            id: text(get(entry, "enrollment_id").unwrap())
                .unwrap()
                .to_owned(),
            bindings: get(entry, "bindings").unwrap().as_array().unwrap().to_vec(),
        });
    }
    Some(state)
}

fn parse(raw: &str) -> Value {
    values::parse(raw.as_bytes(), Profile::Authority).unwrap()
}

fn replace_node(value: &mut Value, index: usize, key: &str, field: Value) {
    let mut nodes = get(value, "nodes").unwrap().as_array().unwrap().to_vec();
    nodes[index].set_field(key, field).unwrap();
    value.set_field("nodes", Value::Array(nodes)).unwrap();
}

fn document(count: usize) -> Value {
    let template = parse(
        r#"{"task_id":"x","owner":"Worker","acceptance_criteria":[{"id":"check","text":"Evidence"}],"depends_on":[]}"#,
    );
    object(vec![
        ("schema_version", integer(1).unwrap()),
        ("kind", string("task-graph")),
        (
            "nodes",
            Value::Array(
                (0..count)
                    .map(|i| {
                        let mut node = template.clone();
                        node.set_field("task_id", string(format!("n{i:03}")))
                            .unwrap();
                        node
                    })
                    .collect(),
            ),
        ),
    ])
    .unwrap()
}

fn rehash(manifest: &mut Value) {
    let mut nodes = get(manifest, "nodes").unwrap().as_array().unwrap().to_vec();
    for node in &mut nodes {
        let hash = Work::default()
            .digest(
                b"coven-task-snapshot-v1\0",
                get(node, "complete_task_snapshot").unwrap(),
            )
            .unwrap();
        node.set_field("task_snapshot_digest", string(hash))
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
fn review_fragment_accounts_for_every_original() {
    let fragment: Json = serde_json::from_str(REVIEWS).unwrap();
    let provenance: Json = serde_json::from_str(PROVENANCE).unwrap();
    assert_eq!(fragment["fixture"]["sha256"], sha(CORPUS.as_bytes()));
    assert_eq!(fragment["provenance"]["sha256"], sha(PROVENANCE.as_bytes()));
    let reviews = fragment["reviews"].as_object().unwrap();
    let methods = provenance["methods"].as_object().unwrap();
    assert_eq!(reviews.len(), 18);
    assert_eq!(
        reviews.keys().collect::<Vec<_>>(),
        methods.keys().collect::<Vec<_>>()
    );
    let mut mapped = 0;
    for (id, review) in reviews {
        assert_eq!(review["cases"], methods[id]["cases"], "{id}");
        assert!(!review["assertion_review"].as_str().unwrap().is_empty());
        if review["status"] == "mapped" {
            mapped += 1;
            assert_eq!(review["category_suggestion"], "A");
            assert!(!review["native_tests"].as_array().unwrap().is_empty());
            assert_eq!(review["fixtures"][0], fragment["fixture"]);
        } else {
            assert_eq!(review["status"], "unresolved");
        }
    }
    assert_eq!(mapped, 14);
}

#[test]
fn frozen_original_calls_and_all_subcases() {
    let corpus = Corpus::load();
    let provenance: Json = serde_json::from_str(PROVENANCE).unwrap();
    let cases = corpus.metadata["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 142);
    assert_eq!(corpus.documents.len(), 161);
    let mut accounted = BTreeSet::new();
    let mut captured_methods = 0;
    for method in provenance["methods"].as_object().unwrap().values() {
        let ids = method["cases"].as_array().unwrap();
        if !ids.is_empty() {
            captured_methods += 1;
        }
        for id in ids {
            assert!(accounted.insert(id.as_str().unwrap()));
        }
    }
    assert_eq!(captured_methods, 15);
    let mut typed = Vec::new();
    let mut direct = 0;
    for case in cases {
        let id = case["id"].as_str().unwrap();
        assert!(accounted.remove(id));
        let before_args = corpus.value(&case["args"]).canonical().unwrap();
        let before_options = corpus.value(&case["options"]).canonical().unwrap();
        if let Some(result) = corpus.run(case) {
            direct += 1;
            assert_eq!(
                result.is_ok(),
                case["ok"].as_bool().unwrap(),
                "{id}: {result:?}"
            );
            if let Ok(actual) = result {
                let expected = corpus.value(&case["output"]);
                if case["operation"] == "canonical" {
                    assert_eq!(
                        actual.as_str().unwrap().as_bytes(),
                        expected.as_str().unwrap().as_bytes(),
                        "{id}"
                    );
                } else {
                    assert_eq!(
                        actual.canonical().unwrap(),
                        expected.canonical().unwrap(),
                        "{id}"
                    );
                }
            }
        } else {
            typed.push(id);
        }
        assert_eq!(
            before_args,
            corpus.value(&case["args"]).canonical().unwrap(),
            "{id}"
        );
        assert_eq!(
            before_options,
            corpus.value(&case["options"]).canonical().unwrap(),
            "{id}"
        );
    }
    assert!(accounted.is_empty());
    assert_eq!(direct, 140);
    assert_eq!(typed, ["g0076", "g0077"]);
}

#[test]
fn independent_domains_order_numeric_distinction_and_mutations() {
    let corpus = Corpus::load();
    let case = corpus.case("g0004");
    let manifest = corpus.run(case).unwrap().unwrap();
    let before = manifest.canonical().unwrap();
    let mut domain = b"coven-task-graph-manifest-v1\0".to_vec();
    domain.extend(&before);
    let manifest_hash = digest(&manifest, &mut Work::default()).unwrap();
    assert_eq!(
        manifest_hash,
        format!("task-graph-manifest-v1:{}", sha(&domain))
    );
    assert_ne!(
        manifest_hash,
        format!("task-graph-manifest-v1:{}", sha(&before))
    );
    let mut reversed_keys = Value::object(
        manifest
            .as_object()
            .unwrap()
            .entries()
            .iter()
            .rev()
            .cloned()
            .collect(),
    )
    .unwrap();
    assert_eq!(
        manifest_hash,
        digest(&reversed_keys, &mut Work::default()).unwrap()
    );
    let mut nodes = get(&manifest, "nodes")
        .unwrap()
        .as_array()
        .unwrap()
        .to_vec();
    assert_eq!(order(&nodes).unwrap(), ["A", "B", "C"]);
    assert_eq!(
        nodes
            .iter()
            .map(|node| text(get(node, "task_id").unwrap()).unwrap())
            .collect::<Vec<_>>(),
        ["B", "A", "C"]
    );
    let node = &nodes[0];
    let payload = Value::object(
        node.as_object()
            .unwrap()
            .entries()
            .iter()
            .filter(|(key, _)| key != "node_digest")
            .cloned()
            .collect(),
    )
    .unwrap();
    let mut domain = b"coven-task-graph-node-v1\0".to_vec();
    domain.extend(payload.canonical().unwrap());
    assert_eq!(
        node_digest(node, &mut Work::default()).unwrap(),
        format!("task-graph-node-v1:{}", sha(&domain))
    );
    let mut ignored = node.clone();
    ignored.set_field("node_digest", Value::Null).unwrap();
    assert_eq!(
        node_digest(node, &mut Work::default()).unwrap(),
        node_digest(&ignored, &mut Work::default()).unwrap()
    );
    let snapshot = get(node, "complete_task_snapshot").unwrap();
    let mut domain = b"coven-task-snapshot-v1\0".to_vec();
    domain.extend(snapshot.canonical().unwrap());
    assert_eq!(
        text(get(node, "task_snapshot_digest").unwrap()).unwrap(),
        sha(&domain)
    );
    let mut changed = snapshot.clone();
    let mut unknown = get(&changed, "unknown").unwrap().clone();
    let mut numbers = get(&unknown, "values")
        .unwrap()
        .as_array()
        .unwrap()
        .to_vec();
    assert!(matches!(numbers[2], Value::Number(Number::Float(_))));
    numbers[2] = integer(1).unwrap();
    unknown.set_field("values", Value::Array(numbers)).unwrap();
    changed.set_field("unknown", unknown).unwrap();
    assert_ne!(snapshot.canonical().unwrap(), changed.canonical().unwrap());
    assert_ne!(
        Work::default()
            .digest(b"coven-task-snapshot-v1\0", snapshot)
            .unwrap(),
        Work::default()
            .digest(b"coven-task-snapshot-v1\0", &changed)
            .unwrap()
    );
    assert!(
        String::from_utf8(before.clone())
            .unwrap()
            .contains("\u{2003}caf\u{e9}")
    );
    nodes.reverse();
    reversed_keys
        .set_field("nodes", Value::Array(nodes))
        .unwrap();
    assert_ne!(
        manifest_hash,
        digest(&reversed_keys, &mut Work::default()).unwrap()
    );
    // Dependency order is persistent data even when the topology is unchanged.
    let mut left = document(3);
    replace_node(&mut left, 2, "depends_on", parse(r#"["n000","n001"]"#));
    let mut right = left.clone();
    replace_node(&mut right, 2, "depends_on", parse(r#"["n001","n000"]"#));
    assert_eq!(
        order(get(&left, "nodes").unwrap().as_array().unwrap()).unwrap(),
        order(get(&right, "nodes").unwrap().as_array().unwrap()).unwrap()
    );
    assert_ne!(
        digest(&left, &mut Work::default()).unwrap(),
        digest(&right, &mut Work::default()).unwrap()
    );
    assert_eq!(before, manifest.canonical().unwrap());
}

#[test]
fn independent_owned_results_and_nested_mutation_isolation() {
    let corpus = Corpus::load();
    let case = corpus.case("g0004");
    let inputs = corpus.value(&case["args"]);
    let original = inputs.canonical().unwrap();
    let first = corpus.run(case).unwrap().unwrap();
    let mut checked = first.clone();
    validate_manifest(&checked, &mut Work::default()).unwrap();
    let mut nodes = get(&checked, "nodes").unwrap().as_array().unwrap().to_vec();
    let mut task = get(&nodes[0], "complete_task_snapshot").unwrap().clone();
    let mut unknown = get(&task, "unknown").unwrap().clone();
    let mut list = get(&unknown, "values")
        .unwrap()
        .as_array()
        .unwrap()
        .to_vec();
    list.push(string("mutation"));
    unknown.set_field("values", Value::Array(list)).unwrap();
    task.set_field("unknown", unknown).unwrap();
    nodes[0].set_field("complete_task_snapshot", task).unwrap();
    nodes[0]
        .set_field("acceptance_criteria", Value::Array(vec![]))
        .unwrap();
    nodes[0]
        .set_field("depends_on", Value::Array(vec![]))
        .unwrap();
    checked.set_field("nodes", Value::Array(nodes)).unwrap();
    assert_ne!(checked.canonical().unwrap(), first.canonical().unwrap());
    let mut document = inputs.as_array().unwrap()[0].clone();
    validate_document(&document, &mut Work::default()).unwrap();
    replace_node(&mut document, 0, "depends_on", Value::Array(vec![]));
    assert_ne!(
        document.canonical().unwrap(),
        inputs.as_array().unwrap()[0].canonical().unwrap()
    );
    assert_eq!(original, inputs.canonical().unwrap());
    assert_eq!(
        first.canonical().unwrap(),
        corpus.run(case).unwrap().unwrap().canonical().unwrap()
    );
}

#[test]
fn independent_closed_fields_and_exact_integer_types() {
    let corpus = Corpus::load();
    let manifest = corpus.run(corpus.case("g0004")).unwrap().unwrap();
    let doc = document(1);
    for (value, frozen) in [(&doc, false), (&manifest, true)] {
        let validate = |value: &Value| {
            if frozen {
                validate_manifest(value, &mut Work::default())
            } else {
                validate_document(value, &mut Work::default())
            }
        };
        validate(value).unwrap();
        for invalid in [Value::Bool(true), parse("1.0"), integer(2).unwrap()] {
            let mut changed = value.clone();
            changed.set_field("schema_version", invalid).unwrap();
            assert!(validate(&changed).is_err());
        }
        let mut changed = value.clone();
        changed.set_field("extra", Value::Null).unwrap();
        assert!(validate(&changed).is_err());
        let mut changed = value.clone();
        replace_node(&mut changed, 0, "extra", Value::Null);
        assert!(validate(&changed).is_err());
    }
    for invalid in [Value::Bool(true), parse("1.0"), integer(0).unwrap()] {
        let mut changed = manifest.clone();
        replace_node(&mut changed, 0, "binding_number", invalid);
        rehash(&mut changed);
        assert_eq!(
            validate_manifest(&changed, &mut Work::default())
                .unwrap_err()
                .0,
            "invalid_graph_binding_number"
        );
    }
}

#[test]
fn independent_drift_reason_order_and_global_priority() {
    let corpus = Corpus::load();
    let case = corpus.case("g0004");
    let args = corpus.value(&case["args"]).as_array().unwrap();
    let manifest = corpus.run(case).unwrap().unwrap();
    let original = args[1].canonical().unwrap();
    let mut source = args[1].clone();
    let mut state = replay(&args[2]).unwrap();
    // A later lexical task has a higher-priority reason than missing enrollment A.
    state.enrollments.retain(|entry| entry.task != "A");
    let mut tasks = get(&source, "tasks").unwrap().as_array().unwrap().to_vec();
    tasks[1].set_field("title", string("changed B")).unwrap();
    source.set_field("tasks", Value::Array(tasks)).unwrap();
    let result = match_source(&manifest, &source, &state, &mut Work::default()).unwrap();
    assert_eq!(result.canonical().unwrap(), br#"{"reasons":[{"reason":"enrollment_changed","task_id":"A"},{"reason":"selected_inputs_changed","task_id":"B"}],"status":"selected_inputs_changed","whole_revision_changed":false}"#);
    let mut identity = get(&source, "identity").unwrap().clone();
    identity
        .set_field("source_id", string("f".repeat(32)))
        .unwrap();
    source.set_field("identity", identity).unwrap();
    source.set_field("tasks", Value::Array(vec![])).unwrap();
    source
        .set_field("roster", Value::Array(vec![string("other")]))
        .unwrap();
    let result = match_source(&manifest, &source, &state, &mut Work::default()).unwrap();
    assert_eq!(
        text(get(&result, "status").unwrap()).unwrap(),
        "source_authority_changed"
    );
    let reasons = get(&result, "reasons").unwrap().as_array().unwrap();
    assert_eq!(reasons.len(), 12);
    for (i, reason) in reasons.iter().enumerate() {
        assert_eq!(
            text(get(reason, "task_id").unwrap()).unwrap(),
            ["A", "B", "C"][i / 4]
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
    assert_eq!(original, args[1].canonical().unwrap());
    assert_eq!(
        manifest.canonical().unwrap(),
        corpus.run(case).unwrap().unwrap().canonical().unwrap()
    );
}

#[test]
fn independent_inclusive_boundaries_document_and_manifest() {
    // These are independent constructions, not expected values for the corpus.
    let mut doc = document(64);
    let mut remaining = 256;
    for i in 0..64 {
        let count = i.min(remaining);
        replace_node(
            &mut doc,
            i,
            "depends_on",
            Value::Array((0..count).map(|j| string(format!("n{j:03}"))).collect()),
        );
        remaining -= count;
    }
    assert_eq!(remaining, 0);
    validate_document(&doc, &mut Work::default()).unwrap();
    let corpus = Corpus::load();
    let mut manifest = corpus.run(corpus.case("g0004")).unwrap().unwrap();
    let template = get(&manifest, "nodes").unwrap().as_array().unwrap()[0].clone();
    let nodes = get(&doc, "nodes")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let mut frozen = template.clone();
            let id = get(node, "task_id").unwrap();
            frozen.set_field("task_id", id.clone()).unwrap();
            frozen
                .set_field("depends_on", get(node, "depends_on").unwrap().clone())
                .unwrap();
            frozen
                .set_field("enrollment_id", string(format!("{i:032x}")))
                .unwrap();
            let mut task = get(&frozen, "complete_task_snapshot").unwrap().clone();
            task.set_field("id", id.clone()).unwrap();
            frozen.set_field("complete_task_snapshot", task).unwrap();
            frozen
        })
        .collect();
    manifest.set_field("nodes", Value::Array(nodes)).unwrap();
    rehash(&mut manifest);
    validate_manifest(&manifest, &mut Work::default()).unwrap();
    replace_node(
        &mut doc,
        63,
        "depends_on",
        Value::Array(vec![string("n000")]),
    );
    assert_eq!(
        validate_document(&doc, &mut Work::default()).unwrap_err().0,
        "graph_edge_limit"
    );
    replace_node(
        &mut manifest,
        63,
        "depends_on",
        Value::Array(vec![string("n000")]),
    );
    rehash(&mut manifest);
    assert_eq!(
        validate_manifest(&manifest, &mut Work::default())
            .unwrap_err()
            .0,
        "graph_edge_limit"
    );
    let nodes = get(&manifest, "nodes")
        .unwrap()
        .as_array()
        .unwrap()
        .to_vec();
    let mut over = nodes;
    over.push(over[0].clone());
    manifest.set_field("nodes", Value::Array(over)).unwrap();
    assert_eq!(
        validate_manifest(&manifest, &mut Work::default())
            .unwrap_err()
            .0,
        "invalid_graph_node_count"
    );

    for count in [0, 65] {
        assert!(validate_document(&document(count), &mut Work::default()).is_err());
    }
    let mut doc = document(1);
    let base = doc.canonical().unwrap().len();
    replace_node(
        &mut doc,
        0,
        "task_id",
        string("x".repeat(MAX_BYTES - base + 4)),
    );
    assert_eq!(doc.canonical().unwrap().len(), MAX_BYTES);
    validate_document(&doc, &mut Work::default()).unwrap();
    replace_node(
        &mut doc,
        0,
        "task_id",
        string("x".repeat(MAX_BYTES - base + 5)),
    );
    assert_eq!(doc.canonical().unwrap().len(), MAX_BYTES + 1);
    assert_eq!(
        validate_document(&doc, &mut Work::default()).unwrap_err().0,
        "graph_size_limit"
    );
    let mut manifest = corpus.run(corpus.case("g0004")).unwrap().unwrap();
    let mut task = get(
        &get(&manifest, "nodes").unwrap().as_array().unwrap()[0],
        "complete_task_snapshot",
    )
    .unwrap()
    .clone();
    task.set_field("padding", string("")).unwrap();
    replace_node(&mut manifest, 0, "complete_task_snapshot", task.clone());
    rehash(&mut manifest);
    let padding = MAX_BYTES - manifest.canonical().unwrap().len();
    for extra in [0, 1] {
        task.set_field("padding", string("x".repeat(padding + extra)))
            .unwrap();
        replace_node(&mut manifest, 0, "complete_task_snapshot", task.clone());
        rehash(&mut manifest);
        assert_eq!(manifest.canonical().unwrap().len(), MAX_BYTES + extra);
        assert_eq!(
            validate_manifest(&manifest, &mut Work::default()).is_ok(),
            extra == 0
        );
    }
}
