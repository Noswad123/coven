//! Native migration core. Only explicitly supported read-only workflows are live.
pub mod archive;
mod canonical;
pub mod collection;
pub mod error;
pub mod handoff;
pub mod ledger;
mod output;
mod platform;
mod read_fs;
pub mod records;
pub mod storage;
mod strict_json;
pub mod tasks;
pub mod values;

pub use error::{Error, Result};
