# App boundaries: Coven, Kitsune, and Djinn

## Overview

Decision date: **2026-10-22**. Status: **accepted target; initial read-only
observation implemented in Coven; production opt-in terminal-border annotations
implemented in Kitsune; explicit participant groups/roles and a foreground publisher
implemented in Coven, including explicit process-evidence group binding independent
of Kitsune's agent inventory; native group status/scheduling, automatic following, extraction,
and workspace annotations remain open**.
Keep three independently useful apps, not one tightly coupled agent stack.
This decision rebaselines near-term priorities; it does not remove current features.

| App | Target responsibility |
| --- | --- |
| Kitsune | General multiplexer: PTY/process/terminal/session/pane operations and generic label/badge rendering. |
| Coven | Cross-runtime agent discovery, observations/provenance, job/task-group membership, participant roles/assignments, orchestration, and job progress. |
| Djinn | Djinn-native agent/session execution, configuration, tools, policy, transcripts, and results. |

Agent-specific discovery, status interpretation, start/resume, and job concepts
should move out of Kitsune into optional Coven integration. Target: without
Coven, Kitsune is a fully functional multiplexer with no agent-aware features.
Kitsune **currently still has built-in agent detection and integrations**; this
decision neither removes them immediately nor claims extraction is complete.

## API / Interfaces

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
Coven owns its explicit group declarations; broader annotation ambitions, native
group status/scheduling, automatic following, and extraction remain open.
This is not Coven taking over the UI.

## Data Flow

These are independent paths, not a required round trip:

```text
observation: Kitsune -> Coven adapter -> sourced observations
annotation: user-enabled producer -> pane.annotate -> Kitsune terminal border
```

Process binding is an explicit local declaration, not automatic enrollment:
`group add --process-evidence` brackets `pane.process_info` with snapshots and stores
canonical PID/family/rule matches. Successful process add/rebind atomically promotes
the group authority from schema 1 (snapshot only) to schema 2 (mixed bindings).
This is an explicit minimum-reader-version opt-in: older readers reject schema 2
and fail closed. UUIDs/counters and `groups-v1` token format/CAS remain intact; that prefix
is not an authority version. Removal or snapshot rebind never downgrades schema 2.

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
  shadow mode and explicit process group binding also request `pane.process_info`. Existing Kitsune
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
    worker/worktree implementation. Native group status/scheduling, automatic following,
    and extraction are not shipped.
6. One-pane generic process evidence and Coven-owned Djinn executable-family
   interpretation are implemented as an opt-in shadow mode. Recognition is not
   active-agent detection. Explicit process-evidence group add/rebind is now
   implemented; built-in removal, automatic adoption and native actions are not.

The full offline suite now passes **645 tests**, including 28 native-observation
tests, the existing 107 group tests, 10 socket-alias regressions and 18 generic
snapshot contract test methods. The native observer also passed eight compiled
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
- [ROADMAP.md](ROADMAP.md) — longer-term orchestration backlog.
- [CODING_ORCHESTRATION_ROADMAP.md](CODING_ORCHESTRATION_ROADMAP.md) — coding backlog.
- Optional developer navigation: [Kitsune decision](../kitsune/APP_BOUNDARIES.md),
  [Djinn decision](../djinn/APP_BOUNDARIES.md). Sibling checkouts are not required.
