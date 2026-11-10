//! Incremental native Coven: explicit readers and guarded task, baseline and
//! evidence operations. No automatic runtime execution or Python fallback.
pub mod actions;
pub mod archive;
mod canonical;
pub mod checkpoint_decisions;
pub mod checkpoint_guidance;
pub mod checkpoint_workspace;
pub mod checkpoints;
pub mod collection;
pub mod console;
pub mod djinn_live_observation;
pub mod djinn_observation;
pub mod error;
pub mod execution;
pub mod group_commands;
pub mod group_live;
pub mod group_presenter;
pub mod group_publisher;
pub mod groups;
pub mod handoff;
pub mod init;
pub mod interactive_sessions;
pub mod kitsune_comparison;
pub mod kitsune_observation;
mod launch;
pub mod lead;
pub mod ledger;
mod logs;
mod observation_socket;
mod observation_time;
mod observation_wire;
mod output;
mod platform;
mod process;
mod read_fs;
pub mod records;
pub mod refresh;
pub mod repositories;
pub mod runtime;
mod runtime_command;
mod runtime_foreground;
pub mod session_inspection;
pub mod session_prompts;
pub mod session_resume_plans;
mod session_support;
pub mod session_views;
pub mod session_watch;
pub mod state_commands;
pub mod storage;
mod strict_json;
mod task_admission;
mod task_delivery;
pub mod task_mutation;
mod task_time;
pub mod tasks;
mod transport_gate;
mod unicode;
pub mod values;
pub mod workers;
mod workspace;
mod workspace_assets;
pub mod workspace_commands;

pub use error::{Error, Result};

/// Private same-binary process supervisor bootstrap; not an application command.
#[doc(hidden)]
pub fn process_supervisor_entry() -> Option<u8> {
    process::supervisor_entry()
}
