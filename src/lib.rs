//! Native migration core. Only explicit readers and local evidence capture are live.
pub mod actions;
pub mod archive;
mod canonical;
pub mod checkpoint_workspace;
pub mod checkpoints;
pub mod collection;
pub mod error;
pub mod execution;
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
