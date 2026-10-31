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
  execution.rs       Historical execution replay state, v1 rules and report boundary
  execution/v2.rs    Upgrade bridge, graph reviews and v2 preparation replay
  execution/graphs.rs Pure bounded graph validation, refreezing and source matching
  actions.rs         Captured launch-control/bundle/checkpoint inspection
  actions/authority.rs Pure all-claim validation, reference lookup and work budget
  actions/plans.rs   Pure deterministic tmux/Herdr frozen-plan validation
  checkpoints.rs     Pure byte-oriented checkpoint views and framed scope hashes
  storage.rs         Shared workspace guard and immutable evidence publication
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
rewritten, accepted via hashes alone or replaced by a projection. The read view
retains request IDs for validation, not a public frozen-receipt lookup API. Mutation
receipt replay must be ported before writers, not inferred from inspection success.

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
The standalone `checkpoint inspect` CLI is **not ported yet**: its broad Python
workspace preflight is a separate compatibility gate, not silently bypassed here.

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

## Validation

Use the existing installed Rust toolchain; this slice was exercised with Rust/
Cargo **1.96.1**. The manifest declares Rust 1.85 compatibility but that compiler
version has not been independently run. Cargo.lock is generated offline.

```sh
just test-rust     # Native unit/integration tests, including Python oracle
just test-python   # All retained Python regressions
just check         # rustfmt check, strict clippy, both test suites
```

The native suite has 105 tests, including a build/CLI-name regression test and an
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

Existing Python regressions remain **941 tests**. They are not all black-box CLI
tests; a passing Python suite alone is not proof of Rust coverage. Native fault
injection and mixed Python/Rust contention will be required for writers.

## Migration coverage and next gates

| Surface | Current implementation | Next gate |
| --- | --- | --- |
| Handoff schema, validate, historical inspect/list | Native Rust preview; Python still default | Close documented path gap before default parity |
| Handoff collection/publication | Native bounded unverified capture; Python still default | Close profile limits/cross-platform validation before default parity |
| Finite values, ordered extensions, records, snapshot task JSON listing | Native bounded preview; Python still default | Close stated acceptance limits before full legacy parity |
| Persistent workspace flock and immutable evidence publication | Native guarded primitives, mixed-language/fault tests | Task-authority publication and native writer guard ordering |
| Workspace init/refresh and task mutations | Python only | Storage/authority guards and mixed-writer fault tests |
| Task-ledger authority read/replay and projection diagnostics | Native bounded read-only preview | Close supported-profile limits before full parity |
| Task-ledger mutations/import/recovery | Python only | Receipt lookup/order, publication/uncertainty and fault tests |
| Execution v1/v2 historical replay, graph reviews, receipts/reservation observation | Native bounded read-only preview | Close profile limits; durable writer guard and receipt lookup ordering still required |
| Execution enable/upgrade, graph mutations, preparation/cancellation guards | Python only | Guarded publication, approval/reserve/receipt ordering and mixed-writer fault tests, not dispatch |
| Runtime-launch action inspection and pure checkpoint decisions | Native bounded read-only preview | Close profile/encoding limits; standalone checkpoint preflight and writers remain |
| Checkpoint CLI/mutations, action consumption/outcomes, repository authority | Python only | Existing scoped approval, receipt ordering, publication/recovery and uncertainty contracts |
| Observers, groups, watch/publication | Python only | Private endpoint/artifact differential tests, expiry/no stale fallback |
| Native probing/start/resume/prompt/legacy transports | Python only | Transport gates, target pins, cleanup and no retry after uncertainty |
| Packaging/default executable/Python retirement | Unchanged | Whole-command parity, asset locations, install/upgrade testing |

The next substantive stage is standalone checkpoint CLI/preflight parity and guarded
publication/recovery primitives. Historical receipt reports are not yet a mutation
retry API. Writers must preserve persistent flock inode/order,
same-directory publication/modes/fsync, receipt-before-mutable-state lookup,
corrupt-authority refusal, prepared-task guards and exit-2 uncertainty. No old
preparation, observation, collected claim or runtime-launch receipt may become a
new execution grant because its implementation language changed.
