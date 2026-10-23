# App boundaries: Coven, Kitsune, and Djinn

## Overview

Decision date: **2026-10-22**. Status: **accepted target; initial read-only
observation implemented in Coven only; extraction, annotations, and grouping not shipped**.
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

Use a small, versioned, capability-described external boundary. Transport and
exact future interface shapes are undecided; no new external schema or annotation
return endpoint is implemented. The initial observation bridge uses the existing
Kitsune API; see [KITSUNE_OBSERVATION_CONTRACT.md](KITSUNE_OBSERVATION_CONTRACT.md).
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

### Separate generic annotation target (not implemented)

Users explicitly opt into each annotation source. Services may annotate **existing
panes/workspaces** with bounded text badges, labels, group hints, or progress;
an agent coordinator, build, test, or export service are equally useful producers.
This is not Coven taking over the UI: no arbitrary panels, embedded apps, scripts,
or unrequested UI space. Kitsune and the user own layout, space allocation,
rendering, clipping, and quotas. Values are source-owned, with explicit conflict
policy and expiry; labels confer neither agent identity nor group membership.
Use generic multiplexer resource vocabulary, never Coven/Djinn core types.
No plugin framework, daemon, new external schema, or return endpoint is shipped.

## Data Flow

These are independent paths, not a required round trip:

```text
observation: Kitsune -> Coven adapter -> sourced observations
annotation (future): user-enabled producer -> generic annotations -> Kitsune
```

Coven-owned grouping/assignment and progress interpretation remain future work.
Native facts remain runtime authority.
Observed activity is not job acceptance; pane presence is not success; screen
matches are heuristic evidence, not lifecycle truth. Coven owns job progress and
acceptance interpretation without overwriting native execution facts.

## Edge Cases

- Missing, unreachable, or incompatible Coven must not break Kitsune startup or
  terminal work, or Djinn's native operation. Hide stale badges or mark them
  unknown; never retain stale active status as current truth.
- Absent, malformed, incompatible, expired, or disconnected annotation input is
  hidden/unknown without loss of terminal usability; source conflicts cannot grant
  terminal control or agent/group authority.
- The observation adapter requests only `session.snapshot`, but existing Kitsune
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

Target scenario, not new CLI syntax: a user groups already-running Djinn and
other-runtime sessions into a review job in Coven and assigns participant roles.
Coven records native references and sourced observations; Kitsune optionally
shows a generic group label and fresh badge. Disconnecting Coven leaves terminals
usable and badges hidden/unknown. No session is started or cancelled by grouping.

## Transition

1. Inspect Kitsune's existing agent-related behavior and boundaries.
2. Characterize that behavior and its protected compatibility before moving it.
3. Initial single-endpoint read-only observation is implemented in Coven; built-in
   interpretation remains in Kitsune. Characterize generic surfaces before extraction.
4. Design separate optional generic annotations, with opt-in and bounded presentation.
5. Group existing sessions in Coven before further worker/worktree implementation.

The older worker/composer backlog is deferred, not invalidated. Native execution
contracts remain necessary for future execution, not for observing existing agents.

## Related files

- [README.md](README.md) — current behavior and existing safety contracts.
- [ROADMAP.md](ROADMAP.md) — longer-term orchestration backlog.
- [CODING_ORCHESTRATION_ROADMAP.md](CODING_ORCHESTRATION_ROADMAP.md) — coding backlog.
- Optional developer navigation: [Kitsune decision](../kitsune/APP_BOUNDARIES.md),
  [Djinn decision](../djinn/APP_BOUNDARIES.md). Sibling checkouts are not required.
