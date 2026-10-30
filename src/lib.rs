//! Native migration core. Only explicitly supported read-only workflows are live.
pub mod archive;
mod canonical;
pub mod error;
pub mod handoff;
mod platform;
mod read_fs;
mod strict_json;

pub use error::{Error, Result};
