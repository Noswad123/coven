# Explicit group live attachments and status

## Overview

An existing Coven participant can explicitly declare one pinned Djinn live source.
`group status` projects fresh **busy|unknown** evidence from declared artifacts;
it never establishes idle, health, native lifecycle, job completion or execution
permission. Attachment is a declaration, not verified terminal/session equivalence.
Status itself adds no labels or native control. A separate explicit
[`group publish --live`](GROUPS_CONTRACT.md#opt-in-live-publication) can publish fresh
scoped busy evidence only with Kitsune's generic expiry capability. See
[groups](GROUPS_CONTRACT.md) for membership/bindings and the
[live reader](DJINN_LIVE_OBSERVATION_CONTRACT.md) for source checks.

## API / Interfaces

```text
coven group attach-live WORKSPACE GROUP --member KEY --file ABS_RUNTIME_UUID_JSON --runtime UUID --instance UUID --session ID [--reattach] [--expect-revision TOKEN] [--timeout SECONDS] [--json]
coven group detach-live WORKSPACE GROUP --member KEY [--expect-revision TOKEN] [--json]
coven group status WORKSPACE GROUP [--member KEY] [--timeout SECONDS] [--json]
```

Workspace, group and member must already exist. Status without `--member` selects
all members; an empty group is valid. Timeout defaults to 1 second, finite in
`(0, 1]`: attach bounds source-read acceptance, status bounds the whole projection.
Neither interrupts blocked filesystem calls or bounds a mutation's lock/fsync time.
Output is JSON; `--json` makes it compact and ASCII-escaped.

### Stored authority: schema 3

This extends the **same** `state/groups.json`, not another authority or task state.
Root fields remain exactly `schema_version`, `workspace_id`, `content_revision`,
`groups`, `publication_counters`; accepted schema versions are `1|2|3`.
Schemas 1/2 retain exactly four member fields: `participant_id`, `key`, `role`,
`binding`. Schema 3 requires those four plus `live_attachment` on **every** member,
either null or exactly this metadata (unknown fields are rejected):

| Attachment field | Required value / constraint |
| --- | --- |
| `adapter` | `djinn_live` |
| `source_contract` | `djinn.live-observation.v1` (native source, not adapter report contract) |
| `artifact_path` | Checked resolved, normalized absolute UTF-8 path, at most 4096 bytes; no leading `//`, NUL or controls; basename exactly `<runtime_id>.json` |
| `runtime_id`, `instance_id` | Canonical lowercase RFC 4122 UUID v4 |
| `session_id` | Opaque ASCII `[!-~]{1,128}` |
| `attached_at` | UTC ISO timestamp with `T` and explicit zero UTC offset; at most 64 UTF-8 bytes; declaration time only |

No activity, sample, sequence, TTL or remaining lifetime is stored. Lexical
authority validation does not open the source or establish its freshness.
First successful attach atomically promotes schema 1/2 to 3, normalizing all
members to null before setting the target attachment. IDs, bindings and counters
are preserved; changed content increments `content_revision` once. Older readers
fail closed. Reads never migrate; detach/removal never downgrade, and process adds
use a minimum schema of 2 without reducing schema 3. New schema-3 members start null.
The `groups-v1` token format remains unchanged and covers attachment declarations.

### Ephemeral status: output schema 1

`schema_version: 1`, `contract: coven.group.status.v1` describe the **report**, not
authority schema 3. `authority_schema_version` identifies the read authority.

| Report fields | Meaning |
| --- | --- |
| `assessment`, `diagnostics` | `observed|unavailable`; safe diagnostic codes |
| `revision`, `group` | Whole-content token; group exactly `{group_id, key, name}` |
| `participants` | Selected rows, or `[]` on whole-projection failure |
| `started_at`, `observed_at`, `duration_ms`, `limitations` | UTC receipt times, monotonic duration and trust/scope caveats |
| `authorization`, `dispatchable` | Always `none`, `false` |
| `native_lifecycle`, `job_completion` | Always `unestablished` |

Each row contains exactly `participant_id`, `key`, `role`, `live_attachment`,
`association_basis`, `assessment`, `activity`, `activity_scope`, `attention`,
`freshness`, `expires_in_ms`, `source`, `diagnostics`, `authorization`,
`dispatchable`, `native_lifecycle`, `job_completion`.
Basis is `explicit_declaration` when attached, otherwise `unattached`.
Row assessment is `observed|unattached|unavailable|incompatible`; activity is only
`busy|unknown`, scope always `legacy_runner_ownership`, attention always `unknown`.
Freshness is `fresh_sample|unknown`; unobserved/expired rows have zero remaining
lifetime and null source. Authority fields have the same fixed values as the report.
`source` contains only `contract`, `runtime_id`, `instance_id`, `session_id`,
`sequence`, `sampled_at_ms`, `valid_for_ms`, `source`, `activity_scope`,
`capabilities`, as defined by the live reader; never the full artifact.

Attach/detach return normal group mutation results including `member` and
`delivery`: exit 0 for committed/unchanged, 2 for uncertain durability, 1 for
ordinary errors. Status exits **0** for a successful projection even if individual
rows are unattached/unavailable/incompatible. Local argument/authority errors exit
1; parser errors exit 2. Whole-content drift, failed recheck or deadline expiry
returns `assessment: unavailable`, `participants: []`, exit 1.

## Data Flow

1. Attach validates selectors/source syntax and local group/member/optional CAS,
   rejecting obviously changed runtime/instance/session pins without `--reattach`
   **before artifact I/O**. It freshly reads the selected source outside the lock;
   observed, fresh `busy` **or** `unknown` is accepted. No saved-report import exists.
2. Persist the reader's checked `file.resolved_path`, not an unchecked `realpath`.
   Alias spellings resolving to the same checked path are not changed identity.
   Stable comparison excludes only `attached_at`; changed stable metadata needs
   `--reattach`. Identical pins still require a fresh read, then return unchanged,
   preserving the original timestamp even with `--reattach`.
3. Always reload under the workspace lock and CAS both the captured preflight
   token and participant UUID, even without user `--expect-revision`. Conflicts
   fail without automatic retry. Commit/fsync may outlast source freshness:
   success declares an association, not a continuing live-activity promise.
4. Detach performs local preflight and the same token/participant fence, with **no
   artifact I/O**. It works with missing, unsafe or stale sources, clears only
   metadata, never deletes native files/controls agents, and does not promote 1/2.
5. Status reads validated groups, then at most 16 selected members serially using
   each call's remaining overall budget. It reads only declared artifacts, never
   checks other workspace-state health, and rereads groups after source I/O.
   The full content token must match (counter-only updates are allowed).

Status takes no locks, writes/creates nothing, and performs no network,
subprocess, launch, readiness, fallback or annotation-publication operations.
Schemas 1/2 project unattached/unknown rows without migration.

## Edge Cases

- After **all** source I/O and the group recheck, recompute each observed row:
  `min(original_expires - ceil(elapsed_ms_since_row_call_start), floor(3000 - final_sample_age_ms))`.
  This deliberately may count latency multiple times. Future samples or nonpositive
  remaining lifetime clear source/freshness/remaining and yield unknown, including
  previously observed-unknown rows as well as busy rows. Final timeout discards all rows.
- Callers must deduct delivery/display time and honor source-sample clock limits.
  `attached_at`, mtime, copying files or cached reports never reset the source TTL.
  Same-host clock/local-filesystem assumptions and stateless rollback/replay limits
  remain; private same-UID permissions/stamp checks are not authentication.
- Sequential native reads are not an atomic snapshot. Declarations may change
  after the final recheck: status is revision-scoped, not linearizable.
  Shared source pins are allowed; rows do not prove distinct workers/executions.
- A changed stable binding through `group add --rebind` atomically clears a
  nonnull attachment and returns `live_attachment_cleared: true`. Role-only,
  timestamp-only, same-terminal pane-only changes and a no-op `--rebind` preserve
  it. Removal drops it; re-add creates a new participant UUID with null attachment.
- Source attachment and terminal binding are separate declarations, never inferred
  from pane, PID, folder, native-agent reference or native UI session. No verified
  association, native identity, job grant or cross-file transaction is implied.
- Persisted observation and worker readiness are unchanged. The static publisher
  never opens attachment artifacts or appends activity: labels remain group/role
  text with 6000 ms TTL. Declaration changes can stop it through normal whole-content
  fencing. Only explicit `publish --live` reads status in-process, verifies the frozen
  revision/group/participant/pin identities before durable whole-target reservation,
  and uses `valid_until_unix_ms` plus decreasing wall/elapsed source budgets. It
  refuses unsupported servers. Unknown/lost/expired evidence becomes role-only;
  a composed busy frame expires as a whole, with no lower-source fallback. See the
  [publisher contract](GROUPS_CONTRACT.md#opt-in-live-publication) for delivery,
  clock assumptions and uncertainty; this status command remains read-only.

## Example usage

Use an existing member and manually inspected actual pins, not these placeholders:

```sh
coven group attach-live WORKSPACE auth-review --member backend \
  --file /absolute/private/RUNTIME_UUID.json --runtime RUNTIME_UUID \
  --instance INSTANCE_UUID --session SESSION_ID --timeout 1 --json
coven group status WORKSPACE auth-review --member backend --timeout 1 --json
coven group detach-live WORKSPACE auth-review --member backend --json
```

## Validation

The full Coven offline suite passes **763 tests**, including **33 opt-in publisher
tests**, **24 attachment authority tests** and **34 workflow/status tests**. The
actual `bin/coven` wrapper is exercised against private fixture artifacts, not an
installed native runtime.
Coverage includes promotion/idempotency, mandatory CAS and remove/re-add races,
counter preservation, uncertain durability, source loss, later-read expiry, clock
rollback during recheck, whole-projection deadlines, and static publisher isolation.
Architecture/safety reviews found no blocking issues; diff whitespace checks pass.
No user group, source, or live application was modified during validation.

## Related files

- [runner/group_live.py](runner/group_live.py) — workflow, projection and expiry fences.
- [runner/groups.py](runner/groups.py) — strict authority, CAS and single-file mutations.
- [runner/coven.py](runner/coven.py) — CLI flags/output/exit mapping.
- [runner/djinn_live_observation.py](runner/djinn_live_observation.py) — private artifact reader.
- [tests/test_group_live_authority.py](tests/test_group_live_authority.py), [tests/test_group_live.py](tests/test_group_live.py) — offline mutation and projection boundaries.
- [GROUPS_CONTRACT.md](GROUPS_CONTRACT.md), [DJINN_LIVE_OBSERVATION_CONTRACT.md](DJINN_LIVE_OBSERVATION_CONTRACT.md), [APP_BOUNDARIES.md](APP_BOUNDARIES.md).
