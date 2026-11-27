# Removing the legacy Python implementation

## Goal and boundary

Remove the old `runner/` implementation and the automatic test dependencies that
still execute it, without losing useful behavior or assertion coverage. The
installed product is already native Rust; this is development-oracle cleanup.

Independent stdlib development utilities, such as the native installer and
read-only audit, are a separate policy boundary. Removing the legacy implementation
does not mean the repository must contain no Python files whatsoever. Do not move
or copy the implementation into test support and call that removal.

## Current snapshot — 2026-11-26

| Measure | Before first removal batch | Current |
| --- | ---: | ---: |
| Original requirements with accepted native mappings | 67 | **90** |
| Original requirements still unresolved | 874 | **851** |
| Direct Python constructor sites in Rust source | 61 | **43** |
| Rust source files containing those sites | 44 | **33** |
| Runner-dependent or mixed source caller files | 40 | **29** |
| Source-local Python files | 25 | **15** |
| Runner-dependent source-local Python files | 21 | **12** |
| Python-launching integration targets | 29 | **28** |
| Runner-backed integration targets | 27 | **26** |
| Legacy runner modules / tracked bytecode files | 25 / 1 | **25 / 1** |
| Independent helper registry: active / retired / historical | 5 / 0 / 5 | **4 / 1 / 5** |

The original checklist remains **941 methods in 44 files**. Its source pins and
IDs are unchanged. Supplemental tests remain separate: 38 methods, for a total
979-method Python inventory. An unresolved requirement is not necessarily a
missing feature; some need evidence, some need additional tests, and Python-only
API obligations need an explicit disposition rather than invented Rust objects.

Constructor counts are lexical, not complete execution counts. They use the
auditor's comment-aware Rust tokenization and
`Command::new\(\s*"python(?:3)?"\s*\)` matches in source/test files. Variable
executables, generated shebangs, macros, shell indirection and transitive helpers
need manual follow-up. The 43 total sites are **not** 43 exclusively runner-backed
sites; four caller files are independent-only and others can be mixed. New capture
tools under `tools/` are outside this source/integration metric.

## Completed batches

### First batch — 2026-11-24

- Standup, agent-status and dashboard comparisons now read **30 independently
  captured outputs and two checkpoint inputs**, rather than launching Python.
  Existing input matrices, guards, filesystem assertions and exact bytes remain.
- Launch comparison tests use **298 frozen reference cases**. Their only live
  consumer of `src/launch/oracle.py` was removed, so that helper was deleted.
- Independent review accepted **11 launch, nine groups and three handoff-schema
  requirements**, with two additional launch candidates deliberately unresolved.
- Focused native tests, interpreter-isolated tests, audit checks and extracted
  source-package compilation passed. See the
  [dated evidence](../changelog/2026-11-24-python-retirement-first-batch.md), not
  these counts alone, for exact scopes and limits.

### Asset/init and checkpoint batch — 2026-11-25

- Asset/init comparisons now use all 22 refresh cases, 44 launch comparisons,
  48 ordered init rows and complete defaults/example data without Python.
- Production-connected label words replace the independent `shlex` call while
  preserving all original unsafe-root literal goldens and syntax checks.
- Checkpoint integration now uses all 2,761 frozen rows, including twelve exact
  resource refusals and eleven historical duplicate-ID pairs, without live Python.
- Four more constructor sites and three helpers were removed. The original-method
  ledger is unchanged; **90 mapped / 851 unresolved** remains the honest status.
- Focused tests, strong interpreter-isolated tests, package checks and extracted
  source compilation passed. See the
  [dated evidence](../changelog/2026-11-25-python-retirement-assets-checkpoints.md)
  for exact capture and validation boundaries.

### Launch executable fallback — 2026-11-25

- Replaced the remaining live launch oracle with twelve complete, independently
  captured cases, using only explicit expected CWD slots. Real missing-interpreter
  fallback, missing-file selection and no-fallback exit 127 remain exercised.
- Deleted `src/launch/executable/native_oracle.py`, reducing another source call
  site and caller file. No new original-method mapping was claimed.
- All 28 launch tests pass without Python in both checkout and extracted source
  package, including actual default fixture compilation. See the
  [dated evidence](../changelog/2026-11-25-python-retirement-launch-fallback.md)
  for mount policy, limits and package/audit results.

### Process identity and PATH — 2026-11-25

- Removed both the literal and variable-executable Python consumers, plus their
  runner-backed identity and independent `shutil.which` helpers.
- Full five-field identity and all six original PATH cases remain; twelve captured
  cases strengthen lexical/path behavior. Runtime metadata binds only to declared
  direct OS observations, never to actual production results.
- All fourteen identity tests pass without Python in checkout and extracted
  package. A reviewed registry transition preserves the independent helper's
  history: four active plus one retired equals the same five baseline names.
- No new original-method mappings are claimed. See the
  [dated evidence](../changelog/2026-11-25-python-retirement-process-identity.md)
  for exact source, fixture, registry and validation limits.

### Worker parser and report units — 2026-11-25

- Replaced the two live unit comparisons with all 31 parser vectors and five
  complete report templates. Exact human output, redaction and exit codes remain.
- Deleted the shared runner-backed `src/workers/oracle.py`; all eleven worker
  unit tests pass without Python in checkout and extracted package. Selected and
  resolved spelling is separately preserved, including directory-alias regression.
- The independent Python producer and separate worker integration target remain
  active. This is unit-oracle closure, not whole-worker testing closure or a new
  original-method mapping. See the
  [dated evidence](../changelog/2026-11-25-python-retirement-worker-units.md).

### Execution-source observation and history — 2026-11-26

- Replaced five full source-byte comparisons across four callers. Snapshot raw
  revisions, signed zero, big integers, Unicode, post-preflight writes and actual
  guarded ledger activation remain. Only two import IDs bind four expected slots.
- Replaced the complete 232-row history comparison, retaining 450 exact pretty
  outputs, seven raw outputs and every native refusal/read-only/resource test.
- Deleted both source-local helpers after consumer closure. All eighteen source
  tests and ten history tests pass without Python in checkout and extracted
  package. Other execution test oracles remain; no new mapping is claimed. See
  [dated evidence](../changelog/2026-11-26-python-retirement-source-history.md).

### Repository-record admission — 2026-11-26

- Replaced the single comparison with all 1,979 authored raw inputs and independent
  booleans, retaining the 1,944 timestamp matrix and huge-integer inspect/replay.
  The original corpus has no NaN/Infinity cases; no such coverage is claimed.
- Deleted `src/repositories/oracle.py`. All twenty-four repository tests pass
  without Python in checkout and extracted package, including real Git and copied
  Git/test-binary reexecution in owned fixtures, not just the pure admission test.
- No production code or original mapping changed. See the
  [dated evidence](../changelog/2026-11-26-python-retirement-repository-records.md).

Cumulatively, nineteen counted source/integration invocation sites and one known
variable-executable site have been removed, with eleven helpers deleted. The
variable call is deliberately reported separately from the literal regex metric.
No whole `runner/` module is yet deletable: other modules and
Python-backed integration parents still import `runner.launch`, among others.

## Next cohorts

1. **Persisted Djinn decoder comparison.** Preserve the complete 161-payload
   matrix in `src/djinn_observation/tests.rs`: capability/schema, timestamp,
   nesting, extension/size limits and a 4,300-digit unknown integer without serde
   narrowing. The helper is decoder-only. Qualify the separate controlled-producer
   test with a built native supervisor; do not infer full-module acceptance from
   the decoder or hide independent producer/timing failures. No installed Djinn
   or provider probe is needed for reference capture.
2. **Group publication counter bytes.** Inspect all sixteen primitive/publisher,
   order, filesystem-stat and prior-stream variants. Preserve whole authority
   bytes, real mutation/ordering and signal-owner isolation. Dynamic fields need
   narrow independent bindings, and checkout discovery must stop requiring the
   current `runner/groups.py` source. Other publisher oracles remain separate.
3. Continue through execution/ledger/storage/concurrency and group observation
   dependencies. Each batch should remove actual callers or close specific
   original assertions; track those two kinds of progress separately.

## Required deletion gates

- [ ] Preserve or explicitly disposition every original assertion, subcase,
  failure boundary, no-effect guarantee and API/platform limitation. Accept
  mappings only after independent review; retain genuine gaps.
- [ ] Account separately for source-local and inline oracle scenarios, parent
  harnesses, generated Python producers, compile-time includes and regeneration
  tools. They are not all counted by the 941 original methods.
- [ ] Remove automatic dependencies on the current runner from native tests,
  integration targets, build/package inputs and development recipes. A retained
  optional freezer may require a deliberately selected historical checkout, not
  silently fall back to the current runner.
- [ ] Run the appropriate complete native gates with the interpreter unavailable,
  plus installed/package and platform checks required by existing policy. Record
  unresolved environments rather than declaring them qualified from Linux tests.
- [ ] Deliberately transition the auditor from requiring live original Python
  files to validating retained historical evidence and accepted native reviews.
  Its current hash/AST requirements must not be disabled early just to permit
  deletion. Preserve original identities and capture provenance after that cutover.
- [ ] Delete all 25 runner modules and the tracked `.pyc`, then prove no active
  consumer or copied implementation remains. Update docs and package assertions
  together. Remove now-obsolete regression harnesses only when their coverage is
  genuinely replaced or their Python-specific contract explicitly retired.

The audit's ordinary success verifies integrity, not deletion readiness.
`--require-complete` still fails for **851 unresolved obligations**. No source
removal, deployment or Git operation is authorized merely by this plan.

## Evidence and related guidance

- [Retirement ledger guide](../../tests/retirement/README.md): exact IDs, categories,
  reviewed fragments and audit commands.
- [Existing retirement policy](RUST_RETIREMENT.md): compatibility, platform,
  distribution and source-removal gates.
- [Development checks](../operations/DEVELOPMENT_CHECKS.md): proportional tests,
  disk-backed outputs and cleanup.
- [Changelog](../changelog/index.md): dated captures and validation results.
