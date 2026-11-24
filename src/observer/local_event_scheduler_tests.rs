use super::*;
use crate::observation_socket::{Endpoint as WireEndpoint, Failure, WireResult};
use crate::observer::{Cache, config::EndpointConfig, source::Request};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

fn endpoint() -> Endpoint {
    Endpoint {
        config: EndpointConfig {
            id: "local".into(),
            label: "Local".into(),
            socket: "/unused/scripted.sock".into(),
            ssh: None,
        },
        cache: Mutex::new(Cache::default()),
        busy: AtomicBool::new(false),
        refresh_due: AtomicBool::new(true),
    }
}

#[test]
fn first_dirty_deadline_is_not_postponed_and_overlap_is_periodic() {
    let start = Instant::now();
    let mut s = Schedule::new(start);
    s.completed(start, RefreshPriority::Periodic, &Ok(()));
    s.hint(start);
    for millis in 1..250 {
        s.hint(start + Duration::from_millis(millis));
    }
    assert_eq!(s.event_due(), Some(start + COALESCE));
    assert!(s.priority(start + COALESCE) == Some(RefreshPriority::Event));
    assert!(s.priority(start + INTERVAL) == Some(RefreshPriority::Periodic));
}

#[test]
fn storm_has_one_extra_per_interval_and_completion_moves_periodic() {
    let start = Instant::now();
    let mut s = Schedule::new(start);
    s.completed(start, RefreshPriority::Periodic, &Ok(()));
    s.hint(start);
    let event = start + COALESCE;
    s.pending.take();
    s.completed(event, RefreshPriority::Event, &Ok(()));
    s.hint(event);
    assert_eq!(s.admission, event + COALESCE);
    assert_eq!(s.event_due(), Some(event + INTERVAL));
    assert_eq!(s.periodic, event + INTERVAL);
    assert!(s.priority(start + INTERVAL).is_none());
    assert!(s.priority(event + INTERVAL) == Some(RefreshPriority::Periodic));
    s.completed(event + INTERVAL, RefreshPriority::Periodic, &Ok(()));
    assert!(s.priority(event + INTERVAL).is_none());
}

#[test]
fn failure_barrier_cannot_be_bypassed_by_hints() {
    let mut now = Instant::now();
    let mut s = Schedule::new(now);
    for delay in [6, 12, 24, 24] {
        s.completed(
            now,
            RefreshPriority::Periodic,
            &Err(crate::Error("timeout")),
        );
        s.hint(now);
        assert_eq!(s.due(), now + Duration::from_secs(delay));
        assert!(s.priority(now + Duration::from_millis(250)).is_none());
        now += Duration::from_secs(delay);
        assert!(s.priority(now) == Some(RefreshPriority::Periodic));
    }
    s.completed(now, RefreshPriority::Periodic, &Ok(()));
    assert_eq!(s.failures, 0);
    assert_eq!(s.periodic, now + INTERVAL);
}

#[test]
fn reconnect_backoff_requires_a_stable_stream_not_an_ack() {
    let mut now = Instant::now();
    let mut r = Reconnect {
        due: now,
        failures: 0,
        opened: None,
    };
    for delay in [1, 2, 4, 8, 16, 30, 30] {
        r.opened = Some(now);
        r.failed(now);
        assert_eq!(r.due, now + Duration::from_secs(delay));
        now = r.due;
    }
    r.opened = Some(now);
    now += INTERVAL;
    r.failed(now);
    assert_eq!(r.due, now + Duration::from_secs(1));
}

#[test]
fn clearing_one_generation_keeps_the_next_and_busy_restores_the_first() {
    let now = Instant::now();
    let mut s = Schedule::new(now);
    s.completed(now, RefreshPriority::Periodic, &Ok(()));
    s.hint(now);
    let acquired = s.pending.take();
    assert_eq!(acquired, Some(now + COALESCE));
    // Represents a buffered hint consumed immediately after acquisition.
    s.hint(now + COALESCE);
    s.completed(now + COALESCE, RefreshPriority::Event, &Ok(()));
    assert_eq!(s.pending, Some(now + COALESCE * 2));
    let pending = s.pending.take();
    s.pending = pending;
    let retry = now + INTERVAL;
    s.completed(
        retry,
        RefreshPriority::Event,
        &Err(crate::Error("endpoint_busy")),
    );
    assert_eq!(s.pending, pending);
    assert_eq!(s.barrier, retry + BUSY_RETRY);
    assert_eq!(s.failures, 0);
}

struct InventoryTransport {
    calls: AtomicUsize,
    times: Mutex<Vec<Instant>>,
    stop_after: usize,
    shutdown: Arc<(Mutex<bool>, Condvar)>,
}
impl Transport for InventoryTransport {
    fn call(&self, _: &EndpointConfig, request: &Request, _: Instant) -> crate::Result<Value> {
        assert!(
            matches!(request, Request::Inventory {}),
            "scheduler must never request output"
        );
        self.times.lock().unwrap().push(Instant::now());
        let count = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if count >= self.stop_after {
            *self.shutdown.0.lock().unwrap() = true;
            self.shutdown.1.notify_all();
        }
        Ok(
            json!({"generation":"a".repeat(64),"agents":[],"discovery_status":"observed","unclassified_panes":count}),
        )
    }
}

struct ScriptedSource<'a> {
    transport: &'a InventoryTransport,
    opens: AtomicUsize,
    drops: Arc<AtomicUsize>,
    error: Option<&'static str>,
    storm: bool,
}
struct ScriptedStream {
    endpoint: WireEndpoint,
    drops: Arc<AtomicUsize>,
    error: Option<&'static str>,
    storm: bool,
}
impl Drop for ScriptedStream {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl LocalEventSource for ScriptedSource<'_> {
    fn open(
        &self,
        path: &str,
        _: Instant,
        cancel: &dyn Cancel,
    ) -> WireResult<Box<dyn LocalEventStream>> {
        assert!(!cancel.interrupted());
        assert!(
            self.transport.calls.load(Ordering::SeqCst) > 0,
            "inventory must qualify before subscribe"
        );
        self.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(ScriptedStream {
            endpoint: WireEndpoint {
                selected: path.into(),
                resolved: path.into(),
                device: 0,
                inode: 0,
            },
            drops: self.drops.clone(),
            error: self.error,
            storm: self.storm,
        }))
    }
}
impl LocalEventStream for ScriptedStream {
    fn endpoint(&self) -> &WireEndpoint {
        &self.endpoint
    }
    fn next(&mut self, until: Instant, cancel: &dyn Cancel) -> WireResult<Option<Hint>> {
        if let Some(error) = self.error.take() {
            return Err(Failure::new(error));
        }
        while Instant::now() < until && !cancel.interrupted() {
            std::thread::sleep(Duration::from_millis(2));
            if self.storm {
                return Ok(Some(Hint::Inventory));
            }
        }
        Ok(None)
    }
}

// The watchdog is joined even on panic; no test-owned workers outlive a test.
fn run_bounded(
    endpoint: &Endpoint,
    transport: &InventoryTransport,
    source: Option<&dyn LocalEventSource>,
) {
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let guard = transport.shutdown.0.lock().unwrap();
            let (mut stop, _) = transport
                .shutdown
                .1
                .wait_timeout_while(guard, Duration::from_secs(9), |stop| !*stop)
                .unwrap();
            *stop = true;
            transport.shutdown.1.notify_all();
        });
        run_poller(endpoint, "test", transport, &transport.shutdown, source);
        *transport.shutdown.0.lock().unwrap() = true;
        transport.shutdown.1.notify_all();
    });
}

fn transport(stop_after: usize) -> InventoryTransport {
    InventoryTransport {
        calls: AtomicUsize::new(0),
        times: Mutex::new(Vec::new()),
        stop_after,
        shutdown: Arc::new((Mutex::new(false), Condvar::new())),
    }
}

#[test]
fn scripted_storm_is_bounded_and_drops_stream_on_shutdown() {
    let endpoint = endpoint();
    let transport = transport(4);
    let drops = Arc::new(AtomicUsize::new(0));
    let source = ScriptedSource {
        transport: &transport,
        opens: AtomicUsize::new(0),
        drops: drops.clone(),
        error: None,
        storm: true,
    };
    run_bounded(&endpoint, &transport, Some(&source));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 4);
    let times = transport.times.lock().unwrap();
    assert!(times[1].duration_since(times[0]) >= COALESCE);
    assert!(times[2].duration_since(times[1]) >= INTERVAL);
    assert!(times[3].duration_since(times[2]) >= COALESCE);
    assert_eq!(source.opens.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(!endpoint.refresh_due.load(Ordering::Acquire));
}

#[test]
fn periodic_reconciliation_detects_changes_without_any_events() {
    let endpoint = endpoint();
    let transport = transport(2);
    run_bounded(&endpoint, &transport, None);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        endpoint
            .cache
            .lock()
            .unwrap()
            .inventory
            .as_ref()
            .unwrap()
            .unclassified_panes,
        2
    );
    let times = transport.times.lock().unwrap();
    assert!(times[1].duration_since(times[0]) >= INTERVAL);
}

#[test]
fn idle_stream_does_not_replace_periodic_reconciliation() {
    let endpoint = endpoint();
    let transport = transport(3);
    let source = ScriptedSource {
        transport: &transport,
        opens: AtomicUsize::new(0),
        drops: Arc::new(AtomicUsize::new(0)),
        error: None,
        storm: false,
    };
    run_bounded(&endpoint, &transport, Some(&source));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 3);
    let times = transport.times.lock().unwrap();
    assert!(times[2].duration_since(times[1]) >= INTERVAL);
    assert_eq!(source.opens.load(Ordering::SeqCst), 1);
    assert_eq!(source.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn identity_replacement_is_followed_by_ordinary_inventory() {
    let endpoint = endpoint();
    let transport = transport(2);
    let source = ScriptedSource {
        transport: &transport,
        opens: AtomicUsize::new(0),
        drops: Arc::new(AtomicUsize::new(0)),
        error: Some("endpoint_replaced"),
        storm: false,
    };
    run_bounded(&endpoint, &transport, Some(&source));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 2);
    assert_eq!(source.drops.load(Ordering::SeqCst), 1);
    let cache = endpoint.cache.lock().unwrap();
    assert!(cache.fresh.is_some());
    assert_eq!(cache.error, None);
    assert_eq!(cache.inventory.as_ref().unwrap().unclassified_panes, 2);
}

#[test]
fn ssh_never_opens_local_events() {
    let mut endpoint = endpoint();
    endpoint.config.ssh = Some(crate::observer::config::SshConfig {
        target: "host".into(),
        helper: "/bin/coven".into(),
    });
    let transport = transport(2);
    let source = ScriptedSource {
        transport: &transport,
        opens: AtomicUsize::new(0),
        drops: Arc::new(AtomicUsize::new(0)),
        error: None,
        storm: true,
    };
    run_bounded(&endpoint, &transport, Some(&source));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 2);
    assert_eq!(source.opens.load(Ordering::SeqCst), 0);
}

#[test]
fn event_busy_does_not_claim_periodic_output_priority() {
    let endpoint = endpoint();
    let transport = transport(usize::MAX);
    endpoint.refresh_due.store(false, Ordering::Release);
    let busy = endpoint.acquire().unwrap();
    assert_eq!(
        endpoint
            .refresh_with_priority("test", &transport, RefreshPriority::Event)
            .unwrap_err()
            .code(),
        "endpoint_busy"
    );
    assert!(!endpoint.refresh_due.load(Ordering::Acquire));
    assert_eq!(
        endpoint.refresh("test", &transport).unwrap_err().code(),
        "endpoint_busy"
    );
    assert!(endpoint.refresh_due.load(Ordering::Acquire));
    drop(busy);
    endpoint.refresh("test", &transport).unwrap();
    assert!(!endpoint.refresh_due.load(Ordering::Acquire));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn identity_errors_are_distinct_from_hint_channel_errors() {
    assert!(identity_failure("endpoint_replaced", "/unused", None));
    assert!(identity_failure("unsafe_endpoint", "/unused", None));
    assert!(identity_failure("io_error", "/unused/nonexistent", None));
    for code in [
        "schema_error",
        "invalid_json",
        "upstream_rejected",
        "eof",
        "timeout",
    ] {
        assert!(!identity_failure(code, "/unused/nonexistent", None));
    }
}

#[test]
fn rejected_subscriptions_back_off_without_invalidating_inventory() {
    struct Reject<'a> {
        endpoint: &'a Endpoint,
        opens: Mutex<Vec<Instant>>,
    }
    impl LocalEventSource for Reject<'_> {
        fn open(
            &self,
            _: &str,
            _: Instant,
            _: &dyn Cancel,
        ) -> WireResult<Box<dyn LocalEventStream>> {
            let cache = self.endpoint.cache.lock().unwrap();
            assert!(cache.fresh.is_some());
            assert_eq!(cache.error, None);
            self.opens.lock().unwrap().push(Instant::now());
            Err(Failure::new("subscription_rejected"))
        }
    }
    let endpoint = endpoint();
    let transport = transport(2);
    let source = Reject {
        endpoint: &endpoint,
        opens: Mutex::new(Vec::new()),
    };
    run_bounded(&endpoint, &transport, Some(&source));
    let opens = source.opens.lock().unwrap();
    assert_eq!(opens.len(), 2);
    assert!(opens[1].duration_since(opens[0]) >= Duration::from_secs(1));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 2);
    assert_eq!(endpoint.cache.lock().unwrap().error, None);
}

#[test]
fn silent_ack_setup_yields_to_each_periodic_inventory_deadline() {
    struct SilentAck {
        opens: Mutex<Vec<(Instant, Instant)>>,
    }
    impl LocalEventSource for SilentAck {
        fn open(
            &self,
            _: &str,
            deadline: Instant,
            cancel: &dyn Cancel,
        ) -> WireResult<Box<dyn LocalEventStream>> {
            let mut opens = self.opens.lock().unwrap();
            opens.push((Instant::now(), deadline));
            if opens.len() == 1 {
                // Force the silent attempt to begin at a reconnect, not just after inventory.
                return Err(Failure::new("subscription_rejected"));
            }
            drop(opens);
            while Instant::now() < deadline && !cancel.interrupted() {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(Failure::new("timeout"))
        }
    }
    let endpoint = endpoint();
    let transport = transport(3);
    let source = SilentAck {
        opens: Mutex::new(Vec::new()),
    };
    run_bounded(&endpoint, &transport, Some(&source));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 3);
    let times = transport.times.lock().unwrap();
    let margin = Duration::from_millis(500);
    for pair in times.windows(2) {
        assert!(pair[1].duration_since(pair[0]) >= INTERVAL);
        assert!(pair[1].duration_since(pair[0]) < INTERVAL + margin);
    }
    let opens = source.opens.lock().unwrap();
    assert_eq!(opens.len(), 3);
    for (index, (opened, deadline)) in opens.iter().enumerate().skip(1) {
        assert!(*opened >= times[index - 1] + Duration::from_secs(1));
        assert!(*deadline <= times[index - 1] + INTERVAL + margin);
        assert!(*deadline < *opened + OPERATION);
    }
    let cache = endpoint.cache.lock().unwrap();
    assert!(cache.fresh.is_some());
    assert_eq!(cache.error, None);
    assert_eq!(
        cache.generation, 0,
        "setup timeouts must not invalidate inventory"
    );
}

#[cfg(unix)]
#[test]
fn eof_reselects_identity_before_drop_and_preserves_unchanged_inventory() {
    use std::{
        os::unix::{fs::PermissionsExt, net::UnixListener},
        path::PathBuf,
    };

    struct EofSource {
        fleet: Arc<Endpoint>,
        replacement: Mutex<Option<PathBuf>>,
        dropped_state: Arc<AtomicUsize>,
        code: &'static str,
    }
    struct EofStream {
        endpoint: WireEndpoint,
        fleet: Arc<Endpoint>,
        replacement: Option<PathBuf>,
        dropped_state: Arc<AtomicUsize>,
        code: &'static str,
    }
    impl LocalEventSource for EofSource {
        fn open(
            &self,
            path: &str,
            _: Instant,
            _: &dyn Cancel,
        ) -> WireResult<Box<dyn LocalEventStream>> {
            Ok(Box::new(EofStream {
                endpoint: observation_socket::select(path)?,
                fleet: self.fleet.clone(),
                replacement: self.replacement.lock().unwrap().take(),
                dropped_state: self.dropped_state.clone(),
                code: self.code,
            }))
        }
    }
    impl LocalEventStream for EofStream {
        fn endpoint(&self) -> &WireEndpoint {
            &self.endpoint
        }
        fn next(&mut self, _: Instant, _: &dyn Cancel) -> WireResult<Option<Hint>> {
            if let Some(replacement) = self.replacement.take() {
                std::fs::rename(replacement, &self.endpoint.selected).unwrap();
                assert_ne!(
                    observation_socket::select(&self.endpoint.selected).unwrap(),
                    self.endpoint
                );
            }
            Err(Failure::new(self.code))
        }
    }
    impl Drop for EofStream {
        fn drop(&mut self) {
            let cache = self.fleet.cache.lock().unwrap();
            let state = if cache.fresh.is_some() && cache.error.is_none() {
                1
            } else if cache.fresh.is_none() && cache.error == Some("terminal_replaced") {
                2
            } else {
                3
            };
            self.dropped_state.store(state, Ordering::SeqCst);
        }
    }

    for code in ["stream_eof", "incomplete_response"] {
        for replaced in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("s");
            let next = dir.path().join("n");
            let _old = UnixListener::bind(&path).unwrap();
            let _new = UnixListener::bind(&next).unwrap();
            for path in [&path, &next] {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            let mut endpoint = endpoint();
            endpoint.config.socket = path.to_str().unwrap().into();
            let endpoint = Arc::new(endpoint);
            let transport = transport(2);
            let dropped_state = Arc::new(AtomicUsize::new(0));
            let source = EofSource {
                fleet: endpoint.clone(),
                replacement: Mutex::new(replaced.then_some(next)),
                dropped_state: dropped_state.clone(),
                code,
            };
            run_bounded(&endpoint, &transport, Some(&source));
            assert_eq!(
                dropped_state.load(Ordering::SeqCst),
                if replaced { 2 } else { 1 }
            );
            assert_eq!(transport.calls.load(Ordering::SeqCst), 2);
            let cache = endpoint.cache.lock().unwrap();
            assert!(cache.fresh.is_some());
            assert_eq!(cache.error, None);
            assert_eq!(cache.inventory.as_ref().unwrap().unclassified_panes, 2);
        }
    }
}

#[test]
fn shutdown_cancels_an_open_without_an_extra_inventory_or_reader_thread() {
    struct Opening(Mutex<bool>, Condvar);
    impl LocalEventSource for Opening {
        fn open(
            &self,
            _: &str,
            deadline: Instant,
            cancel: &dyn Cancel,
        ) -> WireResult<Box<dyn LocalEventStream>> {
            *self.0.lock().unwrap() = true;
            self.1.notify_all();
            while !cancel.interrupted() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(cancel.interrupted());
            Err(Failure::new("interrupted"))
        }
    }
    let endpoint = endpoint();
    let transport = transport(usize::MAX);
    let source = Opening(Mutex::new(false), Condvar::new());
    let start = Instant::now();
    std::thread::scope(|scope| {
        let poller = scope.spawn(|| {
            run_poller(
                &endpoint,
                "test",
                &transport,
                &transport.shutdown,
                Some(&source),
            )
        });
        let opened = source.0.lock().unwrap();
        let (opened, _) = source
            .1
            .wait_timeout_while(opened, Duration::from_secs(2), |opened| !*opened)
            .unwrap();
        let did_open = *opened;
        drop(opened);
        *transport.shutdown.0.lock().unwrap() = true;
        transport.shutdown.1.notify_all();
        poller.join().unwrap();
        assert!(did_open);
    });
    assert!(start.elapsed() < Duration::from_secs(3));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}
