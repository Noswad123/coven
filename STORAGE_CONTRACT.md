# COV-02a: default snapshot-v1 task mutation contract

This documents implemented single-host behavior, not the complete COV-02 ledger
roadmap. The code is in `runner/storage.py` and the task services in
`runner/coven.py`. It supports cooperating current Coven processes on local
POSIX filesystems (macOS/Linux). It is neither a distributed lock nor a sandbox.

This contract applies before explicit task-ledger import. The implemented opt-in
authority, replay, and durable request-receipt protocol is documented separately
in [TASK_LEDGER_CONTRACT.md](TASK_LEDGER_CONTRACT.md). Nothing imports implicitly.

## Authority and compatibility

| Resource | Authority / role |
| --- | --- |
| `state/tasks.json` | Authoritative v1 task list. A task mutation replaces this one file. |
| Checkpoint Markdown | Current local decision authority; revision-bound single-file protocol is documented separately below. |
| Metadata, goal, other state | Existing file authorities; no new cross-file transaction. |
| `logs/events.jsonl`, `logs/messages.jsonl` | Observational append-only history, not a replay ledger. |
| `dashboard.md` | Regenerable projection. |
| `state/.workspace.lock` | Persistent coordination inode, not state or ownership truth. |
| `state/runtime-launch-control.json` | Opt-in single-use startup consumption/outcome authority; see the runtime-launch contract. |

No task envelope, revision sidecar, baseline import, or implicit migration is
introduced. Legacy IDs and extension fields remain intact. New workspaces declare
`source_of_truth = "workspace-files"`; old declarations mentioning events remain
untouched descriptive metadata and do not make old logs replayable.

All cooperating task changes must use `task add` / `task assign`, not direct
JSON writes. New role and starter prompts explain this; refresh preserves custom
role prompts, so existing sessions must stop following any old direct-edit
instructions. Report completion/blockers with `coven message` until an explicit
task lifecycle API exists. Do not invent unsupported status-update commands.

## Lock scope

The workspace lock covers the complete task reload–validate–modify–replace
sequence, task-ID allocation, and its secondary event/dashboard writes. All Coven
event/message appenders use the same lock. Every dashboard publisher locks the
actual render and publication, including `status` and `refresh`; locking just
the final rename would allow an old render to overwrite a newer one.

Goal and lead writers now serialize/atomically publish their individual files
(lead clear syncs its removal), so runtime manifests select coherent cooperating
input writes. This is not an aggregate file/event/runtime transaction.
Checkpoint decisions now have their own serialized, content-bound single-file
publication protocol in [CHECKPOINT_CONTRACT.md](CHECKPOINT_CONTRACT.md), still
without a checkpoint+event+runtime transaction. Runtime I/O and interactive input
are not held inside a global CLI lock.

The optional [runtime-launch contract](RUNTIME_LAUNCH_CONTRACT.md) adds a separate
shared-legacy/exclusive-activation transport gate. Acquire it before the workspace
lock, never upgrade or force-delete it. Legacy transports can finish their native
I/O while admitted; activation waits outside the workspace lock. Approved launch
execution runs after durable claim and outside both locks.

The lock is opened without truncation and held with exclusive POSIX `flock`.
Acquisition waits at most five seconds, then reports busy. Nested helpers reuse
an explicit active token checked against workspace, lock path, process, and
thread. Descriptors are non-inheritable. Root aliases resolve to the same lock.

Never delete, replace, or “force unlock” the lock file while processes might use
it. Its existence is normal; the kernel releases the lock when a holder exits or
dies. There is no PID-based stale-lock stealing. Old binaries, manual editors,
noncooperating workers, network-filesystem behavior, hard-link attacks, and
concurrent filesystem redirection are outside these guarantees.

Read-only task listing does not create a lock. Valid mutations and dashboard
publication may create it on first use in a legacy workspace. Deterministic
validation failures are checked before creation; a later rejection caused by
racing changes can still leave a new persistent coordination file.

## Conditional task changes

`task list --json` returns `{"revision": "...", "tasks": [...]}`. Its revision
is derived from the **same exact bytes** used to obtain that validated task list:

- Existing file: `tasks-sha256:<64 lowercase hexadecimal digits>`.
- Missing legacy file: `tasks-missing:v1` (distinct from a present empty list).

`task add` and `task assign` accept `--expect-revision TOKEN` and `--json`.
The expected token is checked again under the lock. A mismatch reports the
current token and applies no task/history/dashboard changes; Coven does not
silently retry the caller's condition using a new token.

Without a condition, the command operates on the latest state read under the
lock. Concurrent adds both survive and receive distinct IDs. Unconditional
assignment is serialized reassignment: the last successful assignment wins.
Two conditional assignments against one token have one winner, but this is
**not a task-claim or lifecycle protocol**.

Tokens are opaque content fingerprints, not monotonic revisions. Formatting
changes invalidate them; changing and reverting to identical bytes restores the
same token. They are not approval or run revisions. Random mutation/event IDs
help correlate observed work but provide no request deduplication or sequence.

## Commit and failure boundaries

Before locking, a task service validates records/arguments/revision/history and
discards a proposed draft. Inside the lock it reloads everything and constructs
the real proposal. It validates and serializes the task/event inputs before
authoritative replacement.

The snapshot is written to a unique same-directory temporary file, flushed and
file-fsynced, then published with `os.replace`; the containing directory is
fsynced afterward. Existing file mode bits are preserved. Supported readers see
an old or new complete task file, not in-place truncation. Universal power-loss
durability across devices/filesystems is not claimed.

Only after durable snapshot replacement does Coven append/fsync task history
and publish the dashboard atomically. These are separate effects, not a
snapshot+event+projection transaction:

| Task command result | Meaning / safe response |
| --- | --- |
| Exit 1 before replacement | No task replacement. Fix the reported invalid/stale/busy condition and inspect before resubmitting. |
| Exit 0, `outcome: committed`, empty warnings | Snapshot committed; secondary operations completed. |
| Exit 0, `outcome: committed`, warnings | Task committed; history may be incomplete/uncertain or dashboard stale. Do not blindly repeat an add. |
| Exit 2, `outcome: durability_uncertain` | Snapshot was replaced, but directory sync failed. No secondary writes are attempted. Inspect the current snapshot before any retry. |
| Process died or response lost | Caller does not know the outcome. Inspect current tasks/revision; never assume failure means no commit. |

Mutation JSON also includes the task, before/after revisions, mutation ID, and
warnings. Human-readable mode prints warnings to stderr. A secondary failure
never rolls back an authoritative task commit or turns it into an ordinary
failed-task result. No exactly-once/idempotent retry guarantee exists: repeating
an unconditioned add can create another task.

## History corruption and recovery

History readers no longer skip bad lines. Nonblank lines must be UTF-8 JSON
objects with finite JSON numbers, and a nonempty log must terminate in a newline.
Malformed interior records, nonobjects, invalid encoding, and a truncated final
record block protected task mutations and appends with a path/line-numbered
inspect/repair diagnostic. Blank legacy lines are permitted. This checks syntax,
not completeness, event order, or authorization. A killed append can still leave
history requiring intervention; a concurrent read may catch an in-flight tail.

Recovery currently available:

1. Inspect `coven task list --json`; task state does not depend on successful
   history appends or dashboard writes.
2. Regenerate a stale dashboard with `coven status`. `refresh` consumes history
   metadata and can remain blocked until corrupted history is inspected/repaired.
3. Preserve a backup and inspect reported log corruption explicitly. No automatic
   tail truncation, interior-line skipping, or fabricated replacement events.
4. Orphan `.tasks.json.*.tmp` files left by killed processes are not authority.
   Coven never promotes or deletes them as recovery; retain/inspect before any
   deliberate cleanup. They do not prevent subsequent valid task commands.
5. A corrupt authoritative snapshot needs inspected restoration/repair; no event
   replay can reconstruct it from today's incomplete history.

Opt-in task-ledger baseline import and committed-request deduplication are now
available through the separate contract linked above. Snapshot-v1 deliberately
keeps the behavior documented here. Whole-workspace transactions, domain/task
lifecycle revisions, and general multi-file recovery remain open COV-02 work.
