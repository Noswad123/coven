//! Offline Pi extension placement, not native activation or approval proof.
//! Linux mutations use a bounded two-leaf journal and descriptor-relative storage.
//! Detected races retain recovery artifacts, not a global filesystem CAS.
use crate::{Result, task_mutation::TaskOutput};

pub use crate::claude_hooks::Operation;

/// Upstream source commit; package version 1.5.2 is not a release-tag pin.
pub const NATIVE_PIN: &str = "earendil-works/pi 6e11a00a01a568ef0ae0df5074acf02fc107c34b";

pub use crate::extension_assets::Request;

/// Plan always previews. No discovery, native execution, settings or approvals.
pub fn arguments(request: &Request, json: bool) -> Result<TaskOutput> {
    #[cfg(unix)]
    {
        unix::run(request, json)
    }
    #[cfg(not(unix))]
    {
        let _ = (request, json);
        Err(crate::Error("pi_extension_unsupported_platform"))
    }
}

#[cfg(any(unix, test))]
mod asset;
#[cfg(unix)]
pub(crate) mod unix;
