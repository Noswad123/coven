use super::*;
use std::sync::mpsc;

struct Runner {
    wake: Arc<MetadataWake>,
    thread: Option<JoinHandle<()>>,
}
impl Runner {
    fn start(count: usize, work: impl FnMut(usize) -> bool + Send + 'static) -> Self {
        let wake = Arc::new(MetadataWake::default());
        let worker = wake.clone();
        Self {
            wake,
            thread: Some(std::thread::spawn(move || {
                run_scheduler(&worker, count, work)
            })),
        }
    }
}
impl Drop for Runner {
    fn drop(&mut self) {
        self.wake.stop();
        self.thread.take().unwrap().join().unwrap();
    }
}
fn config() -> Config {
    serde_json::from_value(json!({"listen":"127.0.0.1:7341",
        "public_origin":"https://example.invalid", "allowed_logins":["alice","bob"],
        "endpoints":[{"id":"one","label":"One","socket":"/absent/fixture.sock"}]}))
    .unwrap()
}

#[test]
fn sleeping_scheduler_wakes_before_periodic_and_browser_notifications_do_not_wake() {
    let (tx, rx) = mpsc::channel();
    let runner = Runner::start(2, move |index| tx.send(index).is_ok());
    let events = events::Events::default();
    assert!(events.register_metadata(&runner.wake));
    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), 0);
    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
    for _ in 0..1000 {
        events.notify(None);
        events.notify(Some("alice"));
    }
    assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());
    events.wake_metadata();
    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), 0);
    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
    for _ in 0..10000 {
        events.wake_metadata();
    }
    assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());
    let start = Instant::now();
    drop(runner);
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn periodic_priority_round_robin_and_independent_acceleration_budget() {
    let now = Instant::now();
    let mut schedule = Schedule::new(3, now);
    for index in 0..3 {
        assert_eq!(schedule.select(now, 1), Some((index, false)));
        schedule.due[index] = now + INTERVAL;
    }
    // A recent regular collection does not spend the event-only budget.
    let event = now + Duration::from_millis(10);
    for index in 0..3 {
        assert_eq!(schedule.select(event, 2), Some((index, true)));
    }
    for generation in 3..10000 {
        assert_eq!(schedule.select(event, generation), None);
    }
    // All overdue regular work wins, even with pending event work.
    let due = now + INTERVAL;
    for index in 0..3 {
        assert_eq!(schedule.select(due, 10000), Some((index, false)));
        schedule.due[index] = due + INTERVAL;
    }
    assert_eq!(schedule.select(due, 10001), None);
    let next_budget = event + INTERVAL;
    for index in 0..3 {
        assert_eq!(schedule.select(next_budget, 10001), Some((index, true)));
    }
    assert_eq!(schedule.select(next_budget, 10002), None);
    assert_eq!(schedule.due, vec![due + INTERVAL; 3]);
}

#[test]
fn wake_during_collection_survives_consumption_and_throttling() {
    let now = Instant::now();
    let mut schedule = Schedule::new(1, now);
    assert_eq!(schedule.select(now, 0), Some((0, false)));
    schedule.due[0] = now + INTERVAL;
    assert_eq!(schedule.select(now, 1), Some((0, true)));
    assert!(!schedule.pending[0]);
    // Generation 2 arrives during the selected native collection.
    assert_eq!(schedule.select(now, 2), None);
    assert!(schedule.pending[0]);
    assert_eq!(schedule.next(), Some(now + INTERVAL));
    // Make the periodic deadline later to isolate eventual event-only followup.
    schedule.due[0] = now + INTERVAL * 2;
    assert_eq!(schedule.select(now + INTERVAL, 2), Some((0, true)));
    assert_eq!(schedule.select(now + INTERVAL, 2), None);
}

#[test]
fn actual_scheduler_preserves_wake_inside_injected_collection() {
    let (tx, rx) = mpsc::channel();
    let wake = Arc::new(MetadataWake::default());
    let worker = wake.clone();
    let during_work = wake.clone();
    let mut calls = 0;
    let runner = Runner {
        wake,
        thread: Some(std::thread::spawn(move || {
            run_scheduler(&worker, 1, move |index| {
                calls += 1;
                if calls == 1 {
                    during_work.wake();
                }
                tx.send(index).is_ok()
            });
        })),
    };
    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), 0);
    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), 0);
    assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());
    drop(runner);
}

#[test]
fn weak_registry_rejects_collision_and_stale_cleanup_cannot_remove_replacement() {
    let events = events::Events::default();
    let first = Arc::new(MetadataWake::default());
    let second = Arc::new(MetadataWake::default());
    assert!(events.register_metadata(&first));
    assert!(!events.register_metadata(&second));
    assert_eq!(Arc::strong_count(&first), 1);
    events.unregister_metadata(&second);
    events.wake_metadata();
    assert_eq!(first.state.lock().unwrap().generation, 1);
    events.unregister_metadata(&first);
    assert!(events.register_metadata(&second));
    events.unregister_metadata(&first);
    events.wake_metadata();
    assert_eq!(second.state.lock().unwrap().generation, 1);
    let weak = Arc::downgrade(&second);
    drop(second);
    assert!(weak.upgrade().is_none());
    events.wake_metadata();
    assert!(events.register_metadata(&first));
}

#[test]
fn service_lifecycle_collision_sequential_registration_and_no_observer_cycle() {
    let (observer, manual) = tests::fixture();
    let service = MetadataService::start(&config(), &observer).unwrap();
    assert!(MetadataService::start(&config(), &observer).is_err());
    // Dropping an unregistered manual service must not unregister the live one.
    drop(manual);
    assert!(MetadataService::start(&config(), &observer).is_err());
    drop(service);
    let service = MetadataService::start(&config(), &observer).unwrap();
    let weak = Arc::downgrade(&observer);
    drop(service);
    assert_eq!(Arc::strong_count(&observer), 1);
    drop(observer);
    assert!(weak.upgrade().is_none());
}

#[test]
fn cache_reads_and_internal_trigger_never_do_native_io_or_add_unknown_logins() {
    let (observer, service) = tests::fixture(); // Transport panics on any native call.
    assert!(observer.events.register_metadata(&service.stop));
    for _ in 0..100 {
        observer.events.wake_metadata();
        for login in ["alice", "unknown"] {
            assert_eq!(service.read(login, true)["state"], "unavailable");
        }
    }
    assert!(service.cache.lock().unwrap().is_empty());
    let mut empty = Schedule::new(0, Instant::now());
    assert_eq!(empty.select(Instant::now(), 100), None);
    assert_eq!(empty.next(), None);
}
