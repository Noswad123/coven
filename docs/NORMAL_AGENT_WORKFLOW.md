# Normal agent workflow

Implementation contract for the normal-invocation ownership cut. This document is
updated alongside implementation; native macOS qualification remains outstanding.

## Overview

Users invoke native agents normally. No Coven launcher, group enrollment or manual
live-artifact attachment is required. A one-time explicit Kitsune startup
configuration selects a generic integration runtime. Presentation-only
`integrations.sources` does not execute programs or change status ownership.

After that opt-in and explicit native hook/extension configuration, users still
type `djinn`, `codex`, `copilot`, etc. unchanged. Coven supplies attributed plain
rows under the explicitly selected **Coven** integration source, not the old Kit
agent-status widgets, pane-focus/sidebar agent model or mobile completion cues.
Sessions remain unbound: no PID association or authenticated native attestation.

## API / Interfaces

The optional `[integrations.runtime]` configuration has `source`, literal argv
`command`, literal argv `report_command`, and `external_agent_management` fields.
Both commands require absolute executables. The source must be explicitly enabled.
The runtime configuration is startup-only: changing it requires a server restart.
The helper starts after API readiness, owns piped stdin, and exits on stdin EOF.
Kitsune closes stdin, waits a bounded interval, then kills/reaps if necessary.
There is no shell evaluation, automatic restart or legacy fallback on failure.

New eligible panes receive `KITSUNE_INTEGRATION_SOURCE`, JSON-encoded literal argv
`KITSUNE_INTEGRATION_REPORT_COMMAND`, and (only for external ownership)
`KITSUNE_AGENT_MANAGEMENT=external`. Disabled mode strips inherited route variables.
Activation requires a server restart, new panes, and reloading native providers
that already loaded their plugins/hooks. Existing processes are not retrofitted.

In external mode Kit does not accept legacy agent-status authority, run agent
detectors, or execute agent-aware restore policy. Historical resume references
are not fresh status and must not cause automatic execution.
This suppression covers all legacy agent-status and automatic-resume paths even
if helper startup/reporting fails. A requested external ownership mode survives
rejection of its runtime route; an undecodable runtime ownership request,
malformed startup TOML or unreadable startup config also retains external ownership
fail-closed, without authorizing legacy restore. A missing config uses ordinary defaults.
The generic helper is server-only; `--no-session` refuses runtime/external mode.

## Data Flow

`coven agent run WORKSPACE --socket ABS --kitsune-socket ABS --source SOURCE`
is an attached receiver/presenter. Unlike bounded `agent serve`, it runs until
stdin EOF or interruption. The socket parent must already be private and the
workspace explicitly initialized. Socket conflicts are never unlinked/adopted.

The existing five-provider `coven.agent-management.v1` receipt path and persisted
schema remain compatible. Its interpretation stays caller-supplied/unverified.
The normal view adds evidence provenance and monotonic freshness with a 30-second
validity ceiling, independent of the five-second presentation lease. Repainting
does not refresh evidence. Reference-only providers remain unknown. Restarted
receipts are historical; native event observations are bounded and volatile.
Native IDs, prompts, tools, paths and raw payloads are not generic-view fields.

Existing hook commands remain pinned to their explicitly selected configuration
roots, executable and `--socket`; those socket pins must match this helper's
receiver socket. The pane reporter environment does not rewrite installed hooks.
Hook/extension installers remain explicit, preview-first operations, never automatic
installation or an override of native allowed/disable/trust/approval settings
(Codex's explicitly consented shared hooks flag is documented separately).
Codex/Copilot startup references and Pi/OpenCode reference events remain **unknown
even when configured and fresh**. Claude activity is only last-reported evidence,
bounded to 30 seconds in this normal view; legacy v1 inventory/serve is unchanged.

## Djinn native capture boundary

Ordinary Djinn invocation uses the native legacy full-TUI path. Only its source
plugin has changed: use an updated UI build containing the reporter. Updating the
Rust CLI alone is insufficient if `DJINN_UI_BIN`, a saved runtime binding or the
wrapper's `DJINN_UI_TARGET` still selects an old UI build. Restart/reload that
provider in a new eligible pane. Mini, external attach, remote and v2 paths are
not qualified by this cut.

The configured reporter command is typically literal argv
`[ABS_COVEN, "agent", "report-native", "--socket", ABS]`. Djinn spawns it without
a shell and writes only the following closed envelope to stdin:

```json
{"contract":"djinn.native-events.v1","instance_id":"UUID","session_id":"NATIVE_ID","sequence":1,"event":"session.status","status":"busy"}
```

The five base fields are `contract`, `instance_id`, `session_id`, `sequence`, and
`event`. `instance_id` is a canonical UUID, and sequence is a positive JavaScript
safe integer scoped to that plugin instance. Allowed events are:

- `session.created`, `session.updated`, `session.deleted`: exactly base fields;
- `session.status`: base fields plus `status`, one of `busy`, `retry`, `idle`;
- `permission.asked`, `permission.replied`, `permission.cancelled`,
  `question.asked`, `question.replied`, `question.rejected`, `question.cancelled`:
  base fields plus bounded nonempty `request_id`. Cancellation is emitted only
  for an actual native cancellation, never inferred from idle or transport failure.

Only independently established root sessions are eligible. Child/sessionless
events cannot alter root status. Emitter/sequence/queue/root tracking is per
plugin instance. Queue size and child lifetime are bounded, stale queued reports
are dropped, and delivery is never retried or replayed as fresh evidence.
Transport failure never affects native agent execution or invokes legacy fallback.

Coven validates the envelope before using its private client. Native RPC uses
`coven.native-events.v1`, method `report`, and exactly the same allowlisted params
without `contract`. Its response uses the existing id/type/result-or-error framing
with the native RPC contract. Acceptance is volatile, not a durability claim.
Existing v1 inventory/receipts are not reinterpreted as native events.

Coven maps native busy/retry to last-reported working, idle to last-reported idle,
pending native decisions to blocked, and deletion to ended. Duplicate/out-of-order
sequences do not refresh evidence. These are neither task success nor readiness,
and no pane binding or authenticated native origin is claimed. Multiple sessions
remain separate observations.

### Receiver freshness and retention

Normal native observations identify the provider as `djinn`, not `opencode`.
This does not add Djinn to the legacy five-provider v1 allowlist. Root eligibility
is declared `producer_checked`; the caller and native origin remain unverified,
not authenticated by Coven.

Status and each `(permission|question, request_id)` pending decision have their own
monotonic 30-second clocks. Metadata (`session.created`, including repetitions,
and `session.updated`) never renews a status or pending decision. Repeated asks
for an already-pending request do not renew its clock. Replies, rejections and
cancellations remove only the matching pending kind/ID; unmatched removals do
nothing. Busy/idle reports do not renew decision clocks. Once no fresh pending
decision remains, only an independently fresh status can establish working/idle.
Unknown evidence is not task completion, success or readiness.

An instance sequence gap invalidates retained status and pending decisions for
every session in that instance before applying only the incoming independent
fact. Duplicate/out-of-order sequences do not renew evidence or high-water
retention. Deletion establishes a terminal tombstone: while retained, higher
sequence created/updated/status/decision events for the same instance/SID are
rejected without renewing the terminal fact. Repeated deletion is accepted but
does not renew its clock. Valid higher sequences are consumed even if the event
is refused for a retained tombstone or active capacity.

Native storage is independent of the 128-record persisted v1 store. It retains
at most 128 native observation rows, 128 pending decisions per observation,
512 instance high-water marks and 512 terminal tombstones. On observation
pressure, the oldest first-observed ended, expired or fact-less observation is
retired; 128 genuinely active observations cause capacity refusal until facts
expire or end. Deletion still retains its tombstone when no display slot can be
reclaimed. Instance high-water marks expire 10 minutes after the last consumed
sequence, and tombstones expire 10 minutes after the first deletion. Each table
also evicts its oldest entry on count pressure. Forgetting a high-water mark
invalidates derived facts for that instance; forgetting a tombstone retires any
remaining ended observation for that tuple. Expiry is collected on reports.
All native state is lost on restart. Replay/revival protection is consequently
bounded by retention and restart, not unlimited.

`coven.normal-observations.v1` includes `omitted_observations` and the cumulative
`retired_native_observations` count. The view prioritizes fresh evidence, then
native evidence, with stable observation-ID ordering. If rows are omitted or
native history has been retired, at most 127 observation rows plus one explicit
limits/omissions summary row are shown. Native facts without a retained clock
have `freshness: "unknown"`; expired clocks yield unknown state. Historical v1
receipts remain distinct and the v1 stored bytes, inventory and serve view are
unchanged. Parents integrating Djinn must include both cancelled event names in
their closed-envelope contract and validate actual native cancellation delivery.

## Edge Cases

- Helper degradation leaves terminals usable and legacy authority suppressed;
  accepted UI rows disappear when their independent five-second lease expires.
  A live presenter cannot renew the 30-second evidence clocks by repainting.
- There is no automatic helper restart. Inspect failure/socket ownership, repair
  deliberately, then restart the Kit server. Never unlink/adopt a conflicting
  socket to force startup. Concurrent runtimes each need their own Coven workspace
  and receiver socket (one receiver lease per workspace), and the correct Kit API
  socket. A copied fixed socket path is not safe multi-runtime configuration.
- Rollback is a deliberate ownership handoff: stop affected providers/helper,
  preserve/inspect config preimages, explicitly revert runtime ownership and any
  provider configuration using its guarded rollback contract, then restart Kit
  and create new panes/reload providers. Removing rows, helper failure or live
  config reload never hands authority back. Historical references do not resume
  automatically in external mode; ordinary legacy policy returns only after the
  explicit restart handoff.

## Example usage

Copyable TOML shape below: replace the example **literal absolute paths** with
inspected paths for your machine before use. `command` is
`[ABS_COVEN, "agent", "run", WORKSPACE, "--socket", ABS_COV_SOCKET,
"--kitsune-socket", ABS_KIT_API_SOCKET, "--source", "coven"]`; `report_command`
uses that same ABS_COVEN and ABS_COV_SOCKET. No `~`, `$HOME`, environment-variable,
PATH or shell expansion occurs.

**Manual prerequisites:** explicitly initialize a dedicated Coven workspace
outside every application repository, such as `/home/operator/agent-state/normal`,
and separately create/inspect an owned socket parent with exact **0700** mode,
such as `/home/operator/agent-sockets/normal`. Neither `agent run` nor this config
initializes the workspace or creates that parent. Select the actual Kit **API**
socket, not its client socket. Inspect and preserve the existing installed user
config and provider settings; merge deliberately, do not overwrite them with this
snippet. This documentation update does not install anything or change user config.

```toml
[integrations]
sources = ["coven"]

[integrations.runtime]
source = "coven"
command = ["/home/operator/bin/coven", "agent", "run", "/home/operator/agent-state/normal", "--socket", "/home/operator/agent-sockets/normal/coven.sock", "--kitsune-socket", "/home/operator/kit-runtime/normal/api.sock", "--source", "coven"]
report_command = ["/home/operator/bin/coven", "agent", "report-native", "--socket", "/home/operator/agent-sockets/normal/coven.sock"]
external_agent_management = true
```

Retain other enabled sources/settings as needed, but do not run `group present`,
discovery presentation or another publisher against this same `coven` lease.
After manual opt-in, restart a compatible Kit server (omit `--no-session`), open
new panes and reload/restart providers whose plugins/hooks were already loaded.
Select **Coven** in Integrations and invoke native agents normally. The runtime
change is startup-only; a config reload cannot activate or roll it back.

## Acceptance gates

Normal invocation must deliver fresh attributed reports with no Coven launcher.
Evidence must expire while the presenter remains alive. Killing the helper must
expire the UI lease without reviving legacy authority. Disabled/default mode,
nested environment isolation, child cleanup, restore references, private endpoint
admission, provider reference-only behavior and native event bounds remain tested.
Groups, launch orchestration, broader provider lifecycle support, and native Mac
runtime certification are separate follow-up work.

## Related files

- [Receipt API and unchanged v1 inventory](AGENT_MANAGEMENT_CONTRACT.md),
  [ownership/migration](AGENT_MANAGEMENT_MIGRATION.md).
- `src/main.rs`, `src/agent_management.rs`, `src/agent_management/{native,unix}.rs`,
  `src/agent_management_presentation.rs`.
- Kitsune: `src/config/{model,io}.rs`, `src/server/integration_runtime.rs`,
  `src/server/headless.rs`, `src/integration/env.rs`, `src/persist/restore.rs`.
- Djinn: `clients/djinn-ui/packages/opencode/src/plugin/kitsune-agent-state.ts`
  and `src/coven/native-event-reporter.ts` under that same package.
