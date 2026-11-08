//! Read-only v1 upgrade planning and immutable, source-free backup evidence.
use super::super::*;
use crate::{platform, storage::WorkspaceGuard};
use std::{fs::File, io::Write, path::PathBuf};

const CONTROL: &str = "state/execution/control.json";
const UPGRADES: &str = "state/execution/upgrades";

pub(super) struct Plan {
    pub(super) control: Value,
    pub(super) effect: Value,
    root: PathBuf,
    raw: Vec<u8>,
    mode: u32,
    backup: PathBuf,
}

fn capture(root: &Path) -> Result<(Vec<u8>, u32)> {
    let path = read_fs::managed(root, Path::new(CONTROL), false)?;
    let mode = platform::replacement_mode(&path)?.ok_or(Error("execution_disabled"))?;
    let raw = read_fs::read_regular(&path, values::MAX_INPUT)?;
    let file = platform::open_readonly(&path)?;
    require(
        file.metadata()?.is_file(),
        "unsafe_execution_upgrade_source",
    )?;
    platform::verify_replacement_mode(&file, mode)?;
    Ok((raw, mode))
}

pub(super) fn plan(
    root: &Path,
    control: &Value,
    selected: &super::super::Validated,
    metadata: &dyn super::lifecycle::Metadata,
    work: &mut super::super::Work,
) -> Result<Plan> {
    require(selected.schema == 1, "execution_already_upgraded")?;
    let (raw, mode) = capture(root)?;
    let observed = values::parse(&raw, Profile::Authority)?;
    require(
        work.equal(&observed, control)?,
        "execution_upgrade_source_changed",
    )?;
    let encoded = work.encode(control)?;
    let revision = format!(
        "execution-control-v1:{}:{}:{}",
        text(get(control, "execution_authority_id")?)?,
        selected.replayed.receipts.len(),
        canonical::digest(&encoded),
    );
    require(
        revision == selected.replayed.revision,
        "execution_upgrade_source_changed",
    )?;
    let id = metadata.uuid()?;
    require(hex(&id, 32), "invalid_execution_uuid")?;
    let backup = PathBuf::from(format!("{UPGRADES}/{id}/control-v1.bytes"));
    let effect = object(vec![
        ("upgrade_id", string(id)),
        ("source_revision", string(&selected.replayed.revision)),
        ("source_sha256", string(canonical::digest(&raw))),
        ("source_mode", integer(mode as usize)?),
        ("backup_path", string(read_fs::path_text(&backup)?)),
    ])?;
    let candidate = object(vec![
        ("schema_version", integer(2)?),
        (
            "execution_authority_id",
            get(control, "execution_authority_id")?.clone(),
        ),
        (
            "snapshot_source_id",
            get(control, "snapshot_source_id")?.clone(),
        ),
        ("generation", get(control, "generation")?.clone()),
        ("legacy_v1", control.clone()),
        ("upgrade", effect.clone()),
        ("history", Value::Array(Vec::new())),
        ("extensions", get(control, "extensions")?.clone()),
    ])?;
    Ok(Plan {
        control: candidate,
        effect,
        root: root.canonicalize()?,
        raw,
        mode,
        backup,
    })
}

// Private fault seam, not a publication framework. No operation removes artifacts.
trait BackupIo {
    fn mkdir(&self, path: &Path) -> Result<()> {
        Ok(fs::create_dir(path)?)
    }
    fn create(&self, path: &Path) -> Result<File> {
        platform::open_generated(path, true)
    }
    fn write(&self, file: &mut File, raw: &[u8]) -> Result<()> {
        Ok(file.write_all(raw)?)
    }
    fn apply_mode(&self, file: &File, mode: u32) -> Result<()> {
        Ok(platform::apply_replacement_mode(file, mode)?)
    }
    fn verify_mode(&self, file: &File, mode: u32) -> Result<()> {
        Ok(platform::verify_replacement_mode(file, mode)?)
    }
    fn sync_file(&self, file: &File) -> Result<()> {
        Ok(file.sync_all()?)
    }
    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        read_fs::read_regular(path, values::MAX_INPUT)
    }
    fn reopen(&self, path: &Path) -> Result<File> {
        platform::open_readonly(path)
    }
    fn sync_dir(&self, path: &Path) -> Result<()> {
        Ok(platform::sync_directory(path)?)
    }
}
struct SystemBackupIo;
impl BackupIo for SystemBackupIo {}

impl Plan {
    /// Call only after full successor replay, headroom and output admission.
    pub(super) fn publish(&self, root: &Path, guard: &WorkspaceGuard) -> Result<()> {
        self.publish_with(root, guard, &SystemBackupIo)
    }

    fn publish_with(&self, root: &Path, guard: &WorkspaceGuard, io: &impl BackupIo) -> Result<()> {
        guard.validate_for(root)?;
        guard.validate_for(&self.root)?;
        // Under the cooperating-writer contract, refuse a changed authority before
        // creating even an inert directory. Never reconstruct bytes from JSON.
        let (raw, mode) = capture(root)?;
        require(
            raw == self.raw && mode == self.mode,
            "execution_upgrade_source_changed",
        )?;
        let result = (|| {
            let upgrades = read_fs::managed(root, Path::new(UPGRADES), true)?;
            if !upgrades.exists() {
                io.mkdir(&upgrades)?;
            }
            let relative = self
                .backup
                .parent()
                .ok_or(Error("invalid_execution_upgrade_path"))?;
            let directory = read_fs::managed(root, relative, true)?;
            // Fresh UUID directory is mandatory, even if old bytes happen to match.
            io.mkdir(&directory)?;
            let backup = read_fs::managed(root, &self.backup, false)?;
            let mut file = io.create(&backup)?;
            io.write(&mut file, &self.raw)?;
            // Writing may clear special permission bits; apply and verify afterward.
            io.apply_mode(&file, self.mode)?;
            io.verify_mode(&file, self.mode)?;
            io.sync_file(&file)?;
            drop(file);
            let backup = read_fs::managed(root, &self.backup, false)?;
            require(
                io.read(&backup)? == self.raw,
                "execution_upgrade_backup_mismatch",
            )?;
            let file = io.reopen(&backup)?;
            require(
                file.metadata()?.is_file(),
                "unsafe_execution_upgrade_backup",
            )?;
            io.verify_mode(&file, self.mode)?;
            for parent in [
                directory,
                upgrades,
                root.join("state/execution"),
                root.join("state"),
                root.to_owned(),
            ] {
                io.sync_dir(&parent)?;
            }
            Ok(())
        })();
        result.map_err(|_: Error| Error("execution_upgrade_not_published_inert_backup_retained"))
    }
}

#[cfg(test)]
mod tests;
