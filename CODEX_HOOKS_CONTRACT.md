# Codex hooks configuration contract

## Overview

Coven owns new Codex hook configuration on **Linux**, independently of Kitsune.
This offline, preview-first manager handles **both `hooks.json` and `config.toml`**
under one explicit existing root. It generates only reference capture at
`SessionStart`, not working/idle lifecycle reporting. Discovery, start/resume and
agent restore remain pending transfers for Codex and the other providers; see
[the ownership matrix](AGENT_MANAGEMENT_MIGRATION.md).

The shared private filesystem implementation in `src/hook_files.rs` was extracted
from Claude without changing Claude policy, v2 receipts, errors or statuses.
`Spec::codex()` has a separate fixed leaf allowlist, error prefix and recovery
directory. Codex has its own bounded **v1 two-file journal**, not a Claude receipt
upgrade or a globally atomic transaction.

## API / Interfaces

```text
coven agent hooks plan|install|uninstall|rollback --provider codex --config-root ABS_CONFIG_ROOT [--executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET] [--migrate-kitsune] [--apply] [--json]
coven agent report --provider codex --socket ABS_COVEN_SOCKET [--event SessionStart] [--json]
```

All operations preview without `--apply`; `plan` always previews, even with it.
Install needs selected absolute executable/socket paths unless supplied by a
matching receipt. Plan can use placeholders and return
`planned_requires_executable_and_socket`. Choose a trusted Coven binary: paths are
lexically validated, not probed/authenticated. No implicit HOME root, source flag,
provider/version probe, shell execution, daemon or provider start is part of the
manager. The common CLI event parser is broader; Codex's domain validation accepts
**only `SessionStart`**.

Library: `codex_hooks::{Request, arguments}`, reusing `claude_hooks::Operation`.
Success returns `TaskOutput` with exit 0; fixed `codex_hooks_*` errors do not echo
configuration/OS diagnostics. JSON includes provider, status, events, fixed file
names, operation/pending, before/after digests, feature changes, migration flag,
native contract pin and an explicit **native approval not granted** notice.

### Native format provenance and consent

Read-only upstream inspection pins **openai/codex `rust-v0.167.1`**, commit
`0a94398fa60c4fac0e7659e4213a0a36e613dd46`:

- [protocol/src/hooks.rs](https://github.com/openai/codex/blob/0a94398fa60c4fac0e7659e4213a0a36e613dd46/codex-rs/protocol/src/hooks.rs)
- [config/src/hook_config.rs](https://github.com/openai/codex/blob/0a94398fa60c4fac0e7659e4213a0a36e613dd46/codex-rs/config/src/hook_config.rs)
- [generated SessionStart command input schema](https://github.com/openai/codex/blob/0a94398fa60c4fac0e7659e4213a0a36e613dd46/codex-rs/hooks/schema/generated/session-start.command.input.schema.json)

The read-only [release tag reference](https://api.github.com/repos/openai/codex/git/ref/tags/rust-v0.167.1)
resolves through [annotated tag 595a35e8…](https://api.github.com/repos/openai/codex/git/tags/595a35e837a1b6752e60a5de85b89adf415b0ccb)
to that commit; this lookup is provenance, not signature authentication.

This is a release format pin, **not** an earliest-supported-version assertion,
all-modes certification or proof of installed native operation. Independently
declared validation DTOs accept native custom command/prompt/agent/MCP handlers;
representable structured MCP input stays **data**, never recursively scanned for
ownership. Original JSON values and editable TOML are retained, not serialized
from those DTOs. No upstream source or provider dependency is copied into the app.

Native approval is separate from the shared global feature switch. Native/user
controlled `[hooks.state.KEY]` fields `enabled`, `trusted_hash` and
`trusted_content_hash` are preserved. Coven neither manufactures hashes nor grants
approval/bypasses consent. Applying install authorizes **global
`[features].hooks = true`**, potentially enabling other configured hooks too; it
does not promise that the native provider will run this command.

## Data Flow

1. Read and validate **both** inputs (at most **1 MiB each**), receipt and all
   generated images before creating **any apply artifact**, including the lock.
   Strict JSON rejects duplicate keys. TOML uses surgical `toml_edit` changes,
   preserving comments and unrelated semantics, including native trust state.
   Set global `[features].hooks = true` and remove **only** the legacy
   `features.codex_hooks` alias, not similarly named unrelated keys. Inline/dotted
   `features` layouts refuse with `codex_hooks_unsupported_feature_layout`.
2. Preserve custom handlers; optionally remove only verified legacy Kit commands
   from direct native handler positions. Append one JSON group under
   `hooks.SessionStart`, with nested `hooks`, handler `type: "command"`, `timeout: 2`
   and **no matcher**. Its shell-quoted command is:

   ```text
   'ABS_COVEN_BINARY' agent report --provider codex --socket 'ABS_COVEN_SOCKET' --event SessionStart >/dev/null 2>&1 || true # coven-managed:codex-hooks:v1
   ```

3. Linux apply takes `.coven-codex-hooks.lock`, rereads/revalidates under lock, and
   fences both files/receipt/required legacy asset. Persist private 0600
   `coven-codex-hooks.json` pending journal with schema `coven-codex-hooks`, version
   1, paths, migration result, original/installed/source/target pairs, operation
   and pending flag. Each pair records exact bytes, mode and digest, or absence.
   The strict, unknown-field-denying journal is bounded to **8 MiB** and its target
   images are reconstructed from policy before acceptance. Leaf names come from
   fixed array positions, never journal-supplied paths; stored executable/socket
   strings are data, never executed by the manager.
4. Commit **`hooks.json` first, then `config.toml`, then finalize the journal**.
   Recheck the whole observed prefix between leaves and all results before
   finalization. Installed files use 0600. This is recoverable ordered sequencing,
   not global atomicity, global CAS or protection from later writes through old FDs.
5. Native stdin flows through Coven's minimal receipt projection to its private
   Unix receiver. Native's generated input schema requires all seven fields:
   `session_id`, `hook_event_name: "SessionStart"`, `source` (startup/resume/clear/
   compact/fork), `cwd`, nullable `transcript_path`, `model`, `permission_mode`.
   **Coven capture is intentionally narrower:** it requires only session ID/event,
   ignores the other native fields, and drops secrets/unknown fields before RPC,
   persistence or output. `thread_id`/`thread-id` are not aliases; an internal
   `ThreadId` name is not a wire alias. Do not describe a two-field capture example
   as the complete native schema.

The [receiver contract](AGENT_MANAGEMENT_CONTRACT.md) keeps schema_version 1 and
record fields unchanged. IDs hash provider + NUL + exact session ID; existing
Claude hashes stay unchanged and equal IDs across providers produce distinct
records. The **128-record aggregate** limit is shared, not per provider. Codex is
always `reported_state: unknown`, even with a fresh receipt/known age. After
restart **all** records are unknown with null age until a new receipt. Optional
generic rows use dynamic provider labels via unchanged `integration.view.*`;
Kitsune gains no provider model. UI failure does not stop capture.

### Uninstall, rollback and legacy migration

| Operation with `--apply` | Policy |
| --- | --- |
| `plan` | Never writes. |
| `install` | Add Coven command and enable shared global hooks; matching repeated target is unchanged. |
| `uninstall` | Remove the exact Coven command group; **leave global hooks enabled** and never revive Kit references. Disabling shared native hooks requires a separate user/native decision. |
| `rollback` | Restore **both** original byte/mode/absence preimages, only when all managed postimages remain unchanged (also supported after managed uninstall). Can explicitly restore legacy commands, alias and original feature value. |

Explicit `--migrate-kitsune` accepts only exact command strings for the selected
root's **`kitsune-agent-state.sh`** (not Claude's `hooks/` path) and known **version
6** asset SHA-256:

```text
2343713ae8e665baa22968e155ced9fdc1269f43b490af230316754a18caaf9a
```

Recognized commands are `bash 'ABS_ROOT/kitsune-agent-state.sh' ACTION`:
SessionStart session/idle; UserPromptSubmit or PreToolUse working;
PermissionRequest blocked; Stop idle. Only direct command handlers are ownership
positions, not MCP input, ignored metadata or approval state. Missing consent,
edited/unknown references or changed asset bytes refuse. The asset is **untouched**.
On pending migration resume, source leaves still requiring migration reverify the
asset; already migrated target leaves need no asset. No live pane authority is
released and no running session/user config is automatically converted.

## Edge Cases

- Stop providers and editors before applying. Roots use descriptor-relative,
  component-by-component no-follow traversal; owned, non-other-writable roots and
  single-link owned regular leaves only. Unknown byte edits **or chmod-only**
  changes conflict rather than merge. Resupplied paths must match the receipt.
- Pending journals without a recovery directory resume **only the same operation**
  at a legal committed prefix: source/source, target/source, target/target in
  hooks-then-TOML order. Plan may inspect; changing stages or unknown combinations
  refuses. Durability uncertainty is not proof of no effects or rollback permission.
- Shared Linux `RENAME_EXCHANGE` / `RENAME_NOREPLACE` replacement retains displaced
  entries until verification. Detected races may already have changed a live leaf.
  `.coven-codex-hooks-recovery/` (0700) holds applicable `expected`, `replacement`,
  `displaced`, `transaction.json` artifacts. All operations, even preview, refuse
  while it exists. Preserve/inspect evidence with writers stopped; **never blindly
  undo** over newer edits. There is no automatic recovery command. Receipts and
  recovery artifacts can contain secrets; do not publish them.
- Mutation is **Linux-only**. Other Unix platforms preview but refuse apply;
  non-Unix manager/receiver support is absent and macOS capture is unvalidated.
  Kitsune preserves its non-Linux legacy installer until matching Coven adapters
  are ready. Linux retires new install/recommend/update/bulk paths only;
  legacy status/uninstall/old ingress remain compatible. No platform-wide
  retirement, native trust approval or operational capture certification follows.

## Example usage

Reference syntax only, with inspected paths; the hook commands below are previews:

```text
coven agent serve WORKSPACE --socket ABS_COVEN_SOCKET --duration 60 --json
coven agent hooks plan --provider codex --config-root ABS_CONFIG_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --json
coven agent hooks install --provider codex --config-root ABS_CONFIG_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --json
coven agent hooks uninstall --provider codex --config-root ABS_CONFIG_ROOT --json
coven agent hooks rollback --provider codex --config-root ABS_CONFIG_ROOT --json
coven agent inventory --socket ABS_COVEN_SOCKET --json
```

`serve` runs foreground and writes local receipt state; prepare an existing owned
0700 socket parent separately. A plan for an absent/false flag discloses
`features.hooks_before: null/false` to `hooks_after: true`, alias removal if present,
and `native_hook_approval: "user-managed; may be required; not granted by this operation"`.
Review this **shared enablement consent** before adding `--apply` on Linux; writing
config alone cannot promise operational capture. Add `--migrate-kitsune` explicitly
to plan/install only for verified legacy migration. Installation starts no receiver.

Complete pinned native stdin shape (inert data, not a provider invocation):

```json
{"session_id":"example-unverified-id","hook_event_name":"SessionStart","source":"startup","cwd":"/example/workspace","transcript_path":null,"model":"example-model","permission_mode":"default"}
```

The same capture accepts only `session_id`/`hook_event_name` as its minimal input;
that narrower projection is not the complete native schema above.

## Related files

- `src/codex_hooks.rs`, `src/codex_hooks/{policy,native,unix}.rs`, `src/hook_files.rs`
- `src/agent_management{,_presentation}.rs`, `src/agent_management/{store,unix}.rs`, `src/main.rs`
- `tests/{codex_hooks_workflow,codex_agent_capture_workflow}.rs`, `src/codex_hooks/tests.rs`
- [Claude contract](CLAUDE_HOOKS_CONTRACT.md), [receiver](AGENT_MANAGEMENT_CONTRACT.md)
- Optional sibling [Kitsune migration guide](../kitsune/CODEX_HOOK_MIGRATION.md)

Current validation passes **176 focused Coven checks**, including 16 Codex hook
workflows, seven provider/journal fault checks, six shared-file checks, capture,
Claude regressions and existing CLI checks. The generated-command → real Coven CLI
receiver workflow (`tests/codex_agent_migration_workflow.rs`) passes, including
provider-separated IDs, silent offline delivery and exact two-file rollback.
Kitsune passes 330 focused checks and strict lint. Reviews closed the direct-handler
ownership and pinned-native-schema findings; MCP input and approval state remain
data, not migration targets.

The first combined run encountered a full shared `/tmp` and is not counted as a
pass. All affected checks were rerun successfully with disk-backed workspace scratch
space. Native-provider execution/approval, installed-server interoperability,
full-suite and other-platform validation remain unclaimed. No live user
configuration or provider was used.
