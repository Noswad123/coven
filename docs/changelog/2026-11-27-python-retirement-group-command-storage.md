# Fresh group-command binding/storage oracle removal — 2026-11-27

## Scope and retained behavior

Closed the sole executable consumer of `src/group_commands/oracle.py`, then deleted
the helper. All three original cases remain: snapshot with null native reference,
snapshot with full reference, and process binding with null reference. There is
no invented fourth matrix cell. Each still compares complete raw storage bytes,
storage SHA-256, authority revision and compact binding.

The original producer, owned temporary workspace, invalid `coven.json`, public
`create_group`, generated seed, `fresh_binding` and public `add_member` remain.
Complete seed and produced input are checked before binding reconstruction and
addition. Maximum `u64` endpoint device, inode 42, Unicode `session-é`, process
matches 7/9, exact field order and terminal LF remain covered. The other three
complete test bodies are unchanged: fifteen unknown-field variants, large-integer/
ASCII/render/LF behavior and nested-delivery mutation exit codes. Production code
and private mutation seams are unchanged.

## Independent capture and identity handling

Reference commit: `bbda1c4f79a7c71e6785c25d7bd9d939deec6d71`. The optional freezer
requires an explicitly selected historical export outside the checkout, excluding
tracked bytecode. All **71 source paths**, including 25 runner modules, original
helper/caller, native binding, groups/schema/store, value and UUID sources, are
checked against Git before executing the unchanged helper. No copied implementation
or current-tree fallback.

Python **3.14.8 `-I -B`**, in a controlled environment, captured independently
authored seed/producer inputs and a fixed participant ID. Every case was captured
twice; the whole capture was repeated with byte-identical evidence files: twelve
successful helper invocations across the two final complete captures. All 71
per-path in-memory corruption controls and active-checkout rejection passed before
helper execution. Full request/stdout streams use eight-byte length framing:
**2,625 request bytes / 6,596 stdout bytes**, prefixes included.

Only **fifteen expected-side UUID token spans** vary: two seed spans and three
stored-output spans per case. Workspace/group IDs come from the pre-add seed;
participant ID comes only from `result.member.participant_id`, the same input the
old helper already accepted. All three must be distinct canonical lowercase UUIDv4.
Each expected span has hardcoded structural path, literal and byte range. Actual
storage is never parsed, normalized or reserialized for comparison.

Expected raw SHA-256 uses bound expected bytes alone. Expected revision uses a
bounded independent test-only implementation of sorted compact UTF-8 content and
the NUL-terminated `coven.groups.content:v1` domain—not production revision or
canonicalization functions. Exact numeric tokens and object shapes are checked
before serde decoding; only the known unsigned integers can enter this calculation.
All three literal captured raw hashes, revisions, canonical bytes and domain hashes
validate before dynamic binding. Compact binding remains an unchanged captured
literal. No actual stored bytes, digest or revision supplies expectations.

| Evidence | Bytes | SHA-256 |
| --- | ---: | --- |
| `group_commands.json` | 20,131 | `6b8b2b1df050d3b9eb69296ca4fe85c12a58f0c037ccdb95ff959b39f8d22918` |
| `group_commands_provenance.json` | 27,586 | `d6c799ea1ef7b6a2b916706a6a51ba6616a8083b49ff8fc1eea801338a920f00` |

Independent review verified source/evidence hashes, all three authored inputs,
the fifteen spans, independent revision reconstruction and unchanged guards,
finding no blockers. The [approved review](../../tests/retirement/reviews/group_commands_oracle.json)
records detailed provenance, failures, commands and proof limits.

## Validation and cleanup

- Formatting, whitespace and offline locked all-target checking passed.
- **209 distinct selected checkout library tests passed:** group commands 6,
  publisher 88, Djinn 10, repositories 24, launch 28, identity 14, workers 11,
  source 18 and history 10. Fresh matching CLI built before library artifact for
  Djinn; installed compiler discovery retained for launch. Not a full Rust-suite run.
- All six group-command tests passed normally and with Python unavailable in both
  checkout and a separately built extracted source package. Every selected run had
  zero failures and ignored tests. The extracted package also passed 88 publisher
  tests normally; publisher isolation was not requalified. Repeats are not additional
  distinct tests.
- Strong isolation mounted only the test ELF, individual dependencies and verified
  loader aliases, proc/dev and owned writable work/scratch. No checkout, home,
  shell, CLI, compiler, provider, interpreter or broad executable/library trees.
  Native absolute interpreter/stdlib/libpython `ENOENT` controls passed. This is
  scoped filesystem absence evidence, not universal exec/seccomp denial.
- Two native package tests and **53 audit-tool tests** passed. Actual dirty-tree
  Cargo package generation ran outside Cargo tests. Traversal, duplicate entries,
  links/special files and prohibited payloads were checked. All four required
  group-command inputs matched source; retired helper, runner and Git tree were
  absent. Validation-time archive, before evidence-only closeout edits: **483 files /
  25,656,501 uncompressed bytes / 2,307,888 compressed bytes**. All 480 non-generated
  payloads and `Cargo.toml.orig` matched their source inputs.
- Ordinary audit passed before/after tests; completion mode exited **1** solely
  for **851 unresolved original obligations**. All 103 pinned original runner,
  Python-test, coverage, audit and Cargo paths remained unchanged against the new
  reference commit. Source/index snapshots and pre-existing bytecode were unchanged;
  no new bytecode was added.
- Two initial capture-harness failures were corrected: tracked bytecode was
  properly excluded from export, and Unicode storage scanning switched from ASCII
  assumptions to UTF-8 byte offsets. Failed runs were cleaned. Final captures,
  native tests and sandboxes passed; the parent aggregate run had no failures.
- Linux, Rust/Cargo 1.99.1, Python 3.14.8, offline locked two-job builds and system
  OpenSSL used verified disk-backed unique runs with scoped `TMPDIR` and
  `CARGO_TARGET_DIR`. All owned capture/export/build/package/sandbox artifacts and
  temporary harnesses were removed after owned process groups stopped/waited.
  Pre-existing `pn.HC9lGV` and user/source caches were untouched. No retained
  artifacts or cleanup failures.

No full Rust/original Python suite, non-Linux platform, installed/release acceptance,
live producer or global retirement was qualified. No downloads, deployment, app
restart, staging or commit occurred during this batch.

## Inventory and next boundary

Source inventory is now **39 literal Python constructor sites in 29 Rust files**,
25 runner-dependent/mixed caller files and eleven source-local Python files, eight
runner-dependent. Counts are lexical, not complete execution proof. Cumulative
removal is 23 counted sites, one known variable-executable site and fifteen helpers.
Original obligations remain **90 mapped / 851 unresolved**; independent-helper
history remains **4 active / 1 retired / 5 historical**. No new mapping credit.

Next is the five-consumer group-authority helper, preserving all 178 ordered
reference cases and independent exact revisions/storage while reviewing deterministic
setup identities. Other integration, fault and concurrency obligations remain.
See the [removal plan](../plans/PYTHON_REMOVAL.md) and
[ledger guide](../../tests/retirement/README.md).
