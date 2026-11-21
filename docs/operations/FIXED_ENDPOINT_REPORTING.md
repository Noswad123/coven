# Advanced fixed-endpoint agent reporting

## Prefer normal invocation

Use the [normal agent workflow](../guides/NORMAL_AGENT_WORKFLOW.md#example-usage)
for ordinary native invocation and explicit `--from-kitsune` setup. This guide is
only for deliberately managed foreground receivers and executable/socket-pinned
hook or extension routes. Fixed routes are not normal setup prerequisites and
are not automatically converted to normal managed routes.

The installed `coven` product is native Rust. Commands below run from the
repository root with inspected absolute physical paths. Uppercase operands are
placeholders, not values to copy literally. No implicit HOME/config-root lookup,
provider discovery/probe, daemon startup or session adoption is performed.

Current [app boundaries](../architecture/APP_BOUNDARIES.md#current-retirement-overlay-2026-11-16)
and [migration ownership](../plans/AGENT_MANAGEMENT_MIGRATION.md#current-global-retirement-overlay-2026-11-16)
supersede older Linux-only Kit retirement claims: all six fresh legacy installers
retired on every platform. Remaining status/uninstall/frozen assets are explicit
cleanup, not automatic migration or Coven macOS/Windows qualification.

## Start one explicit receiver

Select an existing workspace and owned socket parent of **exact mode 0700**.
`serve` runs in the foreground and writes local receipt state:

```text
coven agent serve WORKSPACE --socket ABS_COVEN_SOCKET --duration 60 --json
# In another terminal while that foreground receiver is running:
coven agent inventory --socket ABS_COVEN_SOCKET --json
```

Core receipt capture works without Kitsune or a native provider. Optional paired
`--kitsune-socket ABS_KITSUNE_SOCKET --source coven` on `serve` publishes read-only
rows through generic snapshots/Integration views only. The source must already
be configured manually. Kit failure does not stop core capture; this creates no
pane binding, adoption or native control.

The private receiver supports Linux and source-enabled/provisional local-APFS
macOS, **not native macOS qualification**. Other Unix receivers refuse before
bind; Windows is unsupported. Linux receiver startup needs available/permitted
`fchmodat2` (Linux 6.5 or a backport), without a pathname-following fallback; this
is not a kernel minimum for every Coven command or reporting hook.

Store/lease/socket entries are owned private objects; socket/store leaves require
0600. Physical symlink ancestors and `..` refuse. Root/state admission is not
generally exact 0700, unlike the socket parent. macOS admission requires eligible
local APFS, ownership enabled and no ACL entries. These checks are not provider
authentication, hostile same-user protection or an ancestor-wide privacy audit.
See [private storage](../architecture/AGENT_PRIVATE_STORAGE_CONTRACT.md).

## Preview pinned hook and extension placement

These operations are previews: **no `--apply` is shown**. `plan` always previews;
install/uninstall/rollback require explicit `--apply` to mutate provider files.

```text
coven agent hooks plan --provider claude --config-root ABS_CONFIG_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --json
coven agent hooks install --provider claude --config-root ABS_CONFIG_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --json
coven agent hooks plan --provider codex --config-root ABS_CONFIG_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --json
coven agent hooks install --provider codex --config-root ABS_CONFIG_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --json
coven agent hooks plan --provider copilot --config-root ABS_CONFIG_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --json
coven agent extensions plan --provider pi --extension-root EXISTING_ABS_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --migrate-kitsune --json
```

Choose one trusted Coven executable and explicit existing provider root. Pi uses
extension placement, OpenCode plugin placement, not hook JSON. OpenCode accepts
one existing absolute `plugin` or `plugins` root; there is no across-root dedupe.
Do not double-install. No directory/package creation, root autodiscovery or native
activation proof follows from file placement.

File mutation is source-enabled on Linux and provisionally eligible local-APFS
macOS; other Unix is preview-only, Windows unsupported. Stop providers, editors
and watchers before mutation. Restart/reload native applications deliberately;
OpenCode needs native **server** restart, not just TUI attach. There is no atomic
activation, automatic restart or release of in-memory authority. See the
[shared backend/support matrix](../architecture/HOOK_FILE_BACKEND_CONTRACT.md),
interpreting historical retirement rows through the current overlay above.

## Understand the receipts, not just freshness

Native `agent report` accepts Claude/Codex/Copilot hook JSON only. Input is bounded
to 1 MiB; prompt/transcript/CWD/unknown fields are dropped before wire/store/output.
Claude subagents are ignored. Its six-event capture is Claude-only; Codex and
Copilot capture exact `SessionStart` references and stay **unknown even fresh**.
Aliases or guessed events do not broaden these schemas.

Pi/OpenCode use separate `report-extension` envelopes with exactly three string
fields and matching explicit event: Pi's `coven.pi-extension.v1`/`session_start`,
OpenCode's `coven.opencode-extension.v1`/`session.created` or `session.updated`.
Unknown extension fields refuse rather than being dropped. These are reference
receipts, not native lifecycle signals. Separate newer native profiles/hook facts
and selected-root attachment contracts do not extend frozen `agent serve`.

All five providers share schema-v1 storage and an **aggregate 128-record cap**.
Full IDs remain in private state; inventory exposes provider-qualified hashes and
last-reported state/age, not native identity, readiness, completion or permission
to resume. Hashes are pseudonymous, not authentication or guessing resistance.
Restarted records are unknown with null age until a new receipt.

**Upgrade receivers first.** Older binaries reject newer provider wire/store
records despite unchanged schema 1; persisting them limits downgrade. Do not
equate schema number compatibility with supported provider semantics. Exact
wire/store fields, deadlines, outcomes and lifetime are canonical in
[agent management](../architecture/AGENT_MANAGEMENT_CONTRACT.md).

## Apply, migrate and recover deliberately

`--migrate-kitsune` is explicit consent for exact verified legacy commands/assets,
not permission to adopt arbitrary edits. Historical Pi/OpenCode reference-asset
migration drops working/blocked/idle to reference-only; it is not full native
management transfer. Normal uninstall never restores Kit references. Explicit
guarded rollback can restore original bytes/mode/absence; it does not resurrect
retired Kit runtime behavior or prove safe live activation.

Private receipts/backups can contain secrets. Preserve conflicts and recovery
evidence for inspection; never blindly retry, exchange back or undo newer edits.
Multi-file sequencing/journals are not global atomicity. Errors can occur after
live file changes, and uncertainty is not rollback or proof of zero effects.
No configuration or running session is automatically migrated. Reporting is
best-effort when offline, not exactly-once delivery or a safe resend protocol.

## Provider details and related files

Keep exact native release/source pins, supported layouts, migration fingerprints
and recovery steps in their canonical contracts rather than copying them here:

- [Claude hooks](../architecture/CLAUDE_HOOKS_CONTRACT.md).
- [Codex hooks](../architecture/CODEX_HOOKS_CONTRACT.md): two-file management and shared enablement, not native trust.
- [Copilot hooks](../architecture/COPILOT_HOOKS_CONTRACT.md): strict supported settings subset and documentation pin.
- [Pi extensions](../architecture/PI_EXTENSION_CONTRACT.md): explicit roots and frozen reference assets.
- [OpenCode extensions](../architecture/OPENCODE_EXTENSION_CONTRACT.md): server-instance scope, pins and restart limits.

Source entry points: [`src/agent_management.rs`](../../src/agent_management.rs),
`src/agent_management/{unix,files,store}.rs`, `src/private_access.rs`,
`src/hook_files.rs` and provider-specific hook/extension modules, routed by
[`src/main.rs`](../../src/main.rs). Historical validation and platform evidence
remain in the migration records; this extraction claims no fresh runtime tests,
installed activation or provider certification.
