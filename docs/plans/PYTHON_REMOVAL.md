# Removing the legacy Python implementation

## Goal and boundary

Remove the old `runner/` implementation and the automatic test dependencies that
still execute it, without losing useful behavior or assertion coverage. The
installed product is already native Rust; this is development-oracle cleanup.

Independent stdlib development utilities, such as the native installer and
read-only audit, are a separate policy boundary. Removing the legacy implementation
does not mean the repository must contain no Python files whatsoever. Do not move
or copy the implementation into test support and call that removal.

## Dependency-layer strategy

The user approved layer-sized batches, not per-helper ceremonies. Run focused
capture and static implementation work in parallel, then use **one shared review
and normal / Python-absence / source-package / audit gate** for the layer. Reuse
build targets after fixes; do not repeat full regression gates or documentation
records for every module. Preserve assertions, faults, races and complete expected
bytes; explicitly disposition Python-only contracts rather than deleting tests.

1. **Source-test dependency layer — closed.** All 34 remaining runner-backed
   constructor sites across 25 source callers are closed; independent Python
   comparisons are outside this acceptance.
2. **Integration native-fixture / CLI-workflow layer — next.** Close the 26
   runner-backed targets as a layer, with focused inventory groups for workspace,
   actions, ledger, execution, checkpoints and observations. These targets are not
   presumed trivial or interchangeable.
3. **Historical-evidence audit cutover and runner deletion — later.** Resolve or
   explicitly disposition all 941 original obligations, transition the audit's
   live-source requirements, then satisfy runner-deletion and platform gates.

## Current snapshot — 2026-11-27

| Measure | Before first removal batch | Current |
| --- | ---: | ---: |
| Original requirements with accepted native mappings | 67 | **90** |
| Original requirements still unresolved | 874 | **851** |
| Direct Python constructor sites in Rust source | 61 | **5** |
| Rust source files containing those sites | 44 | **5** |
| Runner-dependent or mixed source caller files | 40 | **0** |
| Source-local Python files | 25 | **3** |
| Runner-dependent source-local Python files | 21 | **0** |
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
need manual follow-up. The five remaining source comparisons are independent:
`unicode.rs`, `cli_operands/tests`, `workspace.rs` (tomllib),
`checkpoint_guidance/timestamp` and `workspace_commands/python`. The three remaining
source Python helpers are the worker fixture and the latter two comparisons'
helpers; all are independent. Capture tools under `tools/` are outside this metric.

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

### Persisted Djinn decoder — 2026-11-27

- Replaced the decoder comparison with all 161 raw payloads and complete projected
  outcomes/errors, preserving duplicate/nonfinite/depth/extension boundaries and
  the 4,300-digit unknown integer without input narrowing.
- Deleted the helper after closing its sole live consumer. All ten module tests
  pass without Python in checkout and extracted package, with a matching fresh
  native supervisor and inert shell producer. No installed Djinn/provider was
  probed or qualified.
- Other transport/redaction/identity/profile assertions remain unchanged; no new
  original-method mapping is claimed. See the
  [dated evidence](../changelog/2026-11-27-python-retirement-djinn-decoder.md).

### Group publication counter bytes — 2026-11-27

- Replaced all sixteen primitive/publisher, declaration-order, filesystem-stat and
  prior-stream comparisons with independently captured complete authority bytes.
  Real native input construction, socket identity, mutation order and signal-owner
  isolation remain. Only reviewed expected-side scalar spans bind to pre-state.
- Deleted the single counter helper and the caller's `runner/groups.py` checkout
  discovery. All five native publisher tests pass without Python in checkout and
  extracted package; maximum integers, exact ordering and final newlines remain.
- The separate static/live engine oracle remained active at this step and is closed
  by the subsequent batch below. No original-method mapping is claimed. See the
  [dated evidence](../changelog/2026-11-27-python-retirement-group-counter.md).

### Group publisher static/live engine — 2026-11-27

- Replaced both consumers of the shared engine oracle with all sixteen independently
  captured complete report/write outputs. The original eight modes in each static
  and live variant, two-member accounting and fault distinctions remain. Complete
  inputs are checked before native execution and remain unchanged afterward.
- Deleted the helper after closing both callers. No dynamic bindings, output
  normalization or expectation generation from native results are used.
- All **88 publisher tests** pass without Python in checkout and extracted package,
  including existing native socket, timing, expiry, interrupt and signal paths.
  This closes publisher-local test oracles, not global runner consumers or original
  requirement mappings. See the
  [dated evidence](../changelog/2026-11-27-python-retirement-group-publisher-engine.md).

### Fresh group-command binding/storage — 2026-11-27

- Replaced all three original snapshot-null, snapshot-native and process-null
  comparisons, retaining exact storage bytes, SHA-256, authority revision and
  compact binding. No fourth matrix cell was invented.
- Native public create/add, original producer and complete pre-mutation seed
  remain. Only fifteen reviewed expected-side UUID spans vary; expected revisions
  use independently checked bounded canonical content, not production hash output.
- Deleted the helper after closing its sole caller. All six group-command tests
  pass without Python in checkout and extracted package, retaining unknown-field,
  large-integer/render and mutation exit-code guards. No production code or original
  mapping changed. See the
  [dated evidence](../changelog/2026-11-27-python-retirement-group-command-storage.md).

### Source-test dependency layer — 2026-11-27

The [approved layer review](../../tests/retirement/reviews/source_dependency_layer.json)
closes all remaining runner-backed source callers: **34 sites in 25 files**, seven
automatically executed helpers and one projection-regeneration-only helper deleted.
Nine new optional freezers require verified historical source, with no active-tree
fallback. The completed group-authority work includes all 178 ordered cases.

The shared Linux gate passed **580 distinct normal checkout tests** (371 affected
plus 209 prior regressions). The affected closure passed **371 normal / 369 strong**
in both checkout and extracted package. Only the independent timestamp and tomllib
comparisons were excluded from strong isolation; both passed normally. The other
three independent comparisons are outside this affected closure, not qualified by
its strong gate. See the [single dated closeout](../changelog/2026-11-27-python-retirement-source-layer.md)
for preserved concurrency contracts, package corrections and qualification limits.

Cumulatively, **23 + 34 = 57 counted source/integration invocation sites** and one
known variable-executable site have been removed. **15 + 8 = 23 helpers** were
deleted, including one regeneration-only helper with no invocation credit. No whole
`runner/` module is yet deletable: integration parents still import it.

## Next layer

Inventory the **26 runner-backed integration targets** by the workflow groups above,
including parent harnesses, generated producers, CLI setup and transitive imports.
Replace dependencies with native fixtures or independently captured evidence while
retaining original assertions, complete expected bytes, no-effect guards, fault
ordering and races. Keep explicit Python-only dispositions and genuine gaps visible.
The two independent integration targets remain separate: all 28 Python-launching
targets are unchanged by source-layer closure. Dependency removal and accepted
original-method mappings remain separate measures of progress.

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
