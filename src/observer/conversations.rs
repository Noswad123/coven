use super::*;
use config::{ConversationConfig, ConversationSourceConfig};
use std::collections::HashSet;
use std::sync::atomic::AtomicU64;

#[derive(Default)]
pub(super) struct Conversations {
    registered: Vec<(String, ConversationConfig)>,
    sources: Vec<Source>,
    epoch: String,
    state: Mutex<State>,
}

struct Source {
    config: ConversationSourceConfig,
    busy: AtomicBool,
    generation: AtomicU64,
}

#[derive(Clone)]
struct Cached {
    id: String,
    source: usize,
    selected: discovery::Selected,
}

#[derive(Clone)]
pub(super) struct DecisionTarget {
    pub source_id: String,
    pub team: String,
    pub selected: discovery::Selected,
    pub workspace: Option<String>,
    pub writable: bool,
    pub epoch: String,
}

#[derive(Default)]
struct State {
    generation: u64,
    refs: Vec<Cached>,
    attempts: HashSet<String>,
    active: HashSet<(String, String)>,
    coverage: Vec<Coverage>,
}

#[derive(Clone, Default)]
struct Coverage {
    unavailable: usize,
    failed: bool,
    revision: u64,
}

#[derive(Clone)]
pub(super) struct MetadataProof {
    pub identity: Option<String>,
    pub card: Option<String>,
    pub expires: Option<Instant>,
}

pub(super) struct Flight<'a> {
    state: &'a Mutex<State>,
    key: (String, String),
}
impl Drop for Flight<'_> {
    fn drop(&mut self) {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .active
            .remove(&self.key);
    }
}

impl Conversations {
    pub(super) fn metadata_fence(&self, login: &str) -> MetadataProof {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        self.metadata_proof(&state, login)
    }
    fn metadata_proof(&self, state: &State, login: &str) -> MetadataProof {
        let expires = state
            .refs
            .iter()
            .filter(|r| {
                self.sources[r.source]
                    .config
                    .allowed_logins
                    .iter()
                    .any(|l| l == login)
            })
            .map(|r| r.selected.expires)
            .min();
        MetadataProof {
            identity: self.coverage_proof(state, login, false),
            card: self.proof(state, login),
            expires,
        }
    }

    pub(super) fn card_proof(&self, login: &str) -> Option<String> {
        self.proof(&self.state.lock().unwrap_or_else(|p| p.into_inner()), login)
    }
    fn proof(&self, state: &State, login: &str) -> Option<String> {
        self.coverage_proof(state, login, true)
    }
    fn coverage_proof(&self, state: &State, login: &str, require_complete: bool) -> Option<String> {
        let mut parts = vec![self.epoch.clone(), login.to_owned()];
        for (index, source) in self
            .sources
            .iter()
            .enumerate()
            .filter(|(_, s)| s.config.allowed_logins.iter().any(|l| l == login))
        {
            let coverage = state.coverage.get(index)?;
            // Expiration invalidates the whole proof; it never elects a new winner.
            if require_complete
                && (coverage.failed
                    || state
                        .refs
                        .iter()
                        .any(|r| r.source == index && r.selected.expires <= Instant::now()))
            {
                return None;
            }
            parts.push(source.config.id.clone());
            parts.push(coverage.revision.to_string());
            if !require_complete {
                // Partial metadata has provenance too, but never proves that
                // unavailable candidates cannot compete for a card binding.
                parts.push(coverage.failed.to_string());
                parts.push(coverage.unavailable.to_string());
            }
            parts.extend(Self::candidate_ids(state, index));
        }
        Some(digest(
            &parts.iter().map(String::as_str).collect::<Vec<_>>(),
        ))
    }
    fn candidate_ids(state: &State, source: usize) -> Vec<String> {
        let mut ids = state
            .refs
            .iter()
            .filter(|r| r.source == source)
            .map(|r| r.id.clone())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }
    pub(super) fn attention_unavailable(&self, login: &str) -> usize {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        self.sources
            .iter()
            .enumerate()
            .filter(|(_, s)| s.config.allowed_logins.iter().any(|l| l == login))
            .map(|(i, _)| state.coverage.get(i).map_or(0, |c| c.unavailable))
            .sum()
    }
    pub(super) fn known_attempt(&self, attempt: &str) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .attempts
            .contains(attempt)
    }
    pub(super) fn claim(&self, attempt: &str, runtime: &str, session: &str) -> Result<Flight<'_>> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.attempts.contains(attempt) {
            return Err(Error("request_already_seen"));
        }
        if state.attempts.len() >= 4096 {
            return Err(Error("request_capacity"));
        }
        state.attempts.insert(attempt.into());
        let key = (runtime.into(), session.into());
        if state.active.len() >= 8 || !state.active.insert(key.clone()) {
            return Err(Error("submission_busy"));
        }
        Ok(Flight {
            state: &self.state,
            key,
        })
    }

    pub(super) fn admitted(&self, login: &str) -> bool {
        self.sources
            .iter()
            .any(|s| s.config.allowed_logins.iter().any(|l| l == login))
            || self
                .registered
                .iter()
                .any(|(_, t)| t.allowed_logins.iter().any(|l| l == login))
    }

    pub(super) fn decision_target(&self, login: &str, id: &str) -> Result<DecisionTarget> {
        let r = self.cached(login, id)?;
        let source = &self.sources[r.source].config;
        Ok(DecisionTarget {
            source_id: source.id.clone(),
            team: source.team.clone(),
            selected: r.selected,
            workspace: source.control_workspace.clone(),
            writable: source.decision_logins.iter().any(|l| l == login)
                && source.control_workspace.is_some(),
            epoch: self.epoch.clone(),
        })
    }
    pub(super) fn new(config: &Config, epoch: &str) -> Result<Self> {
        // Full validated configuration plus per-process entropy invalidates every
        // reference on configuration changes/restarts, including ACL-only changes.
        let generation = serde_json::to_string(config).map_err(|_| Error("invalid_config"))?;
        Ok(Self {
            registered: config
                .conversations
                .iter()
                .map(|target| {
                    (
                        digest(&[
                            epoch,
                            &generation,
                            &target.id,
                            &target.team,
                            &target.expected_runtime,
                            &target.session,
                        ]),
                        target.clone(),
                    )
                })
                .collect(),
            sources: config
                .conversation_sources
                .iter()
                .cloned()
                .map(|config| Source {
                    config,
                    busy: AtomicBool::new(false),
                    generation: AtomicU64::new(0),
                })
                .collect(),
            epoch: digest(&[epoch, &generation]),
            state: Mutex::new(State {
                coverage: vec![
                    Coverage {
                        failed: true,
                        ..Coverage::default()
                    };
                    config.conversation_sources.len()
                ],
                ..State::default()
            }),
        })
    }

    #[cfg(test)]
    pub(super) fn list(&self, login: &str) -> Value {
        self.list_with_proof(login).0
    }

    pub(super) fn list_with_proof(&self, login: &str) -> (Value, MetadataProof) {
        let deadline = crate::shared_native::deadline();
        let mut statuses = Vec::new();
        for (index, source) in self.sources.iter().enumerate() {
            // Source ACL precedes even registry metadata inspection.
            if !source.config.allowed_logins.iter().any(|l| l == login) {
                continue;
            }
            let status = if source
                .busy
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                let _busy = Busy(&source.busy);
                // Keep live handles during I/O. Native grant checks remain the
                // authority; only this refresh generation may replace the cache.
                let (old, generation) = {
                    let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                    state.generation += 1;
                    let old = state
                        .refs
                        .iter()
                        .filter(|r| r.source == index)
                        .cloned()
                        .collect::<Vec<_>>();
                    source.generation.store(state.generation, Ordering::Release);
                    (old, state.generation)
                };
                let result = discovery::scan_coverage(&source.config.directory, deadline);
                let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                if source.generation.load(Ordering::Acquire) != generation {
                    "busy"
                } else {
                    let previous_ids = Self::candidate_ids(&state, index);
                    let previous_coverage = state.coverage[index].clone();
                    state.refs.retain(|r| r.source != index);
                    let status = match result {
                        Ok((selected, mut unavailable)) => {
                            let mut refs = Vec::new();
                            for target in selected {
                                if target.expires <= Instant::now() {
                                    unavailable += 1;
                                    continue;
                                }
                                let previous = old.iter().find(|r| {
                                    r.selected.expires > Instant::now()
                                        && r.selected.endpoint == target.endpoint
                                        && r.selected.runtime == target.runtime
                                        && r.selected.grant.grant_id == target.grant.grant_id
                                        && r.selected.grant.session_id == target.grant.session_id
                                        && r.selected.grant.can_submit == target.grant.can_submit
                                });
                                let id = previous.map(|r| r.id.clone()).unwrap_or_else(|| {
                                    digest(&[
                                        &self.epoch,
                                        &generation.to_string(),
                                        &source.config.id,
                                        &format!("{:?}", target.endpoint),
                                        &target.runtime,
                                        &target.grant.grant_id,
                                        &target.grant.session_id,
                                    ])
                                });
                                refs.push(Cached {
                                    id,
                                    source: index,
                                    selected: target,
                                });
                            }
                            state.refs.extend(refs);
                            state.refs.sort_by_key(|r| r.source);
                            state.coverage[index].unavailable = unavailable;
                            state.coverage[index].failed = unavailable > 0;
                            "observed"
                        }
                        Err(_) => {
                            state.coverage[index].unavailable = 0;
                            state.coverage[index].failed = true;
                            "unavailable"
                        }
                    };
                    let coverage = &state.coverage[index];
                    if previous_ids != Self::candidate_ids(&state, index)
                        || previous_coverage.failed != coverage.failed
                        || previous_coverage.unavailable != coverage.unavailable
                    {
                        state.coverage[index].revision = previous_coverage.revision.wrapping_add(1);
                    }
                    status
                }
            } else {
                "busy"
            };
            statuses.push(json!({"id":source.config.id,"label":source.config.label,"team":source.config.team,"status":status}));
        }
        let mut rows = self.registered.iter()
            .filter(|(_, target)| target.allowed_logins.iter().any(|l| l == login))
            .map(|(id, target)| json!({"id":id,"label":target.label,"team":target.team,"host":"Local native","can_submit":false,"max_prompt_bytes":65536}))
            .collect::<Vec<_>>();
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut omitted = 0;
        for cached in self.discovered(&state, login) {
            let source = &self.sources[cached.source].config;
            if rows.len() >= 64 {
                omitted += 1;
                continue;
            }
            rows.push(
                json!({"id":cached.id,"label":source.label,"team":source.team,"host":"Local native",
                "can_submit":Self::writable(source, cached, login),"max_prompt_bytes":65536}),
            );
        }
        (
            json!({"contract":"coven.conversations.v1","conversations":rows,"sources":statuses,"omitted_conversations":omitted}),
            self.metadata_proof(&state, login),
        )
    }

    fn discovered<'a>(
        &'a self,
        state: &'a State,
        login: &'a str,
    ) -> impl Iterator<Item = &'a Cached> {
        state.refs.iter().filter(move |r| {
            r.selected.expires > Instant::now()
                && self.sources[r.source]
                    .config
                    .allowed_logins
                    .iter()
                    .any(|l| l == login)
        })
    }

    fn writable(source: &ConversationSourceConfig, cached: &Cached, login: &str) -> bool {
        cached.selected.expires > Instant::now()
            && cached.selected.grant.can_submit
            && source.allowed_logins.iter().any(|l| l == login)
            && source.submit_logins.iter().any(|l| l == login)
            && source.control_workspace.is_some()
    }

    fn cached(&self, login: &str, id: &str) -> Result<Cached> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let registered = self
            .registered
            .iter()
            .filter(|(_, t)| t.allowed_logins.iter().any(|l| l == login))
            .count();
        // Hidden overflow rows must not remain usable through an older handle.
        self.discovered(&state, login)
            .take(64usize.saturating_sub(registered))
            .find(|r| r.id == id)
            .cloned()
            .ok_or(Error("invalid_reference"))
    }

    /// ACL-filtered, pinned metadata only. Static registrations have no provenance.
    pub(super) fn selected(&self, login: &str, id: &str) -> Result<discovery::Selected> {
        self.cached(login, id).map(|r| r.selected)
    }

    /// Cache-only pre-body authorization. Never stats a path or contacts native.
    pub(super) fn submission_allowed(&self, login: &str, id: &str) -> bool {
        self.cached(login, id)
            .is_ok_and(|r| Self::writable(&self.sources[r.source].config, &r, login))
    }

    pub(super) fn submit(
        &self,
        login: &str,
        id: &str,
        request_id: &str,
        text: &str,
    ) -> Result<Value> {
        use sha2::{Digest, Sha256};
        if !crate::session_prompts::protocol::uuid(request_id) {
            return Err(Error("invalid_prompt_uuid"));
        }
        crate::session_prompts::validate_text(text)?;
        let cached = self.cached(login, id)?;
        let source = &self.sources[cached.source].config;
        if !Self::writable(source, &cached, login) {
            return Err(Error("invalid_reference"));
        }
        let report = |outcome: &str, delivery: &str, code: &str| {
            json!({"contract":"coven.conversation-submit.v1",
            "id":id,"request_id":request_id,"outcome":outcome,"delivery":delivery,"diagnostics":[code],
            "prompt_bytes":text.len(),"prompt_sha256":format!("{:x}",Sha256::digest(text.as_bytes())),"native_prompt_acceptance":"unestablished"})
        };
        let _flight = match self.claim(
            request_id,
            &cached.selected.runtime,
            &cached.selected.grant.session_id,
        ) {
            Ok(flight) => flight,
            Err(e) if e.code() == "request_already_seen" => {
                return Ok(report("uncertain", "uncertain", e.code()));
            }
            Err(e) => return Ok(report("unavailable", "unsent", e.code())),
        };
        let mut value = crate::session_prompts::shared_submit(
            std::path::Path::new(
                source
                    .control_workspace
                    .as_deref()
                    .ok_or(Error("invalid_reference"))?,
            ),
            &cached.selected.endpoint,
            &cached.selected.runtime,
            &cached.selected.grant,
            request_id,
            text,
            || {
                self.cached(login, id).is_ok_and(|now| {
                    now.selected.endpoint == cached.selected.endpoint
                        && now.selected.runtime == cached.selected.runtime
                        && now.selected.grant.grant_id == cached.selected.grant.grant_id
                        && now.selected.grant.session_id == cached.selected.grant.session_id
                        && Self::writable(source, &now, login)
                })
            },
        )?;
        value["contract"] = json!("coven.conversation-submit.v1");
        value["id"] = json!(id);
        value["request_id"] = json!(request_id);
        Ok(value)
    }

    pub(super) fn read(&self, login: &str, id: &str) -> Result<Value> {
        // Never connect, select or stat an endpoint before resource/team admission.
        if let Ok(cached) = self.cached(login, id) {
            let source = &self.sources[cached.source].config;
            let mut connection = crate::shared_native::Connection::new(
                &cached.selected.endpoint,
                &cached.selected.runtime,
                crate::shared_native::deadline(),
            );
            let mut value = connection
                .conversation(&cached.selected.grant)
                .map_err(|_| Error("native_unavailable"))?;
            // A concurrent refresh/revocation cannot restore a stale handle on completion.
            let current = self.cached(login, id)?;
            if current.selected.endpoint != cached.selected.endpoint
                || current.selected.grant.grant_id != cached.selected.grant.grant_id
            {
                return Err(Error("invalid_reference"));
            }
            value["contract"] = json!("coven.conversations.v1");
            value["id"] = json!(id);
            value["team"] = json!(source.team);
            return Ok(value);
        }
        let (_, target) = self
            .registered
            .iter()
            .find(|(reference, target)| {
                reference == id && target.allowed_logins.iter().any(|l| l == login)
            })
            .ok_or(Error("invalid_reference"))?;
        let mut value = crate::session_conversations::read(
            &target.native_socket,
            &target.expected_runtime,
            &target.session,
        )?;
        value["contract"] = json!("coven.conversations.v1");
        value["id"] = json!(id);
        value["team"] = json!(target.team);
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_schema_is_closed_and_bounded() {
        let base = json!({"listen":"127.0.0.1:7341","public_origin":"https://example.invalid",
            "allowed_logins":["alice"],"endpoints":[{"id":"fleet","label":"Fleet","socket":"/absent/f.sock"}],
            "conversations":[{"id":"one","label":"One","team":"research","allowed_logins":["alice"],
                "native_socket":"/private/native.sock","expected_runtime":"11111111-2222-4333-8444-555555555555","session":"ses_one"}]});
        for (key, value) in [
            ("id", json!("../secret")),
            ("label", json!("\n")),
            ("team", json!("")),
            ("allowed_logins", json!([])),
            ("allowed_logins", json!(["alice", "alice"])),
            ("native_socket", json!("/private/../native.sock")),
            ("native_socket", json!(format!("/{}", "x".repeat(100)))),
            ("expected_runtime", json!("not-a-uuid")),
            ("session", json!("ses_")),
        ] {
            let mut bad = base.clone();
            bad["conversations"][0][key] = value;
            assert!(
                serde_json::from_value::<Config>(bad)
                    .unwrap()
                    .validate()
                    .is_err(),
                "{key}"
            );
        }
        let mut unknown = base.clone();
        unknown["conversations"][0]["ssh"] = json!({});
        assert!(serde_json::from_value::<Config>(unknown).is_err());
        let mut missing = base.clone();
        missing["conversations"][0]
            .as_object_mut()
            .unwrap()
            .remove("session");
        assert!(serde_json::from_value::<Config>(missing).is_err());
        let mut duplicate = base.clone();
        duplicate["conversations"]
            .as_array_mut()
            .unwrap()
            .push(base["conversations"][0].clone());
        assert!(
            serde_json::from_value::<Config>(duplicate)
                .unwrap()
                .validate()
                .is_err()
        );
        let mut too_many = base.clone();
        too_many["conversations"] = json!(vec![base["conversations"][0].clone(); 65]);
        assert_eq!(
            serde_json::from_value::<Config>(too_many)
                .unwrap()
                .validate()
                .unwrap_err()
                .code(),
            "conversation_limit"
        );
    }
    #[test]
    fn acl_and_full_configuration_generation_fence_references() {
        let mut config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:7341",
            "public_origin":"https://example.invalid","allowed_logins":["alice","bob"],
            "endpoints":[{"id":"fleet","label":"Fleet","socket":"/unavailable/fleet.sock"}],
            "conversations":[{"id":"secret-id","label":"Registered","team":"research",
                "allowed_logins":["alice"],"native_socket":"/unavailable/native.sock",
                "expected_runtime":"11111111-2222-4333-8444-555555555555","session":"ses_secret"}]})).unwrap();
        config.validate().unwrap();
        let registry = Conversations::new(&config, "epoch").unwrap();
        let list = registry.list("alice");
        let id = list["conversations"][0]["id"].as_str().unwrap();
        assert!(
            registry.list("bob")["conversations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            registry.read("bob", id).unwrap_err().code(),
            "invalid_reference"
        );
        assert_ne!(
            id,
            Conversations::new(&config, "other-epoch")
                .unwrap()
                .list("alice")["conversations"][0]["id"]
        );
        config.endpoints[0].label = "Changed unrelated config".into();
        assert_ne!(
            id,
            Conversations::new(&config, "epoch").unwrap().list("alice")["conversations"][0]["id"]
        );
        config.conversations[0].allowed_logins = vec!["outsider".into()];
        assert!(config.validate().is_err());
        config.conversations[0].allowed_logins = vec!["alice".into()];
        config.conversations[0].native_socket = "ssh://remote/socket".into();
        assert!(config.validate().is_err());
    }
}

#[cfg(all(test, unix))]
#[path = "sharing_tests.rs"]
mod sharing_tests;
