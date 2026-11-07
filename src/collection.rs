//! Explicit unverified handoff capture, not task mutation or native admission.
//! Context intentionally does not load execution/runtime controls or task CLI
//! preflight: their authority cannot grant (or block) local evidence capture.
use crate::{
    Error, Result,
    archive::{self, ClaimReport, TaskBinding},
    canonical,
    error::require,
    handoff::{self, Digest, TaskRevision, ValidatedHandoff},
    ledger, read_fs,
    records::{TaskRecords, TeamRecord},
    storage::{self, Delivery, EvidenceIo, SystemEvidenceIo, WorkspaceGuard},
    strict_json,
    values::{self, Profile, Value},
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Serialize)]
pub struct CollectionDetails {
    collection_id: String,
    handoff_sha256: String,
    handoff_bytes: usize,
    task_binding: TaskBinding,
    artifact_path: String,
    publication: &'static str,
    delivery: Delivery,
    effect: &'static str,
}
#[derive(Serialize)]
#[serde(transparent)]
pub struct CollectedReport(ClaimReport<CollectionDetails>);
impl CollectedReport {
    pub fn delivery(&self) -> Delivery {
        self.0.details().delivery
    }
}

fn strict_context(raw: &[u8]) -> Result<Value> {
    strict_json::check_depth(raw, 16)?;
    values::parse(raw, Profile::Authority)
}

fn context(root: &Path, input: &ValidatedHandoff, expected: &TaskRevision) -> Result<TaskBinding> {
    read_fs::managed(root, Path::new("state"), true)?;
    let team_path = read_fs::managed(root, Path::new("coven.json"), false)?;
    let team = TeamRecord::validate(strict_context(&read_fs::read_regular(&team_path, 65536)?)?)?;
    let (revision, tasks) = if let Some(view) = ledger::read(root)? {
        view.into_task_values()
    } else {
        require(!ledger::pending(root)?, "pending_task_authority")?;
        let path = read_fs::managed(root, Path::new("state/tasks.json"), false)?;
        let raw = read_fs::read_regular(&path, 1_048_576)?;
        let records = TaskRecords::validate(strict_context(&raw)?)?;
        (ledger::snapshot_revision(Some(&raw)), records.into_value())
    };
    let task = input.task();
    require(
        revision == expected.as_str() && revision == task.revision.as_str(),
        "stale_handoff_task_revision",
    )?;
    let tasks = tasks.as_array().ok_or(Error("invalid_task_records"))?;
    let matches = tasks
        .iter()
        .filter(|value| {
            value
                .as_object()
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
                == Some(task.id.as_str())
        })
        .collect::<Vec<_>>();
    require(matches.len() == 1, "handoff_task_not_found")?;
    let selected = matches[0];
    require(
        selected
            .as_object()
            .and_then(|value| value.get("owner"))
            .and_then(Value::as_str)
            == Some(task.owner.as_str())
            && team.agent_ids()?.contains(&task.owner.as_str()),
        "handoff_owner_mismatch",
    )?;
    let mut encoded = b"coven.handoff-task.v1\0".to_vec();
    encoded.extend_from_slice(&selected.canonical()?);
    Ok(TaskBinding {
        task_id: task.id.clone(),
        owner: task.owner.clone(),
        revision: task.revision.clone(),
        task_sha256: Digest::parse(&canonical::digest(&encoded))?,
    })
}

struct Prepared {
    root: PathBuf,
    raw: Vec<u8>,
    input: ValidatedHandoff,
    digest: Digest,
    revision: TaskRevision,
}
impl Prepared {
    fn read(
        workspace: &Path,
        source: &Path,
        expected_sha256: &str,
        expected_revision: &str,
    ) -> Result<Self> {
        let digest = Digest::parse(expected_sha256)?;
        let revision = TaskRevision::parse(expected_revision)?;
        let raw = read_fs::read_regular(source, handoff::MAX_INPUT)?;
        let input = handoff::decode(&raw)?;
        require(
            canonical::digest(&raw) == digest.as_str(),
            "handoff_input_changed",
        )?;
        let root = read_fs::workspace(workspace)?;
        context(&root, &input, &revision)?;
        archive::scan(&root)?;
        Ok(Self {
            root,
            raw,
            input,
            digest,
            revision,
        })
    }
    fn finish(self, io: &impl EvidenceIo) -> Result<CollectedReport> {
        let guard = WorkspaceGuard::acquire(&self.root, Duration::from_secs(5))?;
        self.publish(&guard, io)
    }
    fn publish(self, guard: &WorkspaceGuard, io: &impl EvidenceIo) -> Result<CollectedReport> {
        guard.validate_for(&self.root)?;
        let binding = context(&self.root, &self.input, &self.revision)?;
        let (identity, encoded) = archive::capture_bytes(&self.raw, binding.clone(), &self.root)?;
        let relative = Path::new("output/handoffs").join(format!("{identity}.json"));
        let target = read_fs::managed(&self.root, &relative, false)?;
        let scan = archive::scan(&self.root)?;
        let replayed = target.exists();
        let observed = if replayed {
            Some(storage::observe_existing(guard, &relative, &encoded)?)
        } else {
            require(
                scan.count + 2 <= archive::MAX_ENTRIES
                    && scan.total + 2 * encoded.len() as u64 <= archive::MAX_RETAINED,
                "handoff_archive_capacity",
            )?;
            None
        };
        let prepare = || -> Result<()> {
            for directory in ["output", "output/handoffs"] {
                storage::prepare_directory_with(guard, Path::new(directory), io)?;
            }
            Ok(())
        };
        let delivery = if replayed {
            match prepare() {
                Ok(()) => storage::confirm_observed_with(
                    guard,
                    observed.as_ref().ok_or(Error("missing_observed_capture"))?,
                    io,
                )?,
                Err(error) if error.code() == "input_or_storage_unavailable" => {
                    Delivery::DurabilityUncertain
                }
                Err(error) => return Err(error),
            }
        } else {
            prepare()?;
            storage::publish_new_with(guard, &relative, &encoded, io)?
        };
        Ok(CollectedReport(ClaimReport::new(
            "collected_unverified",
            self.input,
            CollectionDetails {
                collection_id: identity,
                handoff_sha256: self.digest.as_str().to_owned(),
                handoff_bytes: self.raw.len(),
                task_binding: binding,
                artifact_path: read_fs::path_text(&target)?,
                publication: if replayed { "existing" } else { "created" },
                delivery,
                effect: "local_artifact_capture_only",
            },
        )))
    }
}

pub fn collect(
    workspace: &Path,
    file: &Path,
    expected_sha256: &str,
    expected_revision: &str,
) -> Result<CollectedReport> {
    Prepared::read(workspace, file, expected_sha256, expected_revision)?.finish(&SystemEvidenceIo)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{cell::Cell, fs, io};

    struct Fixture {
        _temp: tempfile::TempDir,
        root: PathBuf,
        input: PathBuf,
        raw: Vec<u8>,
        digest: String,
        revision: String,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("control");
            fs::create_dir(&root).unwrap();
            fs::create_dir(root.join("state")).unwrap();
            fs::write(
                root.join("coven.json"),
                r#"{"name":"fixture","agents":["operator"]}"#,
            )
            .unwrap();
            let task=br#"[{"id":"task","title":"Assigned","status":"assigned","owner":"operator","extra":{"float":-0.0,"integer":18446744073709551617}}]"#;
            fs::write(root.join("state/tasks.json"), task).unwrap();
            let revision = ledger::snapshot_revision(Some(task));
            let raw=serde_json::to_vec(&json!({"schema_version":1,"contract":handoff::CONTRACT,
                "task":{"id":"task","owner":"operator","revision":revision},"claimed_status":"completed",
                "summary":"Only a claim λ 🦊","confidence":"medium","findings":[],"files_inspected":[],"files_changed":[],
                "checks":[],"blockers":[],"risks":[],"follow_ups":[],"references":[],"source_claim":null})).unwrap();
            let input = temp.path().join("worker.json");
            fs::write(&input, &raw).unwrap();
            let digest = canonical::digest(&raw);
            Self {
                _temp: temp,
                root,
                input,
                raw,
                digest,
                revision,
            }
        }
        fn prepare(&self) -> Prepared {
            Prepared::read(&self.root, &self.input, &self.digest, &self.revision).unwrap()
        }
        fn result(&self, io: &impl EvidenceIo) -> CollectedReport {
            self.prepare().finish(io).unwrap()
        }
        fn artifacts(&self) -> Vec<PathBuf> {
            archive::scan(&self.root).unwrap().paths
        }
    }
    #[derive(Clone, Copy)]
    enum Fault {
        None,
        Mkdir,
        Temp,
        Write,
        FileSync,
        Link,
        Directory(usize),
        Cleanup,
        ReadForSync,
        OpenForSync,
        MetadataForSync,
        PartialWriteCleanup,
    }
    struct FaultIo {
        fault: Fault,
        dirs: Cell<usize>,
    }
    impl FaultIo {
        fn new(fault: Fault) -> Self {
            Self {
                fault,
                dirs: Cell::new(0),
            }
        }
    }
    fn failed<T>() -> io::Result<T> {
        Err(io::Error::other("private fixture failure"))
    }
    impl EvidenceIo for FaultIo {
        fn read_for_sync(&self, path: &Path, limit: usize) -> Result<Vec<u8>> {
            if matches!(self.fault, Fault::ReadForSync) {
                Err(Error("input_or_storage_unavailable"))
            } else {
                read_fs::read_regular(path, limit)
            }
        }
        fn open_for_sync(&self, path: &Path) -> io::Result<std::fs::File> {
            if matches!(self.fault, Fault::OpenForSync) {
                failed()
            } else {
                SystemEvidenceIo.open_for_sync(path)
            }
        }
        fn regular_for_sync(&self, file: &std::fs::File) -> io::Result<bool> {
            if matches!(self.fault, Fault::MetadataForSync) {
                failed()
            } else {
                SystemEvidenceIo.regular_for_sync(file)
            }
        }
        fn mkdir(&self, path: &Path) -> io::Result<()> {
            if matches!(self.fault, Fault::Mkdir) {
                failed()
            } else {
                crate::platform::mkdir_private(path)
            }
        }
        fn temporary(&self, parent: &Path) -> io::Result<tempfile::NamedTempFile> {
            if matches!(self.fault, Fault::Temp) {
                failed()
            } else {
                SystemEvidenceIo.temporary(parent)
            }
        }
        fn write(&self, file: &mut std::fs::File, bytes: &[u8]) -> io::Result<()> {
            if matches!(self.fault, Fault::PartialWriteCleanup) {
                SystemEvidenceIo.write(file, &bytes[..bytes.len().min(8)])?;
                return failed();
            }
            if matches!(self.fault, Fault::Write) {
                failed()
            } else {
                SystemEvidenceIo.write(file, bytes)
            }
        }
        fn sync_file(&self, file: &std::fs::File) -> io::Result<()> {
            if matches!(self.fault, Fault::FileSync) {
                failed()
            } else {
                file.sync_all()
            }
        }
        fn link(&self, source: &Path, target: &Path) -> io::Result<()> {
            if matches!(self.fault, Fault::Link) {
                failed()
            } else {
                fs::hard_link(source, target)
            }
        }
        fn sync_dir(&self, path: &Path) -> io::Result<()> {
            let next = self.dirs.get() + 1;
            self.dirs.set(next);
            if matches!(self.fault,Fault::Directory(at) if at==next) {
                failed()
            } else {
                crate::platform::sync_directory(path)
            }
        }
        fn cleanup(&self, temp: tempfile::NamedTempFile) -> io::Result<()> {
            if matches!(self.fault, Fault::Cleanup | Fault::PartialWriteCleanup) {
                let (file, _path) = temp.keep()?;
                drop(file);
                failed()
            } else {
                temp.close()
            }
        }
    }
    #[test]
    fn every_prelink_io_failure_leaves_no_final_artifact() {
        for fault in [
            Fault::Mkdir,
            Fault::Temp,
            Fault::Write,
            Fault::FileSync,
            Fault::Link,
            Fault::Directory(1),
            Fault::Directory(2),
            Fault::Directory(3),
            Fault::Directory(4),
        ] {
            let fixture = Fixture::new();
            let before = fs::read(fixture.root.join("state/tasks.json")).unwrap();
            assert!(fixture.prepare().finish(&FaultIo::new(fault)).is_err());
            assert!(fixture.artifacts().is_empty());
            assert_eq!(
                fs::read(fixture.root.join("state/tasks.json")).unwrap(),
                before
            );
        }
    }
    #[test]
    fn postlink_sync_failure_retains_final_and_retry_confirms_without_replacement() {
        let fixture = Fixture::new();
        let uncertain = fixture.result(&FaultIo::new(Fault::Directory(5)));
        assert_eq!(uncertain.delivery(), Delivery::DurabilityUncertain);
        let path = fixture.artifacts().pop().unwrap();
        let before = fs::read(&path).unwrap();
        let replay = fixture.result(&SystemEvidenceIo);
        assert_eq!(replay.delivery(), Delivery::Committed);
        assert_eq!(replay.0.details().publication, "existing");
        assert_eq!(fs::read(path).unwrap(), before);
        assert_eq!(
            uncertain.0.details().collection_id,
            replay.0.details().collection_id
        );
    }
    #[test]
    fn replay_sync_failures_remain_existing_uncertainty() {
        for fault in [
            Fault::Directory(1),
            Fault::Directory(5),
            Fault::FileSync,
            Fault::ReadForSync,
            Fault::OpenForSync,
            Fault::MetadataForSync,
        ] {
            let fixture = Fixture::new();
            fixture.result(&SystemEvidenceIo);
            let before = fs::read(fixture.artifacts().pop().unwrap()).unwrap();
            let replay = fixture.result(&FaultIo::new(fault));
            assert_eq!(replay.delivery(), Delivery::DurabilityUncertain);
            assert_eq!(replay.0.details().publication, "existing");
            assert_eq!(
                fs::read(fixture.artifacts().pop().unwrap()).unwrap(),
                before
            );
        }
    }
    #[test]
    fn cleanup_failure_keeps_only_an_inert_owned_temp_and_never_rolls_back_final() {
        let fixture = Fixture::new();
        let report = fixture.result(&FaultIo::new(Fault::Cleanup));
        assert_eq!(report.delivery(), Delivery::Committed);
        let scan = archive::scan(&fixture.root).unwrap();
        assert_eq!(scan.paths.len(), 1);
        assert_eq!(scan.count, 2);
        assert_eq!(
            fixture.result(&SystemEvidenceIo).0.details().publication,
            "existing"
        );
    }
    #[test]
    fn wrong_guard_cannot_authorize_other_workspace_publication() {
        let first = Fixture::new();
        let other = Fixture::new();
        let guard = WorkspaceGuard::acquire(&other.root, Duration::ZERO).unwrap();
        assert!(first.prepare().publish(&guard, &SystemEvidenceIo).is_err());
        assert!(first.artifacts().is_empty());
        assert!(other.artifacts().is_empty());
    }
    #[test]
    fn locked_context_recheck_rejects_changed_task_or_roster() {
        for roster in [false, true] {
            let fixture = Fixture::new();
            let prepared = fixture.prepare();
            if roster {
                fs::write(
                    fixture.root.join("coven.json"),
                    r#"{"name":"fixture","agents":["different"]}"#,
                )
                .unwrap();
            } else {
                fs::write(fixture.root.join("state/tasks.json"), b"[]").unwrap();
            }
            assert!(prepared.finish(&SystemEvidenceIo).is_err());
            assert!(fixture.artifacts().is_empty());
        }
    }
    #[test]
    fn prepared_input_is_captured_once_even_if_worker_rewrites_file() {
        let fixture = Fixture::new();
        let prepared = fixture.prepare();
        fs::write(&fixture.input, b"changed worker file").unwrap();
        prepared.finish(&SystemEvidenceIo).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(fixture.artifacts().pop().unwrap()).unwrap()).unwrap();
        assert_eq!(
            value["payload"]["handoff_json"]
                .as_str()
                .unwrap()
                .as_bytes(),
            fixture.raw
        );
    }
    #[test]
    fn existing_corruption_is_rejected_and_never_overwritten() {
        let fixture = Fixture::new();
        fixture.result(&SystemEvidenceIo);
        let path = fixture.artifacts().pop().unwrap();
        fs::write(&path, b"tampered").unwrap();
        assert!(fixture.prepare().finish(&SystemEvidenceIo).is_err());
        assert_eq!(fs::read(path).unwrap(), b"tampered");
    }
    #[test]
    fn unrelated_controls_are_not_loaded_or_used_as_authority() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.root.join("state/execution")).unwrap();
        fs::create_dir_all(fixture.root.join("state/actions")).unwrap();
        fs::write(
            fixture.root.join("state/execution/control.json"),
            b"bad execution",
        )
        .unwrap();
        fs::write(
            fixture.root.join("state/actions/control.json"),
            b"bad startup",
        )
        .unwrap();
        fixture.result(&SystemEvidenceIo);
        assert_eq!(
            fs::read(fixture.root.join("state/execution/control.json")).unwrap(),
            b"bad execution"
        );
    }
    #[test]
    fn normal_capture_retains_fixed_claim_only_report() {
        let fixture = Fixture::new();
        let report = fixture.result(&FaultIo::new(Fault::None));
        let value = serde_json::to_value(report).unwrap();
        assert_eq!(value["assessment"], "collected_unverified");
        assert_eq!(value["authorization"], "none");
        assert_eq!(value["dispatchable"], false);
        assert_eq!(value["verification"]["task_completion"], "unestablished");
    }
    #[test]
    fn partial_write_and_cleanup_failure_leaves_only_an_inert_temp() {
        let fixture = Fixture::new();
        assert!(
            fixture
                .prepare()
                .finish(&FaultIo::new(Fault::PartialWriteCleanup))
                .is_err()
        );
        let scan = archive::scan(&fixture.root).unwrap();
        assert!(scan.paths.is_empty());
        assert_eq!(scan.count, 1);
        assert_eq!(scan.total, 8);
    }
    #[test]
    fn pending_authority_between_preflight_and_lock_never_falls_back() {
        let fixture = Fixture::new();
        let prepared = fixture.prepare();
        fs::create_dir(fixture.root.join(ledger::DIRECTORY)).unwrap();
        assert!(prepared.finish(&SystemEvidenceIo).is_err());
        assert!(fixture.artifacts().is_empty());
    }
    #[test]
    #[cfg(unix)]
    fn activated_authority_between_preflight_and_lock_makes_old_binding_stale() {
        let fixture = Fixture::new();
        let prepared = fixture.prepare();
        let code = "from runner import coven;from pathlib import Path;import sys;w=sys.argv[1];r=coven.read_task_snapshot(Path(w))[0];raise SystemExit(coven.main(['state','import-tasks',w,'--expect-revision',r,'--json']))";
        let output = std::process::Command::new("python3")
            .arg("-c")
            .arg(code)
            .arg(&fixture.root)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("COVEN_MULTIPLEXER", "")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            prepared.finish(&SystemEvidenceIo).err().unwrap().code(),
            "stale_handoff_task_revision"
        );
        assert!(fixture.artifacts().is_empty());
    }
    #[test]
    fn retained_bytes_reserve_both_links_at_boundary_and_allow_existing_replay() {
        for over in [false, true] {
            let fixture = Fixture::new();
            let prepared = fixture.prepare();
            let binding = context(&prepared.root, &prepared.input, &prepared.revision).unwrap();
            let (_, encoded) =
                archive::capture_bytes(&prepared.raw, binding, &prepared.root).unwrap();
            let total = archive::MAX_RETAINED - 2 * encoded.len() as u64 + u64::from(over);
            let directory = fixture.root.join("output/handoffs");
            fs::create_dir_all(&directory).unwrap();
            let mut remaining = total;
            let mut index = 0;
            while remaining > 0 {
                let count = remaining.min(archive::MAX_ARCHIVE as u64);
                fs::File::create(directory.join(format!(".handoff-budget-{index}.tmp")))
                    .unwrap()
                    .set_len(count)
                    .unwrap();
                remaining -= count;
                index += 1;
            }
            let result = prepared.finish(&SystemEvidenceIo);
            assert_eq!(result.is_ok(), !over);
            if !over {
                assert_eq!(
                    fixture.result(&SystemEvidenceIo).0.details().publication,
                    "existing"
                );
            }
        }
    }
    #[test]
    fn observed_confirmation_does_not_turn_mismatch_or_unsafe_path_into_uncertainty() {
        let fixture = Fixture::new();
        fixture.result(&SystemEvidenceIo);
        let relative =
            Path::new("output/handoffs").join(fixture.artifacts()[0].file_name().unwrap());
        let guard = WorkspaceGuard::acquire(&fixture.root, Duration::ZERO).unwrap();
        let path = fixture.root.join(&relative);
        let raw = fs::read(&path).unwrap();
        let observed = storage::observe_existing(&guard, &relative, &raw).unwrap();
        fs::write(&path, b"mismatch").unwrap();
        assert!(storage::confirm_observed(&guard, &observed).is_err());
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&fixture.input, &path).unwrap();
        assert!(storage::confirm_observed(&guard, &observed).is_err());
    }
}
