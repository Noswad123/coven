# Agent receipt management contract

## Overview

Coven owns an independent, foreground Claude/Codex/Copilot native-hook and Pi/OpenCode
extension-envelope receiver and redacted inventory.
Transport is Unix-only (Windows unsupported); macOS operation has not been validated.
This is distinct from the Linux-only hook mutation/Kit installer retirement cut;
Unix transport alone is not a matching non-Linux configuration adapter.
Contract: `coven.agent-management.v1`. These are **caller-supplied, unverified
receipts**, not an authenticated native-session registry or task authority. See
[the migration matrix](AGENT_MANAGEMENT_MIGRATION.md) for the all-provider target
and the still-pending discovery/start/resume/restore transfers.

## API / Interfaces

```text
coven agent serve WORKSPACE --socket ABS_COVEN_SOCKET --duration 60 [--kitsune-socket ABS_KITSUNE_SOCKET --source coven] [--json]
coven agent report --provider claude --socket ABS_COVEN_SOCKET [--event EVENT] [--json]
coven agent report --provider codex --socket ABS_COVEN_SOCKET [--event SessionStart] [--json]
coven agent report --provider copilot --socket ABS_COVEN_SOCKET [--event SessionStart] [--json]
coven agent report-extension --provider pi --socket ABS_COVEN_SOCKET --event session_start [--json]
coven agent report-extension --provider opencode --socket ABS_COVEN_SOCKET --event session.created [--json]
coven agent inventory --socket ABS_COVEN_SOCKET [--json]
```

- `serve`: duration defaults to 60 seconds; accepted range 1–3600. Explicit existing
  workspace with a regular `coven.json` marker, not a complete task/configuration
  authority check. Core startup never launches a provider or daemon.
- `report`: native JSON on stdin, at most **1 MiB**, depth bounded and typed-decoded.
  Requires `session_id` and `hook_event_name`. Session ID is nonblank, at most 512
  UTF-8 bytes, with no controls or leading hyphen after leading whitespace.
  Optional `--event` must match the payload event.
- Claude main-session events (unchanged): `SessionStart`, `UserPromptSubmit`, `PreToolUse`,
  `PostToolUse`, `Stop`, `SessionEnd`. `SubagentStop`, or a nonempty string
  `agent_id`, is ignored without sending a report. Non-null, non-string `agent_id` is invalid.
- Codex accepts **only `SessionStart`** in domain validation, even though the
  shared CLI event parser also lists Claude events. It captures a reference, never
  working/idle/ended lifecycle. No `thread_id`/`thread-id` session alias exists.
  Native SessionStart has seven required fields; Coven intentionally requires only
  session ID/event and ignores/drops the rest before RPC. See the pinned native
  schema and complete native example in [the Codex contract](CODEX_HOOKS_CONTRACT.md#data-flow).
- No implicit HOME/config roots, provider executable discovery, PATH lookup,
  probes, native-store inspection or automatic adoption.

Copilot also accepts **only exact `SessionStart`**, requiring `session_id` and
`hook_event_name`; no `sessionId`, camelCase aliases or inferred events. `--event`
cannot supply a missing event. Unknown fields including `initial_prompt`, `cwd`,
native `timestamp`/`source` and agent metadata are dropped before RPC. Claude's
subagent suppression does **not** apply to Copilot; `agent_id` is ignored regardless
of type. The [Copilot contract](COPILOT_HOOKS_CONTRACT.md) pins native documentation,
not an implementation/binary or operational certification.

Library entry points use `ServeRequest { workspace, socket, duration }`,
`ReportRequest { socket, provider, event }`, `InventoryRequest { socket }` and
`serve_arguments`, `report_arguments`/`report_payload`, `inventory_arguments`,
returning `Result<TaskOutput>`. `TaskOutput` carries stdout, warnings and exit code.
The optional presentation entry point additionally takes
`PresentationRequest { socket, source }`.

Pi uses separate `extension_report_arguments`/`extension_report_payload`, not
`report` (which still accepts only the three native-hook providers). Stdin must
contain exactly `{"contract":"coven.pi-extension.v1","event":"session_start","session_id":"ID"}`.
All three fields and required `--event session_start` must match; unknown fields
refuse rather than being dropped. The same payload/ID bounds apply. This is a
Coven-owned envelope, not Pi-native hook JSON. Provider `pi` and event
`session_start` map the `getSessionId` interpretation, not authentication. See
[Pi extension contract](PI_EXTENSION_CONTRACT.md) for producer and placement limits.

OpenCode uses the same separate `extension_report_arguments`/`extension_report_payload`
entry points but requires exactly three string fields
`{"contract":"coven.opencode-extension.v1","event":"session.created","session_id":"ID"}`,
with exact event `session.created` or `session.updated`. Required request `--event`
must equal the payload event and cannot supply a missing one. Unknown fields,
aliases and wrong types refuse. Provider/event maps contract interpretation, not
authenticated origin. The [OpenCode contract](OPENCODE_EXTENSION_CONTRACT.md) pins
release v1.19.5 and documents server-instance scope, conservative full-DTO producer
validation, 32-pending-plus-one-active FIFO, and placement/restart limits. Native
`agent report` still accepts only Claude/Codex/Copilot; extension contracts remain
separate from native hook JSON and from each other.

### Private wire and output

The Unix stream uses bounded newline-delimited requests with exactly `id`,
`contract`, `method`, `params`; correlation IDs are 32 hex characters. Methods are
only `report` and `inventory`. Report params contain exactly `provider`, `event`,
`session_id`; inventory params are empty. Requests are bounded to 8192 bytes;
receiver socket reads/writes use a deadline of at most 200 ms within the serve
deadline. Storage setup/synchronization is not a hard wall-clock bound. Client calls use a
one-second deadline and at most 256 KiB reply budget, with endpoint fences and
closed result validation before stdout.

`report --json` returns `type: accepted`, a 64-hex `record_id`, and delivery
`committed` or `durability_uncertain`; ignored subagents return `type: ignored`,
`reason: subagent`. Human accepted output is `Accepted RECORD_ID`.
Inventory returns `contract`, `server_epoch`, `authorization: none`, and `records`.
Each record contains only:

```text
record_id, provider, last_event, reported_state, age_ms,
reference_trust: caller_supplied_unverified,
native_identity: unestablished, job_completion: unestablished
```

The record ID is SHA-256 of provider (`claude`, `codex`, `copilot`, `pi` or `opencode`), NUL, then the exact
session ID. Existing Claude hashes are unchanged; equal session IDs from different
providers yield distinct records. It is a stable pseudonymous reference, not
authentication or anonymization against guessing.
Native IDs are omitted from inventory, acknowledgements and optional UI rows.
Default inventory/serve output is pretty JSON; `--json` is compact. The CLI adds
a terminal LF. Graceful duration expiry or Ctrl-C returns exit **0** with
`type: stopped`; this route does not promise the watch command's exit-130 policy.

## Data Flow

1. Decode and validate the native root payload or strict Pi/OpenCode envelope, then construct a **new** minimal
   object. Prompt, transcript, cwd and all unknown fields are dropped **before
   transport, storage or stdout**; Claude subagent events never revive the main record.
2. Validate the private endpoint and send one correlated request. No automatic retry.
3. Receiver takes the ordinary workspace guard for each update, rereads and validates
   the store, then atomically replaces it. It releases that guard between reports.
4. Persist `state/agent-management.json` with unchanged `schema_version: 1` as private
   **0600**, at most **128 records aggregate across five providers** (not per provider)
   and **256 KiB**. Private records retain full native IDs, last events, receipt
   timestamps and receiver epochs. Capacity exhaustion refuses a new record; no eviction
   or pruning API is supplied. Stored record fields remain `provider`, `session_id`,
   `last_event`, `timestamp_ms`, `server_epoch`; no schema migration is required.
5. Publish a redacted projection using receipt-local monotonic ages, not native
   event timestamps. Receipt order wins; delayed/spoofed reports are possible.

| Claude last event received in this receiver epoch | Reported state |
| --- | --- |
| `SessionStart` | `unknown` |
| `UserPromptSubmit`, `PreToolUse`, `PostToolUse` | `working` |
| `Stop` | `idle` |
| `SessionEnd` | `ended` |
| Persisted record after restart, until a new receipt | `unknown`, `age_ms: null` |

These states are **last reported**, not current jobs/readiness/identity/completion.
There is no freshness threshold turning them into live guarantees.
Codex, Copilot, Pi and OpenCode records are always **unknown**, even when fresh with a known receipt age.
After restart **all providers'** persisted records have unknown state and null age
until another receipt; a new Codex/Copilot/Pi/OpenCode receipt still does not establish activity.

Upgrade receivers before new Pi/OpenCode producers. Older binaries reject newly
supported providers' wire/store records despite unchanged schema-v1 fields; once
those records are stored, downgrade to those
readers is unsupported without separately preserving/removing incompatible state.
There is no automatic downgrade conversion or reset.

### Receiver lifetime and optional presentation

Unix-only transport requires an existing owned socket parent with **exact 0700**
mode and a private **0600** socket. Aliased/unsafe paths are refused. Bind never
unlinks an existing leaf, even a stale socket. Parent/socket inode fences detect
replacement; cleanup removes only the still-owned endpoint. A SIGINT guard is
installed **before bind**, including startup error cleanup. SIGTERM/SIGKILL cleanup
is not promised: verify no live owner before manually removing a stale socket.

A separate persistent `state/.agent-management.receiver.lock` lease admits only
one receiver per workspace, even on different sockets. Its 0600 inode stays after
exit and is protected from generic replacement by its reserved basename; never
remove/replace it to bypass a live receiver. Startup synchronizes `state/` and the
workspace root before admitting report commits. State/workspace-lock/lease identity
is fenced during service.

Optional `--kitsune-socket` and `--source` must be supplied together. An independent
worker waits for initialized receiver projection before UI IO. It reads only generic
snapshot discovery and uses `integration.view.open/replace/poll/close`, not Kitsune
agent APIs. It sends bounded last-reported label/detail rows with dynamic provider
labels for Claude, Codex, Copilot, Pi and OpenCode; Kitsune gains no provider-specific knowledge. Selecting one adds
read-only trust details. No pane binding, launch, resume, adoption or role follows.
Unavailable/failed Kitsune presentation does not stop core capture; the final report
adds a separate `presentation` outcome. Configure a dedicated generic source
manually and avoid another publisher contending for its lease. Uncertain UI writes
are not replayed; lease expiry is the fallback cleanup.

## Edge Cases

- From first send onward, missing/invalid acknowledgements can be
  `agent_delivery_uncertain`: receipt may already be persisted. Do not retry
  automatically. `durability_uncertain` also means replacement occurred, not
  permission to retry or roll back. Peer error bodies are not echoed.
- Local socket ownership is not native producer authentication. Anyone able to
  call the selected endpoint can make unverified claims. Private IDs never grant
  resume authority or terminal association.
- The workspace marker admits storage, not task execution, readiness or approval.
  Existing task/launch/checkpoint contracts remain independent.
- Receiver availability is optional for installed best-effort hooks; offline
  failure is suppressed by the generated shell command, not turned into native
  provider failure. Bound socket existence alone is not receiver readiness.
- Pi instead uses a best-effort Node child with a two-second kill request, no shell,
  queue or retry; overlaps drop and the slot remains held until child close.
- OpenCode uses a best-effort FIFO of 32 pending plus one active Node child, drops
  incoming events when full, and never deduplicates or retries. Capacity precedes
  DTO work; dispose clears pending work and requests kill without guaranteeing exit.

## Example usage

Reference syntax; select inspected absolute paths and prepare the existing owned
0700 socket parent separately. This starts only Coven's foreground receiver:

```text
coven agent serve WORKSPACE --socket ABS_COVEN_SOCKET --duration 60 --json
coven agent inventory --socket ABS_COVEN_SOCKET --json
```

In a separate invocation, `agent report --provider claude --socket ABS_COVEN_SOCKET
--event Stop --json` accepts stdin such as
`{"session_id":"example-unverified-id","hook_event_name":"Stop"}`. This writes
receipt state, not native status. Add the paired Kitsune options to `serve` only
for optional generic presentation; core inventory requires no Kitsune installation.

## Related files

- `src/agent_management.rs`, `src/agent_management/{store,unix}.rs`
- `src/agent_management_presentation.rs`, `src/main.rs`
- `src/{read_fs,strict_json,session_support,observation_socket}.rs`,
  `src/storage/replacement.rs`, `src/group_presenter/wire.rs`
- `tests/{agent_management_workflow,agent_management_presentation,claude_agent_migration_workflow}.rs`
- [Claude hooks](CLAUDE_HOOKS_CONTRACT.md), [Codex hooks](CODEX_HOOKS_CONTRACT.md), [migration ownership](AGENT_MANAGEMENT_MIGRATION.md)
- [Copilot hooks](COPILOT_HOOKS_CONTRACT.md), `tests/copilot_agent_migration_workflow.rs`
- [OpenCode extension](OPENCODE_EXTENSION_CONTRACT.md), [Pi extension](PI_EXTENSION_CONTRACT.md)
