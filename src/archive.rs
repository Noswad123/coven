//! Evidence model/encoding and read-only history. Collection uses these formats
//! without turning worker claims into task/native execution authority.
use crate::{
    Error, Result, canonical,
    error::require,
    handoff::{self, ClaimedStatus, Digest, Identifier, TaskClaim, TaskRevision, ValidatedHandoff},
    read_fs, strict_json,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const CONTRACT: &str = "coven.handoff-collection.v1";
pub(crate) const MAX_ARCHIVE: usize = 1_048_576;
pub(crate) const MAX_ENTRIES: usize = 256;
pub(crate) const MAX_RETAINED: u64 = 32 * 1_048_576;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskBinding {
    pub(crate) task_id: Identifier,
    pub(crate) owner: Identifier,
    pub(crate) revision: TaskRevision,
    pub(crate) task_sha256: Digest,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Payload {
    handoff_json: String,
    handoff_sha256: Digest,
    handoff_bytes: usize,
    task_binding: TaskBinding,
    workspace_at_collection: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Envelope {
    schema_version: u8,
    contract: String,
    collection_id: Digest,
    payload: Payload,
}

#[derive(Debug, Serialize)]
pub struct Verification {
    worker_identity: &'static str,
    native_binding: &'static str,
    source: &'static str,
    checks: &'static str,
    task_completion: &'static str,
}

impl Verification {
    fn unverified() -> Self {
        Self {
            worker_identity: "unestablished",
            native_binding: "unestablished",
            source: "unverified",
            checks: "worker_reported",
            task_completion: "unestablished",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ClaimReport<T: Serialize> {
    schema_version: u8,
    contract: &'static str,
    assessment: &'static str,
    authorization: &'static str,
    dispatchable: bool,
    verification: Verification,
    handoff: ValidatedHandoff,
    #[serde(flatten)]
    details: T,
}

impl<T: Serialize> ClaimReport<T> {
    pub(crate) fn details(&self) -> &T {
        &self.details
    }
    pub(crate) fn new(assessment: &'static str, handoff: ValidatedHandoff, details: T) -> Self {
        Self {
            schema_version: 1,
            contract: CONTRACT,
            assessment,
            authorization: "none",
            dispatchable: false,
            verification: Verification::unverified(),
            handoff,
            details,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ValidationDetails {
    handoff_sha256: String,
    handoff_bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct HistoricalDetails {
    collection_id: String,
    handoff_sha256: String,
    handoff_bytes: usize,
    task_binding: TaskBinding,
    workspace_at_collection: String,
    artifact_path: String,
    current_task_binding: &'static str,
}

pub fn validate_file(path: &Path) -> Result<ClaimReport<ValidationDetails>> {
    let raw = read_fs::read_regular(path, handoff::MAX_INPUT)?;
    let handoff = handoff::decode(&raw)?;
    Ok(ClaimReport::new(
        "valid_unverified",
        handoff,
        ValidationDetails {
            handoff_sha256: canonical::digest(&raw),
            handoff_bytes: raw.len(),
        },
    ))
}

fn read_archive(path: &Path) -> Result<ClaimReport<HistoricalDetails>> {
    let raw = read_fs::read_regular(path, MAX_ARCHIVE)?;
    let archive: Envelope = serde_json::from_value(strict_json::decode(&raw)?)?;
    require(
        archive.schema_version == 1 && archive.contract == CONTRACT,
        "unsupported_collection_contract",
    )?;
    let payload = &archive.payload;
    let embedded = payload.handoff_json.as_bytes();
    require(
        embedded.len() <= handoff::MAX_INPUT
            && embedded.len() == payload.handoff_bytes
            && canonical::digest(embedded) == payload.handoff_sha256.as_str(),
        "handoff_digest_mismatch",
    )?;
    let handoff = handoff::decode(embedded)?;
    let binding = &payload.task_binding;
    require(
        binding.task_id == handoff.task().id
            && binding.owner == handoff.task().owner
            && binding.revision == handoff.task().revision,
        "invalid_handoff_binding",
    )?;
    handoff::text(&payload.workspace_at_collection, 8192, false, false)?;
    require(
        payload.workspace_at_collection.starts_with('/'),
        "invalid_historical_workspace",
    )?;
    let identity = canonical::collection_id(payload)?;
    require(
        identity == archive.collection_id.as_str()
            && path.file_name().and_then(|value| value.to_str())
                == Some(format!("{identity}.json").as_str())
            && canonical::envelope(&archive)? == raw,
        "collection_integrity_mismatch",
    )?;
    Ok(ClaimReport::new(
        "historical_unverified",
        handoff,
        HistoricalDetails {
            collection_id: identity,
            handoff_sha256: payload.handoff_sha256.as_str().to_owned(),
            handoff_bytes: payload.handoff_bytes,
            task_binding: binding.clone(),
            workspace_at_collection: payload.workspace_at_collection.clone(),
            artifact_path: read_fs::path_text(path)?,
            current_task_binding: "not_checked",
        },
    ))
}

pub fn inspect(workspace: &Path, identity: &str) -> Result<ClaimReport<HistoricalDetails>> {
    let identity = Digest::parse(identity)?;
    let root = read_fs::workspace(workspace)?;
    read_archive(&read_fs::managed(
        &root,
        Path::new(&format!("output/handoffs/{}.json", identity.as_str())),
        false,
    )?)
}

fn temporary(name: &str) -> bool {
    name.strip_prefix(".handoff-")
        .and_then(|value| value.strip_suffix(".tmp"))
        .is_some_and(|value| {
            !value.is_empty()
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        })
}

fn scan_metadata(path: &Path, inert: bool) -> Result<Option<fs::Metadata>> {
    match fs::metadata(path) {
        Ok(value) => Ok(Some(value)),
        Err(error) if inert && error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) struct Scan {
    pub paths: Vec<PathBuf>,
    pub total: u64,
    pub count: usize,
}
pub(crate) fn scan(root: &Path) -> Result<Scan> {
    let directory = read_fs::managed(root, Path::new("output/handoffs"), true)?;
    if !directory.exists() {
        return Ok(Scan {
            paths: Vec::new(),
            total: 0,
            count: 0,
        });
    }
    let mut total = 0_u64;
    let mut paths = Vec::new();
    let mut count = 0;
    for (index, entry) in fs::read_dir(directory)?.enumerate() {
        count = index + 1;
        require(index < MAX_ENTRIES, "handoff_archive_entry_limit")?;
        let entry = entry?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or(Error("unexpected_handoff_archive_entry"))?;
        let inert = temporary(name);
        let relative = Path::new("output/handoffs").join(name);
        let selected = read_fs::managed(root, &relative, false)?;
        let Some(metadata) = scan_metadata(&selected, inert)? else {
            continue;
        };
        require(
            metadata.len() <= MAX_ARCHIVE as u64,
            "handoff_archive_size_limit",
        )?;
        total += metadata.len();
        require(total <= MAX_RETAINED, "handoff_archive_capacity")?;
        if inert {
            continue;
        }
        require(
            name.strip_suffix(".json")
                .is_some_and(|value| handoff::hex(value, 64)),
            "unexpected_handoff_archive_entry",
        )?;
        paths.push(selected);
    }
    paths.sort();
    Ok(Scan {
        paths,
        total,
        count,
    })
}

pub(crate) fn capture_bytes(
    raw: &[u8],
    binding: TaskBinding,
    root: &Path,
) -> Result<(String, Vec<u8>)> {
    let workspace = read_fs::path_text(root)?;
    handoff::text(&workspace, 8192, false, false)?;
    let payload = Payload {
        handoff_json: std::str::from_utf8(raw)?.to_owned(),
        handoff_sha256: Digest::parse(&canonical::digest(raw))?,
        handoff_bytes: raw.len(),
        task_binding: binding,
        workspace_at_collection: workspace,
    };
    let identity = canonical::collection_id(&payload)?;
    let envelope = Envelope {
        schema_version: 1,
        contract: CONTRACT.to_owned(),
        collection_id: Digest::parse(&identity)?,
        payload,
    };
    let encoded = canonical::envelope(&envelope)?;
    require(encoded.len() <= MAX_ARCHIVE, "handoff_archive_size_limit")?;
    Ok((identity, encoded))
}

#[derive(Debug, Serialize)]
struct Summary {
    collection_id: String,
    task: TaskClaim,
    claimed_status: ClaimedStatus,
    summary: String,
    artifact_path: String,
}

#[derive(Debug, Serialize)]
pub struct ListReport {
    schema_version: u8,
    contract: &'static str,
    assessment: &'static str,
    authorization: &'static str,
    dispatchable: bool,
    verification: Verification,
    collections: Vec<Summary>,
    selected_collection_count: usize,
    observed_collection_count: usize,
    current_task_binding: &'static str,
}

pub fn list(workspace: &Path, task_id: Option<&str>) -> Result<ListReport> {
    let selected_task = task_id.map(Identifier::parse).transpose()?;
    let root = read_fs::workspace(workspace)?;
    let reports = scan(&root)?
        .paths
        .into_iter()
        .map(|path| read_archive(&path))
        .collect::<Result<Vec<_>>>()?;
    let observed_collection_count = reports.len();
    let collections = reports
        .into_iter()
        .filter(|report| {
            selected_task
                .as_ref()
                .is_none_or(|task| task == &report.handoff.task().id)
        })
        .map(|report| Summary {
            collection_id: report.details.collection_id,
            task: report.handoff.task().clone(),
            claimed_status: report.handoff.claimed_status().clone(),
            summary: report.handoff.summary().to_owned(),
            artifact_path: report.details.artifact_path,
        })
        .collect::<Vec<_>>();
    Ok(ListReport {
        schema_version: 1,
        contract: CONTRACT,
        assessment: "historical_unverified",
        authorization: "none",
        dispatchable: false,
        verification: Verification::unverified(),
        selected_collection_count: collections.len(),
        collections,
        observed_collection_count,
        current_task_binding: "not_checked",
    })
}

/// JSON output escapes non-ASCII/DEL as Python's CLI does, including surrogate
/// pairs. Canonical archive verification deliberately uses a different encoder.
pub fn output<T: Serialize>(report: &T, compact: bool) -> Result<String> {
    let json = if compact {
        serde_json::to_string(report)?
    } else {
        serde_json::to_string_pretty(report)?
    };
    crate::output::ascii_json(&json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disappeared_inert_temps_are_not_missing_final_archives() {
        let root = tempfile::tempdir().unwrap();
        let absent = root.path().join("missing");
        assert!(scan_metadata(&absent, true).unwrap().is_none());
        assert!(scan_metadata(&absent, false).is_err());
    }
}
