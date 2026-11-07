//! Incremental native Coven: explicit readers and guarded task, baseline and
//! evidence operations. No automatic runtime execution or Python fallback.
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
pub mod refresh;
pub mod storage;
mod strict_json;
mod task_admission;
mod task_delivery;
pub mod task_mutation;
mod task_time;
pub mod tasks;
pub mod values;
mod workspace;

pub use error::{Error, Result};
