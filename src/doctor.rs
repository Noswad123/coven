//! Read-only native compatibility metadata, independent of workspaces and Kitsune.
//!
//! The v1 JSON object has exactly `contract`, `version`, `executable`,
//! `capabilities`, `defaults`, and `diagnostics`. Capability `versions` arrays
//! contain supported protocol versions, not application versions or live readiness.
//! Consumers must negotiate these arrays, never infer compatibility from `version`.
//! Linux and macOS support v1; other platforms advertise empty arrays because
//! the required private runtime admission is unsupported. An unavailable default
//! root is null with a fixed diagnostic code string, not a failed inspection.
use crate::{Error, Result, integration_runtime, task_mutation::TaskOutput};
use serde::Serialize;

/// Stable discriminator for the closed v1 output schema. Changes require a new contract.
pub const STATUS_CONTRACT: &str = "coven.doctor-status.v1";

/// Closed, typed v1 output; no provider, configuration, or live runtime observations.
#[derive(Debug, Serialize)]
pub struct Status {
    pub contract: &'static str,
    /// Human build version only; not a feature compatibility signal.
    pub version: &'static str,
    /// Canonical, absolute UTF-8 identity of the running native executable.
    pub executable: String,
    pub capabilities: Capabilities,
    pub defaults: Defaults,
    /// Fixed code strings, currently default-root policy failures only.
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Capabilities {
    pub integration_runtime: Capability,
    pub native_event_reporting: Capability,
}

#[derive(Debug, Serialize)]
pub struct Capability {
    pub versions: Vec<u32>,
}

#[derive(Debug, Serialize)]
pub struct Defaults {
    /// Policy-derived only: existence, privacy, and readiness are not checked.
    pub integration_state_root: Option<String>,
}

/// Inspect process environment and native executable identity only. Does not
/// launch subprocesses, inspect provider data, connect sockets, or create state.
pub fn status() -> Result<Status> {
    let executable = std::env::current_exe()?
        .canonicalize()?
        .into_os_string()
        .into_string()
        .map_err(|_| Error::non_unicode_operand())?;
    let mut diagnostics = Vec::new();
    let integration_state_root = match integration_runtime::default_state_root().and_then(|root| {
        root.into_os_string()
            .into_string()
            .map_err(|_| Error("integration_invalid_path"))
    }) {
        Ok(root) => Some(root),
        Err(error) => {
            diagnostics.push(error.code().to_owned());
            None
        }
    };
    // Unix entrypoints also require an implemented OS privacy policy.
    #[cfg(unix)]
    let supported = crate::private_access::supports_privacy();
    #[cfg(not(unix))]
    let supported = false;
    let versions = if supported { vec![1] } else { vec![] };
    Ok(Status {
        contract: STATUS_CONTRACT,
        version: env!("CARGO_PKG_VERSION"),
        executable,
        capabilities: Capabilities {
            integration_runtime: Capability {
                versions: versions.clone(),
            },
            native_event_reporting: Capability { versions },
        },
        defaults: Defaults {
            integration_state_root,
        },
        diagnostics,
    })
}

pub fn status_arguments(json: bool) -> Result<TaskOutput> {
    let status = status()?;
    let stdout = if json {
        serde_json::to_string(&status)?
    } else {
        format!(
            "Contract: {}\nVersion: {}\nExecutable: {}\nIntegration runtime versions: {:?}\nNative event reporting versions: {:?}\nDefault integration state root: {}\nDiagnostics: {:?}",
            status.contract,
            status.version,
            status.executable,
            status.capabilities.integration_runtime.versions,
            status.capabilities.native_event_reporting.versions,
            status
                .defaults
                .integration_state_root
                .as_deref()
                .unwrap_or("unavailable"),
            status.diagnostics,
        )
    };
    Ok(TaskOutput {
        stdout,
        warnings: Vec::new(),
        exit_code: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_v1_schema_and_build_capabilities() {
        let status = status().unwrap();
        let versions: &[u32] = if cfg!(any(target_os = "linux", target_os = "macos")) {
            &[1]
        } else {
            &[]
        };
        assert_eq!(status.contract, "coven.doctor-status.v1");
        assert_eq!(status.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(status.capabilities.integration_runtime.versions, versions);
        assert_eq!(
            status.capabilities.native_event_reporting.versions,
            versions
        );
        assert_eq!(
            std::path::Path::new(&status.executable),
            std::env::current_exe().unwrap().canonicalize().unwrap()
        );
        assert!(std::path::Path::new(&status.executable).is_absolute());
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            json!({
                "contract": "coven.doctor-status.v1",
                "version": env!("CARGO_PKG_VERSION"),
                "executable": status.executable,
                "capabilities": {
                    "integration_runtime": {"versions": versions},
                    "native_event_reporting": {"versions": versions}
                },
                "defaults": {"integration_state_root": status.defaults.integration_state_root},
                "diagnostics": status.diagnostics
            })
        );
    }

    #[test]
    fn output_is_metadata_only_and_has_no_library_logs() {
        for json in [false, true] {
            let output = status_arguments(json).unwrap();
            assert_eq!(output.exit_code, 0);
            assert!(output.warnings.is_empty());
            if json {
                serde_json::from_str::<serde_json::Value>(&output.stdout).unwrap();
            } else {
                assert!(output.stdout.contains(STATUS_CONTRACT));
                assert!(output.stdout.contains("Native event reporting versions:"));
                assert!(output.stdout.contains("Default integration state root:"));
                assert!(!output.stdout.contains("ready"));
            }
        }
    }
}
