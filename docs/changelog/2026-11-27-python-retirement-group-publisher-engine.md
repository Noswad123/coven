# Group publisher static/live engine oracle removal — 2026-11-27

## Scope and retained comparisons

Closed both consumers of `src/group_publisher/oracle.py` and deleted the shared
helper after removing its Python launch and ancestor-based checkout discovery.
Both original static/live test names remain. Production publisher behavior and
all existing test bodies are unchanged; only the shared comparison implementation
and its new frozen-evidence reader changed.

All **sixteen cases** remain: static then live, each with success, reservation
error, reservation uncertainty, invalid reservation, rejected delivery, lost
acknowledgment, unsent failure and unknown activity. The static unknown case is
retained separately even where output equals success. Each still uses the two-member
fixture, fixed clock, original fault/reservation setup and accounting invariants.
Preparation callbacks, write order and rejected/uncertain/pre-send distinctions
remain exercised.

Every complete serialized input is checked against independently authored captured
stdin before native execution, then checked unchanged afterward. Complete compact
`{report,writes}` bytes compare directly to historical stdout. There are **zero
dynamic bindings** and no parsing, normalization or reserialization of expected
stdout; literal Unicode, numeric spelling, key order and absent final newline stay
byte-sensitive.

With the preceding [counter removal](2026-11-27-python-retirement-group-counter.md),
the full `group_publisher::` test module has no Python/runner prerequisite. Other
group-command and group-authority oracles remain outside this closure.

## Independent capture and review

The optional freezer requires an explicit historical export of Git commit
`727edc7aad55ed76e98670e0cee6449d611b2285`, outside the checkout, with bytecode
excluded. All **53 source pins**, including 25 runner modules and both callers,
are checked before historical helper execution. There is no current-tree fallback
or copied publisher implementation.

Python **3.14.8 `-I -B`**, in a controlled environment, executed the unchanged helper
twice for every independently transcribed input. All paired outputs matched; the
whole capture was repeated with byte-identical evidence files (**64 successful
helper invocations** across the two runs). All helper effects were its original
mocks, not live providers. `-B` prevents bytecode writes, not every interpreter
cache read.

Whole evidence hashes, strict row/shape/ordered-ID checks, per-row hashes and full
length-framed raw stdin/stdout streams preserve cardinality and byte boundaries.
The streams contain **63,114 stdin bytes / 15,231 stdout bytes**, including sixteen
eight-byte length prefixes each; row strings retain the original unframed bytes.

| Evidence | Bytes | SHA-256 |
| --- | ---: | --- |
| `group_publisher_engine.json` | 93,370 | `8087e14b3e9c76660ec639260c74f5021d7f02ccfcaf950aeb0953c8b0759ec8` |
| `group_publisher_engine_provenance.json` | 163,806 | `ae7638134f1720a5e91bb1f4f10a4a17ec05ea612178e33e65ceb118c13c937b` |

All 53 per-path in-memory tamper controls and active-checkout rejection passed
before helper execution. Independent read-only review reconstructed every input
from the original fixture definitions and verified source/evidence hashes, framed
streams, complete byte comparisons and unchanged tests, finding no blockers.
The [approved review](../../tests/retirement/reviews/group_publisher_engine_oracle.json)
contains capture and validation commands and detailed scope limits.

## Validation and cleanup

- Formatting, whitespace and offline locked all-target checking passed.
- **203 distinct selected library tests passed normally:** publisher 88, Djinn 10,
  repositories 24, launch 28, identity 14, workers 11, source 18 and history 10.
  The Djinn regression used a matching freshly built CLI in the same target/profile;
  launch retained installed compiler discovery. This is not a full Rust-suite run.
- All **88 publisher tests**—42 static, 41 live, five native—passed normally and
  in strong Python-absence isolation in both checkout and a separately built
  extracted source package, with zero failures or ignored tests. Repeated runs
  are not additional distinct tests. Existing real Unix effects, expiry, timing,
  interrupts and owned self-exec/signal paths remained enabled.
- Both sandbox absence flags ran native absolute-path `ENOENT` checks for Python
  interpreters, stdlib and libpython, in both engine consumers and the counter
  child. Only the test ELF, individual dependencies and verified loader aliases,
  proc/dev and owned writable work/scratch were mounted. No checkout, shell,
  compiler, CLI, provider, interpreter, home or broad executable/library trees.
  This proves scoped filesystem absence, not universal exec/seccomp denial.
- Two native package tests and **53 audit-tool tests** passed. Actual dirty-tree
  Cargo packaging occurred outside Cargo tests, with traversal/duplicate/link/
  special-file/prohibited-payload checks. All twelve publisher/Djinn evidence and
  reader/review inputs were present; all three retired Python helpers were absent.
  The package contained no runner or Git directory and was rebuilt in a separate
  fresh target. At validation time, before evidence-only closeout edits, its
  **480 files** totaled **25,572,625 uncompressed / 2,291,477 compressed bytes**;
  477 non-generated payload files and `Cargo.toml.orig` matched source exactly.
- Ordinary audit passed before and after tests. Completion mode exited **1**
  solely for **851 unresolved original obligations**. All 103 pinned runner,
  original-Python, coverage, audit and Cargo paths remained unchanged; source and
  Git index snapshots were unchanged throughout validation. No production edits.
- Linux, Rust/Cargo 1.99.1, Python 3.14.8, offline locked two-job builds, installed
  caches and system OpenSSL. Unique verified disk-backed runs scoped `TMPDIR` and
  `CARGO_TARGET_DIR`; cleanup stopped/waited owned child groups before exact run
  removal. All owned capture/export/build/package/sandbox artifacts were removed,
  with no failures or retained artifacts. Pre-existing `pn.HC9lGV` was untouched.

No full Rust/original Python suite, renewed Djinn sandbox, non-Linux platform,
installed/release acceptance, actual producer or global Python retirement was
qualified. No downloads, deployment, app restart, staging or commit occurred.

## Inventory and next boundary

Current source inventory: **40 literal Python constructor sites in 30 Rust files**,
26 runner-dependent/mixed caller files and twelve source-local Python files, nine
runner-dependent. Constructor counts are lexical, not general execution proof.
Cumulatively 22 counted sites, one known variable-executable site and fourteen
helpers have been removed. Original status remains **90 mapped / 851 unresolved**;
independent-helper history remains **4 active / 1 retired / 5 historical**.

Next is the three-case fresh binding/storage comparison in group commands, not
the much broader group authority/concurrency oracle. Preserve raw storage bytes,
hash, revision and compact binding while reviewing generated identity handling.
See the [removal plan](../plans/PYTHON_REMOVAL.md) and
[ledger guide](../../tests/retirement/README.md).
