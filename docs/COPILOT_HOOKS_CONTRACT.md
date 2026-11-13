# Copilot hooks configuration contract

## Overview

Coven owns new GitHub Copilot hook configuration on **Linux**, scoped to one
explicit global user `settings.json` and reference-only `SessionStart` capture.
Kitsune's Linux new-install/recommendation/update/bulk paths are retired; its
non-Linux production installer/assets/result type and recommendation/update/bulk
paths remain until matching Coven native qualification is complete. All-platform detection,
status interpretation, start/resume, agent-aware restore, legacy status/uninstall
and old ingress remain Kitsune compatibility, not completed extraction.
Pi/OpenCode reference-file placement is also implemented, not full management. See the
[ownership matrix](AGENT_MANAGEMENT_MIGRATION.md).

### Native documentation pin

Format evidence is **documentation only**, pinned to `github/docs` commit
`7c3387d3b513ed43ea26c9b2e4c0e1b12234c010`:

- [Raw hooks reference](https://raw.githubusercontent.com/github/docs/7c3387d3b513ed43ea26c9b2e4c0e1b12234c010/content/copilot/reference/hooks-reference.md)
- [Raw CLI configuration-directory reference](https://raw.githubusercontent.com/github/docs/7c3387d3b513ed43ea26c9b2e4c0e1b12234c010/content/copilot/reference/copilot-cli-reference/cli-config-dir-reference.md)

This is not a native implementation pin, binary/version certification, earliest
supported release claim, or proof of operational delivery in any provider mode.
Native user settings support **JSONC**; Coven deliberately accepts only a
**strict, duplicate-free JSON subset** and refuses comments/trailing commas rather
than making lossy JSONC edits. Global settings are `settings.json`, not the native
internal-state `config.json`. Coven does not discover HOME, `COPILOT_HOME`, native
stores, repository/plugin/policy hook roots, or executable versions.

## API / Interfaces

For the [normal workflow](NORMAL_AGENT_WORKFLOW.md), use the explicit
[`--from-kitsune` environment route](PROVIDER_ENVIRONMENT_ROUTES.md), not manual
receiver/socket setup. CLI environment-route plan/install defaults to the current
Coven executable; omit state-root to use the configured reporter context. Existing
fixed routes require guarded uninstall before selecting the new route. The
fixed-endpoint examples and receipt versions below remain compatibility details.

```text
coven agent hooks plan|install|uninstall|rollback --provider copilot --config-root ABS_CONFIG_ROOT [--executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET] [--migrate-kitsune] [--apply] [--json]
coven agent report --provider copilot --socket ABS_COVEN_SOCKET [--event SessionStart] [--json]
```

All operations without `--apply` are effect-free previews; `plan` ignores
`--apply`. Previews create no lock or files. Actual install/uninstall/rollback
apply uses the shared Linux/macOS backend and may create the persistent lock **before settings or
receipt validation fails**. Do not interpret a refused apply as artifact-free.
macOS explicit `--apply` is source-enabled on eligible local APFS, provisional and
not natively qualified; other Unix platforms preview but refuse mutation, and
Windows is unsupported. See the [backend matrix](HOOK_FILE_BACKEND_CONTRACT.md).
Receiver support is Linux/macOS only, using shared privacy admission; macOS local
APFS remains provisional and natively unqualified. Other Unix receivers refuse
before bind; Windows is unsupported. See [private storage](AGENT_PRIVATE_STORAGE_CONTRACT.md)
for the Linux receiver's permitted/available `fchmodat2` requirement (6.5 or backport).
Kitsune's non-Linux paths remain.

The explicit existing config root must be owned and not writable by others.
Descriptor-relative `openat`/`O_NOFOLLOW` traversal pins directories; owned,
single-link regular leaves are required, not symlinks. Paths must be absolute,
control-free and free of parent traversal. Fixed-route install requires explicit executable
and socket paths; choose a trusted Coven executable. Paths are lexical pins, not
authenticated/probed binaries or proof of receiver readiness. Plan can return
`planned_requires_executable_and_socket` without those arguments. No provider,
receiver, daemon or Kitsune is launched by hook management.

Library: `copilot_hooks::{Operation, Request, arguments}`, reusing Claude's request
types and private `claude_hooks::single_file::Policy` runner. Success returns
`TaskOutput` exit 0; fixed refusals use `copilot_hooks_*`, not raw settings/OS
diagnostics. JSON output includes provider/status, settings basename, events,
before/after digests, migration flag, `native_contract`, `native_hook_approval`
and `native_policy`. Human output is `Copilot hooks: STATUS`.

## Data Flow

1. Read at most **1 MiB** of strict duplicate-free JSON. Validate direct handler
   execution positions, preserving unrelated settings and custom handlers
   semantically. Installed formatting is reserialized; rollback retains exact bytes.
2. Append one **direct** `hooks.SessionStart[]` entry, not a Claude nested matcher
   group: `type: command`, `bash`, `timeoutSec: 10`. The generated command is:

```sh
'/absolute/coven' agent report --provider copilot --socket '/absolute/receiver.sock' --event SessionStart >/dev/null 2>&1 || true # coven-managed:copilot-hooks:v1
```

3. Shell-quote explicit paths and suppress capture errors/output best-effort. No
   Kitsune or native provider command is invoked. Stop providers/settings editors
   before apply; installed configuration does not guarantee native execution.
4. `agent report` requires `session_id` and exact `hook_event_name: SessionStart`.
   Optional `--event` must match; it never supplies a missing payload event.
   No `sessionId`, camelCase event/field aliases or inferred events are accepted.
   SID validation remains nonblank, at most 512 UTF-8 bytes, no controls or leading
   hyphen after leading whitespace. Input is at most 1 MiB with bounded depth.
5. Construct a new minimal object containing only provider, event and SID before
   RPC. Drop all unknown data, including `initial_prompt`, prompt/transcript,
   `cwd`, native `timestamp`/`source` and agent metadata. **No Claude subagent
   suppression applies to Copilot**: `agent_id` is ignored, not interpreted.
6. Use the existing private receiver/store and optional provider-labelled generic
   view. Copilot stays **unknown even when fresh**; age is receipt-local, not native
   activity. SHA-256 of `copilot`, NUL, exact SID separates providers without
   changing Claude/Codex hashes. Store `schema_version: 1`, its existing fields and
   the **128-record aggregate** cap across all three providers remain unchanged.
   After restart, all providers have unknown state/null age until another receipt.

The documentation-shaped native payload is, for example:

```json
{
  "hook_event_name": "SessionStart",
  "session_id": "example-unverified-reference",
  "timestamp": "2026-11-10T00:00:00Z",
  "cwd": "/example/project",
  "source": "startup",
  "initial_prompt": "private prompt, dropped before RPC"
}
```

Coven requires only SID/event, not the other native fields. Reference capture is
caller-supplied and unverified, never working/idle/ended lifecycle, native identity,
readiness, job completion, pane association, adoption, role or resume authority.
See the [receiver contract](AGENT_MANAGEMENT_CONTRACT.md) for endpoint/privacy rules.

### Settings Policy

`disableAllHooks`, sandbox, trust and approval settings are preserved, never
enabled/disabled or granted by this operation. Native policy/approval may prevent
execution. Custom direct command/exec, HTTP and startup prompt handlers are
validated in their execution positions; arbitrary metadata/prompt/env strings are
not recursively scanned as commands. Nested Claude `hooks` groups are refused.
HTTP `allowedEnvVars` requires **HTTPS**, including an empty array; permission and
pre-tool HTTP handlers also require HTTPS. This review constraint is implemented,
not pending. The manager does not perform HTTP requests, grant network policy or
certify native URL admission; native policy may be stricter than local validation.

### Receipt And Operations

Copilot shares the single-file runner, **not Claude's namespace or policy**:

- Receipt: `coven-copilot-hooks.json`, strict **version 2**, private 0600.
- Persistent lock: `.coven-copilot-hooks.lock`, private 0600.
- Recovery: `.coven-copilot-hooks-recovery/`, private 0700.
- Owned marker: `coven-managed:copilot-hooks:v1`; unreceipted ownership is refused.

The receipt records exact original bytes/mode or absence, executable/socket,
migration flag, postimage digest/mode and pending/completed stage. Generated
settings use 0600. Apply fences current bytes/modes, persists pending receipt,
replaces settings, then finalizes the receipt with synchronization. Claude's
behavior/v2 receipt/errors/statuses are preserved by sharing this runner. Codex
still has a **separate v1 two-file journal**, not this single-file receipt protocol.

| Operation | Effect with explicit `--apply` |
| --- | --- |
| `plan` | None, even with apply. |
| `install` | Add owned direct startup entry; exact repeated target is unchanged. |
| `uninstall` | Remove Coven entry from exact managed image; never revive Kitsune references. |
| `rollback` | Guardedly restore exact original bytes/mode or absence; may explicitly restore old Kit references. |

Post-install edits, including chmod-only changes, conflict rather than being
merged or overwritten. Resupplied executable/socket must match the receipt.

### Explicit Legacy Migration

`--migrate-kitsune` is required for the exact v2 POSIX handler in `SessionStart`:

```json
{"type":"command","bash":"bash 'ROOT/hooks/kitsune-agent-state.sh'","timeoutSec":10}
```

`ROOT` must be the exact selected absolute config root with shell quoting matching
the generated legacy command. Additional handler fields, alternate events/commands,
unknown historical or modified references refuse rather than being deleted. The
script must match SHA-256:

```text
40dee35dc22f1486267789e499deab23f382bd78ee92edd940f47e369421afbe
```

The script itself is retained untouched. Pending migrated install at its source
reverifies the script; an already committed target does not need the old asset.
Normal uninstall never reinstates Kit commands; explicit rollback can restore the
exact preimage. No automatic migration, live-pane authority release or running
session conversion occurs. Kit uninstall is separate legacy cleanup; do not run
it first if Coven needs the exact migration preimage.

## Recovery And Limits

The lock coordinates Coven writers, not arbitrary editors/native writers or old
open file descriptors. Retained-displacement replacement is **not global CAS** or
a filesystem-wide transaction. Detected races/crashes can already have changed a
live leaf and retain `expected`, `replacement`, `displaced`, `transaction.json`
artifacts as applicable under the recovery directory. All operations, including
preview, refuse while it exists. Preserve/inspect evidence with writers stopped;
never blindly exchange back over newer edits or delete retained evidence. There
is no automatic recovery command; the manifest is data, not command authority.

A pending receipt without that directory is distinct: inspect with `plan`, then
resume the **same operation** explicitly with `--apply`. Source or already-written
target must match bytes and mode. Durability uncertainty is neither proof of no
effects nor rollback/retry permission. Receipts/preimages/recovery can contain
settings secrets; do not publish raw files. Hook digests do not make them public.

## Example Usage

Reference syntax only, using inspected absolute paths and a trusted Coven binary:

```text
coven agent hooks plan --provider copilot --config-root ABS_CONFIG_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --json
coven agent hooks install --provider copilot --config-root ABS_CONFIG_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --json
coven agent hooks uninstall --provider copilot --config-root ABS_CONFIG_ROOT --json
coven agent hooks rollback --provider copilot --config-root ABS_CONFIG_ROOT --json
```

These are previews. Add `--migrate-kitsune` to plan/install only for verified legacy
migration. After reviewing and stopping writers, explicitly add `--apply` to the
chosen mutation on Linux, or provisionally on eligible macOS local APFS as described
in the backend guide. Start `coven agent serve WORKSPACE --socket
ABS_COVEN_SOCKET --duration 60 --json` separately, with the existing owned exact
0700 socket parent and valid Coven workspace required by the receiver contract.

## Validation

Current implementation evidence: generated commands for **Claude, Codex and
Copilot reach the real Coven receiver end to end**, without native providers or
Kitsune. This validates Coven command/transport integration, redaction, provider
separation and offline tolerance, not native delivery/approval/sandbox behavior.
The current cut passed **188 focused Coven checks**, strict all-target lint in both
applications, Rust 1.85 all-target checking, optimized build, source-package
verification and packaged test-target compilation. Kitsune passed **103 integration
checks** and **8 Copilot checks** (overlapping filters, not additive). See the
[validation record](AGENT_MANAGEMENT_MIGRATION.md#validation-record) for scope.
Historical 176 Coven/330 Kit results there are earlier Claude/Codex evidence, not
new Copilot totals. No full-suite, installed native provider or cross-platform
certification is claimed.

## Related Files

- `src/copilot_hooks.rs`, `src/copilot_hooks/{native,policy,tests}.rs`
- `src/claude_hooks.rs` (`single_file::Policy`), `src/hook_files.rs` (`Spec::copilot()`)
- `src/agent_management.rs`, `src/agent_management/{store,unix}.rs`, `src/main.rs`
- `tests/copilot_hooks_workflow.rs`, `tests/copilot_agent_migration_workflow.rs`
- Legacy fixture/license: `tests/fixtures/copilot_hooks/{kitsune-agent-state.sh,KITSUNE_LICENSE}`
- [Migration ownership](AGENT_MANAGEMENT_MIGRATION.md), [Claude](CLAUDE_HOOKS_CONTRACT.md), [Codex](CODEX_HOOKS_CONTRACT.md)
- Optional sibling guide: [Kit Copilot migration](../kitsune/COPILOT_HOOK_MIGRATION.md)
