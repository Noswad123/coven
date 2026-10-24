# Explicit participant groups and foreground labels

## Overview

Coven can group **already-running participants**, assign display roles, and explicitly
publish `GROUP NAME · ROLE` through Kitsune's generic annotations. Group and
participant UUIDs are Coven declarations, with human selectors for CLI use; they
are independent of task authority, the workspace agent roster, and native runtime
identity. Roles do not change Djinn policy, personas, prompts, or configuration.

Only the Kitsune presentation adapter is implemented in v1. The model is not
Djinn-specific, but this is not universal runtime support, native group status,
scheduling, complete agent-feature extraction, or automatic following of replacement agents.
No agent is launched, resumed, sent terminal input, or cancelled by these commands.

### Quick start: existing agents, separate orchestration workspace

Use an **existing Coven workspace**, or explicitly create a new, separate one:

```sh
# Optional, only for a NEW dedicated directory; never an app repo/current workspace.
coven init /absolute/path/to/new-group-workspace --blank
```

Do not initialize over an existing workspace or inside the Coven, Kitsune, or Djinn
application repositories. Below, replace `WORKSPACE` with the explicit workspace
path and `ABS` with the intended existing Kitsune **API** socket's absolute path.
Inside Kitsune, `--socket "$KITSUNE_SOCKET_PATH"` is usable only after confirming
that variable is nonempty and selects that API endpoint; it is never an implicit
default. No discovery, provider calls, installation, config edits, reloads, or
restarts are performed by the group workflow.

```sh
coven observe kitsune --socket ABS --json
coven group create WORKSPACE auth-review --name 'Authentication review'
coven group add WORKSPACE auth-review --member backend --role 'Investigator' \
  --socket ABS --pane w18:p85
coven group list WORKSPACE --json
```

By default, choose the **actual current pane ID from `observe`'s `agents` inventory**
(the example ID is not a lookup by name). Agents without a native reference can
join; a pane absent from that inventory cannot join through default binding.
Alternatively, explicitly opt into fresh Coven process evidence, independently of
the agent inventory:

```sh
coven observe kitsune --socket ABS --process-pane w18:p85 --json
coven group add WORKSPACE auth-review --member backend --role 'Investigator' \
  --socket ABS --pane w18:p85 --process-evidence
```

The observation is for inspection, not an import: `add --process-evidence` reads
fresh bracketed evidence itself and recognizes only reported `djinn`/`djinn-ui`
executable families. It explicitly opts the group authority into schema 2, which
older readers reject. Both add modes write local membership only; neither sends
an annotation or other UI write.
The older one-shot `observe` command still has no annotation-write capability.

For labels, the user must first manually enable a dedicated source in Kitsune's
configuration, preserving all other settings. This is an array, not a string:

```toml
[annotations]
sources = ["coven", "build"]
```

Order matters: put Coven **first** if `build` has already claimed the same terminal.
Kitsune selects the highest-priority source that has ever recorded that target in
the current epoch. Clear/expiry has **no lower-source fallback**: after Coven
claims a target it shadows `build` even after the publisher stops and its text
expires, until source-policy reset (an ordered source-list change creates a new
epoch). Plan this policy deliberately; Coven never edits/reloads/restarts Kitsune.
See the optional [Kitsune guide](../kitsune/ANNOTATIONS.md) for manual activation.

```sh
coven group publish WORKSPACE auth-review --socket ABS --source coven --duration 60 --json
```

This runs in the foreground; Ctrl-C stops refreshing. Ending a run does not clear
labels: accepted text expires after its 6000 ms TTL. Visibility is not guaranteed:
only spare top-border cells render text; single-pane layouts have no border,
disabled borders hide it, and source priority/title width can suppress it. A label
does not establish native lifecycle, job progress, completion, or success.

## API / Interfaces

All group commands require an explicit existing workspace. Bracketed flags below
are optional; replace `TOKEN` with a freshly inspected group revision.

```text
coven group create WORKSPACE auth-review --name 'Authentication review' [--expect-revision TOKEN] [--json]
coven group add WORKSPACE auth-review --member backend --role 'Investigator' --socket ABS --pane w18:p85 [--process-evidence] [--rebind] [--expect-revision TOKEN] [--json]
coven group remove WORKSPACE auth-review --member backend [--expect-revision TOKEN] [--json]
coven group list WORKSPACE [--json]
coven group publish WORKSPACE auth-review --socket ABS --source coven [--duration SECONDS] [--json]
```

`create --name` defaults to the selector when omitted. `--duration` defaults to 60
seconds and accepts integers 1..3600. Source IDs match `[A-Za-z0-9_.-]{1,64}` and
must already be enabled. Output is JSON in both modes; `--json` makes it compact.

- `create`: same key/name preserves the UUID and returns `unchanged`; a different
  name for the same key conflicts. Rename/group deletion are not provided.
- `add`: same member key and stable binding preserves the participant UUID.
  Role changes are supported. Changing stable evidence requires `--rebind`, which
  accepts the newly observed binding while preserving that participant UUID.
  Stable comparison excludes only `observed_at` and `pane_id_at_binding`: a fresh
  observation timestamp or same-terminal pane move alone is an unchanged retry,
  not a binding refresh. All other binding fields are stable, including endpoint,
  terminal, provenance, process interpreter/matches, agent hint and native reference.
  Process matches are canonicalized before comparison; response ordering alone
  is not a change. Switching evidence variants also requires `--rebind`.
- `add --process-evidence`: fresh `session.snapshot` -> `pane.process_info` ->
  `session.snapshot` reads must agree on the selected pane mapping. No saved
  observation import, inventory fallback, native identity or execution grant exists.
  Omit the flag to retain the default agent-inventory snapshot binding.
- `remove`: removes membership only, including an unchanged result when absent.
  It neither clears remote labels nor cancels agents; publication counters remain.
  Removing then re-adding creates a new participant UUID.
- `list`: reads without peer contact, locks, file creation, or durability
  confirmation. Returns `initialized`, `revision`, `groups`, `authorization: none`,
  `dispatchable: false`, and unestablished native lifecycle/job completion. It
  intentionally omits internal counters. Uninitialized reads do not create a lock.

Mutations return `outcome: committed|unchanged`, `revision`, `group`, `delivery`,
`authorization: none`, and `dispatchable: false`; add/remove also return `member`
(null for an absent removal). `delivery.outcome` is `durably_committed` or
`durability_uncertain`. Success exits 0; uncertain mutation durability exits 2;
ordinary errors exit 1 and argument-parser errors exit 2. There are no durable
exact-request receipts or exactly-once retry promises. Inspect before retrying;
optional CAS checks the fresh current token even for an otherwise unchanged retry.

### Exact authority schema and limits

`state/groups.json` is a separate, bounded authority, not a task-ledger projection.
Schema 1 permits snapshot bindings only; schema 2 permits mixed snapshot/process
bindings. All objects below have exactly the listed fields; unknown/duplicate
JSON keys are rejected. The existing workspace needs a regular `coven.json` and
nonsymlink `state/` directory, not a healthy task/agent/execution authority.

```text
root = {schema_version: 1|2, workspace_id: UUID, content_revision: u64,
        groups: [group], publication_counters: [counter]}
group = {group_id: UUID, key: selector, name: display, members: [member]}
member = {participant_id: UUID, key: selector, role: display, binding: binding}
binding = {presentation: presentation, evidence: evidence}
presentation = {adapter: "kitsune", endpoint: endpoint,
                terminal_id: string, pane_id_at_binding: string}
endpoint = {path: resolved_absolute_path, device: nonnegative_integer,
            inode: nonnegative_integer}
evidence = snapshot_evidence | process_evidence_variant
snapshot_evidence = {provenance: "kitsune.session.snapshot", observed_at: UTC_ISO_timestamp,
             agent_kind_hint: string|null, native_reference: native|null,
             native_binding: "unestablished"|"observed_reference"}
process_evidence_variant = {provenance: "kitsune.pane.process_info",
            observed_at: UTC_ISO_timestamp, agent_kind_hint: "djinn",
            native_reference: null, native_binding: "unestablished",
            process_evidence: {interpreter: "coven.kitsune.djinn-process.v1",
                               matches: [match]}}
match = {pid: positive_u32, executable_family: "djinn"|"djinn-ui",
         rule: "exact_posix_argv0_basename"}
native = {source: string, agent: string, kind: "id"|"path", value: string}
counter = {endpoint: endpoint, source: source_ID, epoch: nonempty_string,
           watermarks: {terminal_id: positive_u64}}
```

- New authorities start at schema 1. Only a successful explicit process add/rebind
  promotes 1 -> 2, atomically inside that group mutation. UUIDs, publication
  counters and the CAS algorithm are preserved; ordinary changed-content revision
  rules still apply. Schema 2 is a minimum-reader-version opt-in: older schema-1
  readers fail closed. Removal of process members or rebind to snapshots never
  downgrades it. Reads and snapshot-only operations do not initiate promotion.
- Process matches are nonempty, at most 1024 entries, with unique PIDs (integers
  1..2^32-1, never booleans), sorted by `(pid, executable_family, rule)`. Stored
  noncanonical order and variant-inappropriate fields are rejected.
- UUIDs are canonical; group/participant UUIDs are unique across the authority.
  The workspace UUID is allocated by first group creation, not borrowed from the
  task ledger. Selectors match `[a-z0-9][a-z0-9_-]{0,63}`; group keys are globally
  unique and member keys are unique within their group.
- Displays are nonblank printable strings, at most **64 UTF-8 bytes** each. The
  combined name + ` · ` + role must be at most 256 UTF-8 bytes.
- Maximum: **16 groups, 64 active participants total, 16 members per group**.
  An endpoint/device/inode + terminal association cannot occur twice, even across
  groups. Stored groups may span endpoints; one publish requires all members on
  the same selected endpoint. There is no automatic multi-server publisher.
- File limit: **1 MiB**; JSON nesting limit 16. Strings reject NUL and invalid
  UTF-8. Default string limit is 4096 UTF-8 bytes; endpoint paths/native path
  values allow 8192, timestamps 64, UUIDs 36. Endpoint paths must be normalized
  absolute paths without a leading `//`; pane IDs are printable without whitespace.
  Native source/agent/value and agent hints may be empty strings.
- `native_reference: null` requires `native_binding: unestablished`; otherwise
  it is `observed_reference`. Neither establishes authenticated, fresh native
  identity, policy, lifecycle, or a one-to-one terminal/native-session mapping.

### Revisions, counters, and leases

The absent-authority token is `groups-uninitialized:v1`. Initialized tokens are
`groups-v1:WORKSPACE_UUID:CONTENT_REVISION:SHA256`, hashing UTF-8 canonical JSON
of exactly `workspace_id`, `content_revision`, and `groups` (sorted keys, compact
separators, unescaped Unicode), prefixed with `coven.groups.content:v1\0`.
This is a whole-group-content token, **not an exact-file-byte hash**:
`publication_counters`, schema metadata, and JSON formatting are excluded.
Content revisions increment on changed declarations, not no-ops or reservations;
overflow is rejected. Counter updates alone do not invalidate CAS/publisher content.
The `groups-v1` prefix denotes the **token format**, not authority schema 1;
schema-2 authorities retain that prefix and hashing algorithm.

All declaration mutations and cycle reservations reload under the persistent
`state/.workspace.lock`; changes atomically replace the one authority with file and
directory fsync and post-publication verification. Unchanged mutations also confirm
the observed file's durability. Uncertainty is not rollback.
The workspace lock normally waits up to five seconds. Publication holds a separate
exclusive `state/.group-publisher.lock` lease for the whole run (immediate busy
failure); lock order is **publisher lease -> workspace lock**. All transport I/O
occurs outside the workspace lock. Never delete/replace lock files or steal by
PID; the kernel releases held locks on exit. This coordinates one workspace,
not all workspaces using the same server/source.

At most **8 endpoint/source counter entries**, each with at most **64 terminal
watermarks** in its current epoch, are retained durably. Member removal never
deletes them. No LRU eviction or same-epoch reset exists. Reservations increment
checked u64 watermarks for the exact whole-group target set, beginning at 1. A
fresh explicit publisher invocation may accept a new server epoch and reset that
entry's watermarks; the same epoch always continues its counters. An epoch change
during a run stops it.

Coordinate a dedicated source per workspace; do not manually reuse the publisher's
source or share it with another workspace. Sources are labels, **not authentication**.
There is no accepted-watermark query or automatic catch-up: external higher
revisions can cause rejection, not automatic revision bump/replay.

## Data Flow

1. `add` validates local selectors/group/CAS, then observes outside the mutation
   lock. Default binding reads one `session.snapshot` and selects an exact agent
   entry. `--process-evidence` instead performs three bracketed reads of the exact
   pane, with a shared 3-second deadline and aggregate 1 MiB read budget. Successful
    evidence is committed under the lock with a fresh check of any supplied CAS token.
2. `publish` freezes the whole-content revision and endpoint, takes the lease,
   and reads a fresh snapshot each cycle. It requires annotation support, the
   selected enabled source and the same endpoint. Snapshot members require unchanged
   agent/native hints. Process members use the current **panes** lookup, then each
   gets three fresh bracketed reads pinned to its bound terminal; stable evidence
   must match before any counter reservation or annotation write. Thus a cycle
   with process members uses one initial snapshot + three reads per process member,
   sharing one 3-second cycle deadline and aggregate 1 MiB read budget. Snapshot-only
   behavior is unchanged; generic annotation writes retain their existing budget.
3. It finds each bound terminal's **current pane**, builds fresh labels and pins
   writes to terminal ID + pane ID + annotation epoch + source. Same-terminal pane
   moves need no membership mutation. The membership does not bind an annotation
   epoch, so enabling annotations after adding members is valid.
4. Before **any annotation write**, one atomic reservation burns new counters for
   **all** targets, including targets later unsent. Unknown/uncertain reservation
   means **zero annotation writes**; numbers may nevertheless have been burned.
5. Serial `pane.annotate` writes require correlated `ok` acknowledgements. Cycle
   starts use a fixed 2-second interval and a shared 3-second cycle I/O budget,
   capped by the remaining run duration; at most 16 targets, no overlap or catch-up
   bursts. TTL is always 6000 ms. Filesystem fsync is not universally wall-clock
   bounded, so these deadlines are not a hard bound on total process lifetime.

## Edge Cases

- Any rejection, missing/invalid acknowledgement, uncertain delivery, changed
  identity/endpoint/epoch, disabled source, or group-content drift stops the run.
  No automatic retry, revision bump, payload replay, rebind, or counter reset.
  Partial remote effects are possible; earlier acknowledgements are not rolled back.
- For snapshot members, a changed agent-kind hint or native reference (including null -> reported),
  missing bound terminal, or replacement socket inode after handoff needs explicit
  inspection and, for changed bindings, `add --rebind`. No automatic adoption.
- For process members, missing/unrecognized/changed evidence stops the cycle before
  any reservation or write, with no snapshot fallback or automatic rebind. Valid
  native-reference/classification changes in Kitsune are ignored for this variant;
  snapshots still validate the legacy agent shape. Same-terminal pane moves are
  allowed once bracketed evidence is stable. PID reuse/ABA and intervening mapping
  changes remain possible: matching samples are not atomic process identity.
- Djinn-family recognition includes help/maintenance modes, not active-agent truth.
  Missing argv, launcher/renamed binaries, background or headless work may remain
  unrecognized; unknown does not prove absence. There is no automatic enrollment,
  migration fallback, replacement following, or new native action.
- Group edits after a cycle is admitted can still allow that cycle's old label to
  be sent. They stop publication on the next content check, not retroactively;
  stopping/removing membership does not cancel admitted writes. Text expires by TTL.
- Socket checks require a same-user private Unix socket (no group/other mode bits),
  resolve parent aliases, and fence device/inode. Filesystem checks are not peer
  authentication, a sandbox, race elimination, or rollback/tamper-proof authority.
  The shorter byte-encoded selected/resolved pathname is used for a single
  connection attempt so parent-symlink expansion need not overflow the Unix-socket
  address limit. Identity stays canonical; alias retargeting fails the fences.
- Read requests can trigger Kitsune's existing housekeeping (expiry, event drainage,
  title sync, possible default-workspace creation/spawn). Snapshot receipt and
  persisted native references do not attest freshness or zero server-side effects.
- No scheduler/daemon, installed-provider requirement, native action, event-ledger
  integration, old-preparation dispatch, or workspace migration follows from groups.

### Publisher summary and uncertainty

Exit codes: **0** normal duration completion, **1** failed, **2** uncertain,
**130** interrupted. Output includes `outcome`, safe `diagnostics`,
`authorization: annotation_publication_only`, `native_execution_authorization: none`,
`dispatchable: false`, `ttl_ms`, and `limitations`. Internal cycle reservation
states are `outcome_unknown`, `durability_uncertain`, or `durably_committed`;
target phases are `unsent`, `in_flight`, `acknowledged`, `rejected`, or `uncertain`.
These records derive the returned aggregate counts; they are not emitted as a
per-target durable receipt log.

| Summary field | Meaning |
| --- | --- |
| `reservation_attempts`, `reservation_attempted_targets` | All recorded reservation attempts and their target counts. |
| `cycles`, `reserved_targets` | Confirmed durable reservations only, not all attempted cycles. |
| `completed_cycles` | Confirmed cycles where every target was acknowledged. |
| `acknowledged`, `rejected`, `unsent`, `uncertain` | Partition of `reservation_attempted_targets`; in-flight interruption counts as uncertain, not falsely unsent. |
| `reservation_uncertain` | Targets with unconfirmed reservation durability: possible counter burns, not uncertain remote writes. These remain unsent. |

Interruption between transport return and recording its acknowledgement is
conservatively uncertain. Inspect output and current bindings/source policy before
starting a new explicit run; success acknowledges transport, not UI visibility or
agent/job status. No cleanup clear is sent on normal exit, failure, or Ctrl-C.

## Example usage

After inspecting the current revision from `group list --json`, a role update can
be conditional without changing the native agent:

```sh
coven group add WORKSPACE auth-review --member backend --role 'Reviewer' \
  --socket ABS --pane w18:p85 --expect-revision '<current group token>' --json
coven group remove WORKSPACE auth-review --member backend --json
```

For changed binding evidence, inspect the selected participant first and explicitly
add `--rebind`; never treat a stale revision or evidence change as an automatic retry.
For a process binding, retain `--process-evidence` on role updates/rebinds:

```sh
coven group add WORKSPACE auth-review --member backend --role 'Reviewer' \
  --socket ABS --pane w18:p85 --process-evidence --rebind \
  --expect-revision '<current group token>' --json
```

## Validation

The full offline suite passes **599 tests**, including **107 group tests**:
35 authority, 42 existing publisher, 12 existing CLI/workflow, and 18 new
process-group workflow/publication tests. Run with:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -q
```

Coverage for this slice includes strict schema variants/PID bounds/order,
atomic promotion preserving IDs/counters, failed promotion and concurrent CAS,
mixed groups, canonical comparison, explicit variant/process rebind, no downgrade,
empty agent inventory, classification independence, stable moves/mapping races,
shared deadline/read-budget exhaustion, uncertain promotion/confirmation, and zero
reservations/writes on failed evidence. Existing interruption, durable reservation,
lost-ack, and phase-accounting tests remain green. Architecture and safety reviews
found no blocking issues; diff whitespace checks pass. Ten additional socket-alias
transport regressions cover short/long pathname selection and unchanged safety
checks. [Live group validation](GROUPS_LIVE_VALIDATION.md) now confirms add/publish,
visible labels, process exit, quiet-terminal TTL expiry and explicit replacement
rebind against installed Linux binaries. It does not establish native lifecycle.

## Related files

- [runner/groups.py](runner/groups.py) — exact schema, CAS, declarations and reservations.
- [runner/group_publisher.py](runner/group_publisher.py) — snapshot/process binding, transport, phases and foreground loop.
- [runner/coven.py](runner/coven.py) — group/observe commands and parser.
- [runner/storage.py](runner/storage.py), [STORAGE_CONTRACT.md](STORAGE_CONTRACT.md) — local locks and single-file durability.
- [runner/kitsune_observation.py](runner/kitsune_observation.py), [KITSUNE_OBSERVATION_CONTRACT.md](KITSUNE_OBSERVATION_CONTRACT.md) — shared snapshot validation and read-only observation.
- [runner/kitsune_process_observation.py](runner/kitsune_process_observation.py), [KITSUNE_PROCESS_OBSERVATION_CONTRACT.md](KITSUNE_PROCESS_OBSERVATION_CONTRACT.md) — bracketed process evidence and interpreter limits.
- [tests/test_groups.py](tests/test_groups.py), [tests/test_group_publisher.py](tests/test_group_publisher.py), [tests/test_group_commands.py](tests/test_group_commands.py) — offline coverage, not live-server validation.
- [tests/test_process_groups.py](tests/test_process_groups.py) — private fake-endpoint process add/rebind and publisher boundaries.
- [tests/test_socket_alias_transport.py](tests/test_socket_alias_transport.py) — long canonical/selected Unix-socket paths and alias/transport safety regressions.
- [GROUPS_LIVE_VALIDATION.md](GROUPS_LIVE_VALIDATION.md) — installed Linux end-to-end evidence, transport fix, cleanup and limits.
- [APP_BOUNDARIES.md](APP_BOUNDARIES.md); optional sibling [Kitsune annotations](../kitsune/ANNOTATIONS.md) — presentation policy, not installation dependencies.
