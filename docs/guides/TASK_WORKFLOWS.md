# Task workflows

Start with ordinary snapshot-v1 unless you explicitly need the opt-in task ledger.
Both support native `task add` and `task assign`, optional compare-and-swap (CAS),
task events, dashboard refresh, and human/JSON delivery outcomes. Neither is a
general task-lifecycle API or an exclusive task-claim protocol.

Examples use installed `coven` from inside an initialized workspace. An explicit
workspace path may precede task selectors/text. For `target/debug/coven`, run from
the repository root with an already-built binary. Replace every placeholder with
actual inspected output; do not paste placeholder tokens literally.
The examples assume `operator` is a declared roster ID, as in the blank
[workspace quickstart](WORKSPACE_BASICS.md#create-a-workspace-without-launching-anything);
otherwise substitute an ID from your own roster.

## Add and assign with the default snapshot

```bash
coven task add "Review parser" --owner operator --json
coven task list --json
# Use the returned task ID, declared roster ID, and exact inspected revision.
coven task assign '<task-id>' operator --expect-revision '<revision token>' --json
```

Explicit owners must resolve to declared roster IDs, not display labels. New
workspaces normalize IDs; safe legacy IDs retain their spelling. Use the task ID
returned by add/list rather than guessing it from a title.

Cooperating current processes serialize add/assign under the persistent workspace
lock, reload state inside it, and atomically replace the complete task list.
`--expect-revision` rejects a stale **whole-task snapshot** without silently
refreshing the condition or retrying. Without it, assignment is ordinary serialized
reassignment: the last successful assignment wins, not an exclusive claim.

Task listing creates no lock and derives its token from the exact bytes read.
Snapshot tokens are content hashes, or `tasks-missing:v1` for an absent legacy
file; they are not monotonic revisions or approval/run tokens. Formatting changes
invalidate a token; restoring identical bytes restores it.

The task snapshot is authoritative. History and dashboard are separate secondary
writes, not a snapshot+event+projection transaction:

| Observation | Safe response |
| --- | --- |
| Exit 0, committed, no warnings | Task committed and secondary operations completed. |
| Exit 0, committed, warnings | Task committed; inspect incomplete history/stale dashboard. Do not repeat an add blindly. |
| Exit 2, `outcome: durability_uncertain` | Replacement occurred but directory sync failed; inspect current task state before retrying. |
| Lost response/process death | Outcome unknown; inspect tasks/revision rather than assuming no effect. |

Snapshot-v1 has no durable idempotency key and rejects `--request-id`. Repeating an
add after a lost response can duplicate work. Ordinary pre-replacement errors
require fixing the reported stale/invalid/busy condition, not automatic retry.

## Respect the lock and history boundaries

Lock acquisition waits at most five seconds; the kernel releases a held lock when
its process exits. Never delete/replace `state/.workspace.lock`, mix these commands
with old binaries, or directly edit shared task JSON. Event/message appends and
dashboard render/publication use this lock too; this does not make goal, lead,
checkpoint, and runtime effects one transaction.

Generated prompts direct workers through task/message APIs. Refresh preserves
custom role prompts, so retire their old direct-edit instructions explicitly.
Report completion/blockers or unsupported lifecycle changes with `coven message`
rather than inventing status-update commands or editing task JSON.

History readers report malformed/nonobject/interior/truncated-tail records with
path/line diagnostics rather than skipping them. Preserve evidence and inspect/
repair explicitly: there is no automatic truncation or event replay recovery.
`status` can regenerate a stale dashboard; it **writes**, unlike `state inspect`.
See [the snapshot storage contract](../architecture/STORAGE_CONTRACT.md) for exact
commit/recovery rules and cooperating single-host local-POSIX scope.

## Opt into task-only ledger authority deliberately

Stop old binaries/direct-file-editing workers first. Import is explicit, preserves
the exact old snapshot, and activates marker-last; init/refresh never activate it.
It does not interpret old observational events as complete history or migrate
goals, agents, checkpoints, or runtimes.

```bash
coven task list --json
# Inspect current work, then substitute its actual snapshot token.
coven state import-tasks --expect-revision '<snapshot revision>' --json
coven state inspect --json
coven task list --json
coven task add "Investigate rendering" --owner operator \
  --expect-revision '<ledger revision>' --request-id rendering-1 --json
```

After import, listing/dashboard use replayed authority; `state/tasks.json` is a
projection. Every imported add/assign requires a request ID. Effect and frozen
receipt share one committed frame: replay the **same key with original arguments
and expected revision**, including its original absence if omitted. Changing the
bound operation, arguments, or revision conflicts. Precommit rejections are not
cached. Do not replace a lost-response key or refresh its revision automatically.

Ledger JSON separates frozen `result` from live `delivery`. Check
`delivery.outcome` and the exit code, not just `result.outcome`. Secondary warnings
still mean committed; exit 2 means observed publication without confirmed
durability. Replayed receipts are not fresh mutation permission and do not repair
projections or backfill events. This is committed-task deduplication, not
exactly-once secondary delivery.

## Inspect and recover explicitly

`state inspect` is read-only and does not confirm fsync durability. A stale,
missing, or unreadable projection does not hide authority or receipts, but new
requests require current projection health. Recovery preserves observed bytes
or records absence before replacement; unsafe paths need explicit inspection.

```bash
coven state inspect --json
coven state recover-tasks --repair-projection --json
# Alternatives for an interrupted, unactivated import only:
coven state recover-tasks --resume-import --expect-revision '<original snapshot token>' --json
coven state recover-tasks --abort-import --json
```

Choose exactly one recovery mode. Resume requires the original snapshot pin and
a complete unchanged baseline. Abort preserves/renames pending artifacts, not
deletes them, and cannot abort active authority. After uncertain activation inspect
the selected authority; do not re-import. Corrupt active authority never falls back
to v1 or permits deleting final frames. Refresh does not repair a projection.

Prepared attempts add same-lock task guards; cancel them before task import/resume
and explicitly rebind enrollment after a source-identity change. See
[preparation workflows](PREPARATION_WORKFLOWS.md). Full formats, limits, recovery,
and acknowledgement semantics belong to the
[task-ledger contract](../architecture/TASK_LEDGER_CONTRACT.md) and
[native task notes](../plans/RUST_MIGRATION.md#native-task-addassign-workflow).
Implementation entry points include [task services](../../runner/coven.py) and
[native task mutation](../../src/task_mutation.rs).
