use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) const CONTRACT: &str = "coven.fleet-observer.v1";
pub(super) const OUTPUT_LIMIT: usize = 64 * 1024;
pub(super) const TITLE_LIMIT: usize = 4096;

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub(super) fn digest(parts: &[&str]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Agent {
    pub terminal_id: String,
    pub pane_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_name: Option<String>,
    pub provider: Option<String>,
    pub project: Option<String>,
    pub activity: String,
    pub evidence: String,
    pub provider_candidates: Vec<String>,
}

impl Agent {
    pub fn same_terminal(&self, other: &Self) -> bool {
        self.terminal_id == other.terminal_id
            && self.pane_id == other.pane_id
            && self.workspace_id == other.workspace_id
            && self.tab_id == other.tab_id
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Inventory {
    pub generation: String,
    pub agents: Vec<Agent>,
    pub discovery_status: String,
    pub unclassified_panes: usize,
}

pub(super) fn provider_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Output {
    pub generation: String,
    pub terminal_id: String,
    pub text: String,
    pub truncated: bool,
}

pub(super) fn truncate(text: &mut String) -> bool {
    if text.len() <= OUTPUT_LIMIT {
        return false;
    }
    let mut end = OUTPUT_LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    true
}
