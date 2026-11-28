# Group publication counter oracle removal — 2026-11-27

## Scope and retained behavior

Retired the sole live caller of `src/group_publisher/counter_oracle.py` and then
deleted that helper. The caller no longer discovers its root by requiring the
current `runner/groups.py`. Production publisher, group authority and observation
code are unchanged. The separate static/live publisher engine oracle remains
active; this is not whole-publisher or global Python retirement.

The original sixteen cases remain: primitive/publisher policy × declaration order
× ordinary/maximum filesystem stats × existing/new publication stream. They still
create actual owned Unix sockets, select their identities, generate native groups
and members, seed exact counters, reserve through the real mutation paths and
compare **complete authority bytes**. Publisher cases reverse the declaration
after session capture, reread it and require the canonical revision unchanged.
Existing endpoint order and reservation values remain separately asserted.

The self-reexecuted child still isolates process-wide signal ownership. All other
four native test bodies are unchanged, including adjacent 100-digit endpoint
mismatches and public publication with owned fake Unix peers/live evidence/signals.

## Independent capture and bindings

Reference source is Git commit `727edc7aad55ed76e98670e0cee6449d611b2285`, exported
outside the checkout without bytecode or a new worktree. The optional freezer
requires explicit historical/output roots and verifies **53 source paths**,
including all 25 runner modules, before invoking the unmodified helper. It refuses
the active checkout and altered sources; no copied implementation or fallback.

Python **3.14.8 `-I -B`**, with a controlled environment, captured an independently
authored whole-state template twice for each of sixteen cases: **32 invocations**.
Each input was restored independently; paired stdout bytes matched completely and
equaled the authority file written by the historical helper. The request inventory
is recorded as authored input, not falsely described as helper stdin. `-B`
prevents bytecode writes, not every transient interpreter cache.

Before any native session or reservation, complete native-generated input bytes
must match the captured initial bytes after narrowly reviewed expected-side token
splices. Bindings are limited to the socket path, ordinary device/inode stats,
observed-at timestamp and three native-generated UUID leaves. UUIDs must be
canonical lowercase UUIDv4 and pairwise distinct. The same pre-state tokens bind
the expected output before mutation; no actual post-result supplies expectations.

Every occurrence has an independently checked structural path, source path,
captured literal and byte range: **128 input + 144 output = 272 binding instances**.
No global replacement or authority reserialization is used. Maximum device/inode
integers remain literal, as do watermarks, revisions, keys, field order, whitespace
and final LF. Outer fixture JSON contains opaque authority strings; no authority
number is narrowed through serde/f64.

| Evidence | SHA-256 |
| --- | --- |
| `group_counter.json` (128,979 bytes) | `05c08611c0ecd220ab4c5aad891253aaf6bed33a35adb9e7f41fc034d1b4d31a` |
| `group_counter_provenance.json` (67,849 bytes) | `f779dba7327c8ea7fb9a60a464416e82527d1c7e76e32616d7a914650dbeb132` |

The [review record](../../tests/retirement/reviews/group_counter_oracle.json)
contains per-case slot counts, source pins, raw request/stdout hashes and repeat
commands. All 53 per-source in-memory corruption controls were rejected before
helper execution. Independent review verified the source hashes, complete fixture
inventory, bindings and unchanged tests, finding no blockers.

## Validation and cleanup

- Formatting, whitespace and offline locked all-target check passed.
- **120 distinct selected library tests** passed normally: native publisher 5,
  Djinn 10, repositories 24, launch 28, identity 14, workers 11, source 18, history
  10. Djinn used the matching freshly built CLI in the same target/profile; see
  [supervisor prerequisites](../operations/DEVELOPMENT_CHECKS.md#unit-supervisor-prerequisites).
  Repeated counter/sandbox executions are not additional distinct tests.
- The complete five-test native publisher module passed normally and with Python
  unavailable in both checkout and separately built extracted source package,
  with zero skips. Sandboxes mounted only the test ELF and individual required
  libraries/loader, proc/dev and owned writable work/scratch directories. No
  checkout, shell, interpreter, compiler, provider, broad library/executable tree
  or home was mounted. Absolute interpreter/stdlib/libpython `ENOENT` controls ran
  in the isolated counter child. This is filesystem absence evidence, not universal
  exec-trace or seccomp proof.
- Both native package tests and **53 audit-tool tests** passed. The actual dirty
  working-tree Cargo archive contained all required counter/Djinn inputs, neither
  retired helper, and no runner or Git directory. Extraction rejected traversal,
  duplicate entries and links/special files. At validation time, before evidence-only
  approval/documentation closeout, it contained **477 files / 25,290,011 uncompressed
  bytes / 2,274,329 compressed bytes**. All 474 non-generated payload files and
  `Cargo.toml.orig` matched their source inputs.
- Ordinary audit passed; completion mode exited **1** solely for **851 unresolved
  original obligations**. The original 941 IDs, 90 mapped obligations, runner,
  original Python tests, global coverage and independent-helper registry remain
  unchanged. Validation changed no source/index bytes.
- Two temporary validation-harness errors were corrected without source edits:
  a `Path.parent` typo and a PATH that hid the launch fixture compiler. Final full
  launch 28/28 and all listed gates passed; no test was weakened or skipped.
- Rust/Cargo 1.99.1, offline locked two-job builds and system OpenSSL used verified
  disk-backed unique runs with scoped `TMPDIR`/`CARGO_TARGET_DIR`. All owned capture,
  export, build, package and sandbox artifacts were removed after owned children
  stopped/reaped, including failed validation runs. Pre-existing `pn.HC9lGV` was
  untouched. No retained artifacts or cleanup failures.

No original Python suite, full Rust suite, remaining live publisher oracle,
non-Linux platform, installed/release acceptance or actual provider was qualified.
No deployment, app restart, staging or commit occurred.

## Remaining work

The [removal plan](../plans/PYTHON_REMOVAL.md) now records **41 literal source sites
in 31 files**, 27 runner-dependent/mixed caller files and 13 source-local Python
files, ten runner-dependent. These lexical counts are not full execution proof.
Original obligations remain **90 mapped / 851 unresolved**; no new mapping credit
is claimed. Next is the separate sixteen-case static/live publisher report/write
comparison, closing both consumers of its shared helper together.
