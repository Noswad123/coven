//! Explicit native activity asset placement, independent of frozen reference assets.
pub use crate::claude_hooks::Operation;
pub use crate::extension_assets::Request;
use crate::{Result, task_mutation::TaskOutput};

#[cfg(unix)]
mod asset;

/// Only the two source-qualified profiles are supported. No native runtime IO.
pub fn arguments(provider: &str, request: &Request, json: bool) -> Result<TaskOutput> {
    #[cfg(unix)]
    {
        let policy = match provider {
            "pi" => &PI,
            "opencode" => &OPENCODE,
            _ => return Err(crate::Error("native_extension_unsupported_provider")),
        };
        crate::extension_assets::run(policy, request, json)
    }
    #[cfg(not(unix))]
    {
        let _ = (provider, request, json);
        Err(crate::Error("native_extension_unsupported_platform"))
    }
}

#[cfg(unix)]
static PI: crate::extension_assets::Policy = crate::extension_assets::Policy {
    leaves: ["coven-native-events.ts", "kitsune-agent-state.ts"],
    receipt: "coven-pi-native.json",
    lock: ".coven-pi-native.lock",
    spec: crate::hook_files::Spec::pi_native(),
    generate: asset::pi,
    native_pin: crate::pi_extension::NATIVE_PIN,
    kit: include_bytes!("../tests/fixtures/pi_extension/kitsune-agent-state.ts"),
    kit_sha: "cce258cccccd2fdf66549cc89f1eee72b3293f558a7da96b8b87fe9071b3f247",
    schema: "coven-pi-native",
    provider: "pi",
    label: "Pi native context activity (not root proof)",
    errors: crate::extension_assets::errors!("pi_native"),
};

#[cfg(unix)]
static OPENCODE: crate::extension_assets::Policy = crate::extension_assets::Policy {
    leaves: ["coven-native-events.js", "kitsune-agent-state.js"],
    receipt: "coven-opencode-native.json",
    lock: ".coven-opencode-native.lock",
    spec: crate::hook_files::Spec::opencode_native(),
    generate: asset::opencode,
    native_pin: crate::opencode_extension::NATIVE_PIN,
    kit: include_bytes!("../tests/fixtures/opencode_extension/kitsune-agent-state.js"),
    kit_sha: "7782f028d172b7b56b95aa364f443b8929446a40ce19a505e55aa8d8e405312b",
    schema: "coven-opencode-native",
    provider: "opencode",
    label: "OpenCode native root activity",
    errors: crate::extension_assets::errors!("opencode_native"),
};
