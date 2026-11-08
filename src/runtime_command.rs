//! Direct runtime CLI execution, deliberately NOT the probe process supervisor.
//! Timeout/output failure says nothing about effects already performed by a server.
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

pub(crate) struct Request {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    /// Launch callers should select the native profile: 60 seconds, not probe's 30.
    pub timeout: Duration,
    /// Combined stdout/stderr budget; proposed launch profile is 1_048_576 bytes.
    pub max_output_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    /// Ordinary exit code, or negative signal number on Unix.
    Exited(i32),
    Timeout,
    OutputLimit,
    /// Command::spawn failed: the chosen target image was never executed.
    /// Only this status permits a caller's explicit executable fallback policy.
    ExecFailed,
    /// Invalid request, setup, or capture failure. Not fallback authorization.
    SpawnError,
}

#[derive(Debug)]
pub(crate) struct Output {
    pub status: Status,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub(crate) trait Runner {
    fn run(&self, request: &Request) -> Output;
}

pub(crate) struct NativeRunner;

impl Runner for NativeRunner {
    fn run(&self, request: &Request) -> Output {
        crate::platform::runtime_command_run(request)
    }
}
