//! Execution-control v1/v2 history and SDK reservation checks. Historical reports
//! never assess durability. Guards confirm retained authority under a borrowed
//! lock, but neither API grants execution admission, approval, or delivery.
//!
//! Native restrictions: Authority JSON input is at most 8 MiB (Python permits
//! 16 MiB), history has at most 1024 entries, and accepted canonical work is at
//! most 256 MiB. A refusal may encode one additional bounded value (at most
//! 32 MiB) before charging its size. Prefix verification is deliberately not
//! linear: a conservative canonical-size preflight bounds it before any prefix
//! cloning/encoding.
use crate::{
    Error, Result, canonical,
    error::require,
    read_fs,
    records::{self, TaskRecords, TeamRecord},
    values::{self, Number, Profile, Value},
};
use sha2::{Digest as _, Sha256};
use std::{collections::HashSet, fs, path::Path};

mod confirmation;
mod graphs;
mod guards;
mod inspection;
mod mutation;
mod recovery;
mod source;
mod v2;
pub use guards::{ReservationCheck, guard_authority_change, guard_task_mutation};
pub use inspection::{
    attempt_inspect_arguments, attempt_list_arguments, graph_export_arguments,
    graph_inspect_arguments, inspect_arguments,
};
pub use mutation::cancel_arguments;
pub use recovery::{RecoveryMode as ExecutionRecoveryMode, recover_arguments};

const DISABLED: &str = "execution-disabled:v1";
const MAX_HISTORY: usize = 1024;
const MAX_WORK: usize = 256 * 1_048_576;
const MAX_CONTROL: usize = 16 * 1_048_576;
const CANCELLATION_RESERVE: usize = 512 * 1024;

#[derive(Default)]
struct Work(usize);
impl Work {
    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.0 = self
            .0
            .checked_add(bytes)
            .ok_or(Error("unsupported_execution_work_profile"))?;
        require(self.0 <= MAX_WORK, "unsupported_execution_work_profile")
    }
    fn encode(&mut self, value: &Value) -> Result<Vec<u8>> {
        // The shared encoder bounds each allocation to 32 MiB before accounting.
        let bytes = value.canonical()?;
        self.charge(bytes.len())?;
        Ok(bytes)
    }
    fn equal(&mut self, left: &Value, right: &Value) -> Result<bool> {
        Ok(self.encode(left)? == self.encode(right)?)
    }
    fn digest(&mut self, domain: &[u8], value: &Value) -> Result<String> {
        let mut digest = Sha256::new();
        digest.update(domain);
        digest.update(self.encode(value)?);
        Ok(format!("{:x}", digest.finalize()))
    }
}

fn preflight(length: usize, entries: usize) -> Result<()> {
    require(
        (1..=MAX_HISTORY).contains(&entries),
        "unsupported_execution_history_profile",
    )?;
    let work = length.checked_add(32).and_then(|n| n.checked_mul(entries));
    require(
        work.is_some_and(|n| n <= MAX_WORK),
        "unsupported_execution_work_profile",
    )
}
fn capacity(length: usize, active: usize) -> Result<()> {
    let reserved = active
        .checked_mul(CANCELLATION_RESERVE)
        .and_then(|n| n.checked_add(length))
        .and_then(|n| n.checked_add(1));
    require(
        reserved.is_some_and(|n| n <= MAX_CONTROL),
        "execution_cancellation_capacity_exceeded",
    )
}
fn object(entries: Vec<(&str, Value)>) -> Result<Value> {
    Value::object(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
    )
}
fn string(value: impl Into<String>) -> Value {
    Value::String(value.into())
}
fn integer(value: usize) -> Result<Value> {
    Value::integer(&value.to_string())
}
fn get<'a>(value: &'a Value, field: &str) -> Result<&'a Value> {
    value
        .as_object()
        .and_then(|o| o.get(field))
        .ok_or(Error("missing_execution_field"))
}
fn text(value: &Value) -> Result<&str> {
    value.as_str().ok_or(Error("invalid_execution_text"))
}
fn fields(value: &Value, names: &str) -> Result<()> {
    let value = value.as_object().ok_or(Error("invalid_execution_fields"))?;
    require(
        value.entries().len() == names.split_whitespace().count()
            && names.split_whitespace().all(|n| value.get(n).is_some()),
        "invalid_execution_fields",
    )
}
fn exact_integer(value: &Value, expected: usize) -> bool {
    matches!(value, Value::Number(Number::Integer(n)) if n.as_str() == expected.to_string())
}
fn version(value: &Value) -> Result<()> {
    require(value.is_integer_one(), "unsupported_execution_version")
}
fn identifier(value: &Value) -> Result<&str> {
    let s = text(value)?;
    require(
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "invalid_execution_identifier",
    )?;
    Ok(s)
}
fn hex(s: &str, length: usize) -> bool {
    s.len() == length
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn uuid(value: &Value) -> Result<&str> {
    let s = text(value)?;
    require(hex(s, 32), "invalid_execution_uuid")?;
    Ok(s)
}
fn decimal(s: &str, zero: bool) -> bool {
    (zero && s == "0")
        || (!s.is_empty()
            && matches!(s.as_bytes()[0], b'1'..=b'9')
            && s.bytes().all(|b| b.is_ascii_digit()))
}
fn execution_revision(value: &Value) -> Result<()> {
    let s = text(value)?;
    let p: Vec<_> = s.split(':').collect();
    require(
        s == DISABLED
            || (p.len() == 4
                && p[0] == "execution-control-v1"
                && hex(p[1], 32)
                && decimal(p[2], false)
                && hex(p[3], 64)),
        "invalid_execution_revision",
    )
}
fn task_revision(value: &Value) -> Result<()> {
    let s = text(value)?;
    let p: Vec<_> = s.split(':').collect();
    require(
        s == "tasks-missing:v1"
            || (p.len() == 2 && p[0] == "tasks-sha256" && hex(p[1], 64))
            || (p.len() == 6
                && p[0] == "tasks-ledger"
                && p[1] == "v1"
                && hex(p[2], 32)
                && hex(p[3], 32)
                && decimal(p[4], true)
                && hex(p[5], 64)),
        "invalid_execution_tasks_revision",
    )
}
fn nonblank(value: &Value, limit: usize) -> Result<&str> {
    let s = text(value)?;
    require(
        !records::trimmed(s).is_empty() && s.len() <= limit,
        "invalid_execution_bounded_text",
    )?;
    Ok(s)
}
fn request_key(value: &Value, criterion: bool) -> Result<&str> {
    let s = text(value)?;
    require(
        !s.is_empty()
            && s.len() <= 128
            && s.as_bytes()[0].is_ascii_alphanumeric()
            && s.bytes().all(|b| {
                b.is_ascii_alphanumeric()
                    || matches!(b, b'_' | b'-')
                    || (!criterion && matches!(b, b'.' | b':'))
            }),
        "invalid_execution_request_or_criterion_id",
    )?;
    Ok(s)
}

fn timestamp(value: &Value) -> Result<()> {
    // No normalization: byte-for-byte timestamp spelling belongs to the record.
    let valid = || -> Option<()> {
        let b = value.as_str()?.as_bytes();
        if b.len() < 20
            || !b.is_ascii()
            || b[4] != b'-'
            || b[7] != b'-'
            || !matches!(b[10], b'T' | b' ')
            || b[13] != b':'
            || b[16] != b':'
        {
            return None;
        }
        let number = |start: usize, end: usize| -> Option<u32> {
            let bytes = b.get(start..end)?;
            if !bytes.iter().all(u8::is_ascii_digit) {
                return None;
            }
            Some(bytes.iter().fold(0, |n, c| n * 10 + u32::from(c - b'0')))
        };
        let year = number(0, 4)?;
        let month = number(5, 7)?;
        let day = number(8, 10)?;
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if leap {
                    29
                } else {
                    28
                }
            }
            _ => return None,
        };
        if year == 0
            || day == 0
            || day > days
            || number(11, 13)? > 23
            || number(14, 16)? > 59
            || number(17, 19)? > 59
        {
            return None;
        }
        let mut at = 19;
        if b.get(at) == Some(&b'.') {
            at += 1;
            let start = at;
            while b.get(at).is_some_and(u8::is_ascii_digit) {
                at += 1;
            }
            if !(1..=6).contains(&(at - start)) {
                return None;
            }
        }
        if b.get(at..) == Some(b"Z") {
            return Some(());
        }
        if b.len() != at + 6
            || !matches!(b.get(at), Some(b'+' | b'-'))
            || b[at + 3] != b':'
            || number(at + 1, at + 3)? > 23
            || number(at + 4, at + 6)? > 59
        {
            return None;
        }
        Some(())
    };
    require(valid().is_some(), "unsupported_execution_timestamp_profile")
}

fn intent(value: &Value, work: &mut Work) -> Result<()> {
    fields(
        value,
        "schema_version kind task_id owner acceptance_criteria",
    )?;
    version(get(value, "schema_version")?)?;
    require(
        text(get(value, "kind")?)? == "task-intent",
        "invalid_execution_intent_kind",
    )?;
    identifier(get(value, "task_id")?)?;
    identifier(get(value, "owner")?)?;
    let criteria = get(value, "acceptance_criteria")?
        .as_array()
        .ok_or(Error("invalid_execution_criteria"))?;
    require(
        (1..=64).contains(&criteria.len()),
        "invalid_execution_criteria",
    )?;
    let mut seen = HashSet::new();
    for c in criteria {
        fields(c, "id text")?;
        require(
            seen.insert(request_key(get(c, "id")?, true)?),
            "duplicate_execution_criterion",
        )?;
        nonblank(get(c, "text")?, 16384)?;
    }
    require(
        work.encode(value)?.len() <= 1_048_576,
        "execution_intent_size_limit",
    )
}

fn source(value: &Value, snapshot: &Value, work: &mut Work) -> Result<()> {
    fields(value, "identity revision tasks roster")?;
    require(
        work.encode(value)?.len() <= MAX_CONTROL,
        "execution_source_size_limit",
    )?;
    let identity = get(value, "identity")?;
    let rev = get(value, "revision")?;
    task_revision(rev)?;
    let revision = text(rev)?;
    let tasks = get(value, "tasks")?;
    match text(get(identity, "kind")?)? {
        "snapshot-v1" => {
            fields(identity, "kind source_id path")?;
            require(
                text(get(identity, "source_id")?)? == text(snapshot)?
                    && text(get(identity, "path")?)? == "state/tasks.json"
                    && (revision == "tasks-missing:v1" || revision.starts_with("tasks-sha256:")),
                "execution_snapshot_identity_mismatch",
            )?;
            if revision == "tasks-missing:v1" {
                require(
                    tasks.as_array().is_some_and(|v| v.is_empty()),
                    "execution_missing_source_has_tasks",
                )?;
            }
            // Snapshot revisions hash original file bytes, not this retained view.
        }
        "task-ledger-v1" => {
            fields(identity, "kind workspace_id epoch_id path")?;
            let workspace = uuid(get(identity, "workspace_id")?)?;
            let epoch = uuid(get(identity, "epoch_id")?)?;
            require(
                text(get(identity, "path")?)? == "state/task-ledger"
                    && revision.starts_with(&format!("tasks-ledger:v1:{workspace}:{epoch}:")),
                "execution_ledger_identity_mismatch",
            )?;
            require(
                revision.rsplit(':').next() == Some(work.digest(b"", tasks)?.as_str()),
                "execution_ledger_digest_mismatch",
            )?;
        }
        _ => return Err(Error("unsupported_execution_source_identity")),
    }
    TaskRecords::validate(tasks.clone())?;
    // Reuse the canonical roster validator without reading a current team file.
    TeamRecord::validate(object(vec![
        ("name", string("execution history")),
        ("agents", get(value, "roster")?.clone()),
    ])?)?;
    Ok(())
}

fn binding<'a>(value: &'a Value, work: &mut Work) -> Result<&'a str> {
    fields(
        value,
        "schema_version operation parameters expected_execution_revision expected_tasks_revision",
    )?;
    version(get(value, "schema_version")?)?;
    execution_revision(get(value, "expected_execution_revision")?)?;
    let op = text(get(value, "operation")?)?;
    let params = get(value, "parameters")?;
    let tasks = get(value, "expected_tasks_revision")?;
    match op {
        "enable" => {
            fields(params, "")?;
            if !matches!(tasks, Value::Null) {
                task_revision(tasks)?;
            }
        }
        "enroll" | "prepare" => {
            fields(
                params,
                if op == "enroll" {
                    "task_id rebind_source"
                } else {
                    "task_id intent"
                },
            )?;
            task_revision(tasks)?;
            identifier(get(params, "task_id")?)?;
            if op == "enroll" {
                require(
                    matches!(get(params, "rebind_source")?, Value::Bool(_)),
                    "invalid_execution_rebind_flag",
                )?;
            } else {
                let explicit = get(params, "intent")?;
                intent(explicit, work)?;
                require(
                    text(get(explicit, "task_id")?)? == text(get(params, "task_id")?)?,
                    "execution_intent_task_mismatch",
                )?;
            }
        }
        "cancel" => {
            fields(params, "attempt_id reason")?;
            require(
                matches!(tasks, Value::Null),
                "execution_cancel_depends_on_tasks",
            )?;
            uuid(get(params, "attempt_id")?)?;
            nonblank(get(params, "reason")?, 16384)?;
        }
        _ => return Err(Error("unsupported_execution_operation")),
    }
    Ok(op)
}

struct Enrollment {
    task: String,
    id: String,
    bindings: Vec<Value>,
}
struct Attempt {
    id: String,
    task: String,
    prepared: Value,
    transitions: Vec<Value>,
}
impl Attempt {
    fn active(&self) -> bool {
        self.transitions.is_empty()
    }
    fn state(&self) -> &'static str {
        if self.active() {
            "prepared"
        } else {
            "cancelled_before_submission"
        }
    }
}
#[derive(Default)]
struct Replay {
    enrollments: Vec<Enrollment>,
    attempts: Vec<Attempt>,
    cancellations: HashSet<String>,
}
impl Replay {
    fn apply(
        &mut self,
        control: &Value,
        binding: &Value,
        op: &str,
        effect: &Value,
        generation: usize,
        work: &mut Work,
    ) -> Result<Value> {
        self.apply_with_preparation(control, binding, op, effect, generation, work, None)
    }

    // Bindings are checked by the replay driver. V2 supplies its derived intent
    // and graph binding directly, never a fabricated v1 authorization token.
    #[expect(
        clippy::too_many_arguments,
        reason = "Keep checked replay inputs explicit while sharing v1/v2 preparation guards"
    )]
    fn apply_with_preparation(
        &mut self,
        control: &Value,
        binding: &Value,
        op: &str,
        effect: &Value,
        generation: usize,
        work: &mut Work,
        preparation: Option<(&Value, &Value)>,
    ) -> Result<Value> {
        let authority = get(control, "execution_authority_id")?;
        let snapshot = get(control, "snapshot_source_id")?;
        let mut result = object(vec![
            ("operation", string(op)),
            ("generation", integer(generation)?),
            ("execution_authority_id", authority.clone()),
        ])?;
        let params = get(binding, "parameters")?;
        if op == "enable" {
            require(
                generation == 1 && text(get(binding, "expected_execution_revision")?)? == DISABLED,
                "execution_enable_not_first",
            )?;
            fields(effect, "source")?;
            let s = get(effect, "source")?;
            source(s, snapshot, work)?;
            let expected = get(binding, "expected_tasks_revision")?;
            require(
                matches!(expected, Value::Null) || text(expected)? == text(get(s, "revision")?)?,
                "execution_tasks_revision_conflict",
            )?;
            result.set_field("snapshot_source_id", snapshot.clone())?;
            result.set_field("source", s.clone())?;
            return Ok(result);
        }
        require(generation > 1, "execution_history_must_start_with_enable")?;
        if op == "cancel" {
            fields(effect, "cancel_id attempt_id reason cancelled_at")?;
            let cancel = uuid(get(effect, "cancel_id")?)?;
            timestamp(get(effect, "cancelled_at")?)?;
            require(
                text(get(effect, "attempt_id")?)? == text(get(params, "attempt_id")?)?
                    && text(get(effect, "reason")?)? == text(get(params, "reason")?)?,
                "execution_cancel_effect_mismatch",
            )?;
            let attempt_id = text(get(params, "attempt_id")?)?;
            let attempt = self
                .attempts
                .iter_mut()
                .find(|a| a.id == attempt_id)
                .ok_or(Error("execution_attempt_not_prepared"))?;
            require(attempt.active(), "execution_attempt_not_prepared")?;
            require(
                self.cancellations.insert(cancel.to_owned()),
                "duplicate_execution_cancellation_id",
            )?;
            attempt.transitions.push(effect.clone());
            result.set_field("attempt_id", string(&attempt.id))?;
            result.set_field("state", string(attempt.state()))?;
            result.set_field("transition", effect.clone())?;
            return Ok(result);
        }
        fields(
            effect,
            if op == "enroll" {
                "source enrollment"
            } else {
                "source prepared"
            },
        )?;
        let s = get(effect, "source")?;
        source(s, snapshot, work)?;
        require(
            text(get(s, "revision")?)? == text(get(binding, "expected_tasks_revision")?)?,
            "execution_tasks_revision_conflict",
        )?;
        let task_id = text(get(params, "task_id")?)?;
        let task = get(s, "tasks")?
            .as_array()
            .ok_or(Error("invalid_execution_tasks"))?
            .iter()
            .find(|t| {
                t.as_object()
                    .and_then(|o| o.get("id"))
                    .and_then(Value::as_str)
                    == Some(task_id)
            })
            .ok_or(Error("execution_task_not_found"))?;
        let previous = self.enrollments.iter().position(|e| e.task == task_id);
        let active = self
            .attempts
            .iter()
            .any(|a| a.task == task_id && a.active());
        let identity = get(s, "identity")?;
        if op == "enroll" {
            let rebind = matches!(get(params, "rebind_source")?, Value::Bool(true));
            if let Some(index) = previous {
                let last = self.enrollments[index]
                    .bindings
                    .last()
                    .ok_or(Error("invalid_execution_enrollment"))?;
                require(
                    rebind
                        && !work.equal(get(last, "task_authority_identity")?, identity)?
                        && !active,
                    "execution_invalid_source_rebind",
                )?;
            } else {
                require(!rebind, "execution_rebind_unenrolled_task")?;
            }
            let record = get(effect, "enrollment")?;
            fields(
                record,
                "enrollment_id task_id binding_number task_authority_identity task_source_revision complete_task_snapshot task_snapshot_digest enrolled_at",
            )?;
            let id = uuid(get(record, "enrollment_id")?)?;
            timestamp(get(record, "enrolled_at")?)?;
            if let Some(index) = previous {
                require(
                    self.enrollments[index].id == id,
                    "execution_rebind_changed_enrollment_id",
                )?;
            } else {
                require(
                    !self.enrollments.iter().any(|e| e.id == id),
                    "duplicate_execution_enrollment_id",
                )?;
            }
            let expected = object(vec![
                ("enrollment_id", string(id)),
                ("task_id", string(task_id)),
                (
                    "binding_number",
                    integer(previous.map_or(1, |i| self.enrollments[i].bindings.len() + 1))?,
                ),
                ("task_authority_identity", identity.clone()),
                ("task_source_revision", get(s, "revision")?.clone()),
                ("complete_task_snapshot", task.clone()),
                (
                    "task_snapshot_digest",
                    string(work.digest(b"coven-task-snapshot-v1\0", task)?),
                ),
                ("enrolled_at", get(record, "enrolled_at")?.clone()),
            ])?;
            require(
                work.equal(record, &expected)?,
                "execution_enrollment_effect_mismatch",
            )?;
            let index = if let Some(index) = previous {
                index
            } else {
                self.enrollments.push(Enrollment {
                    task: task_id.to_owned(),
                    id: id.to_owned(),
                    bindings: vec![],
                });
                self.enrollments.len() - 1
            };
            self.enrollments[index].bindings.push(record.clone());
            result.set_field("task_id", string(task_id))?;
            result.set_field("enrollment_id", string(id))?;
            result.set_field("enrollment", record.clone())?;
        } else {
            let enrolled = previous
                .and_then(|i| self.enrollments.get(i))
                .ok_or(Error("execution_task_not_enrolled"))?;
            let last = enrolled
                .bindings
                .last()
                .ok_or(Error("invalid_execution_enrollment"))?;
            require(
                work.equal(get(last, "task_authority_identity")?, identity)?,
                "execution_enrollment_source_mismatch",
            )?;
            let explicit = match preparation {
                Some((explicit, _)) => explicit,
                None => get(params, "intent")?,
            };
            let owner = text(get(explicit, "owner")?)?;
            require(
                task.as_object()
                    .and_then(|o| o.get("owner"))
                    .and_then(Value::as_str)
                    == Some(owner)
                    && get(s, "roster")?
                        .as_array()
                        .is_some_and(|r| r.iter().any(|v| v.as_str() == Some(owner))),
                "execution_prepared_owner_mismatch",
            )?;
            require(!active, "execution_task_already_prepared")?;
            let record = get(effect, "prepared")?;
            fields(
                record,
                if preparation.is_some() {
                    "schema_version attempt_id task_id enrollment_id attempt_number previous_attempt_id execution_authority_id task_authority_identity task_source_revision complete_task_snapshot task_snapshot_digest intent intent_digest prepared_at graph_binding"
                } else {
                    "schema_version attempt_id task_id enrollment_id attempt_number previous_attempt_id execution_authority_id task_authority_identity task_source_revision complete_task_snapshot task_snapshot_digest intent intent_digest prepared_at"
                },
            )?;
            require(
                exact_integer(
                    get(record, "schema_version")?,
                    if preparation.is_some() { 2 } else { 1 },
                ),
                "unsupported_execution_version",
            )?;
            let id = uuid(get(record, "attempt_id")?)?;
            timestamp(get(record, "prepared_at")?)?;
            require(
                !self.attempts.iter().any(|a| a.id == id),
                "duplicate_execution_attempt_id",
            )?;
            let older: Vec<_> = self.attempts.iter().filter(|a| a.task == task_id).collect();
            let mut expected = object(vec![
                (
                    "schema_version",
                    integer(if preparation.is_some() { 2 } else { 1 })?,
                ),
                ("attempt_id", string(id)),
                ("task_id", string(task_id)),
                ("enrollment_id", string(&enrolled.id)),
                ("attempt_number", integer(older.len() + 1)?),
                (
                    "previous_attempt_id",
                    older.last().map_or(Value::Null, |a| string(&a.id)),
                ),
                ("execution_authority_id", authority.clone()),
                ("task_authority_identity", identity.clone()),
                ("task_source_revision", get(s, "revision")?.clone()),
                ("complete_task_snapshot", task.clone()),
                (
                    "task_snapshot_digest",
                    string(work.digest(b"coven-task-snapshot-v1\0", task)?),
                ),
                ("intent", explicit.clone()),
                ("prepared_at", get(record, "prepared_at")?.clone()),
            ])?;
            let mut payload = object(vec![("explicit_intent", explicit.clone())])?;
            for key in [
                "execution_authority_id",
                "enrollment_id",
                "task_authority_identity",
                "task_source_revision",
                "complete_task_snapshot",
            ] {
                payload.set_field(key, get(&expected, key)?.clone())?;
            }
            if let Some((_, graph_binding)) = preparation {
                expected.set_field("graph_binding", graph_binding.clone())?;
                payload.set_field("graph_binding", graph_binding.clone())?;
            }
            expected.set_field(
                "intent_digest",
                string(work.digest(
                    if preparation.is_some() {
                        b"coven-task-attempt-intent-v2\0"
                    } else {
                        b"coven-task-attempt-intent-v1\0"
                    },
                    &payload,
                )?),
            )?;
            require(
                work.equal(record, &expected)?,
                "execution_prepared_effect_mismatch",
            )?;
            self.attempts.push(Attempt {
                id: id.to_owned(),
                task: task_id.to_owned(),
                prepared: record.clone(),
                transitions: vec![],
            });
            result.set_field("attempt_id", string(id))?;
            result.set_field("attempt", record.clone())?;
            result.set_field("state", string("prepared"))?;
        }
        Ok(result)
    }
}

fn envelope(status: &str, revision: String) -> Result<Value> {
    object(vec![
        ("status", string(status)),
        ("revision", string(revision)),
        ("authorization", string("none")),
        ("dispatchable", Value::Bool(false)),
        ("current_source", string("not_assessed")),
        ("durability", string("not_assessed")),
        ("assessment", string("historical_authority_only")),
    ])
}
#[cfg(test)]
fn replay(control: &Value) -> Result<Value> {
    validated(control)?.report(control)
}

// Complete validated authority, before any historical presentation is built.
struct Validated {
    replayed: Replayed,
    authority: String,
    schema: usize,
    graph: Option<Value>,
}

impl Validated {
    fn report(self, control: &Value) -> Result<Value> {
        let mut output = report(control, self.replayed, self.schema)?;
        if let Some(graph) = self.graph {
            for (key, value) in graph
                .as_object()
                .ok_or(Error("invalid_execution_fields"))?
                .entries()
            {
                output.set_field(key, value.clone())?;
            }
        }
        Ok(output)
    }
}

fn validated(control: &Value) -> Result<Validated> {
    if control
        .as_object()
        .and_then(|o| o.get("schema_version"))
        .is_some_and(|v| exact_integer(v, 2))
    {
        return v2::validated_with_work(control, &mut Work::default());
    }
    let replayed = replay_v1(control, &mut Work::default())?;
    Ok(Validated {
        replayed,
        authority: text(get(control, "execution_authority_id")?)?.to_owned(),
        schema: 1,
        graph: None,
    })
}

struct Replayed {
    state: Replay,
    revision: String,
    receipts: Vec<(String, Value)>,
    requests: HashSet<String>,
}

// Keep validated state separate from presentation so an upgrade carries the
// original attempts, cancellation IDs, and receipts without a second replay.
fn replay_v1(control: &Value, work: &mut Work) -> Result<Replayed> {
    fields(
        control,
        "schema_version execution_authority_id snapshot_source_id generation history extensions",
    )?;
    version(get(control, "schema_version")?)?;
    let authority = uuid(get(control, "execution_authority_id")?)?;
    uuid(get(control, "snapshot_source_id")?)?;
    require(
        get(control, "extensions")?.as_object().is_some(),
        "invalid_execution_extensions",
    )?;
    let history = get(control, "history")?
        .as_array()
        .ok_or(Error("invalid_execution_history"))?;
    require(
        exact_integer(get(control, "generation")?, history.len()),
        "invalid_execution_generation",
    )?;
    let encoded = work.encode(control)?;
    preflight(encoded.len(), history.len())?;
    let revision = format!(
        "execution-control-v1:{authority}:{}:{}",
        history.len(),
        canonical::digest(&encoded)
    );
    let mut state = Replay::default();
    let mut receipts = Vec::new();
    let mut requests = HashSet::new();
    for (index, entry) in history.iter().enumerate() {
        let generation = index + 1;
        fields(entry, "generation request_id binding effect result")?;
        require(
            exact_integer(get(entry, "generation")?, generation),
            "invalid_execution_history_generation",
        )?;
        let key = request_key(get(entry, "request_id")?, false)?;
        require(
            requests.insert(key.to_owned()),
            "duplicate_execution_request_id",
        )?;
        let b = get(entry, "binding")?;
        let op = binding(b, work)?;
        if index > 0 {
            // Final extensions are intentional even for early prefix hashes.
            let mut prefix = object(vec![
                ("history", Value::Array(history[..index].to_vec())),
                ("generation", integer(index)?),
            ])?;
            for key in [
                "schema_version",
                "execution_authority_id",
                "snapshot_source_id",
                "extensions",
            ] {
                prefix.set_field(key, get(control, key)?.clone())?;
            }
            let expected = format!(
                "execution-control-v1:{authority}:{index}:{}",
                work.digest(b"", &prefix)?
            );
            require(
                text(get(b, "expected_execution_revision")?)? == expected,
                "execution_history_revision_mismatch",
            )?;
        }
        let result = state.apply(control, b, op, get(entry, "effect")?, generation, work)?;
        require(
            work.equal(&result, get(entry, "result")?)?,
            "execution_receipt_effect_mismatch",
        )?;
        // Immutable retained receipt: cancellation only mutates derived attempts.
        receipts.push((key.to_owned(), entry.clone()));
    }
    capacity(
        encoded.len(),
        state.attempts.iter().filter(|a| a.active()).count(),
    )?;
    Ok(Replayed {
        state,
        revision,
        receipts,
        requests,
    })
}

fn report(control: &Value, replayed: Replayed, schema: usize) -> Result<Value> {
    let Replayed {
        state,
        revision,
        receipts,
        ..
    } = replayed;
    let mut report = envelope("active", revision)?;
    report.set_field("schema_version", integer(schema)?)?;
    for key in [
        "execution_authority_id",
        "snapshot_source_id",
        "generation",
        "extensions",
    ] {
        report.set_field(key, get(control, key)?.clone())?;
    }
    let mut reserved: Vec<_> = state
        .attempts
        .iter()
        .filter(|a| a.active())
        .map(|a| a.task.clone())
        .collect();
    reserved.sort();
    reserved.dedup();
    report.set_field(
        "reserved_task_ids",
        Value::Array(reserved.into_iter().map(string).collect()),
    )?;
    report.set_field(
        "enrollments",
        Value::object(
            state
                .enrollments
                .into_iter()
                .map(|e| {
                    Ok((
                        e.task.clone(),
                        object(vec![
                            ("enrollment_id", string(e.id)),
                            ("task_id", string(e.task)),
                            ("bindings", Value::Array(e.bindings)),
                        ])?,
                    ))
                })
                .collect::<Result<_>>()?,
        )?,
    )?;
    report.set_field(
        "attempts",
        Value::object(
            state
                .attempts
                .into_iter()
                .map(|a| {
                    let status = a.state();
                    Ok((
                        a.id,
                        object(vec![
                            ("prepared", a.prepared),
                            ("state", string(status)),
                            ("transitions", Value::Array(a.transitions)),
                        ])?,
                    ))
                })
                .collect::<Result<_>>()?,
        )?,
    )?;
    report.set_field("receipts", Value::object(receipts)?)?;
    Ok(report)
}

/// Inspect retained execution authority only. No current source, lock, write,
/// process, checkpoint, or durability operation is performed.
pub fn history(workspace: &Path) -> Result<Value> {
    crate::platform::ensure_supported()?;
    let root = workspace.canonicalize()?;
    require(root.is_dir(), "invalid_execution_workspace")?;
    match read_typed(&root)? {
        Some((control, validated)) => validated.report(&control),
        None => envelope("disabled", DISABLED.to_owned()),
    }
}

// None means only that the managed execution directory is absent. Each call
// owns its existing replay budget; two reads are not an aggregate CPU bound.
fn read_typed(root: &Path) -> Result<Option<(Value, Validated)>> {
    let directory = read_fs::managed(root, Path::new("state/execution"), true)?;
    let control = read_fs::managed(root, Path::new("state/execution/control.json"), false)?;
    match fs::symlink_metadata(&control) {
        Ok(_) => {
            // Explicitly narrower than Python's 16 MiB persisted-control limit.
            let raw = read_fs::read_regular(&control, values::MAX_INPUT)?;
            let control = values::parse(&raw, Profile::Authority)?;
            let validated = validated(&control)?;
            Ok(Some((control, validated)))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::symlink_metadata(directory) {
                Ok(_) => Err(Error("pending_execution_authority")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error.into()),
            }
        }
        Err(error) => Err(error.into()),
    }
}

/// CLI-only workspace expansion and bounded presentation; the Path API is literal.
pub fn history_json_argument(workspace: &Path) -> Result<String> {
    let workspace = crate::platform::workspace_argument(workspace)?;
    let report = history(&workspace)?.compact()?;
    crate::output::ascii_json(std::str::from_utf8(&report)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Value {
        values::parse(s.as_bytes(), Profile::Authority).unwrap()
    }
    pub(super) fn enabled() -> Value {
        parse(
            r#"{
          "schema_version":1,"execution_authority_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
          "snapshot_source_id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","generation":1,
          "extensions":{"retained":[1.0,123456789012345678901234567890]},
          "history":[{"generation":1,"request_id":"enable","binding":{
            "schema_version":1,"operation":"enable","parameters":{},
            "expected_execution_revision":"execution-disabled:v1","expected_tasks_revision":null},
            "effect":{"source":{"identity":{"kind":"snapshot-v1",
              "source_id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","path":"state/tasks.json"},
              "revision":"tasks-sha256:0000000000000000000000000000000000000000000000000000000000000000",
              "tasks":[{"id":"task","title":"T","status":"open","owner":"worker","extra":-0.0}],"roster":["worker"]}},
            "result":{"operation":"enable","generation":1,"execution_authority_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
              "snapshot_source_id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","source":{"identity":{"kind":"snapshot-v1",
                "source_id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","path":"state/tasks.json"},
                "revision":"tasks-sha256:0000000000000000000000000000000000000000000000000000000000000000",
                "tasks":[{"id":"task","title":"T","status":"open","owner":"worker","extra":-0.0}],"roster":["worker"]}}}]
        }"#,
        )
    }
    fn append(control: &mut Value, operation: &str, params: Value, effect: Value) {
        let generation = get(control, "history").unwrap().as_array().unwrap().len() + 1;
        let revision = get(&replay(control).unwrap(), "revision").unwrap().clone();
        let tasks = if operation == "cancel" {
            Value::Null
        } else {
            get(get(&effect, "source").unwrap(), "revision")
                .unwrap()
                .clone()
        };
        let b = object(vec![
            ("schema_version", integer(1).unwrap()),
            ("operation", string(operation)),
            ("parameters", params),
            ("expected_execution_revision", revision),
            ("expected_tasks_revision", tasks),
        ])
        .unwrap();
        // Build the expected receipt through a fresh replay of earlier effects;
        // corruption tests below independently change records/bindings/receipts.
        let mut state = Replay::default();
        let mut work = Work::default();
        for (i, entry) in get(control, "history")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let binding = get(entry, "binding").unwrap();
            state
                .apply(
                    control,
                    binding,
                    text(get(binding, "operation").unwrap()).unwrap(),
                    get(entry, "effect").unwrap(),
                    i + 1,
                    &mut work,
                )
                .unwrap();
        }
        let result = state
            .apply(control, &b, operation, &effect, generation, &mut work)
            .unwrap();
        let entry = object(vec![
            ("generation", integer(generation).unwrap()),
            ("request_id", string(format!("request-{generation}"))),
            ("binding", b),
            ("effect", effect),
            ("result", result),
        ])
        .unwrap();
        control
            .set_field("generation", integer(generation).unwrap())
            .unwrap();
        let mut entries = get(control, "history")
            .unwrap()
            .as_array()
            .unwrap()
            .to_vec();
        entries.push(entry);
        control.set_field("history", Value::Array(entries)).unwrap();
    }

    #[test]
    fn exact_work_history_and_capacity_limits() {
        assert_eq!(values::MAX_INPUT, 8 * 1_048_576);
        assert!(preflight(MAX_WORK / 1024 - 32, 1024).is_ok());
        assert!(preflight(MAX_WORK / 1024 - 31, 1024).is_err());
        assert!(preflight(0, 1025).is_err());
        assert!(preflight(0, 0).is_err());
        assert!(preflight(usize::MAX, 1).is_err());
        assert!(preflight(usize::MAX / 2, 3).is_err());
        let mut work = Work(MAX_WORK - 1);
        work.charge(1).unwrap();
        assert!(work.charge(1).is_err());
        assert!(Work(usize::MAX).charge(1).is_err());
        let mut work = Work(MAX_WORK - 8);
        assert!(work.equal(&Value::Null, &Value::Null).unwrap());
        assert_eq!(work.0, MAX_WORK);
        assert!(work.encode(&Value::Null).is_err());
        assert!(capacity(MAX_CONTROL - 1, 0).is_ok());
        assert!(capacity(MAX_CONTROL, 0).is_err());
        assert!(capacity(MAX_CONTROL - 1 - CANCELLATION_RESERVE, 1).is_ok());
        assert!(capacity(MAX_CONTROL - CANCELLATION_RESERVE, 1).is_err());
        assert!(capacity(0, usize::MAX).is_err());
    }

    #[test]
    fn timestamp_profile_is_calendar_checked_without_normalization() {
        for s in [
            "0001-01-01T00:00:00Z",
            "9999-12-31 23:59:59.123456-00:00",
            "2000-02-29T12:30:40.1+23:59",
            "2024-02-29T00:00:00-23:59",
        ] {
            assert!(timestamp(&string(s)).is_ok(), "{s}");
        }
        for s in [
            "0000-01-01T00:00:00Z",
            "1900-02-29T00:00:00Z",
            "2025-02-29T00:00:00Z",
            "2024-04-31T00:00:00Z",
            "2024-00-01T00:00:00Z",
            "2024-01-00T00:00:00Z",
            "2024-01-01T24:00:00Z",
            "2024-01-01T00:60:00Z",
            "2024-01-01T00:00:60Z",
            "2024-01-01T00:00:00+24:00",
            "2024-01-01T00:00:00+00:60",
            "2024-01-01T00:00:00.Z",
            "2024-01-01T00:00:00.1234567Z",
            "20240101T000000Z",
            "2024-01-01t00:00:00Z",
            "2024-01-01T00:00:00",
            "2024-01-01T00:00:00+00:00:01",
        ] {
            assert_eq!(
                timestamp(&string(s)).unwrap_err().0,
                "unsupported_execution_timestamp_profile",
                "{s}"
            );
        }
    }

    #[test]
    fn enable_retains_generic_values_and_rejects_corrupt_receipt() {
        let control = enabled();
        let report = replay(&control).unwrap();
        assert_eq!(text(get(&report, "status").unwrap()).unwrap(), "active");
        assert_eq!(
            get(&report, "extensions").unwrap().canonical().unwrap(),
            get(&control, "extensions").unwrap().canonical().unwrap()
        );
        assert!(
            get(&report, "reserved_task_ids")
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );
        let mut bad = control.clone();
        let mut entries = get(&bad, "history").unwrap().as_array().unwrap().to_vec();
        entries[0].set_field("result", Value::Null).unwrap();
        bad.set_field("history", Value::Array(entries)).unwrap();
        assert_eq!(
            replay(&bad).unwrap_err().0,
            "execution_receipt_effect_mismatch"
        );
        assert_eq!(
            replay(&parse("null")).unwrap_err().0,
            "invalid_execution_fields"
        );
        assert_eq!(
            replay(&parse(r#"{"schema_version":2}"#)).unwrap_err().0,
            "invalid_execution_fields"
        );
        assert_eq!(
            replay(&parse(r#"{"schema_version":2.0}"#)).unwrap_err().0,
            "invalid_execution_fields"
        );
    }

    fn source_fixture(control: &Value) -> Value {
        let first = &get(control, "history").unwrap().as_array().unwrap()[0];
        get(get(first, "effect").unwrap(), "source")
            .unwrap()
            .clone()
    }

    pub(super) fn upgraded(legacy: Value) -> Value {
        let revision = replay_v1(&legacy, &mut Work::default()).unwrap().revision;
        let metadata = object(vec![
            ("upgrade_id", string("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee")),
            ("source_revision", string(&revision)),
            ("source_sha256", string("0".repeat(64))),
            ("source_mode", integer(4095).unwrap()),
            (
                "backup_path",
                string(
                    "state/execution/upgrades/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee/control-v1.bytes",
                ),
            ),
        ])
        .unwrap();
        let generation = get(&legacy, "history").unwrap().as_array().unwrap().len() + 1;
        let authority = get(&legacy, "execution_authority_id").unwrap().clone();
        let result = object(vec![
            ("operation", string("upgrade")),
            ("generation", integer(generation).unwrap()),
            ("execution_authority_id", authority.clone()),
            ("upgrade_id", get(&metadata, "upgrade_id").unwrap().clone()),
            (
                "backup_path",
                get(&metadata, "backup_path").unwrap().clone(),
            ),
            ("source_revision", string(&revision)),
        ])
        .unwrap();
        let entry = object(vec![
            ("generation", integer(generation).unwrap()),
            ("request_id", string("upgrade")),
            (
                "binding",
                object(vec![
                    ("schema_version", integer(2).unwrap()),
                    ("operation", string("upgrade")),
                    ("parameters", object(vec![]).unwrap()),
                    ("expected_execution_revision", string(revision)),
                    ("expected_tasks_revision", Value::Null),
                ])
                .unwrap(),
            ),
            ("effect", metadata.clone()),
            ("result", result),
        ])
        .unwrap();
        object(vec![
            ("schema_version", integer(2).unwrap()),
            ("execution_authority_id", authority),
            (
                "snapshot_source_id",
                get(&legacy, "snapshot_source_id").unwrap().clone(),
            ),
            ("extensions", get(&legacy, "extensions").unwrap().clone()),
            ("generation", integer(generation).unwrap()),
            ("legacy_v1", legacy),
            ("upgrade", metadata),
            ("history", Value::Array(vec![entry])),
        ])
        .unwrap()
    }

    #[test]
    fn extracted_state_retains_receipts_cancellations_and_work() {
        let mut control = enabled();
        enroll(&mut control);
        let first = "ffffffffffffffffffffffffffffffff";
        let second = "11111111111111111111111111111111";
        prepare(&mut control, first, Value::Null, 1);
        cancel(&mut control, first, "dddddddddddddddddddddddddddddddd");
        prepare(&mut control, second, string(first), 2);
        cancel(&mut control, second, "22222222222222222222222222222222");
        let mut work = Work::default();
        let extracted = replay_v1(&control, &mut work).unwrap();
        // Frozen canonical-encoding charges, not a dynamically calibrated budget.
        const V1_WORK: usize = 54_929;
        const V2_WORK: usize = 81_919;
        assert_eq!(work.0, V1_WORK);
        assert_eq!(extracted.state.enrollments.len(), 1);
        assert_eq!(extracted.requests.len(), 6);
        assert_eq!(extracted.receipts.len(), 6);
        assert_eq!(extracted.state.attempts.len(), 2);
        assert!(
            extracted
                .state
                .attempts
                .iter()
                .all(|a| !a.active() && a.transitions.len() == 1)
        );
        assert_eq!(
            extracted.state.cancellations,
            HashSet::from([
                "dddddddddddddddddddddddddddddddd".to_owned(),
                "22222222222222222222222222222222".to_owned(),
            ])
        );
        assert_eq!(
            text(get(&extracted.state.attempts[1].prepared, "previous_attempt_id").unwrap())
                .unwrap(),
            first
        );
        let frozen = get(&extracted.receipts[2].1, "result").unwrap();
        assert_eq!(text(get(frozen, "state").unwrap()).unwrap(), "prepared");
        assert_eq!(
            report(&control, extracted, 1).unwrap().canonical().unwrap(),
            replay(&control).unwrap().canonical().unwrap()
        );
        let mut exact = Work(MAX_WORK - V1_WORK);
        replay_v1(&control, &mut exact).unwrap();
        assert_eq!(exact.0, MAX_WORK);
        let mut exhausted = Work(MAX_WORK - V1_WORK + 1);
        assert_eq!(
            replay_v1(&control, &mut exhausted).err().unwrap().0,
            "unsupported_execution_work_profile"
        );
        assert_eq!(exhausted.0, MAX_WORK + 1);

        let control = upgraded(control);
        let mut work = Work::default();
        let report = v2::replay_with_work(&control, &mut work).unwrap();
        assert_eq!(work.0, V2_WORK);
        assert!(work.0 > V1_WORK);
        let mut exact = Work(MAX_WORK - V2_WORK);
        assert_eq!(
            v2::replay_with_work(&control, &mut exact)
                .unwrap()
                .canonical()
                .unwrap(),
            report.canonical().unwrap()
        );
        assert_eq!(exact.0, MAX_WORK);
        let mut exhausted = Work(MAX_WORK - V2_WORK + 1);
        assert_eq!(
            v2::replay_with_work(&control, &mut exhausted)
                .unwrap_err()
                .0,
            "unsupported_execution_work_profile"
        );
        assert_eq!(exhausted.0, MAX_WORK + 1);
    }

    pub(super) fn cancel(control: &mut Value, attempt: &str, cancellation: &str) {
        append(
            control,
            "cancel",
            object(vec![
                ("attempt_id", string(attempt)),
                ("reason", string("withdraw")),
            ])
            .unwrap(),
            object(vec![
                ("cancel_id", string(cancellation)),
                ("attempt_id", string(attempt)),
                ("reason", string("withdraw")),
                ("cancelled_at", string("0001-01-01T00:00:00+00:00")),
            ])
            .unwrap(),
        );
    }

    #[test]
    fn upgrade_report_is_closed_and_does_not_assess_backup_bytes() {
        let control = upgraded(enabled());
        let report = replay(&control).unwrap();
        fields(&report, "status revision authorization dispatchable current_source durability assessment schema_version execution_authority_id snapshot_source_id generation extensions reserved_task_ids enrollments attempts receipts graph_id current_graph_revision graphs graph_managed_tasks").unwrap();
        assert!(matches!(get(&report, "graph_id").unwrap(), Value::Null));
        assert!(
            get(&report, "revision")
                .unwrap()
                .as_str()
                .unwrap()
                .starts_with("execution-control-v2:")
        );
        assert_eq!(
            get(&report, "receipts")
                .unwrap()
                .as_object()
                .unwrap()
                .entries()
                .len(),
            2
        );
        for mode in [
            string("4095"),
            Value::Bool(true),
            integer(4096).unwrap(),
            parse("1.0"),
            parse("-1"),
        ] {
            let mut bad = control.clone();
            let mut metadata = get(&bad, "upgrade").unwrap().clone();
            metadata.set_field("source_mode", mode).unwrap();
            bad.set_field("upgrade", metadata).unwrap();
            assert!(replay(&bad).is_err());
        }
        let mut bad = control.clone();
        let mut entries = get(&bad, "history").unwrap().as_array().unwrap().to_vec();
        entries[0]
            .set_field("request_id", string("enable"))
            .unwrap();
        bad.set_field("history", Value::Array(entries)).unwrap();
        assert_eq!(
            replay(&bad).unwrap_err().0,
            "duplicate_execution_request_id"
        );
        let mut oversized = control.clone();
        oversized
            .set_field(
                "history",
                Value::Array(vec![
                    get(&control, "history").unwrap().as_array().unwrap()[0]
                        .clone();
                    MAX_HISTORY
                ]),
            )
            .unwrap();
        assert_eq!(
            replay(&oversized).unwrap_err().0,
            "unsupported_execution_history_profile"
        );
    }

    #[test]
    fn v2_standalone_preparation_uses_null_graph_binding_and_v2_domain() {
        let mut legacy = enabled();
        enroll(&mut legacy);
        let previous = "11111111111111111111111111111111";
        let cancelled = "dddddddddddddddddddddddddddddddd";
        prepare(&mut legacy, previous, Value::Null, 1);
        cancel(&mut legacy, previous, cancelled);
        let generation = get(&legacy, "history").unwrap().as_array().unwrap().len() + 2;
        let mut control = upgraded(legacy.clone());
        let revision = get(&replay(&control).unwrap(), "revision").unwrap().clone();
        prepare(
            &mut legacy,
            "ffffffffffffffffffffffffffffffff",
            string(previous),
            2,
        );
        let old_entry = get(&legacy, "history")
            .unwrap()
            .as_array()
            .unwrap()
            .last()
            .unwrap();
        let mut effect = get(old_entry, "effect").unwrap().clone();
        let mut record = get(&effect, "prepared").unwrap().clone();
        record
            .set_field("schema_version", integer(2).unwrap())
            .unwrap();
        record.set_field("graph_binding", Value::Null).unwrap();
        let mut payload = object(vec![
            ("explicit_intent", get(&record, "intent").unwrap().clone()),
            ("graph_binding", Value::Null),
        ])
        .unwrap();
        for key in [
            "execution_authority_id",
            "enrollment_id",
            "task_authority_identity",
            "task_source_revision",
            "complete_task_snapshot",
        ] {
            payload
                .set_field(key, get(&record, key).unwrap().clone())
                .unwrap();
        }
        record
            .set_field(
                "intent_digest",
                string(
                    Work::default()
                        .digest(b"coven-task-attempt-intent-v2\0", &payload)
                        .unwrap(),
                ),
            )
            .unwrap();
        effect.set_field("prepared", record.clone()).unwrap();
        let mut b = get(old_entry, "binding").unwrap().clone();
        b.set_field("schema_version", integer(2).unwrap()).unwrap();
        b.set_field("expected_execution_revision", revision)
            .unwrap();
        let mut result = get(old_entry, "result").unwrap().clone();
        result
            .set_field("generation", integer(generation).unwrap())
            .unwrap();
        result.set_field("attempt", record.clone()).unwrap();
        let entry = object(vec![
            ("generation", integer(generation).unwrap()),
            ("request_id", string("v2-prepare")),
            ("binding", b),
            ("effect", effect),
            ("result", result),
        ])
        .unwrap();
        let mut entries = get(&control, "history")
            .unwrap()
            .as_array()
            .unwrap()
            .to_vec();
        entries.push(entry);
        control
            .set_field("history", Value::Array(entries.clone()))
            .unwrap();
        control
            .set_field("generation", integer(generation).unwrap())
            .unwrap();
        let report = replay(&control).unwrap();
        let attempt = string("ffffffffffffffffffffffffffffffff");
        let transition = object(vec![
            ("cancel_id", string(cancelled)),
            ("attempt_id", attempt.clone()),
            ("reason", string("withdraw")),
            ("cancelled_at", string("0001-01-01T00:00:00+00:00")),
        ])
        .unwrap();
        let duplicate = object(vec![
            ("generation", integer(generation + 1).unwrap()),
            ("request_id", string("v2-cancel")),
            (
                "binding",
                object(vec![
                    ("schema_version", integer(2).unwrap()),
                    ("operation", string("cancel")),
                    (
                        "parameters",
                        object(vec![
                            ("attempt_id", attempt.clone()),
                            ("reason", string("withdraw")),
                        ])
                        .unwrap(),
                    ),
                    (
                        "expected_execution_revision",
                        get(&report, "revision").unwrap().clone(),
                    ),
                    ("expected_tasks_revision", Value::Null),
                ])
                .unwrap(),
            ),
            ("effect", transition.clone()),
            (
                "result",
                object(vec![
                    ("operation", string("cancel")),
                    ("generation", integer(generation + 1).unwrap()),
                    (
                        "execution_authority_id",
                        get(&control, "execution_authority_id").unwrap().clone(),
                    ),
                    ("attempt_id", attempt),
                    ("state", string("cancelled_before_submission")),
                    ("transition", transition),
                ])
                .unwrap(),
            ),
        ])
        .unwrap();
        let mut bad = control.clone();
        let mut duplicate_entries = entries.clone();
        duplicate_entries.push(duplicate);
        bad.set_field("history", Value::Array(duplicate_entries))
            .unwrap();
        bad.set_field("generation", integer(generation + 1).unwrap())
            .unwrap();
        assert_eq!(
            replay(&bad).unwrap_err().0,
            "duplicate_execution_cancellation_id"
        );
        assert_eq!(
            get(
                get(
                    get(&report, "attempts").unwrap(),
                    "ffffffffffffffffffffffffffffffff"
                )
                .unwrap(),
                "prepared"
            )
            .unwrap()
            .canonical()
            .unwrap(),
            record.canonical().unwrap()
        );
        record
            .set_field("graph_binding", object(vec![]).unwrap())
            .unwrap();
        let mut effect = get(&entries[1], "effect").unwrap().clone();
        effect.set_field("prepared", record).unwrap();
        entries[1].set_field("effect", effect).unwrap();
        control.set_field("history", Value::Array(entries)).unwrap();
        assert_eq!(
            replay(&control).unwrap_err().0,
            "execution_prepared_effect_mismatch"
        );
        let mut active = enabled();
        enroll(&mut active);
        prepare(
            &mut active,
            "ffffffffffffffffffffffffffffffff",
            Value::Null,
            1,
        );
        assert_eq!(
            replay(&upgraded(active)).unwrap_err().0,
            "execution_upgrade_has_prepared_attempts"
        );
    }
    pub(super) fn enroll(control: &mut Value) {
        let s = source_fixture(control);
        let task = &get(&s, "tasks").unwrap().as_array().unwrap()[0];
        let record = object(vec![
            ("enrollment_id", string("cccccccccccccccccccccccccccccccc")),
            ("task_id", string("task")),
            ("binding_number", integer(1).unwrap()),
            (
                "task_authority_identity",
                get(&s, "identity").unwrap().clone(),
            ),
            ("task_source_revision", get(&s, "revision").unwrap().clone()),
            ("complete_task_snapshot", task.clone()),
            (
                "task_snapshot_digest",
                string(
                    Work::default()
                        .digest(b"coven-task-snapshot-v1\0", task)
                        .unwrap(),
                ),
            ),
            ("enrolled_at", string("2024-02-29T00:00:00Z")),
        ])
        .unwrap();
        append(
            control,
            "enroll",
            parse(r#"{"task_id":"task","rebind_source":false}"#),
            object(vec![("source", s), ("enrollment", record)]).unwrap(),
        );
    }
    pub(super) fn prepare(control: &mut Value, id: &str, predecessor: Value, number: usize) {
        let s = source_fixture(control);
        let task = &get(&s, "tasks").unwrap().as_array().unwrap()[0];
        let intent = parse(
            r#"{"schema_version":1,"kind":"task-intent","task_id":"task","owner":"worker","acceptance_criteria":[{"id":"a","text":"explicit evidence"}]}"#,
        );
        let mut record = object(vec![
            ("schema_version", integer(1).unwrap()),
            ("attempt_id", string(id)),
            ("task_id", string("task")),
            ("enrollment_id", string("cccccccccccccccccccccccccccccccc")),
            ("attempt_number", integer(number).unwrap()),
            ("previous_attempt_id", predecessor),
            (
                "execution_authority_id",
                get(control, "execution_authority_id").unwrap().clone(),
            ),
            (
                "task_authority_identity",
                get(&s, "identity").unwrap().clone(),
            ),
            ("task_source_revision", get(&s, "revision").unwrap().clone()),
            ("complete_task_snapshot", task.clone()),
            (
                "task_snapshot_digest",
                string(
                    Work::default()
                        .digest(b"coven-task-snapshot-v1\0", task)
                        .unwrap(),
                ),
            ),
            ("intent", intent.clone()),
            ("prepared_at", string("2024-02-29 00:00:00.123456-00:00")),
        ])
        .unwrap();
        let mut payload = object(vec![("explicit_intent", intent.clone())]).unwrap();
        for key in [
            "execution_authority_id",
            "enrollment_id",
            "task_authority_identity",
            "task_source_revision",
            "complete_task_snapshot",
        ] {
            payload
                .set_field(key, get(&record, key).unwrap().clone())
                .unwrap();
        }
        record
            .set_field(
                "intent_digest",
                string(
                    Work::default()
                        .digest(b"coven-task-attempt-intent-v1\0", &payload)
                        .unwrap(),
                ),
            )
            .unwrap();
        append(
            control,
            "prepare",
            object(vec![("task_id", string("task")), ("intent", intent)]).unwrap(),
            object(vec![("source", s), ("prepared", record)]).unwrap(),
        );
    }
    #[test]
    fn replay_cancel_preserves_receipts_and_insertion_order_predecessor() {
        let mut control = enabled();
        enroll(&mut control);
        let first_id = "ffffffffffffffffffffffffffffffff";
        prepare(&mut control, first_id, Value::Null, 1);
        let frozen = get(&replay(&control).unwrap(), "receipts")
            .unwrap()
            .canonical()
            .unwrap();
        append(
            &mut control,
            "cancel",
            object(vec![
                ("attempt_id", string(first_id)),
                ("reason", string("withdraw")),
            ])
            .unwrap(),
            object(vec![
                ("cancel_id", string("dddddddddddddddddddddddddddddddd")),
                ("attempt_id", string(first_id)),
                ("reason", string("withdraw")),
                ("cancelled_at", string("0001-01-01T00:00:00+00:00")),
            ])
            .unwrap(),
        );
        let report = replay(&control).unwrap();
        let receipt = get(
            get(get(&report, "receipts").unwrap(), "request-3").unwrap(),
            "result",
        )
        .unwrap();
        assert_eq!(text(get(receipt, "state").unwrap()).unwrap(), "prepared");
        let cancelled = get(get(&report, "attempts").unwrap(), first_id).unwrap();
        assert_eq!(
            text(get(cancelled, "state").unwrap()).unwrap(),
            "cancelled_before_submission"
        );
        let mut prior = get(&report, "receipts")
            .unwrap()
            .as_object()
            .unwrap()
            .entries()
            .to_vec();
        prior.pop();
        assert_eq!(Value::object(prior).unwrap().canonical().unwrap(), frozen);
        assert!(
            get(&report, "reserved_task_ids")
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );
        prepare(
            &mut control,
            "11111111111111111111111111111111",
            string(first_id),
            2,
        );
        let report = replay(&control).unwrap();
        assert_eq!(
            get(&report, "reserved_task_ids")
                .unwrap()
                .canonical()
                .unwrap(),
            br#"["task"]"#
        );
        let mut bad = control.clone();
        bad.set_field("extensions", parse(r#"{"changed":true}"#))
            .unwrap();
        assert_eq!(
            replay(&bad).unwrap_err().0,
            "execution_history_revision_mismatch"
        );
        let mut bad = control.clone();
        let mut entries = get(&bad, "history").unwrap().as_array().unwrap().to_vec();
        entries[1]
            .set_field("request_id", string("enable"))
            .unwrap();
        bad.set_field("history", Value::Array(entries)).unwrap();
        assert_eq!(
            replay(&bad).unwrap_err().0,
            "duplicate_execution_request_id"
        );
    }

    #[test]
    fn ledger_source_digest_and_roster_aliases_are_checked() {
        let control = enabled();
        let mut s = source_fixture(&control);
        s.set_field("identity", parse(r#"{"kind":"task-ledger-v1","workspace_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","epoch_id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","path":"state/task-ledger"}"#)).unwrap();
        let hash = Work::default()
            .digest(b"", get(&s, "tasks").unwrap())
            .unwrap();
        s.set_field("revision", string(format!("tasks-ledger:v1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:0:{hash}"))).unwrap();
        let snapshot = get(&control, "snapshot_source_id").unwrap();
        source(&s, snapshot, &mut Work::default()).unwrap();
        s.set_field("tasks", Value::Array(vec![])).unwrap();
        assert_eq!(
            source(&s, snapshot, &mut Work::default()).unwrap_err().0,
            "execution_ledger_digest_mismatch"
        );
        let mut s = source_fixture(&control);
        s.set_field("roster", parse(r#"["Worker","-worker-"]"#))
            .unwrap();
        assert_eq!(
            source(&s, snapshot, &mut Work::default()).unwrap_err().0,
            "ambiguous_team_alias"
        );
    }

    #[test]
    fn root_needs_no_marker_and_present_controls_never_disable() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            history(temp.path()).unwrap().canonical().unwrap(),
            envelope("disabled", DISABLED.to_owned())
                .unwrap()
                .canonical()
                .unwrap()
        );
        let directory = temp.path().join("state/execution");
        fs::create_dir_all(&directory).unwrap();
        assert_eq!(
            history(temp.path()).unwrap_err().0,
            "pending_execution_authority"
        );
        fs::write(directory.join("control.json"), b"null").unwrap();
        assert!(history(temp.path()).is_err());
        fs::write(directory.join("control.json"), enabled().storage().unwrap()).unwrap();
        assert_eq!(
            text(get(&history(temp.path()).unwrap(), "status").unwrap()).unwrap(),
            "active"
        );
    }

    #[cfg(unix)]
    #[test]
    fn managed_directories_and_control_reject_symlinks_and_wrong_types() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), temp.path().join("state")).unwrap();
        assert!(history(temp.path()).is_err());
        fs::remove_file(temp.path().join("state")).unwrap();
        fs::create_dir(temp.path().join("state")).unwrap();
        let directory = temp.path().join("state/execution");
        symlink(outside.path(), &directory).unwrap();
        assert!(history(temp.path()).is_err());
        fs::remove_file(&directory).unwrap();
        fs::write(&directory, b"not a directory").unwrap();
        assert!(history(temp.path()).is_err());
        fs::remove_file(&directory).unwrap();
        fs::create_dir(&directory).unwrap();
        let control = directory.join("control.json");
        symlink(outside.path().join("absent"), &control).unwrap();
        assert!(history(temp.path()).is_err());
        fs::remove_file(&control).unwrap();
        fs::create_dir(&control).unwrap();
        assert!(history(temp.path()).is_err());
        fs::remove_dir(&control).unwrap();
        // A FIFO must be rejected without waiting for a writer.
        let path =
            std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(control.as_os_str()))
                .unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert!(history(temp.path()).is_err());
    }
}
