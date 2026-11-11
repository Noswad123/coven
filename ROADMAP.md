# Coven Roadmap

## Confirmed all-agent transfer — first functional cut

The authoritative [agent-management migration record](AGENT_MANAGEMENT_MIGRATION.md)
now owns the surface/provider matrix and next-transfer order. Scope is **all**
discovery/status interpretation, hooks, start/resume and agent restore in Coven;
Kitsune keeps generic multiplexing and optional presentation/messaging. This
supersedes any reading of the older queue as characterization-only or Claude-only.

Implemented: independent Coven Claude hook receiver/private persistence/redacted
last-reported inventory, explicit preview-first settings plan/install/uninstall/
rollback, and optional independent generic view worker. Kitsune's new Claude
installer is retired **on Linux only**, before provider/config discovery;
non-Linux production installer/assets/result type and recommendation/update/bulk
paths remain until matching Coven adapters are ready. All six target entries and
status/uninstall/old ingress remain. Receiver transport is Unix-only, Windows
unsupported and macOS unvalidated. See [receiver](AGENT_MANAGEMENT_CONTRACT.md) and
[hooks](CLAUDE_HOOKS_CONTRACT.md) contracts. No native provider/daemon starts,
automatic user migration or live-session conversion follows.

Next: add matching non-Linux Claude/Codex/Copilot/Pi/OpenCode adapters before retiring working Kit paths;
transfer remaining provider/source hook owners, discovery/status and
process/screen fallback, provider start/resume, then agent-aware restore/reference
ownership with actual runtime failure/dedupe gates. Claude fallback and restore
are **still Kitsune**. Subsequent Codex/Copilot hook and Pi/OpenCode extension cuts are
recorded below; other providers remain pending applicable transfers;
native runtime facts/execution remain native. Do not equate receipt rows with
verified activity/readiness, completion, roles or execution permission. Final
validation counts belong in the parent implementation record; no new full-suite,
installed-provider or cross-platform acceptance is asserted here.

## Priority rebaseline — 2026-10-22

### Subsequent Codex hook transfer

Implemented within the [Codex contract](CODEX_HOOKS_CONTRACT.md): offline two-file
hooks.json/config.toml management, reference-only SessionStart capture in the
existing Unix receiver/inventory, optional dynamic-provider generic view, and Linux
Kit new-install/recommend/update/bulk retirement before provider paths. Codex is no
longer in the wholly pending hook-owner group. Claude behavior,
v2 receipts/errors/statuses remain unchanged after shared filesystem extraction;
Codex uses separate v1 journaling and recovery names. Native release format is
pinned, not earliest-version/all-modes/operational certification.

Preview discloses absent/false → true global hooks and native approval not granted;
apply authorizes shared enablement, not trust. Uninstall removes Coven commands but
leaves that shared flag enabled; guarded rollback restores both original images.
Ordered journaling is not global atomicity. Next non-Linux adapter work covers
**Claude, Codex, Copilot, Pi and OpenCode** before retiring preserved Kit paths. Full discovery/status,
start/resume/restore and legacy ingress/API/UI transfer remain pending for all
providers. Earlier owner-reported 16 Codex workflows + 7 fault tests are historical
scoped evidence, not a fresh Copilot/full-suite/platform result; see the migration
record's historical 176 Coven/330 Kit validation. Final Copilot counts are parent-owned.

### Subsequent Copilot hook transfer

Implemented: [Copilot global settings.json management](COPILOT_HOOKS_CONTRACT.md),
direct `hooks.SessionStart[]` command/bash/timeoutSec-10 handlers, exact startup
reference capture in the shared Unix receiver and Linux Kit new-install/recommend/
update/bulk retirement. Scoped Linux new hook ownership is complete; non-Linux Kit
paths and all-platform detection/status/start/resume/restore/old ingress/uninstall
remain. OpenCode's subsequent reference-plugin transfer is recorded below.

Copilot uses the shared Claude single-file v2 runner with per-provider namespaces;
Codex's v1 two-file journal stays separate. Native JSONC is deliberately refused in
favor of strict duplicate-free JSON. Exact opt-in legacy migration retains the
script; normal uninstall never revives Kit, guarded rollback restores bytes/mode/
absence. Previews create no files/lock; actual apply may create the persistent
lock before validation. Native disable/approval/sandbox/trust remain unchanged.
Only `session_id` and exact `hook_event_name: SessionStart` are required, without
aliases/inference or Claude subagent suppression. Fresh Copilot records stay
unknown; schema 1 and aggregate 128-record capacity remain.

The `github/docs` pin is documentation evidence, not native implementation/binary
certification. Generated three-provider commands reach real Coven end to end;
188 focused Coven checks pass, with strict lint in both apps, MSRV 1.85,
release and package checks. Kitsune's 103 integration and 8 Copilot checks are
overlapping filters. See the [validation record](AGENT_MANAGEMENT_MIGRATION.md#validation-record),
including an intermittent receipt-deadline test failure and successful fresh-fixture reruns.

### Subsequent Pi reference-extension transfer

Implemented: [Pi explicit existing-root placement](PI_EXTENSION_CONTRACT.md),
preview-first plan/install/uninstall/rollback and separate strict Coven-envelope
`report-extension`, with Linux Kit install/recommend/update/bulk retirement.
**Migration drops working/blocked/idle to reference-only capture**, unknown even
fresh. This is not full Pi management completion or native activation proof.
Non-Linux Kit full-v7 paths and all-platform status/uninstall/old ingress/lifecycle
remain until explicit migration; discovery/start/resume/restore remain pending.

Source pin is main `6e11a00a01a568ef0ae0df5074acf02fc107c34b` declaring 1.5.2,
not a release pin. hasUI includes TUI/RPC, not root/TUI proof or headless capture.
Exact-v7 consent and a separate private two-leaf v1 journal preserve immutable
originals and guarded rollback after uninstall; normal uninstall never revives
Kit. No approval changes, atomic activation, autoload or in-memory release.
Five providers now share unchanged store-v1 fields/aggregate 128 cap; upgrade receiver
first because older binaries reject newly supported providers, limiting downgrade. The historical
[Pi validation record](AGENT_MANAGEMENT_MIGRATION.md#pi-validation-table) covers
212 focused Rust checks, generated-asset Node/Bun checks, Kit retirement checks,
lint/MSRV/build/package checks and the retained receipt timing limitation. Earlier
Copilot scopes above are historical, not additive Pi results.

### Subsequent OpenCode reference-plugin transfer

Implemented: [OpenCode explicit existing plugin/plugins root placement](OPENCODE_EXTENSION_CONTRACT.md),
strict three-string Coven envelope with matching required request/payload event,
and Linux Kit new-install/recommend/update/bulk retirement. **Migration reduces
working/blocked/idle to references only**, unknown even fresh, not full OpenCode
management completion. Non-Linux full-v9 paths, legacy CLI/status/uninstall/all six
targets/old ingress remain; discovery/status/start/resume/restore remain pending.
Canonical anomalyco/opencode release v1.19.5 commit
`810c0eba1a7348036f8bdf61e6632d0c36c6876c` is authoritative; changed dev `611cee9`
unsupported. Exact server directory-instance scope is not whole project or selected
TUI/pane/attach scope; remote servers need the executable/socket there. Quiet resumed
sessions need not emit events. Full root DTO validation is a conservative subset
with 256-entry summary.diffs/permission caps. Capacity precedes DTO work; FIFO 32
pending plus one active, incoming-full drop, no persistence/deduplication/retry.

Shared private fixed-two-leaf v1 journaling has OpenCode namespaces and protects
[frozen Pi 887451b API/receipts/generated bytes](tests/pi_extension_frozen_compatibility.rs).
Exact-v9 consent, immutable originals and rollback after uninstall remain required.
Stop servers/watchers before apply, restart native server after manual apply, not
just TUI attach; no auto-restart or in-memory release. No across-root dedupe/authority:
do not install in both plugin and plugins. The [current scoped table](AGENT_MANAGEMENT_MIGRATION.md#opencode-validation-table)
records shared 20, workflows 14, Node producer 12 with Bun harness coverage, Kit
111 integration/10 OpenCode overlapping checks and six legacy-asset harness checks.
The broader current scope passed 236 focused Rust checks plus lint/MSRV/build/
package checks; those shared/workflow counts are subsets. Recorded receipt-timing
limits remain. Earlier Pi totals are historical, not additive.
No installed-native operational or cross-platform certification is claimed.

Current delivery direction: **Coven owns management; Kitsune exposes generic
presentation, messaging and multiplexer controls.** The functional
[`group present` bridge](GROUP_PRESENTATION_CONTRACT.md) now drives a persistent
Integrations block and read-only selection roundtrip from Coven's own declarations.
It is a generic multi-app UI, not a Coven-only agent panel: manual
`[integrations] sources = ["coven", "build"]` enables up to eight independent
providers, and a local vertical selector chooses whose label/detail rows to show.
Coven finds its source anywhere in whole-list `snapshot.integration_views` discovery
and opens only its own `integration.view.*` lease. Successful replace/poll renews
its five-second TTL; close/expiry/removal affects only that source, and reorder
preserves retained leases/queues/selection/scroll. Empty config shows
`No integrations configured`, never native-agent fallback. Sources/local selection
are routing, not auth; at-most-once row intents are not execution grants or pixel acks.
Next: extend Coven-owned runtime/status/action capabilities through that generic
boundary, then retire matching legacy Kitsune management. Characterization remains
a compatibility/removal gate, not the sole forward milestone. Do not add another
agent-specific registry, sidebar model or execution policy to Kitsune.

The generic-view step replaces default desktop lower-agent presentation only. Agent APIs/detection,
mobile UI, native hooks, restore/resume and fixed-target legacy hook installers
remain separate migration work except the Linux new Claude/Codex/Copilot hook and Pi/OpenCode extension installers retired above;
see the [scope matrix](AGENT_MANAGEMENT_MIGRATION.md#api--interfaces).
The obsolete local singleton config is diagnosed/ignored, not auto-rewritten;
there are no old-method aliases, broker, plugins or scripts. Keep the independent
characterization records below as removal evidence, not the permanent forward queue.

[APP_BOUNDARIES.md](APP_BOUNDARIES.md) is the accepted target boundary. Initial
single-endpoint Kitsune observation is implemented in Coven only; see
[the observation contract](KITSUNE_OBSERVATION_CONTRACT.md). Kitsune's built-ins
remain. Explicit Coven participant groups/roles and the foreground publisher through
Kitsune's user-enabled generic annotations are initially implemented; see
[GROUPS_CONTRACT.md](GROUPS_CONTRACT.md). Native group status/lifecycle, scheduling,
automatic following, and complete extraction remain open. Characterize generic surfaces
before extraction and before further worker/worktree implementation. No scheduler, daemon, plugin framework, or native
admission design is prerequisite for observation/grouping.

Local progress: native opt-in `observe kitsune --compare-agents` now characterizes
built-in hints against generic Djinn-family evidence; see the
[comparison contract](KITSUNE_AGENT_COMPARISON_CONTRACT.md). Existing modes and
Kitsune built-ins remain unchanged; extraction/replacement equivalence is not
established. A separate pure `session plan-resume` now characterizes 16 historical
provider mappings against an independently captured reference corpus; see
[the resume-planning contract](SESSION_RESUME_PLAN_CONTRACT.md). It adds no
execution or Kitsune dependency. Independent source-backed process-selection and
native-reference corpora now add 5,491 cases; see
[the extraction gates](KITSUNE_EXTRACTION_CHARACTERIZATION.md). Windows evidence
is already agent-selected, and restore consumes policy as ownership/history state,
so neither is ready for deletion or transparent delegation. A local preserving
Kitsune resolver/neutral-selector candidate matches all 209 baseline cases. With
compatible Zig/pinned Rust selected, 145 focused real-application tests and strict
lint pass, including logical-authority release, metadata-only multi-workspace
restore and deferred-start guard characterization.
Full-suite approval and Windows/other-platform validation remain outstanding.
Next: deterministic imported-runtime/rollback/spawn/send failure coverage and
additive generic tree evidence design before delegation. Per-runtime/cross-platform gates precede
removal; no scheduler is needed. Candidate/replay changes remain local/uncommitted
and claim no new remote-CI or installed-provider interoperability result.

Annotations target bounded badges/labels/group hints/progress on existing
panes/workspaces, useful for build/test/export services as well as coordinators.
Kitsune/user own space, layout, rendering, clipping, and quotas; source-owned values
need conflict policy/expiry and hidden/unknown failure handling, not stale active
status. The border return path is generic `pane.annotate`; persistent application
rows use the separate integration-view contract above. Neither provides workspace
annotations, arbitrary panels, embedded apps/scripts or UI takeover; display
conveys no execution authority.

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
