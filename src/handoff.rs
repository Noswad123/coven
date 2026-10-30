//! Closed, typed claim models. Validation never upgrades worker assertions.
use crate::{Error, Result, canonical, error::require, strict_json};
use serde::{Deserialize, Deserializer, Serialize, de};

pub const CONTRACT: &str = "coven.worker-handoff.v1";
pub const MAX_INPUT: usize = 262_144;

pub(crate) fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

fn revision(value: &str) -> bool {
    if value == "tasks-missing:v1" {
        return true;
    }
    if let Some(digest) = value.strip_prefix("tasks-sha256:") {
        return hex(digest, 64);
    }
    let fields = value.split(':').collect::<Vec<_>>();
    fields.len() == 6
        && fields[0] == "tasks-ledger"
        && fields[1] == "v1"
        && hex(fields[2], 32)
        && hex(fields[3], 32)
        && hex(fields[5], 64)
        && (fields[4] == "0"
            || (!fields[4].is_empty()
                && fields[4].as_bytes()[0] != b'0'
                && fields[4].bytes().all(|c| c.is_ascii_digit())))
}

fn digest(value: &str) -> bool {
    hex(value, 64)
}
fn object_id(value: &str) -> bool {
    hex(value, 40) || hex(value, 64)
}
fn repository_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

macro_rules! token {
    ($name:ident, $valid:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            pub fn parse(value: &str) -> Result<Self> {
                require($valid(value), "invalid_handoff_token")?;
                Ok(Self(value.to_owned()))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(input: D) -> std::result::Result<Self, D::Error> {
                let value = String::deserialize(input)?;
                Self::parse(&value).map_err(de::Error::custom)
            }
        }
    };
}

token!(Identifier, identifier);
token!(TaskRevision, revision);
token!(Digest, digest);
token!(ObjectId, object_id);
token!(RepositoryId, repository_id);

pub(crate) fn required_option<'de, D, T>(input: D) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(input)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaimedStatus {
    Completed,
    Partial,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimedCheck {
    Passed,
    Failed,
    NotRun,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    Transcript,
    Artifact,
    Check,
    NativeSession,
    Repository,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceState {
    Committed,
    Uncommitted,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskClaim {
    pub id: Identifier,
    pub owner: Identifier,
    pub revision: TaskRevision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub priority: Priority,
    pub claim: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckClaim {
    pub name: String,
    pub claimed_outcome: ClaimedCheck,
    pub details: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub kind: ReferenceKind,
    pub uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceClaim {
    pub state: SourceState,
    #[serde(deserialize_with = "required_option")]
    pub repository_id: Option<RepositoryId>,
    #[serde(deserialize_with = "required_option")]
    pub base_sha: Option<ObjectId>,
    #[serde(deserialize_with = "required_option")]
    pub source_sha: Option<ObjectId>,
    #[serde(deserialize_with = "required_option")]
    pub tree_sha: Option<ObjectId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Handoff {
    pub schema_version: u8,
    pub contract: String,
    pub task: TaskClaim,
    pub claimed_status: ClaimedStatus,
    pub summary: String,
    pub confidence: Confidence,
    pub findings: Vec<Finding>,
    pub files_inspected: Vec<String>,
    pub files_changed: Vec<String>,
    pub checks: Vec<CheckClaim>,
    pub blockers: Vec<String>,
    pub risks: Vec<String>,
    pub follow_ups: Vec<String>,
    pub references: Vec<Reference>,
    #[serde(deserialize_with = "required_option")]
    pub source_claim: Option<SourceClaim>,
}

/// Construction requires strict schema and byte-limit validation.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct ValidatedHandoff(Handoff);

impl ValidatedHandoff {
    pub fn task(&self) -> &TaskClaim {
        &self.0.task
    }
    pub fn claimed_status(&self) -> &ClaimedStatus {
        &self.0.claimed_status
    }
    pub fn summary(&self) -> &str {
        &self.0.summary
    }
}

pub(crate) fn text(value: &str, limit: usize, blank: bool, multiline: bool) -> Result<()> {
    require(
        value.len() <= limit
            && (blank || !value.trim().is_empty())
            && !value.chars().any(|c| {
                (c <= '\u{1f}' && (!multiline || !matches!(c, '\n' | '\r' | '\t')))
                    || ('\u{7f}'..='\u{9f}').contains(&c)
            }),
        "invalid_handoff_text",
    )
}

fn strings(values: &[String], limit: usize, paths: bool) -> Result<()> {
    require(values.len() <= limit, "handoff_list_limit")?;
    let mut seen = std::collections::HashSet::new();
    for value in values {
        text(value, 4096, false, !paths)?;
        require(seen.insert(value), "duplicate_handoff_value")?;
        if paths {
            require(
                !value.starts_with('/')
                    && !value.ends_with('/')
                    && !value.contains('\\')
                    && !value
                        .split('/')
                        .any(|part| part.is_empty() || part == "." || part == ".."),
                "invalid_claimed_path",
            )?;
        }
    }
    Ok(())
}

pub fn decode(raw: &[u8]) -> Result<ValidatedHandoff> {
    require(raw.len() <= MAX_INPUT, "handoff_size_limit")?;
    let model: Handoff = serde_json::from_value(strict_json::decode(raw)?)?;
    require(
        model.schema_version == 1 && model.contract == CONTRACT,
        "unsupported_handoff_contract",
    )?;
    text(&model.summary, 16384, false, true)?;
    strings(&model.files_inspected, 256, true)?;
    strings(&model.files_changed, 256, true)?;
    for values in [&model.blockers, &model.risks, &model.follow_ups] {
        strings(values, 64, false)?;
    }
    require(
        model.findings.len() <= 64 && model.checks.len() <= 64 && model.references.len() <= 64,
        "handoff_list_limit",
    )?;
    for finding in &model.findings {
        text(&finding.claim, 8192, false, true)?;
        strings(&finding.evidence, 16, false)?;
    }
    for check in &model.checks {
        text(&check.name, 256, false, false)?;
        text(&check.details, 8192, true, true)?;
        strings(&check.evidence, 16, false)?;
    }
    for reference in &model.references {
        text(&reference.uri, 4096, false, false)?;
    }
    if let Some(source) = &model.source_claim {
        let hashes = [&source.base_sha, &source.source_sha, &source.tree_sha]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if let Some(first) = hashes.first() {
            require(
                hashes
                    .iter()
                    .all(|id| id.as_str().len() == first.as_str().len()),
                "mixed_object_formats",
            )?;
        }
        require(
            if source.state == SourceState::Committed {
                hashes.len() == 3
            } else {
                source.source_sha.is_none() && source.tree_sha.is_none()
            },
            "inconsistent_source_claim",
        )?;
    }
    require(
        model.files_changed.is_empty() || model.source_claim.is_some(),
        "changed_files_require_source_claim",
    )?;
    let validated = ValidatedHandoff(model);
    require(
        canonical::handoff(&validated)?.len() <= MAX_INPUT,
        "handoff_size_limit",
    )?;
    Ok(validated)
}

impl From<std::str::Utf8Error> for Error {
    fn from(_: std::str::Utf8Error) -> Self {
        Self("invalid_utf8")
    }
}
