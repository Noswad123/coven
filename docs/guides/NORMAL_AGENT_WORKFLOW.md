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
presentation-only; section disclosure in the UI never launches a process.

After that opt-in and explicit native hook/extension configuration, users still
type `djinn`, `codex`, `copilot`, etc. unchanged. Coven supplies attributed plain
rows in the **Coven** integration section, not the old Kit
agent-status widgets, pane-focus/sidebar agent model or mobile completion cues.
No PID association or authenticated native attestation follows.

The desktop sidebar shows Spaces first and all configured integrations as peer
sections simultaneously, with independent collapse and equal expanded-body shares
(stable-order remainder of at most one row). There is no provider selector,
Integrations umbrella or healthy `Available` preamble; unavailable/waiting/empty
messages appear only in expanded bodies. Empty source config leaves only Spaces.
Expand Coven if collapsed; this queues no intent and does not close its lease.
At tiny heights, wheel input on headers navigates the section window; body-wheel
input stays within its section, preserving workspace selection when Spaces fits.
Global compact/hidden mode remains separate from persisted per-section collapse;
the global menu, bottom toggle, width drag and workspace controls remain. The old
vertical two-section divider is retired. See the
[Kit presentation contract](../../../kitsune/INTEGRATION_VIEWS.md) for compact identity,
visual-order workspace numbering, snapshot compatibility and render/queue fences.
This adds no runtime API/protocol or native-agent meaning; mobile adoption and
native Windows/macOS operational qualification remain out of scope.

Peer bodies use one-line summaries. `prefix+a` (the existing customizable
`agent_selector` keybinding) opens Kit's generic local **Integration details**
inspector: source/item lists, search and wrapped scrollable details. Tab/Shift+Tab
cycles focus; arrows or `j`/`k` and PageUp/PageDown navigate/scroll; `/` edits search.
Esc first blurs search, then closes. Source/item clicks only inspect, with no
intent; Enter in Items explicitly sends the existing publisher selection intent.
Enter in Details activates nothing. No native focus, acknowledgement, input,
start/resume/cancel or old agent navigator is added. Server source facts remain
shared; inspector focus/query/scroll are client presentation only.

Structured v3 shows the human name once in Items, then **Attention**, **Current
activity**, **Working context** and collapsed **Diagnostics**, rather than a Coven
metadata dump. With Details focused and search inactive, `d` toggles Diagnostics;
the footer hints it and its visible, scroll-clipped header is mouse-toggleable.
Search covers all information/diagnostics; a hidden-only match says "Match in
Diagnostics — expand to view." without expanding or activating. Expansion resets
on source/epoch/item change, disappearance or close/reopen, not ordinary revisions
(including sampled age). Background rendering and successful-frame geometry/hit-map
fences remain unchanged. Empty information sections show honest missing-data
placeholders, not Idle/no-pending assertions. Without information, generic Details
keeps flat text visible for old v1/v2 and plain v3 third-party publishers; Kit does
not parse Coven-specific diagnostics.

Normal presentation negotiates an additive JSON item profile: descriptor version
1 with `supported_versions: [1,2,3]`; absence means v1 only. Coven validates the
whole advertisement before open/write, selects advertised 3, else 2, else 1, and pins the
profile per lease. Malformed advertisement fails before open; uncertain mutation
never causes downgrade. V1 remains exactly `{id,label,detail}`; v2 optionally adds
`presentation` with `label` up to 48 UTF-8 bytes and optional generic status.
Unknown/duplicate fields and explicit presentation/status null reject; v1 rejects
presentation without lease renewal. Existing limits remain: 128-byte IDs,
160-byte labels, 512-byte details, 128 items/source, eight sources, 32 intents/source,
five-second leases and 96 KiB raw text/source including presentation labels.
V3 additionally permits strict optional non-null `information` with only
`attention`, `activity`, `context`: string arrays defaulting to `[]`, at most four
entries each, each nonblank and at most 256 UTF-8 bytes, rejecting controls/bidi.
Information is at most 1,536 raw bytes/item and is included in the existing 96 KiB
source quota; 128 maximum-size rows are not promised to fit one frame. Unknown/
duplicate fields, null objects/arrays/entries and wrong types reject. V1/v2 reject
information atomically without renewal or partial frame/revision/queue changes.
V3 requires explicit open. Required `detail` remains bounded to 512 bytes and is
diagnostic text when information is present; `information: {}` is valid.
Plain old publishers work. Protocol 18 and binary wire structs are unchanged.

Coven normal `item.label` is a human name only: provider default (for example,
`Djinn`), preferring a valid associated explicit manual pane name. V2/v3 send
`presentation.label: ""`; sidebar status is icon/animation only, without state
words or hashes. Stable opaque row IDs remain internal selection identity, not
display names or native session IDs. Duplicate names are allowed without ID or
suffix disambiguation. V1 remains name-only with exactly `{id,label,detail}`, no
presentation metadata. Third-party nonempty generic captions still render.
Fresh Working uses generic `running`, Blocked uses `attention`; Idle and Ended
omit status and intentionally share the neutral icon. Inspector details distinguish
their states. Expired/history/unordered native
facts are Unknown. Host statuses are `pending`, `running`, `attention`, `error`,
`success`, `unknown`, plus neutral when absent. Success/error are explicit source
assertions available to generic publishers, NOT normal Coven job proof; never
idle=success. `[ui] integration_animations` defaults true; disabling it removes
optional local working spinners, not facts. Animation never refreshes evidence.
Headless presentation animation stops when no full App renderer is attached and
resumes on reattachment without renewing evidence or discarding saved UI state.
No authority bits, private IDs, styles or injected programs enter the generic UI.
Successful-frame fences cover source/epoch/revision/item/query/focus/layout/scroll;
animation phase is excluded and fact age remains independent.

Name lookup requires a fresh valid association, exact endpoint binding and live
terminal runtime generation. An explicit generic `pane.manual_label` may already
exist or be renamed now; OSC title, CWD and historical `agent_name` are never manual
identity fallbacks. Publisher-only private hints are not public raw targets. The
read-only lookup has a shared 300 ms bound, uses no blocking receiver mutex, and
creates no association, focus, input or acknowledgement. Confirmed association,
generation or endpoint loss uses the provider default; temporary incoherence defers
publication instead. Name refresh changes no activity time or evidence authority.
Flat fallback Details shows `Status: running`/`Status: neutral` even with blank
captions; structured v3 uses the concise source-owned information above.

Attention uses fresh reported pending counts, not exhaustive native inventory:
"No pending request in recent reports" is not global all-clear. Current activity
reports Working/Idle/Blocked only from evidence. Missing data says "No recent
activity evidence" or "No activity report received yet"; reference-only providers
say current activity is unavailable. No guessed tools/commands, task summaries,
history, costs or native prompt scope are projected. Evidence age is the selected
fact's age, not last receipt time; it is owner-sampled coarsely once/second, not a
continuous TUI clock, and may freeze during a publisher stall until lease expiry.

Working context is labeled `Kit project`/`Kit branch`, not native Djinn CWD proof.
Project uses associated worktree `repo_name`, else the workspace human label.
Optional JSON `NewWorkspaceInfo.git_branch` supplies cached current provenance in
the same snapshot/event contexts; missing stays unavailable, never inferred from
label/path/native metadata. Projection performs no Git/filesystem/process I/O.
Name/project/branch share the 300 ms qualified lookup bundle: endpoint/generation,
unique pane plus workspace/tab, association epoch, admission and original deadline
are rechecked after I/O and at transport pre-send. Coherently established loss
clears the entire bundle to provider name/no context without hiding an otherwise
eligible current row.
Private qualification fields enter neither public observations nor native payloads
and renew no native clock.

Every candidate, including empty/partial frames, coherently brackets current
receiver projection, name/context lookup and actual pre-send. Temporary projection-
lock contention, changed/odd admission or unavailable lookup evidence defers the
candidate, not fake absence or a transient provider-name fallback that loses a
name-filtered selection. Last-shown publication remains bounded by the minimum
original membership/context deadline and five-second view TTL; deferrals cannot
poll-renew borrowed context indefinitely. Real monotonic membership expiry still
removes rows; coherent explicit removal clears selection. Only safely Unsent
pre-send refresh-needed candidates may rebuild on the same owner/candidate revision
within that ceiling; attempted/uncertain writes never retry.

Read-only receiver observers (legacy inventory/control observations, `attention.list`,
association inspection and `focus.prepare`) do not invalidate admission. Strict
authority/closed-envelope method classification precedes marking; mutations such
as reports, attachments, acknowledgements and association changes remain
conservatively fenced. Reads renew neither native evidence nor association clocks.
Cached branch acquisition is unit-qualified only; composition accepts a cached
branch or unavailable, not physical real-branch acquisition qualification.

The bounded producer profile preserves manual human name and primary attention/
activity under frame pressure: first replace diagnostics with an omission marker,
then compact primary lines and drop context/evidence-age secondary text with an
explicit diagnostic explanation. Coverage includes all 128 escape-heavy rows with
160-byte labels/full 512-byte diagnostics, not arbitrary accepted 3,072-byte line
combinations losslessly fitting a frame. V1/v2 keep legacy provider fallback budgets.

[Checked CLI navigation](../architecture/AGENT_NAVIGATION_CONTRACT.md) permits source-qualified
local Djinn selected-root producer associations as well as unchanged temporary
explicit user assertions. Producer display attachment is not execution ownership.
See the [approved adoption record](../changelog/FEATURE_ADOPTION_STATUS.md) for current scope.

## API / Interfaces

The normal configuration is `[integrations] sources = ["coven"]`; see the
[doctor compatibility contract](../architecture/DOCTOR_STATUS_CONTRACT.md). The optional
`[integrations.coven] auto_start = false` disables the implicit runtime and
inferred ownership for presentation-only/manual publishers. Changes require a
server restart. This is a deliberate change from the older presentation-only
meaning of a source-only Coven configuration.

The advanced managed setup interface remains `coven integration plan|enable|disable`:
`plan` is always read-only; enable/disable preview unless `--apply` is explicit.
See [setup defaults, ownership and recovery](../architecture/INTEGRATION_SETUP_CONTRACT.md).
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
[global retirement, validation and artifact cleanup](../../../kitsune/AGENT_CONTROL_RETIREMENT.md).

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
its original whole-connection budget. See [transport details](../architecture/AGENT_MANAGEMENT_CONTRACT.md#private-wire-and-output).

All five hook/extension installers support explicit `--from-kitsune`, resolving
the active private instance from inherited Kit context at report time. Omit
`--state-root`: the configured generic reporter argv supplies it. Use the same
Coven executable selected at startup or by explicit setup; the CLI defaults to its current executable
for environment-route plans/installs. Library requests still require an executable.
Old fixed `--socket` installations remain valid but are not silently converted:
guardedly uninstall the old route first, then explicitly install the new one.
See [route receipts and migration](../architecture/PROVIDER_ENVIRONMENT_ROUTES.md).
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
decision-correlation claim. See [native profiles](../architecture/NATIVE_PROVIDER_EVENTS_CONTRACT.md)
for exact source/API and loader/macOS qualification limits. Doctor's existing
closed-v1 capabilities are unchanged; it does not discover these new profiles.

### Codex/Copilot Native Facts

[Native hook facts](../architecture/NATIVE_HOOK_FACTS_CONTRACT.md) adds `coven.provider-facts.v1`,
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

[Native attachment](../architecture/NATIVE_ATTACHMENT_CONTRACT.md) and Djinn's
[producer contract](../../../djinn/docs/local-native-attachment.md) define the delivered
A2 scope: normal local full TUI, default integrated worker/local RPC and implicit
local workspace. The existing selected-load callback observes only a loaded,
mounted Chat root. The frontend captures `agent attachment-target --from-kitsune
--json` once, then uses its own lifetime UUID/revisions and mounted-only one-second
observations. Generation-fenced worker RPC revalidates actual native SDK root
metadata and the actual reporter UUID registry before `report-attachment` frames.
Coven joins exact instance/SID and pins the Kit API device/inode, terminal and
runtime incarnation under the captured receiver epoch.

Only the selected root hosts, not other roots in the backend. Root qualification
remains pure and read-only, consumes no entries in the 256-entry native event map
and emits no native event. Separately, after valid selection, Djinn samples actual
local `Session.Service.get` and `SessionStatus.Service.get` through
`src/plugin/native-status-snapshot.ts`, independently of hosting semantics. Explicit
busy/retry/idle readings use the existing native status event contract. Sampling is
at most once per second with a 1,900 ms bound; selection change, unmount, deletion,
reporter replacement/ambiguity and newer native callbacks cancel or fence stale work.
No sparse SDK-list absence is interpreted as idle. The closed actual local native
service's canonical idle default is native-owned state semantics, not process liveness.
Native status setters commit and publish in per-instance, per-session semaphore
order; `get`/`list` remain unlocked for callback reads. This protects initial samples
against newer callbacks and concurrent publication. The immutable query-start time
survives query, queue and IPC within one total 1,900 ms budget, never restamped at
completion or replayed as fresh evidence.
The five-second producer lease is independent of the 30-second native evidence
clocks and five-second generic view lease. Attachment heartbeats alone never renew
activity or pending decisions. Only a successful fresh native query renews activity;
it does not renew permission/question pending occurrences, acknowledge them or prove
an outcome. Failed samples update no activity; evidence becomes Unknown after the
30-second ceiling. This recovers quiet mounted normal full-TUI roots, not every
live Unknown: unmounted, event-only, remote and unavailable profiles still have no
automatic status heartbeat. Missing, expired, invalid and unsupported evidence remain
distinct Unknown causes, never silently idle because a process is alive.
The source fix addresses quiet/resumed selected-root initial and 30-second expiry
gaps. The user's two existing Djinn instances, including the Unknown row, were not
live-inspected; this does not establish their cause or claim they are already fixed.
Chat leave, selection change and plugin disposal release;
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
unqualified. The seven-scenario [source interop gate](../../tools/normal-workflow-check/README.md#a2-source-native-interop)
uses actual SDK/plugin factory/load/controller/registry and actual Coven/Kit, not
a physically rendered Djinn TUI or separate Bun Worker/private-RPC handshake.

### Current Djinn View Versus History

The normal publisher's current Djinn view is distinct from retained observation
history. Keep a row when any unexpired producer-selected-root lease exists
(including conflicting targets that cannot be focused), when a valid manual
association matches the fact, or when non-ended native Working/Blocked/Idle
activity is still within its 30-second grace. Keep every independently current
Djinn root, including a second quiet attached root; labels and whether the user is
typing are not membership criteria. Failed native queries may leave an attached
row Unknown, never idle inferred from liveness.

Hide unhosted ended, lifecycle-only Unknown, expired and historical v1 Djinn rows
from normal publication. An old quiet reporter without activity disappears five
seconds after its last producer heartbeat; real old activity can retain it for at
most 30 seconds after the last native evidence. These are independent clocks, not
a restart timeout. Never guess replacement by label, provider SID, PID or CWD, or
merge opaque observation IDs. Hiding is not deletion, stopped/success evidence or
acknowledgement. CLI observations still expose retained history; `prefix+a` sees
only current published items, not a hidden-history browser.

Filtering is publisher-private and precedes the 128-item cap. Hidden Djinn history
does not count as private omission/retirement loss or reserve an Evidence limits
slot; non-Djinn notices and public aggregate counts are unchanged. Public
observations and exported `run_with_projection` retain the same six-field envelope
and history IDs; `_publisher_*` hints remain private. Frozen legacy `agent serve`
and other providers' display policy are unchanged.

Private membership deadlines survive cached-model and manual-name I/O, with a
conservative deadline floor and admission/receiver-epoch fences. A final transport
pre-send check runs after frame encoding, connection and endpoint fencing. Only
a safely Unsent refresh-needed result permits rebuilding on the same owner lease
and candidate revision. Attempted writes, including uncertain delivery, never
retry or trigger a close; the lease expires. Membership/name/render/animation
checks renew neither attachment nor native-evidence clocks. That stale-row increment needs
no new Kit/Djinn source, API or binary-version changes.

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
See [attention and checked CLI focus](../architecture/AGENT_NAVIGATION_CONTRACT.md); generic row
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

These source changes are not installed activation. For this structured-information/
continuity increment, explicitly approve rebuilding/installing Coven and Kit, then
restart the Kit server to pin the new Coven generation and complete the normal
new-pane/provider handoff as needed. No Djinn rebuild is required by this increment;
the updated native full-TUI baseline remains a prerequisite. Provider configuration
is never automatically edited.

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
Ensure Djinn's UI selection points at the updated build. Expand **Coven** if
collapsed and type `djinn`, `codex`, etc. normally. Do not run `group present`, discovery presentation
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

The historical Coven-only stale-row increment's validation is recorded in the
[gate record](../../tools/normal-workflow-check/README.md#current-stale-row-increment).
It is not qualification or activation of the user's existing rows; no live rows
were inspected. That filter required no new Kit/Djinn build; the current structured
information feature changes Kit/Coven JSON DTOs and UI. Its separate validation is
in the [gate record](../../tools/normal-workflow-check/README.md#current-structured-information-increment),
not an installed activation or new Djinn/native-provider qualification.
Final observed qualification passed with independent final review READY and no
P1/P2 after the continuity fix. The central record separates the earlier full Kit
suite from the latest Coven-only production-fix rerun and records limits/cleanup.

**Historical name/icon/quiet-root baseline: QUALIFIED.** Supplied final results are
green, recorded as separate qualifications, not one combined command:

- Final Kit: 2,578 binary tests passed, five ignored; 193 integration tests across
  11 targets and one explicitly invoked real-Coven roundtrip passed. This covers
  duplicate/icon/name changes; the later compiled Kit hash matched the last unified run.
- Latest unified run: 19 sidebar, 32 normal, four adoption and 16 hosting/status-sampling
  gates; six Coven presentation integration tests and 127 filtered agent-management
  units; 26 Djinn sampling/contract/publication tests across three files and one
  actual-source interop fixture passed.
- After that run, a type-binding-only fix captured Session/Status services outside
  and provided them inside per-instance state, retaining per-instance snapshots,
  with an explicit optional-signal snapshot annotation. The actual full-package
  installed `tsgo --noEmit --incremental false --pretty false` exited 0 with zero
  errors; the final broader Djinn run passed 84 tests across nine files, including
  the new two-instance `Plugin.init/list` bootstrap test. Default and external
  plugins were disabled in that test; it is not live Kitsune-plugin initialization proof.
- Independent review confirmed no P1/P2 findings after the semaphore race fix.

The full unified pipeline was not rerun after the type-binding fix; typecheck and
targeted/broader Bun checks qualify that follow-up separately. Interop qualifies
bounded source callbacks, not a full physical native frontend. No new provider,
mobile, remote or actual model qualification, full CI or full Coven suite is claimed.
All owned children stopped/reaped and cached run directories were empty after
cleanup, with no retained artifacts. No installation/configuration activation has
occurred; activation of that historical cross-app baseline requires all three new
builds and explicit user approval and handoff, unlike the Coven-only follow-up.

Historical prior status/inspector baseline, not tests of this increment: actual
Kit/Coven qualification passed 16 composed sidebar gates, 32 normal
workflow gates, four adoption scenarios, 12 hosting scenarios, five Coven
CLI/presentation tests and 14 presentation-module unit tests. Hosting's actual-source
Bun interop test also passed. Separate Kit validation passed 2,575 binary tests,
all 193 integration tests and the opt-in real-Coven roundtrip. Held-key isolation,
identical-pixel capture changes, failed queues, visible/occluded spinners and
detach/reattach animation scheduling have regression coverage. These checks used
native-shaped fixtures and actual-source adapters, not authenticated provider/model
execution; they are not full CI or a full Coven suite. All owned artifacts were
cleaned. See the
[gate record](../../tools/normal-workflow-check/README.md#sidebar-qualification).

## Related files

- [Receipt API and unchanged v1 inventory](../architecture/AGENT_MANAGEMENT_CONTRACT.md),
  [ownership/migration](../plans/AGENT_MANAGEMENT_MIGRATION.md).
- [Setup and bootstrap](../architecture/INTEGRATION_SETUP_CONTRACT.md),
  [doctor compatibility](../architecture/DOCTOR_STATUS_CONTRACT.md),
  [provider environment routes](../architecture/PROVIDER_ENVIRONMENT_ROUTES.md).
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
