use super::{NATIVE_PIN, Request, asset};
use crate::{Result, task_mutation::TaskOutput};
pub(crate) static POLICY: crate::extension_assets::Policy = crate::extension_assets::Policy {
    leaves: ["coven-agent-reference.ts", "kitsune-agent-state.ts"],
    receipt: "coven-pi-extension.json",
    lock: ".coven-pi-extension.lock",
    spec: crate::hook_files::Spec::pi(),
    generate: asset::generate,
    native_pin: NATIVE_PIN,
    kit: include_bytes!("../../tests/fixtures/pi_extension/kitsune-agent-state.ts"),
    kit_sha: "cce258cccccd2fdf66549cc89f1eee72b3293f558a7da96b8b87fe9071b3f247",
    schema: "coven-pi-extension",
    provider: "pi",
    label: "Pi",
    errors: crate::extension_assets::errors!("pi_extension"),
};
pub(super) fn run(request: &Request, json: bool) -> Result<TaskOutput> {
    crate::extension_assets::run(&POLICY, request, json)
}
