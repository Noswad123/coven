# Pi reference extension contract

## Scope and ownership

Coven owns new Pi reference-extension placement on Linux, independent Unix
receipt capture, redacted inventory and optional generic Kitsune presentation.
This is **not full Pi management completion**. Pi lifecycle/status interpretation,
discovery, start/resume and agent-aware restore remain pending transfers. OpenCode
now has [separate reference-plugin placement](OPENCODE_EXTENSION_CONTRACT.md);
separate Djinn handling is not folded into Pi.
Mutation uses the [shared Linux/macOS backend](HOOK_FILE_BACKEND_CONTRACT.md).
Explicit macOS `--apply` is source-enabled, provisional local APFS only and not
natively qualified. Other Unix systems preview but refuse apply; Windows is
unsupported. macOS receiver/privacy and native activation remain unvalidated,
and all Kitsune non-Linux paths remain pending native qualification.

**Explicit functional reduction:** migration drops Kitsune's working/blocked/idle
reporting in favor of session references only. Fresh Pi receipts remain `unknown`.
Do not present this as lifecycle parity, live identity, readiness, completion or
permission to resume. Kitsune's non-Linux installer/recommendation/update/bulk paths,
all six `IntegrationTarget::ALL` entries, all-platform legacy status/uninstall,
old full-v7 asset ingress and lifecycle support remain until explicit migration.
Linux new install/recommend/update/bulk paths retire without auto-running Coven.

## Public commands

Reference syntax only; inspect the absolute paths first:

```text
coven agent extensions plan --provider pi --extension-root EXISTING_ABS_ROOT --executable ABS_COVEN --socket ABS --migrate-kitsune --json
coven agent extensions install --provider pi --extension-root EXISTING_ABS_ROOT --executable ABS_COVEN --socket ABS --migrate-kitsune --apply --json
coven agent extensions uninstall --provider pi --extension-root EXISTING_ABS_ROOT --json
coven agent extensions rollback --provider pi --extension-root EXISTING_ABS_ROOT --json
coven agent report-extension --provider pi --socket ABS --event session_start --json
```

All four placement operations accept `--executable`, `--socket`,
`--migrate-kitsune`, `--apply` and `--json`. `plan` always previews; the others
preview unless `--apply` is explicit. The extension root must already exist;
no automatic directory creation, HOME/root discovery, PATH lookup, native app
execution or discovery probes occur. First install requires executable/socket;
plan may omit them and returns `planned_requires_executable_and_socket`.
Later operations use the journal's pins; supplied pins must match. The receiver
uses an independently prepared owned socket parent with exact 0700 mode.

`agent report` remains native-hook JSON ingestion for **Claude, Codex and Copilot
only**. Pi uses `report-extension`; it does not have an invented Pi-native JSON
hook schema. Its required `--event session_start` cannot supply a missing payload
event. Stdin is bounded to 1 MiB, duplicate-free, depth-bounded JSON, with exactly
these three fields and string values:

```json
{"contract":"coven.pi-extension.v1","event":"session_start","session_id":"example-unverified-id"}
```

Unknown fields, aliases, other contracts/events and missing fields refuse. IDs
are nonblank, at most 512 UTF-8 bytes, contain no controls and cannot begin with
a hyphen after leading whitespace. The new minimal RPC object is only `provider`,
`event`, `session_id`. Pi + `session_start` identifies Coven's envelope and the
`getSessionId` interpretation, **not authenticated provenance**. Local callers can
forge receipts. Acknowledgements and inventory follow the
[receiver contract](AGENT_MANAGEMENT_CONTRACT.md), including delivery uncertainty
and no automatic retry.

## Native source and activation limits

Source pin: [`earendil-works/pi` at
`6e11a00a01a568ef0ae0df5074acf02fc107c34b`](https://github.com/earendil-works/pi/tree/6e11a00a01a568ef0ae0df5074acf02fc107c34b).
It declares package **1.5.2 on main**, not a release-tag pin and not the `b4af..`
release revision. This is source-API evidence, not installed-binary certification.
The pinned `session_start` API has five reasons: `startup`, `reload`, `new`,
`resume`, `fork`. The extension snapshots `ctx.sessionManager.getSessionId()` in
that callback, not a field inferred from a native JSON event.

Eligibility requires `ctx.hasUI === true`, which includes **TUI and RPC**. It
does not prove a root session or TUI origin; headless capture is not provided.
File placement is not native activation proof: native discovery can ignore the
selected root because of flags, root/index discovery rules or overrides. This
manager does not autoload an extension, change approvals, policy or settings,
prove delivery, or release existing in-memory Kitsune authority.

## Generated producer

`coven-agent-reference.ts` is distinct from legacy `kitsune-agent-state.ts`.
The generated producer imports Node `child_process.spawn`, uses the explicit
absolute Coven executable/socket with `shell: false`, and writes only the three
envelope fields to stdin. No prompt, path, cwd or native event/context object is
serialized. stdout/stderr are ignored; child failure is best effort.

The ID is snapshotted before spawning. Each extension factory has one active
child slot: overlapping callbacks are dropped, with no queue or retry. A two-second
timer requests `SIGKILL`; only `close` releases the slot, not an error or kill
request. `session_shutdown` permanently stops that factory and kills any active
child. Lost load-time events, dropped overlaps and uncertain reports are not replayed.

## Storage and upgrade order

The receiver retains unchanged `schema_version: 1` and private record fields
`provider`, `session_id`, `last_event`, `timestamp_ms`, `server_epoch`. Capacity is
**128 aggregate records across five providers**, not 128 each. Record IDs hash
provider + NUL + exact session ID; existing provider hashes remain stable. Full IDs
stay private; inventory/UI omit them. Pi is always `unknown`, even fresh; after
restart all providers have `unknown` state and null age until another receipt.

Upgrade the receiver before the producer. Older binaries reject Pi wire/store
records despite the unchanged schema number. Once Pi records exist, downgrade
is limited: an older receiver cannot read that store. No automatic deletion,
schema reset or downgrade conversion is supplied.
The same receiver-first rule applies to newly supported OpenCode records: an
older Pi-capable binary still rejects an unknown OpenCode provider despite schema 1.

## Migration and recovery

`--migrate-kitsune` explicitly authorizes replacement only of exact legacy v7
bytes with SHA-256:

```text
cce258cccccd2fdf66549cc89f1eee72b3293f558a7da96b8b87fe9071b3f247
```

A marker/filename is insufficient; modified assets refuse. Do not uninstall Kit
first if Coven needs its preimage. The private journal `coven-pi-extension.json`
has schema `coven-pi-extension`, version 1 and fixed leaf positions, never arbitrary
journal-supplied paths. Validation reconstructs the generated installed image,
requires original new-leaf absence, verifies exact legacy bytes/SHA and saved mode,
and validates current bytes/modes against the recorded transition. Original images
remain immutable across reinstall, including after uninstall or rollback.

Both leaves and reconstructed journal are validated before apply artifacts, then
reread under `.coven-pi-extension.lock`. Each leaf is bounded to 1 MiB and the
journal to 8 MiB. New extension/journal files are 0600. Install publishes a pending
journal, writes the new leaf first, deletes the verified legacy leaf second, then
finalizes the journal. Rollback restores legacy first, then removes the new leaf;
rollback after uninstall is supported. Normal uninstall **never revives Kit** and
retains the original preimage for explicit rollback.

Pending recovery accepts only a legal prefix of that operation's fixed write
order. Plan can inspect it; only the same operation may resume, not an arbitrary
operation switch. Preserve journal/lock/recovery artifacts on conflicts rather
than deleting them to force progress. Shared private `hook_files::Root` protections
are descriptor-relative cooperating-filesystem checks with recovery retention,
not global CAS or atomic native activation. Stop providers and watchers before
mutation: intermediate two-leaf states can be observed. Nothing forces autoload,
unloads running code or relinquishes legacy in-memory authority.

## Validation

The shared private `extension_assets` fixed-two-leaf engine now serves Pi and
OpenCode with separate policy/artifact/error namespaces. Pi's public API, v1 receipt
protocol and generated bytes remain frozen at `887451b`. Independent
[fixtures](tests/fixtures/pi_extension_887451b/) and
[compatibility tests](tests/pi_extension_frozen_compatibility.rs) exercise completed
and pending receipts, legal prefixes, reinstall and rollback after uninstall.
This extraction changes no Pi producer queue/shutdown behavior or protocol.
See the [current scoped validation table](AGENT_MANAGEMENT_MIGRATION.md#opencode-validation-table);
it records 236 current focused Rust checks and rerun Node/Bun producer checks,
with receipt-timing limitations retained. Historical totals are not additive.

The earlier Pi cut passed **212 focused Rust checks**, plus **7 Node producer-harness
tests** and the same harness under Bun. Generated TypeScript reached the real Coven
receiver through a fake Pi callback, including private storage/redaction and
rollback after uninstall. Kitsune passed **107 integration checks** and **9 Pi
checks** (overlapping filters). Strict lint, MSRV 1.85, optimized build, package
verification and packaged test-target compilation passed. See the
[validation table](AGENT_MANAGEMENT_MIGRATION.md#pi-validation-table) for scope,
corrected fixture failures and the retained high-parallel receipt timing limitation.
No live Pi capture, full-suite, macOS/Windows or native activation result is claimed.
Earlier Copilot results retain their original scopes and are not additive Pi totals.

The generated-asset Rust fixture needs a development Node runtime supporting
TypeScript loading (Node >=22.19). It selects `COVEN_TEST_NODE` or resolves `node`
from the test process's PATH before clearing the child's environment; no host path
or Node major is hardcoded. The validated host used Node 26, not every admitted
version. This is a test prerequisite, not Coven runtime executable discovery.

## Related files

- `src/pi_extension.rs`, `src/pi_extension/{asset.rs,template.js,unix.rs}`
- `src/extension_assets.rs`, `src/extension_assets/engine{.rs,/tests.rs,/policy_tests.rs}`
- `tests/pi_extension_frozen_compatibility.rs`, `tests/fixtures/pi_extension_887451b/`
- `src/agent_management.rs`, `src/agent_management/{store,unix}.rs`, `src/main.rs`
- `tests/pi_extension_workflow.rs`, `tests/pi_extension_asset.test.mjs`
- `tests/fixtures/pi_extension/{kitsune-agent-state.ts,KITSUNE_LICENSE}`
- [Ownership matrix](AGENT_MANAGEMENT_MIGRATION.md), [app boundaries](APP_BOUNDARIES.md)
- Optional sibling guide: [Kitsune Pi migration](../kitsune/PI_EXTENSION_MIGRATION.md)
