# Worker parser and report unit-oracle removal — 2026-11-25

Removed both live Python constructors from `src/workers/tests.rs` and deleted
their shared `src/workers/oracle.py` after consumer-closure and independent
assertion review. All eleven worker unit tests pass without Python in checkout
and extracted source package. Production worker/help/process code is unchanged.
See the [removal plan](../plans/PYTHON_REMOVAL.md).

## Exact retained scope

- **31 parser vectors:** twelve generated route/variant inputs, eighteen explicit
  help-text cases and invalid UTF-8. All original input bytes, route order,
  command/flag arrays and raw-input help hashes remain. The reference accepts
  sixteen and rejects fifteen; cardinality checks replace the old truncating zip.
- **Five complete report templates:** complete, version, missing route, missing
  route plus transport failure, and missing route plus parse failure. Full
  structured reports, exact human strings, exit codes, newline handling and
  redaction remain compared.
- The same five cases now also run through an explicit directory alias, in
  addition to the original input. Lexical `selected_path` is distinct from
  canonical workspace/resolved paths, probe argv and human output. A review found
  and corrected an expected-side binding bug that had conflated those spellings.
- Other malformed/version/visible-Unicode tests, the 65,536-item boundary,
  duplicates, request/environment/limit checks, filesystem mutation and alias
  guards, and private-output/workspace-preservation tests are unchanged.

The expected identity binds only device, inode and nanosecond mtime from direct
OS observations of the declared file. Its size (18 bytes) and independent Python
digest stay literal. Workspace/executable fields use an explicit binding inventory;
actual reports are not path/identity/hash normalized. The only comparison
exception is the existing per-probe `observed_at` field, now checked for presence
and valid UTC/calendar syntax before exclusion.

Human-path binding requires a printable-ASCII canonical fixture-root prefix. The
literal Unicode/newline filename is preserved. This is an explicit test-fixture
precondition, not a product path restriction or universal Unicode-root renderer
claim. Parser vectors should not be described as a broad valid-Unicode corpus;
the separate native Unicode/visible-text guards and report-path checks remain.

## Independent evidence

Python 3.14.8 `-I -B` capture uses verified historical source at
`6e92c180ca19fed05950bdb08ea8a3e7b10f5526`. All 41 Git source pins, including
25 runner modules, tracked bytecode, helper and native source were checked.
`-B` is not claimed to prohibit cache reads. No native output generates expected
results, and no worker/parser implementation is copied into test support.

The two hash-pinned files contain 134,315 corpus bytes and 131,644 provenance bytes.
Raw parser output and all five report-helper outputs are retained and checked.
Exactly **66 JSON-pointer bindings plus five human segments** reconstitute the
complete original records. Only declared expected fields vary at test time.
The optional freezer requires explicit historical source, not an active fallback.

## Validation and boundary

| Gate | Recorded result |
| --- | --- |
| Full worker units, normal | 11 passed, no skips |
| Full worker units without Python, checkout | 11 passed, no skips |
| Full worker units without Python, extracted package | 11 passed, no skips |
| External symlinked-TMPDIR full-report regression | Passed normally and in both sandboxes |
| Existing launch and identity regression groups, normal | 28 and 14 passed |
| Separate worker integration target, normal | 4 passed with retained Python producer |
| Native package checks | 2 passed |
| Audit-tool suite | 53 passed |
| Formatting, whitespace, offline locked all-target check and package compilation | Passed |

The unit sandbox exposes only individually selected libraries/ELF loader, a guard
shell, read-only source and an owned writable cwd/temp directory. It exposes no
full root, library tree, host executable directory, compiler, Homebrew or user
home. Five absolute interpreter execution probes fail with `ENOENT`; Python's
stdlib directory and shared library are also absent. No observed fixture
executable is run. This is filesystem-isolation evidence, not an exec-trace or
universal seccomp proof. The extracted-package run does not mount the checkout.

Source packaging requires the corpus, provenance, review and native decoder and
rejects restoration of the retired oracle. It explicitly retains the independent
`src/workers/fixture.py` producer. The 462-file archive has 22,190,954 uncompressed
file bytes and 2,087,187 compressed bytes.

**Worker integration is not Python-free.** Its controlled producer and four-test
target are separate from this runner-backed oracle and remain active. The
independent registry remains four active + one retired = five historical helpers.
Original accounting stays **941 requirements, 90 mapped / 851 unresolved**; no
new mapping, runtime authorization, platform qualification or full-suite result
is claimed. Ordinary audit passes; completion still fails solely for unresolved
requirements.

Owned capture/build/package/sandbox artifacts were cleaned. No deployments,
installations, restarts, staging, commits or pushes occurred. The reference
metadata and narrow parent acceptance are in
`tests/retirement/reviews/worker_readiness_oracles.json`.
