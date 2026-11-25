# Removing the legacy Python implementation

## Goal and boundary

Remove the old `runner/` implementation and the automatic test dependencies that
still execute it, without losing useful behavior or assertion coverage. The
installed product is already native Rust; this is development-oracle cleanup.

Independent stdlib development utilities, such as the native installer and
read-only audit, are a separate policy boundary. Removing the legacy implementation
does not mean the repository must contain no Python files whatsoever. Do not move
or copy the implementation into test support and call that removal.

## Current snapshot — 2026-11-25

| Measure | Before first removal batch | Current |
| --- | ---: | ---: |
| Original requirements with accepted native mappings | 67 | **90** |
| Original requirements still unresolved | 874 | **851** |
| Direct Python constructor sites in Rust source | 61 | **50** |
| Rust source files containing those sites | 44 | **39** |
| Runner-dependent or mixed source caller files | 40 | **35** |
| Source-local Python files | 25 | **22** |
| Runner-dependent source-local Python files | 21 | **18** |
| Python-launching integration targets | 29 | **28** |
| Runner-backed integration targets | 27 | **26** |
| Legacy runner modules / tracked bytecode files | 25 / 1 | **25 / 1** |

The original checklist remains **941 methods in 44 files**. Its source pins and
IDs are unchanged. Supplemental tests remain separate: 38 methods, for a total
979-method Python inventory. An unresolved requirement is not necessarily a
missing feature; some need evidence, some need additional tests, and Python-only
API obligations need an explicit disposition rather than invented Rust objects.

Constructor counts are lexical, not complete execution counts. They use the
auditor's comment-aware Rust tokenization and
`Command::new\(\s*"python(?:3)?"\s*\)` matches in source/test files. Variable
executables, generated shebangs, macros, shell indirection and transitive helpers
need manual follow-up. The 50 total sites are **not** 50 exclusively runner-backed
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

No whole `runner/` module is deletable from these batches. In particular, launch's
separate executable-fallback test still calls `native_oracle.py`, and other modules
still import `runner.launch`.

## Next cohorts

1. **Launch executable fallback.** Replace the remaining live reference comparison
   while retaining real compiled-fixture checks for missing interpreters, missing
   files, exit 127, fallback selection, deadlines and exact attempts. Do not
   confuse this with the comparison helper already deleted.
2. **Process identity.** Address both the runner-backed identity oracle and the
   variable-executable stdlib `shutil.which` oracle in its mixed caller. Removing
   only the literal `Command::new("python3")` site would leave Python required and
   hide that remaining dependency from the simple constructor metric.
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
