# Coven coding orchestration roadmap

## Purpose and status

Take the useful safeguards from MPC Forge and implement them natively in Coven,
without adding Forge as a dependency or adopting its full release process.

This is an implementation backlog, not a list of shipped capabilities. Local
implementation progress is recorded below; unmarked items remain proposed.
It complements [ROADMAP.md](ROADMAP.md), which already describes decomposition,
Djinn adapters, lifecycle projection, summaries, and synthesis. Those existing
plans are not counted as new discoveries here; this document adds implementation
gaps, coding-specific contracts, priorities, and testable completion criteria.

Assessment date: 2026-10-21. Source baselines:

- Coven: `ef6a18b438b2e5316802c350a6f37aa3077c6f8e`.
- Forge: `af0737247a7901a8170c318fc0ee6f4caf83a6b0`.
- Coven's existing local README and roadmap were also consulted; they contained
  uncommitted changes. They were preserved, not treated as implemented code.
- Findings come from source inspection. Neither application nor its tests were
  executed for this assessment. Recheck findings when implementing each item.

## Product decision: compose every multi-agent result

For a run with more than one contributing agent, Coven should automatically
create a dedicated composer session. Count contributors across the run, not just
simultaneously running processes; running two workers serially still needs
composition. Do not count the composer itself as a contributor for this trigger.

- For research, the composer reconciles findings and produces a cited synthesis.
- For coding, the composer assembles changes in its own integration worktree,
  checks compatibility, runs integrated validation, and prepares one reviewable
  candidate.
- A lead coordinates the plan; a composer owns the combined result. These are
  distinct responsibilities. Combining them into one session should be an
  explicit policy override, not an assumption based on the name "tech lead."
- Reserve capacity for the composer. Avoid a fan-out limit that leaves no room to
  compose. Do not add another agent for an ordinary single-contributor task.
- Worker success is not run success. Composer success is not permission to push,
  merge into the user's branch, or deploy.

Target coding workflow:

```text
user goal -> approved plan and acceptance criteria
                        |
                  Coven scheduler
                   /           \
             worker A        worker B
             worktree A      worktree B
                   \           /
                exact-commit handoffs
                        |
                 composer session
                 integration worktree
                        |
              merged-result checks + review
                        |
             one candidate and delivery report
                        |
              explicit user/CI merge decision
```

## Current capabilities versus confirmed gaps

The current runner creates inspectable workspace files, starts agent panes,
assigns tasks, records messages/events, tracks a designated lead, and presents
checkpoint approvals. Keep those capabilities; do not rebuild them gratuitously.

The table below records the assessment baseline. Implementation progress notes
supersede repaired gaps; it is not a claim that all baseline behavior is current.

| Area | Implemented today | Missing or insufficient |
| --- | --- | --- |
| Execution | OpenCode command in Herdr/tmux panes; configurable launch string | Native worker identity, acknowledged submission, reliable lifecycle/result collection; Djinn adapter is still planned |
| Tasks | Add, assign, list; `pending`/`assigned` written by handlers | Validated transitions, dependencies, attempts, atomic claims, completion gates |
| Leadership | Lead record and coordination prompt | Dedicated composer stage and enforced combined-result ownership |
| Isolation | Shared `work/` and `output/` directories | Repository/base registration and per-worker Git worktrees |
| Scope | Prompt says to stay within assignment | Path/resource reservations, bounded delegation, actual-diff validation |
| Checks | Example specs describe acceptance; QA is a seeded task | Executable checks, source-bound receipts, integrated validation |
| Approvals | Markdown checkpoints and approval events | Revision-scoped enforcement, revocation, separate mutation/integration authorization |
| Persistence | Direct JSON writes and JSONL appends | Locked mutation, stale-write rejection, recoverable durable state |
| Recovery | Refresh, prompt regeneration, narrow migrations | General replay/reconciliation and interrupted integration recovery |
| Delivery | Pre-delivery checkpoint | Exact candidate identity and a validated composer handoff |

Pane existence is currently reported as runtime status. It is not evidence that
an agent accepted work, finished successfully, or produced a correct result.

## Prioritized feature list

Priorities: **P0** fixes existing unsafe behavior; **P1** is required for the first
reliable multi-agent coding workflow; **P2** improves operation after that works;
**P3** is conditional on demonstrated need. Items remain open unless explicitly
completed in the progress notes.

### Implementation progress — 2026-10-21

**M0 / COV-01: scaffold safeguards completed locally; immediate COV-03 revocation
repaired.** No destructive reset is provided; task-ledger import is explicit.
Local slices:

- [x] Refuse reinitialization and partial workspace collisions before writes,
  including existing/dangling symlinks; `--force` cannot authorize reset.
- [x] Read/validate seed records and render templates before creation; create
  initial files exclusively and retain partial files if an I/O failure occurs.
- [x] Fix named default roles and canonical example owners. Resolve explicit
  task-owner aliases against the roster and reject unknown/ambiguous owners,
  without rewriting legacy persisted IDs.
- [x] Document actual JSON/TOML configuration authority and the lack of replay.
- [x] Centralize strict checkpoint/roster ID validation and managed-path checks
  across CLI, console, checkpoint discovery, consumed log metadata, refresh,
  bootstrap, and runtime operations. Reject managed symlinks (including inward
  and dangling links) and invalid file/directory types before side effects.
- [x] Preflight batch targets, persisted lead IDs, and checkpoint readability;
  decode all bootstrap prompts before sending. Cover case-insensitive `.MD`
  aliases and retain safe legacy IDs without migration. Root aliases and
  unrelated artifact symlinks remain supported. This is not a race-proof sandbox.
- [x] Make refresh generated-output-only: preserve task IDs/history, metadata,
  decisions, notes, work artifacts, and logs byte-for-byte. Do not restore missing
  checkpoint/guidance files or append refresh events. Remove implicit task-ID
  migration and unsafe approval-replay helpers rather than exposing a reset path.
- [x] Repair immediate COV-03 revocation: `suggest` records `changes_requested`,
  clears current approval, and retains feedback. Explicit reapproval clears
  rejection. Dashboard/standup/review/next use current file decisions; old events
  cannot reapprove checkpoints during refresh. Preserve fenced/nested history and
  reject ambiguous decision layouts or incompatible notes before effects.
- [x] Add pure v1 workspace/task/agent/lead record contracts and validate current
  persisted state before command and shared-helper effects. Validate live runtime
  configuration, reject corrupt/unsupported records and ambiguous roster aliases,
  preserve unknown fields and legacy ID/status/timestamp values, and retain
  explicit missing-file defaults without silently discarding malformed files.
- [x] Separate optional JSON display names from stable IDs. Capture new input
  labels without migrating existing workspaces; sanitize human-facing output and
  keep labels out of selectors, paths, owners, logs, and runtime identity. Validate
  both generated TOML documents before initialization, including emoji labels.
- [x] Establish an offline standard-library test suite, including the wrapper,
  re-init preservation, reserved-name/symlink collisions, invalid seed inputs,
  exclusive-create failure, canonical owners, legacy compatibility, and path
  safety through commands/helpers and both mocked runtime adapters. Include
  refresh-preservation snapshots and approve -> suggest -> refresh/reapprove
  exercises through CLI, console, and interactive review. Add corrupt-state CLI
  and cached-helper matrices, unknown-field/legacy preservation, and safe label
  routing/rendering/Unicode serialization coverage.
  Validation: `python3 -m unittest discover -s tests -v` — 382 tests pass;
  `bash -n bin/coven` and `git diff --check` pass.

Implicit migration and destructive reset remain unavailable. Task-only baseline
import is implemented below with exact backups and explicit activation. Record
schemas are structural, not new task lifecycle/reference rules.

**COV-02a: bounded task-snapshot mutation foundation implemented locally.**
Contract and operator guidance: [STORAGE_CONTRACT.md](STORAGE_CONTRACT.md).

- [x] Persistent single-host POSIX workspace lock with bounded acquisition,
  explicit nested-token reuse, no PID stealing, and kernel release on process exit.
- [x] Serialize full task add/assign reload/validation/ID allocation/replacement;
  preserve the v1 task list and unknown fields. Expose exact-content revisions
  through `task list --json` and optional `--expect-revision` checks under lock.
- [x] Publish task snapshots with same-directory temp/file fsync/replace/directory
  fsync; distinguish precommit failure, replaced-but-durability-uncertain outcome,
  and committed task with secondary history/dashboard warnings. No rollback fiction.
- [x] Serialize all log appends and actual dashboard render/publication with the
  same lock. Reject malformed/interior/nonobject/truncated-tail history with
  explicit inspection diagnostics, not silent skipping. New event/mutation IDs
  aid correlation, but do not establish a sequenced or replayable ledger.
- [x] Correct new authority declarations to workspace files and replace generated
  prompt instructions for direct shared-file edits with supported CLI operations.
  Preserve legacy metadata and customized role prompts; starter prompts state
  that old direct-edit instructions must not be followed.
- [x] Prove independent-process add/add and add/assign retention, conditional
  assignment conflict, complete reader snapshots, lock death release, stale
  dashboard prevention through both entrypoints, and before/after-replace crashes.
  Cover event fsync/secondary projection failures and explicit uncertain outcomes.

**COV-02b: opt-in task ledger and committed-request deduplication implemented.**
Protocol and operator guidance: [TASK_LEDGER_CONTRACT.md](TASK_LEDGER_CONTRACT.md).

- [x] Explicit revision-conditional import preserves exact source bytes/mode and
  source absence, allocates stable workspace/epoch identities, and activates only
  after a validated/durable baseline. Existing v1 workspaces stay v1 by default.
- [x] Immutable sequenced frames atomically publish each task effect and frozen
  receipt together. Validate canonical bounded frames, digests, identities,
  sequence/hash chain, effect invariants, and result consistency; never treat a
  malformed final frame as a disposable tail or fall back to snapshot authority.
- [x] Imported add/assign require request IDs bound to original structured input
  and expected revision. Same-key retries return frozen committed results before
  current revision/roster checks; changed bindings conflict. Rejected requests
  are not cached and secondary events are not delivered exactly once.
- [x] Task reads/status use authority despite malformed/missing/unreadable
  projections. Require healthy projection for new requests; explicit recovery
  preserves observed bytes before repair. Pending import inspection/archival
  remains possible even when the source has become corrupt.
- [x] Confirm observed authority durability before retry acknowledgement, expose
  post-publication uncertainty separately, and support explicit pending-import
  resume or nondestructive artifact archival. Old event/message logs remain
  observational and do not gate ledger receipts or fabricate a past history.
- [x] Test concurrent identical keys, process death after frame before projection,
  original-revision replay after later state/roster edits, import-versus-v1-writer
  race, activation/frame fsync uncertainty, no-fallback corruption, and explicit
  source/projection preservation.

**COV-02c / COV-03a: serialized checkpoint-local content binding implemented.**
Contract: [CHECKPOINT_CONTRACT.md](CHECKPOINT_CONTRACT.md).

- [x] One Markdown authority atomically publishes inline decision binding and
  managed history under the workspace lock. No sidecar, checkpoint ledger, or
  observational-event replay supplies current approval.
- [x] Distinguish exact checkpoint snapshot/absence revisions from local scope
  hashes. Conditional decisions recheck the inspected snapshot under lock; covered
  text edits stale effective approval on read. Bare legacy approvals are shown as
  unbound without rewriting them. External linked content is explicitly not bound.
- [x] Preserve covered bytes, fenced/nested examples, and old human history. Only
  trailing machine-history regions are excluded; relocating them into content or
  appending covered prose invalidates the prior binding. Reject malformed/unknown
  machinery and ambiguous layouts before effects.
- [x] Interactive review and console confirmation bind displayed snapshots, reject
  stale input without automatic retry, and preserve conflict/uncertainty outcomes.
- [x] Treat logs/dashboard/notices as secondary. Uncertain publication skips them;
  postcommit failure warns without rollback. Notices run outside the lock, include
  decision/scope identity, and suppress already-observed stale/superseded decisions.
  Transport exit codes, including a failed Herdr enter, do not report success.
- [x] Test actual process races, whole-file atomic readers, missing-file creation
  conflicts, UI edit races, before/after-replace failures, lock-free delivery,
  stale/superseded notices, legacy compatibility, and unchanged task-ledger scope.
  Covered by the current offline regression suite.

**COV-03b: opt-in reviewed unrestricted runtime startup implemented.**
Contract: [RUNTIME_LAUNCH_CONTRACT.md](RUNTIME_LAUNCH_CONTRACT.md).

- [x] Explicit persistent activation blocks legacy launch/restart/bootstrap/pane
  delivery routes, including shared helpers. A separate shared-legacy/exclusive-
  activation transport gate drains admitted operations before activation can
  publish. Acquire it before the workspace lock; never wait for the gate while
  holding that workspace lock. Cached pre-activation teams cannot bypass mode.
- [x] Freeze a closed guidance/config/task/roster/prompt inventory, effective
  runtime/environment selection, workspace/activation identity, and deterministic
  cold transport plan. A dedicated checkpoint binds the immutable manifest;
  generic local text approval alone remains insufficient to execute anything.
- [x] Revalidate current inputs and exact bound approval under lock, then publish
  consumption and in-flight claim in one atomic control replacement. Execute only
  the durable frozen claim outside locks. Before-claim edits/revocation block;
  after-claim changes do not retroactively cancel that attempt.
- [x] Return existing consumed attempts without relaunch, including failures,
  crashes, and uncertain claims. Keep explicit unknown acknowledgement separate
  from proof of stopped processes, and require a new prepared/reviewed action for
  a new attempt. No implicit cleanup, claim reset, or adoption of partial panes.
- [x] Check every required tmux/Herdr response and typed dynamic handle. Use
  frozen literal startup text rather than delayed mutable-file reads. Report CLI
  transport completion only—not native worker readiness, acceptance, or sessions.
  Scoped operator stop distinguishes attempted/succeeded/uncertain results and
  refuses to mistake a permission/query failure for established absence.
- [x] Serialize participating goal/lead input writers with claims and refresh;
  preserve native policy independence and fail closed on corrupt/moved control.
  Prove concurrent submit executes once, activation races, immutable input checks,
  before/after-claim failure behavior, process death, frozen post-claim execution,
  and lock-free runtime transport, covered by the current offline suite.

This is deliberately **not read-only dispatch**: current pane adapters cannot
enforce that. Task revisions/checkpoint hashes alone are not execution grants.

**COV-05a: fail-closed native adapter compatibility inspection implemented.**
Contract: [WORKER_READINESS_CONTRACT.md](WORKER_READINESS_CONTRACT.md).

- [x] Inspect the installed Djinn help surface/source rather than assuming the
  proposed worker CLI exists. Keep native/folder/UI/PID/pane identities distinct.
  No provider/session/model operations or credential/configuration inspection were
  requested; native CLI probes used help only, not a native-code purity attestation.
- [x] Add `worker readiness` with versioned Coven report, resolved executable
  fingerprint, bounded advertised help/version probes, provenance, actionable
  blockers, and explicit `authorization: none`. Separate advertised syntax from
  semantic policy/identity/lifecycle evidence, which remains unverified.
- [x] Bound subprocess output, timeout, and supervisor lifetime. Private completion
  and parent-liveness descriptors never reach native code; group cleanup covers
  ordinary exits, limits, caller SIGTERM/SIGKILL, and stalled parent cleanup.
- [x] Prove unchanged v1/task-ledger/approved-launch workspace bytes and no
  authority locks or state writes. Do not invoke native status/watch/list as
  non-mutating probes, fabricate a capabilities API, create attempts, or fallback
  to worker execution, covered by the current offline suite.

The installed help-only probe reports `unknown`, not a ready worker. Required
native contracts remain versioned admission, recoverable attempt correlation,
non-mutating lifecycle inspection, cancellation, bound result pointers, and
immutable per-attempt tool/resource enforcement. Help-only inspection cannot
establish them, even when flags or role names suggest read-only behavior.

**COV-04a: explicit preparation-only execution intent implemented.**
Contract: [EXECUTION_INTENT_CONTRACT.md](EXECUTION_INTENT_CONTRACT.md).

- [x] Explicit enable enrolls zero tasks; selected enrollment, immutable prepared
  attempt identity, and local `cancelled_before_submission` transitions live in one
  bounded execution-control authority, with effect and frozen request receipt in
  the same atomic publication. No task-ledger upgrade or task status rewriting.
- [x] Bind complete source task/authority identity/revision, exact canonical owner,
  declarative criteria, and domain-separated intent digests. Retain unknown source
  fields, enrollment rebind history, old preparations, cancellations and numbered
  retry links. Every result is non-dispatchable with authorization `none`.
- [x] Require explicit CAS/request binding; replay committed requests before
  mutable source/roster parsing, including after cancellation or source corruption.
  Post-publication verification failures preserve the known receipt with uncertain
  delivery and unknown current state, never an ordinary failed mutation result.
- [x] Guard fresh task effects and import/resume under the same workspace lock,
  including low-level ledger paths. Prepared/stale tasks cannot be reassigned even
  to the same owner; historical task receipts remain replayable before this guard.
  Cancellation release is durably confirmed; source changes need explicit rebind.
- [x] Inspect frozen state despite source corruption; source assessment never
  creates an absent lock. Missing/bad enabled control fails closed without global
  preflight burying unrelated task reads or historical receipts. Pending archival
  preserves artifacts and never resets a present malformed control.
- [x] Reserve cancellation capacity for every prepared attempt before all history
  growth; maximum-sized valid reasons cannot be crowded out. Test concurrency and
  both task/import lock orders, before/after-publication process death, uncertainty,
  malformed source diagnostics, retained extensions, and native-call prohibition.
  Covered by the current offline regression suite.

**COV-04b: reviewed immutable DAGs and graph-bound preparation implemented.**
Contract: [TASK_GRAPH_CONTRACT.md](TASK_GRAPH_CONTRACT.md).

- [x] Explicit execution-control v2 upgrade preserves verified raw v1 bytes/mode,
  stable identities, validated legacy prefix and original receipts. No implicit
  task-ledger upgrade, receipt rewrite, downgrade, or backup promotion.
- [x] Validate bounded DAGs from explicitly enrolled canonical tasks/owners, freeze
  complete selected snapshots/criteria/edges/enrollment bindings, and expose
  immutable revision/digests/topology and source-impact inspection/export.
- [x] Planning decisions live in the same execution authority and publish with
  their request receipts. Only the current reviewed head supplies preparation;
  successors start unreviewed and old approvals cannot become current again.
- [x] Proposal stickily manages selected IDs; standalone intent cannot bypass
  proposed/revoked/stale/removed-node gates. Preparation derives criteria and
  freezes graph/decision/node bindings. Unmanaged tasks retain local intent mode.
- [x] Preserve source-free revocation/cancellation and receipt/export access with
  broken mutable source. Keep selected-node drift distinct from unrelated task
  revision changes, without weakening existing attempt whole-source freshness.
- [x] Reserve revocation room for the approved head plus cancellation room per
  prepared task before history growth. Prove CLI lifecycle, concurrent proposal
  keys/CAS, selected-source review races, preparation versus successor, exact
  backup/recovery across process death, and legacy compatibility. All 382 offline
  regressions pass.

This supplies reviewed decomposition and enforceable **preparation** criteria,
not a scheduler: downstream nodes can be prepared while prerequisite acceptance
remains unestablished, and every node is non-dispatchable. Actual COV-05 execution
still requires the supported upstream native admission/reconciliation/policy
contract and separately scoped authorization. Native lifecycle, accepted results,
scheduling/cancellation propagation, composers, run completion, repository/mutation/
integration/delivery grants, and recovery/retention hardening remain open. More
nominal execution-policy envelopes cannot replace that native prerequisite.

| ID | Priority | Feature | Depends on |
| --- | --- | --- | --- |
| COV-01 | P0 | Safe initialization, identities, and confined paths | None |
| COV-02 | P0 | Locked state mutations, revisions, and recovery foundation | COV-01 |
| COV-03 | P0/P1 | Revocable, revision-bound approvals | COV-02 |
| COV-04 | P1 | Task graph, attempts, and explicit lifecycle | COV-02 |
| COV-05 | P1 | Native worker adapter and honest lifecycle facts | COV-04 |
| COV-06 | P1 | Dedicated composer stage and result ownership | COV-03, COV-04, COV-05 |
| COV-07 | P1 | Git repository registration and isolated worktrees | COV-01, COV-02, COV-04 |
| COV-08 | P1 | Scope reservations and bounded delegation | COV-02, COV-04, COV-07 |
| COV-09 | P1 | Structured, exact-source worker handoffs | COV-05, COV-07, COV-08 |
| COV-10 | P1 | Executable, source-bound verification | COV-03, COV-07, COV-09 |
| COV-11 | P1 | Serialized, journaled composer integration | COV-06, COV-09, COV-10 |
| COV-12 | P1 | Candidate handoff and truthful run completion | COV-03, COV-10, COV-11 |
| COV-13 | P2 | Operator recovery, blockers, and evidence navigation | COV-02 through COV-12 |
| COV-14 | P3 | Immutable build reuse | Demonstrated repeated-build cost |
| COV-15 | P3 | Cross-clone coordination and accepted ownership transfer | Demonstrated multi-host need |

### COV-01 — Stop damaging existing workspaces

**Assessed gap (baseline):** `command_init()` overwrites metadata, task state, and agent state
even without `--force`. Default agent IDs are lowercased, but named roles and
example owners use capitalized names. Checkpoint IDs become filesystem paths
without a confinement check.

**Build:**

- Refuse initialization over an existing workspace. Separate initialization,
  migration, refresh, and explicitly authorized reset.
- Preserve logs, approvals, notes, dirty files, and task history during refresh.
- Separate stable canonical agent IDs from display names; normalize and validate
  example owners, assignments, and role lookup consistently.
- Validate record schemas and constrain IDs/paths to their intended directories,
  including resolved symlink handling. Never trust worker-provided paths.
- Clarify config authority: `load_team()` currently overlays only runtime fields
  from TOML. Do not silently imply that all generated TOML sections are live.

**Done when:** default seeding assigns the intended roles and task owners;
reinitialization leaves an existing fixture unchanged; traversal IDs are rejected;
explicit reset previews what will be lost and requires separate consent.

**Local outcome:** current scaffold safeguards, structural v1 contracts, and
identity/display separation are implemented and tested. Reset stays disabled,
rather than introducing a destructive operation without preview/consent. COV-02b
provides explicit backed-up task import; existing state is never automatically
renamed, imported, or upgraded by refresh.

### COV-02 — Make concurrent state changes safe

**Assessed gap (baseline):** task handlers perform unlocked read/modify/write of whole files.
Events have timestamps but no stable event IDs or sequence. Metadata calls events
the source of truth, but no general reducer rebuilds state from them.

**Local progress:** COV-02a fixes cooperating task lost updates without changing
v1 format. COV-02b adds explicitly imported task-ledger authority, replay, and
committed-request receipts; it does not migrate all workspace state or solve
whole-workspace transaction/recovery semantics. COV-02c adds checkpoint-local
serialized single-file decisions with snapshot conditions and scope invalidation.
COV-03b also serializes goal/lead writers and single-use startup control, with a
separate activation/legacy-transport gate rather than workspace locks over I/O.

**Build:**

- Give workspace, run, task, attempt, and event stable identities independent of
  pane labels. Version schemas and record expected revisions on mutations.
- Serialize mutations with a workspace lock; reject stale expected revisions.
- Choose and document one authority model. Preferred: a sequenced append-only
  transition ledger with derived JSON/Markdown projections, not two competing
  sources of truth. Record initialization inputs needed for reconstruction.
- Use atomic snapshot replacement and a defined flush/fsync policy. Define
  ledger commit boundaries and replay behavior; rename alone is not a transaction.
- Give commands idempotency keys. Repeating a request succeeds only if its intended
  effect is durably observed, not because an earlier attempt probably ran.
- Report malformed/interior log corruption. Treat a truncated final event through
  an explicit recovery procedure, not silent line skipping.
- Migrate old snapshots/logs through a backed-up, versioned import event. Never
  pretend incomplete historical logs can reconstruct old state exactly.
- Replace prompts telling workers to edit shared task JSON/events/dashboard with
  calls through the mutation API. Human edits require validated import/reconcile.

**Done when:** concurrent claims have one winner; stale edits cannot overwrite
newer work; interrupted projection writes can be rebuilt; repeated requests do
not duplicate tasks; corrupt history blocks unsafe transitions with a fix path.

### COV-03 — Bind approval to what was actually approved

**Assessed gap (baseline):** requesting changes does not revoke an approved checkpoint.
Historical approval events can reapprove it during refresh. Assignment and launch
do not enforce checkpoint approval.

**Local progress:** explicit change requests revoke approval and refresh never
replays it. Checkpoint-local content/snapshot binding now detects covered edits
and stale decision attempts; legacy approvals remain unbound until explicit
action. COV-03b adds a concrete opt-in `runtime_launch` manifest and enforced
single-use claim boundary. Other run/plan/repository/candidate binding, withdrawal,
and worker/mutation/integration/delivery enforcement remain open; local text
approval by itself is not an execution grant.

**Build:**

- First fix revocation: changes requested, withdrawal, or a new plan revision must
  invalidate approval in both state and projections.
- Bind decisions to run, plan digest/revision, action, repository, task scope,
  decision source, and optional expiry. Delivery approval binds the candidate SHA.
- Distinguish execution permission, local worker commits, composer integration,
  push, target-branch merge, cleanup, and deployment. No blanket inherited grant.
- Enforce decisions at dispatch/integration/delivery transitions, not merely in
  prompts. Djinn policy remains independently authoritative.
- Record stakeholder feedback as new events linked to the reviewed revision;
  retain the original task and criteria rather than replacing their history.

**Done when:** approve -> request changes -> refresh remains unapproved; changing
the plan prevents dispatch under old approval; approval for one path/task cannot
authorize another; approving code review does not authorize push or deployment.

### COV-04 — Represent tasks and attempts, not just assignments

**Current gap:** there is no validated task lifecycle or dependency scheduling.
`task assign` resets status to `assigned`; renderers guess categories from strings.

**Build:**

- Define task transitions, for example `proposed -> ready -> running -> review ->
  accepted`, with explicit blocked/failed/cancelled branches and reopen policy.
- Store retries as separate attempts with native execution references and results.
  Do not overwrite a failed attempt when starting another.
- Validate dependency DAGs and distinguish worker completion, result availability,
  acceptance, integration, and run completion.
- Add concurrency/cost limits and cancellation propagation. Do not create more
  workers merely because a plan can be split; serial execution remains valid.
- Version task descriptions and acceptance criteria. A criteria change requires
  impact review/reapproval, not an unnoticed edit beneath running workers.

**Done when:** cycles and invalid transitions fail; unmet dependencies prevent
dispatch; cancelling a run cannot leave its queued tasks dispatchable; completed
attempts survive retries; two assignees cannot win the same claim revision.

### COV-05 — Treat native sessions as execution, panes as presentation

**Current gap:** agents launch in panes and receive text after a fixed delay.
Some sends ignore subprocess failure. Pane detection does not establish agent
health or successful submission. Native session/transcript IDs are not stored.

**Build:**

- Implement the already-planned Djinn adapter behind submit/inspect/cancel/
  collect-result/reconcile operations; retain other harnesses behind the same
  narrow contract where feasible.
- Store run/task/attempt -> native session and transcript/result pointers. Keep
  Herdr/tmux references optional and separate from execution identity.
- Distinguish launch requested, accepted, running, awaiting permission, result
  available, failed, cancelled, and unknown. Do not equate process exit with
  application correctness.
- Acknowledge submissions and check transport exit codes. When an acknowledgement
  is lost, reconcile by attempt/idempotency identity before launching a duplicate.
- Use supported installed Djinn interfaces and capability checks. The proposed
  `djinn agent worker run` in the existing roadmap is not a shipped contract to
  assume. A blocking adapter must account for background-by-default session runs.

**Done when:** failed delivery is visibly failed; killing a pane does not erase
task identity; restart finds the existing native attempt; headless execution can
complete and collect a result without Herdr/tmux; permission pauses stay visible.

### COV-06 — Make composition an actual stage

**Current gap:** a lead has a prompt and a state file, but no enforced composition
stage exists. Existing synthesis plans focus on findings, not code assembly.

**Build:**

- Automatically allocate the composer under the trigger defined above, with its
  native session and task recorded before multi-contributor dispatch.
- Add explicit composition states: waiting for inputs, ready, integrating,
  blocked, validating, ready for user review, failed, cancelled.
- Give the composer a manifest of required contributions and a dependency order.
  Missing, failed, cancelled, or rejected work requires an explicit plan decision.
- Have the composer reconcile interfaces and assumptions, not just copy summaries.
  Send substantial repairs back as new worker attempts. Record composer-authored
  fixes and apply the same scope/check rules to them.
- For research-only runs, require cited synthesis but no Git/check ceremony.

**Done when:** more than one contributor cannot start without a composer record;
all workers reporting success does not complete the run; a missing contribution
blocks composition unless an approved scope reduction excludes it; composer
failure can be retried without rerunning successful workers unnecessarily.

### COV-07 — Isolate code-changing attempts

**Current gap:** all panes use the same Coven workspace. `work/` is a convention,
not an isolated checkout. No repository or Git base is recorded.

**Build:**

- Register the target repository separately from the orchestration workspace.
  Record the immutable base SHA and intended delivery target.
- Create a branch/worktree per mutating attempt and a composer integration
  worktree. Do not copy dirty changes, credentials, databases, or mutable runtimes.
- Preflight dirty/untracked state, worktree collisions, repository identity, and
  required executables. Never reset or stash user work implicitly.
- Allow explicitly registered existing worktrees after validation, rather than
  forcing all workspaces to be newly provisioned.
- Make cleanup owner-aware and refuse to delete dirty, untracked, unresolved, or
  otherwise unretained work. Preserve failed work until explicitly disposed of.

**Done when:** workers edit different checkouts; an unrelated dirty user checkout
is unchanged by setup/integration/cleanup; restarting setup reuses the correct
attempt workspace instead of creating an orphan; composer writes nowhere else.

### COV-08 — Reserve scopes before paying for conflicting implementations

**Current gap:** assignments carry no machine-checked paths or resources.

**Build:**

- Reserve normalized repository-relative paths/subtrees and optional named
  resources such as `contract:provider-config` before dispatch.
- Detect equal/ancestor/descendant path conflicts conservatively. Two functions
  in the same file are not automatically independent scopes.
- Allow a parent scope to delegate disjoint child scopes to named attempts.
  Reject siblings that overlap and children outside the parent scope.
- Hold reservations through blocked/review/handoff states. A timeout or dead pane
  is not proof the old worker stopped writing; recovery requires reconciliation
  and accepted transfer or explicit abandonment, not automatic lock stealing.
- Compare actual changed paths, including additions/deletions/renames, with the
  reservation before accepting a handoff. A named contract does not grant paths.
- Resolve conflicts by serialization, revised scope, or a single shared-change
  owner; do not provide a silent "ignore overlap" path.

**Done when:** two overlapping workers cannot both acquire scope; unrelated files
can proceed; out-of-scope diffs block handoff; crashed-worker claims remain
visible; approved scope expansion is recorded before its new edits are accepted.

These are cooperative coordination checks, not an OS sandbox. Different files
can still conflict semantically; the composer must identify shared contracts.

### COV-09 — Collect exact-source handoffs

**Current gap:** neither a structured coding result nor a source identity is
required. The existing roadmap's summary contract is a good starting point.

**Build:** require a versioned result containing:

```text
run_id, task_id, attempt_id, approved_plan_revision
repository_id, base_sha, source_sha, tree_sha
actual_changed_paths, scope_revision
summary, interface_changes, dependencies
check_receipt_refs, blockers, risks, follow_ups
native_session_id, transcript_uri, result_artifact_digest
```

Require clean committed Git-visible source for a coding handoff; explicitly
authorize local commits in the mutation plan. Capture results under attempt IDs
without copying native transcripts into Coven. Validate identity and content
digests; quarantine malformed or foreign results instead of ingesting them.

**Done when:** a branch advancing after handoff cannot silently change the
accepted source; another attempt's result is rejected; missing required evidence
blocks readiness; an unchanged result can be collected twice without duplication.

### COV-10 — Execute checks and bind evidence to exact inputs

**Current gap:** QA and acceptance are prose, not executable completion gates.

**Build:**

- Configure named check sets for worker and integrated-result stages: argv, cwd,
  timeout, environment policy, required/optional status, expected exit codes.
- Record source commit/tree, target/base context, check-policy digest, declared
  tool/environment identity, command, timestamps, exit status, and output digest.
- Retain immutable receipts and output pointers. Never use a task's `verified`
  boolean or a worker's self-report as a substitute for a receipt.
- Recheck source before/after execution; checks mutating source cannot qualify
  that candidate. Define declared generated/ignored inputs rather than claiming
  Git cleanliness proves all external state was unchanged.
- Reject stale evidence when source, target, criteria, or policy changes. Preserve
  the older receipt with the invalidation reason instead of rewriting history.
- No configured checks means unverified, not passed. Explicitly scoped human-only
  review may exist, but must not be presented as automated test evidence.

**Done when:** failed/empty checks block coding delivery; changing code after a
green run invalidates its eligibility; a hung check times out; source-mutating
checks fail qualification; worker checks do not satisfy merged-result checks.

Independent review can be a policy option. A different actor label alone is not
proof of an independent person or session; never claim otherwise.

### COV-11 — Give the composer a recoverable integration transaction

**Current gap:** no integration operation, single writer, journal, or conflict
recovery exists. A composer prompt alone would not provide these guarantees.

**Build:**

1. Acquire exclusive integration authority and a lock for the target.
2. Validate ownership, clean expected target, exact handoff SHA, scope, approvals,
   and already-integrated dependencies.
3. Durably record operation ID, source SHA, target-before SHA, policy identity,
   and phase before mutating Git.
4. Integrate the recorded commit through one explicit MVP strategy, such as a
   non-fast-forward merge. Preserve a mapping from result to integrated commit.
5. Record target-after SHA and execute admission checks against the merged code.
6. Mark accepted only after required checks pass. Retain pending state on failure.

On restart, compare journal and actual refs before acting. Recognize already
applied work only with verified ancestry/parent and operation identity. Unexpected
refs or conflicting pending sources require intervention, not an optimistic retry.
Preserve merge conflicts and dirty repair work; do not automatically hard-reset.

Define a separate, authorized repair transition for failed integrated checks.
Ordinary admission of another source remains blocked while an operation is
pending. A repair transition links a clean repair attempt to that pending operation,
records the expected failed-target SHA and repair-source SHA before changing Git,
and retains the original failed receipts. Integrate the repair under the same
exclusive authority, invalidate evidence for the changed target, and rerun the
required combined checks. Neither the original contribution nor its repair is
accepted until the resulting target passes. Journal repair phases for restart;
unexpected target movement blocks repair just as it blocks ordinary admission.
An abort/supersession path must also be explicit and authorized, preserve the
failed branch/evidence, and never discard dirty work automatically.

**Done when:** admissions are serialized; failed merged checks remain pending;
retry does not duplicate integration; crash injection before merge, after merge,
after checks, and during projection updates is recoverable; unrelated ref movement
blocks instead of being adopted as success; a code-level failure can be repaired
through the journaled transition without bypassing the pending-operation guard.

Local file locks do not constitute distributed integration authority. Keep the
first version single-host and do not imply a cross-clone transaction guarantee.

### COV-12 — Deliver one exact, reviewable candidate

**Current gap:** the pre-delivery checkpoint is not bound to code or evidence.

**Build:**

- Produce a candidate record containing base/target, integrated SHA, included and
  excluded result IDs, composer changes, conflict resolutions, check receipts,
  review references, known risks, and unmet criteria.
- Mark a candidate ready for review only after all required contributions are
  accounted for and integrated checks pass.
- Bind user acceptance to this candidate and criteria revision. Any subsequent
  edit produces a new candidate and requires revalidation/reapproval.
- Default delivery to a branch/commit and human-readable report. Offer PR or
  merge actions only through explicit adapters/authorization; do not replace CI.
- If a target moves, requalify the combined result against it. Do not imply
  Forge's local promotion model handles GitHub merge-queue results.
- Keep accepted work, merged work, pushed work, and deployed work distinct.

**Done when:** no task/run completion based solely on worker summaries; reports
link to the exact code checked; a moved target or edited candidate cannot reuse
old approval; delivery has no implicit push, user-branch merge, or deployment.

### COV-13 — Make recovery and evidence usable

**Build:** expose an operator view answering: who owns each path, which native
attempt is active, what is blocked, which operation is pending, which evidence is
stale, and what action is safe next. Link through to native transcripts and check
logs rather than duplicating them.

Add inspected recovery for interrupted dispatch, orphaned worktrees, composer
replacement, permission pauses, stale reservations, and inconsistent projections.
Retain linked feedback/rework history and explicit acceptance decisions.

**Done when:** a stopped process can be reconciled without guessing from pane
names; replacing the composer preserves accepted inputs and pending operations;
recovery previews effects and cannot silently seize ownership or discard work.

### COV-14 — Reuse immutable builds only if it saves real time

**Current gap:** no build cache; this is not a blocker for the composer workflow.

If measurements justify it, key artifacts by declared inputs, dependencies,
toolchain/platform, command/config, and environment. Build in private staging;
publish only complete, validated output under a per-key lock. Verify inventories
on reuse and pin each consumer to immutable output. Never share writable compiler
state, editable environments, or live `dist` directories across workers.

**Done when:** identical inputs reuse one artifact; changed frontend inputs miss;
failed builds never become hits; tampering is rejected; consumers cannot silently
switch artifacts. Do not claim hermeticity from hashing an incomplete input list.

Prefer an existing build system's cache when it already provides these guarantees.

### COV-15 — Cross-clone coordination only when needed

**Current gap:** coordination is local. That is acceptable for the initial scope.

If multi-host work becomes necessary, add portable task/claim metadata with
freshness, conditional updates, explicit recipient-accepted ownership transfer,
and independent source/evidence transport. Offline cached observations must not
authorize new claims. Do not replicate machine-local paths, runtimes, credentials,
or databases as if they were portable task state.

**Done when:** racing assignments have one winner; stale observations cannot
authorize work; accepted transfers preserve evidence and rebind local workspaces;
two clones cannot both claim integration authority under the supported protocol.

Do not copy Forge's Git-backed HANDOFF ledger unless Git transport is the actual
requirement. First establish why Coven needs coordination beyond one machine.

## Implementation sequence and milestone exits

### M0 — Repair and test the current scaffold

COV-01, the mutation foundation of COV-02, and immediate checkpoint revocation
from COV-03. Establish automated tests; no test suite was present at the assessment
baseline. Test the wrapper and CLI with mocked native runtimes and disposable files.

**Exit:** no destructive re-init, no silent lost task updates, correct default
owners/roles, and no stale approval revival. Stop here if normal use needs no
additional orchestration.

### M1 — Prove the composer with read-only workers

COV-04, COV-05, COV-06, plus revision-bound plan approval. Deliver the existing
roadmap's research story using two workers and a dedicated composer. Keep runtime
facts separate from terminal presentation.

**Exit:** structured cited synthesis, recoverable task/session mapping, failed
delivery reported honestly, and run completion gated on composition. This
milestone does not authorize code-changing fan-out.

### M2 — First complete coding vertical slice

COV-07 through COV-12, using the minimum supported single-repository/single-host
policies. Build one small end-to-end slice rather than implementing every storage,
scheduler, or Git feature in advance.

**Exit:** the acceptance exercise below passes, including failure injection.

### M3 — Operational hardening, then measured optimization

COV-13 and broader cancellation/recovery tests. COV-14/COV-15 require a measured
need and separate design approval. Do not block the first useful composer on them.

## Acceptance exercise: two green workers, one broken combination

Use a disposable Git repository and stubbed workers/check executors in automated
tests; add a separately authorized live Djinn smoke test, not model calls in CI.

1. Create an approved plan for two contributors and an automatically allocated
   composer. Pin the base commit and real acceptance commands.
2. Deliberately request overlapping scopes. Verify the second reservation fails.
   Revise to disjoint files before either worker edits code.
3. Worker A changes a configuration default in one file. Worker B adds a consumer
   in another file assuming the old default. Their local tests pass individually.
4. Collect clean exact-commit handoffs. Verify missing or out-of-scope artifacts
   cannot be admitted and all-worker completion does not finish the run.
5. Compose both changes. A configured cross-component test fails on the merged
   result; the run stays blocked/pending, not accepted.
6. Interrupt the composer after a merge but before final acceptance. Restart and
   reconcile without a duplicate merge or deletion of the worktree.
7. Request repair and record a new attempt. Use the explicit pending-operation
   repair transition, not ordinary admission, to integrate it and pass combined
   checks. Verify failed evidence remains linked to the accepted repaired result.
8. Change the candidate after verification. Verify stale receipts/approval cannot
   qualify it. Rerun checks and approve the new exact candidate.
9. Produce one branch/commit and delivery report. Assert no push, target merge,
   deployment, or destructive cleanup occurred implicitly.
10. Repeat with workers executed serially; composition remains required. Repeat a
    single-contributor task; no extra composer or release hierarchy is forced.

## Architecture constraints and extension seams

- Keep Coven the orchestration authority and Djinn the native execution authority.
  Coven stores references and selected facts, not a second native transcript.
- Preserve the lightweight file-backed model. No service framework or database
  scheduler is needed to implement this roadmap's first milestones.
- Gradually extract storage, transitions, runtime adapters, Git workspace handling,
  checks, and composition from `runner/coven.py`; keep `bin/coven` thin. Do not
  require a wholesale rewrite before delivering the first safeguard.
- Explicitly declare capabilities for non-Git/research tasks; do not force coding
  receipts onto prose synthesis or fake a code acceptance result.
- Proposed APIs/commands should be designed alongside tests. Existing `coven plan`
  prints multiplexer layout; do not silently repurpose it as task-plan approval.
- Approval metadata is not authentication; scope reservations are not sandboxing;
  test receipts are not semantic proof; local locks are not distributed locks.

Not adopting by default: Forge as a dependency; PM/feature/release branch layers;
one-active-release policy; local-main promotion; a second issue tracker; a wiki or
memory system overlapping Djinn; cross-host cache transport; autonomous deployment.

## Source evidence and implementation entry points

Line ranges describe the inspected baseline and will drift. Prefer the named
functions when locating current code.

### Coven evidence

All runner references below are in [runner/coven.py](runner/coven.py).

| Finding | Functions / baseline lines |
| --- | --- |
| IDs versus named roles | `parse_agents`, `agent_role`, `starter_tasks`; 41–58, 217–229, 344–371; also [example owners](examples/interactive-demo/tasks.json) |
| Unlocked storage/event append | `write_json`, `append_event`, `append_message`; 112–131 |
| Limited config overlay | `load_team`; 134–146 |
| Prompts request shared-file edits | `prompt_text`, `starter_prompt_text`, `lead_prompt`; 232–273, 1297–1312 |
| Historical approval restoration | `sync_checkpoint_approvals_from_events`; 613–624 |
| Initialization overwrites state | `command_init`; 677–736 |
| Flat task mutations | `command_task_add`, `command_task_assign`; 758–802 |
| Malformed event lines skipped | `iter_jsonl`; 838–851 |
| Pane existence / unchecked delivery | `send_to_agent_pane`, `agent_runtime_status`; 979–1010 |
| Revocation missing | `approve_checkpoint`, `suggest_checkpoint_changes`, `command_next`; 1257–1294, 1365–1377 |
| Shared launch workspace / presentation lifecycle | `start_agent_window`, `create_coven_tmux_session`, `create_coven_herdr_workspace`; 1013–1058, 1796–1877 |
| Actual commands, no Git/check/composition API | `parse_args`; 1948–2073 |

### Forge safeguards borrowed, not dependencies

Paths below are relative to the separate `mpc-forge` repository at the pinned
baseline above, not files expected inside Coven. Tests were inspected, not run.

| Safeguard | Implementation | Relevant test source |
| --- | --- | --- |
| Scope overlap, delegation, actual diff checks | `mpc_forge/devflow_lib/ledger.py`: `overlap`, `validate`, `check_paths` | `tests/devflow/test_coordination.py` |
| Exact-source check receipts | `mpc_forge/devflow_lib/core.py`: `Context.checks`; `mpc_forge/devflow_lib/workflow.py`: `verify_increment`, `handoff` | `tests/devflow/test_coordination.py` |
| Pending merge journal and idempotent retry | `mpc_forge/devflow_lib/workflow.py`: `admit` | `tests/devflow/test_coordination.py` |
| Revision/assignment concurrency checks | `mpc_forge/devflow_lib/task_contract.py`, `mpc_forge/devflow_lib/tasks.py`, `mpc_forge/devflow_lib/ledger.py` | `tests/devflow/test_tasks.py` |
| Immutable build publication and pins | `mpc_forge/devflow_lib/builds.py` | `tests/devflow/test_builds.py` |

The transferable idea is small: agents can produce work independently, but only
an owned, exact-source, checked, recoverable composition step turns their outputs
into an accepted combined result.
