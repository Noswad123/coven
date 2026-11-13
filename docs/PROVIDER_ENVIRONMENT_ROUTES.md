# Explicit provider environment routes

## Overview

All five offline installers (Claude, Codex, Copilot, Pi and OpenCode) support an
explicit environment route. The CLI selects `--from-kitsune`, optionally with
`--state-root ABS`, instead of `--socket ABS`. No existing installation opts in
automatically. Executable paths must still be valid absolute paths in library
requests; only the CLI may explicitly choose its validated current executable.

## API / Interfaces

The crate exports `pub mod integration_route;`. Existing provider
`Request` structs are unchanged. Set their `socket` field to
`Some(integration_route::installed_pin(state_root)?)`, where the exact public
signature is `pub fn installed_pin(state_root: Option<&Path>) -> Result<String>`.
Pass `None` as the request socket when inspecting or undoing the stored route.
Do not pass an encoded installer pin to a reporter's socket argument.

Generated commands contain literal `--from-kitsune [--state-root ABS]`, never
`--socket` with a synthetic endpoint. Shell hooks quote each path independently;
extensions use JSON-encoded argument arrays and `spawn` with `shell: false`.
The fixed-route JavaScript templates and fixed hook commands are unchanged.
Environment plans include a typed `route` object in JSON and a route label in
text. No API address or native session identifier is stored in route metadata.

## Data Flow

The installer never resolves the environment or contacts Kit. At capture time,
the reporter must resolve the inherited context through
`integration_runtime::reporter_socket(None)` when no root override is present.
This derives the already-configured root from the selected generic Kit
`reportCommand`, not a default endpoint. Explicit roots must agree with that
context. Missing, stale, foreign or incomplete runtime context must refuse;
there is no fallback to another receiver.

## Receipts And Migration

Fixed-route receipts retain their existing versions and serialized fields:
Claude/Copilot v2; Codex/Pi/OpenCode v1. Environment receipts use v3 and v2,
respectively, with an explicit validated `from_kitsune` route enum and canonical
versioned installer pin. Their route, pin and receipt version must agree. Old
readers reject the new version/field rather than treating a pin as a socket.
Changing only a new receipt's version back to the legacy version is refused.

An active or pending receipt cannot change routes. Such a request refuses with
the provider's existing conflict error. Explicitly uninstall the old route
without resupplying a different pin, then install with `--from-kitsune`. A
completed rollback also permits a fresh route selection. Exact preimage and
mode checks still apply. Extension journals preserve the first original images
across that transition; existing provider-specific uninstall/rollback semantics
remain in force. Pending operations must resume using the same recorded route.

## Edge Cases

Placement is not native activation, hook approval, trust or consent. There are
no changes to legacy Kit fingerprints, settings migration consent, native event
allowlists, unknown-state behavior or payload privacy. Codex/Copilot still emit
only their supported native events. Pi/OpenCode remain reference-only reporters
with the same eligibility checks, bounded child ownership and best-effort loss
behavior. Providers must restart in new panes after a Kit server restart;
environment routing does not add process/native-session identity joins.

Linux offline execution does not qualify real provider UI emission or macOS.

## Example usage

After reviewing `coven integration plan` and explicitly applying setup, select
an existing absolute provider root. Stop providers/editors before mutation:

```text
coven agent hooks plan --provider claude --config-root ABS_CLAUDE_ROOT --from-kitsune
coven agent hooks install --provider claude --config-root ABS_CLAUDE_ROOT --from-kitsune --apply
coven agent extensions plan --provider pi --extension-root ABS_EXTENSION_ROOT --from-kitsune
coven agent extensions install --provider pi --extension-root ABS_EXTENSION_ROOT --from-kitsune --apply
```

Use the same Coven executable as setup. Only CLI environment-route plan/install
defaults to its current executable; fixed routes/library requests do not gain
executable discovery. No state-root repetition is needed: the hook omits it and
the inherited selected reporter argv supplies the configured value. Restart Kit
and providers in new panes; for OpenCode restart its native server, not just attach.
Use `codex`/`copilot` for the other hooks and `opencode` for the other extension.
Native approval remains separate. Existing verified Kit assets require explicit
provider-specific `--migrate-kitsune` consent. An old active fixed route must first
be guardedly uninstalled without supplying a replacement route; never edit receipt
versions or force a new route over it.

## Related files

- [Setup contract](INTEGRATION_SETUP_CONTRACT.md),
  [normal workflow](NORMAL_AGENT_WORKFLOW.md).
- `src/main.rs` — `installer_endpoint`, `installer_executable`, reporter endpoint.
- `src/integration_route.rs` — canonical pins and receipt-version validation.
- `src/integration_runtime.rs` — inherited-context and active-instance resolution.
- [Claude](CLAUDE_HOOKS_CONTRACT.md), [Codex](CODEX_HOOKS_CONTRACT.md),
  [Copilot](COPILOT_HOOKS_CONTRACT.md), [Pi](PI_EXTENSION_CONTRACT.md),
  [OpenCode](OPENCODE_EXTENSION_CONTRACT.md) — provider-specific mutation/undo rules.
