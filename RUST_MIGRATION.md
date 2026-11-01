# Coven Rust migration

Status: **native evidence capture and read-only task/execution/action authority**, not a complete Rust replacement.
The retained Python implementation and its safety contracts remain the public
default and compatibility oracle. `bin/coven` is unchanged. No workspace/state
migration, executable replacement, install or authority activation happens when
the Rust crate is built.

## One command, one replacement

The Rust crate defines exactly one binary target, **`coven`**. Development and
parity tests invoke its explicit Cargo/build path, never the installed command
or an automatically selected implementation. There is no second native command
or compatibility alias to maintain.

Python remains temporarily as the current entrypoint and migration oracle while
coverage is incomplete. This is a staged replacement, not a dual-implementation
product: the final package will ship only Rust `coven`. Cutover must update the
existing entrypoint/install assets and remove Python runtime packaging after the
remaining parity gates pass; the Rust binary will not delegate unsupported work
to Python. Renaming the build target does not authorize an early install/cutover.

## Native commands available

```sh
cargo run --offline --locked --bin coven -- handoff validate FILE --json
cargo run --offline --locked --bin coven -- handoff collect WORKSPACE --file FILE --expect-sha256 HEX --expect-tasks-revision TOKEN --json
cargo run --offline --locked --bin coven -- handoff inspect WORKSPACE COLLECTION_ID --json
cargo run --offline --locked --bin coven -- handoff list WORKSPACE --task TASK_ID --json
cargo run --offline --locked --bin coven -- task list WORKSPACE --json
cargo run --offline --locked --bin coven -- state inspect WORKSPACE --json
cargo run --offline --locked --bin coven -- execution history WORKSPACE --json
cargo run --offline --locked --bin coven -- action inspect WORKSPACE ACTION_ID --json
cargo run --offline --locked --bin coven -- checkpoint inspect WORKSPACE CHECKPOINT_ID --json
```

The compiled `target/debug/coven` can also be invoked directly, including with
Python absent from PATH. Its production code has no Python, subprocess, runtime
or network dependency. Python is needed only for migration-oracle tests and the
existing default CLI. Unsupported commands, including task mutation/import/recovery, reject
at argument parsing before effects; there is **no automatic Python fallback**.

Handoff commands preserve [HANDOFF_CONTRACT.md](HANDOFF_CONTRACT.md): syntax-valid
handoffs remain worker claims, collected history remains unverified, authorization
none and dispatchable false. Historical reads report current task binding not
checked; collection checks only its explicit current local context. No source/check/
native reference is followed or verified. Reads create no lock, directory,
dashboard, repair, migration marker or runtime state.

## Structure and dependencies

```text
src/
  main.rs            Thin explicit CLI; escaped report/error output
  handoff.rs         Typed identifiers, revisions, claims, enums, closed schemas
  strict_json.rs     UTF-8, duplicate-key, finite-number and depth safeguards
  canonical.rs       Closed-domain v1 evidence encoding and SHA256
  archive.rs         Historical archive verification, bounded list/filter reports
  collection.rs      Pinned unverified capture with locked context revalidation
  read_fs.rs         Bounded read-only file/managed-path operations
  values/            Finite compatibility values, duplicate profiles and encoders
  records.rs         Extension-preserving validated workspace/task views
  tasks.rs           Preflight, captured task/state reports and authority fences
  ledger.rs          Complete bounded selected task-ledger replay/projection health
  ledger/confirmation.rs Explicit locked selected-history synchronization
  execution.rs       Historical execution replay state, v1 rules and report boundary
  execution/v2.rs    Upgrade bridge, graph reviews and v2 preparation replay
  execution/graphs.rs Pure bounded graph validation, refreezing and source matching
  execution/guards.rs SDK-only confirmed reservation checks under a borrowed guard
  actions.rs         Captured launch-control/bundle/checkpoint inspection
  actions/authority.rs Pure all-claim validation, reference lookup and work budget
  actions/plans.rs   Pure deterministic tmux/Herdr frozen-plan validation
  checkpoints.rs     Pure byte-oriented checkpoint views and framed scope hashes
  checkpoint_workspace.rs Ordered standalone workspace selection and checkpoint preflight
  storage.rs         Shared workspace guard and immutable evidence publication
  storage/replacement.rs Guarded atomic mutable-file replacement, no domain callers
  output.rs          Bounded safely escaped CLI presentation
  platform/          Compile-gated OS open flags and unsupported-platform refusal
```

Serde/serde_json provide typed JSON, clap the CLI, sha2 the existing digest
algorithm, and Unix libc the descriptor/lock operations. Ryu supplies shortest
ties-to-even float digits; the compatibility encoder separately applies Python's
formatting policy. TOML parses the existing workspace runtime configuration.
Tempfile creates exclusive private same-directory evidence temps. All are
conventional cached crates, not an external agent,
interpreter/daemon dependency or a new plugin framework.

Current filesystem support is POSIX Linux/macOS, matching existing Coven scope;
only Linux was executed here. Non-Unix filesystem commands fail closed. The pure
model/codec modules remain platform-independent. No Windows support is claimed.

## Byte identity and current limits

Raw input, typed values, canonical bytes and hashes are separate representations.
The native reader accepts existing Python-produced handoff archives, verifies
raw-input hash/count, captured binding, exact canonical envelope plus LF, content
ID/domain and final filename. It never recomputes historical task SHA against
today's task file or uses a current roster to manufacture verification.

The canonical encoder is **only for the accepted closed handoff/archive domain**:
sorted fixed schema keys, compact separators, literal UTF-8, bounded integer
version/byte-count fields. It is intentionally private to those typed models.
This handoff encoder is **not** the generic task-value codec. The separate
`values` model preserves bounded decimal integers without a binary64/Serde-number
round trip, distinguishes floats (including signed zero), and preserves insertion
order. Its legacy snapshot profile is last-wins without moving an existing key;
its authority profile rejects duplicates. Canonical encoding sorts keys/compact/
no LF; storage encoding preserves order with indent=2 and a terminal LF. The
general codec is used by bounded read-only authority validation, not authority writers. Never
silently adopt Serde/JCS encoding under existing v1 digest domains.

Frozen launch plans use an additional private sorted/spaced mode equivalent to
Python `json.dumps(sort_keys=True, ensure_ascii=False)`, with default `, ` / `: `
separators and no LF. This is separate from compact digest encoding, insertion-order
storage and report presentation. Existing canonical/storage/compact behavior is
unchanged; plans are compared and size-checked in their own encoding domain.

The first native preview deliberately refuses historical artifact reporting
under non-UTF-8 POSIX paths (`non_unicode_path_unsupported`). Python can represent
those paths through surrogateescape. A differential case captures this known
gap; Rust does not substitute a lossy name or label the archive corrupt. It must
be resolved, or explicitly restricted by a separately approved compatibility
decision, before default-entrypoint parity is claimed.

CLI JSON presentation need not match whitespace/key order, but structured values,
trust fields, raw hashes/counts and success/refusal behavior must match. Untrusted
text, bidi characters, DEL/C1 and non-BMP content are safely escaped for display;
canonical archive bytes use the separate UTF-8 encoder.

## Snapshot task preview and shared-lock foundation

`task list WORKSPACE --json` validates the known workspace managed paths, JSON
team/agent/lead records, checkpoint paths/text and TOML runtime fields. It retains
the complete task values, unknown nested extensions, exact IDs, arbitrary legacy
status strings, missing versus null owner, and uninterpreted timestamp strings.
It does not impose owner-roster membership or add defaults to task records.

The snapshot success envelope remains exactly `revision` and `tasks`. Revisions hash the
original bytes; missing and present-empty snapshots differ. No metadata is made
authoritative, and no read creates a lock, event, dashboard or recovery artifact.
CLI empty workspace means current directory and quoted `~/...` uses HOME; library
Path APIs remain literal. Named-user expansion and HOME-absent expansion explicitly
refuse instead of selecting a literal tilde directory or doing implicit user lookup.

Runtime-launch control/action artifacts remain unsupported before reading and at
final return. A task-authority marker instead selects the complete native reader
below. An absent marker plus ledger directory is unresolved import: native task
listing conservatively refuses it, while `state inspect --json` reports snapshot
`import_pending: true`, including `source_error` without invented tasks/revision
when source is malformed. These checks are not an atomic authority snapshot and
cannot be reused as mutation authorization.

## Read-only task-ledger authority

A present marker must validate: exact schemas/types/IDs, selected raw manifest
digest, source presence/mode/length/digest/revision, exact epoch entries, strict
source task JSON and contiguous canonical final frames. The reader checks complete
envelope bytes/LF, payload hash/length, baseline/source task equality, predecessor
chain, request uniqueness/bindings, add slug/collision/owner/title/status rules,
assign preservation of every other field/extension, and frozen before/after
revisions/result sequences. Hash-consistent but semantically invalid effects are
not accepted. A bad final frame never yields a last-good result or snapshot fallback.

Active task-list output retains the established ledger envelope: authority,
revision/tasks, workspace/epoch/sequence/frame digest and projection health. State
inspection has the same active envelope with narrower existing preflight: current
agent/lead/projection contents are not parsed as task state. Both skip projection
path/type validation in active preflight, classifying it through health instead.
Current roster cannot re-resolve historical owners; timestamps have no new meaning.

Projection health is raw-byte current/missing/diverged/unreadable with separate
snapshot hashes, never authority or a repair trigger. Sequence zero preserves the
original baseline bytes; absent source expects `[]\n`, not absence. After mutation,
expected projection uses the existing Python-compatible indented storage encoding.
Observed projections are stream-compared/hashed, including readable inputs above
the frame-size bound. Diagnostic text uses safe native codes, not OS/Python prose.

Marker bytes are rechecked after replay and before captured command report return.
Snapshot-to-marker, marker change/disappearance, runtime-authority publication and
pending-state transitions refuse. A frame appended outside the captured filename
set can leave a fully validated prefix: there is no latest-head, linearizable,
authentication or durability-confirmation claim. No lock, fsync or repair is done.

Native restrictions: 8 MiB individual authority inputs/frames, 4096 final frames,
256 MiB aggregate final-frame bytes, 16384 entries per enumerated directory
(including inert temps), existing value/reconstruction/response limits, and 256 MiB
streamed projection observation. Over-budget projections are diagnostic unreadable
with an unsupported-profile code rather than unbounded hashing. Exact recognized
regular temp names are inert/type-checked across epoch/frame directories; they are
never promoted or deleted.

Python supports some raw source encodings or numeric spellings outside the finite
UTF-8 native profile. Native retains raw bytes for digests/projection but still
requires supported parsing and source/frame comparison; unsupported input is not
rewritten, accepted via hashes alone or replaced by a projection. The read view now
retains validated historical receipts for the pure library lookup below. Mutation
retry coordination and delivery must still be ported before writers, not inferred
from lookup, inspection or the explicit synchronization API below.

### Frozen task-ledger receipts (library only)

`ledger::RequestKey` and `RequestBinding` have validated, private representations;
bindings can be parsed from compatibility values or built with explicit add/assign
arguments. `View::lookup` returns an immutable `FrozenResult`, a missing captured
key, or `request_key_binding_conflict`. Only complete successful replay constructs
frozen results. There is no new lookup CLI, mutation, acknowledgement or recovery
command and no public unchecked result constructor.

Binding equality compares canonical bytes, ignoring only object insertion order.
Raw selector case/whitespace, notes/title spelling, operation and null versus
concrete expected-revision tokens remain part of identity. Historical alias
equivalence in an effect does not make different request bindings interchangeable.
Lookup does not compare an old expected revision to the current head: replay still
validates its CAS at the frame's original position. Later assignments cannot rewrite
an earlier result's complete task, owner, after_revision or historical sequence.
Large integers, floating signed zero and unknown extensions retain `values` identity.
Borrowed results are immutable and owned clones cannot mutate retained receipts.

Receipt retention replaces the request-ID set as the sole uniqueness authority.
Each receipt is inserted only after all frame/effect/result checks; every selected
frame, including a corrupt tail after a matching key, validates before a view is
returned. Existing marker checks remain, but do not authenticate append-only history
or detect every removed suffix, pin the latest head, or make an unlocked prefix
linearizable. Captured lookup itself takes no filesystem/root/guard/current-state
arguments. Explicit fencing is separate; known historical results remain known if
the marker later changes or becomes unreadable.

Missing authority/pending import and a missing key inside a valid active view are
different conditions. `None` from lookup means only absence from that captured
history—not current receipt absence, permission to execute, fresh CAS success or
durability acknowledgement. A private test-only route proves hit/conflict handling
precedes a fresh-request continuation, not that a writer or durable retry pipeline
has been implemented. Future task-command preflight remains a separate contract.

A separate 256 MiB retained-canonical-data limit counts UTF-8 key bytes plus
canonical binding and frozen-result bytes with checked accumulation. Raw history
remains independently capped at 256 MiB and 4096 frames. This is not a heap/peak
memory bound; retaining receipts adds per-frame result storage and encoding work,
without cloning the whole accumulated map on each frame.

Known-result policies must not be silently unified during later writer work:
Python ledger confirmation can fail hard during a current-authority reread outside
its sync-uncertainty wrapper, whereas execution preserves known results after
verification failure. Pure lookup adds no delivery fields and changes neither
policy. Historical `result.outcome: committed` is not a new fsync confirmation.

General JSON preview bounds: 8 MiB input, depth 64, 262144 parsed value nodes,
16384 object members, 65536 array items, 1 MiB decoded strings, 8192-byte number
tokens, 4300 integer digits excluding sign, 32 MiB encoded/final ASCII output.
Checkpoint/TOML reads are bounded to 1 MiB and checkpoint scans to 4096 entries.
These are explicit native restrictions: legacy Python can read some nonfinite,
surrogate-containing, deeper/larger extensions that this finite scalar profile
refuses. They are not silently rewritten or misrepresented as current authority.
Parser node budget is not a promise about arbitrary programmatically built values;
encoding separately enforces depth/container/string/output limits.

`storage::WorkspaceGuard` underpins explicit native handoff capture.
Explicit `acquire` may create a persistent mode-0600 lock; `existing` cannot create
a missing lock/directory. The token is root/PID/thread owned and not Send/Sync,
uses the same kernel flock/inode as Python, never steals/unlinks/replaces it, and
releases on normal exit or process death. The destructor avoids explicitly
unlocking a parent's inherited open-file description after fork. Task and handoff
reads do **not** acquire this token. No native task writer, authority publication,
import, recovery or automatic dispatch is enabled by its availability.

### Guarded mutable replacement (library only)

`storage::atomic_replace_bytes(guard, relative, bytes)` is a low-level foundation,
with no production CLI/domain-writer callers. The matching root/PID/thread guard
proves coordination only—not CAS, approval, receipt absence, execution permission
or whole-ledger confirmation. Parent directories must already exist and managed
paths must remain safe. Both `.workspace.lock` and `.runtime-transport.lock`
basenames are rejected in every directory, preserving the persistent flock inode.

An exclusive same-directory `.<target>.<random>.tmp` receives all bytes, then
descriptor mode application, descriptor mode verification and file fsync. Existing
07777 mode bits are preserved; absent targets receive 0600. Mode is applied after
writing because writes may clear special bits. A successful chmod can also silently
clear SGID; descriptor verification rejects any mismatch before publication, leaving
the old target intact. Ownership, ACLs and xattrs are not part of preservation.

After path/parent recheck, atomic rename is the visibility boundary, followed by
parent-directory sync. Pre-rename errors leave old bytes/mode/inode intact (or the
target absent). Successful rename plus failed parent sync returns known-new
`Delivery::DurabilityUncertain`; there is no post-rename reread, rollback or claim
that nothing happened. Cleanup touches only the owned temp and never masks the
primary outcome. Inert orphans are neither adopted nor broadly removed.

The independent 32 MiB byte-transport cap does not raise domain limits or native
reader profiles. Existing immutable evidence publication retains its 8 MiB cap and
hard-link no-overwrite semantics. Ledger frames/manifests/baselines, the initial
task-authority marker and action bundles must never use mutable replacement.
No such call sites are introduced. Domain-safe immutable publication, activation-
last ancestor syncing and recovery remain future work. Explicit selected-history
confirmation below does not supply a publication or recovery protocol.
Local ordinary rename-error semantics and static cooperating paths
are assumed; this is not a hostile-path sandbox, distributed unknown-rename
protocol or universal power-loss guarantee.

### Locked selected-ledger confirmation (SDK only)

`ledger::confirm_selected(workspace, expected_view, borrowed_guard)` validates the
matching root/PID/thread guard before authority I/O, then fully rereads selected
history and compares **both** revision and head-frame digest. A marker-only fence,
earlier receipt or matching task-state hash is insufficient. It rereads the selected
manifest before synchronization, preserving Python's pre-sync hard-failure boundary.
Native additionally binds the raw manifest digest to the fresh validated marker
metadata and requires Boolean source_present; this is deliberately stricter than
Python confirmation-time truthiness under concurrent tampering.

Files synchronize in order: task marker, manifest, baseline source if present,
then every frame from zero through the current sequence. Directories follow:
frames, epoch, task-ledger, state, workspace root. Nothing is published or repaired,
and projection/configuration/events/other epochs/execution authority are not read
or synchronized. There is no post-sync replay or new latest-head guarantee.

The must-use `LedgerConfirmation` distinguishes Confirmed from
SynchronizationUncertain. Failed replay/head verification or manifest reread remains
a hard error. During synchronization, operational path-metadata, open, descriptor-
metadata, file-fsync or directory-sync errors stop immediately as uncertainty;
unsafe paths, detected nonregular descriptors and other policy errors remain hard.
Historical FrozenResult values are not rewritten or wrapped in a new delivery
envelope. Confirmed assesses selected-file synchronization, not publication,
execution permission, or a completed durable task-command retry.

### Confirmed execution reservation checks (SDK only)

`execution::guard_task_mutation(workspace, task_id, borrowed_guard)` and
`guard_authority_change(workspace, borrowed_guard)` consume complete typed replay
state, never report fields or frozen preparation receipts. Internal typed extraction
precedes report projection without changing report values, replay order, Work
charges or V2 prefix-capacity validation. Historical `history()` still acquires no
lock and performs no fsync; the SDK checks are explicit, separate operations.

Task-ID validation occurs first, then matching-guard validation before authority
I/O. A missing execution directory/control returns DisabledObserved without fsync;
pending, null, malformed or unsafe authority never becomes disabled. Active
authority requires a second complete replay matching authority ID and revision,
then fsync of control.json and directories execution, state and root. Only afterward
does derived live attempt state determine a task-specific or authority-wide block.
Confirmation is mandatory even for unrelated tasks, no attempts, all-cancelled
history, approval-only graphs, or an already known prepared blocker.

Only DisabledObserved or ActiveConfirmedClear is successful. A reservation returns
`prepared_attempt_blocks_mutation`; operational synchronization failure returns
`execution_synchronization_uncertain`; current-authority verification failures and
unsafe paths remain errors. Thus an uncertain cancellation cannot clear a guard.
Graph revocation does not cancel prepared attempts; approval alone is not a task
reservation. No current source/task/roster/projection or upgrade backup is read.

The must-use result describes a locked observation, not a transferable permission
token, fresh CAS, receipt-absence proof, approval or task/execution authorization.
Future domain code must retain the same guard and perform its own receipt/head/CAS
and publication checks. DisabledObserved is administrative absence observation,
not durable absence or anti-rollback proof. Two active-authority reads each retain
the existing per-replay bounds; no combined CPU/peak-memory ceiling is implied.

Neither API acquires, replaces or releases the borrowed lock. Obtaining a native
WorkspaceGuard currently requires a regular coven.json marker, unlike Python's
low-level stores; it does not parse configuration. This inherited acquisition
restriction remains explicit, while marker-free historical readers stay unchanged.
No CLI, domain writer, activation, recovery, provider/runtime request or receipt-
delivery policy is enabled by either foundation. Static cooperating filesystem
assumptions remain; no hostile namespace sandbox or universal crash durability is
claimed.

## Explicit unverified handoff collection

`handoff collect` is the first native write command: it captures **local evidence
only**, not accepted execution/results. It requires explicit raw SHA256 and task
revision pins, validates the selected handoff once, and retains its bytes even if
the worker changes the file later. It never follows references or runs checks,
Git, native apps or tools. Fixed reports remain `collected_unverified`,
authorization none, dispatchable false and `local_artifact_capture_only`.

The collector has a dedicated context path, not task-command preflight. It checks
strict depth-16/duplicate-free JSON for roster (64 KiB) and actual legacy snapshot
(1 MiB), or selects/replays active task authority without reading its projection.
Pending/corrupt task authority refuses. Expected/current/claimed revision and exact
task/owner/current roster membership must agree before effects and again under
the shared exclusive flock. Full-task hash includes all extensions using the
existing `coven.handoff-task.v1` domain and compatibility codec. Separate corrupt
runtime/execution controls neither authorize nor block evidence capture. Prepared
attempts remain reserved, and all task/ledger/control/log/dashboard bytes stay put.

Native context uses the bounded finite value profile as well as byte/depth caps:
some Python-valid contexts, such as a 65,537-item extension array, are explicitly
unsupported. This is a tested preview restriction, not a corrupt-authority or
snapshot-fallback classification. Existing historical reads remain independent.

Directory preparation, publication and confirmation all require a matching owned
`WorkspaceGuard`. New output dirs request mode0700; each dir and its parent are
synced, including existing parents on retry. A mode0600 temp is created in the
destination directory, written/synced, then exclusively hard-linked to the final
content ID. No overwrite/rename fallback. A pre-link failure leaves no final; a
post-link directory-sync failure retains final bytes and returns
`durability_uncertain`/exit2. Cleanup only attempts this operation's own temp;
orphan temps stay inert. Entry/byte accounting reserves both links before publish.

Exact existing bytes are observed under the guard before directory preparation.
A private-field `ObservedEvidence` token preserves that fact for confirmation.
Later operational read/open/metadata/file-sync/dir-sync failures return existing
uncertainty, not ordinary rejection; mismatched bytes and unsafe paths still fail
without replacement. Identical Python/Rust retries preserve ID, bytes, inode and
mtime. No timestamp/index/secondary log or authority transaction is added. Context
must still be current on recollection; this is not frozen task-ledger command
receipt replay or permission to repeat an uncertain native task.

## Historical execution-control v1/v2 reader

`execution history WORKSPACE --json` is a new native preview report, not a port of
Python's current-source execution inspection or mutation delivery envelope. It
requires only an existing directory, not current `coven.json`, task, checkpoint or
runtime files. It loads only managed `state/execution/control.json`. Missing
directory/control means disabled; a directory without control is pending and
refuses. Present null, malformed authority, unsafe types, unreadable data and
unsupported schemas never become disabled or an empty reservation report.

The reader fully replays v1 enable, enrollment/source rebind, preparation and
cancellation. It checks exact schemas and literal integers, generation/history,
unique request keys, every expected prefix revision with final extensions, retained
source identities/rosters/task records, ledger task-content digests, complete
enrollment/preparation snapshots, domain-separated task/intent hashes, predecessor
and numbering rules, unique identities and the sole permissible frozen results.
Snapshot revisions retain their raw-byte meaning: parsed historical tasks cannot
reconstruct an unavailable original snapshot's raw hash. Final canonical length
plus LF and 512 KiB per prepared attempt must fit Python's 16 MiB authority budget.

Reports retain enrollments, attempts, receipts and extensions. Cancellation changes
derived attempt state but never rewrites the earlier preparation receipt's
`state: prepared`. Sorted `reserved_task_ids` lists only still-prepared tasks;
enrollment, staleness or unrelated current-file changes cannot grant or release a
reservation. Status active means valid v1/v2 historical authority, not active execution.
Both active/disabled reports say authorization none, dispatchable false,
assessment historical_authority_only and current_source/durability not_assessed.

Reads do not create/acquire a lock, fsync, publish, repair, archive or load current
sources. The observed revision is not a latest/linearizable, authenticated or
durability-confirmed view. Historical reservation observation is **not** a reusable
mutation guard: writers still need receipt-before-mutable-state ordering, matching
lock ownership and durable authority confirmation. No enable/enroll/prepare/cancel,
upgrade, graph approval, task mutation or runtime admission command is enabled.
Task and handoff readers/collectors retain their existing independence from this
execution history path. Unsupported or malformed schemas refuse rather than
returning partial legacy state or an empty reservation report.

### Complete v2 historical graph/review replay

V2 validates its complete embedded v1 authority before any graph-era history:
no active legacy preparations, unchanged identities/extensions, exact upgrade
metadata and first/unique upgrade request. Raw source SHA is shape-validated, not
reconstructed from canonical values; the exact backup path and mode are retained
data only. Missing, corrupt, unsafe or FIFO backups are never opened. Original
legacy bindings, record schemas, receipts, cancellations and attempt order carry
forward directly as validated state, not by reparsing a legacy report.

All v2 requests bind the exact prior revision, starting at the validated legacy
revision. Prefixes include the embedded authority, upgrade and final extensions.
Every v2 prefix, not only the final state, must retain 512 KiB per prepared attempt
and 512 KiB for an approved current graph head within Python's 16 MiB limit.
Embedded v1 keeps its original final-only reserve policy.

Pure graph helpers enforce closed schemas, 1..64 unique nodes, at most 256 edges,
no duplicate/dangling/self/cyclic dependencies, inclusive 1 MiB canonical document
and manifest limits, and exact task/node/manifest digest domains. Persisted node
and dependency order stays intact; lexical topology is validation, not a rewrite.
Proposal replay re-freezes the document against the retained **full** source and
replayed enrollment bindings, including current retained task extensions rather
than an enrollment's older snapshot. A self-consistent manifest alone is not enough.

Historical approval and graph preparation match all selected nodes against their
entry's retained source. Unrelated full-source revision drift may still be current;
selected snapshot, authority, enrollment or roster drift cannot. Graph family/head,
successor, globally unique decision IDs and latest decision binding are checked.
Managed task IDs remain sticky across removed/superseded nodes, so standalone
preparation cannot bypass graph rules. Dependencies do not gate preparation.
Revocation never cancels preparations; cancellation never revokes review.
V2 intent hashes include graph_binding, including explicit null for standalone
unmanaged preparations; legacy v1 records are not given new fields or digests.

Schema2 reports add graph_id, current_graph_revision, graphs and sorted
graph_managed_tasks to the existing historical envelope. Graph review states and
source_match inside frozen receipts are historical facts only: no fresh approval,
preparation eligibility, current-source match, durable writer guard or execution
admission is inferred. No graph mutation or upgrade command is enabled.

Native profile: 8 MiB raw input, the shared finite value/output bounds, at most
1024 total entries across legacy plus v2, and conservative prefix preflight
`(whole_control_canonical_length + 32) * total_entries <= 256 MiB` before either
version clones/encodes history prefixes. Standalone v1 keeps its existing profile.
All reducer canonical comparisons and hash encodings, including graph helpers and
embedded legacy replay, charge one shared 256 MiB aggregate accepted-work budget.
A refusal may perform one further encoding capped at 32 MiB before charging;
this is neither a
strict CPU/memory ceiling nor a linear replay algorithm. Final compact and escaped
ASCII reports each have their own 32 MiB bound and finish before stdout is written.

Timestamp acceptance is deliberately narrower than Python's `fromisoformat`:
ASCII `YYYY-MM-DD[T or space]HH:MM:SS[.1..6 digits](Z|+/-HH:MM)`, years 0001..9999,
Gregorian calendar/leap-day checks, hours/offset hours 00..23, minutes/seconds/offset
minutes 00..59. Spelling including `-00:00` is preserved; no chronology is invented.
Other Python-valid ISO forms return unsupported_execution_timestamp_profile.
These limits are explicit preview restrictions, not a claim that refused Python
authorities are corrupt, nor a reason for fallback or persisted-value rewriting.

## Runtime-launch action inspection and checkpoint decisions

`action inspect [WORKSPACE] ACTION_ID --json` requires only an existing root; one
operand means an action ID in the current directory, while two supply an explicit
workspace (including quoted empty workspace and tilde expansion). No coven.json
marker or current task/config/runtime preflight is required. The command validates
the complete launch control before selecting a claim, then reads only that action's
bundle and dedicated `runtime-launch-<id>` checkpoint. It neither scans other
bundles nor follows frozen inventory paths, environment values or shell strings.

Control validates all claims, including unselected ones: exact schemas and literal
integers/booleans, UUID/reference tokens, head membership, unique claim IDs, status/
acknowledgement/report rules, and every frozen plan. Actual resolved root/device/
inode must match persisted identity. Platform code alone acquires OS identity;
persisted arbitrary decimal integers are compared without truncation. Root aliases
resolve normally; copies/moves never silently rebind launch authority. Non-UTF-8
actual roots explicitly refuse. A missing control disables only without an actions
directory; present null, orphan bundles, unsafe paths and corrupt controls refuse.

Plans are rebuilt exactly for tmux or Herdr, preserving ordered agents, literal
commands/starters/environment, typed handles/output selectors, argv, steps and
integer sleep values. No executor, executable lookup, sleep or environment capture
exists in this module. Plan validation is structure, not permission. Terminal
reports follow Python's intentionally permissive schema: step counts/indices and
handle inventories are not inferred from plan length or backend. Acknowledged
terminal claims and late results on an older claim remain valid.

Bundles validate their own identity and plan while keeping `inputs` an opaque
finite object. They need not match today's activation/control revision for
inspection. Manifest/control token labels are not hash preimage domains: each
hashes sorted compact UTF-8 values without LF. An internal classifier compares
only validated consumed manifest/checkpoint/decision references, with no optional
evidence/current-source callbacks. It is not a durable retry or submit API.

Successful output preserves Python's inspection envelope and `outcome: inspected`;
exit is zero even when a retained claim is claimed, outcome_unknown or failed.
Full control and selected claim remain independently visible despite ordinary
missing/corrupt/unsafe bundle or checkpoint artifacts. Existing diagnostic string
slots use stable native codes rather than Python OS/exception prose:
`action_bundle_missing`, `action_bundle_unsafe_path`, `action_bundle_unavailable`,
`action_bundle_invalid`, and checkpoint unsafe_path/unavailable/invalid equivalents.
Missing checkpoint is a successful missing view, not a diagnostic. These fields
are not a fresh approval, readiness, coherent latest snapshot, authentication or
durability claim. No new grant fields or durable mutation envelopes are invented.

The pure checkpoint API ports Python's exact byte scanner, not a general Markdown
parser. It preserves LF/CRLF/bare-CR offsets, fences, comments, list nesting,
reserved three-line blocks, direct checkboxes and history separator/trailer rules.
Only specified presentation/machinery spans are excluded, and remaining original
bytes are concatenated and length-framed once for the scope hash. Snapshot hashes
cover every original byte; missing and empty differ. Contradiction takes precedence
over stale scope; stale changes-requested remains rejected. Decided-at is scalar
text, not an ISO timestamp requirement. Linked external content is never read.
The standalone command below adds Python's broader workspace preflight separately;
action inspection continues using only the narrow pure checkpoint reader.

Native profile: raw action authority at most 8 MiB, UTF-8 without BOM, existing
finite value limits, compact canonical authority at most 16 MiB, sorted/spaced plan
at most 8 MiB, selected checkpoint at most 1 MiB. Python accepts some BOM/UTF-16/32
authority encodings; native returns `unsupported_action_encoding_profile`, even for
optional bundles, rather than labeling this profile gap ordinary corruption.
One command-scoped 256 MiB accepted serialization budget covers control/plans/
reports/bundle/final compact and escaped output. A refusal may encode one additional
bounded value before charging; this is not a CPU/peak-memory ceiling. Compact and
escaped reports each have an independent 32 MiB bound, both complete before stdout.
Library inspection charges compact output only; CLI also charges ASCII presentation.

Resource/profile errors anywhere are fatal with no success stdout—even inside
malformed checkpoint machinery or an optional bundle. Ordinary malformed optional
content remains diagnostic. No lock acquisition/inspection, fsync, transport,
publication, recovery, current input/task/projection validation or runtime/provider
call is performed. Existing task/runtime fences remain unchanged.

## Standalone workspace-aware checkpoint inspection

`checkpoint inspect [WORKSPACE] [CHECKPOINT_ID] --json` reuses the pure byte reader
only after ordered workspace validation. No operands means cwd and plan-approval.
The first operand selects a workspace **only** when its expanded spelling has an
existing or symlinked `coven.json`; otherwise all original operands are checkpoint
selectors in cwd. This is marker-based, not a two-operand arity heuristic or ancestor
search. Marker directory/symlink types can select a workspace and fail managed-path
validation later. Root aliases resolve, while marker contents are not selection data.

Preflight follows the existing Python order: known managed directories and task
marker path; selected ledger replay; managed files; conventional checkpoint text
and sorted actual-name scan; persisted team and TOML overrides/effective team;
environment multiplexer and complete launch-control validation; repeated paths,
ledger and checkpoint text checks plus roster paths; third independent ledger
read or legacy tasks; agent/lead records. Only then are excess operands or an
invalid selected ID rejected. Missing selected checkpoints succeed after preflight.
Unrelated recognized checkpoint files require valid UTF-8 text, not valid decision
grammar. Known log/prose/prompt paths are type-checked without parsing their content.

Pending ledger evidence without a task marker is allowed with a valid snapshot,
unlike the task-list preview's conservative pending fence. Active authority is fully
validated without projection path/type/content or health reads. A corrupt selected
ledger never falls back. All three ledger reads remain distinct observations;
there is no added cross-pass/final equality or pending/runtime fence. Each reused
`ledger::read` retains its own existing internal marker checks. This is not an
atomic, latest or coherent workspace snapshot or a reusable mutation guard.

The narrow launch-control-only seam validates every claim/plan/report and actual
workspace location, including orphan-control rules, but never opens bundles or
dedicated checkpoints or constructs action output. Valid unresolved claims do not
block checkpoint inspection. Existing action inspection remains configuration-
independent; existing task runtime/pending fences and pure checkpoint semantics
are unchanged. Execution authority is not added as a dependency.

TOML runtime overrides apply trimmed strings to an effective team and revalidate it;
environment selection cannot rescue invalid persisted or effective configuration.
Nested TOML dates/times are checked against Python calendar/year/clock/timezone
constraints (including rejection of leap second 60 and year zero). The cached
parser's signed-i64 integer bounds and rejected positive floating overflow are
explicit `unsupported_checkpoint_toml_scalar_profile` restrictions, not a claim that
Python rejects those ignored scalars. Ignored TOML NaN/inf literals and parser-
accepted negative overflow are not subjected to the finite JSON policy. Numeric
refusal classification depends on the pinned parser diagnostics; some malformed
overflow-prefixed integer tokens can receive the same unsupported-profile code.
Both implementations still refuse those tokens. No alternate parser/dependency or
value rewriting is introduced, and full TOML grammar equivalence is not claimed.

Preview syntax requires `--json`; Python's flag-free pretty mode is not yet ported.
Unambiguous long-option prefixes (`--j`, `--js`, `--jso`) are supported for this
command. The flag belongs before or after the operand group, not between operands
or before a trailing terminator following operands, matching Python parser errors
before workspace loading. Repeated `--json` is rejected natively although Python
accepts it. Tilde expansion precedes the marker probe but never rewrites a retained
checkpoint selector; named-user expansion and HOME-absent implicit lookup remain
explicitly unsupported. Non-UTF-8 paths and directory names refuse without lossy
conversion, including unrecognized names that Python could ignore.

Limits are per read/pass: 8 MiB finite JSON, 1 MiB checkpoint/TOML text, 4096 total
directory entries per checkpoint scan, existing ledger replay limits and action
control encoding/work limits. Conventional checkpoint files are checked before
the bounded sorted scan. Compact and escaped presentation each finish within
32 MiB before stdout. Repeated passes do not imply an aggregate CPU/peak-memory
ceiling. No locks, fsync, publication, repair, approval, task/control mutation,
transport, runtime/provider call or Python fallback is added.

## Validation

Use the existing installed Rust toolchain; this slice was exercised with Rust/
Cargo **1.96.1**. The manifest declares Rust 1.85 compatibility but that compiler
version has not been independently run. Cargo.lock is generated offline.

```sh
just test-rust     # Native unit/integration tests, including Python oracle
just test-python   # All retained Python regressions
just check         # rustfmt check, strict clippy, both test suites
```

The native suite has 150 tests, including a build/CLI-name regression test and an
integration harness with **109 handoff parity
cases** and one explicit deferred non-UTF-8 reporting case. Characterizations cover
otherwise-valid duplicate keys and escaped-equivalent keys, required null versus
missing, literal integer versions, Unicode/byte boundaries, source-state claims,
frozen canonical bytes, Python-produced literal-Unicode archives, unknown fields,
hash/binding/filename/encoding/LF corruption, historical reads despite corrupt
current tasks, full-cohort filtering, 256/257 entries, exact/over byte limits,
inert temps, symlink/FIFO refusal and effect-free unsupported commands.

The independent CPython value corpus adds **17,464 byte/bit cases**: every finite
exponent region with selected significands, seeded random binary64 patterns,
fixed/scientific boundaries, extremes/subnormals, exact decimal midpoints and
perturbations, negative zero, bounded large integers, Unicode, duplicate order,
canonical bytes and indented storage bytes. Python's integer digit limit is pinned
to 4300 in the oracle. This is strong characterization, not exhaustive binary64 or
arbitrary-token proof. A frozen decimal-tie regression catches the real difference
found with Rust's standard Debug float digits versus CPython's even candidate.

The snapshot CLI oracle adds **43 cases**, including broader preflight, exact-byte
revision, extensions, missing/invalid records, explicit nonfinite/scalar refusals,
tilde/empty argument behavior, ledger/runtime refusal before snapshot access and
no filesystem mutation. Mixed Python/Rust lock tests establish both directions of
exclusion, aliases, unchanged inode and kernel release after killing the owner.
Fork/exec lifetime corner cases, macOS execution and network-filesystem behavior
have not been independently established by this test set.

The task-ledger oracle adds **94 authority/corruption/effect cases** against real
Python-produced imports/histories. It includes noncanonical and absent baselines,
CAS-bound historical requests, Unicode/float/big-int extensions, roster drift,
projection modes (including FIFO/symlink and a 9 MiB readable projection), wrong
field/type/identity/mode/digest/chain/sequence/directory/envelope data, duplicate
requests, semantic effects with recomputed hashes/revisions, strict backup keys,
unsupported raw encoding, and invalid final frames. Native unit tests exercise
captured-command transitions, exact temp recognition, frame/history/directory
boundary checks and overflow. Reads leave bytes/modes/namespace/lock inode unchanged.

Collection adds **16 mixed Python/native producer cases**: native-first and
Python-first exact archive bytes/IDs, existing inode/mtime preservation, concurrent
collectors, active ledger with bad/FIFO projection, unrelated FIFO controls,
real prepared reservation preservation, stale/hash/owner/roster/duplicate/pending/
corrupt/path/capacity refusals and an explicit out-of-native-profile context.
Internal fault fixtures cover mkdir/temp/write/partial-write/file-sync/link/
pre/post-link directory-sync/cleanup, replay read/open/metadata faults, authority
changes before locked capture, wrong guard, integrity/unsafe-path refusal and
exact/over byte reservations. CLI uncertainty maps to exit2. Fault injection is
internal to tests, never activated by production environment variables.

The original execution corpus has **120 oracle cases**: 36 accepted reports,
70 corruption/path/capacity refusals and 14 Python-valid native-profile refusals
(the previously refused valid v2 history is now accepted). Real Python histories cover
snapshot/ledger source rebind, cancellation/reprepare, multiple reservations and
frozen preparation receipts after cancellation. Re-signed downstream prefixes
isolate semantic corruption from hash failure. Tests retain numeric/Unicode values,
move identities, ignore malformed/missing/FIFO current files, reject unsafe managed
paths, retain v2 reservations, and establish success without Python on PATH.
They snapshot namespace/bytes/modes/inodes/mtimes on success and refusal. Native
unit tests cover exact work/history/capacity limits and timestamp calendar bounds;
a valid small control with DEL text exercises final ASCII expansion refusal with
empty stdout and no writes. Source inspection, rather than syscall tracing, also
establishes the absence of network/runtime/fsync operations in this isolated path.

The v2 corpus adds **175 cases**: 56 accepted reports, 117 semantic/schema/path
refusals and 2 Python-valid native-profile refusals. It covers actual backed-up
Python upgrades, mixed-era receipts/attempts, standalone/null graph bindings,
snapshot/ledger proposals, repeated reviews, downstream-first graph preparation,
revocation/cancellation independence, successors and sticky membership, unrelated
source drift, full-source refreezing, and invalid early reserve prefixes followed
by valid final states. Semantic corruptions re-sign all downstream prefixes;
representative uniqueness/head/managed-task/reserve fixtures repair corresponding
receipts and pin both oracle diagnostics and native error codes. Backups/current
files and lock paths may be absent, malformed, unsafe, FIFO or held without becoming
reader dependencies. Success/refusal snapshots retain bytes, namespace, modes,
inodes and mtimes; success without Python on PATH remains covered.

Focused graph units exercise exact node/edge/size limits, lexical topology without
array normalization, digest vectors, stale enrollment snapshot refreezing and
source-match reason ordering. Extraction units carry actual cancellation identities,
immutable receipts and predecessor state. Fixed work totals (54,929 v1 bytes and
81,919 through v2 upgrade for one fixture) pin the shared meter, with exact
precharged MAX_WORK success/one-over refusal; deliberately resetting the embedded
meter was verified to fail the regression. CLI work-profile fixtures separately
cover conservative combined preflight, not this private shared-meter invariant.

Action inspection adds **132 cases**: 54 accepted parity, 53 control refusals,
16 native-profile refusals, 4 resource-policy refusals and 5 unsupported-CLI refusals.
Real Python enable/prepare/review/claim/outcome fixtures never execute runtime submit.
Cases cover both plan backends, all-claim validation, stale/opaque bundles, claimed/
unknown/terminal and acknowledged late outcomes, consumed-reference independence,
current-file/lock FIFO and held-lock independence, optional artifact diagnostics,
copy/move refusal and actual device/inode plus 2^64 truncation traps. Recorded
claims are duplicated in output; a dedicated DEL fixture proves the escaped report
can exceed 32 MiB even when control alone and compact output fit, with empty stdout.
Independently calculated work formulas and precharged command seams pin exhaustion
inside optional bundle validation, compact output and ASCII output without huge
allocations. Plan units cover sorted/spaced exact limits and frozen Python vectors.

Pure checkpoint inspection adds **2,761 byte-level oracle cases**: 2,212 accepted
views and 549 rejected inputs, including 12 exact resource refusals. These cover
framed digest vectors, Unicode status/trim/lowercase, line endings, scanner and
machinery precedence, nested examples, trailers/internal history, presentation
contradiction and stale scope. Parser resource errors retain their classification
through action inspection; ordinary malformed JSON remains a diagnostic retaining
the known claim. Existing storage/numeric corpora also protect unchanged codec modes.

Standalone checkpoint workspace inspection adds **280 CLI cases**: 101 accepted,
150 semantic/preflight refusals, 13 parser-boundary refusals, 15 explicit profile
refusals and one permission-sensitive marker-selection probe. The permission probe
ran here and pinned cwd selection/first configuration error; it explicitly skips
when privilege makes denial ineffective. Cases cover marker/empty/tilde ambiguity,
failure order, full managed inventories, team/TOML/environment/state, sorted Unicode/
dotfile/name edges, pending import, active authority with unsafe projections, valid
runtime control and unusable bundle children, ignored execution control, held locks,
no-Python PATH, unchanged bytes/modes/inodes/mtimes and old-command policy separation.
Unit tests compare managed inventories to Python constants, test exact 4096/4097
scan boundaries and controlled authority changes between independent ledger reads.
An independent nested TOML scalar/runtime-field corpus protects calendar and numeric
profile behavior; parser grouping tests preserve semantic arity checks after preflight.

Frozen receipt lookup adds **95 direct-library oracle cases**: 25 results/misses,
16 changed-binding conflicts, 8 invalid keys, 38 invalid bindings, 2 absent/pending
authorities and 6 corrupt-tail read refusals. Python-produced histories preserve
old bindings/results after later owner changes; raw aliases, stale original CAS,
object order, extensions, numeric identity, clones, poisoned current files and
marker changes are characterized without CLI or writer activation. The corpus
has a 60-second isolated child-process watchdog with parent-owned fixture cleanup
so accidental FIFO reads cannot hang the suite. Units pin multi-receipt accounting,
exact budget/overflow, programmatic string bounds, raw float version spellings and
the pre-existing replay validation precedence.

Mutable replacement adds a **120-combination fault matrix** across existing/absent
targets, permission modes, I/O boundaries and cleanup outcomes, plus a silent-SGID-
loss regression using the real descriptor verifier. Failures before rename preserve
old bytes/mode/inode; post-rename parent-sync failure retains new bytes as uncertain.
Tests cover wrong ownership tokens, unsafe paths/FIFOs, both lock names, missing
parents, exact/over transport cap and inert orphans. A local mixed Python/Rust test
performs 65 Python and 64 Rust writes under the same persistent flock, checks whole
payload observations from both writers, 16 exclusion checks, preserved special
mode bits and unchanged lock device/inode. The SGID regression simulates the kernel
result; no administrator/nonmember-group setup or crash/power-loss test is claimed.

Locked authority confirmation adds **95 direct-library cases** with independently
pinned counts: 24 ledger confirmations, 30 active confirmed-clear reservation
checks, 2 disabled observations, 6 reservation blocks, 31 invalid-authority refusals,
1 native-profile refusal and 1 absent ledger. Three Python-first and three native-
first lock-order scenarios verify append/prepare/cancel serialization, exclusion
even after SDK errors, borrowed-lock lifetime and persistent lock identity.
Fixtures and native probes have bounded watchdogs and process-group cleanup.

Ledger units trace the exact selected files and leaf-to-root directories for
present/absent baselines and multiple frames. Every sync-stage path/open/descriptor/
fsync fault checks outcome and immediate stop; pre-sync reread failures remain hard.
Independent revision/head checks, changed manifest digest/type, corrupt tails and
wrong-root-before-I/O are covered. Execution units inject all sync faults across
matching/unrelated/authority-wide checks in clear/prepared/cancelled and V2 approved/
graph-prepared/revoked states, plus second-read cancellation/upgrade/disappearance/
corruption. Existing 54,929/81,919 work counters and 295 report-parity cases continue
passing. Synchronization may change storage durability; no namespace/content/mode/
inode/mtime mutation is expected, and atime is not used as an invariant.

Existing Python regressions remain **941 tests**. They are not all black-box CLI
tests; a passing Python suite alone is not proof of Rust coverage. Native fault
injection and mixed Python/Rust contention will be required for writers.

## Migration coverage and next gates

| Surface | Current implementation | Next gate |
| --- | --- | --- |
| Handoff schema, validate, historical inspect/list | Native Rust preview; Python still default | Close documented path gap before default parity |
| Handoff collection/publication | Native bounded unverified capture; Python still default | Close profile limits/cross-platform validation before default parity |
| Finite values, ordered extensions, records, snapshot task JSON listing | Native bounded preview; Python still default | Close stated acceptance limits before full legacy parity |
| Persistent workspace flock, immutable evidence publication and mutable replacement | Native guarded library primitives, mixed-language/fault tests | Domain-safe authority publication, recovery and writer integration |
| Workspace init/refresh and task mutations | Python only | Storage/authority guards and mixed-writer fault tests |
| Task-ledger read/replay, frozen receipt lookup, selected confirmation and projection diagnostics | Native bounded library/read-only preview | Close supported-profile limits; durable retry coordination/delivery still required |
| Task-ledger mutations/import/recovery | Python only | Receipt/head/guard pipeline, immutable publication/uncertainty, recovery and fault tests |
| Execution v1/v2 historical replay, graph reviews and confirmed reservation checks | Native bounded SDK/read-only preview | Close profile limits; writer integration and receipt delivery/order still required |
| Execution enable/upgrade, graph mutations, preparation/cancellation guards | Python only | Guarded publication, approval/reserve/receipt ordering and mixed-writer fault tests, not dispatch |
| Runtime-launch action inspection and pure checkpoint decisions | Native bounded read-only preview | Close profile/encoding limits before default parity |
| Standalone checkpoint inspection and ordered workspace preflight | Native bounded read-only preview | Close documented parser/TOML/path/resource restrictions before default parity |
| Checkpoint mutations, action consumption/outcomes, repository authority | Python only | Existing scoped approval, receipt ordering, publication/recovery and uncertainty contracts |
| Observers, groups, watch/publication | Python only | Private endpoint/artifact differential tests, expiry/no stale fallback |
| Native probing/start/resume/prompt/legacy transports | Python only | Transport gates, target pins, cleanup and no retry after uncertainty |
| Packaging/default executable/Python retirement | Unchanged | Whole-command parity, asset locations, install/upgrade testing |

The next substantive stage is domain-safe immutable publication/activation and
recovery, followed by the receipt/head/CAS/guard writer pipeline. Frozen lookup,
confirmation and replacement are not yet a mutation retry API. Writers must preserve persistent flock inode/order,
same-directory publication/modes/fsync, receipt-before-mutable-state lookup,
corrupt-authority refusal, prepared-task guards and exit-2 uncertainty. No old
preparation, observation, collected claim or runtime-launch receipt may become a
new execution grant because its implementation language changed.
