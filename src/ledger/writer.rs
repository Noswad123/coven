//! Guarded task-frame publication. Receipt-first retry and secondary delivery
//! policy belong to the task service, not this low-level writer.
use super::{
    publication::{self, ArtifactRole, Publication},
    *,
};
use crate::{
    execution, platform,
    storage::{self, Delivery, WorkspaceGuard},
    task_admission,
};

pub(crate) struct LedgerCommit {
    pub view: View,
    pub delivery: Delivery,
}

trait WriterIo {
    fn validate(&self, workspace: &Path, guard: &WorkspaceGuard) -> Result<()> {
        guard.validate_for(workspace)
    }
    fn reload(&self, root: &Path) -> Result<Option<View>> {
        super::read(root)
    }
    fn projection(&self, root: &Path, view: &View) -> Result<Value> {
        projection_status(root, view)
    }
    fn execution(&self, root: &Path, task: &str, guard: &WorkspaceGuard) -> Result<()> {
        match execution::guard_task_mutation(root, task, guard)? {
            execution::ReservationCheck::DisabledObserved
            | execution::ReservationCheck::ActiveConfirmedClear => Ok(()),
        }
    }
    fn id(&self) -> Result<String> {
        platform::random_uuid_hex()
    }
    fn publish(
        &self,
        guard: &WorkspaceGuard,
        role: ArtifactRole<'_>,
        raw: &[u8],
    ) -> Result<Publication> {
        publication::publish(guard, role, raw, None)
    }
    fn replace(&self, guard: &WorkspaceGuard, raw: &[u8]) -> Result<Delivery> {
        storage::atomic_replace_bytes(guard, Path::new("state/tasks.json"), raw)
    }
}
struct SystemIo;
impl WriterIo for SystemIo {}

fn current(root: &Path, expected: &View, io: &impl WriterIo) -> Result<View> {
    let actual = io.reload(root)?.ok_or(Error("ledger_head_changed"))?;
    require(
        actual.revision == expected.revision && actual.head_digest == expected.head_digest,
        "ledger_head_changed",
    )?;
    Ok(actual)
}

pub(crate) fn commit_task(
    workspace: &Path,
    expected: &View,
    key: &RequestKey,
    binding: &RequestBinding,
    task: Value,
    guard: &WorkspaceGuard,
) -> Result<LedgerCommit> {
    commit_with(workspace, expected, key, binding, task, guard, &SystemIo)
}

fn commit_with(
    workspace: &Path,
    expected: &View,
    key: &RequestKey,
    binding: &RequestBinding,
    task: Value,
    guard: &WorkspaceGuard,
    io: &impl WriterIo,
) -> Result<LedgerCommit> {
    io.validate(workspace, guard)?;
    let root = guard.root();
    let mut actual = current(root, expected, io)?;
    require(
        actual.lookup(key, binding)?.is_none(),
        "request_key_already_committed",
    )?;
    let binding = values::parse(&binding.0, Profile::Authority)?;
    let expected_revision = binding_revision(&binding)?;
    require(
        null(expected_revision) || eq_text(expected_revision, &actual.revision),
        "ledger_expected_revision_mismatch",
    )?;
    require(
        eq_text(get(&io.projection(root, &actual)?, "status")?, "current"),
        "ledger_projection_not_current",
    )?;
    io.execution(root, string(get(&task, "id")?)?, guard)?;
    let sequence = actual
        .sequence
        .checked_add(1)
        .ok_or(Error("unsupported_ledger_history_profile"))?;
    require(sequence < MAX_FRAMES, "unsupported_ledger_history_profile")?;
    let (operation, args) = super::binding(&binding)?;
    // Only the proposed task list is copied. The receipt map stays owned by the
    // fresh replay, which is discarded if any candidate validation fails.
    let mut tasks = actual.tasks.clone();
    if operation == "add" {
        tasks.array_mut()?.push(task.clone());
    } else {
        let id = string(arg(args, "task_id")?)?;
        for item in tasks.array_mut()? {
            if task_field(item, "id").is_some_and(|value| eq_text(value, id)) {
                *item = task.clone();
            }
        }
    }
    let after = revision(&actual.workspace_id, &actual.epoch_id, sequence, &tasks)?;
    drop(tasks);
    let result = Value::object(vec![
        ("outcome".into(), Value::String("committed".into())),
        ("mutation_id".into(), Value::String(io.id()?)),
        ("task".into(), task),
        (
            "before_revision".into(),
            Value::String(actual.revision.clone()),
        ),
        ("after_revision".into(), Value::String(after)),
        ("sequence".into(), Value::integer(&sequence.to_string())?),
    ])?;
    let frame = Value::object(vec![
        ("version".into(), Value::integer("1")?),
        (
            "workspace_id".into(),
            Value::String(actual.workspace_id.clone()),
        ),
        ("epoch_id".into(), Value::String(actual.epoch_id.clone())),
        ("sequence".into(), Value::integer(&sequence.to_string())?),
        (
            "previous_digest".into(),
            Value::String(actual.head_digest.clone()),
        ),
        ("request_id".into(), Value::String(key.0.clone())),
        ("binding".into(), binding),
        ("result".into(), result),
    ])?;
    let encoded = frame.canonical()?;
    let mut raw = Value::object(vec![
        ("version".into(), Value::integer("1")?),
        ("length".into(), Value::integer(&encoded.len().to_string())?),
        ("sha256".into(), Value::String(canonical::digest(&encoded))),
        ("payload".into(), frame),
    ])?
    .canonical()?;
    raw.push(b'\n');
    let total = history_bytes(actual.history_bytes, raw.len())?;
    // Reparse actual canonical bytes, so new task object order agrees with Python
    // while untouched baseline records retain their original storage order.
    actual.apply(payload(&raw)?, canonical::digest(&raw))?;
    actual.history_bytes = total;
    admit(&actual, key)?;
    let delivery = match io.publish(
        guard,
        ArtifactRole::Frame {
            epoch: &actual.epoch_id,
            sequence,
        },
        &raw,
    )? {
        Publication::Synced => Delivery::Committed,
        Publication::SyncUncertain => Delivery::DurabilityUncertain,
    };
    Ok(LedgerCommit {
        view: actual,
        delivery,
    })
}

fn admit(view: &View, key: &RequestKey) -> Result<()> {
    let projection = view.projection_bytes()?;
    require(
        projection.len() <= values::MAX_OUTPUT,
        "unsupported_replacement_size",
    )?;
    require(
        projection.len() <= MAX_PROJECTION_BYTES,
        "unsupported_projection_profile",
    )?;
    let receipt = view
        .receipts
        .get(&key.0)
        .ok_or(Error("missing_ledger_receipt"))?;
    task_admission::response(receipt.result.value())?;
    let snapshot = Value::String(snapshot_revision(Some(&projection)));
    // Same fields as View::report, without reading any secondary artifact.
    let report = Value::object(vec![
        ("authority".into(), Value::String("task-ledger-v1".into())),
        ("revision".into(), Value::String(view.revision.clone())),
        ("tasks".into(), view.tasks.clone()),
        (
            "workspace_id".into(),
            Value::String(view.workspace_id.clone()),
        ),
        ("epoch_id".into(), Value::String(view.epoch_id.clone())),
        (
            "sequence".into(),
            Value::integer(&view.sequence.to_string())?,
        ),
        (
            "frame_digest".into(),
            Value::String(view.head_digest.clone()),
        ),
        (
            "projection".into(),
            Value::object(vec![
                ("status".into(), Value::String("current".into())),
                ("expected_revision".into(), snapshot.clone()),
                ("observed_revision".into(), snapshot),
            ])?,
        ),
    ])?;
    task_admission::response(&report)
}

pub(crate) fn publish_projection(
    workspace: &Path,
    expected: &View,
    guard: &WorkspaceGuard,
) -> Result<Delivery> {
    projection_with(workspace, expected, guard, &SystemIo)
}

fn projection_with(
    workspace: &Path,
    expected: &View,
    guard: &WorkspaceGuard,
    io: &impl WriterIo,
) -> Result<Delivery> {
    io.validate(workspace, guard)?;
    let actual = current(guard.root(), expected, io)?;
    let raw = actual.projection_bytes()?;
    let path = read_fs::managed(guard.root(), Path::new("state/tasks.json"), false)?;
    require(
        fs::symlink_metadata(path.parent().ok_or(Error("invalid_replacement_parent"))?)?.is_dir(),
        "unsafe_replacement_parent",
    )?;
    io.replace(guard, &raw)
}

pub(crate) fn projection_status(root: &Path, view: &View) -> Result<Value> {
    projection_health(root, &view.projection_bytes()?)
}

pub(crate) fn event_fields(view: &View) -> Result<Value> {
    Value::object(vec![
        ("epoch_id".into(), Value::String(view.epoch_id.clone())),
        (
            "sequence".into(),
            Value::integer(&view.sequence.to_string())?,
        ),
        (
            "frame_digest".into(),
            Value::String(view.head_digest.clone()),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, process::Command, time::Duration};

    fn json(raw: &str) -> Value {
        values::parse(raw.as_bytes(), Profile::Authority).unwrap()
    }
    fn task() -> Value {
        json(
            r#"{"id":"task","title":"Task","status":"pending","owner":null,"notes":"","created_at":"then","updated_at":"then","extension":[123456789012345678901234567890,-0.0,1e-7,"\u03bb\ud83e\udd8a"]}"#,
        )
    }
    fn fixture() -> (tempfile::TempDir, WorkspaceGuard, View) {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("coven.json"), b"marker").unwrap();
        let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap();
        let raw =
            br#" [ { "title":"Old", "id":"old", "status":"pending", "extension":{"z":1,"a":2} } ]
"#;
        fs::write(root.path().join("state/tasks.json"), raw).unwrap();
        let view = import_tasks(root.path(), &snapshot_revision(Some(raw)), &guard)
            .unwrap()
            .into_view();
        (root, guard, view)
    }

    #[derive(Default)]
    struct Spy {
        trace: RefCell<Vec<&'static str>>,
        fail: Option<&'static str>,
        change: Option<&'static str>,
        uncertain: bool,
        raw: RefCell<Vec<u8>>,
    }
    impl Spy {
        fn step(&self, name: &'static str) -> Result<()> {
            self.trace.borrow_mut().push(name);
            require(self.fail != Some(name), "writer_fault")
        }
    }
    impl WriterIo for Spy {
        fn validate(&self, workspace: &Path, guard: &WorkspaceGuard) -> Result<()> {
            self.step("guard")?;
            guard.validate_for(workspace)
        }
        fn reload(&self, root: &Path) -> Result<Option<View>> {
            self.step("reload")?;
            let mut view = super::super::read(root)?.unwrap();
            match self.change {
                Some("head") => view.head_digest = "f".repeat(64),
                Some("revision") => view.revision.push('x'),
                Some("frames") => view.sequence = MAX_FRAMES - 1,
                Some("history") => view.history_bytes = MAX_HISTORY_BYTES,
                Some("receipts") => view.receipt_bytes = MAX_RECEIPT_BYTES,
                Some("missing") => return Ok(None),
                _ => {}
            }
            Ok(Some(view))
        }
        fn projection(&self, root: &Path, view: &View) -> Result<Value> {
            self.step("projection")?;
            SystemIo.projection(root, view)
        }
        fn execution(&self, root: &Path, task: &str, guard: &WorkspaceGuard) -> Result<()> {
            self.step("execution")?;
            SystemIo.execution(root, task, guard)
        }
        fn id(&self) -> Result<String> {
            self.step("id")?;
            Ok("d".repeat(32))
        }
        fn publish(
            &self,
            guard: &WorkspaceGuard,
            role: ArtifactRole<'_>,
            raw: &[u8],
        ) -> Result<Publication> {
            self.step("publish")?;
            *self.raw.borrow_mut() = raw.to_vec();
            let delivery = SystemIo.publish(guard, role, raw)?;
            Ok(if self.uncertain {
                Publication::SyncUncertain
            } else {
                delivery
            })
        }
        fn replace(&self, guard: &WorkspaceGuard, raw: &[u8]) -> Result<Delivery> {
            self.step("replace")?;
            *self.raw.borrow_mut() = raw.to_vec();
            SystemIo.replace(guard, raw)
        }
    }
    fn run(
        expected: &View,
        guard: &WorkspaceGuard,
        binding: &RequestBinding,
        io: &Spy,
    ) -> Result<LedgerCommit> {
        commit_with(
            guard.root(),
            expected,
            &RequestKey::parse("add:1").unwrap(),
            binding,
            task(),
            guard,
            io,
        )
    }
    fn error(result: Result<LedgerCommit>) -> &'static str {
        match result {
            Err(error) => error.0,
            Ok(_) => panic!("expected rejection"),
        }
    }

    #[test]
    fn guarded_protocol_order_and_prepublication_faults() {
        let binding = RequestBinding::add("Task", None, "", None).unwrap();
        let steps = [
            "guard",
            "reload",
            "projection",
            "execution",
            "id",
            "publish",
        ];
        for (index, step) in steps.iter().enumerate() {
            let (_root, guard, view) = fixture();
            let io = Spy {
                fail: Some(step),
                ..Spy::default()
            };
            assert_eq!(error(run(&view, &guard, &binding, &io)), "writer_fault");
            assert_eq!(*io.trace.borrow(), steps[..=index]);
            let actual = super::super::read(guard.root()).unwrap().unwrap();
            assert_eq!(actual.sequence, 0);
            assert!(
                !guard
                    .root()
                    .join(DIRECTORY)
                    .join(&view.epoch_id)
                    .join("frames/00000000000000000001.frame")
                    .exists()
            );
        }
    }

    #[test]
    fn complete_reload_requires_both_revision_and_digest() {
        let (_root, guard, view) = fixture();
        let binding = RequestBinding::add("Task", None, "", None).unwrap();
        for change in ["head", "revision", "missing"] {
            let io = Spy {
                change: Some(change),
                ..Spy::default()
            };
            assert_eq!(
                error(run(&view, &guard, &binding, &io)),
                "ledger_head_changed"
            );
            assert_eq!(*io.trace.borrow(), ["guard", "reload"]);
            assert_eq!(
                projection_with(guard.root(), &view, &guard, &io)
                    .unwrap_err()
                    .code(),
                "ledger_head_changed"
            );
        }
        let other = tempfile::tempdir().unwrap();
        let io = Spy::default();
        assert!(
            commit_with(
                other.path(),
                &view,
                &RequestKey::parse("k").unwrap(),
                &binding,
                task(),
                &guard,
                &io
            )
            .is_err()
        );
        assert_eq!(*io.trace.borrow(), ["guard"]);
    }

    #[test]
    fn low_level_cas_precedes_projection_but_lookup_precedes_cas() {
        let (root, guard, view) = fixture();
        let binding = RequestBinding::add("Task", None, "", Some(&view.revision)).unwrap();
        let committed = run(&view, &guard, &binding, &Spy::default()).unwrap();
        assert_eq!(committed.delivery, Delivery::Committed);
        // Projection is now stale and the original binding CAS is stale too.
        let io = Spy::default();
        assert_eq!(
            error(run(&committed.view, &guard, &binding, &io)),
            "request_key_already_committed"
        );
        assert_eq!(*io.trace.borrow(), ["guard", "reload"]);
        let changed = RequestBinding::add("Task", None, "changed", None).unwrap();
        assert_eq!(
            error(run(&committed.view, &guard, &changed, &Spy::default())),
            "request_key_binding_conflict"
        );
        let io = Spy::default();
        let fresh = RequestKey::parse("fresh").unwrap();
        assert_eq!(
            error(commit_with(
                root.path(),
                &committed.view,
                &fresh,
                &binding,
                task(),
                &guard,
                &io
            )),
            "ledger_expected_revision_mismatch"
        );
        assert_eq!(*io.trace.borrow(), ["guard", "reload"]);
        let io = Spy::default();
        assert_eq!(
            error(commit_with(
                root.path(),
                &committed.view,
                &fresh,
                &changed,
                task(),
                &guard,
                &io
            )),
            "ledger_projection_not_current"
        );
        assert_eq!(*io.trace.borrow(), ["guard", "reload", "projection"]);
    }

    #[test]
    fn uncertain_publication_returns_known_candidate_without_secondaries() {
        let (_root, guard, view) = fixture();
        let binding = RequestBinding::add("Task", None, "", None).unwrap();
        let io = Spy {
            uncertain: true,
            ..Spy::default()
        };
        let committed = run(&view, &guard, &binding, &io).unwrap();
        assert_eq!(committed.delivery, Delivery::DurabilityUncertain);
        assert_eq!(
            *io.trace.borrow(),
            [
                "guard",
                "reload",
                "projection",
                "execution",
                "id",
                "publish"
            ]
        );
        let key = RequestKey::parse("add:1").unwrap();
        let frozen = committed
            .view
            .lookup(&key, &binding)
            .unwrap()
            .unwrap()
            .value()
            .canonical()
            .unwrap();
        let replay = super::super::read(guard.root()).unwrap().unwrap();
        assert_eq!(
            replay
                .lookup(&key, &binding)
                .unwrap()
                .unwrap()
                .value()
                .canonical()
                .unwrap(),
            frozen
        );
        assert_eq!(
            replay.history_bytes,
            view.history_bytes + io.raw.borrow().len()
        );
        assert_eq!(
            get(
                &projection_status(guard.root(), &committed.view).unwrap(),
                "status"
            )
            .unwrap()
            .as_str(),
            Some("diverged")
        );
        assert_eq!(
            event_fields(&committed.view)
                .unwrap()
                .as_object()
                .unwrap()
                .entries()
                .len(),
            3
        );
    }

    #[test]
    fn candidate_replay_and_retention_fail_before_publication() {
        let (_root, guard, view) = fixture();
        let binding = RequestBinding::add("Task", None, "", None).unwrap();
        for (change, code) in [
            ("frames", "unsupported_ledger_history_profile"),
            ("history", "unsupported_ledger_history_bytes"),
            ("receipts", "unsupported_ledger_receipt_bytes"),
        ] {
            let io = Spy {
                change: Some(change),
                ..Spy::default()
            };
            assert_eq!(error(run(&view, &guard, &binding, &io)), code);
            assert!(!io.trace.borrow().contains(&"publish"));
        }
        let io = Spy::default();
        let bad = RequestBinding::add("Wrong", None, "", None).unwrap();
        assert_eq!(
            error(run(&view, &guard, &bad, &io)),
            "invalid_ledger_add_effect"
        );
        assert!(!io.trace.borrow().contains(&"publish"));
        assert_eq!(
            super::super::read(guard.root())
                .unwrap()
                .unwrap()
                .tasks
                .storage()
                .unwrap(),
            view.tasks.storage().unwrap()
        );
    }

    #[test]
    fn admission_checks_full_list_ascii_reserve_and_projection_storage() {
        let (_root, guard, view) = fixture();
        let binding = RequestBinding::add("Task", None, "", None).unwrap();
        let mut candidate = run(&view, &guard, &binding, &Spy::default()).unwrap().view;
        let key = RequestKey::parse("add:1").unwrap();
        let mut extensions = vec![Value::String("\u{e9}".repeat(values::MAX_STRING / 2)); 10];
        let remainder = values::MAX_OUTPUT / 3 - 10 * values::MAX_STRING - 1024;
        extensions.push(Value::String("\u{e9}".repeat(remainder / 2)));
        candidate.tasks.array_mut().unwrap()[0]
            .set_field("large", Value::Array(extensions))
            .unwrap();
        // Storage and compact UTF-8 fit, but the full list's ASCII response
        // consumes the reserved delivery/warning headroom. The receipt is small.
        assert!(candidate.projection_bytes().unwrap().len() < values::MAX_OUTPUT);
        assert_eq!(
            admit(&candidate, &key).unwrap_err().code(),
            "unsupported_task_response_profile"
        );
        let extensions = vec![Value::String("x".repeat(values::MAX_STRING)); 32];
        candidate.tasks.array_mut().unwrap()[0]
            .set_field("large", Value::Array(extensions))
            .unwrap();
        assert_eq!(
            admit(&candidate, &key).unwrap_err().code(),
            "unsupported_json_output_profile"
        );
    }

    #[test]
    fn projection_uses_fresh_view_not_caller_projection_bytes() {
        let (root, guard, mut view) = fixture();
        let expected = view.projection_bytes().unwrap();
        view.baseline_projection = Some(b"caller must not choose bytes".to_vec());
        let io = Spy::default();
        projection_with(root.path(), &view, &guard, &io).unwrap();
        assert_eq!(*io.raw.borrow(), expected);
        assert_eq!(*io.trace.borrow(), ["guard", "reload", "replace"]);
    }

    #[test]
    fn assign_replays_complete_extensions_and_rejects_unrelated_changes() {
        let (_root, guard, view) = fixture();
        let binding = RequestBinding::assign("old", " WORKER ", Some(&view.revision)).unwrap();
        let mut assigned = view.tasks.as_array().unwrap()[0].clone();
        assigned
            .set_field("owner", Value::String("worker".into()))
            .unwrap();
        assigned
            .set_field("status", Value::String("assigned".into()))
            .unwrap();
        assigned
            .set_field("updated_at", Value::String("later".into()))
            .unwrap();
        let key = RequestKey::parse("assign:1").unwrap();
        let mut invalid = assigned.clone();
        invalid.set_field("extension", Value::Null).unwrap();
        let io = Spy::default();
        assert_eq!(
            error(commit_with(
                guard.root(),
                &view,
                &key,
                &binding,
                invalid,
                &guard,
                &io
            )),
            "invalid_ledger_assign_effect"
        );
        assert!(!io.trace.borrow().contains(&"publish"));
        let committed = commit_with(
            guard.root(),
            &view,
            &key,
            &binding,
            assigned.clone(),
            &guard,
            &Spy::default(),
        )
        .unwrap();
        assert_eq!(
            committed.view.tasks.as_array().unwrap()[0]
                .canonical()
                .unwrap(),
            assigned.canonical().unwrap()
        );
        assert_eq!(
            get(
                committed
                    .view
                    .lookup(&key, &binding)
                    .unwrap()
                    .unwrap()
                    .value(),
                "task"
            )
            .unwrap()
            .canonical()
            .unwrap(),
            assigned.canonical().unwrap()
        );
    }

    #[test]
    fn canonical_frame_and_projection_match_python_with_deterministic_mutation_id() {
        let (root, guard, view) = fixture();
        let binding = RequestBinding::add("Task", None, "", Some(&view.revision)).unwrap();
        let output = Command::new("python3")
            .args([
                "-c",
                r#"
import json, sys
from runner import task_ledger as l
v = l.LedgerStore(sys.argv[1]).read()
b, t = json.loads(sys.argv[2]), json.loads(sys.argv[3])
s = v.sequence + 1
r = dict(outcome='committed', mutation_id='d'*32, task=t, before_revision=v.revision,
         after_revision=l._revision(v.workspace_id, v.epoch_id, s, v.tasks + [t]), sequence=s)
p = dict(version=1, workspace_id=v.workspace_id, epoch_id=v.epoch_id, sequence=s,
         previous_digest=v.head_digest, request_id='add:1', binding=b, result=r)
raw = l._envelope(p)
c = l._apply(v, l._payload(raw), l._digest(raw))
print(json.dumps(dict(frame=raw.decode(), projection=c.projection_bytes.decode())))
"#,
            ])
            .arg(root.path())
            .arg(std::str::from_utf8(&binding.0).unwrap())
            .arg(String::from_utf8(task().compact().unwrap()).unwrap())
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let python = values::parse(&output.stdout, Profile::Authority).unwrap();
        let io = Spy::default();
        let committed = run(&view, &guard, &binding, &io).unwrap();
        assert_eq!(
            *io.raw.borrow(),
            get(&python, "frame").unwrap().as_str().unwrap().as_bytes()
        );
        assert_eq!(
            committed.view.projection_bytes().unwrap(),
            get(&python, "projection")
                .unwrap()
                .as_str()
                .unwrap()
                .as_bytes()
        );
        let io = Spy::default();
        assert_eq!(
            projection_with(root.path(), &committed.view, &guard, &io).unwrap(),
            Delivery::Committed
        );
        assert_eq!(*io.trace.borrow(), ["guard", "reload", "replace"]);
        assert_eq!(
            fs::read(root.path().join("state/tasks.json")).unwrap(),
            committed.view.projection_bytes().unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn projection_never_replaces_unsafe_targets() {
        use std::os::unix::fs::symlink;
        let (root, guard, view) = fixture();
        let path = root.path().join("state/tasks.json");
        fs::remove_file(&path).unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), b"untouched").unwrap();
        symlink(outside.path(), &path).unwrap();
        assert!(publish_projection(root.path(), &view, &guard).is_err());
        assert_eq!(fs::read(outside.path()).unwrap(), b"untouched");
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(publish_projection(root.path(), &view, &guard).is_err());
        assert!(path.is_dir());
    }
}
