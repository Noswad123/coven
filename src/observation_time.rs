//! Receipt/freshness clocks shared by concrete observation adapters. These clocks
//! are not authentication or universal bounds on synchronous filesystem calls.
use crate::Result;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub(crate) trait Clock {
    fn monotonic(&self) -> Instant;
    fn wall_ms(&self) -> f64;
    fn timestamp(&self) -> Result<String>;
}

pub(crate) struct SystemClock;
impl Clock for SystemClock {
    fn monotonic(&self) -> Instant {
        Instant::now()
    }
    fn wall_ms(&self) -> f64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs_f64() * 1000.0,
            Err(error) => -error.duration().as_secs_f64() * 1000.0,
        }
    }
    fn timestamp(&self) -> Result<String> {
        crate::task_time::now()
    }
}
