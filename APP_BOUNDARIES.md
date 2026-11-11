# App boundaries: Coven, Kitsune, and Djinn

## Overview

Decision date: **2026-10-22**. Status: **accepted target; initial read-only
observation implemented in Coven; production opt-in terminal-border annotations
implemented in Kitsune; explicit participant groups/roles and a foreground publisher
implemented in Coven, including explicit process-evidence group binding independent
of Kitsune's agent inventory; explicit Djinn live attachments and read-only
busy/unknown group status and explicit expiry-bounded live publishing implemented;
native opt-in agent comparison implemented as extraction characterization;
Coven-owned Claude/Codex/Copilot hook and Pi/OpenCode reference-extension cuts implemented with Linux-only Kitsune new-install retirement;
production Coven-owned Linux all-22-family discovery/watch implemented with temporary duplicate legacy recognition policy, not detector retirement;
native lifecycle/scheduling, automatic following, complete extraction, and workspace
annotations remain open**.
Keep three independently useful apps, not one tightly coupled agent stack.
The confirmed scope includes all agent management, including hooks and agent-aware
restore, not only discovery/status/start/resume. The first targeted retirement is
Kitsune's new Claude hook installer on Linux only, followed by Codex, Copilot, Pi and OpenCode on Linux. The ultimate target covers all
agent features/platforms, but matching non-Linux adapters are still pending. See the authoritative
[ownership matrix and next transfers](AGENT_MANAGEMENT_MIGRATION.md).

| App | Target responsibility |
| --- | --- |
| Kitsune | General multiplexer: PTY/process/terminal/session/pane operations and generic label/badge rendering. |
| Coven | Cross-runtime agent discovery/status, hooks, start/resume and agent restore policy; observations/provenance, groups/roles, orchestration and job progress. |
| Djinn | Djinn-native agent/session execution, configuration, tools, policy, transcripts, and results. |

Agent-specific discovery, status interpretation, hooks, start/resume, agent-aware
restore and job concepts move out of Kitsune into optional Coven integration. Target: without
Coven, Kitsune is a fully functional multiplexer with no agent-aware features.
Kitsune **currently still has built-in agent detection and integrations**; this
decision neither removes them immediately nor claims extraction is complete.

## API / Interfaces

**Coven owns agent management; Kitsune exposes generic multiplexer controls,
side-menu presentation and app messaging.** The functional
[group presentation bridge](GROUP_PRESENTATION_CONTRACT.md) now supplies Coven's
own participant/role rows within a multi-app Integrations block and consumes
read-only selection intents. Kitsune's
generic model contains only opaque IDs and plain label/detail text, not agent or
job semantics. This is an optional ownership step, not completed removal of legacy
Kitsune agent management.

The desktop lower block defaults to `Integrations`, with `No integrations configured`
for an empty list and never detected-agent fallback. Manual `[integrations]`
`sources = ["coven", "build"]` configures up to eight ordered unique ASCII IDs.
The user selects an app locally from a vertical list; only its generic label/detail
rows are shown. Coven validates the whole `snapshot.integration_views` descriptor
and finds its source anywhere, not only first. `integration.view.open/replace/poll/close`
give each provider its own lease, frame, revision and queue. Successful replace/poll
renews its five-second TTL; close/expiry/removal affects only that provider and
reorder preserves retained state/selection/scroll. No source selection is auth;
at-most-once intents grant no native execution or physical-pixel acknowledgement.

### Legacy migration scope

| Surface | Current scope / next owner |
| --- | --- |
| Desktop persistent lower menu | Generic multi-app block, not a built-in agent inventory. |
| New Claude hook configuration | Linux: Coven-owned settings operations, Kit install/recommendations/updates retired. Non-Linux: Kit production installer/assets/result type and recommendation/update/bulk paths preserved until matching Coven adapters. See [hook contract](CLAUDE_HOOKS_CONTRACT.md). |
| New Codex hook configuration | Linux: Coven-owned hooks.json + config.toml operations; Kit install/recommend/update/bulk paths retired before provider discovery. Non-Linux legacy installer/assets/result type preserved until matching Coven adapters. Shared global enablement is not native approval. See [Codex contract](CODEX_HOOKS_CONTRACT.md). |
| Independent Codex receipts | Same private Unix receiver/store; SessionStart reference capture only, always unknown even fresh. No working/idle lifecycle or native operational certification. |
| New Copilot hook configuration | Linux: Coven owns explicit global settings.json direct SessionStart handlers; Kit install/recommend/update/bulk paths retired. Non-Linux Kit installer/assets/result type and recommendation/update/bulk paths remain. Strict duplicate-free JSON only; native JSONC refused. See [Copilot contract](COPILOT_HOOKS_CONTRACT.md). |
| Independent Copilot receipts | Same Unix receiver/store; exact SessionStart reference capture only, unknown even fresh; no camelCase/inferred events or Claude subagent suppression. Documentation pin, not native implementation/binary certification. |
| New Pi reference extension | Linux: Coven explicit existing-root placement and exact opt-in v7 migration; Kit new install/recommend/update/bulk retired. Non-Linux Kit full asset/installer/result and recommendation/update/bulk remain. File placement is not activation; see [Pi contract](PI_EXTENSION_CONTRACT.md). |
| Independent Pi receipts | Separate report-extension with strict Coven three-field envelope; session_start/getSessionId references only, always unknown. Migration drops Kit working/blocked/idle; no full Pi management transfer. |
| New OpenCode reference plugin | Linux: Coven explicit existing plugin or plugins root placement and exact opt-in full-v9 migration; Kit new install/recommend/update/bulk retired. Non-Linux full legacy paths preserved. See [contract](OPENCODE_EXTENSION_CONTRACT.md). |
| Independent OpenCode receipts | Separate strict Coven three-string envelope, session.created/updated root references only, unknown even fresh. Native server exact-directory-instance scope, not selected TUI/pane/attach or whole project; migration drops working/blocked/idle. |
| Independent Claude receipts | Coven Unix private receiver/inventory; Windows unsupported, macOS not validated. Not a non-Linux hook-configuration replacement. See [receiver contract](AGENT_MANAGEMENT_CONTRACT.md). |
| Generic Linux foreground-cohort recognition | Coven `agent discover` recognizes all 22 literal families without Kit agent APIs/hints, workspace/storage or receipt/group joins; optional bounded watch/generic view. Partial scope, not native identity/activity/full status or detector retirement. See [contract](AGENT_DISCOVERY_CONTRACT.md). |
| Agent APIs/detection, navigator/attention and mobile UI | Retained with typed foreground/fallback/history separation and deliberate readiness/authority hardening; scoped validation complete with limits below, no external mode or matching retirement. |
| Native references, agent restore/resume and remaining lifecycle hooks | Still Kitsune, including Claude process/screen fallback and restore; transfer to Coven/runtime adapters with subsystem-specific removal gates. |
| `integration.install/uninstall`, fixed `IntegrationTarget`, legacy agent-hook settings | All six target entries, status/uninstall/old ingress remain on all platforms. Claude/Codex/Copilot/Pi/OpenCode new installs remain non-Linux only; legacy UI settings remain for other providers/platforms. Not a generic provider registry. |

New Claude settings are explicitly selected and preview by default; Linux-only
apply can migrate only exact verified legacy commands/script with consent. No
automatic migration, live authority release or conversion of running sessions.
Coven's private receipt inventory is last-reported, caller-supplied evidence, not
native identity/current activity/readiness/job completion or resume permission.
An optional independent worker presents only generic snapshot/`integration.view.*`
rows after receiver readiness; failed Kitsune UI does not stop core capture. This
creates no pane binding, launch, adoption, role or task authority.

Codex's strict v1 two-file journal is separate from Claude's preserved v2 receipt.
The shared private filesystem engine supplies provider-specific allowlists/recovery
names, not common provider policy. Codex validates both files before any artifacts,
rereads under lock and commits pending journal → hooks → TOML → final journal;
there is no global atomicity claim. Normal uninstall leaves the shared global
feature enabled, never revives Kit commands; explicit guarded rollback restores both
original images/modes/absence. Native trust state stays user/native-controlled.
Full discovery/status/start/resume/restore transfer remains pending for every
provider despite the independent literal-family discovery path.

Copilot shares Claude's single-file v2 operation runner with independent receipt,
lock, recovery and error namespaces, not Codex's separate two-file journal.
Previews create no lock/files; actual single-file apply may create a persistent
lock before validation. Copilot preserves native disable/approval/sandbox/trust
policy. Exact opt-in legacy migration retains the script; normal uninstall never
revives Kit, while guarded rollback restores bytes/mode/absence. The scoped Linux
new hook transfer is complete; all-platform Copilot detection/status/start/resume/
restore and legacy ingress/status/uninstall remain. Pi/OpenCode broader discovery/lifecycle/
start/resume/restore remain pending; separate Djinn handling is not folded into them.
Earlier generated three-provider real-Coven end-to-end checks pass; the
[validation record](AGENT_MANAGEMENT_MIGRATION.md#validation-record) covers focused
checks and an intermittent receipt-deadline test failure, not native certification.

Pi's source pin is earendil-works/pi `6e11a00a01a568ef0ae0df5074acf02fc107c34b`,
declaring 1.5.2 on main, not a release pin. `hasUI` includes TUI/RPC, not root/TUI
proof or headless capture. Its namespaced two-leaf journal preserves immutable
preimages and explicit rollback after uninstall; normal uninstall never revives
Kit. Stop providers/watchers before mutation: no atomic activation, autoload or
in-memory authority release follows. Five providers share the same five schema-v1
fields and aggregate 128 cap; older readers reject newly supported Pi/OpenCode
providers, so upgrade receivers first and respect downgrade limits. The historical
[Pi validation table](AGENT_MANAGEMENT_MIGRATION.md#pi-validation-table)
records 212 focused Rust checks plus generated-asset and Kit checks, including
the retained receipt timing limitation; it is not native activation certification.

OpenCode's authoritative canonical anomalyco/opencode release is v1.19.5 commit
`810c0eba1a7348036f8bdf61e6632d0c36c6876c`; changed dev `611cee9` is unsupported.
The server may be remote: executable/socket must exist there. Root full DTO checks
require info.id/sessionID equality and parentID undefined/omitted, without cache or
SDK reads; quiet resumed sessions need not emit events. This conservative subset
caps summary.diffs and permission at 256. Queue admission precedes DTO work; the
FIFO is 32 pending plus one active, with no retry/deduplication. See the
[OpenCode contract](OPENCODE_EXTENSION_CONTRACT.md) for all producer bounds.
The shared private fixed-two-leaf v1 engine has independent OpenCode namespaces
and preserves [Pi API/receipt/generated-byte fixtures from 887451b](tests/pi_extension_frozen_compatibility.rs).
Stop native servers/watchers before mutation and restart the native server after
manual apply, not just TUI attach; no automatic restart/in-memory release. Do not
double-install plugin and plugins roots; no across-root dedupe/authority exists.
The [historical OpenCode scoped table](AGENT_MANAGEMENT_MIGRATION.md#opencode-validation-table)
records 20 shared checks, 14 workflows, 12 Node/Bun producer checks, Kit 111/10
overlapping filters and six legacy-asset checks. The historical OpenCode scope passed
236 focused Rust checks plus lint/MSRV/build/package checks, with recorded receipt
timing limits retained. Historical Pi totals are not additive. No installed-native
certification is claimed.

The obsolete local singleton `[external_view]` is diagnosed/ignored in favor of
`integrations.sources`, with no automatic config writes. Its unreleased `view.*`
methods have no aliases; both repositories now use the new contract together.
Build out Coven management and adapters, then retire legacy surfaces; do not turn
characterization into the only product milestone or add a broker/plugin/script host.

The separate [Djinn live observer](DJINN_LIVE_OBSERVATION_CONTRACT.md) reads one
explicitly pinned private heartbeat from a new, startup-enabled legacy full-TUI
worker build. Fresh evidence is busy-only runner ownership, never idle, health,
progress or completion. No app launch/restart, HTTP, folder fallback, automatic
group enrollment, readiness upgrade or dispatch follows; persisted folder observation
remains independent. The separate [group live contract](GROUP_LIVE_STATUS_CONTRACT.md)
adds explicit existing-member source declarations and ephemeral revision-scoped status,
not verified terminal/session association or native identity.

The first native persisted-evidence slice is implemented separately from Kitsune:
[`coven observe djinn --executable ABS_BINARY --folder ABS_FOLDER --json`](DJINN_OBSERVATION_CONTRACT.md).
One trusted selected binary supplies bounded root manifest/event evidence through
Djinn's native `session observe` contract. No live/native-store/UI status, group
enrollment, help-only readiness upgrade, per-attempt admission or dispatch follows.
No status/list/watch fallback is allowed; their recovery/probing semantics remain
unchanged. Djinn's core stays Coven-neutral.

Target dependency direction: Coven adapters consume supported runtime and
multiplexer capabilities. Kitsune and Djinn have no core dependency on Coven or
each other. Optional Djinn reporting and optional Kitsune external decoration
are integration edges, not startup or installation prerequisites.

### Production Agent Discovery

[Discovery v1](AGENT_DISCOVERY_CONTRACT.md) adds a workspace-free production
`coven agent discover --socket ABS_SOCKET`, optional pane/terminal pin, bounded
watch and optional generic presentation. It uses only generic snapshot/process
reads, raw argv in memory and redacted PID/provider/rule/locator outputs. All
matching processes across 22 literal families are reported without leader/ranking;
activity/attention stay unknown and native identity/lifecycle/completion unestablished.
Help/server/maintenance family hints are not native interactive certification.
No storage, adoption, launch/input/stop/resume, roles, native-ID association or
receipt/group join is added. The classifier intentionally diverges from the
209-case legacy process corpus; it is not parity.

Kit's optional `snapshot.process_observation` version 1 advertises
`platform: linux`, `scope: pane_foreground_cohort`, `selection: unclassified`.
Protocol 18 is unchanged and capability discovery is additive. Coven rejects
Windows `selected_process`/`legacy_agent_aware` and macOS in this first cut.
Reported Linux metadata is not authenticated platform/live-process attestation,
a process tree/host catalog or native generation. Matching inventory brackets and
endpoint fences discard partial failures but do not make attribution atomic or
eliminate PID ABA. Read requests can ensure a default workspace/shell through
existing Kit housekeeping; they are not zero-effect operations.

No matching Kit detector retirement has occurred. The internal typed
[foreground/fallback/history seam](../kitsune/DETECTOR_EVIDENCE_SEPARATION.md) is
implemented. Historical/screen-only data cannot mint native ownership, takeover
or effectiveness. Restored Idle/name/reference/pending resume/managed Active remain;
readiness needs a matching probe or accepted fallback, not foreground-only proof.
Native input's independent fresh process check remains. This deliberately narrows
public semantics, not blanket compatibility. No external mode, new Config,
wire/API/snapshot fields or toggle/kill/hide controls ship. This is functional Coven recognition
with temporary duplicate legacy policy, not completed sole ownership/full status.
The five Linux configuration retirements and pending non-Linux adapters stand.
The 201 focused Coven checks, 249 Kit API checks and lint/MSRV/release/package
validation are recorded in
the [discovery table](AGENT_MANAGEMENT_MIGRATION.md#discovery-validation-table),
not added to historical OpenCode 236. No actual live installed Kit commands were
used in this documentation session. Discovery results are historical, not added to
**1,318 distinct Kit seam checks** or separate **50 Coven cross-compatibility checks**.
The [state-seam table and limits](AGENT_MANAGEMENT_MIGRATION.md#state-seam-validation-table)
record completed scoped validation, strict all-target lint, final confirmation
reruns, ENOSPC-aborted workspace CLI selection, zero-match tab filter and reproduced
stale golden/test-only corrections. No full-suite or new production UI behavior
is claimed beyond intended evidence hardening.

Use a small, versioned, capability-described external boundary. Kitsune now has
additive `pane.annotate` and optional snapshot `annotations` capability/epoch
discovery (binary protocol 18 unchanged). Broader future interfaces remain
undecided. The initial observation bridge uses the existing Kitsune API; see
[KITSUNE_OBSERVATION_CONTRACT.md](KITSUNE_OBSERVATION_CONTRACT.md).
An opt-in [Djinn process-family shadow slice](KITSUNE_PROCESS_OBSERVATION_CONTRACT.md)
interprets generic foreground-process evidence in Coven, without using Kitsune's
agent classification. It establishes neither active-agent nor native lifecycle
identity and does not replace built-in detection. Explicit
`group add --process-evidence` now consumes fresh bracketed process evidence for a participant
binding, without requiring inventory membership or importing a saved observation.
Default snapshot group binding is unchanged; neither mode grants native actions.
Start with discovery/status observations, not a universal
execution API. Djinn is the first runtime adapter, not the required native model
for all agents. Future runtimes expose supported capabilities; missing guarantees
remain unsupported/unknown rather than being falsely normalized.

The separate [interactive start/resume contract](INTERACTIVE_SESSION_CONTRACT.md)
now composes Djinn UI requests in Coven. Preview is the default; explicit execution
uses a pinned generic `pane.submit_command` API and the existing workspace transport
gate. This is unrestricted interactive command input, not native worker admission,
verified session identity, startup/readiness or task completion. Legacy commands
remain available and approved-launch mode cannot be bypassed.

The separate [pure resume planner](SESSION_RESUME_PLAN_CONTRACT.md) characterizes
16 historical provider argv mappings in Coven from explicitly supplied unverified
references. It performs no IO or execution and deliberately hardens validation.
It neither widens Djinn-only interactive execution nor replaces Kitsune's restore,
reference ingestion or built-in policy. The independent Claude hook cut above is
not a consequence of planner equivalence. Optional compatibility delegation
and per-runtime/cross-platform gates still precede any removal.

[Independent extraction characterization](KITSUNE_EXTRACTION_CHARACTERIZATION.md)
now records process classification/Windows selection and native-reference policy
from actual pinned source. Windows acquisition already filters by agent policy;
the current payload cannot support moving that selection into Coven. Actual
restore/state sequencing remains a separate executable gate. These development-only
corpora add no live integration or replacement guarantees.

A local preserving Kitsune refactor now separates injected path resolution and
neutral Windows tree selection internally. Source-subset replay matches all 209
baseline cases; compatible Zig/pinned Rust now also pass 145 focused real Linux
application tests and strict lint. Logical-authority release, metadata-only restore
and deferred-start guard characterization preserve current quirks rather than
creating new authority. Runtime import/failure/rollback and global duplicate
traversal remain separate gates.
Full-suite and Windows/other-platform validation remain outstanding. No production
API, evidence semantics, external
delegation, configuration or dependency direction changes follow.

The separate [session-target inspection contract](SESSION_INSPECTION_CONTRACT.md)
adds bounded read-only list/inspect commands over generic Kitsune pane/process
evidence, joined to existing Coven declarations by endpoint/terminal. No legacy
agent inventory or live native artifact is interpreted, no new registry is created,
and native lifecycle/readiness/activity/identity remain unestablished or unknown.
Stale/changeable declarations never become native facts or control permissions.

Pure group/member/recognition views and a bounded foreground observation watch
compose on top of those inspections. Selection cannot bypass the scanner's
cohort/budget or turn failed observations into empty successes. Watch emits each
fresh unavailable/unknown outcome rather than retaining old positive data; it
adds no daemon, stored roster, role grant, native control or activity authority.

The separate [native interactive prompt contract](SESSION_PROMPT_CONTRACT.md)
adds explicit preview/execute against an opted-in Djinn-native socket. Native
runtime/session pins replace pane identity; message acceptance is separate from
worker-policy admission or completion. No Kitsune agent command, raw keystroke,
V2 drain, permission inheritance, retry or default agent/model selection follows.

The [structured handoff capture](HANDOFF_CONTRACT.md) is Coven-owned local
evidence: explicit worker claims bound to collection-time task context, not native
execution facts, verified check/source receipts or task completion. It archives
selected data without following references, probing Git/native apps, dispatching,
releasing attempts, changing approvals or creating another task authority.

Keep these concepts distinct:

- native runtime/session identity, scoped to its runtime;
- Coven participant identity and role/assignment;
- job/task-group identity and membership;
- optional presentation references such as multiplexer/pane locators.

A pane reference cannot be the sole agent identity. Do not assume a one-to-one
pane/session mapping. Generic Kitsune decoration may carry a badge, label, group
label, and freshness; its core must not acquire Djinn/Coven-specific types.
Read-only display grants no permission to launch, send input, resume, or stop.

### Separate generic annotations: first production slice

Kitsune's [annotation guide](../kitsune/ANNOTATIONS.md) documents manual source
opt-in, snapshot discovery, and pinned API/CLI writes (optional sibling navigation,
not a dependency). Generic build/test/coordinator producers can submit bounded,
revisioned, TTL-limited text for existing terminals. Coven now has an explicit
foreground group publisher; see [GROUPS_CONTRACT.md](GROUPS_CONTRACT.md).
Source order controls priority; clear/expiry retains watermarks without fallback.
Source labels are not authentication: any existing local API caller can claim an
enabled source.

Kitsune/user own layout, rendering, clipping, and quotas. Text only appends in spare
existing top-border cells, preserving manual titles and terminal space. Single-pane
layouts have no border; disabling borders hides labels. No panels, widgets,
workspace annotations, Kitsune-owned group membership/agent identity,
seen/notifications, native success facts, or terminal-control authority are added.
Coven owns its explicit group/source declarations and read-only busy/unknown
projection; broader annotation ambitions, native lifecycle/scheduling, automatic
following, and extraction remain open. The default static publisher never opens
attachments and keeps its group/role-only 6000 ms TTL. Explicit `publish --live`
requires generic `annotations.valid_until_unix_ms: true` discovery and deadline
enforcement after server queueing. Only fresh pinned legacy-runner busy gains a
scoped suffix, with decreasing source-age/wall/elapsed ceilings and no receipt TTL
renewal; unknown is role-only. The composed frame expires together, without a hidden
static layer or lower-source fallback. Compatible Unix clocks and normal monotonic
progression are assumed, not arbitrary rollback/skew/suspend or pixel-expiry safety.
This is not Coven taking over the UI.

### Agent comparison and extraction gates

Native `coven observe kitsune --compare-agents --socket ABS_SOCKET` now compares
strict built-in snapshot hints with Coven's generic Djinn-family process evidence.
It is opt-in, conflicts with `--process-pane`, and leaves existing modes unchanged.
One shared deadline/budget bounds N+2 reads over at most 64 panes; no bindings,
workspace writes, input, adoption or built-in disabling follow. Matching hints do
not establish active agents, native identity/readiness or replacement equivalence.
Read requests can still trigger existing server housekeeping.

See [the comparison contract](KITSUNE_AGENT_COMPARISON_CONTRACT.md) for exact
categories, private legacy-change detection, provenance and the extraction matrix.
Panes/process evidence, command submission and terminal annotations already have
generic surfaces. Status/attention authority, native references/automatic restore,
provider resume/remaining hook installers and Windows agent-aware process selection remain
extraction blockers. No removal until characterized per runtime and cross-platform.
Next: generic process-selection characterization and pure provider policy behind
compatibility wrappers, not a scheduler. Broader workspace annotations remain proposed.

## Data Flow

These are independent paths, not a required round trip:

```text
observation: Kitsune -> Coven adapter -> sourced observations
annotation: user-enabled producer -> pane.annotate -> Kitsune terminal border
integration views: independent apps -> per-source frames -> local app selector/rows -> per-source intents
Claude/Codex/Copilot receipts: explicit native hooks -> Coven report -> private receiver/store -> redacted inventory -> optional generic view
Pi references: explicit extension -> Coven report-extension -> same receiver/store/inventory -> optional generic view
group status: explicit participant/source declaration -> pinned local artifact -> Coven ephemeral busy/unknown rows
```

Process binding is an explicit local declaration, not automatic enrollment:
`group add --process-evidence` brackets `pane.process_info` with snapshots and stores
canonical PID/family/rule matches. Successful process add/rebind atomically promotes
the group authority from schema 1 (snapshot only) to schema 2 (mixed bindings),
without reducing schema 3.
This is an explicit minimum-reader-version opt-in: older readers reject schema 2
and fail closed. UUIDs/counters and `groups-v1` token format/CAS remain intact; that prefix
is not an authority version. Removal or snapshot rebind never downgrades schema 2.

Explicit `group attach-live` freshly reads a pinned source outside the workspace
lock, then always fences the mutation with the preflight content token and
participant UUID under lock. First success atomically promotes the same authority
to schema 3, normalizing every member's `live_attachment` to null before setting
the target; IDs/bindings/counters remain intact. Older readers fail closed, reads
never migrate, detach never downgrades, and no sample/activity/TTL is stored.
Detach performs no artifact I/O or native deletion/control. Changed stable terminal
bindings through `group add --rebind` clear attachments atomically; role, timestamp
and pane-only changes preserve them. Source and terminal bindings remain separate.

`group status` takes no locks and writes nothing: only groups and declared sources
are read, at most 16 members serially within a shared acceptance deadline of at
most one second. A second groups read fences the whole content token; counter-only
updates are allowed. Drift/recheck/deadline failure discards all rows. All row
lifetimes are conservatively reduced after all I/O and the recheck, and callers
must deduct delivery/display time. This is revision-scoped, not a linearizable
native snapshot. Shared pins do not prove distinct workers. Nothing touches other
workspace-state health, readiness, persisted observation, startup or execution.

Before any reservation/write, publication resolves current panes and reobserves
every process member: one initial snapshot + three reads per process member under
one 3-second cycle deadline and aggregate 1 MiB read budget. Missing, unrecognized
or changed evidence stops publication without fallback/rebind. Snapshot-only and
generic annotation-write behavior, including 6000 ms TTL, remain unchanged.

Process reads use a generic pane-locator projection, separate from unchanged strict
legacy snapshot validation. All process brackets are generic, including mixed-group
brackets. A publication cycle's initial snapshot is generic only for pure-process
groups; mixed and snapshot-only groups retain strict legacy validation and
snapshot-member agent/native-hint checks. Validated member provenance selects the
contract before parsing, with no strict-to-generic fallback. Projection ignores
legacy metadata semantics, not JSON safety: the whole response must pass shared
decoding/bounds before filtering. Process observation/add need no annotation opt-in;
publication retains strict descriptor-presence, enabled-source and epoch checks
(absent descriptor is unsupported; present null is invalid).

Coven-owned participant grouping/display roles and explicit label publication are
implemented; native group lifecycle/progress interpretation and scheduling remain open.
Native facts remain runtime authority.
Observed activity is not job acceptance; pane presence is not success; screen
matches are heuristic evidence, not lifecycle truth. Coven owns job progress and
acceptance interpretation without overwriting native execution facts.

## Edge Cases

- Missing, unreachable, or incompatible Coven must not break Kitsune startup or
  terminal work, or Djinn's native operation. Hide stale badges or mark them
  unknown; never retain stale active status as current truth.
- Invalid annotation writes are rejected without replacing accepted content.
  Timer-driven expiry hides labels even on quiet terminals; disconnect is not
  immediately detected, so accepted text can last until TTL. Source conflicts
  cannot grant terminal control or agent/group authority.
- Default observation requests only `session.snapshot`; the explicit process-pane
   shadow mode, explicit comparison and explicit process group binding also request
   `pane.process_info`. Existing Kitsune
  dispatch housekeeping can expire metadata, drain events, sync titles, or create
  a default workspace (potentially spawning a shell). Zero server-side effects
  are not promised. Snapshot receipt does not establish server freshness.
- Process bindings ignore Kitsune native-reference/classification metadata, even
  absent or semantically malformed legacy fields on generic reads. Mixed groups'
  initial snapshots still require the legacy agent shape. Only observation
  time and pane-at-binding are excluded from stable binding comparison; canonical
  match ordering is not meaningful. Other evidence changes require explicit
  `--rebind`. Stable same-terminal moves are allowed, but PID reuse/ABA remains a
  caveat. Djinn-family evidence includes help/maintenance, not active-agent truth.
- Coven must be usable headless or with other multiplexers; a missing pane does
  not erase an agent/session or prove completion. This is a target, not a claim
  that every current workflow already supports headless operation.
- Execution, cancellation, and other effects require separately supported,
  explicitly authorized adapters. Observation/grouping needs no scheduler,
  daemon, plugin framework, or native admission design as a prerequisite.
- Existing storage, checkpoint, launch, task-ledger, graph, and preparation safety
  contracts remain valid. No automatic migration, replay authority, approval,
  dispatch, or execution of old preparations follows from this decision.

## Example usage

Current workflow: a user groups already-running participants from Kitsune's inventory in
Coven and assigns display roles, with optional native references (not fresh identity
or lifecycle verification). The explicit foreground publisher sends generic group/role
labels; only the Kitsune presentation adapter is supported in v1. See the
[quick start](GROUPS_CONTRACT.md#quick-start-existing-agents-separate-orchestration-workspace).
For inventory-independent binding to one current pane, explicitly opt in:

```sh
coven group add WORKSPACE auth-review --member backend --role Investigator \
  --socket ABS --pane w18:p85 --process-evidence
```

Use an existing Coven workspace, an explicit absolute API socket, and an inspected
current pane; add `--rebind` for changed stable evidence and optionally
`--expect-revision '<current group token>'`. No execution/identity grant follows.
Disconnecting Coven leaves terminals
usable; accepted annotations expire by TTL, not immediate disconnect detection.
No session is started or cancelled by grouping. The working manual build-label
example is also in the [Kitsune guide](../kitsune/ANNOTATIONS.md).

## Transition

1. Inspect Kitsune's existing agent-related behavior and boundaries.
2. Characterize that behavior and its protected compatibility before moving it.
3. Initial single-endpoint read-only observation is implemented in Coven; built-in
   interpretation remains in Kitsune. Characterize generic surfaces before extraction.
4. First optional generic terminal-border annotation slice implemented in Kitsune;
   explicit Coven foreground publishing implemented; broader workspace/presentation work remains open.
5. Explicit groups/roles for existing agents implemented in Coven before further
   worker/worktree implementation. Native lifecycle/scheduling,
   automatic following, and extraction are not shipped.
6. One-pane generic process evidence and Coven-owned Djinn executable-family
   interpretation are implemented as an opt-in shadow mode. Recognition is not
   active-agent detection. Explicit process-evidence group add/rebind is now
   implemented; built-in removal, automatic adoption and native actions are not.
7. Explicit live-source attach/detach and read-only group busy/unknown status are
   implemented in Coven; source declarations do not verify native/UI-session or
   terminal equivalence, authorize work, or change the static annotation publisher.
8. Explicit `publish --live` composes fresh scoped busy using generic Kitsune absolute
   expiry discovery, whole-target durable reservations and decreasing source budgets.
   Static defaults stay unchanged; unsupported servers fail closed. Offline fixtures
   are not actual Kitsune enforcement or live-runtime validation.

9. Native opt-in agent comparison is implemented as characterization, not completed
   extraction. Built-ins remain; replacement equivalence is not established. The
   [comparison contract](KITSUNE_AGENT_COMPARISON_CONTRACT.md) records the scoped
   parent validation (10 unit, 29 workflow, 21 existing observer passes and MSRV 1.85),
   source provenance and remaining extraction gates; no real endpoint/UI/provider
   calls or full-CI result are claimed. This update remains local and uncommitted.

The historical offline suite passed **763 tests**, including 33 opt-in live-publisher
tests, 24 live-attachment authority tests and 34 workflow/status tests alongside
the existing observer, publisher and transport coverage. The separate live producer passed
54 focused opencode tests, 3 schema tests and both Bun typechecks, not the full UI
suite. The [live observer contract](DJINN_LIVE_OBSERVATION_CONTRACT.md#example-usage)
records cross-TS→Python fixture interop; no real UI/model calls have validated that
slice. New group attachment/status validation used private fixtures and the real
CLI wrapper, without modifying existing declarations or native applications.
The separate persisted native observer passed eight compiled
producer/consumer fixture cases; it does not verify running agents or change native
authorization. [Live group validation](GROUPS_LIVE_VALIDATION.md) confirms installed Linux
binding/publication, exit/TTL and explicit rebind behavior, not native lifecycle.
Those historical runs used valid real-server snapshots, not the artificial
malformed-legacy payloads covered offline in this slice.
Validation cases and limits are tracked in
[the group contract](GROUPS_CONTRACT.md#validation).

The older worker/composer backlog is deferred, not invalidated. Native execution
contracts remain necessary for future execution, not for observing existing agents.

## Related files

- [README.md](README.md) — current behavior and existing safety contracts.
- [KITSUNE_AGENT_COMPARISON_CONTRACT.md](KITSUNE_AGENT_COMPARISON_CONTRACT.md) — native bounded comparison and extraction decision matrix.
- [GROUP_LIVE_STATUS_CONTRACT.md](GROUP_LIVE_STATUS_CONTRACT.md) — schema-3 declarations, status expiry and no-authority boundaries.
- [ROADMAP.md](ROADMAP.md) — longer-term orchestration backlog.
- [CODING_ORCHESTRATION_ROADMAP.md](CODING_ORCHESTRATION_ROADMAP.md) — coding backlog.
- Optional developer navigation: [Kitsune decision](../kitsune/APP_BOUNDARIES.md),
  [Djinn decision](../djinn/APP_BOUNDARIES.md). Sibling checkouts are not required.
