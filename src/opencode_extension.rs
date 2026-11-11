//! Offline placement into one explicit existing OpenCode plugin or plugins root.
//! Placement is not proof of activation. No settings, discovery or native IO.
pub use crate::claude_hooks::Operation;
pub use crate::extension_assets::Request;
use crate::{Result, task_mutation::TaskOutput};
pub const NATIVE_PIN: &str = "anomalyco/opencode v1.19.5 810c0eba1a7348036f8bdf61e6632d0c36c6876c";
#[cfg(any(unix, test))]
mod asset;
#[cfg(unix)]
pub(crate) static POLICY: crate::extension_assets::Policy = crate::extension_assets::Policy {
    leaves: ["coven-agent-reference.js", "kitsune-agent-state.js"],
    receipt: "coven-opencode-extension.json",
    lock: ".coven-opencode-extension.lock",
    spec: crate::hook_files::Spec::opencode(),
    generate: asset::generate,
    native_pin: NATIVE_PIN,
    kit: include_bytes!("../tests/fixtures/opencode_extension/kitsune-agent-state.js"),
    kit_sha: "7782f028d172b7b56b95aa364f443b8929446a40ce19a505e55aa8d8e405312b",
    schema: "coven-opencode-extension",
    provider: "opencode",
    label: "OpenCode",
    errors: crate::extension_assets::errors!("opencode_extension"),
};
pub fn arguments(request: &Request, json: bool) -> Result<TaskOutput> {
    #[cfg(unix)]
    {
        crate::extension_assets::run(&POLICY, request, json)
    }
    #[cfg(not(unix))]
    {
        let _ = (request, json);
        Err(crate::Error("opencode_extension_unsupported_platform"))
    }
}
