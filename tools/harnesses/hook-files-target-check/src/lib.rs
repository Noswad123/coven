//! Development-only target check of the real hook-file Rust sources, without
//! Coven's unrelated native crypto build or the Apple-SDK C compilation step.
//! `cargo check` here proves no linking, SDK ABI, syscall or native behavior.
#![allow(dead_code)] // The check deliberately imports private, uncalled application code.

#[path = "../../../../src/error.rs"]
mod error;
pub use error::{Error, Result};

#[path = "../../../../src/private_access.rs"]
mod private_access;

#[path = "../../../../src/storage/delivery.rs"]
mod delivery;
mod storage {
    pub use crate::delivery::Delivery;
}

#[path = "../../../../src/agent_management/files.rs"]
mod agent_files;

#[path = "../../../../src/hook_files.rs"]
mod hook_files;
