# Structured worker claims and explicit collection v1

This is an optional **unverified evidence-capture** surface. It makes a worker's
summary, findings, file/source claims, check claims and references inspectable
without importing a transcript or changing task/execution state. It is not native
worker admission, accepted coding delivery, check verification or completion.

```text
coven handoff validate FILE [--json]
coven handoff collect WORKSPACE --file FILE --expect-sha256 HEX --expect-tasks-revision TOKEN [--json]
coven handoff inspect WORKSPACE COLLECTION_ID [--json]
coven handoff list WORKSPACE [--task TASK_ID] [--json]
```

No Djinn/Kitsune connection, native application, Git command, referenced file,
check command, URI or tool is invoked by these commands. Strings are inert data.
No dependency, automatic result discovery, watcher, scheduler or composer is added.

## Worker handoff schema

Use exact persisted task/owner IDs and the task-authority revision from
`coven task list WORKSPACE --json`. Replace the illustrative IDs/token below;
do not submit a placeholder revision as if it described the current task.

```json
{
  "schema_version": 1,
  "contract": "coven.worker-handoff.v1",
  "task": {
    "id": "review-worker-result",
    "owner": "operator",
    "revision": "tasks-sha256:<actual 64 lowercase hex digits>"
  },
  "claimed_status": "partial",
  "summary": "Implemented the assigned helper; independent review remains.",
  "confidence": "medium",
  "findings": [
    {"priority": "high", "claim": "An edge case needs follow-up.", "evidence": []}
  ],
  "files_inspected": ["src/example.rs"],
  "files_changed": ["src/example.rs"],
  "checks": [
    {"name": "unit", "claimed_outcome": "passed", "details": "Worker reports running just test.", "evidence": []}
  ],
  "blockers": [],
  "risks": ["Not independently verified"],
  "follow_ups": ["Review the edge case"],
  "references": [
    {"kind": "artifact", "uri": "file:///explicit/worker/handoff-notes.md"}
  ],
  "source_claim": {
    "state": "uncommitted",
    "repository_id": null,
    "base_sha": null,
    "source_sha": null,
    "tree_sha": null
  }
}
```

All shown fields are required; empty lists and null source claims are explicit.
`claimed_status` is `completed`, `partial`, `blocked` or `failed`; **completed is
only the worker's claim**. Confidence and finding priority are high/medium/low.
Checks use `claimed_outcome: passed|failed|not_run`, never a verified flag.
Reference kinds: transcript, artifact, check, native_session, repository. Reference
and evidence strings are opaque pointers, not validated native identity or fetched
evidence. Native session references do not bind an admitted task attempt.

`source_claim` may be null when no files are declared changed. Changed files
require an explicit source state, not a fabricated commit or unauthorized commit:

- `committed`: full lowercase same-format base/source/tree object IDs are required
  (40 or 64 hex digits). This is still a claim; cleanliness, object existence,
  ancestry, registration and changed paths are **not verified**.
- `uncommitted` or `unknown`: source/tree must be null; a known base ID is optional.
- Repository ID is a declared canonical UUID or null for unregistered/unknown
  repositories. It is not automatically adopted into repository authority.

Files are unique relative POSIX path strings; absolute paths, `..`, redundant
separators/dot components and backslashes are rejected. They are never opened or
compared with a diff. Grant, acceptance, verified, task-status and unknown schema
fields are rejected, rather than copied into trusted state.

Input is strict UTF-8 JSON: duplicate keys, nonfinite numbers, invalid scalar text,
depth over 16 and unsupported versions fail closed. Raw input and canonical
handoff are each at most 256 KiB. Summary is at most 16 KiB, finding/check details
8 KiB, ordinary list strings/paths/references 4 KiB, IDs 128 ASCII characters.
At most 64 findings/checks/references/blockers/risks/follow-ups; 16 evidence pointers
per finding/check; 256 paths per file list. Controls except tab/CR/LF in multiline
text are rejected; paths/URI fields reject all controls. Validated text is still
untrusted and JSON-escaped for human/JSON CLI output.

## Validation versus capture

`validate` reads only the selected bounded regular input file, rejecting a leaf
symlink/FIFO, and returns its parsed handoff, raw SHA256/byte count and
`assessment: valid_unverified`. It does not inspect a workspace, bind an owner,
confirm source/checks, grant permission or save an artifact.

`collect` requires the exact raw SHA256 printed by validation. It reads the input
once; file drift fails before publication. Before creating output, and again under
the existing workspace flock, it reads current task authority/roster and requires:

- Handoff task revision = caller expected revision = current task revision.
- One exact persisted task ID, exact assigned owner and membership in the current
  Coven roster. Owner aliases, pane/session labels and display names do not route.
- Supported snapshot-v1 or imported ledger authority. Corrupt/pending ledger
  authority cannot fall back to an apparently healthy `tasks.json` projection.

The roster file is bounded to 64 KiB and a legacy snapshot task file to 1 MiB for
this collector. Larger legacy inputs are unavailable here, not truncated or
reinterpreted; imported ledgers retain their existing reader bounds.

An imported task ledger remains authoritative with a broken projection. Selected
task digest covers the entire task record, including extensions, without updating
it. This match is local collection context, **not proof of worker/run identity,
accepted execution, criteria satisfaction or authorization**. A whole-task revision
can change because of unrelated work; this first slice conservatively refuses it.

Collection adds one self-contained canonical archive:

```text
output/handoffs/<collection-id>.json
```

It retains exact raw handoff JSON text, input hash/count, task ID/owner/revision/
full-task digest, and collection-time canonical workspace path. ID is a
domain-separated SHA256 of that payload. No mutable index, timestamps, new task
authority, lifecycle record or binding registry is added. Workspace path is
historical context, not a stable/global workspace identity; moves/copies do not
automatically establish equivalent execution identities.

The report says `collected_unverified`, authorization none, dispatchable false,
effect `local_artifact_capture_only`. Fixed verification fields leave worker/native
identity and task completion unestablished, source unverified and checks worker-
reported. Worker data cannot replace these fields. No preparation is consumed,
cancelled, completed or released; no task/ledger/agent/group/checkpoint/runtime
authority, dashboard, log or native state is changed. A missing persistent workspace
lock can be created for capture. Runtime transport admission is not used for a
non-native artifact write; unrelated broken runtime controls are not new authority.

## Publication, retries and bounds

Publication uses a private synced temporary file, exclusive hard link to the final
name (no overwrite), and directory syncing. An identical raw handoff/current
binding/workspace context yields the same ID. Existing bytes must match exactly;
recollection confirms sync durability instead of replacing them. Hashes are
integrity identifiers, **not authentication, anti-rollback or OS immutability**.
Static path checks and the flock coordinate cooperating local writers, not hostile
same-user editors or a distributed filesystem transaction.

Exit 0 plus delivery `committed` means confirmed local capture, not a completed
task. Exit 2 plus `durability_uncertain` means final bytes are observed but sync
confirmation failed. The report retains ID/path. Before-publication failure adds
no final artifact, though created parent/lock directories or inert temps can remain.
Never promote temporary files implicitly. Identical recollection needs the original
input and a still-current task binding; if context changed, inspect the historical
artifact and make a separate decision. This is not task-ledger frozen receipt
replay, durable native request reconciliation or permission to rerun a worker.

Artifacts are at most 1 MiB; directory entries are bounded to 256, retained byte
accounting to 32 MiB, including known inert temps. New capture reserves both temp
and final entries/size headroom for interrupted hard-link publication. Existing
valid captures can be reconfirmed at capacity. No pruning/reset/quarantine command
is supplied; rejected external input is left untouched. Managed symlinks and
nonregular archive entries fail closed. No hard filesystem syscall/time bound is
claimed; imported authority uses its existing retained-history reader.

## Historical inspect/list

Reads create no lock, fsync, repair or output paths. They validate canonical archive
encoding, content ID, raw input hash/count, embedded schema and captured binding.
Current task/roster/runtime state is not loaded: reports explicitly say
`current_task_binding: not_checked`. A later task change or broken source cannot
turn historical evidence into current acceptance, nor hide a valid old artifact.

`list` validates the entire bounded cohort before applying `--task`. It returns
only summaries/IDs/declared task/status, not automatic transcript/check/artifact
imports; observed and selected counts remain separate. A corrupt archive fails the
list rather than disappearing behind a filter; exact-ID inspection of another
valid archive remains available. Known inert temp names are not collection entries.

## Scope and checks

This is the evidence-capture foundation, **not completion of COV-09**. Verified
native attempt/result binding, source/cleanliness/scope verification, executable
check receipts, accepted results, dependency readiness and composer integration
still require separate contracts. Preparation-only attempts are not dispatched or
upgraded by collection, and reported success never releases their reservations.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -p test_handoffs.py -v
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -q
```

Tests use private files and Python concurrency/CLI fixtures only. No native agents,
Git probes, check commands, providers or models are needed.
