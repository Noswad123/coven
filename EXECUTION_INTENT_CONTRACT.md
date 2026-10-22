# COV-04a: preparation-only task attempts

This protocol supplies durable local attempt identity and frozen declarative
intent. It does **not** submit work, probe a provider/runtime, inspect a native
session, grant tools, execute acceptance checks, or authorize dispatch. Every
result/inspection says `authorization: none` and `dispatchable: false`.

The only attempt transition is:

```text
prepared -> cancelled_before_submission
```

No running/completed/failed/native-cancel state exists here. Existing task status
strings remain unchanged and are not reinterpreted as execution lifecycle.
Runtime-launch claims and help-only readiness reports are separate and cannot
authorize these attempts. A future adapter must not auto-dispatch old preparations.

## Explicit enable, selected enrollment, and preparation

```bash
coven execution inspect --json
coven execution enable --expect-execution-revision execution-disabled:v1 \
  --request-id enable-1 --json
coven task list --json
coven execution enroll core-chatbot --expect-tasks-revision '<task-token>' \
  --expect-execution-revision '<execution-token>' --request-id enroll-1 --json
coven task attempt prepare core-chatbot --intent intent.json \
  --expect-tasks-revision '<current-task-token>' \
  --expect-execution-revision '<current-execution-token>' --request-id prepare-1 --json
```

Substitute actual tokens after inspection. Optional workspace paths precede the
task/attempt selector. Enable enrolls zero tasks and changes no task bytes,
task-ledger frames, checkpoints, startup controls, prompts, or observational logs.
Enrollment explicitly selects one task; it does not reserve that task until an
attempt is prepared. No task-ledger operation/schema upgrade occurs.

An operator-selected intent is bounded strict UTF-8 JSON, for example:

```json
{
  "schema_version": 1,
  "kind": "task-intent",
  "task_id": "core-chatbot",
  "owner": "jamal",
  "acceptance_criteria": [
    {"id": "evidence", "text": "Describe the behavior and supporting evidence."}
  ]
}
```

Use exact persisted IDs—not display names or owner aliases. Preparation requires
an assigned source owner equal to intent owner and present in the current roster.
Criteria are explicit operator data, not inferred from legacy extension fields.
There must be 1–64 unique bounded criteria IDs; text is nonblank and at most
16 KiB each, intent at most 1 MiB. Unknown fields such as argv, environment,
native IDs, grants, or policy assertions are rejected. Commands/paths embedded
in text are never evaluated, expanded, followed, or executed.

Bind canonical intent values, not its filename or JSON formatting. Text
whitespace and Unicode are not normalized. Linked files/repositories and runtime
policy are **not** covered merely by appearing in criteria. Preserve the original
intent values and request binding for retry; changing a document under the same
key conflicts.

## One execution authority, unchanged task authorities

```text
state/execution/control.json
```

This single atomic snapshot contains schema version, random execution-authority
and snapshot-source UUIDs, generation, retained committed history/effects/frozen
receipts, and inert extensions. Replay derives enrollments, attempts, transitions,
and request lookup while checking exact shapes, identities, references, digests,
full historical CAS tokens, and result/effect consistency. Effect and receipt
publish together; there is no receipt sidecar or observational-event authority.
History is logically retained, not physically append-only or authenticated.

Activation states are deliberate:

- Execution directory absent: disabled, with `execution-disabled:v1` token.
- Directory with valid supported control: enabled.
- Directory with missing, malformed, unsupported, or unsafe control: unavailable;
  protected fresh task effects fail closed, never fall back to disabled/empty.

Snapshot-v1 tasks stay in their original list. Imported task-ledger-v1 keeps its
baseline/add/assign-only protocol. Execution-control paths are validated at their
own operation/guard boundary, not globally before historical task receipt lookup.
Read-only task listing and committed task-ledger retries therefore remain usable
when execution control is unavailable.

Snapshot source identity is an execution-control UUID plus logical managed path;
ledger source identity is its existing workspace/epoch plus ledger path. Do not
equate these with runtime activation IDs or a future run ID. Moving a workspace
preserves execution identity; independently writable copies are not coordinated.

Each immutable preparation retains task/source/enrollment identity, authoritative
source revision, complete task snapshot including unknown fields, task digest,
canonical owner and explicit intent/digest, attempt UUID/number/predecessor, and
preparation time. Cancellation appends a transition; it never changes that record.
Digests domain-separate canonical JSON. The intent digest binds execution and
enrollment identity, task-authority identity/revision, the complete task snapshot
(including any task timestamps), and explicit intent. Attempt UUID/number/
predecessor and preparation time are excluded. Snapshot byte revisions still
distinguish formatting, while control CAS uses canonical JSON and generation.

## Conditions, receipts, and single-file durability

Every mutation requires a request ID and expected execution revision. Enrollment
and preparation additionally require an expected task revision. Under the same
workspace lock as task writes, receipt lookup happens **before** current CAS,
source/roster/owner interpretation, or mutable source parsing:

- Same key and original binding returns the frozen result after durability
  confirmation, even after cancellation or broken current task/config state.
- Changed operation/arguments/criteria/revisions under that key conflict.
- Precommit rejection is not durably cached.
- A new request rechecks current control and authoritative source before publish.

Responses separate frozen `result` from live `delivery`. A historical preparation
receipt still says prepared after cancellation; `delivery.attempt_state` describes
the current local state when verified. Current control revision is not embedded
inside its own frozen receipt; results retain committed generation and stable IDs.

Publication uses same-directory temp, file fsync, atomic replacement, directory
and required parent sync. Exit 0 means committed/confirmed; exit 2 with
`delivery.outcome: durability_uncertain` means a known receipt is retained but
current durability/verification was not confirmed. `current_revision` and live
attempt state may be null; `known_control_revision` names the known snapshot
containing the receipt, not a claim about current disk state. Preserve/retry the
original key and binding, inspect, or confirm authority—do not treat uncertainty
as ordinary rejection and invent another preparation key.

Before replacement, ordinary errors commit no domain effect. A crash before
initial control publication can leave a pending activation directory. Inert temps
are never promoted automatically. No events/dashboard/native delivery is part of
this slice. Universal power-loss durability and multi-authority transactions are
not claimed.

## Reservations and source freshness

At most one prepared attempt exists per task, including stale ones. Both supported
task backends block new mutation of that task, **even assignment to the same
owner**, under the same lock. Unrelated task changes remain allowed. Cancellation
must be durably confirmed before a new task effect relies on its released guard.
An already committed task-ledger receipt remains replayable before this guard.

Task import/resume refuses any prepared attempt, including low-level entrypoints.
After cancellation, explicit import can change source identity; old records stay
bound to the old source. New preparation then needs explicit
`execution enroll --rebind-source`, with current task/execution CAS and a new key.
Retain the enrollment ID/binding history and all old attempts; never transplant
their ownership or source snapshots.

```bash
coven task attempt list --task core-chatbot --json
coven task attempt inspect '<attempt-id>' --json
coven task attempt cancel '<attempt-id>' --reason "Revise assignment" \
  --expect-execution-revision '<current-execution-token>' --request-id cancel-1 --json
```

Cancellation deliberately reads no current task source or roster and requires no
task revision. It remains available when those inputs are stale/corrupt; it is
not native cancellation or proof of stopped execution. A new preparation gets a
new number/UUID and links to its predecessor. An original cancellation key replays;
a second cancellation under a new key rejects.

Inspection never changes these domain states. It reports current/changed task,
source revision, source authority, owner, or source-unavailable diagnostics. A
whole-task revision can change due to unrelated work even when this task's content
is identical. Staleness never automatically releases a reservation. Broken current
source—including unknown nonfinite/invalid-Unicode fields—does not hide valid
frozen records. Coherent source assessment uses an **existing** workspace lock;
if absent/busy/unsafe it reports unavailable rather than creating a read-side lock
or pretending a cross-authority read was atomic.

These guards protect cooperating Coven task operations, not source files, native
tools, unrestricted runtime startup, old binaries, or direct editors. They are
not COV-08 path reservations, a worker sandbox, or execution authorization.

## Capacity and recovery

Control is bounded at 16 MiB with permanent retained receipts/history; no pruning
or reset exists. Every history-growing operation reserves 512 KiB for **each**
active preparation, enough for a valid maximum-escaped 16 KiB cancellation reason
and bounded metadata. New preparations/history are rejected before publication
rather than consuming the room needed to cancel existing preparations.

```bash
coven execution recover --confirm-authority --expect-execution-revision '<inspected-token>' --json
coven execution recover --archive-pending --json
```

Confirmation syncs validated observed authority without changing generation.
Archival is allowed only when control is absent; it preserves the entire directory
as `state/execution-pending-archive-<id>` and leaves tasks untouched. Inspect/backup
first: missing control is not proof that no older preparations ever existed, and
explicit archival permits disabled-mode task changes afterward. Present control,
even malformed, can never be archived/reinitialized by enable. Restore/repair
corruption through a separately inspected procedure. Administrative archival
I/O failures may leave the preserved rename observed; inspect before retrying.

Stop old/noncooperating writers before rollout. Hashes are not authentication or
anti-rollback: restoring old control may resurrect or release guards. Do not edit
authority/history directly. Future native admission needs a versioned upgrade,
fresh source/intent/policy checks, and separate task-scoped authorization; it must
not automatically submit the preparations recorded by this protocol.
