# COV-02b: explicitly imported task ledger

This is **opt-in, task-only** authority and deduplication of committed requests.
Ordinary workspaces remain snapshot-v1 under [STORAGE_CONTRACT.md](STORAGE_CONTRACT.md).
Goals, agents, checkpoints, runtimes, and observational logs are not migrated.
The cooperating-process, single-host local POSIX scope still applies.

## Explicit activation

Stop old binaries and direct-file-editing workers before importing. A marker
cannot force old software to obey the new authority.

```bash
coven task list --json
# Substitute actual tokens from output; placeholders below are not valid tokens.
coven state import-tasks --expect-revision '<snapshot revision>' --json
coven state inspect --json
coven task list --json
coven task add "Investigate rendering" --owner jamal \
  --expect-revision '<ledger revision>' --request-id rendering-1 --json
```

Commands accept an optional workspace path. Init/refresh never import implicitly.
`--request-id` is rejected in v1 and required for every imported add/assign.
`state inspect` is read-only: unlike `status`, it neither publishes a dashboard
nor confirms fsync durability. Pending corrupt sources report `source_error`.

```text
state/
├── task-authority.json                 # sole activation marker
├── task-ledger/<epoch-id>/
│   ├── manifest.json
│   ├── baseline.tasks.bytes            # exact source, if present
│   └── frames/
│       ├── 00000000000000000000.frame   # complete validated baseline
│       └── 00000000000000000001.frame   # effect and committed receipt
└── tasks.json                          # projection after activation
```

Import allocates stable random workspace/epoch IDs, preserves source bytes and
mode in backup/manifest, records absence explicitly, and syncs baseline files and
directory entries before publishing the marker last. **Marker publication
switches authority.** Metadata/config are unchanged; descriptive `source_of_truth`
fields are not alternate selectors.

No marker/artifacts means v1. A ledger directory without a marker is unresolved
import: valid source remains readable, but new task mutations are blocked until
explicit recovery. A valid supported marker selects its ledger; corrupt/missing
authority never falls back to the snapshot. Moving a workspace preserves IDs;
independently writable copies are not coordinated. Active epochs cannot be aborted.

## Commit and replay

One immutable sequenced frame contains the structured request, complete resulting
task, mutation ID, and frozen result. Effect and receipt share one publication
boundary, with no independent receipt sidecar or authoritative HEAD file.
Frame zero captures current state—not a reconstruction of historical events.

Replay validates versions, IDs, contiguous filenames/sequences, predecessor hashes,
canonical JSON, payload length/hash, effects, request uniqueness, and result/revision
consistency. It never calls the clock, allocates IDs, or resolves old selectors
against today's roster. Duplicate JSON keys, nonfinite numbers, malformed final
frames, gaps, and unexpected entries are corruption, not disposable tails.
Frames are capped at 8 MiB. Hashes bind content but are not authentication.

A temporary file is written/fsynced, linked to its final sequence name without
overwriting, then directory-synced. Known temp files are inert, never promoted.
A synced final frame commits both effect and receipt. Projection/event/dashboard
writes are independent secondary effects, not a whole-workspace transaction.

Ledger tokens contain workspace/epoch IDs, sequence, and canonical task-state
digest. They advance within an epoch and differ from v1 byte fingerprints; they
are not approval/run bindings, lifecycle claims, or distributed coordination.
Frame hashes are separate, avoiding a circular hash of the result's own revision.

## Retry the original request

Keys are 1–128 ASCII characters: letter/digit first, then letters/digits or `._:-`.
Each binds operation, raw selectors/text, expanded defaults, and expected revision
(including absence). Same key/binding returns the frozen result; changed operation,
arguments, or revision conflicts. Precommit rejections are not durably cached.

Receipt lookup precedes current revision and roster interpretation. Retries use
the **original key and arguments/revision**, even after later commits or valid
roster changes. Never replace a lost-response key or refresh its revision.

Ledger mutation JSON separates frozen authority and live delivery:

```json
{
  "result": {"outcome": "committed", "task": {}, "mutation_id": "...", "sequence": 1,
             "before_revision": "...", "after_revision": "..."},
  "delivery": {"outcome": "committed", "replayed": true, "warnings": [],
               "current_revision": "...", "projection": {"status": "current"}}
}
```

Real `result.task` is complete. Result values are frozen; delivery metadata and
serialization formatting are not. Replay does not repair projections or backfill
events: committed-task deduplication is not exactly-once secondary delivery.

Check **`delivery.outcome` and exit code**, not only frozen `result.outcome`:

- Exit 0: authority durability confirmed; secondary/projection warnings may remain.
- Exit 2 / `durability_uncertain`: frame/marker observed, sync failed; no subsequent
  secondary writes. Retry validates and syncs observed authority before acknowledgement.
- Lost response/process death: retry the same key/binding, not a replacement request.

No universal power-loss guarantee, pruning, compaction, or epoch rotation exists.
Reads replay retained history and can become expensive; use bounded local workflows.

## Local preparation guards

The optional [execution-intent authority](EXECUTION_INTENT_CONTRACT.md) retains
task enrollment, immutable prepared attempts, and local pre-submission cancellation
outside this ledger. It does not add operation types or upgrade ledger-v1.
Fresh add/assign effects check that authority under the same workspace lock;
prepared target tasks remain guarded even when stale. Already committed request
lookup/replay occurs first and is not hidden by an unavailable execution control.

Import/resume refuses any prepared attempt. After explicit cancellation, import
can proceed; enrollments keep their original source identity until an explicit
CAS-bound rebind. Old preparations and receipts are never transplanted. Projection
repair and pending-import archival do not change task authority and do not require
releasing preparations. These are local task-API guards, not worker admission or
native termination facts.

## Divergence and explicit recovery

Task listing, dashboard rendering, and receipt lookup read ledger authority even
with missing, malformed, stale, or unreadable projections. Health is `current`,
`missing`, `diverged`, or `unreadable`. Projection I/O errors do not hide valid
receipts. Unsafe symlink/nonregular projections are never followed or overwritten.
New requests require positively current health.

```bash
coven state inspect --json
coven state recover-tasks --repair-projection --json
```

Repair confirms authority and backs up exact observed bytes—or records absence—
under `state/task-recovery/<id>/` before publishing from replay. It never admits
manual edits into the ledger. Unreadable/unsafe paths need explicit inspection
before safe backup/replacement. Older valid bytes could be crash lag or a manual
revert; neither is silently treated as harmless. Refresh does not repair projections.

Event/message corruption does not gate ledger commits or receipts; event-append
failure becomes a delivery warning. Log-consuming commands still parse strictly.
Old observational history is never imported as authoritative past.

## Interrupted import

```bash
coven state recover-tasks --resume-import --expect-revision '<original snapshot token>' --json
coven state recover-tasks --abort-import --json
```

Resume requires one complete unactivated baseline with unchanged exact source.
Incomplete/multiple epochs are not adopted. Abort, allowed only without a marker,
renames pending artifacts to `state/task-ledger-aborted-<id>` without deleting
anything or changing source bytes—even malformed source. It does not repair it.

After uncertain activation, inspect observed active authority; do not re-import.
Use `--repair-projection` to confirm/sync authority and repair if needed. Corrupt
active authority needs inspected restoration: no frame deletion, tail truncation,
or automatic fallback. Partial import artifacts remain inspectable on failure.
