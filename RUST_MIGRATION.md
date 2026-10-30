# Coven Rust migration

Status: **native read-only handoff and task-authority preview**, not a complete Rust replacement.
The retained Python implementation and its safety contracts remain the public
default and compatibility oracle. `bin/coven` is unchanged. No workspace/state
migration, executable replacement, install or authority activation happens when
the Rust crate is built.

## Native commands available

```sh
cargo run --offline --locked --bin coven-rs -- handoff validate FILE --json
cargo run --offline --locked --bin coven-rs -- handoff inspect WORKSPACE COLLECTION_ID --json
cargo run --offline --locked --bin coven-rs -- handoff list WORKSPACE --task TASK_ID --json
cargo run --offline --locked --bin coven-rs -- task list WORKSPACE --json
cargo run --offline --locked --bin coven-rs -- state inspect WORKSPACE --json
```

The compiled `target/debug/coven-rs` can also be invoked directly, including with
Python absent from PATH. Its production code has no Python, subprocess, runtime
or network dependency. Python is needed only for migration-oracle tests and the
existing default CLI. Unsupported commands, including `handoff collect`, reject
at argument parsing before effects; there is **no automatic Python fallback**.

These readers preserve [HANDOFF_CONTRACT.md](HANDOFF_CONTRACT.md): syntax-valid
handoffs remain worker claims, collected history remains unverified, authorization
none, dispatchable false and current task binding not checked. No source/check/
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
  read_fs.rs         Bounded read-only file/managed-path operations
  values/            Finite compatibility values, duplicate profiles and encoders
  records.rs         Extension-preserving validated workspace/task views
  tasks.rs           Preflight, captured task/state reports and authority fences
  ledger.rs          Complete bounded selected task-ledger replay/projection health
  storage.rs         Explicit library-only shared workspace flock primitive
  output.rs          Bounded safely escaped CLI presentation
  platform/          Compile-gated OS open flags and unsupported-platform refusal
```

Serde/serde_json provide typed JSON, clap the CLI, sha2 the existing digest
algorithm, and Unix libc the descriptor/lock operations. Ryu supplies shortest
ties-to-even float digits; the compatibility encoder separately applies Python's
formatting policy. TOML parses the existing workspace runtime configuration.
Tempfile is test-only. All are conventional cached crates, not an external agent,
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
general codec is used by bounded read-only ledger replay, not authority writers. Never
silently adopt Serde/JCS encoding under existing v1 digest domains.

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

`storage::WorkspaceGuard` is a **library-only** primitive for future workflows.
Explicit `acquire` may create a persistent mode-0600 lock; `existing` cannot create
a missing lock/directory. The token is root/PID/thread owned and not Send/Sync,
uses the same kernel flock/inode as Python, never steals/unlinks/replaces it, and
releases on normal exit or process death. The destructor avoids explicitly
unlocking a parent's inherited open-file description after fork. Task and handoff
reads do **not** acquire this token. No native task writer, authority publication,
import, recovery or automatic dispatch is enabled by its availability.

## Validation

Use the existing installed Rust toolchain; this slice was exercised with Rust/
Cargo **1.96.1**. The manifest declares Rust 1.85 compatibility but that compiler
version has not been independently run. Cargo.lock is generated offline.

```sh
just test-rust     # Native unit/integration tests, including Python oracle
just test-python   # All retained Python regressions
just check         # rustfmt check, strict clippy, both test suites
```

The native suite has 34 tests, including an integration harness with **109 handoff parity
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

Existing Python regressions remain **941 tests**. They are not all black-box CLI
tests; a passing Python suite alone is not proof of Rust coverage. Native fault
injection and mixed Python/Rust contention will be required for writers.

## Migration coverage and next gates

| Surface | Current implementation | Next gate |
| --- | --- | --- |
| Handoff schema, validate, historical inspect/list | Native Rust preview; Python still default | Close documented path gap before default parity |
| Handoff collection/publication | Python only | Shared storage/locks, uncertainty and mixed-writer tests |
| Finite values, ordered extensions, records, snapshot task JSON listing | Native bounded preview; Python still default | Close stated acceptance limits before full legacy parity |
| Persistent workspace flock | Native library primitive, mixed-language tests | Publication/uncertainty primitives and native writer guard ordering |
| Workspace init/refresh and task mutations | Python only | Storage/authority guards and mixed-writer fault tests |
| Task-ledger authority read/replay and projection diagnostics | Native bounded read-only preview | Close supported-profile limits before full parity |
| Task-ledger mutations/import/recovery | Python only | Receipt lookup/order, publication/uncertainty and fault tests |
| Execution v1/v2, graph/preparation guards | Python only | Identity/approval/reservation compatibility, not dispatch |
| Checkpoints/actions/repository authority | Python only | Existing scoped approval/publication/uncertainty contracts |
| Observers, groups, watch/publication | Python only | Private endpoint/artifact differential tests, expiry/no stale fallback |
| Native probing/start/resume/prompt/legacy transports | Python only | Transport gates, target pins, cleanup and no retry after uncertainty |
| Packaging/default executable/Python retirement | Unchanged | Whole-command parity, asset locations, install/upgrade testing |

The next substantive stage is execution/approval authority readers and guarded
publication/recovery primitives. Writers must preserve persistent flock inode/order,
same-directory publication/modes/fsync, receipt-before-mutable-state lookup,
corrupt-authority refusal, prepared-task guards and exit-2 uncertainty. No old
preparation, observation, collected claim or runtime-launch receipt may become a
new execution grant because its implementation language changed.
