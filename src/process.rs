//! Bounded private transport for trusted, closed-argv observation adapters.
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) mod identity;

pub(crate) struct ProbeRequest {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub cwd: PathBuf,
    pub environment: BTreeMap<OsString, OsString>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProbeStatus {
    Ok,
    Nonzero,
    Timeout,
    OutputLimit,
    SpawnError,
}

impl ProbeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Nonzero => "nonzero",
            Self::Timeout => "timeout",
            Self::OutputLimit => "output_limit",
            Self::SpawnError => "spawn_error",
        }
    }
}

#[derive(Debug)]
pub(crate) struct ProbeResult {
    pub status: ProbeStatus,
    pub returncode: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub duration_ms: f64,
}

pub(crate) trait ProbeRunner {
    fn run(&self, request: &ProbeRequest) -> ProbeResult;
}

#[derive(Default)]
pub(crate) struct NativeRunner;

impl ProbeRunner for NativeRunner {
    fn run(&self, request: &ProbeRequest) -> ProbeResult {
        match std::env::current_exe() {
            Ok(path) => run_with_supervisor(request, &path),
            Err(_) => failed_result(),
        }
    }
}

pub(crate) fn failed_result() -> ProbeResult {
    ProbeResult {
        status: ProbeStatus::SpawnError,
        returncode: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
        duration_ms: 0.0,
    }
}

pub(crate) fn run_with_supervisor(request: &ProbeRequest, supervisor: &Path) -> ProbeResult {
    crate::platform::process_run(request, supervisor)
}

pub(crate) fn supervisor_entry() -> Option<u8> {
    crate::platform::supervisor_entry()
}
