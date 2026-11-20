use super::tests::{Fake, config, doc, fixture, idle, until};
use super::*;

fn scoped(stream: &str, alert: Option<&str>, complete: bool) -> Value {
    let mut value = alert
        .map(doc)
        .unwrap_or_else(|| json!({"contract":"coven.attention.v1","state":"partial","items":[]}));
    value["streams"] = json!([{"id":stream,"complete":complete,"alert_ids":alert.into_iter().collect::<Vec<_>>()}]);
    value
}
fn service(
    root: &tempfile::TempDir,
    collect: Collect,
    sender: Arc<dyn Sender>,
) -> Arc<PushService> {
    PushService::start_with(&config(root), collect, sender)
        .unwrap()
        .unwrap()
}
fn register(service: &PushService) -> String {
    let (sub, _, _) = fixture("synthetic-review");
    service
        .subscribe("alice", &serde_json::to_value(sub).unwrap())
        .unwrap()["subscription_id"]
        .as_str()
        .unwrap()
        .to_owned()
}
fn manual(root: &tempfile::TempDir, collect: Collect, sender: Arc<dyn Sender>) -> Arc<PushService> {
    let mut service = service(root, collect, sender);
    service.inner.work.lock().unwrap().stop = true;
    service.inner.wake.notify_all();
    for thread in Arc::get_mut(&mut service).unwrap().threads.drain(..) {
        thread.join().unwrap();
    }
    service.inner.work.lock().unwrap().stop = false;
    service
}
#[test]
fn more_than_64_confirmed_retired_occurrences_deliver_and_rotation_dedups() {
    let root = tempfile::tempdir().unwrap();
    let stream = "a".repeat(64);
    let current = Arc::new(Mutex::new(scoped(&stream, None, true)));
    let c = current.clone();
    let sender = Fake::new(&[]);
    let service = service(
        &root,
        Arc::new(move |_| c.lock().unwrap().clone()),
        sender.clone(),
    );
    let id = register(&service);
    for n in 1..=70 {
        let alert = format!("{n:064x}");
        let mut sample = scoped(&stream, Some(&alert), true);
        sample["items"][0]["can_respond"] = json!(false);
        sample["items"][0]["details_available"] = json!(false);
        *current.lock().unwrap() = sample;
        service.inner.poll("alice");
        until(|| {
            service.inner.state.lock().unwrap().records[&id]
                .seen
                .get(&alert)
                .is_some_and(|n| n.phase == ledger::Phase::Sent)
        });
        assert_eq!(sender.messages.lock().unwrap().len(), n);
        assert!(service.inner.state.lock().unwrap().records[&id].seen.len() <= 1);
        current.lock().unwrap()["items"][0]["id"] = json!(format!("{n:064x}-new-grant"));
        service.inner.poll("alice");
        assert_eq!(sender.messages.lock().unwrap().len(), n);
    }
    *current.lock().unwrap() = scoped(&stream, None, true);
    service.inner.poll("alice");
    assert!(
        service.inner.state.lock().unwrap().records[&id]
            .seen
            .is_empty()
    );
}
#[test]
fn retirement_needs_the_matching_complete_stream_and_retention_is_bounded() {
    let root = tempfile::tempdir().unwrap();
    let service = service(&root, idle(), Fake::new(&[]));
    let id = register(&service);
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    let x = "1".repeat(64);
    let y = "2".repeat(64);
    let legacy = "3".repeat(64);
    let at = ledger::now();
    service
        .inner
        .mutate(|s| {
            for (alert, stream) in [
                (&x, Some(a.clone())),
                (&y, Some(b.clone())),
                (&legacy, None),
            ] {
                s.records.get_mut(&id).unwrap().seen.insert(
                    alert.clone(),
                    ledger::Notice {
                        stream,
                        at,
                        phase: ledger::Phase::Sent,
                        token: "0".repeat(32),
                    },
                );
            }
            Ok(())
        })
        .unwrap();
    let sample = ledger::Snapshot::read(&scoped(&b, None, true)).unwrap();
    service.inner.reconcile("alice", Some(&sample), at).unwrap();
    assert!(
        service.inner.state.lock().unwrap().records[&id]
            .seen
            .contains_key(&x)
    );
    assert!(
        !service.inner.state.lock().unwrap().records[&id]
            .seen
            .contains_key(&y)
    );
    assert!(
        service.inner.state.lock().unwrap().records[&id]
            .seen
            .contains_key(&legacy)
    );
    for doc in [
        scoped(&a, None, false),
        json!({"contract":"coven.attention.v1","state":"observed","items":[]}),
        json!({"contract":"coven.attention.v1","state":"unavailable","items":[]}),
    ] {
        service
            .inner
            .reconcile("alice", ledger::Snapshot::read(&doc).as_ref(), at)
            .unwrap();
        assert_eq!(
            service.inner.state.lock().unwrap().records[&id].seen.len(),
            2
        );
    }
    service
        .inner
        .reconcile("alice", None, at + ledger::RETENTION + 1)
        .unwrap();
    assert!(
        service.inner.state.lock().unwrap().records[&id]
            .seen
            .is_empty()
    );
}

struct BlockedRetry {
    calls: std::sync::atomic::AtomicUsize,
    entered: std::sync::mpsc::Sender<()>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}
impl Sender for BlockedRetry {
    fn send(&self, _: web_push::WebPushMessage) -> Outcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        Outcome::Retry
    }
}
#[test]
fn every_retry_requires_current_positive_metadata_and_never_retargets() {
    for mode in ["unavailable", "absent", "other"] {
        let root = tempfile::tempdir().unwrap();
        let stream = "a".repeat(64);
        let alert = "1".repeat(64);
        let current = Arc::new(Mutex::new(scoped(&stream, Some(&alert), true)));
        let c = current.clone();
        let (entered, entry) = std::sync::mpsc::channel();
        let (release, receiver) = std::sync::mpsc::channel();
        let sender = Arc::new(BlockedRetry {
            calls: std::sync::atomic::AtomicUsize::new(0),
            entered,
            release: Mutex::new(receiver),
        });
        let service = manual(
            &root,
            Arc::new(move |_| c.lock().unwrap().clone()),
            sender.clone(),
        );
        let id = register(&service);
        service.inner.poll("alice");
        let job = service
            .inner
            .work
            .lock()
            .unwrap()
            .queue
            .pop_front()
            .unwrap();
        let inner = service.inner.clone();
        let thread = std::thread::spawn(move || send_job(&inner, job));
        entry.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut sample = scoped(&stream, None, mode == "absent");
        if mode == "unavailable" {
            sample["state"] = json!("unavailable");
        }
        if mode == "other" {
            sample = scoped(&stream, Some(&"2".repeat(64)), false);
        }
        *current.lock().unwrap() = sample;
        release.send(()).unwrap();
        thread.join().unwrap();
        let retry = service
            .inner
            .work
            .lock()
            .unwrap()
            .queue
            .pop_front()
            .unwrap();
        send_job(&service.inner, retry);
        assert_eq!(sender.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            service.inner.state.lock().unwrap().records[&id]
                .seen
                .contains_key(&alert),
            mode != "absent"
        );
    }
}
#[test]
fn queued_unknown_or_resolved_jobs_never_post() {
    for known in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let stream = "a".repeat(64);
        let alert = "1".repeat(64);
        let current = Arc::new(Mutex::new(scoped(&stream, None, known)));
        let c = current.clone();
        let sender = Fake::new(&[]);
        let service = manual(
            &root,
            Arc::new(move |_| c.lock().unwrap().clone()),
            sender.clone(),
        );
        let id = register(&service);
        service
            .inner
            .mutate(|s| {
                s.records.get_mut(&id).unwrap().seen.insert(
                    alert.clone(),
                    ledger::Notice {
                        stream: Some(stream.clone()),
                        at: ledger::now(),
                        phase: ledger::Phase::Pending,
                        token: "0".repeat(32),
                    },
                );
                Ok(())
            })
            .unwrap();
        let record = service.inner.state.lock().unwrap().records[&id].clone();
        service.inner.work.lock().unwrap().queue.push_back(Job {
            id: id.clone(),
            generation: record.generation,
            token: record.seen[&alert].token.clone(),
            subscription: record.subscription,
            owner: "alice".into(),
            alert: alert.clone(),
            created: Instant::now(),
            due: Instant::now(),
            attempts: 1,
        });
        let job = service
            .inner
            .work
            .lock()
            .unwrap()
            .queue
            .pop_front()
            .unwrap();
        send_job(&service.inner, job);
        assert!(sender.messages.lock().unwrap().is_empty());
        assert_eq!(
            service.inner.state.lock().unwrap().records[&id]
                .seen
                .contains_key(&alert),
            !known
        );
    }
}
#[test]
fn storage_degradation_during_inflight_attempt_fences_every_later_attempt() {
    let root = tempfile::tempdir().unwrap();
    let (entered, entry) = std::sync::mpsc::channel();
    let (release, receiver) = std::sync::mpsc::channel();
    let sender = Arc::new(BlockedRetry {
        calls: std::sync::atomic::AtomicUsize::new(0),
        entered,
        release: Mutex::new(receiver),
    });
    let service = manual(&root, Arc::new(|_| doc(&"a".repeat(64))), sender.clone());
    let id = register(&service);
    service.inner.poll("alice");
    let job = service
        .inner
        .work
        .lock()
        .unwrap()
        .queue
        .pop_front()
        .unwrap();
    let inner = service.inner.clone();
    let thread = std::thread::spawn(move || send_job(&inner, job));
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    service.inner.store.fault.store(2, Ordering::SeqCst);
    assert!(service.unsubscribe("alice", &id).is_err());
    release.send(()).unwrap();
    thread.join().unwrap();
    assert!(service.inner.work.lock().unwrap().queue.is_empty());
    assert_eq!(sender.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        service.status("alice")["delivery_status"],
        "storage_degraded"
    );
}
#[test]
fn committed_fsync_failure_updates_ram_revokes_delivery_and_cannot_clobber_disk() {
    let root = tempfile::tempdir().unwrap();
    let cfg = config(&root);
    let sender = Fake::new(&[]);
    let service = service(&root, idle(), sender.clone());
    let id = register(&service);
    let public = service.status("alice")["public_key"].clone();
    service.inner.store.fault.store(1, Ordering::SeqCst);
    assert!(service.unsubscribe("alice", &id).is_err());
    assert_eq!(service.status("alice")["subscriptions"], json!([id]));
    assert!(service.registration_allowed("alice"));
    service.inner.store.fault.store(2, Ordering::SeqCst);
    assert!(service.unsubscribe("alice", &id).is_err());
    assert_eq!(service.status("alice")["subscriptions"], json!([]));
    assert_eq!(
        service.status("alice")["delivery_status"],
        "storage_degraded"
    );
    assert!(!service.registration_allowed("alice"));
    let disk = service
        .inner
        .store
        .read("subscriptions.json")
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<State>(&disk)
            .unwrap()
            .records
            .len(),
        0
    );
    service.inner.store.fault.store(0, Ordering::SeqCst);
    let (sub, _, _) = fixture("synthetic-after-uncertain");
    assert!(
        service
            .subscribe("alice", &serde_json::to_value(sub).unwrap())
            .is_err()
    );
    assert_eq!(
        service
            .inner
            .store
            .read("subscriptions.json")
            .unwrap()
            .unwrap(),
        disk
    );
    assert!(sender.messages.lock().unwrap().is_empty());
    drop(service);
    let service = PushService::start_with(&cfg, idle(), Fake::new(&[]))
        .unwrap()
        .unwrap();
    assert_eq!(service.status("alice")["subscriptions"], json!([]));
    assert_eq!(service.status("alice")["public_key"], public);
}
#[test]
fn failed_history_recovers_after_a_bounded_cooldown_not_a_lifetime_ban() {
    let root = tempfile::tempdir().unwrap();
    let sender = Fake::new(&[Outcome::Failed]);
    let alert = "9".repeat(64);
    let a = alert.clone();
    let service = service(&root, Arc::new(move |_| doc(&a)), sender.clone());
    let id = register(&service);
    until(|| service.delivery_status() == "delivery_failed");
    service
        .inner
        .mutate(|s| {
            s.records
                .get_mut(&id)
                .unwrap()
                .seen
                .get_mut(&alert)
                .unwrap()
                .at = ledger::now() - ledger::RECOVERY;
            Ok(())
        })
        .unwrap();
    service.inner.poll("alice");
    until(|| {
        service.inner.state.lock().unwrap().records[&id].seen[&alert].phase == ledger::Phase::Sent
    });
    assert_eq!(sender.messages.lock().unwrap().len(), 2);
}

#[test]
fn global_history_capacity_fits_disk_bound_and_retention_releases_it() {
    let root = tempfile::tempdir().unwrap();
    let sender = Fake::new(&[]);
    let service = manual(&root, Arc::new(|_| doc(&"f".repeat(64))), sender.clone());
    let id = register(&service);
    service
        .inner
        .mutate(|state| {
            let base = state.records[&id].clone();
            for n in 0..63 {
                let mut r = base.clone();
                r.owner = format!("revoked{n}");
                r.owner.push_str(&"x".repeat(254 - r.owner.len()));
                r.subscription.endpoint =
                    format!("https://fcm.googleapis.com/synthetic-capacity-{n}-");
                r.subscription
                    .endpoint
                    .push_str(&"x".repeat(2048 - r.subscription.endpoint.len()));
                if n < 16 {
                    for i in 0..ledger::SUB_LIMIT {
                        r.seen.insert(
                            format!("{:064x}", n * 256 + i),
                            ledger::Notice {
                                stream: Some("a".repeat(64)),
                                at: ledger::now(),
                                phase: ledger::Phase::Pending,
                                token: "0".repeat(32),
                            },
                        );
                    }
                }
                state.records.insert(r.subscription.id(), r);
            }
            assert_eq!(
                state.records.values().map(|r| r.seen.len()).sum::<usize>(),
                ledger::GLOBAL_LIMIT
            );
            assert!(serde_json::to_vec(state).unwrap().len() <= store::LIMIT);
            Ok(())
        })
        .unwrap();
    service.inner.poll("alice");
    assert_eq!(service.delivery_status(), "history_or_state_limit");
    assert!(service.inner.work.lock().unwrap().queue.is_empty());
    service
        .inner
        .reconcile("alice", None, ledger::now() + ledger::RETENTION + 1)
        .unwrap();
    service.inner.poll("alice");
    let job = service
        .inner
        .work
        .lock()
        .unwrap()
        .queue
        .pop_front()
        .unwrap();
    send_job(&service.inner, job);
    assert_eq!(sender.messages.lock().unwrap().len(), 1);
}

fn next_job(service: &PushService) -> Job {
    service
        .inner
        .work
        .lock()
        .unwrap()
        .queue
        .pop_front()
        .unwrap()
}

#[test]
fn precommit_reconcile_failure_preserves_the_job_and_retries_within_original_ttl() {
    let root = tempfile::tempdir().unwrap();
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    let stream_a = "1".repeat(64);
    let stream_b = "2".repeat(64);
    let current = Arc::new(Mutex::new(scoped(&stream_a, Some(&a), true)));
    let c = current.clone();
    let sender = Fake::new(&[]);
    let service = manual(
        &root,
        Arc::new(move |_| c.lock().unwrap().clone()),
        sender.clone(),
    );
    let id = register(&service);
    service.inner.poll("alice");
    service
        .inner
        .mutate(|s| {
            s.records.get_mut(&id).unwrap().seen.insert(
                b.clone(),
                ledger::Notice {
                    stream: Some(stream_b.clone()),
                    at: ledger::now(),
                    phase: ledger::Phase::Sent,
                    token: "0".repeat(32),
                },
            );
            Ok(())
        })
        .unwrap();
    current.lock().unwrap()["streams"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":stream_b,"complete":true,"alert_ids":[]}));
    let mut job = next_job(&service);
    job.created -= Duration::from_secs(30);
    job.attempts = 1;
    let created = job.created;
    let token = job.token.clone();
    service.inner.store.fault.store(1, Ordering::SeqCst);
    send_job(&service.inner, job);
    assert_eq!(service.delivery_status(), "storage_retry");
    assert!(sender.messages.lock().unwrap().is_empty());
    let retry = next_job(&service);
    assert_eq!(retry.created, created);
    assert_eq!(retry.attempts, 1);
    assert_eq!(retry.token, token);
    {
        let state = service.inner.state.lock().unwrap();
        assert!(state.records[&id].seen[&a].phase == ledger::Phase::Pending);
        assert!(state.records[&id].seen[&b].phase == ledger::Phase::Sent);
    }
    service.inner.store.fault.store(0, Ordering::SeqCst);
    send_job(&service.inner, retry);
    let messages = sender.messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].ttl > 0 && messages[0].ttl <= 270);
    let state = service.inner.state.lock().unwrap();
    assert!(state.records[&id].seen[&a].phase == ledger::Phase::Sent);
    assert!(!state.records[&id].seen.contains_key(&b));
}

#[test]
fn unknown_before_first_transport_resumes_without_consuming_attempts_or_resetting_ttl() {
    let root = tempfile::tempdir().unwrap();
    let alert = "a".repeat(64);
    let positive = scoped(&"1".repeat(64), Some(&alert), true);
    let current = Arc::new(Mutex::new(positive.clone()));
    let c = current.clone();
    let sender = Fake::new(&[]);
    let service = manual(
        &root,
        Arc::new(move |_| c.lock().unwrap().clone()),
        sender.clone(),
    );
    let id = register(&service);
    service.inner.poll("alice");
    let mut job = next_job(&service);
    job.created -= Duration::from_secs(30);
    let created = job.created;
    let token = job.token.clone();
    current.lock().unwrap()["state"] = json!("unavailable");
    send_job(&service.inner, job);
    assert!(sender.messages.lock().unwrap().is_empty());
    let retry = next_job(&service);
    assert_eq!(retry.created, created);
    assert_eq!(retry.attempts, 0);
    assert_eq!(retry.token, token);
    assert!(
        service.inner.state.lock().unwrap().records[&id].seen[&alert].phase
            == ledger::Phase::Pending
    );
    *current.lock().unwrap() = positive;
    send_job(&service.inner, retry);
    let messages = sender.messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].ttl > 0 && messages[0].ttl <= 270);
    assert!(
        service.inner.state.lock().unwrap().records[&id].seen[&alert].phase == ledger::Phase::Sent
    );
}

#[test]
fn rotation_recovers_unknown_suspended_work_with_new_keys_and_preserves_sent_history() {
    let root = tempfile::tempdir().unwrap();
    let alert = "a".repeat(64);
    let sent = "b".repeat(64);
    let mut positive = scoped(&"1".repeat(64), Some(&alert), true);
    positive["streams"][0]["alert_ids"]
        .as_array_mut()
        .unwrap()
        .push(json!(sent));
    let current = Arc::new(Mutex::new(positive.clone()));
    let c = current.clone();
    let sender = Fake::new(&[]);
    let service = manual(
        &root,
        Arc::new(move |_| c.lock().unwrap().clone()),
        sender.clone(),
    );
    let id = register(&service);
    service.inner.poll("alice");
    service
        .inner
        .mutate(|s| {
            s.records.get_mut(&id).unwrap().seen.insert(
                sent.clone(),
                ledger::Notice {
                    stream: Some("1".repeat(64)),
                    at: ledger::now(),
                    phase: ledger::Phase::Sent,
                    token: "0".repeat(32),
                },
            );
            Ok(())
        })
        .unwrap();
    current.lock().unwrap()["state"] = json!("unavailable");
    send_job(&service.inner, next_job(&service));
    let old = next_job(&service);
    let old_generation = old.generation.clone();
    let record = service.inner.state.lock().unwrap().records[&id].clone();
    let (mut rotated, private, auth) = fixture("ignored-rotation-path");
    rotated.endpoint = record.subscription.endpoint;
    let value = serde_json::to_value(&rotated).unwrap();
    service.subscribe("alice", &value).unwrap();
    assert!(service.subscribe("bob", &value).is_err());
    {
        let state = service.inner.state.lock().unwrap();
        assert!(state.records[&id].seen[&alert].phase == ledger::Phase::Ready);
        assert!(state.records[&id].seen[&sent] == record.seen[&sent]);
    }
    send_job(&service.inner, old);
    assert!(sender.messages.lock().unwrap().is_empty());
    *current.lock().unwrap() = positive;
    service.inner.poll("alice");
    let new = next_job(&service);
    assert_ne!(new.generation, old_generation);
    assert!(new.subscription == rotated);
    send_job(&service.inner, new);
    let messages = sender.messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    let plaintext = ece::decrypt(
        &private,
        &auth,
        &messages[0].payload.as_ref().unwrap().content,
    )
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&plaintext).unwrap(),
        json!({"version":1,"alert_id":alert})
    );
    assert!(service.inner.state.lock().unwrap().records[&id].seen[&sent] == record.seen[&sent]);
}

#[test]
fn failed_expiry_bookkeeping_has_bounded_orphan_recovery_without_restart() {
    let root = tempfile::tempdir().unwrap();
    let alert = "a".repeat(64);
    let positive = scoped(&"1".repeat(64), Some(&alert), true);
    let current = Arc::new(Mutex::new(positive.clone()));
    let c = current.clone();
    let sender = Fake::new(&[]);
    let service = manual(
        &root,
        Arc::new(move |_| c.lock().unwrap().clone()),
        sender.clone(),
    );
    let id = register(&service);
    service.inner.poll("alice");
    let mut job = next_job(&service);
    job.created -= Duration::from_secs(301);
    service.inner.store.fault.store(1, Ordering::SeqCst);
    send_job(&service.inner, job);
    assert!(service.inner.work.lock().unwrap().queue.is_empty());
    assert!(
        service.inner.state.lock().unwrap().records[&id].seen[&alert].phase
            == ledger::Phase::Pending
    );
    service.inner.store.fault.store(0, Ordering::SeqCst);
    let at = ledger::now() - ledger::PENDING_LEASE;
    service
        .inner
        .mutate(|s| {
            s.records
                .get_mut(&id)
                .unwrap()
                .seen
                .get_mut(&alert)
                .unwrap()
                .at = at;
            Ok(())
        })
        .unwrap();
    current.lock().unwrap()["state"] = json!("unavailable");
    service.inner.poll("alice");
    assert!(service.inner.work.lock().unwrap().queue.is_empty());
    {
        let state = service.inner.state.lock().unwrap();
        let notice = &state.records[&id].seen[&alert];
        assert!(notice.phase == ledger::Phase::Ready);
        assert_eq!(notice.at, at);
        assert_eq!(notice.stream.as_deref(), Some("1".repeat(64).as_str()));
    }
    assert!(sender.messages.lock().unwrap().is_empty());
    *current.lock().unwrap() = positive;
    service.inner.poll("alice");
    send_job(&service.inner, next_job(&service));
    assert_eq!(sender.messages.lock().unwrap().len(), 1);
}

#[test]
fn pending_startup_recovery_requires_fresh_positive_metadata_without_failure_cooldown() {
    let root = tempfile::tempdir().unwrap();
    let alert = "a".repeat(64);
    let a = alert.clone();
    let sender = Fake::new(&[]);
    let service = manual(&root, Arc::new(move |_| doc(&a)), sender.clone());
    register(&service);
    service.inner.poll("alice");
    assert!(!service.inner.work.lock().unwrap().queue.is_empty());
    assert!(sender.messages.lock().unwrap().is_empty());
    drop(service);
    let a = alert.clone();
    let service =
        PushService::start_with(&config(&root), Arc::new(move |_| doc(&a)), sender.clone())
            .unwrap()
            .unwrap();
    until(|| service.delivery_status() == "sent");
    assert_eq!(sender.messages.lock().unwrap().len(), 1);
}

#[test]
fn old_inflight_receipt_cannot_mark_the_new_generation_sent() {
    struct ReceiptGate {
        calls: std::sync::atomic::AtomicUsize,
        entered: std::sync::mpsc::Sender<()>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl Sender for ReceiptGate {
        fn send(&self, _: web_push::WebPushMessage) -> Outcome {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                self.entered.send(()).unwrap();
                self.release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            Outcome::Sent
        }
    }
    let root = tempfile::tempdir().unwrap();
    let alert = "a".repeat(64);
    let a = alert.clone();
    let (entered, entry) = std::sync::mpsc::channel();
    let (release, receiver) = std::sync::mpsc::channel();
    let sender = Arc::new(ReceiptGate {
        calls: std::sync::atomic::AtomicUsize::new(0),
        entered,
        release: Mutex::new(receiver),
    });
    let service = manual(&root, Arc::new(move |_| doc(&a)), sender.clone());
    let id = register(&service);
    service.inner.poll("alice");
    let job = next_job(&service);
    let inner = service.inner.clone();
    let thread = std::thread::spawn(move || send_job(&inner, job));
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    let (mut rotated, _, _) = fixture("ignored-inflight-path");
    rotated.endpoint = service.inner.state.lock().unwrap().records[&id]
        .subscription
        .endpoint
        .clone();
    service
        .subscribe("alice", &serde_json::to_value(rotated).unwrap())
        .unwrap();
    service.inner.poll("alice");
    release.send(()).unwrap();
    thread.join().unwrap();
    assert!(
        service.inner.state.lock().unwrap().records[&id].seen[&alert].phase
            == ledger::Phase::Pending
    );
    send_job(&service.inner, next_job(&service));
    assert!(
        service.inner.state.lock().unwrap().records[&id].seen[&alert].phase == ledger::Phase::Sent
    );
    // The provider-accepted old send cannot be retracted: generic duplicates are
    // possible across rotation, but an old receipt is never new-generation proof.
    assert_eq!(sender.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn same_generation_replacement_fences_expired_old_jobs_and_bookkeeping() {
    for retired in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let alert = "a".repeat(64);
        let stream = "1".repeat(64);
        let positive = scoped(&stream, Some(&alert), true);
        let current = Arc::new(Mutex::new(positive.clone()));
        let c = current.clone();
        let sender = Fake::new(&[]);
        let service = manual(
            &root,
            Arc::new(move |_| c.lock().unwrap().clone()),
            sender.clone(),
        );
        let id = register(&service);
        service.inner.poll("alice");
        let mut old = next_job(&service);
        old.created -= Duration::from_secs(ledger::PENDING_LEASE + 1);
        if retired {
            *current.lock().unwrap() = scoped(&stream, None, true);
            service.inner.poll("alice");
            assert!(
                !service.inner.state.lock().unwrap().records[&id]
                    .seen
                    .contains_key(&alert)
            );
            *current.lock().unwrap() = positive;
        } else {
            service
                .inner
                .mutate(|s| {
                    s.records
                        .get_mut(&id)
                        .unwrap()
                        .seen
                        .get_mut(&alert)
                        .unwrap()
                        .at = ledger::now() - ledger::PENDING_LEASE - 1;
                    Ok(())
                })
                .unwrap();
        }
        service.inner.poll("alice");
        let replacement = next_job(&service);
        assert_eq!(replacement.generation, old.generation);
        assert_ne!(replacement.token, old.token);
        assert!(!service.inner.eligible(&old));
        assert!(service.inner.eligible(&replacement));
        let pending = service.inner.state.lock().unwrap().records[&id].seen[&alert].clone();
        let disk: State = serde_json::from_slice(
            &service
                .inner
                .store
                .read("subscriptions.json")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(disk.records[&id].seen[&alert].token, replacement.token);
        assert!(
            !service
                .status("alice")
                .to_string()
                .contains(&replacement.token)
        );
        // Fence mutation as well as the initial eligibility check: expiration
        // or any late bookkeeping must not fail the replacement's admission.
        service.inner.finish(&old, ledger::Phase::Failed);
        send_job(&service.inner, old);
        assert!(service.inner.state.lock().unwrap().records[&id].seen[&alert] == pending);
        assert!(sender.messages.lock().unwrap().is_empty());
        assert!(service.inner.eligible(&replacement));
        send_job(&service.inner, replacement);
        assert_eq!(sender.messages.lock().unwrap().len(), 1);
        assert!(
            service.inner.state.lock().unwrap().records[&id].seen[&alert].phase
                == ledger::Phase::Sent
        );
    }
}

#[test]
fn same_generation_replacement_fences_every_inflight_outcome() {
    struct ReceiptGate {
        first: Outcome,
        calls: std::sync::atomic::AtomicUsize,
        entered: std::sync::mpsc::Sender<()>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl Sender for ReceiptGate {
        fn send(&self, _: web_push::WebPushMessage) -> Outcome {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                self.entered.send(()).unwrap();
                self.release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                self.first
            } else {
                Outcome::Sent
            }
        }
    }
    for first in [
        Outcome::Sent,
        Outcome::Failed,
        Outcome::Retry,
        Outcome::Gone,
    ] {
        let root = tempfile::tempdir().unwrap();
        let alert = "a".repeat(64);
        let a = alert.clone();
        let (entered, entry) = std::sync::mpsc::channel();
        let (release, receiver) = std::sync::mpsc::channel();
        let sender = Arc::new(ReceiptGate {
            first,
            calls: std::sync::atomic::AtomicUsize::new(0),
            entered,
            release: Mutex::new(receiver),
        });
        let service = manual(&root, Arc::new(move |_| doc(&a)), sender.clone());
        let id = register(&service);
        service.inner.poll("alice");
        let old = next_job(&service);
        let generation = old.generation.clone();
        let old_token = old.token.clone();
        let inner = service.inner.clone();
        let thread = std::thread::spawn(move || send_job(&inner, old));
        entry.recv_timeout(Duration::from_secs(5)).unwrap();
        service
            .inner
            .mutate(|s| {
                s.records
                    .get_mut(&id)
                    .unwrap()
                    .seen
                    .get_mut(&alert)
                    .unwrap()
                    .at = ledger::now() - ledger::PENDING_LEASE - 1;
                Ok(())
            })
            .unwrap();
        service.inner.poll("alice");
        let replacement = next_job(&service);
        assert_eq!(replacement.generation, generation);
        assert_ne!(replacement.token, old_token);
        let pending = service.inner.state.lock().unwrap().records[&id].seen[&alert].clone();
        release.send(()).unwrap();
        thread.join().unwrap();
        assert!(service.inner.state.lock().unwrap().records[&id].seen[&alert] == pending);
        assert!(service.inner.work.lock().unwrap().queue.is_empty());
        assert!(service.inner.eligible(&replacement));
        send_job(&service.inner, replacement);
        assert!(
            service.inner.state.lock().unwrap().records[&id].seen[&alert].phase
                == ledger::Phase::Sent
        );
        assert_eq!(sender.calls.load(Ordering::SeqCst), 2);
    }
}
