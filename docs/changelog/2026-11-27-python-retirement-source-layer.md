# Source-test dependency-layer closeout — 2026-11-27

## Scope

Closed the remaining runner-backed Rust source-test dependency layer as one
user-approved batch: **34 constructor sites in 25 files**, seven automatically
executed Python helpers and one projection-regeneration-only helper deleted.
The [approved review](../../tests/retirement/reviews/source_dependency_layer.json)
is authoritative for the nine evidence slices, 18 fixture/provenance pins and eight
retired-helper identities; this record does not duplicate its hash inventory.
No production changes, original-method mappings or global deletion approval.

## Inventory and retained Python

Literal source constructors fell **39 → 5**, Rust files **29 → 5**, runner-backed/mixed
callers **25 → 0**, source Python files **11 → 3**, runner-dependent helpers **8 → 0**.
Remaining independent comparisons: `unicode.rs`, `cli_operands/tests`, `workspace.rs`
tomllib, `checkpoint_guidance/timestamp`, and `workspace_commands/python`. The three
source helpers are the worker fixture and the latter two comparisons' helpers.
Integration inventory is unchanged: **28 targets = 26 runner-backed + 2 independent**.
All **25 runner modules / one tracked bytecode file** remain.

Cumulative removal is **23 + 34 = 57 counted source/integration sites**, plus one
variable-executable site reported separately; **15 + 8 = 23 helpers** includes the
regeneration-only deletion, which earns no invocation credit. Counts are lexical,
not Rust-AST or general dynamic-execution proof. Independent helper history remains
**4 active / 1 retired / 5 historical**; original ledger **90 mapped / 851 unresolved
of 941**, categories C **0**, unchanged. Helper C accounting is separate (**4 + 1**).

## Independent evidence and native coverage

Nine new optional historical freezers require explicitly selected source at
`a249a5dfe8d32fd75022f8ed2ad524264b7436a0`; source pins include all 25 runner modules.
Independent capture used verified Python **3.14.8 `-I -B`**, never native-generated
expectations, copied implementation or current-tree fallback. Complete inputs,
outputs, opaque large integers, raw storage and hashes remain pinned. The near-node
capacity recipe independently matches the old full bytes without a giant literal
blob; existing projection vectors are byte-unchanged.

Two group comparison setups use the existing deterministic `Fixed` UUID seam with
real filesystem publication; other public/SystemIo guards remain unchanged. All
178 ordered group cases remain. Full real-filesystem, fault, CAS and race coverage
is retained. Requests validate before deferred native execution, including an
explicit mismatch/no-effects demonstration. Scoped independent reviews and final
review accepted corrected upgrade integer typing, request ordering and evidence pins.

## Historical-only interoperability dispositions

Legacy Python cross-language lock, native-first preparation and initializer
qualifications now have explicit **historical-only dispositions**. Independent
native children preserve actual shared/exclusive contention, same-inode fencing,
death/reap/reacquisition, stale CAS and no-attempt/no-receipt assertions, plus the
noncooperating marker-writer race and exact no-extra-output guards. This is **not
a current Python-interoperability claim** and does not delete original tests.

## Consolidated validation

Use the [phased plan](../plans/PYTHON_REMOVAL.md) to select the next dependency layer;
use this evidence for its scoped source closure, not as a full-suite green gate.

- **580 distinct normal checkout passes = 371 affected + 209 prior regressions**.
  All affected **371 normal / 369 strong** passed in checkout **and** extracted
  package. Strong excludes only independent timestamp and workspace tomllib;
  both passed normally. The other three independent source comparisons are outside
  the affected closure and are not strong-qualified by this run.
- Full library inventory is **1,628 including 11 ignored**, not all passed. Four
  new ignored IPC child entries are exercised by parents, not extra pass counts.
- Fresh CLI built before library in the same profile; formatting, whitespace and
  offline locked two-job all-target checking passed. Two native package tests and
  **53 audit-tool tests** passed once. Ordinary audit passed; completion exited 1
  solely for the expected 851 unresolved obligations. The 979-method Python
  inventory (941 original + 38 supplemental) and 7-target/55-ID candidate inventory
  are unchanged, not newly executed suites.
- First extracted compile failed because three readers embedded excluded optional
  tools. Readers now use literal source-pinned capture-tool hashes, not shipped
  tools; source assertions were not weakened. Incremental **62 normal / 61 strong**
  reader reruns are not additive coverage; package reran the full **371 / 369**.
- Actual dirty-tree source packaging ran outside Cargo tests. Before evidence-only
  closeout: **505 files / 31,314,176 uncompressed / 2,706,368 compressed bytes**.
  All 502 non-generated payloads and `Cargo.toml.orig` matched source; traversal,
  duplicate/link/special/prohibited checks passed. All 18 evidence inputs and the
  provisional review were included; runner, Git, optional tools and retired helpers
  were absent. This is validation-time packaging, not a final archive-size promise.
- Strong bwrap isolation exposed only the exact test ELF, fresh CLI, native probe,
  individual dependencies/verified loader aliases, proc/dev and owned work/scratch.
  No host checkout/home, Python, shell, compiler, provider or broad executable/library
  trees. Eight native absolute-path `ENOENT` controls passed before test execution.
  This proves scoped filesystem absence, not universal exec denial.
- Linux Rust/Cargo **1.99.1**, verified disk-backed btrfs cache, scoped temporary/build
  paths, offline two-job builds and system OpenSSL. One owned **4.1 GiB** target was
  temporarily retained for package repair/reuse, then fully removed. All owned
  children were stopped/reaped and runs cleaned, including corrected failed cleanup
  attempts; pre-existing `pn.HC9lGV` was untouched. No retained owned artifacts,
  downloads, staging, commits, deployments, app restarts or provider calls.

No full Rust/original Python/runner-backed integration suite, non-Linux platform,
installed/release acceptance or live-provider qualification is claimed.

## Next layer and remaining gates

The [plan](../plans/PYTHON_REMOVAL.md) makes the **26-target integration native-fixture /
CLI-workflow layer next**, not a per-helper regression/documentation cycle. Preserve
assertions, faults, races, exact expected bytes and explicit Python-only dispositions;
do not assume every target is trivial. Historical-evidence audit cutover, all 941
dispositions and runner deletion/platform gates follow. See the
[ledger guide](../../tests/retirement/README.md) and
[retirement policy](../plans/RUST_RETIREMENT.md). Earlier dated records remain history.
