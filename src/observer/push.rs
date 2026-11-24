//! Opt-in generic Web Push. Metadata is a locator, never decision authority.
mod delivery;
mod ledger;
#[cfg(test)]
mod review_tests;
mod store;
#[cfg(test)]
mod tests;

use super::Config;
use crate::{Error, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use delivery::{Outcome, Sender};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

const CONTRACT: &str = "coven.web-push.v1";
type Collect = Arc<dyn Fn(&str) -> Value + Send + Sync>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushConfig {
    pub state_directory: String,
}
impl PushConfig {
    pub fn validate(&self) -> Result<()> {
        let p = &self.state_directory;
        if !p.starts_with('/')
            || p.len() > 4096
            || p == "/"
            || p.ends_with('/')
            || p.chars().any(char::is_control)
            || p.split('/')
                .skip(1)
                .any(|p| p.is_empty() || matches!(p, "." | ".."))
        {
            return Err(Error("push_invalid_directory"));
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Keys {
    p256dh: String,
    auth: String,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Subscription {
    endpoint: String,
    keys: Keys,
}
impl Subscription {
    fn validate(&self) -> Result<()> {
        delivery::endpoint(&self.endpoint)?;
        if self.keys.auth.len() != 22 || self.keys.p256dh.len() != 87 {
            return Err(Error("push_invalid_subscription"));
        }
        let auth = URL_SAFE_NO_PAD
            .decode(&self.keys.auth)
            .map_err(|_| Error("push_invalid_subscription"))?;
        let public = URL_SAFE_NO_PAD
            .decode(&self.keys.p256dh)
            .map_err(|_| Error("push_invalid_subscription"))?;
        if auth.len() != 16 || public.len() != 65 || public[0] != 4 {
            return Err(Error("push_invalid_subscription"));
        }
        let group = openssl::ec::EcGroup::from_curve_name(openssl::nid::Nid::X9_62_PRIME256V1)
            .map_err(|_| Error("push_crypto_failed"))?;
        let mut ctx = openssl::bn::BigNumContext::new().map_err(|_| Error("push_crypto_failed"))?;
        let point = openssl::ec::EcPoint::from_bytes(&group, &public, &mut ctx)
            .map_err(|_| Error("push_invalid_subscription"))?;
        if point.is_infinity(&group) || !point.is_on_curve(&group, &mut ctx).unwrap_or(false) {
            return Err(Error("push_invalid_subscription"));
        }
        Ok(())
    }
    fn id(&self) -> String {
        format!("{:x}", Sha256::digest(self.endpoint.as_bytes()))
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    owner: String,
    generation: String,
    subscription: Subscription,
    seen: BTreeMap<String, ledger::Notice>,
}
impl Record {
    fn pending_job(&self, job: &Job) -> bool {
        self.owner == job.owner
            && self.generation == job.generation
            && self.subscription == job.subscription
            && self
                .seen
                .get(&job.alert)
                .is_some_and(|n| n.phase == ledger::Phase::Pending && n.token == job.token)
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u8,
    records: BTreeMap<String, Record>,
}
struct Job {
    id: String,
    generation: String,
    token: String,
    subscription: Subscription,
    owner: String,
    alert: String,
    created: Instant,
    due: Instant,
    attempts: u8,
}
struct Work {
    queue: VecDeque<Job>,
    stop: bool,
}
struct Inner {
    allowed: BTreeSet<String>,
    state: Mutex<State>,
    // Serializes disk transactions, not native callbacks or network operations.
    writes: Mutex<()>,
    store: store::Store,
    key: Vec<u8>,
    public_key: String,
    subject: String,
    work: Mutex<Work>,
    wake: Condvar,
    collect: Collect,
    sender: Arc<dyn Sender>,
    diagnostic: Mutex<&'static str>,
    degraded: AtomicBool,
    #[cfg(test)]
    before_final_sample: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}
pub struct PushService {
    inner: Arc<Inner>,
    threads: Vec<JoinHandle<()>>,
}

impl PushService {
    /// `collect` must read the shared metadata cache only, including current
    /// freshness/fences. It is also called at final network admission. Drop joins
    /// this service's collector and bounded transport worker.
    pub fn start(
        config: &Config,
        collect: Arc<dyn Fn(&str) -> Value + Send + Sync>,
    ) -> Result<Option<Arc<PushService>>> {
        if config.push.is_none() {
            return Ok(None);
        }
        Self::start_with(config, collect, Arc::new(delivery::Network))
    }
    fn start_with(
        config: &Config,
        collect: Collect,
        sender: Arc<dyn Sender>,
    ) -> Result<Option<Arc<Self>>> {
        let Some(push) = &config.push else {
            return Ok(None);
        };
        config.validate()?;
        let store = store::Store::new(&push.state_directory)?;
        let key = match store.read("vapid.pem")? {
            Some(key) => key,
            None => {
                // Never rotate a missing key underneath an existing subscription database.
                if store.read("subscriptions.json")?.is_some() {
                    return Err(Error("push_missing_key"));
                }
                let group =
                    openssl::ec::EcGroup::from_curve_name(openssl::nid::Nid::X9_62_PRIME256V1)
                        .map_err(|_| Error("push_crypto_failed"))?;
                let ec = openssl::ec::EcKey::generate(&group)
                    .map_err(|_| Error("push_crypto_failed"))?;
                let key = ec
                    .private_key_to_pem()
                    .map_err(|_| Error("push_crypto_failed"))?;
                if store.write("vapid.pem", &key)? != store::Commit::Durable {
                    return Err(Error("push_durability_uncertain"));
                }
                key
            }
        };
        if key.len() > 4096 {
            return Err(Error("push_invalid_key"));
        }
        let vapid = web_push::VapidSignatureBuilder::from_pem_no_sub(key.as_slice())
            .map_err(|_| Error("push_invalid_key"))?;
        let public_key = URL_SAFE_NO_PAD.encode(vapid.get_public_key());
        let state: State = match store.read("subscriptions.json")? {
            Some(bytes) => {
                serde_json::from_slice(&bytes).map_err(|_| Error("push_corrupt_state"))?
            }
            None => State {
                version: 2,
                records: BTreeMap::new(),
            },
        };
        if state.version != 2
            || state.records.len() > 64
            || state.records.values().map(|r| r.seen.len()).sum::<usize>() > ledger::GLOBAL_LIMIT
        {
            return Err(Error("push_corrupt_state"));
        }
        let mut owners = BTreeMap::<&str, usize>::new();
        for (id, record) in &state.records {
            record.subscription.validate()?;
            let n = owners.entry(&record.owner).or_default();
            *n += 1;
            if *n > 8
                || record.owner.is_empty()
                || record.owner.len() > 254
                || !record.owner.is_ascii()
                || record
                    .owner
                    .bytes()
                    .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
                || *id != record.subscription.id()
                || record.generation.len() != 32
                || !record
                    .generation
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || record.seen.len() > ledger::SUB_LIMIT
                || record.seen.iter().any(|(id, notice)| {
                    !super::valid_id(id)
                        || notice.token.len() != 32
                        || !notice
                            .token
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                        || notice.at > ledger::now().saturating_add(300)
                        || notice.stream.as_ref().is_some_and(|s| !super::valid_id(s))
                })
            {
                return Err(Error("push_corrupt_state"));
            }
        }
        let allowed = config
            .allowed_logins
            .iter()
            .filter(|login| {
                config
                    .conversation_sources
                    .iter()
                    .any(|source| source.allowed_logins.contains(login))
                    || config
                        .conversations
                        .iter()
                        .any(|source| source.allowed_logins.contains(login))
            })
            .cloned()
            .collect();
        let inner = Arc::new(Inner {
            allowed,
            state: Mutex::new(state),
            writes: Mutex::new(()),
            store,
            key,
            public_key,
            subject: config.public_origin.clone(),
            work: Mutex::new(Work {
                queue: VecDeque::new(),
                stop: false,
            }),
            wake: Condvar::new(),
            collect,
            sender,
            diagnostic: Mutex::new("idle"),
            degraded: AtomicBool::new(false),
            #[cfg(test)]
            before_final_sample: Mutex::new(None),
        });
        let mut service = Self {
            inner,
            threads: Vec::new(),
        };
        // A crash can leave a durable pending marker without an in-memory job.
        // Recover it through the normal finite-attempt path, never as a sent alert.
        service.inner.mutate(|state| {
            for r in state.records.values_mut() {
                for notice in r.seen.values_mut() {
                    if notice.phase == ledger::Phase::Pending {
                        notice.phase = ledger::Phase::Ready;
                    }
                }
            }
            Ok(())
        })?;
        for (name, worker) in [
            ("push-collector", collector as fn(Arc<Inner>)),
            ("push-sender", send_loop as fn(Arc<Inner>)),
        ] {
            let inner = service.inner.clone();
            service.threads.push(
                std::thread::Builder::new()
                    .name(name.into())
                    .spawn(move || worker(inner))
                    .map_err(|_| Error("push_start_failed"))?,
            );
        }
        Ok(Some(Arc::new(service)))
    }
    pub fn registration_allowed(&self, login: &str) -> bool {
        self.inner.allowed.contains(login) && !self.inner.degraded.load(Ordering::Acquire)
    }
    pub fn status(&self, login: &str) -> Value {
        let enabled = self.inner.allowed.contains(login);
        let subscriptions: Vec<_> = if enabled {
            self.inner
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .records
                .iter()
                .filter(|(_, r)| r.owner == login)
                .map(|(id, _)| id.clone())
                .collect()
        } else {
            Vec::new()
        };
        json!({"contract":CONTRACT,"enabled":enabled,"public_key":if enabled {Some(&self.inner.public_key)} else {None},"subscriptions":subscriptions,"delivery_status":if enabled {self.delivery_status()} else {"disabled"}})
    }
    pub fn subscribe(&self, login: &str, value: &Value) -> Result<Value> {
        if !self.registration_allowed(login) {
            return Err(Error("push_forbidden"));
        }
        // Caller supplies only the closed subscription, never the HTTP envelope.
        if value.as_object().is_none_or(|o| o.len() != 2)
            || value["endpoint"].as_str().is_none_or(|s| s.len() > 2048)
            || value["keys"].as_object().is_none_or(|o| o.len() != 2)
            || value["keys"]["p256dh"]
                .as_str()
                .is_none_or(|s| s.len() != 87)
            || value["keys"]["auth"].as_str().is_none_or(|s| s.len() != 22)
        {
            return Err(Error("push_invalid_subscription"));
        }
        let subscription: Subscription = serde_json::from_value(value.clone())
            .map_err(|_| Error("push_invalid_subscription"))?;
        subscription.validate()?;
        let id = subscription.id();
        self.inner.mutate(|state| {
            if let Some(old) = state.records.get_mut(&id) {
                if old.owner != login {
                    return Err(Error("push_forbidden"));
                }
                if old.subscription != subscription {
                    old.generation = crate::platform::random_uuid_hex()
                        .map_err(|_| Error("push_state_unavailable"))?;
                    // Old-generation jobs/receipts cannot update the new one.
                    // Reinspect unsent work without discarding sent dedup history.
                    for notice in old.seen.values_mut() {
                        if notice.phase == ledger::Phase::Pending {
                            notice.phase = ledger::Phase::Ready;
                        }
                    }
                }
                old.subscription = subscription;
            } else {
                if state.records.len() >= 64
                    || state.records.values().filter(|r| r.owner == login).count() >= 8
                {
                    return Err(Error("push_subscription_limit"));
                }
                state.records.insert(
                    id.clone(),
                    Record {
                        owner: login.into(),
                        generation: crate::platform::random_uuid_hex()
                            .map_err(|_| Error("push_state_unavailable"))?,
                        subscription,
                        seen: BTreeMap::new(),
                    },
                );
            }
            Ok(())
        })?;
        self.inner.wake.notify_all();
        Ok(json!({"contract":CONTRACT,"subscription_id":id,"subscribed":true}))
    }
    pub fn unsubscribe(&self, login: &str, id: &str) -> Result<Value> {
        if !self.registration_allowed(login) || !super::valid_id(id) {
            return Err(Error("push_forbidden"));
        }
        self.inner.mutate(|state| {
            if state.records.get(id).is_some_and(|r| r.owner != login) {
                return Err(Error("push_forbidden"));
            }
            state.records.remove(id);
            Ok(())
        })?;
        Ok(json!({"contract":CONTRACT,"subscription_id":id,"subscribed":false}))
    }
    /// Closed diagnostics only; never expose provider errors, endpoints or keys.
    pub fn delivery_status(&self) -> &'static str {
        if self.inner.degraded.load(Ordering::Acquire) {
            return "storage_degraded";
        }
        *self
            .inner
            .diagnostic
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }
}
impl Drop for PushService {
    fn drop(&mut self) {
        self.inner
            .work
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .stop = true;
        self.inner.wake.notify_all();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}
impl Inner {
    fn sample(&self, owner: &str) -> Option<(ledger::Snapshot, Instant)> {
        // Anchor before the cache read: cloning/validation and lock waits spend
        // the lease too. A copied remaining_ms must never start a new lease.
        let started = Instant::now();
        let document = (self.collect)(owner);
        let freshness = document["freshness"].as_object()?;
        let generation = freshness.get("generation")?.as_str()?;
        let remaining = freshness.get("remaining_ms")?.as_u64()?;
        if freshness.len() != 2
            || generation.is_empty()
            || generation.len() > 128
            || !generation.is_ascii()
            || !generation.contains(':')
            || !(1..=15000).contains(&remaining)
        {
            return None;
        }
        let expires = started + Duration::from_millis(remaining);
        let mut snapshot = ledger::Snapshot::read(&document)?;
        snapshot.expires = Some(expires);
        (Instant::now() < expires).then_some((snapshot, expires))
    }
    fn mutate(&self, change: impl FnOnce(&mut State) -> Result<()>) -> Result<()> {
        let _write = self.writes.lock().unwrap_or_else(|p| p.into_inner());
        if self.degraded.load(Ordering::Acquire) {
            return Err(Error("push_storage_degraded"));
        }
        let mut next = self.state.lock().unwrap_or_else(|p| p.into_inner()).clone();
        change(&mut next)?;
        if next == *self.state.lock().unwrap_or_else(|p| p.into_inner()) {
            return Ok(());
        }
        let bytes = serde_json::to_vec(&next).map_err(|_| Error("push_state_unavailable"))?;
        let commit = self.store.write("subscriptions.json", &bytes)?;
        if commit == store::Commit::Uncertain {
            self.degraded.store(true, Ordering::Release);
        }
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = next;
        if commit == store::Commit::Uncertain {
            self.wake.notify_all();
            return Err(Error("push_durability_uncertain"));
        }
        Ok(())
    }
    fn stopped(&self) -> bool {
        self.work.lock().unwrap_or_else(|p| p.into_inner()).stop
    }
    fn wait(&self, delay: Duration) {
        let work = self.work.lock().unwrap_or_else(|p| p.into_inner());
        if work.stop {
            return;
        }
        let _ = self.wake.wait_timeout(work, delay);
    }
    fn diagnostic(&self, code: &'static str) {
        *self.diagnostic.lock().unwrap_or_else(|p| p.into_inner()) = code;
    }
    fn eligible(&self, job: &Job) -> bool {
        !self.degraded.load(Ordering::Acquire)
            && self.allowed.contains(&job.owner)
            && self
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .records
                .get(&job.id)
                .is_some_and(|r| r.pending_job(job))
    }
    fn try_admit(&self, job: &Job) -> Option<bool> {
        let work = self.work.try_lock().ok()?;
        let state = self.state.try_lock().ok()?;
        Some(
            !work.stop
                && !self.degraded.load(Ordering::Acquire)
                && self.allowed.contains(&job.owner)
                && state
                    .records
                    .get(&job.id)
                    .is_some_and(|r| r.pending_job(job)),
        )
    }
}
fn collector(inner: Arc<Inner>) {
    let mut last = String::new();
    while !inner.stopped() {
        let owners: BTreeSet<_> = inner
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .records
            .values()
            .filter(|r| inner.allowed.contains(&r.owner))
            .map(|r| r.owner.clone())
            .collect();
        let owner = owners
            .iter()
            .find(|s| **s > last)
            .or_else(|| owners.first())
            .cloned();
        if let Some(owner) = owner {
            last = owner.clone();
            inner.poll(&owner);
        }
        inner.wait(Duration::from_secs(3));
    }
}
fn send_loop(inner: Arc<Inner>) {
    while !inner.stopped() {
        let job = {
            let mut work = inner.work.lock().unwrap_or_else(|p| p.into_inner());
            let index = work.queue.iter().position(|j| j.due <= Instant::now());
            index.and_then(|i| work.queue.remove(i))
        };
        let Some(job) = job else {
            inner.wait(Duration::from_millis(100));
            continue;
        };
        send_job(&inner, job);
    }
}
fn send_job(inner: &Inner, mut job: Job) {
    if !inner.eligible(&job) {
        return;
    }
    if job.created.elapsed() >= Duration::from_secs(300) {
        inner.finish(&job, ledger::Phase::Failed);
        return;
    }
    // A queued locator is not authority. Every attempt, including retries,
    // needs a new positive metadata sample outside every state/work lock.
    let sample = inner.sample(&job.owner);
    if let Err(error) = inner.reconcile(&job.owner, sample.as_ref().map(|(s, _)| s), ledger::now())
    {
        if error.code() == "push_state_unavailable" && !inner.degraded.load(Ordering::Acquire) {
            // No commit occurred. Preserve this job's TTL and transport budget;
            // nothing may reach the network until reconciliation succeeds.
            inner.diagnostic("storage_retry");
            inner.requeue(job, Duration::from_secs(3));
        } else {
            inner.diagnostic("state_failed");
        }
        return;
    }
    if !inner.eligible(&job) {
        return;
    }
    if !sample
        .as_ref()
        .is_some_and(|(s, expires)| s.alerts.contains(&job.alert) && Instant::now() < *expires)
    {
        inner.diagnostic("eligibility_unknown");
        inner.requeue(job, Duration::from_secs(3));
        return;
    }
    if job.created.elapsed() >= Duration::from_secs(300) {
        inner.finish(&job, ledger::Phase::Failed);
        return;
    }
    let ttl = 300u32.saturating_sub(job.created.elapsed().as_secs() as u32);
    let mut message = match delivery::message(
        &job.subscription,
        &inner.key,
        &inner.subject,
        &job.alert,
        ttl,
    ) {
        Ok(m) => m,
        Err(_) => {
            inner.diagnostic("crypto_failed");
            inner.finish(&job, ledger::Phase::Failed);
            return;
        }
    };
    #[cfg(test)]
    {
        let hook = inner.before_final_sample.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
    }
    if inner.stopped() || !inner.eligible(&job) {
        return;
    }
    // The cache read can block too. Recheck cancellation afterwards without
    // waiting on admission locks, then spend only the remaining sample lease.
    let current = inner.sample(&job.owner);
    if job.created.elapsed() >= Duration::from_secs(300) {
        inner.finish(&job, ledger::Phase::Failed);
        return;
    }
    message.ttl = message
        .ttl
        .min(300u32.saturating_sub(job.created.elapsed().as_secs() as u32));
    match inner.try_admit(&job) {
        Some(true) => {}
        Some(false) => return,
        None => {
            inner.requeue(job, Duration::from_secs(3));
            return;
        }
    }
    if !current
        .as_ref()
        .is_some_and(|(s, expires)| s.alerts.contains(&job.alert) && Instant::now() < *expires)
    {
        inner.diagnostic("eligibility_unknown");
        inner.requeue(job, Duration::from_secs(3));
        return;
    }
    let outcome = inner.sender.send(message);
    // An admission may have been retired/replaced while transport was in flight.
    if !inner.eligible(&job) {
        return;
    }
    match outcome {
        Outcome::Sent => {
            inner.finish(&job, ledger::Phase::Sent);
            inner.diagnostic("sent");
        }
        Outcome::Gone => {
            let result = inner.mutate(|state| {
                if state
                    .records
                    .get(&job.id)
                    .is_some_and(|r| r.pending_job(&job))
                {
                    state.records.remove(&job.id);
                }
                Ok(())
            });
            inner.diagnostic(if result.is_ok() {
                "subscription_expired"
            } else {
                "state_failed"
            });
        }
        Outcome::Retry if job.attempts < 2 => {
            job.attempts += 1;
            let delay = Duration::from_secs(1 << job.attempts);
            inner.requeue(job, delay);
            inner.diagnostic("retrying");
        }
        _ => {
            inner.finish(&job, ledger::Phase::Failed);
            inner.diagnostic("delivery_failed");
        }
    }
}
