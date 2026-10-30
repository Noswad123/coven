#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{ensure_supported, open_readonly};

#[cfg(not(unix))]
mod unsupported;
#[cfg(not(unix))]
pub(crate) use unsupported::{ensure_supported, open_readonly};
