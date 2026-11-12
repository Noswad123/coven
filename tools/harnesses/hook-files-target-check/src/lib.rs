//! Development-only target check of the real hook-file Rust sources, without
//! Coven's unrelated native crypto build or the Apple-SDK C compilation step.
//! `cargo check` here proves no linking, SDK ABI, syscall or native behavior.
#![allow(dead_code)] // The check deliberately imports private, uncalled application code.

#[path = "../../../../src/error.rs"]
mod error;
pub use error::{Error, Result};

#[path = "../../../../src/hook_files.rs"]
mod hook_files;
