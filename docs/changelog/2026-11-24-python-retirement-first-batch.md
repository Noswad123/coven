# First native-oracle removal and mapping batch — 2026-11-24

This batch reduces real dependencies on the retained Python implementation and
accepts new assertion-level native evidence. It does not remove `runner/` or
declare global retirement complete. The [removal plan](../plans/PYTHON_REMOVAL.md)
tracks the remaining path to deletion.

## Removed live dependencies

Eight direct Python constructor sites were removed from three Rust test files:

| Caller | Before → after |
| --- | --- |
| `src/runtime/presentation/tests.rs` | 5 → 0 |
| `src/task_delivery/tests.rs` | 2 → 0 |
| `src/launch/tests.rs` | 1 → 0 |

Presentation/dashboard tests now use 30 frozen outputs and two reference-prepared
checkpoint inputs. Launch tests use 298 cases: all 232 previous comparison inputs
plus 66 transcripts from selected original tests. Existing matrices, exactness
policies, effect guards and refusal assertions remain active. Expected data came
from pinned Python reference code at `24d73fe`, not Rust-generated outputs.

`src/launch/oracle.py` was deleted after closing its only live consumer. Optional
recapture requires an explicit historical reference tree; the Python implementation
was not copied into the new capture utility. The separate executable-fallback
oracle remains, so whole-launch-subtree Python independence is not claimed.

## Accepted original requirements

Independent assertion review and parent integration accepted:

- Launch: **nine A / two B mapped**, with eight D methods unresolved. The happy
  transport kwargs/environment-identity contract and invalid-stop report-validator
  analogue remain gaps despite useful captured cases.
- Groups: **nine B mapped**, preserving retry/CAS, no-write, identity, capacity,
  whole-cycle counter and canonical-revision relational assertions.
- Handoff schema: **three B mapped**, covering authority-field refusal, source
  claims and strict JSON decoding. Mutable-return identity and dynamically typed
  API requirements remain unresolved.

Overall: **90 mapped / 851 unresolved**, from 67 / 874. Categories are now
**62 A / 29 B / 0 C / 850 D**. The original 941 IDs, all 44 source pins and eight
previous fixture/provenance files are unchanged. Prior 80 review objects and
67 accepted mappings remain intact. Supplemental inventory stays 38 and the
candidate integration whitelist stays seven targets / 55 IDs. Presentation adds
zero original-method mappings; dependency removal is tracked separately.

## Validation

- **68 focused native tests passed**: presentation 9, task delivery 19, full launch
  28, groups retirement 9 and handoff retirement 3.
- **61 tests passed with the interpreter unavailable** in a bubblewrap filesystem:
  presentation 9, task delivery 19, launch comparison 7, launch retirement 14,
  groups 9 and handoff 3. Empty executable directories, no Homebrew/home mounts,
  and an absolute `/usr/bin/python3` execution probe failing with `ENOENT` establish
  filesystem isolation—not only PATH hiding. This is not a universal no-exec
  seccomp claim. Seven existing launch executable tests remain outside this gate;
  they passed separately in the full launch filter.
- **40 audit-tool tests passed**. Ordinary audit passes; completion deliberately
  exits 1 solely for 851 unresolved obligations.
- **Two package checks passed**. All four new JSON files and Rust helpers are
  included; the retired oracle is absent. The 450-file archive was extracted and
  its library tests compiled without Python on PATH. That compile smoke is
  separate from the stronger runtime isolation above.
- Offline locked all-target check, source/fixture invariants and whitespace passed.
  The initial formatting check found test-module ordering; it was corrected and
  the final full formatting check passed.

Native totals are scoped test executions, not full migration acceptance. The full
941/979 Python suite and broad Rust library suite were not run. No new all-platform,
provider, runtime or deployment claim follows. Existing production behavior is
unchanged; additions to production-module files are test-gated declarations.

Reference capture used installed Python 3.14.8 with explicit source pins and
isolated `-I -B` execution. Rust validation used cached offline locked dependencies,
two jobs and system OpenSSL. Original test assertions were not deleted or disabled.
Corpus/provenance hashes, method/case inventories and review limits are in
`tests/retirement/fixtures/` and the accepted review fragments.

All owned capture/build/package/isolation artifacts were cleaned. No installation,
service restart, staging, commit or push was performed. The source package still
contains other oracles and the checkout still retains all 25 runner modules.
