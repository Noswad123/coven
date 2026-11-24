//! One owned scheduler publishes paired transcript-free snapshots for every principal.
use super::*;
use std::{collections::HashMap, sync::Weak};

pub(super) struct Fence {
    conversations: conversations::MetadataProof,
    pub(super) cards: (u64, Option<Instant>),
    attention: u64,
    fleet: Vec<FleetFence>,
}
impl PartialEq for Fence {
    fn eq(&self, other: &Self) -> bool {
        // Source CAS generations fence acquisitions, not committed authority.
        // Renewed expiry evidence never changes a published snapshot's deadline.
        self.conversations.identity == other.conversations.identity
            && self.cards.0 == other.cards.0
            && self.attention == other.attention
            && self.fleet == other.fleet
    }
}
impl Eq for Fence {}
struct FleetFence {
    signature: String,
    expires: Option<Instant>,
}
impl PartialEq for FleetFence {
    fn eq(&self, other: &Self) -> bool {
        // Inventory renewal alone must not invalidate or extend a metadata lease.
        self.signature == other.signature
    }
}
impl Eq for FleetFence {}
impl Fence {
    pub(super) fn read(observer: &Observer, login: &str) -> Self {
        let mut fence = Self::for_list(
            observer,
            observer.conversations.metadata_fence(login),
            observer.attention_revision(login),
        );
        fence.cards = observer.metadata_card_fence(login);
        fence
    }
    pub(super) fn for_list(
        observer: &Observer,
        conversations: conversations::MetadataProof,
        attention: u64,
    ) -> Self {
        Self {
            conversations,
            cards: (0, None),
            attention,
            fleet: observer
                .endpoints
                .iter()
                .filter(|e| e.config.ssh.is_none())
                .map(|e| {
                    let c = e.cache.lock().unwrap_or_else(|p| p.into_inner());
                    let mut identities = c
                        .inventory
                        .iter()
                        .flat_map(|i| &i.agents)
                        .zip(&c.ids)
                        .map(|(a, id)| (id, &a.terminal_id, &a.pane_id, &a.workspace_id, &a.tab_id))
                        .collect::<Vec<_>>();
                    identities.sort_unstable();
                    FleetFence {
                        signature: digest(&[
                            &c.generation.to_string(),
                            c.inventory.as_ref().map_or("", |i| i.generation.as_str()),
                            &serde_json::to_string(&identities).expect("string-only identities"),
                            if c.fresh.is_some_and(|at| at.elapsed() < STALE) {
                                "fresh"
                            } else {
                                "stale"
                            },
                        ]),
                        expires: c.fresh.map(|at| at + STALE),
                    }
                })
                .collect(),
        }
    }
    fn expires(&self, started: Instant) -> Instant {
        self.fleet
            .iter()
            .filter_map(|fleet| fleet.expires)
            .chain(self.conversations.expires)
            .chain(self.cards.1)
            .fold(started + STALE, Instant::min)
    }
}
struct Snapshot {
    generation: String,
    expires: Instant,
    fence: Fence,
    conversations: Value,
    attention: Value,
}
pub(super) struct MetadataService {
    observer: Weak<Observer>,
    epoch: String,
    cache: Arc<Mutex<HashMap<String, Snapshot>>>,
    stop: Arc<MetadataWake>,
    thread: Option<JoinHandle<()>>,
}
#[derive(Default)]
struct WakeState {
    stopped: bool,
    generation: u64,
}
#[derive(Default)]
pub(super) struct MetadataWake {
    state: Mutex<WakeState>,
    changed: Condvar,
}
impl MetadataWake {
    pub(super) fn wake(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.generation = state.generation.wrapping_add(1);
        self.changed.notify_one();
    }
    fn stop(&self) {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).stopped = true;
        self.changed.notify_all();
    }
}

struct Schedule {
    due: Vec<Instant>,
    pending: Vec<bool>,
    last_accel: Vec<Option<Instant>>,
    generation: u64,
    cursor: usize,
}
impl Schedule {
    fn new(count: usize, now: Instant) -> Self {
        Self {
            due: vec![now; count],
            pending: vec![false; count],
            last_accel: vec![None; count],
            generation: 0,
            cursor: 0,
        }
    }
    fn select(&mut self, now: Instant, generation: u64) -> Option<(usize, bool)> {
        if self.generation != generation {
            self.pending.fill(true);
            self.generation = generation;
        }
        let (index, due) = self.due.iter().enumerate().min_by_key(|(_, at)| **at)?;
        let selected = if *due <= now {
            Some((index, false))
        } else {
            (0..self.due.len()).find_map(|offset| {
                let index = (self.cursor + offset) % self.due.len();
                (self.pending[index]
                    && self.last_accel[index].is_none_or(|at| now >= at + INTERVAL))
                .then_some((index, true))
            })
        };
        if let Some((index, accelerated)) = selected {
            // Consume before I/O; a newer generation will mark it pending again.
            self.pending[index] = false;
            if accelerated {
                self.last_accel[index] = Some(now);
                self.cursor = (index + 1) % self.due.len();
            }
        }
        selected
    }
    fn next(&self) -> Option<Instant> {
        self.due
            .iter()
            .copied()
            .chain(self.pending.iter().enumerate().filter_map(|(i, pending)| {
                pending.then(|| self.last_accel[i].map_or_else(Instant::now, |at| at + INTERVAL))
            }))
            .min()
    }
}

fn run_scheduler(wake: &MetadataWake, count: usize, mut work: impl FnMut(usize) -> bool) {
    let mut schedule = Schedule::new(count, Instant::now());
    loop {
        let state = wake.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.stopped {
            break;
        }
        let generation = state.generation;
        let now = Instant::now();
        if let Some((index, accelerated)) = schedule.select(now, generation) {
            drop(state);
            if !work(index) {
                break;
            }
            if !accelerated {
                schedule.due[index] = Instant::now() + INTERVAL;
            }
        } else if let Some(next) = schedule.next() {
            let _ = wake.changed.wait_timeout_while(
                state,
                next.saturating_duration_since(now),
                |state| !state.stopped && state.generation == generation,
            );
        } else {
            break;
        }
    }
}
fn unavailable(attention: bool) -> Value {
    if attention {
        json!({"contract":"coven.attention.v1","state":"unavailable","items":[],"streams":[],"omitted_count":null,"unavailable_count":1,"unlinked_count":0})
    } else {
        json!({"contract":"coven.conversations.v1","state":"unavailable","conversations":[],"sources":[],"omitted_conversations":0})
    }
}
fn collect(
    observer: &Observer,
    login: &str,
    epoch: &str,
    serial: u64,
    cache: &Mutex<HashMap<String, Snapshot>>,
) {
    let started = Instant::now();
    let (conversations, attention, fence) = observer.collect_metadata(login);
    let has_rows = conversations["conversations"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty());
    let incomplete = fence.conversations.card.is_none()
        || conversations["sources"]
            .as_array()
            .is_some_and(|sources| sources.iter().any(|s| s["status"] != "observed"));
    let expires = if Fence::read(observer, login) != fence
        || fence.conversations.identity.is_none()
        || conversations["state"] == "unavailable"
        || (!has_rows && incomplete)
    {
        started
    } else {
        fence.expires(started)
    };
    cache.lock().unwrap_or_else(|p| p.into_inner()).insert(
        login.into(),
        Snapshot {
            generation: format!("{epoch}:{serial}"),
            expires,
            fence,
            conversations,
            attention,
        },
    );
    observer.events.notify(Some(login));
}
impl MetadataService {
    #[cfg(test)]
    pub(super) fn manual(observer: &Arc<Observer>) -> Self {
        Self {
            observer: Arc::downgrade(observer),
            epoch: "testepoch".into(),
            cache: Arc::default(),
            stop: Arc::default(),
            thread: None,
        }
    }
    #[cfg(test)]
    pub(super) fn refresh(&self, login: &str, serial: u64) {
        collect(
            &self.observer.upgrade().unwrap(),
            login,
            &self.epoch,
            serial,
            &self.cache,
        );
    }
    pub(super) fn start(config: &Config, observer: &Arc<Observer>) -> Result<Self> {
        let mut service = Self {
            observer: Arc::downgrade(observer),
            epoch: crate::platform::random_uuid_hex()
                .map_err(|_| Error("observer_start_failed"))?,
            cache: Arc::default(),
            stop: Arc::default(),
            thread: None,
        };
        if !observer.events.register_metadata(&service.stop) {
            return Err(Error("observer_start_failed"));
        }
        let (weak, cache, stop, epoch, logins) = (
            service.observer.clone(),
            service.cache.clone(),
            service.stop.clone(),
            service.epoch.clone(),
            config.allowed_logins.clone(),
        );
        service.thread = Some(
            std::thread::Builder::new()
                .name("observer-metadata".into())
                .spawn(move || {
                    let mut serial = 0u64;
                    run_scheduler(&stop, logins.len(), |index| {
                        let Some(observer) = weak.upgrade() else {
                            return false;
                        };
                        let login = &logins[index];
                        serial += 1;
                        collect(&observer, login, &epoch, serial, &cache);
                        true
                    });
                })
                .map_err(|_| Error("observer_start_failed"))?,
        );
        Ok(service)
    }
    pub(super) fn read(&self, login: &str, attention: bool) -> Value {
        let observer = self.observer.upgrade();
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        let fence = observer.as_ref().map(|o| Fence::read(o, login));
        let snapshot = cache.get_mut(login);
        // Once invalidated, this generation cannot regain authority on a later GET.
        let snapshot = snapshot.map(|s| {
            if let Some(current) = fence.as_ref().filter(|f| *f == &s.fence) {
                // Same identity can acquire a shorter lease. Reads may reduce
                // authority to that evidence, but never extend this generation.
                s.expires = s.expires.min(current.expires(Instant::now()));
            } else {
                s.expires = s.expires.min(Instant::now());
            }
            &*s
        });
        let remaining = snapshot
            .filter(|s| fence.as_ref() == Some(&s.fence))
            .filter(|s| !attention || s.attention["state"] != "unavailable")
            .map_or(0, |s| {
                s.expires
                    .saturating_duration_since(Instant::now())
                    .as_millis() as u64
            });
        let mut value = if remaining == 0 {
            unavailable(attention)
        } else {
            let s = snapshot.unwrap();
            if attention {
                s.attention.clone()
            } else {
                s.conversations.clone()
            }
        };
        value["freshness"] = json!({"generation":snapshot.map(|s| s.generation.clone()).unwrap_or_else(|| format!("{}:0", self.epoch)),"remaining_ms":remaining.min(15000)});
        value
    }
}
impl Drop for MetadataService {
    fn drop(&mut self) {
        self.stop.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Some(observer) = self.observer.upgrade() {
            observer.events.unregister_metadata(&self.stop);
        }
    }
}

#[cfg(test)]
#[path = "metadata_wake_tests.rs"]
mod wake_tests;

#[cfg(test)]
mod tests {
    use super::*;
    struct NoIo;
    impl Transport for NoIo {
        fn call(&self, _: &EndpointConfig, _: &Request, _: Instant) -> Result<Value> {
            panic!("metadata GET must not perform native work")
        }
    }
    pub(super) fn fixture() -> (Arc<Observer>, MetadataService) {
        let observer = Arc::new(Observer {
            events: Arc::default(),
            attention: Mutex::default(),
            cardlinks: Mutex::default(),
            conversations: conversations::Conversations::default(),
            endpoints: vec![],
            allow_output: false,
            transport: Arc::new(NoIo),
            shutdown: Arc::new((Mutex::new(false), Condvar::new())),
            pollers: vec![],
        });
        let service = MetadataService {
            observer: Arc::downgrade(&observer),
            epoch: "epoch".into(),
            cache: Arc::default(),
            stop: Arc::default(),
            thread: None,
        };
        (observer, service)
    }
    fn publish(observer: &Observer, service: &MetadataService, expires: Instant, counter: u64) {
        service.cache.lock().unwrap().insert("alice".into(), Snapshot {
            generation: format!("epoch:{counter}"), expires, fence: Fence::read(observer, "alice"),
            conversations: json!({"contract":"coven.conversations.v1","conversations":[{"id":"opaque"}]}),
            attention: json!({"contract":"coven.attention.v1","state":"observed","items":[{"id":"opaque"}],"streams":[]}),
        });
    }
    #[test]
    fn cache_only_paired_reads_decrease_and_expired_or_missing_are_unknown() {
        let (observer, service) = fixture();
        for attention in [true, false] {
            let missing = service.read("alice", attention);
            assert_eq!(missing["state"], "unavailable");
            assert_eq!(missing["freshness"]["remaining_ms"], 0);
        }
        publish(&observer, &service, Instant::now() + STALE, 1);
        let a = service.read("alice", true);
        std::thread::sleep(Duration::from_millis(5));
        let b = service.read("alice", false);
        assert_eq!(a["freshness"]["generation"], b["freshness"]["generation"]);
        assert!(
            a["freshness"]["remaining_ms"].as_u64().unwrap()
                > b["freshness"]["remaining_ms"].as_u64().unwrap()
        );
        assert_eq!(service.read("bob", true)["state"], "unavailable");
        publish(&observer, &service, Instant::now(), 2);
        for attention in [true, false] {
            let expired = service.read("alice", attention);
            assert_eq!(expired["state"], "unavailable");
            assert_eq!(expired["freshness"]["remaining_ms"], 0);
            assert_eq!(
                expired[if attention { "items" } else { "conversations" }],
                json!([])
            );
        }
    }
    #[test]
    fn superseded_generation_never_revives_and_new_evidence_gets_new_generation() {
        let (observer, service) = fixture();
        publish(&observer, &service, Instant::now() + STALE, 1);
        service
            .cache
            .lock()
            .unwrap()
            .get_mut("alice")
            .unwrap()
            .fence
            .attention += 1;
        assert_eq!(service.read("alice", true)["freshness"]["remaining_ms"], 0);
        service
            .cache
            .lock()
            .unwrap()
            .get_mut("alice")
            .unwrap()
            .fence = Fence::read(&observer, "alice");
        assert_eq!(service.read("alice", false)["freshness"]["remaining_ms"], 0);
        publish(&observer, &service, Instant::now() + STALE, 2);
        let renewed = service.read("alice", true);
        assert_eq!(renewed["freshness"]["generation"], "epoch:2");
        assert!(renewed["freshness"]["remaining_ms"].as_u64().unwrap() > 0);
    }
    #[test]
    fn expiry_is_collection_start_and_earliest_internal_lease_not_response_time() {
        let (observer, _) = fixture();
        let start = Instant::now();
        let mut fence = Fence::read(&observer, "alice");
        assert_eq!(fence.expires(start), start + STALE);
        fence.conversations.expires = Some(start + Duration::from_secs(7));
        fence.cards.1 = Some(start + Duration::from_secs(2));
        assert_eq!(fence.expires(start), start + Duration::from_secs(2));
        fence.fleet.push(FleetFence {
            signature: "fleet".into(),
            expires: Some(start + Duration::from_secs(1)),
        });
        assert_eq!(fence.expires(start), start + Duration::from_secs(1));
    }

    #[test]
    fn scheduler_covers_every_configured_principal_without_viewers_or_push_and_joins() {
        let (observer, _) = fixture();
        let config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:7341","public_origin":"https://example.invalid",
            "allowed_logins":["alice","bob"],"endpoints":[{"id":"one","label":"One","socket":"/absent/fixture.sock"}]})).unwrap();
        let service = MetadataService::start(&config, &observer).unwrap();
        let until = Instant::now() + Duration::from_secs(1);
        loop {
            let cache = service.cache.lock().unwrap();
            if cache.len() == 2 {
                assert!(cache.contains_key("alice") && cache.contains_key("bob"));
                break;
            }
            drop(cache);
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(2));
        }
        for login in ["alice", "bob"] {
            let doc = service.read(login, true);
            assert_eq!(doc["state"], "unavailable");
            assert_eq!(doc["freshness"]["remaining_ms"], 0);
        }
        let weak = Arc::downgrade(&observer);
        drop(observer);
        let start = Instant::now();
        drop(service);
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(weak.upgrade().is_none());
    }
}
