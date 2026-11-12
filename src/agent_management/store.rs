use super::files::{Files, Guard, Image};
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(test)]
const RELATIVE: &str = "state/agent-management.json";
const MAX_STORE: usize = 256 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    provider: String,
    session_id: String,
    last_event: String,
    timestamp_ms: u64,
    server_epoch: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Saved {
    schema_version: u32,
    records: BTreeMap<String, Record>,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            schema_version: 1,
            records: BTreeMap::new(),
        }
    }
}

pub(super) struct Store {
    pub root: PathBuf,
    pub epoch: String,
    saved: Saved,
    receipts: BTreeMap<String, Instant>,
    files: Files,
}

fn record_id(provider: &str, sid: &str) -> String {
    // Provider and a NUL domain separator make the tuple unambiguous.
    format!(
        "{:x}",
        Sha256::digest([provider.as_bytes(), b"\0", sid.as_bytes()].concat())
    )
}

impl Store {
    pub fn open(workspace: &Path) -> Result<Self> {
        Self::open_inner(workspace, None)
    }

    #[cfg(test)]
    fn open_with_sync(workspace: &Path, sync: &dyn Fn(&Path) -> Result<()>) -> Result<Self> {
        Self::open_inner(workspace, Some(sync))
    }

    fn open_inner(workspace: &Path, sync: Option<&dyn Fn(&Path) -> Result<()>>) -> Result<Self> {
        let files = Files::open(workspace)?;
        let guard = files.guard(Duration::from_millis(100))?;
        let root = files.root().to_owned();
        // Sync on every startup, including recovery after an earlier failed sync.
        // Replacing the record only syncs state/, not the root's new state entry.
        if let Some(sync) = sync {
            sync(&root.join("state"))?;
            sync(&root)?;
        }
        files.sync_startup()?;
        let (saved, _) = Self::load(&files, &guard)?;
        drop(guard);
        Ok(Self {
            root,
            epoch: crate::platform::random_uuid_hex()?,
            saved,
            receipts: BTreeMap::new(),
            files,
        })
    }

    fn load(files: &Files, guard: &Guard<'_>) -> Result<(Saved, Image)> {
        let image = files.load(guard)?;
        let Some(bytes) = &image.bytes else {
            return Ok((Saved::default(), image));
        };
        let saved: Saved = serde_json::from_value(decode(bytes, MAX_STORE)?)?;
        require(saved.schema_version == 1, "agent_invalid_store")?;
        require(saved.records.len() <= MAX_RECORDS, "agent_capacity")?;
        for (id, record) in &saved.records {
            require(
                record_id(&record.provider, session(&record.session_id)?) == *id,
                "agent_invalid_store",
            )?;
            event(&record.provider, &record.last_event)?;
            require(
                record.server_epoch.len() == 32
                    && record.server_epoch.bytes().all(|b| b.is_ascii_hexdigit()),
                "agent_invalid_store",
            )?;
        }
        Ok((saved, image))
    }

    pub(super) fn fence(&self) -> Result<()> {
        self.files.fence()
    }

    pub fn report(&mut self, params: &Value) -> Result<Value> {
        require(
            params.as_object().is_some_and(|o| o.len() == 3),
            "agent_invalid_schema",
        )?;
        let sid = session(text(params, "session_id")?)?;
        let provider = text(params, "provider")?;
        let name = event(provider, text(params, "event")?)?;
        let id = record_id(provider, sid);
        self.fence()?;
        let guard = self.files.guard(Duration::from_millis(100))?;
        self.fence()?;
        let (mut saved, image) = Self::load(&self.files, &guard)?;
        require(
            saved.records.contains_key(&id) || saved.records.len() < MAX_RECORDS,
            "agent_capacity",
        )?;
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error("agent_clock_error"))?
            .as_millis()
            .min(u64::MAX as u128) as u64;
        saved.records.insert(
            id.clone(),
            Record {
                provider: provider.into(),
                session_id: sid.into(),
                last_event: name.into(),
                timestamp_ms,
                server_epoch: self.epoch.clone(),
            },
        );
        let bytes = serde_json::to_vec(&saved)?;
        require(bytes.len() <= MAX_STORE, "agent_store_limit")?;
        let nonce = crate::platform::random_uuid_hex()?;
        let delivery = self.files.replace(&guard, &image, &nonce, &bytes)?;
        // Both delivery outcomes mean replacement happened. Never retry/roll back.
        self.saved = saved;
        self.receipts.insert(id.clone(), Instant::now());
        Ok(json!({"type":"accepted", "record_id":id, "delivery":delivery}))
    }

    pub fn projection(&self) -> Value {
        let records: Vec<Value> = self.saved.records.iter().map(|(id, record)| {
            let age = if record.server_epoch == self.epoch { self.receipts.get(id).map(|at| at.elapsed().as_millis().min(u64::MAX as u128) as u64) } else { None };
            let state = if age.is_none() || matches!(record.provider.as_str(), "codex" | "copilot" | "pi" | "opencode") { "unknown" } else { match record.last_event.as_str() {
                "UserPromptSubmit" | "PreToolUse" | "PostToolUse" => "working",
                "Stop" => "idle",
                "SessionEnd" => "ended",
                _ => "unknown",
            }};
            json!({"record_id":id,"provider":record.provider,"last_event":record.last_event,"reported_state":state,"age_ms":age,"reference_trust":"caller_supplied_unverified","native_identity":"unestablished","job_completion":"unestablished"})
        }).collect();
        json!({"contract":CONTRACT,"server_epoch":self.epoch,"records":records,"authorization":"none"})
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{cell::RefCell, os::unix::fs::MetadataExt};

    #[test]
    fn startup_directory_sync_failure_never_readies_a_store_or_claims_commit() {
        for fail_root in [false, true] {
            let root = crate::private_access::test_support::tempdir();
            std::fs::write(root.path().join("coven.json"), b"marker").unwrap();
            let synced = RefCell::new(Vec::new());
            let result = Store::open_with_sync(root.path(), &|path| {
                synced.borrow_mut().push(path.to_owned());
                if (path == root.path()) == fail_root {
                    return Err(Error("agent_startup_sync_failed"));
                }
                crate::platform::sync_directory(path)?;
                Ok(())
            });
            assert_eq!(result.err().unwrap().code(), "agent_startup_sync_failed");
            assert_eq!(synced.borrow()[0], root.path().join("state"));
            assert_eq!(synced.borrow().len(), if fail_root { 2 } else { 1 });
            if fail_root {
                assert_eq!(synced.borrow()[1], root.path());
            }
            assert!(!root.path().join(RELATIVE).exists());
            let lease = root.path().join("state/.agent-management.receiver.lock");
            let inode = std::fs::metadata(&lease).unwrap().ino();
            let mut store = Store::open(root.path()).unwrap();
            assert_eq!(std::fs::metadata(&lease).unwrap().ino(), inode);
            let receipt = store
                .report(&json!({"provider":"claude","event":"Stop","session_id":"s"}))
                .unwrap();
            assert_eq!(receipt["delivery"], "committed");
        }
    }
}
