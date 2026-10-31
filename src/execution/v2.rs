//! Pure, bounded replay of retained v2 preparation authority.
use super::*;
use std::collections::BTreeMap;

struct Graphs {
    id: Value,
    head: Value,
    revisions: BTreeMap<String, Value>,
    managed: HashSet<String>,
    decisions: HashSet<String>,
}

fn revision(value: &Value, schema: usize) -> Result<()> {
    let parts: Vec<_> = text(value)?.split(':').collect();
    require(
        parts.len() == 4
            && parts[0] == format!("execution-control-v{schema}")
            && hex(parts[1], 32)
            && decimal(parts[2], false)
            && hex(parts[3], 64),
        "invalid_execution_revision",
    )
}

fn binding<'a>(value: &'a Value, work: &mut Work) -> Result<&'a str> {
    fields(
        value,
        "schema_version operation parameters expected_execution_revision expected_tasks_revision",
    )?;
    require(
        exact_integer(get(value, "schema_version")?, 2),
        "unsupported_execution_version",
    )?;
    let op = text(get(value, "operation")?)?;
    revision(
        get(value, "expected_execution_revision")?,
        if op == "upgrade" { 1 } else { 2 },
    )?;
    let params = get(value, "parameters")?;
    fields(
        params,
        match op {
            "upgrade" => "",
            "enroll" => "task_id rebind_source",
            "prepare" => "task_id intent",
            "cancel" => "attempt_id reason",
            "graph_propose" => "document",
            "graph_decide" => "graph_revision decision reason expected_graph_digest",
            "prepare_graph" => "task_id graph_revision",
            _ => return Err(Error("unsupported_execution_operation")),
        },
    )?;
    let mut needs_source = matches!(op, "enroll" | "prepare" | "graph_propose" | "prepare_graph");
    match op {
        "enroll" | "prepare" | "prepare_graph" => {
            identifier(get(params, "task_id")?)?;
            if op == "enroll" {
                require(
                    matches!(get(params, "rebind_source")?, Value::Bool(_)),
                    "invalid_execution_rebind_flag",
                )?;
            } else if op == "prepare" {
                let explicit = get(params, "intent")?;
                intent(explicit, work)?;
                require(
                    text(get(explicit, "task_id")?)? == text(get(params, "task_id")?)?,
                    "execution_intent_task_mismatch",
                )?;
            } else {
                uuid(get(params, "graph_revision")?)?;
            }
        }
        "cancel" => {
            uuid(get(params, "attempt_id")?)?;
            nonblank(get(params, "reason")?, 16384)?;
        }
        "graph_propose" => graphs::validate_document(get(params, "document")?, work)?,
        "graph_decide" => {
            uuid(get(params, "graph_revision")?)?;
            let decision = text(get(params, "decision")?)?;
            require(
                matches!(decision, "approve" | "changes-requested"),
                "invalid_execution_graph_decision",
            )?;
            nonblank(get(params, "reason")?, 16384)?;
            let digest = text(get(params, "expected_graph_digest")?)?;
            require(
                digest
                    .strip_prefix("task-graph-manifest-v1:")
                    .is_some_and(|s| hex(s, 64)),
                "invalid_execution_graph_digest",
            )?;
            needs_source = decision == "approve";
        }
        _ => {}
    }
    if needs_source {
        task_revision(get(value, "expected_tasks_revision")?)?;
    } else {
        require(
            matches!(get(value, "expected_tasks_revision")?, Value::Null),
            "execution_operation_depends_on_tasks",
        )?;
    }
    Ok(op)
}

fn checked_source<'a>(
    control: &Value,
    binding: &Value,
    effect: &'a Value,
    work: &mut Work,
) -> Result<&'a Value> {
    let s = get(effect, "source")?;
    source(s, get(control, "snapshot_source_id")?, work)?;
    require(
        text(get(s, "revision")?)? == text(get(binding, "expected_tasks_revision")?)?,
        "execution_tasks_revision_conflict",
    )?;
    Ok(s)
}

impl Graphs {
    fn head(&self, revision: &Value) -> Result<&Value> {
        require(
            !matches!(self.head, Value::Null) && text(revision)? == text(&self.head)?,
            "execution_graph_not_head",
        )?;
        self.revisions
            .get(text(revision)?)
            .ok_or(Error("execution_graph_not_head"))
    }

    fn no_prepared(&self, state: &Replay, selected: &HashSet<String>) -> Result<()> {
        require(
            !state.attempts.iter().any(|a| {
                a.active() && (self.managed.contains(&a.task) || selected.contains(&a.task))
            }),
            "execution_prepared_blocks_graph",
        )
    }

    fn current(
        &self,
        graph: &Value,
        source: &Value,
        state: &Replay,
        work: &mut Work,
    ) -> Result<Value> {
        let matched = graphs::match_source(get(graph, "manifest")?, source, state, work)?;
        require(
            text(get(&matched, "status")?)? == "current",
            "execution_graph_source_not_current",
        )?;
        Ok(matched)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Keep checked history inputs and the shared state/work budget explicit"
    )]
    fn apply(
        &mut self,
        control: &Value,
        state: &mut Replay,
        binding: &Value,
        op: &str,
        effect: &Value,
        generation: usize,
        work: &mut Work,
    ) -> Result<Value> {
        if matches!(op, "enroll" | "cancel") {
            return state.apply(control, binding, op, effect, generation, work);
        }
        let params = get(binding, "parameters")?;
        let mut result = object(vec![
            ("operation", string(op)),
            ("generation", integer(generation)?),
            (
                "execution_authority_id",
                get(control, "execution_authority_id")?.clone(),
            ),
        ])?;
        match op {
            "upgrade" => {
                require(
                    work.equal(effect, get(control, "upgrade")?)?,
                    "execution_upgrade_effect_mismatch",
                )?;
                for key in ["upgrade_id", "backup_path", "source_revision"] {
                    result.set_field(key, get(effect, key)?.clone())?;
                }
            }
            "graph_propose" => {
                fields(effect, "source manifest")?;
                let s = checked_source(control, binding, effect, work)?;
                let manifest = get(effect, "manifest")?;
                graphs::validate_manifest(manifest, work)?;
                let id = if matches!(self.id, Value::Null) {
                    get(manifest, "graph_id")?
                } else {
                    &self.id
                };
                let expected = graphs::freeze(
                    get(params, "document")?,
                    s,
                    state,
                    graphs::FreezeIds {
                        authority: get(control, "execution_authority_id")?,
                        snapshot: get(control, "snapshot_source_id")?,
                        graph_id: id,
                        revision: get(manifest, "graph_revision")?,
                        predecessor: &self.head,
                        proposed_at: get(manifest, "proposed_at")?,
                    },
                    work,
                )?;
                require(
                    work.equal(manifest, &expected)?,
                    "execution_graph_manifest_mismatch",
                )?;
                let revision = text(get(manifest, "graph_revision")?)?;
                require(
                    !self.revisions.contains_key(revision),
                    "duplicate_execution_graph_revision",
                )?;
                let selected: HashSet<_> = get(manifest, "nodes")?
                    .as_array()
                    .ok_or(Error("invalid_execution_graph_nodes"))?
                    .iter()
                    .map(|n| Ok(text(get(n, "task_id")?)?.to_owned()))
                    .collect::<Result<_>>()?;
                self.no_prepared(state, &selected)?;
                self.id = get(manifest, "graph_id")?.clone();
                self.head = string(revision);
                self.managed.extend(selected);
                let digest = string(graphs::digest(manifest, work)?);
                self.revisions.insert(
                    revision.to_owned(),
                    object(vec![
                        ("manifest", manifest.clone()),
                        ("digest", digest.clone()),
                        ("decisions", Value::Array(vec![])),
                        ("current_review", string("proposed")),
                    ])?,
                );
                result.set_field("graph_id", self.id.clone())?;
                result.set_field("graph_revision", string(revision))?;
                result.set_field("graph_digest", digest)?;
                result.set_field("manifest", manifest.clone())?;
            }
            "graph_decide" => {
                let approve = text(get(params, "decision")?)? == "approve";
                fields(
                    effect,
                    if approve {
                        "source decision"
                    } else {
                        "decision"
                    },
                )?;
                let graph = self.head(get(params, "graph_revision")?)?;
                require(
                    text(get(params, "expected_graph_digest")?)? == text(get(graph, "digest")?)?,
                    "execution_graph_digest_conflict",
                )?;
                if approve {
                    let matched = self.current(
                        graph,
                        checked_source(control, binding, effect, work)?,
                        state,
                        work,
                    )?;
                    self.no_prepared(state, &HashSet::new())?;
                    result.set_field("source_match", matched)?;
                }
                let decision = get(effect, "decision")?;
                fields(
                    decision,
                    "decision_id graph_revision decision reason state decided_at",
                )?;
                let id = uuid(get(decision, "decision_id")?)?.to_owned();
                timestamp(get(decision, "decided_at")?)?;
                require(
                    !self.decisions.contains(&id),
                    "duplicate_execution_graph_decision",
                )?;
                let review = if approve {
                    "approved_for_preparation"
                } else {
                    "changes_requested"
                };
                let expected = object(vec![
                    ("decision_id", string(&id)),
                    ("graph_revision", get(params, "graph_revision")?.clone()),
                    ("decision", get(params, "decision")?.clone()),
                    ("reason", get(params, "reason")?.clone()),
                    ("state", string(review)),
                    ("decided_at", get(decision, "decided_at")?.clone()),
                ])?;
                require(
                    work.equal(decision, &expected)?,
                    "execution_graph_decision_mismatch",
                )?;
                result.set_field("graph_id", self.id.clone())?;
                result.set_field("graph_digest", get(graph, "digest")?.clone())?;
                result.set_field("decision", decision.clone())?;
                let key = text(&self.head)?;
                let graph = self
                    .revisions
                    .get_mut(key)
                    .ok_or(Error("execution_graph_not_head"))?;
                let mut decisions = get(graph, "decisions")?
                    .as_array()
                    .ok_or(Error("invalid_execution_graph_decisions"))?
                    .to_vec();
                decisions.push(decision.clone());
                graph.set_field("decisions", Value::Array(decisions))?;
                graph.set_field("current_review", string(review))?;
                self.decisions.insert(id);
            }
            "prepare" | "prepare_graph" => {
                fields(effect, "source prepared")?;
                let task = text(get(params, "task_id")?)?;
                let (explicit, graph_binding) = if op == "prepare_graph" {
                    let graph = self.head(get(params, "graph_revision")?)?;
                    require(
                        text(get(graph, "current_review")?)? == "approved_for_preparation",
                        "execution_graph_not_approved",
                    )?;
                    self.current(
                        graph,
                        checked_source(control, binding, effect, work)?,
                        state,
                        work,
                    )?;
                    let node = get(get(graph, "manifest")?, "nodes")?
                        .as_array()
                        .ok_or(Error("invalid_execution_graph_nodes"))?
                        .iter()
                        .find(|n| get(n, "task_id").ok().and_then(Value::as_str) == Some(task))
                        .ok_or(Error("execution_task_not_in_graph"))?;
                    let explicit = object(vec![
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
                        .and_then(|d| d.last())
                        .ok_or(Error("execution_graph_not_approved"))?;
                    (
                        explicit,
                        object(vec![
                            ("graph_id", self.id.clone()),
                            ("graph_revision", self.head.clone()),
                            ("graph_digest", get(graph, "digest")?.clone()),
                            ("decision_id", get(latest, "decision_id")?.clone()),
                            ("node_digest", get(node, "node_digest")?.clone()),
                        ])?,
                    )
                } else {
                    require(!self.managed.contains(task), "execution_graph_managed_task")?;
                    (get(params, "intent")?.clone(), Value::Null)
                };
                return state.apply_with_preparation(
                    control,
                    binding,
                    op,
                    effect,
                    generation,
                    work,
                    Some((&explicit, &graph_binding)),
                );
            }
            _ => return Err(Error("unsupported_execution_operation")),
        }
        Ok(result)
    }
}

pub(super) fn replay(control: &Value) -> Result<Value> {
    replay_with_work(control, &mut Work::default())
}

pub(super) fn replay_with_work(control: &Value, work: &mut Work) -> Result<Value> {
    fields(
        control,
        "schema_version execution_authority_id snapshot_source_id generation legacy_v1 upgrade history extensions",
    )?;
    let legacy = get(control, "legacy_v1")?;
    let history = get(control, "history")?
        .as_array()
        .ok_or(Error("invalid_execution_history"))?;
    let legacy_history = get(legacy, "history")?
        .as_array()
        .ok_or(Error("invalid_execution_history"))?;
    let total = legacy_history
        .len()
        .checked_add(history.len())
        .ok_or(Error("unsupported_execution_history_profile"))?;
    require(!history.is_empty(), "invalid_execution_history")?;
    // Bound the combined history before either replay creates any prefix clone.
    require(
        (1..=MAX_HISTORY).contains(&total),
        "unsupported_execution_history_profile",
    )?;
    let encoded = work.encode(control)?;
    preflight(encoded.len(), total)?;
    require(
        exact_integer(get(control, "generation")?, total),
        "invalid_execution_generation",
    )?;
    let mut replayed = replay_v1(legacy, work)?;
    require(
        !replayed.state.attempts.iter().any(Attempt::active),
        "execution_upgrade_has_prepared_attempts",
    )?;
    for key in ["execution_authority_id", "snapshot_source_id", "extensions"] {
        require(
            work.equal(get(control, key)?, get(legacy, key)?)?,
            "execution_upgrade_identity_mismatch",
        )?;
    }
    let metadata = get(control, "upgrade")?;
    fields(
        metadata,
        "upgrade_id source_revision source_sha256 source_mode backup_path",
    )?;
    let upgrade = uuid(get(metadata, "upgrade_id")?)?;
    require(
        hex(text(get(metadata, "source_sha256")?)?, 64),
        "invalid_execution_upgrade_digest",
    )?;
    let valid_mode = match get(metadata, "source_mode")? {
        Value::Number(Number::Integer(n)) => {
            n.as_str().parse::<u16>().ok().is_some_and(|n| n <= 4095)
        }
        _ => false,
    };
    require(
        valid_mode
            && text(get(metadata, "source_revision")?)? == replayed.revision
            && text(get(metadata, "backup_path")?)?
                == format!("state/execution/upgrades/{upgrade}/control-v1.bytes"),
        "invalid_execution_upgrade_metadata",
    )?;
    let authority = text(get(control, "execution_authority_id")?)?;
    let mut graphs = Graphs {
        id: Value::Null,
        head: Value::Null,
        revisions: BTreeMap::new(),
        managed: HashSet::new(),
        decisions: HashSet::new(),
    };
    for (index, entry) in history.iter().enumerate() {
        let generation = legacy_history.len() + index + 1;
        fields(entry, "generation request_id binding effect result")?;
        require(
            exact_integer(get(entry, "generation")?, generation),
            "invalid_execution_history_generation",
        )?;
        let key = request_key(get(entry, "request_id")?, false)?;
        require(
            replayed.requests.insert(key.to_owned()),
            "duplicate_execution_request_id",
        )?;
        let b = get(entry, "binding")?;
        let op = binding(b, work)?;
        require(
            (op == "upgrade") == (index == 0),
            "execution_upgrade_not_first_and_unique",
        )?;
        require(
            text(get(b, "expected_execution_revision")?)? == replayed.revision,
            "execution_history_revision_mismatch",
        )?;
        let result = graphs.apply(
            control,
            &mut replayed.state,
            b,
            op,
            get(entry, "effect")?,
            generation,
            work,
        )?;
        require(
            work.equal(&result, get(entry, "result")?)?,
            "execution_receipt_effect_mismatch",
        )?;
        replayed.receipts.push((key.to_owned(), entry.clone()));
        let mut prefix = object(vec![
            ("generation", integer(generation)?),
            ("history", Value::Array(history[..=index].to_vec())),
        ])?;
        for field in [
            "schema_version",
            "execution_authority_id",
            "snapshot_source_id",
            "legacy_v1",
            "upgrade",
            "extensions",
        ] {
            prefix.set_field(field, get(control, field)?.clone())?;
        }
        // One encoding supplies both capacity and the next revision digest.
        let bytes = work.encode(&prefix)?;
        let approved = if matches!(graphs.head, Value::Null) {
            false
        } else {
            text(get(graphs.head(&graphs.head)?, "current_review")?)? == "approved_for_preparation"
        };
        capacity(
            bytes.len(),
            replayed
                .state
                .attempts
                .iter()
                .filter(|a| a.active())
                .count()
                + usize::from(approved),
        )?;
        replayed.revision = format!(
            "execution-control-v2:{authority}:{generation}:{}",
            canonical::digest(&bytes)
        );
    }
    let mut report = report(control, replayed, 2)?;
    report.set_field("graph_id", graphs.id)?;
    report.set_field("current_graph_revision", graphs.head)?;
    report.set_field(
        "graphs",
        Value::object(graphs.revisions.into_iter().collect())?,
    )?;
    let mut managed: Vec<_> = graphs.managed.into_iter().collect();
    managed.sort();
    report.set_field(
        "graph_managed_tasks",
        Value::Array(managed.into_iter().map(string).collect()),
    )?;
    Ok(report)
}
