# Process identity and PATH oracle removal — 2026-11-25

Removed both Python invocation paths from the process-identity tests and deleted
`src/process/identity/tests/oracle.py` and `which_oracle.py` after closing their
live consumers. The complete fourteen-test identity group passes without Python
in checkout and extracted source package. Production identity/platform code is
unchanged. See the [removal plan](../plans/PYTHON_REMOVAL.md).

## Preserved independent evidence

- The original 200,000-byte file test still compares the entire five-field public
  identity. Size and independently captured `hashlib` digest stay literal. Only
  device, inode and nanosecond mtime bind to direct OS observations of the declared
  fixture, using exact integers and `i128` timestamp arithmetic—not production
  metadata helpers or fields copied from the result.
- All six original PATH comparisons remain, with twelve additional captured,
  declarative cases for ordering, empty components, direct operands, lexical
  spelling, nonexecutable files, directories and aliases. Comparisons use raw
  `OsStr` values; actual results are not normalized.
- Unset PATH's default-node expectations use a finite `/bin:/usr/bin` table with
  direct file-type/access observations, not `find_in_path` on both sides or a
  frozen capture-host symlink layout.
- Existing growth/replacement/truncation races, negative timestamps, non-UTF8
  aliases, extreme integer values, limits and environment filtering are preserved.

Historical Python 3.14.8 capture uses verified `d6515b0` source, with 55 source
pins plus interpreter, stdlib and tool hashes. Both new fixture digests and the
exact twelve-case inventory are checked natively. Optional regeneration requires
explicit historical source. No identity/PATH implementation was copied into test
support and no new original-method mapping is claimed.

This is Unix-scoped evidence, validated on Linux UID/EUID 1000. Native lookup's
existing rejection of executable special files is stricter than `shutil.which`;
universal special-file or unprivileged permission-denial parity is not claimed.
The new Unix-only test helper and its two consumers are cfg-scoped so unsupported
platforms do not import Unix metadata extensions. All fourteen Linux names ran;
no non-Unix execution or platform qualification follows.

## Reviewed independent-helper transition

`which_oracle.py` was one of five separately registered independent stdlib helpers.
It was not silently dropped from that history. The ledger now records:

- `independent_python_baseline`: the exact five historical paths.
- `independent_python`: **four active helpers**.
- `retired_independent_python`: **one retired helper**, linked to the approved,
  hash-pinned identity review and its native tests and fixtures.

The audit requires unique, disjoint active/retired sets whose union is the
baseline. Missing history, incomplete/pending/tampered evidence, invalid linkage,
missing native tests, changed fixtures and helper resurrection—including aliases—
fail closed. Historical source identity was verified manually against Git; the
runtime auditor stays read-only and does not invoke Git or historical Python.

This changes no original category-C method count: it remains **zero**. Original
requirements remain **941**, with **90 mapped / 851 unresolved**. Original source
pins, reviews, candidates, accepted batches and supplemental inventory are intact.
Reversing only the helper-registry transition reproduces the previous ledger bytes.

## Validation

| Gate | Recorded result |
| --- | --- |
| Full identity group, normal | 14 passed, no skips |
| Full launch regression group, normal | 28 passed, no skips |
| Both groups without Python, checkout | 14 + 28 passed, no skips |
| Both groups without Python, extracted source package | 14 + 28 passed, no skips |
| Native package checks | 2 passed |
| Audit-tool suite | 53 passed |
| Formatting, whitespace, offline locked all-target check and package compilation | Passed |

These are 42 distinct selected Rust tests executed in several environments, not
a full-library total. Identity's sandbox exposes libraries, an owned writable cwd
and individual shell nodes for default-PATH observation, but no compiler, host
home, broad executable tree or Python. Six absolute interpreter probes return
`ENOENT`. Launch's separate sandbox adds only selected native compiler tools and
libraries. This is filesystem-isolation evidence, not an exec-trace/seccomp claim.

The extracted-package tests do not mount the checkout. The 459-file source archive
contains 21,908,375 uncompressed file bytes and 2,066,882 compressed bytes; required
evidence/helper inputs are present and both retired Python paths absent.

Ordinary audit passes; completion still exits 1 solely for 851 unresolved
requirements. No full Rust or original Python suite was run. Owned capture/build,
package and sandbox artifacts were cleaned after child completion. No deployment,
installation, restart, staging, commit or push occurred. Pre-existing uncommitted
launch-fallback work was preserved.
