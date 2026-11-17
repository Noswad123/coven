# Normal agent workflow

Implementation contract for the normal-invocation ownership cut. This document is
updated alongside implementation; native macOS qualification remains outstanding.

## Overview

Users invoke native agents normally. No Coven launcher, group enrollment or manual
live-artifact attachment is required. With compatible Kitsune/Coven builds, adding
`"coven"` to `integrations.sources` selects the default runtime and external agent
ownership at server startup. Kitsune negotiates capabilities through
`coven doctor status --json`, retains the selected native executable/default root,
and lets Coven privately bootstrap state. Unknown source IDs remain
presentation-only; selecting a source in the UI never launches a process.

After that opt-in and explicit native hook/extension configuration, users still
type `djinn`, `codex`, `copilot`, etc. unchanged. Coven supplies attributed plain
rows under the explicitly selected **Coven** integration source, not the old Kit
agent-status widgets, pane-focus/sidebar agent model or mobile completion cues.
No PID association or authenticated native attestation follows.
[Checked CLI navigation](AGENT_NAVIGATION_CONTRACT.md) permits source-qualified
local Djinn selected-root producer associations as well as unchanged temporary
explicit user assertions. Producer display attachment is not execution ownership.
See the [approved adoption record](FEATURE_ADOPTION_STATUS.md) for current scope.

## API / Interfaces

The normal configuration is `[integrations] sources = ["coven"]`; see the
[doctor compatibility contract](DOCTOR_STATUS_CONTRACT.md). The optional
`[integrations.coven] auto_start = false` disables the implicit runtime and
inferred ownership for presentation-only/manual publishers. Changes require a
server restart. This is a deliberate change from the older presentation-only
meaning of a source-only Coven configuration.

The advanced managed setup interface remains `coven integration plan|enable|disable`:
`plan` is always read-only; enable/disable preview unless `--apply` is explicit.
See [setup defaults, ownership and recovery](INTEGRATION_SETUP_CONTRACT.md).
Optional absolute `--kitsune-config`, `--executable` and `--state-root` overrides
are advanced choices, not normal setup requirements.

Managed plans/undo reuse recorded executable/state-root pins when omitted;
explicitly different pins refuse. An active plan can warn with `runtime_ready: false`
about an unhealthy recorded executable/root without losing the config
description. Enable requires read-only root/ACL/creation-access preflight before
config changes; this does not establish live helper or provider readiness.

The advanced/internal `[integrations.runtime]` configuration has `source`, literal argv
`command`, literal argv `report_command`, and `external_agent_management` fields.
Both commands require absolute executables. The source must be explicitly enabled.
An explicit runtime block wins as a whole over the Coven defaults, including
`auto_start = false`; invalid explicit configuration never falls back to defaults.
Existing managed configurations/receipts are not rewritten or adopted by discovery.
The runtime configuration is startup-only: changing it requires a server restart.
The helper starts after API readiness, owns piped stdin, and exits on stdin EOF.
Kitsune closes stdin, waits a bounded interval, then kills/reaps if necessary.
There is no shell evaluation, automatic restart or legacy fallback on failure.

New eligible panes receive `KITSUNE_INTEGRATION_SOURCE`, JSON-encoded literal argv
`KITSUNE_INTEGRATION_REPORT_COMMAND`, and (only for external ownership)
`KITSUNE_AGENT_MANAGEMENT=external`. Disabled mode strips inherited route variables.
Activation requires a server restart, new panes, and reloading native providers
that already loaded their plugins/hooks. Existing processes are not retrofitted.

Kit's live agent detector/consumer and automatic provider-resume paths are now
deleted globally, not merely suppressed in external mode. Historical references,
names/argv and history are data only, not readiness or execution consent. The
generic 100 ms maintenance tick, OSC titles/CWD/clipboard, terminal UUID, actual
child wait/import EOF, manual labels and explicit notifications remain without
agent meaning. Idle-inferred finished sounds/toasts and mobile agent counts are
removed. A requested external ownership mode survives
rejection of its runtime route; an undecodable runtime ownership request,
malformed startup TOML or unreadable startup config also retains external ownership
fail-closed, without authorizing legacy restore. A missing config uses ordinary defaults.
The generic helper is server-only; `--no-session` refuses runtime/external mode.

All 12 live Kitsune `agent.*` methods now return correlated `feature_retired` in
both ownership modes before target/input validation, lookup or housekeeping;
convenience and raw API CLI fail locally before socket/protocol/native execution.
They are not aliases for Coven observations. AgentStatus subscriptions and agent
wait matchers retire, with mixed generic/agent requests rejected atomically before
patterns/probes/streams. Generic output/scroll/workspace subscriptions, event
streams and `pane.wait_for_output` remain; previously unsupported generic
`events.wait` matchers are still unsupported.

The four legacy pane hook methods retain `external_agent_management` precedence
externally, but return `feature_retired` standalone. Generic metadata reporting is
token-only; any retired presentation field mixed with tokens rejects the whole
request, without partial writes. Snapshots expose `agents: []` in both modes,
preserving historical pane references for conservative recovery skip vetoes.
Handoff primary-screen history is preserved independently of those references;
alternate-screen behavior follows generic policy. Optional JSON snapshot
`respawn_shell_on_exit` captures actual generic shell policy: import honors only
true without argv/FD inference; cold restore is false even if saved true. Binary
DTOs/client protocol remain 18. Launch metadata never authorizes provider restore.

Fresh legacy installation for all six Kit targets on all platforms returns
`feature_retired` before discovery/version probes/writes; recommendation/update
policy is deleted. Status/uninstall and frozen assets/fingerprints/licenses remain
for explicit cleanup, never automatic uninstall. Generic Integrations sources/
leases/views are separate. Coven macOS/Windows replacements are not thereby
qualified. Old remote files/config are ignored with diagnostics and untouched;
old resume/CJK-agent config is likewise inert even with wrong legacy field types.
Explicit generic CJK reveal remains. Offline static local/bundled evaluation and
`agent explain --file` plus old DTO decoding remain compatibility maintenance only.
Windows legacy-agent-aware process acquisition remains a separate known contract
rejected by Coven, not a lifecycle producer. No runtime agent-manager fallback
exists. External ownership is reporting-route intent plus the hook error
distinction, not a switch to restore built-ins. See
[global retirement, validation and artifact cleanup](../../kitsune/AGENT_CONTROL_RETIREMENT.md).

## Data Flow

The selected `coven integration runtime` helper reads Kit's actual API address
from `KITSUNE_SOCKET_PATH`. It creates a private per-address instance, minimal
receiver workspace and receiver socket, then runs the attached receiver/presenter
until stdin EOF or interruption. Users do not run `init`, select sockets or repeat
state-root flags. Conflicting sockets and unreleased crash state are refused,
never unlinked/adopted. The older explicit `agent run WORKSPACE --socket ABS
--kitsune-socket ABS --source SOURCE` interface remains an advanced alternative;
only that manual route requires a preinitialized workspace/private socket parent.

The existing five-provider `coven.agent-management.v1` receipt path and persisted
schema remain compatible. Its interpretation stays caller-supplied/unverified.
The normal view adds evidence provenance and monotonic freshness with a 30-second
validity ceiling, independent of the five-second presentation lease. Repainting
does not refresh evidence. Reference-only providers remain unknown. Restarted
receipts are historical; native event observations are bounded and volatile.
Native IDs, prompts, tools, paths and raw payloads are not generic-view fields.

Normal `agent run` uses a 200 ms receive/admission window and a separate fixed
200 ms reply-write window started after handling, before the existing privacy
fence. Persisted receipts are not acknowledged ahead of commit; native-event
acceptance remains volatile. Filesystem calls can outlast either window and the
unchanged one-second shared client deadline, so delivery can still be uncertain;
never resend or retry a mutation automatically. Frozen v1 `agent serve` retains
its original whole-connection budget. See [transport details](AGENT_MANAGEMENT_CONTRACT.md#private-wire-and-output).

All five hook/extension installers support explicit `--from-kitsune`, resolving
the active private instance from inherited Kit context at report time. Omit
`--state-root`: the configured generic reporter argv supplies it. Use the same
Coven executable selected at startup or by explicit setup; the CLI defaults to its current executable
for environment-route plans/installs. Library requests still require an executable.
Old fixed `--socket` installations remain valid but are not silently converted:
guardedly uninstall the old route first, then explicitly install the new one.
See [route receipts and migration](PROVIDER_ENVIRONMENT_ROUTES.md).
Hook/extension installers remain explicit, preview-first operations, never automatic
installation or an override of native allowed/disable/trust/approval settings
(Codex's explicitly consented shared hooks flag is documented separately).
Codex/Copilot startup references and Pi/OpenCode reference events remain **unknown
even when configured and fresh**. Claude activity is only last-reported evidence,
bounded to 30 seconds in this normal view; legacy v1 inventory/serve is unchanged.

Independent `agent native-extensions` profiles now support pinned Pi context
metadata/busy/settled-idle and OpenCode known-root lifecycle/status/permission/
question observations. They require separate explicit installation, use separate
policies/journals, and do not update historical `agent extensions` reference
assets or receipts. Codex/Copilot current-state inference remains unsupported;
their separate unordered native hook facts are described below. Unknown
resumed OpenCode roots use bounded on-demand native metadata lookup, with silent
eligibility registration and the original observation-age budget; missing or
invalid root DTOs fail closed. Pi makes no root or
decision-correlation claim. See [native profiles](NATIVE_PROVIDER_EVENTS_CONTRACT.md)
for exact source/API and loader/macOS qualification limits. Doctor's existing
closed-v1 capabilities are unchanged; it does not discover these new profiles.

### Codex/Copilot Native Facts

[Native hook facts](NATIVE_HOOK_FACTS_CONTRACT.md) adds `coven.provider-facts.v1`,
source-qualified `codex-hook-facts.v1` and documentation-qualified
`copilot-hook-facts.v1`. These are actual fact displays, never working/idle/blocked/
ended/done state or paired decisions inferred from tool names. Public state is
always UNKNOWN; no shared fake instance UUID or sequence is synthesized. Codex
has local receipt age only, with source age unknown. Copilot UTC-Z source time
anchors monotonic freshness; equal timestamps cannot renew and differing equal-time
events are ambiguous. Facts are bounded and volatile, separate from reference
journals, ordered native profiles and attachment leases.

Generate a deterministic inert snippet for deliberate manual review/merge:

```text
coven agent native-hooks plan --provider codex --config-root ABS_CODEX_ROOT --executable ABS_COVEN --from-kitsune --json
coven agent native-hooks plan --provider copilot --config-root ABS_COPILOT_ROOT --executable ABS_COVEN --from-kitsune --json
```

Generated commands use `agent report-fact --provider PROVIDER --event EVENT
--from-kitsune --best-effort`: zero stdout and exit zero even on reporting failure,
not native hook decisions. Managed native-hooks install/uninstall/rollback is
**not implemented**. Do not manually edit an image owned by existing `agent hooks`
exact-image receipts: use its unchanged explicit uninstall/rollback workflow first
and inspect the handoff, or select a fresh explicit trusted root. Never delete its
journal. Preserve unrelated hooks/settings and native disable/trust/approval policy;
no credentials or provider configuration are automatically edited.

## Djinn native capture boundary

Ordinary Djinn invocation uses the native legacy full-TUI path. Its source plugin
and optional frontend attachment path require an updated UI build. Updating the
Rust CLI alone is insufficient if `DJINN_UI_BIN`, a saved runtime binding or the
wrapper's `DJINN_UI_TARGET` still selects an old UI build. Restart/reload that
provider in a new eligible pane. Mini, external attach, remote and v2 paths are
not qualified by this cut.

Setup generates literal reporter argv
`[ABS_COVEN, "agent", "report-native", "--from-kitsune", "--state-root", ROOT]`.
Djinn spawns it without
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

Existing/resumed sessions need not replay a creation event. When a live status or
decision event names an unknown session, the updated Djinn plugin looks up that
session's native metadata to establish root/child eligibility. Metadata registration
is silent: it emits no lifecycle/status report, consumes no sequence and does not
restore historical pending requests. Merely opening a historical session therefore
does not establish fresh activity. Lookups run only on demand, never during plugin
initialization, with one active lookup and at most 32 waiting live observations.
Lookup time counts against the original 1,900 ms observation-age budget; failures,
stale observations, deletion and disposal cannot turn late metadata into fresh
activity. This requires an updated Djinn UI build, not a Coven receiver restart.

Coven validates the envelope before using its private client. Native RPC uses
`coven.native-events.v1`, method `report`, and exactly the same allowlisted params
without `contract`. Its response uses the existing id/type/result-or-error framing
with the native RPC contract. Acceptance is volatile, not a durability claim.
Existing v1 inventory/receipts are not reinterpreted as native events.

Coven maps native busy/retry to last-reported working, idle to last-reported idle,
pending native decisions to blocked, and deletion to ended. Duplicate/out-of-order
sequences do not refresh evidence. These are neither task success nor readiness,
and events alone establish no pane binding or authenticated native origin.
Multiple sessions remain separate observations.

### Local Selected-Root Attachment

[Native attachment](NATIVE_ATTACHMENT_CONTRACT.md) and Djinn's
[producer contract](../../djinn/docs/local-native-attachment.md) define the delivered
A2 scope: normal local full TUI, default integrated worker/local RPC and implicit
local workspace. The existing selected-load callback observes only a loaded,
mounted Chat root. The frontend captures `agent attachment-target --from-kitsune
--json` once, then uses its own lifetime UUID/revisions and mounted-only one-second
observations. Generation-fenced worker RPC revalidates actual native SDK root
metadata and the actual reporter UUID registry before `report-attachment` frames.
Coven joins exact instance/SID and pins the Kit API device/inode, terminal and
runtime incarnation under the captured receiver epoch.

Only the selected root hosts, not other roots in the backend. Quiet root
qualification is read-only, consumes no entries in the 256-entry native event map
and emits no native event. Its observation remains UNKNOWN with null activity age.
The five-second producer lease is independent of the 30-second native evidence
clocks and five-second generic view lease. Heartbeats never renew activity or
pending decisions. Chat leave, selection change and plugin disposal release;
lost release expires. Conflicting targets are ambiguous, with no newest-PID/SID
inference. Transient work is bounded and capture is never automatically rebound
after endpoint/runtime/epoch replacement. The revocation owner token is cleared
before awaiting release, fencing concurrent disposal.

Private CLI observations expose sanitized `hosting_state` and
`association_revision` for deliberate checked focus. Generic rows may label a
producer-selected root but omit raw targets/IDs/revisions and add no automatic click
action or acknowledgement. Existing 30-second user assertions are unchanged.
Source-qualified root/display origin remains caller-unverified, not authentication,
physical-pixel proof or provider execution ownership. Mini, external attach,
remote/explicit workspaces, HTTP transport and other clients/runtimes remain
unqualified. The seven-scenario [source interop gate](../tools/normal-workflow-check/README.md#a2-source-native-interop)
uses actual SDK/plugin factory/load/controller/registry and actual Coven/Kit, not
a physically rendered Djinn TUI or separate Bun Worker/private-RPC handshake.

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
pressure, the oldest inactive, unleased observation is retired; fresh native facts
and live producer leases, including conflicting leases, are protected. At capacity,
admission refuses until a row is reclaimable. Deletion still retains its tombstone when no display slot can be
reclaimed. Instance high-water marks expire 10 minutes after the last consumed
sequence, and tombstones expire 10 minutes after the first deletion. Each table
also evicts its oldest entry on count pressure. Forgetting a high-water mark
invalidates derived facts for that instance; forgetting a tombstone retires any
remaining ended observation for that tuple. Expiry is collected on reports.
All native state is lost on restart. Replay/revival protection is consequently
bounded by retention and restart, not unlimited.

`coven.normal-observations.v1` includes `omitted_observations` and the cumulative
`retired_native_observations` count. The view prioritizes unacknowledged fresh
attention, then acknowledged fresh attention, then existing fresh/native evidence
priority, with stable observation-ID ties, not oldest-first ordering. Occurrence
acknowledgement changes no pending/native facts; expiry is unknown, not done.
See [attention and checked CLI focus](AGENT_NAVIGATION_CONTRACT.md); generic row
clicks remain read-only. If rows are omitted or
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
  socket to force startup. Generated setup isolates concurrent exact Kit API
  addresses automatically. Unreleased/partial crash state needs diagnosis and
  deliberate manual recovery; restart alone is not self-healing. Private instance
  admission is capped at 64, with no automatic cleanup. See the setup contract.
  Shared registry contention waits cooperatively up to three seconds; competing
  ownership of the same instance refuses immediately. Receipt-monitor failure
  cancels the receiver and joins/cleans up owned work, retaining unreleased state.
- Rollback is a deliberate ownership handoff: stop affected providers/helper,
  preserve/inspect config preimages, explicitly revert runtime ownership and any
  provider configuration using its guarded rollback contract, then restart Kit
  and create new panes/reload providers. Removing rows, helper failure or live
  config reload never hands authority back. Historical references do not resume
  automatically in external mode; ordinary legacy policy returns only after the
  explicit restart handoff.

## Example usage

With compatible Coven/Kit builds and an updated Djinn native full-TUI build:

These source changes are not installed activation. Rebuild all three apps first
when ready, then perform the explicit server restart/new-pane/provider handoff.
Provider configuration is never automatically edited.

```toml
[integrations]
sources = ["coven"]
```

Preserve any other configured sources. Put the intended Coven launcher first on
startup PATH; `coven doctor status --json` shows the selected native identity and
capabilities without creating state. No explicit runtime block is needed. Existing
explicit blocks still win, so a Brew-pinned block does not follow a new dev
selection. Do not blindly delete a setup-managed block: its exact-image receipt
requires a deliberate handoff.

For advanced explicit executable/root pinning, the existing managed setup remains:

```sh
coven integration plan
coven integration enable --apply
```

Review the selected config/executable/state root before applying. Setup preserves
other sources and unrelated settings. An unmanaged runtime (including the previous
manual `agent run` snippet) refuses: follow the explicit manual handoff in the
setup contract rather than overwriting/adopting it.

Other providers require one-time explicit setup. Replace the uppercase roots with
inspected absolute existing provider roots; review each preview before applying:

```text
coven agent hooks plan --provider claude --config-root ABS_CLAUDE_ROOT --from-kitsune
coven agent hooks install --provider claude --config-root ABS_CLAUDE_ROOT --from-kitsune --apply
coven agent native-extensions plan --provider pi --extension-root ABS_PI_EXTENSION_ROOT --from-kitsune
coven agent native-extensions install --provider pi --extension-root ABS_PI_EXTENSION_ROOT --from-kitsune --apply
```

The hook form also supports `codex`/`copilot`; the native extension form supports
`opencode`. Historical `agent extensions` remains reference-only; choose this
independent `native-extensions` workflow for bounded activity, not an in-place upgrade.
Review old reference uninstall pins and coexistence limits before installing.
No automatic provider install, native approval or trust override
occurs. Existing Kit assets require the provider's verified migration and explicit
`--migrate-kitsune` consent, not blind replacement. Stop providers/watchers before
mutation; restart the native OpenCode server, not just its attached TUI.

Restart Kit (without `--no-session`), open new panes and restart/reload providers.
Ensure Djinn's UI selection points at the updated build. Select **Coven** and type
`djinn`, `codex`, etc. normally. Do not run `group present`, discovery presentation
or another publisher against the same `coven` lease. A config reload cannot activate
or roll back this startup-only route. To stop the implicit runtime, remove the Coven
source or set `[integrations.coven] auto_start = false`, then complete the deliberate
restart/new-pane ownership handoff above. For managed explicit setup, preview
`coven integration disable`, then use `coven integration disable --apply` only
against the exact managed config; later edits refuse. It restores the exact
preimage, which can itself enable the implicit Coven runtime; heed the warning and
deliberately opt out if necessary. Disabling does not undo provider setup or stop a
live helper. Guarded disable
does not require a healthy runtime root or an existing recorded executable and
does not mutate runtime state; config/receipt safety checks still apply.

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
- [Setup and bootstrap](INTEGRATION_SETUP_CONTRACT.md),
  [doctor compatibility](DOCTOR_STATUS_CONTRACT.md),
  [provider environment routes](PROVIDER_ENVIRONMENT_ROUTES.md).
- `src/integration_setup.rs`, `src/integration_setup/paths.rs`,
  `src/integration_runtime.rs`, `src/integration_runtime/private_fs.rs`,
  `src/integration_route.rs`.
- `src/main.rs`, `src/agent_management.rs`, `src/agent_management/{native,unix}.rs`,
  `src/agent_management_presentation.rs`.
- Kitsune: `src/config/{model,io}.rs`, `src/server/integration_runtime.rs`,
  `src/server/headless.rs`, `src/integration/env.rs`, `src/persist/restore.rs`.
- Djinn: `clients/djinn-ui/packages/opencode/src/plugin/kitsune-agent-state.ts`
  and `src/plugin/native-event-adapter.ts`, `src/coven/native-event-reporter.ts`
  under that same package.
