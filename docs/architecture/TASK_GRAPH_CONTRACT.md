# COV-04b: reviewed task graphs and graph-bound preparation

This is a useful planning/preparation consumer, **not native admission, a
scheduler, result acceptance, or an execution grant**. Current native readiness
remains unverified. Graph approval cannot start a worker or replace the missing
upstream bounded-worker contract.

## Explicit execution-control v2 upgrade

Task snapshots and task-ledger-v1 are unchanged. Only an explicitly upgraded
execution control gains graph operations:

```bash
coven execution inspect --json
coven execution upgrade --expect-execution-revision '<current-v1-token>' \
  --request-id upgrade-graphs-1 --json
```

Cancel every existing preparation first, including unmanaged tasks. Upgrade
preserves authority/source UUIDs, enrollments, old attempts, transitions, inert
extensions, original v1 bindings/results, and a validated `legacy_v1` prefix.
It backs up **exact original bytes and mode**, verifies/syncs that backup, then
atomically publishes v2 effect and upgrade receipt in the same control file:

```text
state/execution/control.json
state/execution/upgrades/<upgrade-id>/control-v1.bytes
```

An interrupted backup or pre-publication failure leaves v1 authoritative, with
inert artifacts retained. A post-publication lost response is recovered by the
same key/original request. Backups are operator evidence, not a read dependency
or alternate authority; their later absence does not hide committed v2 receipts.
There is no downgrade/reset or automatic backup adoption.

Execution v1 tokens remain valid for **their original receipt replays**. New v2
mutations require `execution-control-v2:<authority>:<generation>:<digest>`.
Legacy results are never rewritten as v2 effects. Old binaries must be stopped;
they do not acquire graph semantics by seeing new task extensions.

## Author a bounded DAG

Enroll selected existing tasks explicitly, then provide an operator JSON document:

```json
{
  "schema_version": 1,
  "kind": "task-graph",
  "nodes": [
    {"task_id": "core-chatbot", "owner": "jamal",
     "acceptance_criteria": [{"id": "evidence", "text": "Describe behavior with evidence."}],
     "depends_on": []},
    {"task_id": "qa-verification", "owner": "eric",
     "acceptance_criteria": [{"id": "reconcile", "text": "Reconcile findings and assumptions."}],
     "depends_on": ["core-chatbot"]}
  ]
}
```

The document has 1–64 unique canonical task IDs, at most 256 edges, and a 1 MiB
bound. Owners must exactly match assigned IDs in the current Coven roster, not
display names, aliases, or native role names. Criteria reuse the strict
declarative task-intent schema. Unknown/executable fields, duplicate edges,
dangling references, self-edges, and cycles are rejected. Text/paths are data,
never evaluated or followed.

`B.depends_on = [A]` means A's **accepted output is intended to precede B's
execution**, not its preparation. No accepted-result authority exists in this
slice. Downstream preparation is allowed and still reports dependency acceptance
`unestablished`, authorization `none`, and `dispatchable: false`. Root nodes are
not executable merely because they have no dependencies.

```bash
coven task graph propose --document graph.json --expect-tasks-revision '<task-token>' \
  --expect-execution-revision '<v2-token>' --request-id graph-1 --json
coven task graph inspect --json
coven task graph export '<graph-revision>' --json
coven task graph decide '<graph-revision>' --decision approve --reason "Reviewed criteria" \
  --expect-graph-digest '<graph-digest>' --expect-tasks-revision '<current-task-token>' \
  --expect-execution-revision '<current-v2-token>' --request-id review-1 --json
```

Optional workspace paths precede selectors. Inspect defaults to the current
revision; export reads immutable manifest values and never becomes another
authority or implicitly consumed runtime prompt. Planning decisions are local
to execution control, **not generic checkpoint approval**. Approving
`plan-approval.md` alone cannot review this graph.

## Immutable criteria and sticky preparation gates

Proposal freezes graph/family/revision/predecessor identity, source authority and
observed revision, selected enrollment IDs/binding numbers, complete selected
task snapshots/digests, owners, criteria, dependency edges, and proposal time.
Graph/node digests domain-separate canonical JSON. No linked-file/repository,
tool-policy, native-session, or actor-authentication coverage is implied.

One current graph family exists per execution authority. A successor creates a
new immutable revision and starts `proposed`; it never inherits review. Old
revisions/decisions remain inspectable. Only the current revision can receive a
fresh decision or supply a fresh preparation; old request receipts still replay.

**Successful proposal immediately makes its node IDs sticky graph-managed.**
Unreviewed/revoked/superseded plans block preparation rather than restoring the
old `--intent` route. Removed nodes remain managed/excluded and cannot bypass
review through standalone criteria; reintroduce them in a reviewed successor.
Unmanaged tasks retain the standalone v2 intent workflow.

Proposal and new approval require cancellation of active preparations belonging
to managed/new selected tasks. Unrelated unmanaged preparations can remain.
Revocation via `changes-requested` is allowed with active preparations and broken
current task source; it preserves those immutable preparations and their guards.
Revocation is not cancellation or native termination.

```bash
coven task attempt prepare core-chatbot --graph-revision '<current-reviewed-revision>' \
  --expect-tasks-revision '<current-task-token>' --expect-execution-revision '<current-v2-token>' \
  --request-id prepare-from-plan-1 --json
```

Intent is derived exactly from the graph node; independently editable `--intent`
input is rejected for managed tasks, even if identical. Fresh v2 preparation
records retain graph revision/digest, planning-decision ID, and node digest.
The v2 intent digest covers that binding plus execution/enrollment identity,
source identity/revision, complete task snapshot and explicit criteria; attempt
UUID/number/predecessor and preparation time remain outside the intent digest.
Old v1 preparation records/digests are not converted.

## Freshness, review, and impact

Approval requires the exact displayed graph digest and current execution/task
CAS tokens. Preparation rechecks current head/review and **all selected** task
snapshots, owners, and enrollment bindings under the workspace lock.

- Selected task/owner/notes/status/extensions changes stale the plan; propose a
  successor rather than approving an old manifest against new data.
- Unrelated task changes can change the whole-source token without changing
  selected content. Inspection reports that drift separately; approval may proceed
  after the operator inspects the current task revision. Existing attempts retain
  their original conservative whole-source freshness behavior.
- Task-authority import requires no prepared attempts. It changes source identity;
  explicit enrollment rebind **and a successor proposal/review** are then required.
  Historical graphs/preparations are never transplanted.

Inspection separates stored `planning_review`, structural dependency order,
source drift, graph/node `preparation_eligible`, affected preparations, and
unestablished dependency acceptance. None is a “ready to run” signal. Source
corruption does not hide frozen manifests, receipts, local cancellation, or
source-free `changes-requested` decisions. Inspect uses existing coordination
for source assessment; export needs no current source.

## One atomic authority and retained receipts

V2 retains the validated v1 prefix and adds validated upgrade/graph/decision/
preparation history in `state/execution/control.json`. Full historical CAS,
effect/result consistency, predecessor/head references, criteria derivation, and
graph binding are replay-validated. No task-ledger operation, graph sidecar,
checkpoint, observational log, or separately committed receipt is authority.

Every request binds canonical document values/selectors/reason/defaults and its
original expected revisions/digest. Paths and JSON formatting are not identity;
text changes are. Receipt lookup precedes current CAS/source/review checks. Same
key/binding returns frozen results; changed bindings conflict; precommit rejection
is not cached. A frozen approval receipt describes past planning state, not an
evergreen grant. Live inspection and fresh operation gates remain authoritative.

The existing fsync/atomic-replace and uncertain-delivery contract applies.
Known receipts survive post-publication verification errors, without inventing
current revision/state. Process-kill tests prove process-death recovery, not
universal power-loss durability. Backup/temp artifacts are never promoted.

Control remains bounded at 16 MiB. Every history prefix retains cancellation
capacity per active preparation **and 512 KiB revocation capacity while the
current graph is approved**. Revocation records no task-source payload and can
release its reserve without spending cancellation reserves. No pruning, receipt
eviction, downgrade, compaction, anti-rollback, or malicious-file-forgery defense
is provided. Stop old/direct writers before rollout; native tools/unrestricted
runtime startup remain outside these cooperative preparation guards.

This advances reviewed decomposition/criteria and enforceable local preparation.
Actual worker submission, accepted prerequisite results, scheduling, composers,
completion, and execution authorization still require their real contracts. Do
not substitute additional approval envelopes for the missing native admission
interface or automatically dispatch existing records when one later arrives.
