# COV-07a: immutable local repository registration

## Overview

One explicit registration pins an existing checkout, a full base commit and its
root tree, and an intended delivery ref in `state/repository.json`. This is a
fact-only prerequisite, not an execution, mutation, integration, or delivery
grant. Every report has `authorization: none` and `dispatchable: false`.

There is no reset, replace, unregister, checkout creation, task/attempt binding,
scheduler, composer, or native worker operation. Coven does not change target Git
refs, index, configuration, or checkout contents. Dirty work is allowed but is
excluded from the base and never copied. No remote contacts are requested and no
object-retention guarantee is established.

This independent prerequisite does not advance automated M2 rollout while M1's
native admission/reconciliation/policy contract remains blocked. Registration
does not bypass that prerequisite or confer worktree or tool-policy permissions.

## API / Interfaces

```text
coven repository inspect [workspace] [--verify] [--json]
coven repository register [workspace] --path CHECKOUT --base FULL_COMMIT \
  --delivery-target refs/heads/NAME --expect-repository-revision TOKEN [--json]
```

The optional orchestration workspace defaults to `.`. It must already exist with
a regular `coven.json` marker and nonsymlink `state/` directory; registration does
not parse task, roster, graph, execution, checkpoint, or runtime authority.

Inputs:

- `--path` selects the existing checkout root, not a subdirectory or bare repo.
  It must be nonempty, NUL-free, at most 8,192 filesystem-encoded bytes, and contain
  no `..` path components. Relative paths are made absolute against the process
  working directory, **not** the selected orchestration workspace. Syntactic
  normalization precedes replay lookup; symlink resolution does not.
- `--base` must be a full lowercase 40-digit SHA-1 or 64-digit SHA-256 commit ID
  matching the repository format. Branches, abbreviations, revision expressions,
  annotated tag objects, trees, and blobs are rejected rather than peeled.
- `--delivery-target` is a syntactically validated full `refs/heads/...` ref, at
  most 1,024 characters. It is intended destination text only: Coven neither
  requires it to exist nor resolves, creates, reserves, or updates it.
- Fresh registration requires `--expect-repository-revision
  repository-unregistered:v1`. A registered revision does not authorize replacement.
  On retry, retain the **original** expected token and other inputs.

Both output modes print ASCII-escaped JSON; `--json` selects compact rather than
indented output. The version-1 envelope contains `status`, `revision`,
`authorization`, and `dispatchable`, plus `registration` when readable.

The strict version-1 registration contains exactly:

| Fields | Meaning |
| --- | --- |
| `schema_version`, `kind` | `1`, `repository-registration` |
| `registration_id`, `registered_at` | Canonical UUID and UTC registration timestamp |
| `canonical_root`, `git_dir`, `common_dir` | Resolved absolute checkout/Git locators; linked worktrees may have distinct Git/common dirs |
| `filesystem` | `root`, `git_dir`, `common_dir`, each with integer `device` and `inode` |
| `object_format`, `base_commit`, `base_tree` | `sha1` or `sha256`, exact base commit and root-tree IDs |
| `delivery_target` | Intended full local branch ref |
| `original_request` | Syntactically normalized `path`, original `base`, `delivery_target`, and `expected_revision` |

The locator/device/inode combination is a **registration-local identity**, not a
globally intrinsic repository ID. No checkout contents are frozen by this record.

Authority reads are limited to 65,536 bytes and reject duplicate keys, malformed
UTF-8/JSON, unsupported schemas, unexpected fields, invalid identities, and
inconsistent request/base/target fields. Revisions are `repository-unregistered:v1`
for absence, otherwise `repository-sha256:` plus SHA-256 of
`b"coven.repository-registration:v1\0" + exact_authority_bytes`. Formatting changes
change the token; these are content fingerprints, not approval revisions.

Core interfaces in `runner/repositories.py` are `validate_inputs`,
`read_registration(workspace)`, `inspect_repository(workspace, verify=False)`, and
`register_repository(workspace, path, base, delivery_target, expected_revision)`.
`read_registration` returns a validated record or `None`; invalid authority raises
`RepositoryError`, a `storage.StorageError`. Inspection reports unavailability
instead of treating malformed authority as absence.

## Data Flow

### Inspection

Default inspect reads only workspace structure and registration authority. It
invokes no Git, acquires/creates no lock, performs no fsync, and is source-free:
deleted checkouts and corrupt mutable task/graph data do not hide stored facts.

`--verify` adds bounded local Git observations without rewriting registration:

- Readable absence remains `status: unregistered`, with
  `observations: {available: false, reason: unregistered}`.
- A readable registration is reobserved using its original selector. Frozen
  locators, filesystem identity, object format, base commit/tree, and target must
  match. Probe failure or a changed frozen identity yields `status: unavailable`
  and reason `repository_unavailable_or_changed`, **retaining the stored
  registration and revision**.
- Malformed/unreadable authority or unsafe workspace structure yields
  `status: unavailable`, `revision: null`, no registration, and diagnostic
  `repository_authority_unavailable`. Verify adds reason `authority_unavailable`.

Successful live observations contain current `head` (`commit` plus commit ID, or
`unborn` plus ref), `dirty`, `tracked_changes`, `untracked`, and
`excluded_from_base: true`. Status ignores submodule changes. These are ephemeral
observations, never authority; HEAD need not equal the base, and later HEAD/dirty
changes alone do not invalidate registration. No current observation is implied
by default inspect or an exact registration retry.

### Registration and retries

1. Validate syntax and read authority before opening Git. If a record exists,
   compare the normalized original request, including its original expected token.
   An exact match replays it without reopening the checkout, even if deleted or
   retargeted; different inputs conflict, including a different symlink alias that
   resolves to the same checkout.
2. For fresh authority, require the unregistered CAS token and observe the
   checkout before lock acquisition. Acquire `state/.workspace.lock`, recheck
   authority, and repeat observation. Frozen facts must match before publication;
   HEAD and dirty state are not frozen comparison fields.
3. Publish one complete record through same-directory temp write, file fsync,
   atomic replace, and directory fsync. Re-read exact published bytes and confirm
   file/directory durability. There are no secondary log/dashboard updates.
4. Exact retries confirm durability under the workspace lock and return
   `replayed: true`, with `observations: {status: not_assessed}`. Initial publication
   returns `replayed: false` and live observations. A concurrently published exact
   request can also be replayed under the lock.

Known publication/replay results are preserved when durability confirmation,
authority rechecking, or retry lock acquisition fails: they return
`delivery.outcome: durability_uncertain`, not a fictional rollback or ordinary
failed registration. The retained revision identifies the known record, not proof
that current authority remains unchanged. Success uses `durably_committed`.

| Exit | Meaning / response |
| --- | --- |
| **0** | Durably committed registration/retry, or readable inspect (including unregistered inspect with `--verify`). Readability alone is not checkout availability. |
| **1** | Prepublication error/conflict/busy failure, or inspect `status: unavailable`. Inspect and fix explicitly; no reset is offered. |
| **2** | Known publication/replay with uncertain durability. Retain the result and retry the original exact request/token, never a new base/path/target. |

A lost response or killed process is not proof of failure. Inspect stored
authority and retry the original request. Temporary files are not authority and
are never promoted as recovery. Never delete/replace the persistent workspace
lock to force progress. These are cooperating single-host POSIX guarantees, not
a multi-file or distributed transaction.

## Edge Cases

Git **2.48 or newer** is required. SHA-1, SHA-256, and existing linked worktrees
are supported subject to conservative exclusions:

- Split-index configuration unless explicitly false, and any `sharedindex.*`
  artifact in either Git directory, including stale artifacts and case variants.
- Promisor/partial-clone configuration or `.promisor` pack artifacts; alternate
  object stores (`alternates`, `http-alternates`, alternate-ref commands).
- Local includes/conditional includes, filters, worktree-specific configuration,
  and extensions other than `extensions.objectformat`.
- Unsupported object-directory/pack layouts, excessive directory entries, and
  missing, malformed, oversized, or slow probe results.

The base must name an actual commit. Coven hashes the raw commit and its raw root
tree against their object IDs and validates root-tree framing, modes, names, and
canonical ordering. It does **not** validate full history or recursive blob/subtree
closure. Successful observations explicitly report
`base_object_validation: commit_and_root_tree_only` and
`complete_checkout_availability: unestablished`. A valid registration is not proof
that a complete checkout can be materialized or that objects will be retained.

Git is selected from the caller's `PATH` and trusted as executable code. Probes use
direct allowlisted argv, a restricted environment, disabled optional locks,
fsmonitor/hooks/automatic maintenance/replacement objects/lazy fetching, and denied
protocols. They are supervised with a 3-second per-probe budget, 20-second
per-observation budget, and 1 MiB combined stdout/stderr limit per probe. Raw Git
diagnostics are withheld. This is not a sandbox, security boundary, authentication
mechanism, or guarantee against malicious Git or concurrent filesystem changes.
Editors and unrelated Git processes do not participate in Coven's lock; observations
are not atomic snapshots and checkout contents are neither copied nor frozen.

## Example usage

Use an existing orchestration workspace and independently reviewed full commit ID:

```bash
coven repository inspect /path/to/orchestration --json
coven repository register /path/to/orchestration \
  --path /path/to/checkout --base '<full lowercase commit ID>' \
  --delivery-target refs/heads/main \
  --expect-repository-revision repository-unregistered:v1 --json
coven repository inspect /path/to/orchestration --verify --json
```

Substitute the real commit ID, not `HEAD`. On uncertain/lost registration delivery,
repeat the original register command unchanged, including the unregistered token;
do not substitute the returned registered revision. Default inspect remains
available without opening the checkout. Neither command enables execution.

## Related files

- [runner/repositories.py](runner/repositories.py): authority, Git observation,
  immutable publication and retry contract.
- [runner/coven.py](runner/coven.py): repository CLI parser, handlers and exit mapping.
- [runner/storage.py](runner/storage.py) and [STORAGE_CONTRACT.md](STORAGE_CONTRACT.md):
  shared workspace lock and single-file publication primitives.
- [tests/test_repositories.py](tests/test_repositories.py): disposable Git/core
  regression fixtures and deterministic subprocess-boundary tests.
- [tests/test_repository_commands.py](tests/test_repository_commands.py): CLI,
  wrapper, authority preservation, concurrency, and process-death regressions.
- [TASK_GRAPH_CONTRACT.md](TASK_GRAPH_CONTRACT.md),
  [EXECUTION_INTENT_CONTRACT.md](EXECUTION_INTENT_CONTRACT.md), and
  [WORKER_READINESS_CONTRACT.md](WORKER_READINESS_CONTRACT.md): independent
  preparation and native readiness boundaries, unchanged by registration.
