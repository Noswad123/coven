# Coven Rust migration

Status: **first native read-only slice**, not a complete Rust replacement.
The retained Python implementation and its safety contracts remain the public
default and compatibility oracle. `bin/coven` is unchanged. No workspace/state
migration, executable replacement, install or authority activation happens when
the Rust crate is built.

## Native commands available

```sh
cargo run --offline --locked --bin coven-rs -- handoff validate FILE --json
cargo run --offline --locked --bin coven-rs -- handoff inspect WORKSPACE COLLECTION_ID --json
cargo run --offline --locked --bin coven-rs -- handoff list WORKSPACE --task TASK_ID --json
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
  platform/          Compile-gated OS open flags and unsupported-platform refusal
```

Serde/serde_json provide typed JSON, clap the CLI, sha2 the existing digest
algorithm, and Unix libc the no-follow/nonblocking descriptor flags needed to
refuse symlinks/FIFOs safely. Tempfile is test-only. These are conventional cached
crates, not application/runtime dependencies or a new plugin framework.

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
This is **not** a general Python-compatible canonicalizer for arbitrary task/
ledger extension values. Float spelling/thresholds/negative zero and large integer
handling must be characterized before state writers or authority replay move.
Never silently adopt Serde/JCS encoding under existing v1 digest domains.

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

## Validation

Use the existing installed Rust toolchain; this slice was exercised with Rust/
Cargo **1.96.1**. The manifest declares Rust 1.85 compatibility but that compiler
version has not been independently run. Cargo.lock is generated offline.

```sh
just test-rust     # Native unit/integration tests, including Python oracle
just test-python   # All retained Python regressions
just check         # rustfmt check, strict clippy, both test suites
```

The native suite has 12 tests, including an integration harness with **109 parity
cases** and one explicit deferred non-UTF-8 reporting case. Characterizations cover
otherwise-valid duplicate keys and escaped-equivalent keys, required null versus
missing, literal integer versions, Unicode/byte boundaries, source-state claims,
frozen canonical bytes, Python-produced literal-Unicode archives, unknown fields,
hash/binding/filename/encoding/LF corruption, historical reads despite corrupt
current tasks, full-cohort filtering, 256/257 entries, exact/over byte limits,
inert temps, symlink/FIFO refusal and effect-free unsupported commands.

Existing Python regressions remain **941 tests**. They are not all black-box CLI
tests; a passing Python suite alone is not proof of Rust coverage. Native fault
injection and mixed Python/Rust contention will be required for writers.

## Migration coverage and next gates

| Surface | Current implementation | Next gate |
| --- | --- | --- |
| Handoff schema, validate, historical inspect/list | Native Rust preview; Python still default | Close documented path gap before default parity |
| Handoff collection/publication | Python only | Shared storage/locks, uncertainty and mixed-writer tests |
| Workspace records, init/refresh, task snapshots | Python only | Extension-preserving typed models and general canonicalization |
| Task ledger/import/recovery | Python only | Exact frame/hash/receipt replay and authority-selection tests |
| Execution v1/v2, graph/preparation guards | Python only | Identity/approval/reservation compatibility, not dispatch |
| Checkpoints/actions/repository authority | Python only | Existing scoped approval/publication/uncertainty contracts |
| Observers, groups, watch/publication | Python only | Private endpoint/artifact differential tests, expiry/no stale fallback |
| Native probing/start/resume/prompt/legacy transports | Python only | Transport gates, target pins, cleanup and no retry after uncertainty |
| Packaging/default executable/Python retirement | Unchanged | Whole-command parity, asset locations, install/upgrade testing |

The next substantive stage is general record/codec compatibility and shared
storage/authority readers. Writers must preserve persistent flock inode/order,
same-directory publication/modes/fsync, receipt-before-mutable-state lookup,
corrupt-authority refusal, prepared-task guards and exit-2 uncertainty. No old
preparation, observation, collected claim or runtime-launch receipt may become a
new execution grant because its implementation language changed.
