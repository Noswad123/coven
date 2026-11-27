# Repository-record admission oracle removal — 2026-11-26

Removed the single live Python comparison in repository unit tests and deleted
`src/repositories/oracle.py` after closing its sole executable consumer. All
twenty-four repository tests now pass without Python in checkout and extracted
source package, retaining real Git fixture behavior. Repository registration,
record decoding, timestamp validation and Git observation code are unchanged.
This is test-dependency removal, not a new repository-management feature.

## Complete historical admission corpus

All **1,979 raw record strings** remain in their original authored order:

| Segment | Inputs |
| --- | ---: |
| Base | 1 |
| Top-level field mutations | 13 |
| Inode bool/float/integer variants | 5 |
| Raw huge-integer token | 1 |
| Six paths × two field targets | 12 |
| Nine dates × four separators × nine clocks × six zones | 1,944 |
| Null, empty object and duplicate-member object | 3 |

Reference results contain **684 admitted / 1,295 refused** booleans. Duplicate
inputs remain separate indexed entries; no filtering, sampling or deduplication
occurred. The historical corpus contains **no NaN/Infinity inputs**; this corrects
an earlier plan description rather than inventing pre-existing coverage.

The native decoder verifies complete corpus/provenance hashes, literal boolean
types, counts, IDs and per-input digests. Before any admission comparison, every
frozen raw string must equal its unchanged authored Rust input. There is no
truncating zip. Serde handles only the outer string/boolean envelope; embedded
records are never parsed/reserialized through its bounded numeric representation.
The raw huge-integer token and schema bool/float distinctions remain intact.

The original post-comparison inspect and exact-registration-replay assertions
remain unchanged. They use `Never` to reject Git probes and require success plus
the complete huge-integer substring in both outputs. This preserves those original
assertions; it is not a new whole-report byte-equivalence claim.

## Independent reference

Expected booleans come from the unchanged historical Python helper and
`datetime.fromisoformat`-based record validation at
`b97823c7f4f30e53f768258347eaf66daec74053`, not native results or a copied
acceptance formula. Python 3.14.8 ran with `-I -B` and a clean environment against
an explicit, source-verified Git export. Capture requires no Git observations or
registrations against any checkout.

All 31 source pins, including 25 runner modules, are checked before execution.
Literal input construction is independent of `Authority::decode`. Raw request and
boolean stdout hashes are retained; two captures matched. Native checks pin both
new JSON files. The optional freezer requires historical source and has no active
checkout fallback or copied validator.

## Validation

| Gate | Recorded result |
| --- | --- |
| Full `repositories::` filter, normal | 24 passed, no skips |
| Same filter without Python, checkout | 24 passed, no skips |
| Same filter without Python, extracted package | 24 passed, no skips |
| Prior launch / identity / worker / source / history regressions | 28 / 14 / 11 / 18 / 10 passed |
| Native package checks | 2 passed |
| Audit-tool suite | 53 passed |
| Formatting, whitespace, offline locked all-target check and package compilation | Passed |

The full repository group retains six direct tests, one admission comparison,
five Git-unit tests and twelve real-Git tests. SHA-1/SHA-256, linked worktrees,
split indexes, byte/metadata preservation, concurrency, publication faults and
all four Git-pin child reexecution modes run unchanged. Git writes are confined
to owned disposable fixtures; source/user repositories and global config are not
modified.

Python-free sandboxes expose actual Git 2.56.0, individual `sh`/`touch` binaries,
templates and ELF dependencies, not fake Git, broad executable/library trees,
Homebrew, user home, compiler or network. A positive shell/touch canary proves
those tools can run before the exclusion tests. Five absolute interpreter probes
return `ENOENT`; native checks also require absent stdlib/libpython paths. The
extracted-package run does not mount the checkout. This is Linux filesystem-
isolation evidence, not full-platform or execution-trace proof.

Source packaging requires the new data/review/decoder and rejects the retired
helper. The archive contains 471 files, 24,226,658 uncompressed file bytes and
2,228,251 compressed bytes. Other Python-dependent subtrees remain.

Original-method mapping stays **90 mapped / 851 unresolved**, with no new credit.
The 941 IDs, 44 source pins, previous fixtures/reviews and **4 active / 1 retired /
5 historical** independent-helper registry are unchanged. Ordinary audit passes;
completion fails solely for unresolved obligations. No full native/Python suite,
installed package or non-Unix qualification is claimed.

Owned capture/build/Git-fixture/package/sandbox artifacts were cleaned. No user
configuration change, installation, deployment, restart, staging, commit or push
occurred. See the [removal plan](../plans/PYTHON_REMOVAL.md) for remaining callers
and deletion gates.
