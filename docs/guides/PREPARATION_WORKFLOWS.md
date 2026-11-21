# Preparation-only task attempts and reviewed graphs

These workflows explicitly opt into local preparation authority. They do **not**
submit work, start a worker, grant tools, execute acceptance checks, or authorize
dispatch. Results remain `dispatchable: false`, authorization `none`. Runtime
startup and readiness are separate; a future adapter must not auto-dispatch old
preparations. Graph review is planning review, not execution permission.

Commands using `target/debug/coven` assume the repository root and an already-built
native binary. Installed `coven` and the native-only checkout launcher expose the
same routes. Examples below are **shell-style pseudocode**, not copy/paste setup
scripts: choose new disposable absolute workspace paths, create the JSON documents,
and set variables from actual inspected output at each comment. Never reuse a stale
execution token after a mutation.

## Inspect before choosing a mutation

```bash
target/debug/coven execution inspect /path/to/workspace --json
target/debug/coven execution history /path/to/workspace --json
target/debug/coven task attempt list /path/to/workspace --task core-chatbot --json
target/debug/coven task attempt inspect /path/to/workspace '<attempt-id>' --json
target/debug/coven task graph inspect /path/to/workspace --json
target/debug/coven task graph export /path/to/workspace '<graph-revision>' --json
```

Use actual 32-lowercase-hex attempt IDs/graph revisions. Attempt inspect and graph
export require one selector, optionally preceded by a workspace by **arity**,
even if the single selector names a marked path. Graph inspect recognizes a
marker-bearing workspace first, then an optional revision (default: graph head).

Inspection validates retained history before optional current-source reads.
Ordinary source failures leave frozen facts visible with native diagnostics;
actual resource/work/output/encoding-profile failures exit 1 without stdout.
Export is source-independent and needs one full replay, but corruption anywhere
in retained history is fatal. Readers write/fsync nothing. Existing-lock source
observations confer neither freshness nor mutation permission. See
[native inspection notes](../plans/RUST_MIGRATION.md#native-execution-inspection-and-graph-export)
for precedence, filtering, budgets, and graph eligibility semantics.

## Fresh native standalone loop

Choose a fresh absolute `$WS`. Create `$INTENT` before preparation, using the
[intent schema](../architecture/EXECUTION_INTENT_CONTRACT.md#explicit-enable-selected-enrollment-and-preparation):
the exact task ID, exact assigned owner `operator`, and explicit declarative
acceptance criteria. IDs are not display labels; paths/commands inside criteria
are data, not executed checks. The schema link is authoritative for fields/limits.

```sh
target/debug/coven init "$WS" --blank --agents operator,reviewer
target/debug/coven task add "$WS" "Review parser" --owner operator --json
target/debug/coven task list "$WS" --json
# Set TASK and TASKS from the actual task ID and task revision above.
target/debug/coven execution inspect "$WS" --json
# Set EXEC from the inspected disabled revision (execution-disabled:v1).
target/debug/coven execution enable "$WS" --expect-execution-revision "$EXEC" --request-id enable-1 --json
target/debug/coven execution inspect "$WS" --json
# Set EXEC to the new inspected revision.
target/debug/coven execution enroll "$WS" "$TASK" --expect-tasks-revision "$TASKS" \
  --expect-execution-revision "$EXEC" --request-id enroll-1 --json
target/debug/coven execution inspect "$WS" --json
# Update EXEC; write $INTENT with this exact TASK, owner operator and explicit criteria.
target/debug/coven task attempt prepare "$WS" "$TASK" --intent "$INTENT" \
  --expect-tasks-revision "$TASKS" --expect-execution-revision "$EXEC" \
  --request-id prepare-1 --json
target/debug/coven task attempt list "$WS" --task "$TASK" --json
# Set ATTEMPT from the actual attempt ID; inspect execution again and update EXEC.
target/debug/coven execution inspect "$WS" --json
target/debug/coven task attempt cancel "$WS" "$ATTEMPT" --reason "Revise assignment" \
  --expect-execution-revision "$EXEC" --request-id cancel-1 --json
```

Enable enrolls zero tasks; enrollment explicitly selects a task and retains its
complete snapshot/stable source identity. Preparation changes no task bytes/status,
does not upgrade task-ledger-v1, and starts no worker. One immutable prepared
attempt per task records source revision, owner, criteria/digests, and predecessor.

## Cancel locally before reassignment, not to kill a worker

A prepared task is guarded against fresh task-API mutation/reassignment, including
assignment to the **same owner**, until cancellation is durably confirmed. Changed
sources/owners are diagnostics, not automatic guard release. Unrelated task changes
remain allowed. Cancellation is **cancelled before submission**, never native
termination or proof that execution stopped. It preserves the preparation and
allows a new numbered attempt after reassignment.

Cancellation reads no current task source/roster and needs no task revision, so it
remains available with stale/corrupt current inputs. Use inspected values:

```bash
target/debug/coven task attempt cancel WORKSPACE '<actual-attempt-id>' \
  --reason "Revise assignment" --expect-execution-revision '<inspected-execution-token>' \
  --request-id cancel-review-1 --json
```

Import/resume of task authority requires no prepared attempts. After cancellation
and a source-identity change, explicitly use `execution enroll --rebind-source`
with current task/execution CAS and a new request key. Old preparations/receipts
remain bound to their original source; do not transplant them.

## Replay original requests; do not automate uncertainty away

Execution effects and frozen receipts share one bounded atomic control file.
Every writer requires a request key and execution CAS; enrollment/preparation also
require task CAS. Retry only the original key/content/revisions. Receipt lookup
precedes mutable source/CAS checks and returns the frozen result even after local
cancellation. Changed bindings conflict; precommit rejections are not cached.

Intent/document adapters still read and validate files **before** invoking the
mutator, even on replay. Preserve original parsed values, not just pathnames.
Formatting/path changes do not change binding, but text/criteria changes do.

Confirmed new/replayed results exit 0. Known-result verification/durability
uncertainty exits 2; current revision/state may be unknown while the known receipt
is retained. Ordinary errors exit 1 (parser errors can also exit 2). Inspect rather
than blindly choosing a new key. A frozen preparation or approval result describes
past facts, not fresh permission. Consult live `delivery`/inspection separately.

Pending/corrupt execution control fails closed for fresh task effects; it never
silently resets to disabled or repairs activation. Corrupt current source does not
hide valid frozen attempts. Inspection never creates an absent workspace lock.
These are cooperative task-API guards, not source-path reservations, tool policy,
a sandbox, scheduling, or unrestricted-runtime controls.

## Fresh native reviewed two-node loop

Choose a **separate fresh** absolute `$WS`. Set `$A`, `$B`, and `$TASKS` from the
actual task IDs/revision in `task list --json`. Write `$GRAPH_DOC` with the
[graph document schema](../architecture/TASK_GRAPH_CONTRACT.md#author-a-bounded-dag),
exact task/owner IDs and explicit criteria, with B depending on A. Prepare no
attempts yet. Execution stays v1 until an explicit backed-up upgrade; cancel all
existing preparations before upgrading. Task snapshots/task-ledger-v1 are unchanged.

```sh
target/debug/coven init "$WS" --blank --agents operator,reviewer
target/debug/coven task add "$WS" "Review parser" --owner operator --json
target/debug/coven task add "$WS" "Reconcile findings" --owner reviewer --json
target/debug/coven task list "$WS" --json
# Set A, B and TASKS; write GRAPH_DOC with those exact IDs and owners.
target/debug/coven execution inspect "$WS" --json
# Set EXEC from the disabled revision.
target/debug/coven execution enable "$WS" --expect-execution-revision "$EXEC" --request-id enable-1 --json
target/debug/coven execution inspect "$WS" --json
# Update EXEC.
target/debug/coven execution enroll "$WS" "$A" --expect-tasks-revision "$TASKS" \
  --expect-execution-revision "$EXEC" --request-id enroll-a-1 --json
target/debug/coven execution inspect "$WS" --json
# Update EXEC.
target/debug/coven execution enroll "$WS" "$B" --expect-tasks-revision "$TASKS" \
  --expect-execution-revision "$EXEC" --request-id enroll-b-1 --json
target/debug/coven execution inspect "$WS" --json
# Set EXEC from the current v1 revision.
target/debug/coven execution upgrade "$WS" --expect-execution-revision "$EXEC" --request-id upgrade-1 --json
target/debug/coven execution inspect "$WS" --json
# Update EXEC to v2.
target/debug/coven task graph propose "$WS" --document "$GRAPH_DOC" --expect-tasks-revision "$TASKS" \
  --expect-execution-revision "$EXEC" --request-id graph-1 --json
target/debug/coven task graph inspect "$WS" --json
# Inspect manifest/criteria; set GRAPH, DIGEST and EXEC from this report.
target/debug/coven task graph decide "$WS" "$GRAPH" --decision approve --reason "Reviewed criteria" \
  --expect-graph-digest "$DIGEST" --expect-tasks-revision "$TASKS" \
  --expect-execution-revision "$EXEC" --request-id review-1 --json
target/debug/coven execution inspect "$WS" --json
# Update EXEC; either node may be prepared, even B before A.
target/debug/coven task attempt prepare "$WS" "$B" --graph-revision "$GRAPH" \
  --expect-tasks-revision "$TASKS" --expect-execution-revision "$EXEC" \
  --request-id prepare-from-graph-1 --json
target/debug/coven task attempt list "$WS" --task "$B" --json
target/debug/coven execution inspect "$WS" --json
# Update EXEC. Revocation has no tasks pin and does not cancel B's attempt.
target/debug/coven task graph decide "$WS" "$GRAPH" --decision changes-requested --reason "Revise criteria" \
  --expect-graph-digest "$DIGEST" --expect-execution-revision "$EXEC" --request-id revoke-1 --json
target/debug/coven task graph export "$WS" "$GRAPH" --json
```

## Understand the review gates, not a dispatch gate

The bounded DAG selects already-enrolled tasks, exact owners, criteria, and
`depends_on` edges; duplicate/dangling/self/cyclic edges are rejected. Proposal
freezes selected complete snapshots/enrollment bindings, and review applies to one
immutable criteria revision. Successful proposal makes node IDs **sticky managed**:
unreviewed/revoked/superseded or successor-removed tasks cannot bypass review using
`--intent`. Unmanaged tasks retain standalone preparation.

Fresh approval requires displayed graph digest and current task/execution CAS.
Preparation rechecks current head/review and all selected snapshots, owners, and
bindings. Covered changes stale the plan; unrelated whole-source revision drift
is reported separately, not mistaken for a selected-content change. Successors
require cancellation of managed preparations and fresh review. Source-identity
change requires explicit rebind plus successor proposal/review, not transplantation.

`changes-requested` needs no task pin, remains available with unhealthy current
source, and does not cancel old attempts or release their guards. Generic checkpoint
approval is **not** graph review, and graph review is **not** execution approval.
The [checkpoint contract](../architecture/CHECKPOINT_CONTRACT.md) covers a separate
local content-bound decision, not linked plans or graph authority.

Dependencies express intended accepted-output order for execution, not preparation
order. B may be prepared before A: dependency acceptance is `unestablished`, every
node remains non-dispatchable with authorization `none`, including roots. Historical
manifests/receipts remain readable; v1 receipt values survive upgrade unchanged.
See [graph gates and exclusions](../architecture/TASK_GRAPH_CONTRACT.md#immutable-criteria-and-sticky-preparation-gates)
for precise review, capacity, replay, and freshness semantics.

## Use maintenance only after inspecting evidence

Maintenance does not implicitly enable, enroll, prepare, or launch anything:

```bash
target/debug/coven execution history WORKSPACE --json
# Separately inspect the current token before explicit durability confirmation.
target/debug/coven execution recover WORKSPACE --confirm-authority \
  --expect-execution-revision '<current-inspected-execution-token>' --json
```

Recovery failures exit 1. Only for a separately inspected disposable pending
execution directory with **no control entry**, use
`target/debug/coven execution recover PENDING_WORKSPACE --archive-pending --json`.
This archives the whole execution directory, not a repair/reset; missing control
does not prove no older preparations existed. Archival permits disabled-mode task
changes afterward. Post-rename sync failure exits 1 with empty stdout and a known
destination diagnostic, without rollback. Inspect before retrying.

**Historical cancellation liveness limit:** a valid older predecessor may have no
admissible native cancellation successor. Exact stored bytes including LF must fit
8 MiB, combined history 1024 entries, prefix/replay work 256 MiB, and finite output
32 MiB. Python's 512 KiB cancellation reserve does not guarantee these native
profiles. New native authoring reserves cancellation/review-revocation headroom;
it does not rehabilitate arbitrary historical control. No pruning, raised limits,
or fallback is provided. See the
[native writer contract](../plans/RUST_MIGRATION.md#native-execution-authoring)
and [execution capacity/recovery contract](../architecture/EXECUTION_INTENT_CONTRACT.md#capacity-and-recovery)
before maintenance. Do not edit authority/history to make it fit.

## Related files

- [Task workflows](TASK_WORKFLOWS.md): task authority, CAS, import, and recovery.
- [Execution intent contract](../architecture/EXECUTION_INTENT_CONTRACT.md): preparation and cancellation.
- [Task graph contract](../architecture/TASK_GRAPH_CONTRACT.md): document schema and review authority.
- [Native execution writers](../../src/execution/mutation/commands.rs) and
  [source capture](../../src/execution/source.rs): implementation boundaries.
