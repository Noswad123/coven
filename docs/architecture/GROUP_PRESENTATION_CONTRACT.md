# Coven-owned group view in Kitsune

## Overview

`coven group present` owns its content and selection meaning within Kitsune's
generic desktop integration section. **Coven supplies the group/participant model;
Kitsune supplies generic rendering and message transport.** No detected-agent
inventory, native session identity, provider status or execution grant is inferred.

This is distinct from `group publish`, which still supplies terminal-border
annotations. Neither command substitutes for the other when unsupported.

## API / Interfaces

First use a compatible running Kitsune build and manually configure sources:

```toml
[integrations]
sources = ["coven", "build"]

[integrations.coven]
auto_start = false
```

The opt-out is necessary for this manual publisher: source-only Coven otherwise
starts the managed normal-workflow runtime and requests external ownership. An
existing explicit runtime block takes precedence; do not run two publishers
against the same Coven lease. Runtime/ownership changes require server restart.

Preserve existing configuration; Coven does not edit/reload/restart/install
Kitsune. Source IDs are logical routing, not authentication: at most eight ordered,
unique ASCII IDs of 1–64 bytes (`A–Z`, `a–z`, digits, `_`, `.` or `-`). The user
sees Spaces first and all configured integrations as ordered peer sections, with
independent disclosure headers rather than an Integrations umbrella/provider
selector. Exact `coven` is titled **Coven**; other IDs remain sanitized literal
titles with collision disambiguation. Only expanded bodies show `Unavailable`
for an absent/expired app, `Waiting for data` for a new lease, or `No items` for
an empty frame. Healthy bodies show rows without an `Available` preamble. Empty
config leaves only Spaces, with no lower placeholder or native-agent fallback.
Legacy hook compatibility interfaces are separate from this source list; this
layout change neither alters global retirement nor adopts integrations on mobile.

Expanded section bodies share height equally (stable-order remainder, at most one
row difference); collapsed sections use only a header. The old vertical split drag
is retired, not sidebar width dragging. Header-wheel input navigates a contiguous
section window at tiny heights; body-wheel input stays in that section. Spaces
retains wheel-based workspace selection when its content fits. The global menu,
bottom toggle and Spaces `new` action are separate from disclosure. Compact/hidden
mode is independent of persisted per-section collapse. See Kitsune's contract for
compact identities, visual-order workspace numbering and tolerant snapshot decoding.

Peer bodies now use one-line summaries. `prefix+a` (the existing customizable
`agent_selector` keybinding) opens the generic local Integration details inspector
with source/item lists, search and wrapped scrollable details. Tab/Shift+Tab cycles
focus; arrows or `j`/`k` and PageUp/PageDown navigate/scroll; `/` edits search.
Esc first blurs search, then closes. Source/item clicks only inspect locally and
queue no intent. Enter in Items explicitly sends the existing publisher selection
intent; Enter in Details activates nothing. No native focus, acknowledgement,
input, start/resume/cancel or retired agent navigator is provided. Shared source
facts are server-owned; inspector focus/query/scroll are client presentation.

Structured v3 normal rows show the selected name once in Items, followed by
Attention, Current activity and Working context, then collapsed Diagnostics.
`d` toggles Diagnostics only with Details focused outside search; the footer hints
it and the visible scroll-clipped header is mouse-toggleable. Search includes all
information and diagnostics; hidden-only matches show "Match in Diagnostics —
expand to view." without automatic expansion/activation. Expansion resets on
source/epoch/item change, disappearance or close/reopen, survives ordinary revisions
including age updates, and cannot be changed by background rendering. Existing
successful-frame geometry/hit-map fences remain. Plain items, including `group
present`, retain visible generic Details with no Coven-specific parser.

The obsolete `[external_view]` config is ignored with a diagnostic directing users
to `integrations.sources`, never rewritten automatically. Both repositories use
the new contract together; no published compatibility with that local prototype
is required.

Use an **existing** Coven workspace/group; do not initialize over an application
repository. For declaration/binding setup see [GROUPS_CONTRACT.md](GROUPS_CONTRACT.md).

```text
coven group present WORKSPACE GROUP --socket ABS_SOCKET --source coven [--duration 60] [--json]
```

Workspace, group, socket and source are explicit. Duration is 1–3600 seconds.
Output is pretty ASCII JSON by default, compact with `--json`, with one terminal LF.
The library exports `group_presenter::{Request, arguments}`.

## Data Flow

1. Read Coven's declaration store and freeze its revision. Display one group header
   and at most 127 participant rows from that store—not Kitsune's agent hints.
2. Pin the explicitly selected endpoint, discover protocol 18 and
   `snapshot.integration_views.version == 1`. Validate the complete ordered
   `sources: [{id, epoch}, ...]` list (including entries after the target) and find
   the requested source anywhere, not only first. Each inactive epoch is null;
   discovery exposes no lease tokens and is independent of section visibility.
3. Open only the requested source's five-second lease, publish a full frame, then
   poll about every 250 ms. `--source coven` does not own/replace the whole sidebar:
   `build` and other producers coexist with independent leases/frames/revisions/queues.
4. Accept only the current epoch/revision and known participant IDs. Re-read the
   local store before publication and after polling. Unknown/stale selections are
   ignored; a store revision change stops the run rather than adopting new state.
5. A valid participant selection publishes a newer frame with read-only details.
   The group header is explanatory; selecting it has no participant-details effect.
6. Duration completion/Ctrl-C attempts one graceful close within the remaining
   budget. Crashes or uncertain delivery rely on lease expiry.

`group present` remains a plain v1 publisher with exactly `{id,label,detail}`.
The descriptor remains version 1 and advertises `supported_versions: [1,2,3]`; absence
means v1 only. Only the normal Coven presenter negotiates higher profiles after validating the
whole advertisement before open/write, choosing advertised 3, else 2, else 1 and pinning
the profile per lease. Malformed negotiation fails before open and uncertain
mutation never triggers downgrade. V2 optionally adds `presentation` with label
up to 48 UTF-8 bytes and optional status `pending|running|attention|error|success|unknown`.
Omitted status is neutral; explicit presentation/status null, unknown/duplicate
fields and presentation on v1 reject without renewal. Presentation-label bytes
count toward the existing 96 KiB bound. Protocol 18/binary wire structs are unchanged.

V3 optionally adds strict non-null `information` with only `attention`, `activity`,
`context`, string arrays defaulting to `[]`, each at most four entries. Entries are
nonblank, at most 256 UTF-8 bytes and reject controls/bidi; all information text
totals at most 1,536 raw bytes/item and counts toward 96 KiB/source. Unknown/duplicate
fields, wrong types and null objects/arrays/entries reject. V1/v2 reject information
atomically without lease renewal or partial frame/revision/queue changes. Explicit
v3 open is required. `detail` remains required and at most 512 bytes, becoming
Diagnostics only when information is present. `information: {}` is valid, showing
honest empty-section placeholders, not Idle/no-pending claims. A 128-row ceiling
does not promise all maximum-size rows fit the source quota.

Normal Coven `item.label` is human-name-only: provider default, preferring a valid
associated explicit manual pane name. V2/v3 send `presentation.label: ""` for
icon/animation-only status, no state words/hashes. Normal v1 remains exactly
`{id,label,detail}`, name-only without metadata; `group present` retains its declared
participant keys/roles. Stable opaque row IDs are internal selection identity, not
native session IDs or displayed labels. Duplicate names need no ID/suffix.
Nonempty generic third-party captions still render; this is not global word stripping.
Fresh Working is `running`, Blocked is `attention`; Idle/Ended intentionally share
a neutral icon, distinguished by detailed state text. Flat fallback Details
shows `Status: running`/`Status: neutral`; structured v3 shows concise information.
Expired/history/unordered facts are Unknown.
Generic success/error require explicit source assertions, not normal Coven job
proof; idle never means success. `[ui] integration_animations` defaults true and
controls only optional local working spinners, never evidence refresh. Generic UI
accepts no authority bits, private native IDs, styles or injected program.

Normal name lookup uses only publisher-private association hints, with fresh
association, endpoint and terminal-runtime generation checks within 300 ms, no
blocking receiver mutex or public raw targets. Already-set/newly renamed explicit
`pane.manual_label` qualifies, never OSC title/CWD/old `agent_name`. Confirmed
association/generation/endpoint loss uses the provider default; temporary
incoherence defers publication. Rendering creates no association/focus/input/
acknowledgement
and name refresh renews no activity or evidence authority. See the
[normal workflow](../guides/NORMAL_AGENT_WORKFLOW.md#local-selected-root-attachment) for the
separate fresh native status query, not attachment-heartbeat activity inference.

The qualified bundle now includes name/project/branch within that same budget.
Endpoint/generation, unique pane/workspace/tab, association epoch, admission and
original deadline are checked after I/O and at transport pre-send. Coherently
established loss clears the entire bundle to default name/no context without hiding an otherwise current
row. `Kit project` is associated worktree `repo_name`, else workspace human label;
`Kit branch` is optional cached current JSON `NewWorkspaceInfo.git_branch` in the
same snapshot/event contexts, not label/path/native inference. Projection does no
Git/filesystem/process I/O. Context is not native Djinn CWD proof. Qualification
fields stay private, outside public observations/native payloads, and renew no clock.

Normal publication coherently brackets receiver projection, lookup and actual
pre-send for every candidate, including empty/partial frames. Temporary projection-
lock contention, changed/odd admission or unavailable lookup evidence defers rather
than publishing fake absence or provider-name fallback that loses name-filtered
selection. Last-shown data remains bounded by the minimum original membership/
context deadline and five-second view TTL; deferrals cannot poll-renew it forever.
Real monotonic membership expiry still removes rows; coherent explicit removal
clears selection. Only safely Unsent pre-send refresh-needed candidates rebuild on
the same owner/candidate revision within the ceiling; attempted/uncertain writes
never retry. Read-only receiver observers (legacy inventory/control observations,
`attention.list`, association inspection and `focus.prepare`) do not invalidate
admission or renew evidence/association clocks. Strict authority/closed-envelope
classification precedes marking; report/attachment/acknowledgement/association
mutations remain conservatively fenced. Cached branch acquisition is unit-qualified
only; composition accepts a cached value or unavailable, not physical branch proof.

Coven owns concise wording: fresh reported pending counts are not exhaustive native
inventory, and "No pending request in recent reports" is not global all-clear.
Activity is reported Working/Idle/Blocked or honest missing/reference-only evidence,
not guessed tool commands or summaries. Selected-fact evidence age, not last receipt,
is owner-sampled once/second; a publisher stall can freeze it until lease expiry,
not a continuously advancing TUI clock. No task history, cost or native prompt scope
is added. Under frame pressure, preserve manual name and primary attention/activity,
first omit diagnostics with a marker, then compact primary lines and drop context/
age with explicit diagnostic explanation. The bounded 128 escape-heavy-row profile
with 160-byte labels/full 512-byte diagnostics is covered; arbitrary accepted
3,072-byte line combinations are not promised lossless. V1/v2 legacy provider
fallback budgeting remains unchanged.

The normal Djinn publisher now separates current rows from retained history; this
does not change declared participant membership in `group present`. Current means
any unexpired producer-selected-root lease (even conflicting, nonfocusable targets),
a valid manual association matching the fact, or non-ended fresh native
Working/Blocked/Idle activity within 30 seconds. Quiet attached Unknown remains
visible on failed queries, as does every independently current second root. Neither
typing nor label selects the current root; liveness is not idle evidence.
Unhosted ended, lifecycle-only Unknown, expired and historical v1 Djinn receipts
are hidden, not deleted/stopped/successful/acknowledged. Old quiet attachment-only
rows expire five seconds after the last heartbeat; real native activity can retain
them for at most 30 seconds after that evidence. No label/provider-SID/PID/CWD
replacement inference or opaque-ID merge occurs.

The private filter precedes the 128-item cap; hidden Djinn history cannot create an
Evidence limits ghost or consume the last current-row slot. Non-Djinn notices and
public aggregates are unchanged. Public observations and exported
`run_with_projection` retain their six-field envelope and history IDs, with no
public `_publisher_*` hints. Generic `prefix+a` sees published rows; CLI history
reads remain available. Frozen legacy serve and other-provider policy are unchanged.
Private deadlines are rechecked across cached models/name I/O and at transport
pre-send after encoding, connection and endpoint fencing, with conservative
deadline-floor, admission and epoch fences. Only safely Unsent refresh-needed
frames rebuild on the same owner/candidate revision; attempted/uncertain writes
never retry or close. Membership/name/render/animation checks refresh no lease or
evidence clock. No new Kit/Djinn source/API/binary-version changes are required.

Participant rows show declared keys/roles. Selection does not bind/adopt a pane,
change membership or roles, focus a terminal, read a native artifact, submit input,
launch/resume/cancel a provider, or change local authority files. Kitsune owns
palette, layout, clipping and scrolling; no app-defined scripts/widgets are sent.

The transport uses only `integration.view.open`, `integration.view.replace`,
`integration.view.poll` and `integration.view.close`; there are no aliases for
the obsolete unreleased `view.*` methods. Responses retain `view_opened`,
`view_replaced`, `view_intents` and `view_closed` result types. Existing observation
read allowlists and terminal submission/admission contracts are unchanged. Endpoint
replacement, incompatible discovery and malformed/uncorrelated replies stop the
run; there is no annotation or native-control fallback.

## Edge Cases

See [Kitsune's generic contract](../../../kitsune/INTEGRATION_VIEWS.md) for exact fields and
bounds. Source/lease/epoch are correlation, not authentication or permission.
Intents are bounded ephemeral `{epoch, revision, item_id}` messages, not commands.

Successful replace/poll alone renews an opened lease; clicks, rejected calls and
local disclosure do not. Collapse queues no intent and closes no lease.
Close/expiry/removal affects only that provider. Config reorder preserves retained
leases, frames, queues, collapse and scroll by ID; removal prunes removed state and
re-add starts expanded. Removing another source does not reset Coven. Quiet timers
handle expiry without terminal activity, not with hard-real-time pixel guarantees.

Per provider: 128 items, unique nonblank IDs up to 128 UTF-8 bytes, nonblank labels
up to 160 bytes, details up to 512 bytes, 96 KiB total raw text, 32 queued intents.
Eight providers allow aggregate maxima of 1,024 items, 768 KiB and 256 intents.
IDs may coincide across providers without sharing state. Explicit sidebar item
activation routes the exact source/epoch/revision/item ID from a visible one-line
summary; wrapped details live in the inspector. The intent payload stays unchanged.
Multi-section layout, client, collapse and scroll changes fence stale hits until
a successful render/frame queue; failed enqueue and invisible rows grant no hits.
Background smaller/mobile draws cannot clamp foreground presentation or replace
its captures. Inspector query/focus/selection/scroll also fence activation; animation
phase is excluded and fact age is independent. There is no implicit native
focus/input; additive JSON v2 changes no binary wire structs (protocol 18). A successful local
draw/server frame queue is not physical client-pixel acknowledgement or permission
to execute. No broker, plugin or scripts are involved.

Poll drains at most once. Lost replies can lose selections; no automatic resend,
reopen or reconnect replay is attempted. A possibly admitted frame or drained poll
is reported uncertain; the lease is left to expire rather than refreshed/retried.
Slow calls may cause server lease rejection even if each fits its three-second
RPC budget. This is fail-closed inspection, not guaranteed delivery or availability.

The report uses `schema_version: 1`, `contract: coven.group-presentation.v1`,
`delivery: ephemeral_ui_only`, `authorization: none`, and `dispatchable: false`.
It reports frame/poll/selection counts, ignored intents, outcome and cleanup state.
Completion exits 0, interruption without delivery uncertainty 130, and operational
failure/uncertainty 2 (including an interruption after a possibly admitted write);
invalid domain arguments use ordinary fixed errors. Peer error text is not echoed.
Local display labels and participant IDs may be sensitive; review before sharing
reports/screenshots. No raw native reference/binding contents are projected.

## Example usage

With the source configured and an existing workspace/group:

```text
coven group present WORKSPACE GROUP --socket ABS_SOCKET --source coven --duration 60 --json
```

Expand **Coven** if collapsed, then open Integration details to inspect locally;
Enter in Items explicitly requests publisher-updated participant details.
Collapsing a section does not stop Coven's lease
or change its management model. This is separate from border annotation priority.

## Validation and next ownership work

Private-socket workflows cover the real CLI and loss/stale/malformed/endpoint
cases without installed apps. The explicit real-Coven fixture connects to actual
Kitsune handlers on a private fixture socket and `TestBackend`, exercising
participant selection, newer details, close and independent `build` isolation.
The historical Coven-only stale-row increment has its own
[gate record](../../tools/normal-workflow-check/README.md#current-stale-row-increment),
not a claim of live-row resolution or installed activation. No new Kit/Djinn
builds were needed for that filter. Current structured information changes Kit/Coven
JSON DTOs/UI; its separate record is in the
[gate record](../../tools/normal-workflow-check/README.md#current-structured-information-increment).
Final observed structured-information/continuity qualification passed; independent
review is READY with no P1/P2. The central record distinguishes the earlier full
Kit suite from the latest Coven-only production-fix rerun and records cleanup and
limits. Activation, when explicitly approved, needs rebuilt/installed Coven and Kit,
then Kit server restart to pin the Coven generation, not a new Djinn rebuild.

**Historical name/icon/quiet-root baseline: QUALIFIED.** Final Kit validation passed
2,578 binary tests (five ignored), 193 integration tests across 11 targets and one
explicit real-Coven roundtrip, including duplicate/icon/name behavior. The separate
latest unified run passed 19 sidebar, 32 normal, four adoption, 16 hosting/status-sampling,
six Coven presentation integration and 127 filtered agent-management unit tests,
plus 26 Djinn tests across three files and one actual-source interop fixture.
After the subsequent type-binding-only fix, full-package installed tsgo passed with
zero errors and broader Djinn validation passed 84 tests across nine files.
Independent review found no P1/P2 after the semaphore race fix. These checks are
separate, not a unified rerun after the binding fix; the
[gate record](../../tools/normal-workflow-check/README.md#sidebar-qualification) gives scope.
No full CI/full Coven suite or new provider/mobile/remote/model qualification.
All owned children stopped/reaped and cached runs were empty, no retained artifacts.
This docs-only update performs no builds, installs or configuration changes;
activation of that historical cross-app baseline requires all three new app builds
and explicit user approval, unlike the Coven-only follow-up. Existing
user Djinn rows were not live-inspected and are not claimed fixed or activated.

Historical prior status/inspector baseline, not this increment: actual Kit/Coven
qualification passed 16 composed sidebar gates, five Coven
CLI/presentation tests and 14 presentation-module unit tests; the existing normal
workflow, adoption and hosting gates also passed with compact-summary/inspector
assertions. Separate Kit validation passed 2,575 binary tests, all 193 integration
tests and the opt-in real-Coven roundtrip. Regression coverage includes input-repeat
isolation, identical-pixel capture changes, failed queues, visible/occluded spinners
and detach/reattach animation scheduling. This is not full CI or a full Coven suite.
Consult the
[actual-server gate record](../../tools/normal-workflow-check/README.md) for current
source-qualified evidence, distinct from fixture/coordinate-helper checks.
The fixture is ignored by default and requires explicit
`COVEN_TEST_BIN`, with no sibling/PATH fallback:

```sh
COVEN_TEST_BIN=/absolute/path/to/coven cargo test --offline --locked --bin kitsune app::api::integration_views_roundtrip_tests::integration_views_real_coven_roundtrip -- --exact --ignored
```

Run that recipe from Kitsune with its selected project toolchains. It uses a
direct-handler socket proxy and coordinate helper, not the full production API
server or physical input. Successful frame queueing is not physical native-TTY
click validation or client-pixel acknowledgement. Native Windows/macOS operation
is not qualified; no installed activation or authenticated provider/model run was
performed.

Next work should extend **Coven's management model and runtime adapters** behind
this generic boundary without reviving globally retired Kitsune management surfaces.
Do not add Coven-specific agent fields/actions to Kitsune or make more
characterization the sole product milestone. Legacy native restore/failure tests
remain removal gates for those subsystems, not prerequisites for every independent
presentation increment. Future effectful actions require separately specified
Coven authorization and native runtime support; a menu click is not that grant.

## Related files

- [src/group_presenter.rs](../../src/group_presenter.rs): request, declaration projection and foreground loop.
- [src/group_presenter/wire.rs](../../src/group_presenter/wire.rs): whole-list discovery and response validation.
- [src/observation_socket.rs](../../src/observation_socket.rs): explicit method allowlist and endpoint fencing.
- [tests/group_presenter_workflow.rs](../../tests/group_presenter_workflow.rs): private-socket workflows.
- [GROUPS_CONTRACT.md](GROUPS_CONTRACT.md), [APP_BOUNDARIES.md](APP_BOUNDARIES.md) — declarations and migration scope.
