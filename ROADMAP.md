# Coven Roadmap

## Priority rebaseline — 2026-10-22

[APP_BOUNDARIES.md](APP_BOUNDARIES.md) is the accepted target boundary. Initial
single-endpoint Kitsune observation is implemented in Coven only; see
[the observation contract](KITSUNE_OBSERVATION_CONTRACT.md). Kitsune's built-ins
remain. Next: characterize generic surfaces before extraction -> design separate,
user-enabled generic annotations -> Coven grouping of existing sessions. Do this before further
worker/worktree implementation. No scheduler, daemon, plugin framework, or native
admission design is prerequisite for observation/grouping.

Annotations target bounded badges/labels/group hints/progress on existing
panes/workspaces, useful for build/test/export services as well as coordinators.
Kitsune/user own space, layout, rendering, clipping, and quotas; source-owned values
need conflict policy/expiry and hidden/unknown failure handling, not stale active
status. No arbitrary panels, embedded apps/scripts, UI takeover, new external
schema, or return endpoint is implemented; display conveys no execution authority.

The worker/composer stories, numbered slices, and execution contracts below are
deferred backlog, not invalidated or current implementation order. Djinn is the
first adapter, not a required universal native agent model; future adapters expose
capabilities without invented guarantees. Coven can work headless or with other
multiplexers. Kitsune still has built-in agent integrations during transition.
Observation is not acceptance or permission to execute/cancel. Existing safety
contracts remain in force; old preparations cannot automatically execute.
Event/replay sketches below do not turn current observational logs into authority;
existing explicit task-ledger and preparation contracts retain their own scope.

This roadmap is the forward-looking work queue for Coven as the multi-agent
orchestration layer. Keep implemented behavior in `README.md` and generated
workspace docs; keep this file focused on product stories, architecture direction,
and actionable future slices.

## Product north star

As a user, I should be able to give Coven a broad goal and trust the lead agent to
notice which parts can be done independently, delegate those parts to worker
agents, monitor progress, then synthesize the results into one coherent answer or
plan.

The ideal user experience is not “manually start a child session.” It is:

1. I describe a goal or list of tasks.
2. The lead agent proposes a parallelizable task graph when useful.
3. I approve, edit, or decline the plan.
4. Coven launches workers through the appropriate harnesses.
5. Coven tracks who is doing what, what state each task is in, and where the
   native transcript/result lives.
6. The lead agent reads worker summaries and produces the final synthesis.

Example goal:

> Audit the agent chat UX. Look at scrolling/copying, tool rendering, lifecycle
> status, and auth error handling. Tell me what to fix first.

Expected orchestration:

- Worker A inspects scrolling/copying UX.
- Worker B inspects tool rendering.
- Worker C inspects lifecycle/session status.
- Worker D inspects auth/provider failure UX.
- The lead ranks findings, identifies conflicts, and writes the final answer.

## Boundary with Djinn

Coven should own orchestration. Djinn should remain a local agent runtime and
worker-session provider.

Coven owns:

- the lead agent and planning loop;
- task decomposition and task graph state;
- worker roster and scheduling;
- concurrency limits and dependency ordering;
- cross-harness event ledger and recovery projection;
- worker result collection;
- final synthesis and dashboard/checkpoint state;
- presentation/layout hints for Herdr, tmux, or other surfaces.

Djinn owns:

- Djinn-native session JSONL transcripts;
- local tools and policy enforcement;
- provider/model execution;
- runtime lifecycle facts for Djinn sessions;
- scoped permission checks and grant records;
- worker summaries/artifacts produced by Djinn sessions.

The integration should use a federated source-of-truth model: Coven tracks the
orchestration graph and result pointers; Djinn tracks exactly what happened inside
each Djinn session.

## Primary user story

As a developer using Coven, I want the lead agent to decompose a broad request
into safe parallel worker tasks, run those tasks, and merge worker findings into a
single response, so I can benefit from parallel agent work without manually
managing child sessions or transcript merging.

Acceptance criteria for the first complete story:

- The lead can propose a parallel task plan before launching workers.
- The user can approve, edit, run serially, or cancel the plan.
- Coven launches multiple read-only worker tasks through Djinn.
- Worker state is visible in `dashboard.md`, `standup`, and event logs.
- Each worker produces a short structured result with links to its native
  transcript.
- The lead synthesizes worker results and cites which worker supplied each major
  finding.
- Worker transcripts are not blindly merged into the lead transcript; summaries
  and pointers are used first.

## Safety model

Start with read-only/analysis-only auto-delegation. Mutation work should require a
separate approval boundary because parallel editors can conflict or create unsafe
changes.

Initial policy:

- Auto-delegation may be enabled for read-only investigation tasks.
- Code-changing workers require explicit user approval of the mutation plan.
- Parent/lead approvals do not implicitly transfer to workers.
- Any permission grant from the lead/user to a worker must be scoped to the worker
  task, action, workspace, and resource/path.
- Coven should cap active workers per orchestration and per workspace before
  autonomous fan-out is allowed.
- The lead must synthesize results; workers should not directly mutate the final
  answer without lead review.

## Orchestration model

Coven’s file-backed workspace remains the core control plane:

```text
coven workspace
├── coven.json / coven.toml          # workspace and runtime config
├── goal.md                          # user goal
├── dashboard.md                     # human projection
├── state/
│   ├── tasks.json                   # task graph projection
│   ├── agents.json                  # worker/lead roster projection
│   └── lead.json                    # active lead
├── logs/
│   ├── events.jsonl                 # append-only orchestration ledger
│   └── messages.jsonl               # user/agent control messages
├── work/                            # intermediate artifacts
└── output/                          # synthesized outputs
```

The event log should describe orchestration facts, not replicate every native
agent transcript. Native runtimes provide transcript/result pointers.

Important event categories:

- `plan.proposed`, `plan.approved`, `plan.rejected`, `plan.edited`
- `task.created`, `task.started`, `task.blocked`, `task.completed`, `task.failed`
- `worker.requested`, `worker.started`, `worker.paused`, `worker.completed`,
  `worker.failed`, `worker.cancelled`
- `worker.result.available`
- `synthesis.started`, `synthesis.completed`, `synthesis.failed`
- `policy.grant.requested`, `policy.grant.approved`, `policy.grant.rejected`

## Worker result contract

Each worker should return a compact result intended for synthesis:

```json
{
  "task_id": "audit-tool-rendering",
  "status": "completed",
  "summary": "Tool output is readable, but long shell output needs stronger collapse affordances.",
  "findings": [
    {
      "priority": "high",
      "claim": "Long shell output can dominate the transcript.",
      "evidence": ["djinn://sessions/agt_...#tool-call-3"]
    }
  ],
  "files_inspected": ["crates/djinn-tui/src/lib.rs"],
  "files_changed": [],
  "confidence": "medium",
  "follow_ups": ["Add a transcript-level collapse-all command."],
  "transcript_uri": "djinn://sessions/agt_..."
}
```

The lead should consume this result, not raw full transcripts by default.

## Djinn worker-readiness backlog

These are Djinn-side capabilities that Coven should be able to rely on when it
uses Djinn as a worker runtime. Track them here because they exist to support the
Coven orchestration story, even if the implementation lands in Djinn.

### Coven metadata on Djinn worker sessions

Djinn worker sessions need stable metadata or metadata events that Coven can use
to map a native Djinn session back to the orchestration graph:

- `orchestration_id`: Coven workspace/run id.
- `task_id`: Coven task graph node id.
- `attempt_id`: stable execution intent/correlation id; retrying an ambiguous
  submission retains it, while a new execution attempt gets a new id.
- `coven_agent_id`: lead/worker roster id, e.g. `reviewer-1`.
- `parent_ref`: optional parent lead/session reference.
- `execution_mode`: foreground/background/headless presentation hint.
- `transcript_uri`: stable pointer such as `djinn://sessions/<id>`.
- `result_uri`: pointer to the structured worker result artifact when available.

Acceptance criteria:

- Coven can launch a Djinn worker and later recover the mapping from Coven task id
  to Djinn native session id.
- A user can inspect either side and see the cross-reference.
- Multiplexer ids stay presentation metadata, not identity truth.

### Coven-facing Djinn worker command

Before implementing execution, establish a supported native contract for stable
attempt/session identity, accepted immutable tool/resource policy, acknowledged
submission with lost-ack reconciliation, non-mutating lifecycle inspection,
cancellation, and bound result/transcript pointers. Existing folder-session
helpers, process spawn output, and role names are not equivalent guarantees.
Coven's help-only compatibility inspector reports observations and blockers,
not worker readiness or authorization; see `WORKER_READINESS_CONTRACT.md`.

The ideal adapter surface is a single command Coven can call for one worker task,
not a chain of manual child-session commands. Shape to refine during
implementation:

```bash
djinn agent worker run \
  --orchestration-id <id> \
  --task-id <task-id> \
  --coven-agent-id <worker-id> \
  --parent-session <optional-djinn-parent> \
  --workspace <project-root> \
  --agent <djinn-agent-role> \
  --mode background \
  --result-uri <file://...> \
  --prompt-file <prompt.md> \
  --json
```

Responsibilities:

- create a normal Djinn agent session;
- attach Coven orchestration metadata;
- run the prompt with Djinn tools/policy;
- write Djinn lifecycle events;
- produce a worker result artifact or summary event;
- return JSON with `session_id`, lifecycle state, transcript URI, result URI, and
  any failure details.

### Bridge-friendly lifecycle/result facts

Djinn should expose facts Coven can mirror into `logs/events.jsonl` without
copying the native transcript:

- worker/session accepted or rejected;
- session created with native id and transcript URI;
- lifecycle changed: running, paused, completed, failed, cancelled;
- output/result available with result URI;
- permission grant applied/rejected;
- worker summary finalized.

These can initially be JSON command output. A file-backed watch or local socket
can come later if polling/command invocation is too clumsy.

### Worker summary generation

Djinn should produce a compact synthesis artifact for Coven workers. It can be a
`Summary` JSONL event plus a file in the Coven workspace, but Coven should receive
the result through a stable pointer and a small JSON envelope.

First summary format should include:

- status;
- one-paragraph summary;
- prioritized findings;
- evidence pointers to transcript/tool events;
- files inspected;
- files changed;
- confidence;
- recommended follow-ups.

### Scoped grant record

When Coven delegates work that needs permission, the grant needs an inspectable
record that does not silently become durable Djinn config:

- orchestration id;
- task id;
- parent/lead id;
- Djinn worker session id;
- action;
- resource/path;
- effect;
- grant source;
- expiry/session scope.

This supports future mutation delegation while keeping the first auto-delegation
slice read-only.

## Roadmap slices

### 1. Lead decomposition proposal

- Add a lead prompt/procedure for detecting independent subtasks.
- Represent a proposed task graph in `state/tasks.json` and `logs/events.jsonl`.
- Render proposed tasks in `dashboard.md` and `coven standup`.
- Add approval flow: run plan, edit plan, run serially, cancel.

### 2. Djinn worker adapter MVP

- Add a Coven adapter that can start a Djinn worker session for one task.
- Pass parent/orchestration/task metadata to Djinn so sessions can be traced back
  to Coven.
- Store Djinn native session id and transcript URI in the Coven task/worker state.
- Treat current Djinn manual child/session commands as adapter plumbing, not the
  primary user UX.

### 3. Worker lifecycle projection

- Mirror selected Djinn lifecycle facts into Coven events.
- Show worker state in dashboard/standup: queued, running, paused, completed,
  failed, cancelled.
- Add recovery by replaying Coven events and verifying Djinn session pointers.

### 4. Worker summaries and result pointers

- Require each worker to emit a short structured result artifact under `work/` or
  `output/`.
- Include transcript URI, files inspected/changed, confidence, and follow-ups.
- Keep raw transcript import explicit.

### 5. Lead synthesis

- Add a lead synthesis prompt that consumes worker result artifacts.
- Produce one final response/plan with citations to worker ids and transcript
  pointers.
- Identify conflicts between workers and ask for a follow-up task when needed.

### 6. Concurrency and safety controls

- Add per-orchestration and per-workspace active worker caps.
- Separate read-only auto-delegation from mutation delegation.
- Require explicit approval for parallel mutation plans and scoped grants.
- Prevent overlapping file mutation without lead/user approval.

### 7. Presentation adapters

- Keep core orchestration headless/file-backed.
- Let Herdr/tmux adapters open panes/tabs for workers when useful.
- Store presentation refs as hints and native adapter ids, not as session truth.

## Non-goals for the near term

- Do not make Djinn the orchestration brain.
- Do not merge full worker transcripts into the lead by default.
- Do not auto-delegate mutation work without an explicit approval boundary.
- Do not require Herdr/tmux for orchestration correctness.
- Do not build a database-backed scheduler until file-backed state shows real
  limits.
