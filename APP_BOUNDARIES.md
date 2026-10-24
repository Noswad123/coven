# App boundaries: Coven, Kitsune, and Djinn

## Overview

Decision date: **2026-10-22**. Status: **accepted target; initial read-only
observation implemented in Coven; production opt-in terminal-border annotations
implemented in Kitsune; explicit participant groups/roles and a foreground publisher
implemented in Coven; native group status/scheduling, automatic following, extraction,
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
identity and does not replace built-in detection or group bindings.
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
  shadow mode also requests `pane.process_info`. Existing Kitsune
  dispatch housekeeping can expire metadata, drain events, sync titles, or create
  a default workspace (potentially spawning a shell). Zero server-side effects
  are not promised. Snapshot receipt does not establish server freshness.
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

Current workflow: a user groups already-running agents from Kitsune's inventory in
Coven and assigns display roles, with optional native references (not fresh identity
or lifecycle verification). The explicit foreground publisher sends generic group/role
labels; only the Kitsune presentation adapter is supported in v1. See the
[quick start](GROUPS_CONTRACT.md#quick-start-existing-agents-separate-orchestration-workspace).
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
   active-agent detection; built-in removal and group adoption remain separate work.

The older worker/composer backlog is deferred, not invalidated. Native execution
contracts remain necessary for future execution, not for observing existing agents.

## Related files

- [README.md](README.md) — current behavior and existing safety contracts.
- [ROADMAP.md](ROADMAP.md) — longer-term orchestration backlog.
- [CODING_ORCHESTRATION_ROADMAP.md](CODING_ORCHESTRATION_ROADMAP.md) — coding backlog.
- Optional developer navigation: [Kitsune decision](../kitsune/APP_BOUNDARIES.md),
  [Djinn decision](../djinn/APP_BOUNDARIES.md). Sibling checkouts are not required.
