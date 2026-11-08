//! Foreground annotation publication only. No retries, refunds, or exit-time clears.
use serde_json::Value;

#[path = "group_publisher/engine.rs"]
mod engine;
#[path = "group_publisher/live.rs"]
mod live;
#[path = "group_publisher/model.rs"]
mod model;
#[path = "group_publisher/native.rs"]
mod native;

pub use model::Request;

/// Invalid caller operands are errors; operational failures are safe summary values.
pub fn publish(request: &Request) -> crate::Result<Value> {
    model::validate(request)?;
    let clock = crate::observation_time::SystemClock;
    match native::Session::open(request) {
        Ok(session) => Ok(engine::run(request, &session, &clock, &session.interrupt)),
        Err(code) => Ok(model::Summary::new(request.live).finish(Some(&code), &[])),
    }
}

#[cfg(test)]
#[path = "group_publisher/tests.rs"]
mod tests;
