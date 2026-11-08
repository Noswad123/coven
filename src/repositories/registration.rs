use super::{
    record::{self, Authority, Record, Request},
    *,
};
use crate::{
    error::require,
    storage::{self, Delivery, WorkspaceGuard},
};
use std::time::Duration;

pub(super) trait RegistrationIo {
    fn lock(&self, root: &Path) -> Result<WorkspaceGuard> {
        record::workspace(root)?;
        WorkspaceGuard::acquire(root, Duration::from_secs(5))
    }
    fn structure(&self, root: &Path) -> Result<()> {
        record::workspace(root).map(|_| ())
    }
    fn read(&self, root: &Path) -> Result<Option<Authority>> {
        record::read(root)
    }
    fn replace(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        storage::atomic_replace_bytes(guard, Path::new(AUTHORITY), bytes)
    }
    fn confirm(&self, guard: &WorkspaceGuard, authority: &Authority) -> Result<bool> {
        self.structure(guard.root())?;
        let current = self
            .read(guard.root())?
            .ok_or(crate::Error("repository_confirmation_unavailable"))?;
        require(
            current.bytes == authority.bytes && current.record == authority.record,
            "repository_confirmation_changed",
        )?;
        Ok(
            storage::confirm_existing(guard, Path::new(AUTHORITY), &authority.bytes)?
                == Delivery::Committed,
        )
    }
}
pub(super) struct SystemIo;
impl RegistrationIo for SystemIo {}

// All possible known-result reports are admitted before publication/confirmation.
// After that boundary only selection is allowed, never fallible serialization.
struct Prepared {
    committed: TaskOutput,
    uncertain: TaskOutput,
}
impl Prepared {
    fn new(
        authority: &Authority,
        replayed: bool,
        observations: &Value,
        compact: bool,
    ) -> Result<Self> {
        let mut value = envelope(Some(authority));
        value["replayed"] = json!(replayed);
        value["observations"] = observations.clone();
        value["delivery"] = json!({"outcome":"durably_committed"});
        let committed = render(&mut value, compact, Some(authority))?;
        value["delivery"]["outcome"] = json!("durability_uncertain");
        let uncertain = render(&mut value, compact, Some(authority))?;
        Ok(Self {
            committed,
            uncertain,
        })
    }
    fn select(self, confirmed: bool) -> TaskOutput {
        if confirmed {
            self.committed
        } else {
            self.uncertain
        }
    }
}

fn replay(
    authority: &Authority,
    root: &Path,
    compact: bool,
    io: &impl RegistrationIo,
) -> Result<TaskOutput> {
    let prepared = Prepared::new(authority, true, &json!({"status":"not_assessed"}), compact)?;
    let confirmed = (|| {
        let guard = io.lock(root)?;
        io.confirm(&guard, authority)
    })()
    .unwrap_or(false);
    Ok(prepared.select(confirmed))
}

pub(super) fn register(
    workspace: &Path,
    request: Request,
    compact: bool,
    runner: &impl crate::process::ProbeRunner,
    io: &impl RegistrationIo,
) -> Result<TaskOutput> {
    let root = record::workspace(workspace)?;
    if let Some(authority) = io.read(&root)? {
        require(
            authority.record.original_request == request,
            "repository_immutable_request_conflict",
        )?;
        return replay(&authority, &root, compact, io);
    }
    require(
        request.expected_revision == UNREGISTERED,
        "repository_revision_conflict",
    )?;
    let (frozen, _) = git::observe(&request, runner)?;
    let guard = io.lock(&root)?;
    io.structure(&root)?;
    if let Some(authority) = io.read(&root)? {
        require(
            authority.record.original_request == request,
            "repository_immutable_request_conflict",
        )?;
        let prepared = Prepared::new(&authority, true, &json!({"status":"not_assessed"}), compact)?;
        return Ok(prepared.select(io.confirm(&guard, &authority).unwrap_or(false)));
    }
    let (checked, observations) = git::observe(&request, runner)?;
    require(checked == frozen, "repository_changed_before_publication")?;
    let record = Record {
        schema_version: 1,
        kind: "repository-registration".to_owned(),
        registration_id: crate::task_time::uuid_hyphenated()?,
        frozen,
        registered_at: crate::task_time::now()?,
        original_request: request,
    };
    let mut bytes = serde_json::to_vec_pretty(&record)?;
    bytes.push(b'\n');
    let authority = Authority::decode(bytes)?;
    let prepared = Prepared::new(&authority, false, &observations, compact)?;
    // Admit replay forms as well, so a stored receipt always remains renderable.
    let _ = Prepared::new(&authority, true, &json!({"status":"not_assessed"}), compact)?;
    let delivery = io.replace(&guard, &authority.bytes)?;
    let confirmed =
        delivery == Delivery::Committed && io.confirm(&guard, &authority).unwrap_or(false);
    Ok(prepared.select(confirmed))
}
