use crate::observation_socket::{Cancel, Endpoint, Failure, ReadBudget, WireResult};
use crate::{Error, Result};
use std::{path::Path, time::Instant};

pub(crate) fn select(_: &str) -> WireResult<Endpoint> {
    Err(Failure::new("unsupported_platform"))
}
pub(crate) fn fence(_: &Endpoint) -> WireResult<()> {
    Err(Failure::new("unsupported_platform"))
}
pub(crate) struct Socket;
impl Socket {
    pub(crate) fn connect(_: &Endpoint, _: Instant, _: &dyn Cancel) -> WireResult<Self> {
        Err(Failure::new("unsupported_platform"))
    }
    pub(crate) fn send(&self, _: &[u8], _: Instant, _: &dyn Cancel) -> WireResult<()> {
        Err(Failure::new("unsupported_platform"))
    }
    pub(crate) fn receive(
        &self,
        _: Instant,
        _: &mut ReadBudget,
        _: &dyn Cancel,
    ) -> WireResult<Vec<u8>> {
        Err(Failure::new("unsupported_platform"))
    }
}
pub(crate) struct PublisherLease;
impl PublisherLease {
    pub(crate) fn acquire(_: &Path) -> Result<Self> {
        Err(Error("unsupported_platform"))
    }
}
pub(crate) struct InterruptGuard;
impl InterruptGuard {
    pub(crate) fn install() -> Result<Self> {
        Err(Error("unsupported_platform"))
    }
    pub(crate) fn interrupted(&self) -> bool {
        false
    }
}
