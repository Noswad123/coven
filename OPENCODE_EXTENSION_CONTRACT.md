# OpenCode reference extension contract

## Scope and ownership

Coven owns new Linux reference-plugin placement, Unix receipt capture, redacted
inventory and optional generic Kitsune presentation. This is **not full OpenCode
management completion**. Migration is an explicit **functional reduction from
working/blocked/idle lifecycle reporting to references only**, always `unknown`,
even fresh. Discovery/status interpretation/start/resume/restore remain pending.
References establish no activity, identity, readiness, completion or resume authority.

Apply uses the [shared Linux/macOS backend](HOOK_FILE_BACKEND_CONTRACT.md).
Explicit macOS `--apply` is source-enabled, provisional local APFS only, not
natively qualified; receiver/privacy and native activation remain unvalidated.
Other Unix platforms preview but refuse apply; Windows is unsupported.
Kit Linux new-install/recommend/
update/bulk paths retire without running Coven. Non-Linux legacy new-install paths,
the full v9 asset, legacy CLI/status/uninstall, all six `IntegrationTarget::ALL`
entries and old ingress/lifecycle remain compatibility until explicit migration.

## Public commands

Reference syntax only; inspect and select absolute paths manually:

```text
coven agent extensions plan --provider opencode --extension-root EXISTING_ABS_ROOT --executable ABS_COVEN --socket ABS_SOCKET --migrate-kitsune --json
coven agent extensions install --provider opencode --extension-root EXISTING_ABS_ROOT --executable ABS_COVEN --socket ABS_SOCKET --migrate-kitsune --apply --json
coven agent extensions uninstall --provider opencode --extension-root EXISTING_ABS_ROOT --json
coven agent extensions rollback --provider opencode --extension-root EXISTING_ABS_ROOT --json
coven agent report-extension --provider opencode --socket ABS_SOCKET --event session.created --json
```

All four placement operations accept executable/socket pins, migration consent,
`--apply` and `--json`. `plan` always previews; other operations preview unless
`--apply` is explicit. The root must already exist and be absolute. Native v1.19.5
accepts both `plugin` and `plugins` roots, including Kit's existing `plugins` root.
Select one: there is **no across-root deduplication or authority arbitration**.
Do not double-install in both roots. No JSON settings, directories or packages are
created; no SDK packages, probes, native execution, PATH discovery or automatic root
discovery are needed. First install requires executable/socket; plan without them
reports `planned_requires_executable_and_socket`. Later operations use journal pins;
supplied pins must match. Prepare the receiver's existing owned socket parent
separately with exact 0700 mode.

`agent report` remains native JSON ingress for **Claude, Codex and Copilot only**.
Pi and OpenCode use `report-extension`, with different contracts/event maps. OpenCode
stdin is bounded to 1 MiB, duplicate-free, depth-bounded JSON containing **exactly
three string fields**:

```json
{"contract":"coven.opencode-extension.v1","event":"session.created","session_id":"ses_example"}
```

Only exact `session.created` or `session.updated` is accepted. The required request
`--event` **must equal the payload event**; it cannot fill a missing payload event.
Unknown fields, aliases, missing fields, wrong types/contracts/events refuse. IDs
are nonblank, at most 512 UTF-8 bytes, contain no controls and cannot begin with a
hyphen after leading whitespace. The CLI envelope does not claim native DTO proof.
It constructs a new RPC object containing only `provider`, `event`, `session_id`.
Provider + event maps a **contract interpretation, not authenticated origin**;
local callers can forge claims. See the [receiver contract](AGENT_MANAGEMENT_CONTRACT.md)
for acknowledgement uncertainty and redaction. Never automatically resend.

## Authoritative native source

The authoritative release is canonical
[`anomalyco/opencode` v1.19.5, commit `810c0eba1a7348036f8bdf61e6632d0c36c6876c`](https://github.com/anomalyco/opencode/tree/810c0eba1a7348036f8bdf61e6632d0c36c6876c).
Evidence is `packages/plugin/src/index.ts`, `packages/schema/src/v1/session.ts`
and `packages/opencode/src/plugin/index.ts` at that revision. The changed current
development API at `611cee9` is **unsupported**. Source/API inspection and fixtures
are not installed-native operational certification; none is claimed.

The generated ESM has one named async export, `CovenAgentReferencePlugin`, returning
async `event` and `dispose` hooks. Native plugins execute in the **server's exact
directory instance**, not the whole project and not a selected TUI, pane or attach
scope. That instance may observe multiple roots. A remote server is possible:
the pinned executable and socket must exist and be usable **on that server**,
not merely on the local TUI client.

Capture accepts `session.created`/`session.updated` only when the full session DTO
is at `event.properties.info`, with `info.id === properties.sessionID` and
`parentID` undefined/omitted only. Null, malformed DTOs and child sessions reject.
There is no parent cache, SDK read or enumeration. A resumed quiet session is not
guaranteed to emit either event; this is not universal resume capture.

Validation is a **conservative bounded subset**, not acceptance of every native
schema-valid DTO. Required strings are slug/projectID/directory/title/version;
the ID must have the `ses` prefix and satisfy producer bounds. Created/updated and
optional compacting timestamps must be nonnegative safe integers; optional archived
time is finite. Optional summary, cost, tokens/cache, share, workspace/path, agent,
model, metadata object, permission and revert fields are shape-checked when present.
Both `summary.diffs` and `permission` have a **256-entry maximum**; larger native
arrays are intentionally not reported. Opaque metadata values and extra keys are
not enumerated or serialized. Queue capacity is checked **before DTO access and
validation**, avoiding expensive DTO work when full.

## Generated producer

`coven-agent-reference.js` is distinct from `kitsune-agent-state.js`. Generation
deterministically prefixes JSON-literal executable/socket constants to the fixed
template; it uses no SDK packages or placeholder substitution. It imports only
Node `child_process.spawn`. ID and event are snapshotted before asynchronous child
work; no prompt, path, cwd, DTO or context is serialized.

Each factory has a FIFO of **32 pending plus one active** report. Full queues drop
the incoming event; there is no persistent queue, deduplication or retry. Explicit
executable/socket pins become spawn argv with `shell: false`, minimal envelope
stdin and ignored stdout/stderr. Failures are best effort. A two-second timer
requests `SIGKILL`; only child `close` releases the active slot, not `error` or a
kill request. `dispose` permanently stops the factory, clears pending snapshots,
and requests termination of its owned child. It returns boundedly without waiting
for close and **does not guarantee child exit**. Lost events are not replayed.

## Storage and upgrade order

All **five providers** share store `schema_version: 1`, unchanged record fields
`provider`, `session_id`, `last_event`, `timestamp_ms`, `server_epoch`, and an
**aggregate 128-record cap**, not per-provider capacity. IDs hash provider + NUL +
exact SID; existing hashes stay stable. Full IDs stay private. OpenCode is unknown
even fresh; after receiver restart all providers are unknown with null age until
another receipt. Upgrade the receiver first: older binaries reject the new provider
on wire and in store despite unchanged schema 1. Stored OpenCode records limit
downgrade; no automatic conversion, deletion or reset is provided.

## Migration and recovery

`--migrate-kitsune` authorizes removal only of exact full-v9 bytes with SHA-256:

```text
7782f028d172b7b56b95aa364f443b8929446a40ce19a505e55aa8d8e405312b
```

Filename/marker is insufficient; modified assets refuse. Do not uninstall Kit
first if migration needs its preimage. The private fixed-two-leaf v1 engine is
shared with Pi, but OpenCode has its own schema `coven-opencode-extension`, journal
`coven-opencode-extension.json`, lock `.coven-opencode-extension.lock`, recovery
and error namespaces. No journal-controlled paths are opened. Pi's public API,
v1 receipt protocol and generated bytes from `887451b` remain frozen and protected
by [frozen compatibility tests](tests/pi_extension_frozen_compatibility.rs) and
[independent fixtures](tests/fixtures/pi_extension_887451b/).

Both leaves and the fully reconstructed journal validate **before any apply
artifact**, then reprepare under lock. Leaves are bounded to 1 MiB, journal to
8 MiB; new extension/journal files are 0600. Validation checks original new-leaf
absence, exact legacy bytes/SHA/mode, generated installed image and transition
images. Original preimages remain immutable across reinstall, including after
uninstall. Install publishes pending journal, writes new first, deletes verified
old second, then finalizes. Rollback restores legacy first, deletes new second,
including rollback after uninstall. Normal uninstall never revives Kit.

Pending recovery accepts only a legal prefix of that pending operation's fixed
order; plan can inspect it, but only the same operation may resume. Preserve
conflicting journal/lock/recovery artifacts. Descriptor-relative cooperating-local-
filesystem fences are **not global CAS or atomic activation**.

**Stop native OpenCode servers and watchers before apply**, including uninstall or
rollback. After manually chosen apply, restart the **native server**, not just a
TUI/pane/attach client. No automatic restart, autoload, unload or in-memory legacy
authority release occurs. Intermediate two-leaf states can otherwise be observed.

## Validation

The [current scoped table](AGENT_MANAGEMENT_MIGRATION.md#opencode-validation-table)
records **236 focused Rust checks**, including 20 shared-engine checks and 14
Pi/OpenCode workflow checks, plus 12 generated Node producer checks with the harness
also run under Bun. Kit reports 111 integration
and 10 OpenCode checks (overlapping filters), plus six legacy-asset harness checks.
Strict lint, Rust 1.85 all-target checking, optimized build, package verification
and packaged test-target compilation passed. See that table for the recorded
presentation-acknowledgement failures and effect-verifying fixture correction.
Full-suite/native/cross-platform certification remains unclaimed. Historical Pi
212-check evidence is not additive current validation.

## Related files

- `src/opencode_extension.rs`, `src/opencode_extension/{asset.rs,template.js}`
- `src/extension_assets.rs`, `src/extension_assets/engine.rs`
- `src/agent_management.rs`, `src/agent_management/{store,unix}.rs`, `src/main.rs`
- `tests/opencode_extension_workflow.rs`, `tests/opencode_extension_asset.test.mjs`
- `tests/fixtures/opencode_extension/{kitsune-agent-state.js,KITSUNE_LICENSE}`
- [Ownership matrix](AGENT_MANAGEMENT_MIGRATION.md), [Pi contract](PI_EXTENSION_CONTRACT.md)
- Optional sibling [Kit migration guide](../kitsune/OPENCODE_EXTENSION_MIGRATION.md)
