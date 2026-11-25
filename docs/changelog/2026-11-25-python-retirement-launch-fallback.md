# Launch executable-fallback oracle removal — 2026-11-25

Removed the last live Python dependency from the launch unit-test subtree and
deleted `src/launch/executable/native_oracle.py` after closing its sole consumer.
All **28 launch tests** now pass with Python unavailable, including default
in-test Rust fixture compilation and real process execution. This does not close
the separate Python-backed runtime-launch integration workflows or make
`runner.launch` deletable. See the [removal plan](../plans/PYTHON_REMOVAL.md).

## Preserved real execution

The original twelve combinations remain: tmux/herdr × launch/stop × missing
interpreter/missing file/exit 127. Tests still execute the unchanged compiled inert
backend through `NativeRunner`, not a mocked transport.

- Missing interpreter is a real kernel pre-image failure, followed by the good
  candidate with a smaller remaining timeout.
- Missing file is excluded during selection; only the good candidate is attempted.
- An executed program returning 127 never causes fallback.
- Entire plans, reports and ordered calls are compared, including arguments,
  CWD, exact environment, exit code, stdout and stderr. Attempts, candidate order,
  output cap, deadline consumption and partial-progress flags remain asserted.
- Adjacent injected status/deadline/exhaustion tests and production sources are
  unchanged. No fixture override, skip or copied Python implementation was added.

## Independent frozen evidence

Expectations were captured from Python 3.14.8 `-I -B` and verified historical
source at `d6515b0ada93036b28869ed1b9ed60633a2c1bbf`, using the unchanged inert
Rust fixture. The corpus is 100,021 bytes and has exactly twelve case IDs.

Only **92 explicitly inventoried CWD leaves** are parameterized in expected plans
and calls. The test checks sentinel values and command-specific argument positions
before substitution; actual native values are never normalized. All other fields
remain literal. Retained raw helper output, raw cases and slot-ledger hashes allow
the captured bytes to be reconstructed exactly. Corpus SHA256 is checked before
native use. The freezer requires explicit historical source and has no active-tree
fallback.

Independent review found no assertion or provenance blocker. Its terminology
correction records **90 mapped / 851 unresolved**, not 90 retired tests. This
batch has zero new original-method mappings; all 941 original test methods and
their source pins remain unchanged.

## Validation

| Gate | Recorded result |
| --- | --- |
| Full launch subtree, normal execution | 28 passed, no skips |
| Full launch subtree, Python-filesystem isolation, checkout | 28 passed, no skips |
| Full launch subtree, same isolation, extracted source package | 28 passed, no skips |
| Native package checks | 2 passed |
| Audit-tool suite | 40 passed |
| Formatting, whitespace, offline locked all-target check and package compilation | Passed |

These are overlapping executions, not 84 distinct launch tests. Both isolated
runs used the default `compiled()` path with actual rustc compilation. The
sandbox exposed only selected native compiler/linker tools, the Rust Cellar and
required libraries, not full host executable directories or Homebrew. Six absolute
Python probes failed with `ENOENT`. An initial runner omitted `/bin/sh`, which an
unchanged PATH-selection test discovers; adding only that individual shell binding
allowed both complete filters to pass. No test assertion or selection changed.
`strace` was unavailable, so no full exec-trace or universal seccomp proof is claimed.

Source-package checks require the corpus, review record and real fixture source,
and reject the retired helper. The extracted package test did not mount the
checkout. The source archive contains 457 files, 21,883,033 uncompressed file bytes
and 2,059,511 compressed bytes. Other library subtrees still require Python.

Ordinary audit passes and completion deliberately fails solely for the same
851 unresolved obligations. The ledger, original source files and seventeen
earlier fixture/provenance files are unchanged. This run did not execute the full
Rust or original Python suite and grants no platform/global-retirement acceptance.

Owned capture/build/package/sandbox artifacts were cleaned. No installs, service
restarts, deployments, staging, commits or pushes occurred. Existing reference
metadata and new evidence are in `tests/retirement/fixtures/launch_fallback.json`
and `tests/retirement/reviews/launch_fallback_oracle.json`.
