//! Local event hints accelerate, but never replace, fenced inventory acquisition.
use super::{
    BUSY_RETRY, Endpoint, INTERVAL, OPERATION, RefreshPriority,
    local_events::{Hint, LocalEventSource, LocalEventStream},
    transport::Transport,
};
use crate::observation_socket::{self, Cancel};
use std::{
    sync::{Condvar, Mutex},
    time::{Duration, Instant},
};

const COALESCE: Duration = Duration::from_millis(250);
const IDENTITY_CHECK: Duration = Duration::from_secs(1);

struct Shutdown<'a>(&'a (Mutex<bool>, Condvar));
impl Cancel for Shutdown<'_> {
    fn interrupted(&self) -> bool {
        *self.0.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

struct Schedule {
    periodic: Instant,
    pending: Option<Instant>,
    admission: Instant,
    extra: Instant,
    barrier: Instant,
    failures: u32,
}
impl Schedule {
    fn new(now: Instant) -> Self {
        Self {
            periodic: now,
            pending: None,
            admission: now,
            extra: now,
            barrier: now,
            failures: 0,
        }
    }
    fn hint(&mut self, now: Instant) {
        self.pending.get_or_insert(now + COALESCE);
    }
    fn event_due(&self) -> Option<Instant> {
        self.pending
            .map(|at| at.max(self.admission).max(self.extra).max(self.barrier))
    }
    fn due(&self) -> Instant {
        self.event_due()
            .map_or(self.periodic, |at| at.min(self.periodic))
            .max(self.barrier)
    }
    fn priority(&self, now: Instant) -> Option<RefreshPriority> {
        if now < self.barrier {
            None
        } else if now >= self.periodic {
            Some(RefreshPriority::Periodic)
        } else if self.event_due().is_some_and(|at| now >= at) {
            Some(RefreshPriority::Event)
        } else {
            None
        }
    }
    fn completed(&mut self, now: Instant, priority: RefreshPriority, result: &crate::Result<()>) {
        if result.as_ref().is_err_and(|e| e.code() == "endpoint_busy") {
            self.barrier = now + BUSY_RETRY;
            return;
        }
        if result.is_ok() {
            self.failures = 0;
        } else {
            self.failures = (self.failures + 1).min(3);
        }
        self.periodic = now + INTERVAL * (1 << self.failures);
        self.barrier = if result.is_err() { self.periodic } else { now };
        self.admission = now + COALESCE;
        if priority == RefreshPriority::Event {
            self.extra = now + INTERVAL;
        }
    }
}

struct Reconnect {
    due: Instant,
    failures: u32,
    opened: Option<Instant>,
}
impl Reconnect {
    fn failed(&mut self, now: Instant) {
        // An ACK followed by EOF is not a healthy connection.
        if self
            .opened
            .take()
            .is_some_and(|at| now.duration_since(at) >= INTERVAL)
        {
            self.failures = 0;
        }
        self.due = now + Duration::from_secs((1u64 << self.failures).min(30));
        self.failures = (self.failures + 1).min(5);
    }
}

fn identity_failure(
    code: &str,
    path: &str,
    previous: Option<&observation_socket::Endpoint>,
) -> bool {
    match code {
        "unsafe_endpoint" | "endpoint_replaced" => true,
        // A closed stream is not an identity failure unless its selected path changed.
        "io_error" | "stream_eof" | "incomplete_response" => match observation_socket::select(path)
        {
            Ok(current) => previous.is_some_and(|old| *old != current),
            Err(_) => true,
        },
        _ => false,
    }
}

pub(super) fn run_poller(
    endpoint: &Endpoint,
    epoch: &str,
    transport: &dyn Transport,
    shutdown: &(Mutex<bool>, Condvar),
    source: Option<&dyn LocalEventSource>,
) {
    let cancel = Shutdown(shutdown);
    let source = source.filter(|_| endpoint.config.ssh.is_none());
    let mut schedule = Schedule::new(Instant::now());
    let mut reconnect = Reconnect {
        due: Instant::now(),
        failures: 0,
        opened: None,
    };
    let mut stream: Option<Box<dyn LocalEventStream>> = None;
    let mut qualified = false;
    while !cancel.interrupted() {
        let now = Instant::now();
        if let Some(priority) = schedule.priority(now) {
            // Consume exactly the dirty generation being acquired. Hints arriving
            // during acquisition remain in the stream, not an unbounded queue.
            let pending = schedule.pending.take();
            let result = match priority {
                RefreshPriority::Periodic => endpoint.refresh(epoch, transport),
                RefreshPriority::Event => {
                    endpoint.refresh_with_priority(epoch, transport, priority)
                }
            };
            let completed = Instant::now();
            if result.as_ref().is_err_and(|e| e.code() == "endpoint_busy") {
                schedule.pending = pending;
            } else {
                qualified = result.is_ok();
            }
            schedule.completed(completed, priority, &result);
            continue;
        }
        if cancel.interrupted() {
            break;
        }
        let now = Instant::now();
        if stream.is_none() && qualified && now >= reconnect.due {
            if let Some(source) = source {
                // Subscription setup must yield to inventory, including pending event work.
                let deadline = (now + OPERATION).min(schedule.due());
                match source.open(&endpoint.config.socket, deadline, &cancel) {
                    Ok(opened) => {
                        stream = Some(opened);
                        reconnect.opened = Some(Instant::now());
                        // Reconnect history is reconciled only by a normal full inventory.
                        schedule.hint(Instant::now());
                    }
                    Err(error) => {
                        if !cancel.interrupted()
                            && identity_failure(&error.code, &endpoint.config.socket, None)
                        {
                            endpoint.fail("endpoint_replaced");
                            qualified = false;
                            schedule.hint(Instant::now());
                        }
                        reconnect.failed(Instant::now());
                    }
                }
                continue;
            }
        }
        let now = Instant::now();
        let mut until = schedule.due().min(now + IDENTITY_CHECK);
        if stream.is_none() && source.is_some() && qualified {
            until = until.min(reconnect.due);
        }
        if let Some(active) = stream.as_mut() {
            match active.next(until, &cancel) {
                Ok(Some(Hint::Inventory)) => schedule.hint(Instant::now()),
                Ok(None) => {}
                Err(error) => {
                    if !cancel.interrupted()
                        && identity_failure(
                            &error.code,
                            &endpoint.config.socket,
                            Some(active.endpoint()),
                        )
                    {
                        endpoint.fail("endpoint_replaced");
                        qualified = false;
                        schedule.hint(Instant::now());
                    }
                    stream = None;
                    reconnect.failed(Instant::now());
                }
            }
        } else {
            let guard = shutdown.0.lock().unwrap_or_else(|p| p.into_inner());
            let _ = shutdown.1.wait_timeout_while(
                guard,
                until.saturating_duration_since(Instant::now()),
                |stop| !*stop,
            );
        }
    }
    // The endpoint worker owns the only stream; it is dropped before join returns.
    drop(stream);
}

#[cfg(test)]
#[path = "local_event_scheduler_tests.rs"]
mod tests;
