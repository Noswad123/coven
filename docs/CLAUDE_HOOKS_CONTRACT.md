# Claude hooks configuration contract

## Overview

Coven owns new Claude hook configuration on **Linux**, with explicit, offline
settings management. Kitsune preserves its non-Linux legacy installer, assets,
result type and recommendation/update/bulk paths until native platform qualification
is complete; macOS file apply is now source-enabled but provisional. See the
[shared backend guide and support matrix](HOOK_FILE_BACKEND_CONTRACT.md).
This is transitional, not permanent ownership. Coven does not discover or
launch Claude, install a provider, select a user's HOME, or require Kitsune. This
is the first [all-agent migration](AGENT_MANAGEMENT_MIGRATION.md) cut, not complete
Claude detection/start/resume/restore extraction.

## API / Interfaces

```text
coven agent hooks plan|install|uninstall|rollback --provider claude --config-root ABS_CONFIG_ROOT [--executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET] [--migrate-kitsune] [--apply] [--json]
```

All operations without `--apply` are **effect-free previews**; `plan` ignores
`--apply`. Preview reads/validates settings and receipts but creates no lock or
files. The existing config root must be explicit, owned by the current user and
not writable by others. Component-by-component `openat`/`O_NOFOLLOW` traversal
pins directory descriptors. Paths are absolute, control-free and contain no `..`.
Existing leaves must be safe owned single-link regular files, not symlinks.

Install requires explicit absolute executable and socket paths; the user must
choose a **trusted Coven executable**. Paths are validated lexically, not probed
or authenticated. Plan can inspect without them and report
`planned_requires_executable_and_socket`. No implicit roots, PATH lookup,
provider probes or native execution occur.

Mutation uses the shared Linux/macOS backend: explicit macOS `--apply` is
source-enabled on eligible local APFS, provisional and not natively qualified.
Other Unix platforms preview but refuse apply; non-Unix is unsupported.
This does not retire Kitsune's macOS/Windows installer.
Coven's receiver supports Linux/macOS only, with shared privacy admission and
provisional, natively unqualified local-APFS macOS support. Other Unix receivers
refuse before binding; Windows is unsupported. See
[private storage and endpoints](AGENT_PRIVATE_STORAGE_CONTRACT.md), including the
Linux receiver's available/permitted `fchmodat2` requirement (6.5 or backport).
Library: `claude_hooks::{Operation, Request, arguments}`; successful plans,
mutations and unchanged results return `TaskOutput` exit 0. Refusals expose fixed
codes, not raw settings/OS diagnostics. JSON reports provider, status, settings
basename, event list, before/after digests and migration flag; default output is
`Claude hooks: STATUS`.

## Data Flow

1. Read typed, duplicate-free native settings JSON (at most **1 MiB**), validate
   hook structure and preserve custom settings/hooks semantically. Installation
   reserializes JSON; it does not preserve formatting in the installed image.
2. Generate six hook entries: `SessionStart`, `UserPromptSubmit`, `PreToolUse`,
   `PostToolUse`, `Stop`, `SessionEnd`, each with a two-second timeout. Each invokes
   only the chosen Coven executable's `agent report --provider claude --socket …
   --event EVENT`, with shell-quoted paths, `>/dev/null 2>&1 || true` and marker
   `coven-managed:claude-hooks:v1`. Never invoke Kitsune or a native provider.
   Offline capture failure is best-effort and silent, not native CLI failure.
3. On explicit apply, take `.coven-claude-hooks.lock` and write private **0600**
   `coven-claude-hooks.json` receipt **version 2**, recording original settings
   absence or exact byte preimage, original mode, target digest/mode, paths,
   migration consent and pending/completed stage. Generated settings are **0600**.
4. Persist pending receipt, replace settings, then finalize receipt. Synchronize
   staged files/directories. This is recoverable sequencing, **not a filesystem-wide
   transaction** across all files or cooperating/noncooperating writers.

| Operation | Effect with explicit `--apply` |
| --- | --- |
| `plan` | None, even with `--apply`. |
| `install` | Add owned entries; identical repeated installed target is `unchanged`. |
| `uninstall` | Remove Coven entries from the exact managed postimage; **never restore Kitsune references**. |
| `rollback` | Restore exact byte preimage and original mode, or original absence, only when the installed postimage remains unmodified. May explicitly restore old Kitsune references. |

Byte **and mode** checks are conservative: custom edits after install, including
chmod-only changes, conflict rather than being merged/overwritten. Unreceipted
Coven marker strings are refused. Optional resupplied paths must match the receipt.

### Explicit legacy Kitsune migration

`--migrate-kitsune` consents only to recognized commands for the exact selected
root's `hooks/kitsune-agent-state.sh` and the known current script SHA-256:

```text
00b90511a1eb781776952232309a8db3f6ed20a54ece679339cf655b9f6a6ff9
```

Recognized event/action pairs include session/idle for `SessionStart`, working for
the known prompt/tool/subagent events, blocked for `PermissionRequest`, idle for
`Stop`, and release for `SessionEnd`. Exact owned command strings and script bytes
must match. Missing consent, ambiguous/modified/historical unknown references or
changed script bytes refuse migration. Custom hooks/settings are preserved; the
old script itself is **left untouched**. An already migrated target needs no old
asset to remain present. Resuming a migrated pending install still at its **source**
reverifies the pinned script before replacing settings.

This changes selected configuration only: no legacy in-memory pane authority is
released and no old running native session is converted. Kitsune retains legacy
status/uninstall/ingress on all platforms. It is not a second new installer on
Linux; on non-Linux its production `legacy_install_claude` path remains until
Coven has natively qualified matching platform support.

## Edge Cases

**Stop native providers and settings editors before apply.** The lock coordinates
Coven writers, not arbitrary native/editor writes or old open file descriptors.
Strict CAS against noncooperating writers is **not** promised. Atomic exchange or
no-replace retains displaced entries until verification; a detected race can
already have changed the live leaf.

Detected races/crashes leave a private **0700**
`.coven-claude-hooks-recovery/` with fixed `expected`, `replacement`, `displaced`,
`transaction.json` artifacts as applicable, and `claude_hooks_recovery_required`.
The manifest is data, never code authority; displaced external entries may be of
unexpected type/size. All operations, including preview, refuse while this recovery
entry exists. Preserve and inspect both current and retained images; resolve
manually with writers stopped. **Never blindly exchange back or delete evidence**
over a potentially newer user edit. There is no automatic recovery command.

A pending receipt without that recovery directory is different: inspect with
`plan`, then explicitly resume the **same operation** with `--apply`. Source or
already-written target must match bytes and mode. A conflicting stage cannot be
silently switched to another operation. Durability uncertainty is not rollback
permission or proof of no effects.

The private receipt/preimage and recovery artifacts can contain settings secrets.
**Do not share backups or raw files** in logs/issues. Redacted digest output is not
permission to publish the original configuration. No live user configuration was
migrated or installed by this documentation task.

## Example usage

Reference syntax only; replace placeholders with inspected paths. First preview:

```text
coven agent hooks plan --provider claude --config-root ABS_CONFIG_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --json
coven agent hooks install --provider claude --config-root ABS_CONFIG_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --json
coven agent hooks uninstall --provider claude --config-root ABS_CONFIG_ROOT --json
coven agent hooks rollback --provider claude --config-root ABS_CONFIG_ROOT --json
```

These examples are all previews. For verified legacy migration, add
`--migrate-kitsune` to plan/install. Only after stopping writers and reviewing the
plan, explicitly add `--apply` to the chosen install/uninstall/rollback on Linux,
or provisionally on macOS subject to the shared backend's local-APFS restrictions.
Prepare and run the [private receiver](AGENT_MANAGEMENT_CONTRACT.md) separately;
installation does not start it or verify native provider compatibility.

## Related files

- Shared private filesystem implementation: `src/hook_files.rs` (`Spec::claude()`).
  Extraction preserves Claude behavior, v2 receipt compatibility, error codes and
  statuses; provider policy/receipts remain in `src/claude_hooks.rs`. Codex uses a
  separate allowlist/recovery namespace and v1 two-file journal, not a Claude
  receipt migration. See [Codex contract](CODEX_HOOKS_CONTRACT.md).
- `src/claude_hooks.rs`, `src/main.rs`, `src/strict_json.rs`
- `tests/claude_hooks_workflow.rs`, `tests/claude_agent_migration_workflow.rs`,
  `tests/fixtures/claude_hooks/KITSUNE_LICENSE`
- [Agent management contract](AGENT_MANAGEMENT_CONTRACT.md),
  [migration ownership](AGENT_MANAGEMENT_MIGRATION.md)
- Optional sibling guide: [Kitsune Claude migration](../kitsune/CLAUDE_HOOK_MIGRATION.md)
