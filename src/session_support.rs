//! Shared Session admission and presentation, with no native transport effects.
use crate::{
    Error, Result, error::require, platform, runtime, task_mutation::TaskOutput,
    transport_gate::TransportGuard, workspace,
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    };
    Ok(std::path::absolute(path)?)
}

pub(crate) fn admit(workspace: &Path) -> Result<TransportGuard> {
    platform::ensure_supported()?;
    let root = absolute(workspace)?.canonicalize()?;
    require(root.is_dir(), "invalid_runtime_workspace")?;
    root.to_str().ok_or(Error("non_unicode_path_unsupported"))?;
    workspace::load(&root)?;
    let gate = TransportGuard::shared(&root, Duration::from_secs(5))?;
    runtime::legacy(gate.root(), &gate)?;
    Ok(gate)
}

pub(crate) fn validate_admission(workspace: &Path, gate: &TransportGuard) -> Result<()> {
    let root = absolute(workspace)?.canonicalize()?;
    runtime::legacy(&root, gate)
}

pub(crate) fn output(report: &serde_json::Value, pretty: bool) -> Result<TaskOutput> {
    struct Bounded(Vec<u8>);
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > crate::values::MAX_OUTPUT.saturating_sub(self.0.len()) {
                return Err(std::io::Error::other("unsupported_json_output_profile"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut bytes = Bounded(Vec::new());
    let encoded = if pretty {
        serde_json::to_writer_pretty(&mut bytes, report)
    } else {
        serde_json::to_writer(&mut bytes, report)
    };
    encoded.map_err(|_| Error("unsupported_json_output_profile"))?;
    Ok(TaskOutput {
        stdout: crate::output::ascii_json(std::str::from_utf8(&bytes.0)?)?,
        warnings: Vec::new(),
        exit_code: 0,
    })
}

#[cfg(test)]
#[path = "session_support/tests.rs"]
mod tests;
