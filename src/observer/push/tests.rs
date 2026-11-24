use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    sync::atomic::{AtomicUsize, Ordering},
};

pub(super) struct Fake {
    pub messages: Mutex<Vec<web_push::WebPushMessage>>,
    results: Mutex<VecDeque<Outcome>>,
}
impl Fake {
    pub fn new(results: &[Outcome]) -> Arc<Self> {
        Arc::new(Self {
            messages: Mutex::new(Vec::new()),
            results: Mutex::new(results.iter().copied().collect()),
        })
    }
}
impl Sender for Fake {
    fn send(&self, message: web_push::WebPushMessage) -> Outcome {
        self.messages.lock().unwrap().push(message);
        self.results
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Outcome::Sent)
    }
}
pub(super) fn config(root: &tempfile::TempDir) -> Config {
    serde_json::from_value(json!({"listen":"127.0.0.1:7341","public_origin":"https://example.invalid","allowed_logins":["alice","bob","blocked"],"endpoints":[{"id":"fleet","label":"Fleet","socket":"/absent/fleet.sock"}],"conversation_sources":[{"id":"source","label":"Source","team":"team","directory":"/absent","allowed_logins":["alice","bob"]}],"push":{"state_directory":root.path().join("push")}})).unwrap()
}
pub(super) fn fixture(path: &str) -> (Subscription, ece::EcKeyComponents, [u8; 16]) {
    let (key, auth) = ece::generate_keypair_and_auth_secret().unwrap();
    (
        Subscription {
            endpoint: format!("https://fcm.googleapis.com/{path}"),
            keys: Keys {
                p256dh: URL_SAFE_NO_PAD.encode(key.pub_as_raw().unwrap()),
                auth: URL_SAFE_NO_PAD.encode(auth),
            },
        },
        key.raw_components().unwrap(),
        auth,
    )
}
pub(super) fn idle() -> Collect {
    Arc::new(|_| json!({"contract":"coven.attention.v1","state":"unavailable","items":[]}))
}
pub(super) fn doc(alert: &str) -> Value {
    json!({"contract":"coven.attention.v1","state":"partial","freshness":{"generation":"epoch:1","remaining_ms":15000},"items":[{"id":"unused","alert_id":alert,"kind":"question","can_respond":true,"details_available":true,"team":"SECRET","question":"SECRET"}]})
}
pub(super) fn until(f: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while !f() {
        assert!(Instant::now() < deadline, "worker deadline");
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn disabled_and_unsubscribed_never_collect() {
    let root = tempfile::tempdir().unwrap();
    let mut cfg = config(&root);
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let collect: Collect = Arc::new(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        Value::Null
    });
    cfg.push = None;
    assert!(PushService::start(&cfg, collect.clone()).unwrap().is_none());
    assert!(!root.path().join("push").exists());
    cfg.push = config(&root).push;
    let sender = Fake::new(&[]);
    let service = PushService::start_with(&cfg, collect, sender.clone())
        .unwrap()
        .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(sender.messages.lock().unwrap().is_empty());
    assert!(!service.registration_allowed("blocked"));
    assert_eq!(
        service.status("blocked"),
        json!({"contract":CONTRACT,"enabled":false,"public_key":null,"subscriptions":[],"delivery_status":"disabled"})
    );
    let now = Instant::now();
    drop(service);
    assert!(now.elapsed() < Duration::from_secs(1));
}
#[test]
fn isolation_endpoint_ownership_and_restart_key() {
    let root = tempfile::tempdir().unwrap();
    let cfg = config(&root);
    let (sub, _, _) = fixture("synthetic-isolation");
    let value = serde_json::to_value(&sub).unwrap();
    let service = PushService::start_with(&cfg, idle(), Fake::new(&[]))
        .unwrap()
        .unwrap();
    let public = service.status("alice")["public_key"].clone();
    let id = service.subscribe("alice", &value).unwrap()["subscription_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(id, sub.id());
    assert!(service.subscribe("bob", &value).is_err());
    assert!(service.unsubscribe("bob", &id).is_err());
    assert!(service.subscribe("blocked", &value).is_err());
    assert_eq!(service.status("bob")["subscriptions"], json!([]));
    assert!(PushService::start_with(&cfg, idle(), Fake::new(&[])).is_err());
    drop(service);
    let service = PushService::start_with(&cfg, idle(), Fake::new(&[]))
        .unwrap()
        .unwrap();
    assert_eq!(service.status("alice")["public_key"], public);
    assert_eq!(service.status("alice")["subscriptions"], json!([id]));
    service.unsubscribe("alice", &id).unwrap();
    service.subscribe("bob", &value).unwrap();
}
#[test]
fn private_storage_rejects_aliases_modes_links_corruption_and_size() {
    let root = tempfile::tempdir().unwrap();
    let cfg = config(&root);
    let service = PushService::start_with(&cfg, idle(), Fake::new(&[]))
        .unwrap()
        .unwrap();
    drop(service);
    let dir = root.path().join("push");
    let key = dir.join("vapid.pem");
    let saved = fs::read(&key).unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(PushService::start_with(&cfg, idle(), Fake::new(&[])).is_err());
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    fs::hard_link(&key, dir.join("alias")).unwrap();
    assert!(PushService::start_with(&cfg, idle(), Fake::new(&[])).is_err());
    fs::remove_file(dir.join("alias")).unwrap();
    fs::rename(&key, dir.join("original")).unwrap();
    symlink("original", &key).unwrap();
    assert!(PushService::start_with(&cfg, idle(), Fake::new(&[])).is_err());
    fs::remove_file(&key).unwrap();
    fs::rename(dir.join("original"), &key).unwrap();
    let store = store::Store::new(dir.to_str().unwrap()).unwrap();
    store.write("subscriptions.json", b"not json").unwrap();
    drop(store);
    assert!(PushService::start_with(&cfg, idle(), Fake::new(&[])).is_err());
    fs::write(dir.join("subscriptions.json"), vec![b' '; store::LIMIT + 1]).unwrap();
    assert!(PushService::start_with(&cfg, idle(), Fake::new(&[])).is_err());
    assert_eq!(fs::read(key).unwrap(), saved);
    let alias = root.path().join("alias-dir");
    symlink(&dir, &alias).unwrap();
    assert!(store::Store::new(alias.to_str().unwrap()).is_err());
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(store::Store::new(dir.to_str().unwrap()).is_err());
}
#[test]
fn closed_config_and_subscription_and_vendor_policy() {
    for path in [
        "/", "relative", "/a//b", "/a/../b", "/a/./b", "/a/", "/a\nb",
    ] {
        assert!(
            PushConfig {
                state_directory: path.into()
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        serde_json::from_value::<PushConfig>(
            json!({"state_directory":"/private/push","endpoint":"x"})
        )
        .is_err()
    );
    for endpoint in [
        "http://fcm.googleapis.com/path",
        "https://fcm.googleapis.com:444/path",
        "https://evil.push.services.mozilla.com.evil/path",
        "https://user@fcm.googleapis.com/path",
        "https://fcm.googleapis.com/path#x",
        "https://127.0.0.1/path",
        "https://fcm.googleapis.com/",
        "https://fcm.googleapis.com/../path",
        "https://fcm.googleapis.com\\@evil/path",
    ] {
        assert!(delivery::endpoint(endpoint).is_err(), "{endpoint}");
    }
    for host in [
        "fcm.googleapis.com",
        "web.push.apple.com",
        "push.services.mozilla.com",
        "updates.push.services.mozilla.com",
    ] {
        assert!(delivery::endpoint(&format!("https://{host}/synthetic")).is_ok());
    }
    for ip in [
        "0.0.0.0",
        "127.0.0.1",
        "10.1.2.3",
        "100.64.0.1",
        "100.127.255.255",
        "169.254.1.1",
        "172.16.0.1",
        "192.168.1.1",
        "192.0.2.1",
        "198.51.100.1",
        "203.0.113.1",
        "198.19.0.1",
        "224.0.0.1",
        "255.255.255.255",
        "::1",
        "::ffff:8.8.8.8",
        "fc00::1",
        "fe80::1",
        "2001:db8::1",
        "2002:0808:0808::1",
        "3fff::1",
    ] {
        assert!(!delivery::public(ip.parse().unwrap()), "{ip}");
    }
    for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
        assert!(delivery::public(ip.parse().unwrap()));
    }
    let (mut sub, _, _) = fixture("synthetic-keys");
    sub.validate().unwrap();
    sub.keys.auth.push('=');
    assert!(sub.validate().is_err());
    sub.keys.auth.pop();
    sub.keys.p256dh = URL_SAFE_NO_PAD.encode([4u8; 65]);
    assert!(sub.validate().is_err());
    let (sub, _, _) = fixture("synthetic-closed");
    let mut value = serde_json::to_value(sub).unwrap();
    value["expirationTime"] = Value::Null;
    assert!(serde_json::from_value::<Subscription>(value).is_err());
}
#[test]
fn background_delivery_decrypt_vapid_dedup_partial_and_restart() {
    let root = tempfile::tempdir().unwrap();
    let cfg = config(&root);
    let alert = "a".repeat(64);
    let a = alert.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let collect: Collect = Arc::new(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        doc(&a)
    });
    let sender = Fake::new(&[]);
    let service = PushService::start_with(&cfg, collect.clone(), sender.clone())
        .unwrap()
        .unwrap();
    let (sub, private, auth) = fixture("synthetic-crypto");
    service
        .subscribe("alice", &serde_json::to_value(&sub).unwrap())
        .unwrap();
    until(|| !sender.messages.lock().unwrap().is_empty());
    {
        let messages = sender.messages.lock().unwrap();
        let m = &messages[0];
        assert!(m.ttl <= 300);
        let payload = m.payload.as_ref().unwrap();
        let plaintext = ece::decrypt(&private, &auth, &payload.content).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&plaintext).unwrap(),
            json!({"version":1,"alert_id":alert})
        );
        let authorization = payload
            .crypto_headers
            .iter()
            .find(|(k, _)| *k == "Authorization")
            .unwrap()
            .1
            .clone();
        let token = authorization
            .split("t=")
            .nth(1)
            .unwrap()
            .split(',')
            .next()
            .unwrap();
        let pubkey = URL_SAFE_NO_PAD
            .decode(service.status("alice")["public_key"].as_str().unwrap())
            .unwrap();
        use jwt_simple::prelude::*;
        let public = ES256PublicKey::from_bytes(&pubkey).unwrap();
        let claims = public
            .verify_token::<BTreeMap<String, Value>>(token, None)
            .unwrap();
        assert_eq!(claims.subject.as_deref(), Some("https://example.invalid"));
        assert!(claims.custom.is_empty());
        assert!(
            claims.audiences.unwrap().contains(
                &["https://fcm.googleapis.com".to_owned()]
                    .into_iter()
                    .collect()
            )
        );
    }
    until(|| calls.load(Ordering::SeqCst) >= 2);
    service.inner.poll("alice");
    assert_eq!(sender.messages.lock().unwrap().len(), 1);
    drop(service);
    let again = Fake::new(&[]);
    let service = PushService::start_with(&cfg, collect, again.clone())
        .unwrap()
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(again.messages.lock().unwrap().is_empty());
    drop(service);
}
#[test]
fn transport_retry_and_gone_removal() {
    let root = tempfile::tempdir().unwrap();
    let cfg = config(&root);
    let sender = Fake::new(&[Outcome::Retry, Outcome::Gone]);
    let service = PushService::start_with(&cfg, Arc::new(|_| doc(&"b".repeat(64))), sender.clone())
        .unwrap()
        .unwrap();
    let (sub, _, _) = fixture("synthetic-gone");
    service
        .subscribe("alice", &serde_json::to_value(sub).unwrap())
        .unwrap();
    until(|| sender.messages.lock().unwrap().len() >= 2);
    until(|| service.status("alice")["subscriptions"] == json!([]));
    assert_eq!(service.delivery_status(), "subscription_expired");
}
#[test]
fn acl_removal_and_stale_jobs_do_not_send() {
    let root = tempfile::tempdir().unwrap();
    let mut cfg = config(&root);
    let sender = Fake::new(&[]);
    let service = PushService::start_with(&cfg, idle(), sender.clone())
        .unwrap()
        .unwrap();
    let (sub, _, _) = fixture("synthetic-acl");
    service
        .subscribe("alice", &serde_json::to_value(&sub).unwrap())
        .unwrap();
    let id = sub.id();
    let generation = service.inner.state.lock().unwrap().records[&id]
        .generation
        .clone();
    let old = Instant::now() - Duration::from_secs(301);
    service.inner.work.lock().unwrap().queue.push_back(Job {
        id,
        generation,
        token: "0".repeat(32),
        subscription: sub,
        owner: "alice".into(),
        alert: "c".repeat(64),
        created: old,
        due: old,
        attempts: 0,
    });
    std::thread::sleep(Duration::from_millis(200));
    assert!(sender.messages.lock().unwrap().is_empty());
    drop(service);
    cfg.conversation_sources[0].allowed_logins = vec!["bob".into()];
    let service = PushService::start_with(
        &cfg,
        Arc::new(|_| panic!("revoked collection")),
        sender.clone(),
    )
    .unwrap()
    .unwrap();
    assert!(!service.registration_allowed("alice"));
    std::thread::sleep(Duration::from_millis(150));
    assert!(sender.messages.lock().unwrap().is_empty());
}

#[test]
fn retry_exhaustion_and_subscription_limit_are_bounded() {
    let root = tempfile::tempdir().unwrap();
    let cfg = config(&root);
    let sender = Fake::new(&[Outcome::Retry, Outcome::Retry, Outcome::Retry]);
    let service = PushService::start_with(&cfg, Arc::new(|_| doc(&"d".repeat(64))), sender.clone())
        .unwrap()
        .unwrap();
    let (sub, _, _) = fixture("synthetic-retry");
    service
        .subscribe("alice", &serde_json::to_value(sub).unwrap())
        .unwrap();
    until(|| service.delivery_status() == "delivery_failed");
    assert_eq!(sender.messages.lock().unwrap().len(), 3);
    // Explicit refresh/grant rotations cannot reset the occurrence ledger.
    service.inner.wake.notify_all();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(sender.messages.lock().unwrap().len(), 3);
    for n in 1..8 {
        let (sub, _, _) = fixture(&format!("synthetic-limit-{n}"));
        service
            .subscribe("alice", &serde_json::to_value(sub).unwrap())
            .unwrap();
    }
    let (sub, _, _) = fixture("synthetic-over-limit");
    assert!(
        service
            .subscribe("alice", &serde_json::to_value(sub).unwrap())
            .is_err()
    );
}

#[test]
fn reader_metadata_without_writer_authority_or_details_still_alerts() {
    let root = tempfile::tempdir().unwrap();
    let cfg = config(&root);
    let sender = Fake::new(&[]);
    let collect: Collect = Arc::new(|_| {
        let mut value = doc(&"e".repeat(64));
        value["items"][0]["can_respond"] = json!(false);
        value["items"][0]["details_available"] = json!(false);
        value
    });
    let service = PushService::start_with(&cfg, collect, sender.clone())
        .unwrap()
        .unwrap();
    let (sub, _, _) = fixture("synthetic-no-authority");
    service
        .subscribe("alice", &serde_json::to_value(sub).unwrap())
        .unwrap();
    until(|| !sender.messages.lock().unwrap().is_empty());
    assert!(
        service
            .inner
            .state
            .lock()
            .unwrap()
            .records
            .values()
            .all(|r| !r.seen.is_empty())
    );
}

#[test]
fn resubscription_fences_old_jobs_and_rotation_keeps_dedup() {
    let root = tempfile::tempdir().unwrap();
    let cfg = config(&root);
    let service = PushService::start_with(&cfg, idle(), Fake::new(&[]))
        .unwrap()
        .unwrap();
    let (sub, _, _) = fixture("synthetic-aba");
    let value = serde_json::to_value(&sub).unwrap();
    service.subscribe("alice", &value).unwrap();
    let id = sub.id();
    let generation = service.inner.state.lock().unwrap().records[&id]
        .generation
        .clone();
    let job = Job {
        id: id.clone(),
        generation,
        token: "0".repeat(32),
        subscription: sub.clone(),
        owner: "alice".into(),
        alert: "a".repeat(64),
        created: Instant::now(),
        due: Instant::now(),
        attempts: 0,
    };
    service
        .inner
        .mutate(|state| {
            state.records.get_mut(&id).unwrap().seen.insert(
                job.alert.clone(),
                ledger::Notice {
                    stream: None,
                    at: ledger::now(),
                    phase: ledger::Phase::Pending,
                    token: job.token.clone(),
                },
            );
            Ok(())
        })
        .unwrap();
    assert!(service.inner.eligible(&job));
    service.unsubscribe("alice", &id).unwrap();
    service.subscribe("alice", &value).unwrap();
    assert!(!service.inner.eligible(&job));
    service
        .inner
        .mutate(|state| {
            state.records.get_mut(&id).unwrap().seen.insert(
                "b".repeat(64),
                ledger::Notice {
                    stream: None,
                    at: ledger::now(),
                    phase: ledger::Phase::Sent,
                    token: "0".repeat(32),
                },
            );
            Ok(())
        })
        .unwrap();
    let (mut rotated, _, _) = fixture("ignored");
    rotated.endpoint = sub.endpoint;
    service
        .subscribe("alice", &serde_json::to_value(rotated).unwrap())
        .unwrap();
    assert!(
        service.inner.state.lock().unwrap().records[&id]
            .seen
            .contains_key(&"b".repeat(64))
    );
}

#[test]
fn global_subscription_limit_and_restart_validation() {
    let root = tempfile::tempdir().unwrap();
    let mut cfg = config(&root);
    cfg.allowed_logins = (0..9).map(|n| format!("principal{n}")).collect();
    cfg.conversation_sources[0].allowed_logins = cfg.allowed_logins.clone();
    let service = PushService::start_with(&cfg, idle(), Fake::new(&[]))
        .unwrap()
        .unwrap();
    let (mut sub, _, _) = fixture("synthetic-global");
    for owner in 0..8 {
        for n in 0..8 {
            sub.endpoint = format!("https://fcm.googleapis.com/synthetic-{owner}-{n}");
            service
                .subscribe(
                    &format!("principal{owner}"),
                    &serde_json::to_value(&sub).unwrap(),
                )
                .unwrap();
        }
    }
    assert_eq!(service.inner.state.lock().unwrap().records.len(), 64);
    sub.endpoint = "https://fcm.googleapis.com/synthetic-65".into();
    assert!(
        service
            .subscribe("principal8", &serde_json::to_value(sub).unwrap())
            .is_err()
    );
    drop(service);
    let service = PushService::start_with(&cfg, idle(), Fake::new(&[]))
        .unwrap()
        .unwrap();
    assert_eq!(service.inner.state.lock().unwrap().records.len(), 64);
}

#[test]
fn mutations_do_not_wait_for_native_callback_and_drop_joins_it() {
    use std::sync::mpsc;
    let root = tempfile::tempdir().unwrap();
    let cfg = config(&root);
    let (entered, entry) = mpsc::channel();
    let (release, receiver) = mpsc::channel();
    let receiver = Mutex::new(receiver);
    let collect: Collect = Arc::new(move |_| {
        entered.send(()).unwrap();
        receiver
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        Value::Null
    });
    let service = PushService::start_with(&cfg, collect, Fake::new(&[]))
        .unwrap()
        .unwrap();
    let (sub, _, _) = fixture("synthetic-join");
    service
        .subscribe("alice", &serde_json::to_value(&sub).unwrap())
        .unwrap();
    entry.recv_timeout(Duration::from_secs(4)).unwrap();
    service.unsubscribe("alice", &sub.id()).unwrap();
    let (finished, done) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        drop(service);
        finished.send(()).unwrap();
    });
    assert!(done.recv_timeout(Duration::from_millis(100)).is_err());
    release.send(()).unwrap();
    done.recv_timeout(Duration::from_secs(2)).unwrap();
    thread.join().unwrap();
}
