//! Incremental native Coven: explicit readers and guarded task, baseline and
//! evidence operations. No automatic runtime execution or Python fallback.
pub mod actions;
pub mod agent_discovery;
pub mod agent_discovery_watch;
pub mod agent_management;
pub mod agent_management_presentation;
mod agent_processes;
pub mod archive;
mod canonical;
pub mod checkpoint_decisions;
pub mod checkpoint_guidance;
pub mod checkpoint_workspace;
pub mod checkpoints;
pub mod claude_hooks;
pub mod codex_hooks;
pub mod collection;
pub mod console;
pub mod copilot_hooks;
pub mod djinn_live_observation;
pub mod djinn_observation;
pub mod error;
pub mod execution;
mod extension_assets;
pub mod group_commands;
pub mod group_live;
pub mod group_presenter;
pub mod group_publisher;
pub mod groups;
pub mod handoff;
#[cfg(unix)]
mod hook_files;
pub mod init;
pub mod integration_route;
pub mod integration_runtime;
#[cfg(unix)]
pub mod integration_setup;
#[cfg(not(unix))]
#[path = "integration_setup/unsupported.rs"]
pub mod integration_setup;
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
pub mod opencode_extension;
mod output;
pub mod pi_extension;
mod platform;
#[cfg(unix)]
mod private_access;
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
