//! Coordinator for new execution writers. Cancellation keeps its existing path.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Enable,
    Enroll,
    Prepare,
    PrepareGraph,
    GraphPropose,
    GraphDecide,
    Upgrade,
}

impl Operation {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Enable => "enable",
            Self::Enroll => "enroll",
            Self::Prepare => "prepare",
            Self::PrepareGraph => "prepare_graph",
            Self::GraphPropose => "graph_propose",
            Self::GraphDecide => "graph_decide",
            Self::Upgrade => "upgrade",
        }
    }
    fn graph(self) -> bool {
        matches!(
            self,
            Self::PrepareGraph | Self::GraphPropose | Self::GraphDecide
        )
    }
}

pub(super) struct Request<'a> {
    pub operation: Operation,
    pub parameters: Value,
    pub expected_execution: &'a str,
    pub expected_tasks: Option<&'a str>,
    pub key: &'a str,
}

impl Request<'_> {
    pub(super) fn binding(&self, schema: usize, work: &mut Work) -> Result<Value> {
        require(
            self.expected_execution.len() <= values::MAX_STRING
                && self
                    .expected_tasks
                    .is_none_or(|value| value.len() <= values::MAX_STRING)
                && self.key.len() <= values::MAX_STRING,
            "unsupported_json_string_profile",
        )?;
        let value = object(vec![
            ("schema_version", integer(schema)?),
            ("operation", string(self.operation.as_str())),
            ("parameters", self.parameters.clone()),
            (
                "expected_execution_revision",
                string(self.expected_execution),
            ),
            (
                "expected_tasks_revision",
                self.expected_tasks.map_or(Value::Null, string),
            ),
        ])?;
        if schema == 2 {
            v2::binding(&value, work)?;
        } else {
            super::super::binding(&value, work)?;
        }
        Ok(value)
    }
    fn validate_key(&self) -> Result<()> {
        require(
            !self.key.is_empty()
                && self.key.len() <= 128
                && self.key.as_bytes()[0].is_ascii_alphanumeric()
                && self.key.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
                }),
            "invalid_execution_request_or_criterion_id",
        )
    }
    fn needs_source(&self) -> Result<bool> {
        Ok(match self.operation {
            Operation::Upgrade => false,
            Operation::GraphDecide => text(get(&self.parameters, "decision")?)? == "approve",
            _ => true,
        })
    }
}

pub(super) trait Metadata {
    fn uuid(&self) -> Result<String>;
}

trait ActivationIo {
    fn mkdir(&self, path: &Path) -> Result<()> {
        Ok(fs::create_dir(path)?)
    }
    fn sync_directory(&self, path: &Path) -> Result<()> {
        Ok(crate::platform::sync_directory(path)?)
    }
}
struct NativeActivationIo;
impl ActivationIo for NativeActivationIo {}

fn activate_with(root: &Path, guard: &WorkspaceGuard, io: &impl ActivationIo) -> Result<()> {
    guard.validate_for(root)?;
    let path = read_fs::managed(root, Path::new("state/execution"), true)?;
    // Exclusive creation: failed activation leaves evidence, never adoption.
    io.mkdir(&path)?;
    io.sync_directory(&root.join("state"))?;
    io.sync_directory(root)?;
    Ok(())
}

trait PrivateIo: Metadata {
    fn now(&self) -> Result<String> {
        crate::task_time::now()
    }
    fn read(&self, root: &Path) -> Result<Option<(Value, Validated)>> {
        read_typed(root)
    }
    fn lock(&self, root: &Path) -> Result<WorkspaceGuard> {
        WorkspaceGuard::execution_maintenance(root, Duration::from_secs(5))
    }
    fn observe(&self, root: &Path, snapshot: &Value, work: &mut Work) -> Result<Value> {
        source::observe(root, snapshot, work)
    }
    fn capture(
        &self,
        root: &Path,
        snapshot: &Value,
        guard: &WorkspaceGuard,
        work: &mut Work,
    ) -> Result<Value> {
        source::capture(root, snapshot, guard, work)
    }
    fn activate(&self, root: &Path, guard: &WorkspaceGuard) -> Result<()> {
        activate_with(root, guard, &NativeActivationIo)
    }
    fn replace(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        storage::atomic_replace_bytes(guard, Path::new("state/execution/control.json"), bytes)
    }
    fn output(
        &self,
        view: &Validated,
        result: &Value,
        replayed: bool,
        json: bool,
    ) -> Result<delivery::Prepared> {
        delivery::Prepared::new(view, result, replayed, json)
    }
    fn confirm(&self, root: &Path, view: &Validated, guard: &WorkspaceGuard) -> Result<()> {
        super::super::confirmation::confirm(root, view, guard)
    }
}

struct NativeIo;
impl Metadata for NativeIo {
    fn uuid(&self) -> Result<String> {
        crate::platform::random_uuid_hex()
    }
}
impl PrivateIo for NativeIo {}

pub(super) fn execute(root: &Path, request: &Request<'_>, json: bool) -> Result<TaskOutput> {
    execute_with(root, request, json, &NativeIo)
}

fn execute_with(
    root: &Path,
    request: &Request<'_>,
    json: bool,
    io: &impl PrivateIo,
) -> Result<TaskOutput> {
    let upgrading = request.operation == Operation::Upgrade;
    if upgrading {
        request.binding(2, &mut Work::default())?;
    } else {
        let initial = io.read(root)?;
        if request.operation.graph() {
            require(
                initial.as_ref().is_some_and(|(_, view)| view.schema == 2),
                "execution_graph_requires_upgrade",
            )?;
        }
        let schema = if request.operation.graph()
            || request
                .expected_execution
                .starts_with("execution-control-v2:")
        {
            2
        } else {
            1
        };
        request.binding(schema, &mut Work::default())?;
        request.validate_key()?;
        if initial.is_none() {
            require(request.operation == Operation::Enable, "execution_disabled")?;
            require(
                request.expected_execution == DISABLED,
                "execution_revision_conflict",
            )?;
            io.observe(root, &string("0".repeat(32)), &mut Work::default())?;
        }
    }

    let guard = io.lock(root)?;
    request.validate_key()?;
    let current = io.read(root)?;
    if let Some((_, view)) = current.as_ref() {
        if let Some((_, entry)) = view
            .replayed
            .receipts
            .iter()
            .find(|(key, _)| key == request.key)
        {
            let retained = get(entry, "binding")?;
            let schema = if exact_integer(get(retained, "schema_version")?, 2) {
                2
            } else {
                1
            };
            let incoming = request
                .binding(schema, &mut Work::default())
                .map_err(|_| Error("execution_request_key_binding_conflict"))?;
            require(
                Work::default().equal(retained, &incoming)?,
                "execution_request_key_binding_conflict",
            )?;
            let outputs = io.output(view, get(entry, "result")?, true, json)?;
            return Ok(outputs.confirm(io.confirm(root, view, &guard)));
        }
    }

    let mut work = Work::default();
    let schema = if upgrading {
        2
    } else {
        current.as_ref().map_or(1, |(_, view)| view.schema)
    };
    let binding = request.binding(schema, &mut work)?;
    if upgrading {
        let (_, view) = current
            .as_ref()
            .ok_or(Error("execution_upgrade_requires_enabled"))?;
        require(view.schema == 1, "execution_already_upgraded")?;
    }
    let revision = current
        .as_ref()
        .map_or(DISABLED, |(_, view)| view.replayed.revision.as_str());
    require(
        request.expected_execution == revision,
        "execution_revision_conflict",
    )?;
    if !upgrading {
        require(
            (request.operation == Operation::Enable) == current.is_none(),
            if current.is_none() {
                "execution_disabled"
            } else {
                "execution_already_enabled"
            },
        )?;
    }
    if upgrading {
        require(
            !current
                .as_ref()
                .is_some_and(|(_, view)| view.replayed.state.attempts.iter().any(Attempt::active)),
            "execution_prepared_blocks_upgrade",
        )?;
    }

    let prior_escapes = current
        .as_ref()
        .map_or(Ok(0), |(_, view)| capacity::escape_count(view))?;
    let generation = current
        .as_ref()
        .map_or(1, |(_, view)| view.replayed.receipts.len() + 1);
    let (mut control, mut selected) = match current {
        Some((control, view)) => (control, Some(view)),
        None => (
            object(vec![
                ("schema_version", integer(1)?),
                ("execution_authority_id", string(io.uuid()?)),
                ("snapshot_source_id", string(io.uuid()?)),
                ("generation", integer(0)?),
                ("history", Value::Array(vec![])),
                ("extensions", object(vec![])?),
            ])?,
            None,
        ),
    };
    let activating = selected.is_none();
    let mut upgrade_plan = None;
    let effect = if upgrading {
        let plan = upgrade::plan(
            root,
            &control,
            selected.as_ref().ok_or(Error("execution_disabled"))?,
            io,
            &mut work,
        )?;
        control = plan.control.clone();
        let effect = plan.effect.clone();
        upgrade_plan = Some(plan);
        effect
    } else {
        // A single operation timestamp precedes the independent fresh source read.
        let now = io.now()?;
        let source = if request.needs_source()? {
            let value = io.capture(
                root,
                get(&control, "snapshot_source_id")?,
                &guard,
                &mut work,
            )?;
            if let Some(expected) = request.expected_tasks {
                require(
                    text(get(&value, "revision")?)? == expected,
                    "execution_tasks_revision_conflict",
                )?;
            }
            Some(value)
        } else {
            None
        };
        if request.operation.graph() {
            graphs::build(
                &control,
                selected.as_ref().ok_or(Error("execution_disabled"))?,
                request,
                source.as_ref(),
                &now,
                io,
                &mut work,
            )?
        } else {
            effects::build(
                &control,
                selected.as_ref(),
                request,
                source.as_ref().ok_or(Error("execution_source_required"))?,
                &now,
                io,
                &mut work,
            )?
        }
    };
    let result = apply_successor(
        &control,
        selected.as_mut(),
        &binding,
        &effect,
        generation,
        &mut work,
    )?;
    let mut history = get(&control, "history")?
        .as_array()
        .ok_or(Error("invalid_execution_history"))?
        .to_vec();
    history.push(object(vec![
        ("generation", integer(generation)?),
        ("request_id", string(request.key)),
        ("binding", binding),
        ("effect", effect),
        ("result", result.clone()),
    ])?);
    control.set_field("history", Value::Array(history))?;
    control.set_field("generation", integer(generation)?)?;
    let mut bytes = control.canonical()?;
    bytes.push(b'\n');
    require(
        bytes.len() <= values::MAX_INPUT,
        "unsupported_json_size_profile",
    )?;
    let candidate = values::parse(&bytes, Profile::Authority)?;
    let mut replay_work = Work::default();
    let proposed = validated_with_work(&candidate, &mut replay_work)?;
    // Only a real approval revocation spends escape reserve. Repeated changes
    // decisions remain growth and must preserve every other outstanding escape.
    let revoking = request.operation == Operation::GraphDecide
        && !request.needs_source()?
        && capacity::escape_count(&proposed)? < prior_escapes;
    if !revoking {
        capacity::admit(&candidate, &proposed, bytes.len(), replay_work.0)?;
    }
    let outputs = io.output(&proposed, &result, false, json)?;
    if activating {
        io.activate(root, &guard)?;
    }
    if let Some(plan) = upgrade_plan {
        plan.publish(root, &guard)?;
    }
    match io.replace(&guard, &bytes)? {
        Delivery::DurabilityUncertain => {
            Ok(outputs.uncertain(Error("execution_control_durability_uncertain")))
        }
        Delivery::Committed => Ok(outputs.confirm(io.confirm(root, &proposed, &guard))),
    }
}

#[cfg(test)]
mod tests;
