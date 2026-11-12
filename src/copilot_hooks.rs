//! Offline management of one explicit Copilot global settings.json.
//! Native disablement, sandbox and approval remain user/native policy. Apply is
//! routed through the shared Linux/macOS backend (macOS provisionally), with
//! the single-file runner's retained-displacement safety,
//! v2 pending receipt protocol and conservative byte/mode conflicts, not global CAS.
//! Executable paths are explicit and lexical, not authenticated or executed here.
pub use crate::claude_hooks::{Operation, Request};
use crate::{Result, task_mutation::TaskOutput};

/// Effect-free unless apply is explicit and operation is not Plan. No discovery,
/// native execution, receiver start, trust grant or environment/home fallback.
pub fn arguments(request: &Request, json: bool) -> Result<TaskOutput> {
    #[cfg(unix)]
    {
        crate::claude_hooks::single_file::run(request, json, policy::POLICY)
    }
    #[cfg(not(unix))]
    {
        let _ = (request, json);
        Err(crate::Error("copilot_hooks_unsupported_platform"))
    }
}

#[cfg(unix)]
mod native;
#[cfg(unix)]
mod policy;
#[cfg(all(test, unix))]
mod tests;
