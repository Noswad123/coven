//! Explicit, workspace-scoped resume recovery. No startup hooks or saved consent.
use crate::{
    Error, Result,
    error::require,
    interactive_sessions as interactive,
    observation_socket::InterruptGuard,
    session_support,
    storage::{self, Delivery, WorkspaceGuard},
    task_mutation::TaskOutput,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

#[path = "session_recovery/present.rs"]
mod present;

const STATE: &str = "state/session-recovery.json";
const MAX_BYTES: usize = 1_048_576;
const MAX_RECORDS: usize = 128;
const LIMITATIONS: [&str; 6] = [
    "Duplicate protection is local to this workspace journal, not global native exactly-once execution.",
    "Imported references are caller-supplied exact-tuple skip vetoes, not authenticated native bindings.",
    "No qualified native-reference inventory is consulted; absence never proves no equivalent is running elsewhere.",
    "Kit pane references are unauthenticated point-in-time skip vetoes and may be stale persisted history, not proof of running status.",
    "Queued input is not native startup, identity, readiness or completion; provider CLI compatibility is unverified.",
    "Durable intents can conservatively block commands never sent after a crash. No automatic retry, eviction or saved permission.",
];

pub struct Request {
    pub workspace: PathBuf,
    pub file: PathBuf,
    pub socket: String,
    pub expect_recovery_revision: String,
    pub execute: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
struct Reference {
    source: String,
    runtime: String,
    kind: Kind,
    value: String,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Id,
    Path,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    key: String,
    reference: Reference,
    executable: String,
    pane_id: String,
    expected_terminal_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    schema_version: u32,
    entries: Vec<Entry>,
    #[serde(default)]
    imported_references: Vec<Reference>,
}
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    SubmissionIntent,
    Queued,
    Uncertain,
    Deferred,
    Rejected,
    Unsent,
}
impl Outcome {
    fn blocks(self) -> bool {
        matches!(
            self,
            Self::SubmissionIntent | Self::Queued | Self::Uncertain
        )
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Attempt {
    entry: Entry,
    outcome: Outcome,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u32,
    revision: u64,
    attempts: Vec<Attempt>,
}

fn decode<T: serde::de::DeserializeOwned>(raw: &[u8]) -> Result<T> {
    require(raw.len() <= MAX_BYTES, "session_recovery_size_limit")?;
    crate::strict_json::check_depth(raw, 16)?;
    let value = crate::values::parse(raw, crate::values::Profile::Authority)?;
    Ok(serde_json::from_slice(&value.canonical()?)?)
}
fn text(value: &str, limit: usize) -> Result<()> {
    require(
        !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control),
        "invalid_recovery_text",
    )
}
fn absolute(value: &str) -> Result<()> {
    text(value, 4096)?;
    require(
        value.starts_with('/')
            && !value.ends_with('/')
            && !value.split('/').any(|part| part == ".."),
        "invalid_recovery_absolute_path",
    )
}
fn reference_shape(reference: &Reference) -> Result<()> {
    text(&reference.source, 512)?;
    text(&reference.runtime, 512)?;
    text(
        &reference.value,
        if reference.kind == Kind::Path {
            4096
        } else {
            512
        },
    )?;
    require(
        !reference.value.trim_start().starts_with('-'),
        "invalid_session_selector",
    )?;
    if reference.kind == Kind::Path {
        absolute(&reference.value)?;
    }
    Ok(())
}
fn reference(reference: &Reference) -> Result<()> {
    reference_shape(reference)?;
    require(
        reference.source == format!("kitsune:{}", reference.runtime),
        "recovery_source_runtime_mismatch",
    )
}
fn entry(entry: &Entry) -> Result<()> {
    text(&entry.key, 128)?;
    reference(&entry.reference)?;
    absolute(&entry.executable)?;
    for id in [&entry.pane_id, &entry.expected_terminal_id] {
        text(id, 4096)?;
        require(
            crate::kitsune_observation::session_opaque(id),
            "invalid_interactive_identifier",
        )?;
    }
    Ok(())
}
fn load(root: &Path) -> Result<(State, String)> {
    let raw = crate::workspace::optional_bytes(
        root,
        Path::new(STATE),
        MAX_BYTES,
        "session_recovery_size_limit",
    )?;
    let state: State = if let Some(raw) = &raw {
        decode(raw)?
    } else {
        State {
            schema_version: 1,
            revision: 0,
            attempts: vec![],
        }
    };
    require(
        state.schema_version == 1,
        "unsupported_session_recovery_schema",
    )?;
    require(
        state.attempts.len() <= MAX_RECORDS,
        "session_recovery_record_limit",
    )?;
    for attempt in &state.attempts {
        entry(&attempt.entry)?;
    }
    // Hash exact stored bytes, including missing-vs-present, and bind to workspace.
    let token = crate::canonical::digest(&serde_json::to_vec(&json!([
        "coven.session-recovery.revision.v1",
        root,
        raw
    ]))?);
    Ok((state, token))
}
fn root(workspace: &Path) -> Result<PathBuf> {
    let absolute = session_support::absolute(workspace)?;
    match absolute.canonicalize() {
        Ok(root) => Ok(root),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(absolute),
        Err(error) => Err(error.into()),
    }
}
trait RecoveryIo {
    fn prepare_directory(&self, guard: &WorkspaceGuard) -> Result<()> {
        storage::prepare_directory(guard, Path::new("state"))
    }
    #[cfg(test)]
    fn after_intent(&self) {}
    #[cfg(test)]
    fn before_result(&self) {}
}
struct SystemRecoveryIo;
impl RecoveryIo for SystemRecoveryIo {}

fn save(guard: &WorkspaceGuard, state: &mut State, io: &impl RecoveryIo) -> Result<String> {
    // Admission/lock acquisition can create state/. Its link in the workspace
    // must be durable too; syncing only the journal and state/ is insufficient.
    io.prepare_directory(guard)?;
    state.revision = state
        .revision
        .checked_add(1)
        .ok_or(Error("session_recovery_revision_overflow"))?;
    let bytes = serde_json::to_vec(state)?;
    require(bytes.len() <= MAX_BYTES, "session_recovery_size_limit")?;
    require(
        storage::atomic_replace_bytes(guard, Path::new(STATE), &bytes)? == Delivery::Committed,
        "session_recovery_durability_uncertain",
    )?;
    Ok(load(guard.root())?.1)
}
fn cas(root: &Path, expected: &str) -> Result<(WorkspaceGuard, State)> {
    let guard = WorkspaceGuard::acquire(root, Duration::from_secs(2))?;
    let (state, token) = load(guard.root())?;
    require(token == expected, "session_recovery_revision_conflict")?;
    Ok((guard, state))
}
fn render(report: &Value, json_output: bool, code: u8) -> Result<TaskOutput> {
    let mut output = session_support::output(report, !json_output)?;
    output.exit_code = code;
    Ok(output)
}
pub fn inspect(workspace: &Path, json_output: bool) -> Result<TaskOutput> {
    let root = root(workspace)?;
    let (state, token) = load(&root)?;
    render(
        &json!({"schema_version":1,"contract":"coven.session-recovery.v1",
        "workspace":root,"recovery_revision":token,"state":state,
        "authorization":"none","limitations":LIMITATIONS}),
        json_output,
        0,
    )
}
fn interactive_request(root: &Path, socket: &str, entry: &Entry) -> interactive::Request {
    interactive::Request {
        workspace: root.into(),
        operation: interactive::Operation::Resume,
        runtime: entry.reference.runtime.clone(),
        executable: entry.executable.clone(),
        socket: socket.into(),
        pane: entry.pane_id.clone(),
        expected_terminal: entry.expected_terminal_id.clone(),
        session: Some(entry.reference.value.clone()),
        session_kind: Some(match entry.reference.kind {
            Kind::Id => interactive::SessionKind::Id,
            Kind::Path => interactive::SessionKind::Path,
        }),
        timeout: 3.0,
        execute: true,
    }
}

pub fn run(request: &Request, json_output: bool) -> Result<TaskOutput> {
    run_with_io(request, json_output, &SystemRecoveryIo)
}

fn run_with_io(request: &Request, json_output: bool, io: &impl RecoveryIo) -> Result<TaskOutput> {
    let root = root(&request.workspace)?;
    absolute(
        request
            .file
            .to_str()
            .ok_or(Error("invalid_recovery_absolute_path"))?,
    )?;
    absolute(&request.socket)?;
    let selection: Selection = decode(&crate::read_fs::read_regular(&request.file, MAX_BYTES)?)?;
    require(
        selection.schema_version == 1,
        "unsupported_session_recovery_schema",
    )?;
    require(
        (1..=16).contains(&selection.entries.len()) && selection.imported_references.len() <= 128,
        "session_recovery_selection_limit",
    )?;
    let mut keys = HashSet::new();
    let mut targets = HashMap::new();
    let mut terminals = HashMap::new();
    let mut plans = Vec::new();
    // Complete validation precedes admission, state writes and all endpoint contact.
    for imported in &selection.imported_references {
        reference(imported)?;
    }
    for selected in &selection.entries {
        entry(selected)?;
        require(keys.insert(&selected.key), "duplicate_recovery_key")?;
        for (map, target) in [
            (&mut targets, &selected.pane_id),
            (&mut terminals, &selected.expected_terminal_id),
        ] {
            if let Some(prior) = map.insert(target, &selected.reference) {
                require(prior == &selected.reference, "conflicting_recovery_target")?;
            }
        }
        let plan = interactive::preview(&interactive_request(&root, &request.socket, selected));
        match plan {
            Ok(value) => plans.push(Some(value)),
            Err(error)
                if matches!(
                    error.code(),
                    "unsupported_interactive_runtime" | "unsupported_interactive_session_kind"
                ) =>
            {
                plans.push(None)
            }
            Err(error) => return Err(error),
        }
    }
    let (mut state, mut token) = load(&root)?;
    require(
        token == request.expect_recovery_revision,
        "session_recovery_revision_conflict",
    )?;
    let mut report = json!({"schema_version":1,"contract":"coven.session-recovery.v1",
        "workspace":root,"recovery_revision":token,"authorization":if request.execute {"current_explicit_execute"} else {"none"},
        "native_running":"unestablished",
        "outcome":if request.execute {"completed"} else {"preview"},"entries":[],"limitations":LIMITATIONS});
    // Transport admission is always first; never hold a workspace lock over network I/O.
    let gate = if request.execute {
        Some(session_support::admit(&root)?)
    } else {
        None
    };
    let interrupt = if request.execute {
        Some(InterruptGuard::install()?)
    } else {
        None
    };
    let imported: HashSet<_> = selection.imported_references.iter().collect();
    if request.execute {
        let (_guard, fresh) = cas(&root, &token)?;
        let mut eligible = HashSet::new();
        for (selected, plan) in selection.entries.iter().zip(&plans) {
            if plan.is_some()
                && !imported.contains(&selected.reference)
                && !fresh
                    .attempts
                    .iter()
                    .any(|a| a.entry.reference == selected.reference && a.outcome.blocks())
            {
                eligible.insert(&selected.reference);
            }
        }
        // Refuse the whole batch before endpoint contact, including slots for
        // potential deferrals. Callback CAS still fences concurrent writers.
        require(
            fresh.attempts.len() + eligible.len() <= MAX_RECORDS,
            "session_recovery_record_limit",
        )?;
        state = fresh;
    }
    // One recovery-specific read gathers every pane's veto before considering
    // any submission. No workspace guard is held across this read.
    let existing = if let Some(cancel) = &interrupt {
        match present::read(&request.socket, cancel) {
            Ok(existing) => Some(existing),
            Err(error) => {
                report["outcome"] = json!("batch_stopped");
                report["reference_read_error"] = json!(error.code);
                report["entries"] = json!(selection.entries.iter().map(|selected| json!({
                    "key":selected.key,"reference":selected.reference,
                    "outcome":"unattempted_reference_read_failed","delivery":"not_attempted",
                    "native_running":"unestablished"
                })).collect::<Vec<_>>());
                return render(
                    &report,
                    json_output,
                    if error.interrupted { 130 } else { 2 },
                );
            }
        }
    } else {
        None
    };
    let mut seen = HashSet::new();
    let mut stopped = false;
    let mut code = 0;
    for (selected, plan) in selection.entries.iter().zip(plans) {
        let mut row = json!({"key":selected.key,"reference":selected.reference,"delivery":"not_attempted",
                "native_running":"unestablished"});
        let skip = if stopped {
            Some("unattempted_batch_stopped")
        } else if imported.contains(&selected.reference) {
            Some("skipped_imported")
        } else if existing
            .as_ref()
            .is_some_and(|(_, references)| references.contains(&selected.reference))
        {
            Some("existing_reference_present")
        } else if !seen.insert(&selected.reference) {
            Some("skipped_duplicate")
        } else if state
            .attempts
            .iter()
            .any(|a| a.entry.reference == selected.reference && a.outcome.blocks())
        {
            Some("skipped_journal")
        } else if plan.is_none() {
            Some("adapter_unsupported")
        } else {
            None
        };
        if let Some(skip) = skip {
            row["outcome"] = json!(skip);
            if skip == "adapter_unsupported" {
                code = 2;
            }
        } else if let (Some(gate), Some(cancel)) = (&gate, &interrupt) {
            if cancel.interrupted() {
                stopped = true;
                code = 130;
                row["outcome"] = json!("unattempted_interrupted");
            } else {
                let mut intent = None;
                let mut persistence_failed = false;
                let result = interactive::execute_with_guard(
                    &interactive_request(&root, &request.socket, selected),
                    gate,
                    cancel,
                    &mut || {
                        let persisted = (|| {
                            // A1 may reselect for its generic preflight, but a
                            // different endpoint must never inherit our veto read.
                            if let Some((endpoint, _)) = &existing {
                                crate::platform::observation_socket::fence(endpoint)
                                    .map_err(|_| Error("session_recovery_endpoint_changed"))?;
                            }
                            let (guard, mut fresh) = cas(&root, &token)?;
                            require(
                                fresh.attempts.len() < MAX_RECORDS,
                                "session_recovery_record_limit",
                            )?;
                            require(
                                !fresh.attempts.iter().any(|a| {
                                    a.entry.reference == selected.reference && a.outcome.blocks()
                                }),
                                "session_recovery_duplicate",
                            )?;
                            let index = fresh.attempts.len();
                            fresh.attempts.push(Attempt {
                                entry: selected.clone(),
                                outcome: Outcome::SubmissionIntent,
                            });
                            token = save(&guard, &mut fresh, io)?;
                            state = fresh;
                            intent = Some(index);
                            Ok(())
                        })();
                        persistence_failed = persisted.is_err();
                        #[cfg(test)]
                        if persisted.is_ok() {
                            io.after_intent();
                        }
                        persisted
                    },
                );
                match result {
                    Ok(result) => {
                        row["interactive"] = result.clone();
                        row["delivery"] = result["delivery"].clone();
                        let outcome = match result["delivery"].as_str() {
                            Some("queued") => Outcome::Queued,
                            Some("uncertain") => Outcome::Uncertain,
                            Some("rejected") => Outcome::Rejected,
                            Some("unsent") if intent.is_some() => Outcome::Unsent,
                            _ => Outcome::Deferred,
                        };
                        row["outcome"] = json!(outcome);
                        if persistence_failed {
                            row["outcome"] = json!("intent_persistence_failed");
                            stopped = true;
                            code = 2;
                        } else {
                            #[cfg(test)]
                            io.before_result();
                            let persisted: Result<()> = (|| {
                                let (guard, mut fresh) = cas(&root, &token)?;
                                if let Some(index) = intent {
                                    fresh.attempts[index].outcome = outcome;
                                } else {
                                    require(
                                        fresh.attempts.len() < MAX_RECORDS,
                                        "session_recovery_record_limit",
                                    )?;
                                    fresh.attempts.push(Attempt {
                                        entry: selected.clone(),
                                        outcome,
                                    });
                                }
                                token = save(&guard, &mut fresh, io)?;
                                state = fresh;
                                Ok(())
                            })();
                            if let Err(error) = persisted {
                                row["journal_error"] = json!(error.code());
                                stopped = true;
                                code = 2;
                            }
                        }
                        if outcome == Outcome::Uncertain {
                            stopped = true;
                            code = 2;
                        } else if matches!(
                            outcome,
                            Outcome::Deferred | Outcome::Rejected | Outcome::Unsent
                        ) && code == 0
                        {
                            code = 1;
                        }
                        if cancel.interrupted() {
                            stopped = true;
                            code = 130;
                        }
                    }
                    Err(error) => {
                        row["outcome"] = json!("execution_error");
                        row["error"] = json!(error.code());
                        stopped = true;
                        code = 2;
                    }
                }
            }
        } else {
            row["outcome"] = json!("preview");
            row["interactive"] = plan.unwrap();
        }
        report["entries"].as_array_mut().unwrap().push(row);
    }
    match load(&root) {
        Ok((_, current)) => report["recovery_revision"] = json!(current),
        Err(error) => {
            report["recovery_revision"] = Value::Null;
            report["journal_error"] = json!(error.code());
            stopped = true;
            code = 2;
        }
    }
    if stopped {
        report["outcome"] = json!("batch_stopped");
    }
    render(&report, json_output, code)
}

#[cfg(test)]
#[path = "session_recovery/tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "session_recovery/unix_tests.rs"]
mod unix_tests;
