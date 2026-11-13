//! Bounded, test-child-only diagnostics. Never records payloads or identities.
use std::{
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

pub(crate) fn stage(code: &'static str) {
    if std::env::var_os("COVEN_RECEIPT_TEST_TRACE").is_none() {
        return;
    }
    static START: OnceLock<Instant> = OnceLock::new();
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    if COUNT.fetch_add(1, Ordering::Relaxed) < 192 {
        eprintln!(
            "receipt_trace {} {code}",
            START.get_or_init(Instant::now).elapsed().as_micros()
        );
    }
}

pub(crate) fn slow_commit() {
    pause("COVEN_RECEIPT_TEST_COMMIT_MS");
}

pub(crate) fn slow_admission() {
    pause("COVEN_RECEIPT_TEST_ADMISSION_MS");
}

fn pause(variable: &str) {
    if let Some(ms) = std::env::var(variable)
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|ms| *ms <= 1500)
    {
        std::thread::sleep(Duration::from_millis(ms));
    }
}
