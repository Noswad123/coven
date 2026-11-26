# Execution-source and history oracle removal — 2026-11-26

Removed two live Python constructors and their sole-consumer helpers:
`src/execution/source/oracle.py` and
`src/workspace_commands/history/oracle.py`. Complete native test groups now pass
with Python unavailable in checkout and extracted source package. Production source,
ledger, value-codec and history code is unchanged. See the
[removal plan](../plans/PYTHON_REMOVAL.md).

## Execution-source byte fidelity

All eighteen source tests remain active. Four original caller tests retain five
complete canonical-byte comparisons:

- A snapshot with unknown extensions, custom owner/status, float `-0.0`, a 30-digit
  integer, `1e-07`, literal UTF-8 `λ🦊`, nested null/bool values and roster order.
- Exact `[]` and whitespace-bearing ` \n[]\t`, with different raw-input revisions.
- A fresh task snapshot written after workspace preflight.
- A newly imported ledger selected after that preflight.

Missing-input and invalid-blank controls remain; missing input is not counted as
a sixth former oracle comparison. The real post-preflight writes, guarded import,
confirmed durability and imported-task assertions remain dynamic native operations.

Expected source stdout stays opaque bytes. Neither expected nor actual source
values pass through default serde JSON numeric conversion. The ledger template
permits only two generated import-report IDs at four fixed 32-byte positions in
identity/revision, with lowercase-hex and context guards. Restoring captured IDs
reproduces the complete original stdout and digest. Revision prefix, sequence 0,
task digest, tasks and roster stay independent historical literals. No source
result, imported revision or task array supplies the expectation.

The other fourteen tests are unchanged, retaining owner/root/thread/fork checks,
work budgets, projection independence, pending authority, refusal ordering,
read-only/no-artifact guarantees and finite/encoding limits. This closes source
observation units, not the other execution mutation/coordination oracles.

## History fidelity

The complete 232-row historical corpus remains: 225 record rows × Events/Messages
for **450 exact pretty outputs**, plus **seven raw outputs**. This preserves
truthiness, defaults, fallback precedence, repr, bool/integer differences, `-0.0`,
the 101-digit `10**100`, Unicode/control characters, CR/CRLF and literal newlines.
Duplicates and original order remain; no output normalization, bindings,
deduplication or sampling were introduced.

Serde unwraps only an outer envelope of JSONL strings. Each complete row still
uses `values::parse(..., LegacySnapshot)` before native file writes and actual
history rendering. All 232 row hashes and the exact 42,199-byte historical stdout,
including its final LF, are checked. The other nine history tests retain their
filesystem, lock, selected-log, raw-tail, refusal and resource assertions unchanged.

## Independent reference and validation

Both captures use explicitly selected Git history at
`ca3b19d87c955d6415231b1c4ebbbc591195a105` and Python 3.14.8 `-I -B`, not native
outputs. Source pins are checked before imports: 37 for source observation and
28 for history, each including all 25 runner modules. Raw captures, source/tool
hashes, case inventories and limits remain in their provenance. Both native
decoders pin corpus/provenance bytes. Optional freezers require historical helpers;
they do not copy implementations or fall back to the active tree.

| Gate | Recorded result |
| --- | --- |
| Full execution-source group, normal | 18 passed, no skips |
| Full history group, normal | 10 passed, no skips |
| Both groups without Python, checkout | 18 + 10 passed, no skips |
| Both groups without Python, extracted package | 18 + 10 passed, no skips |
| Prior launch / identity / worker-unit regression groups | 28 / 14 / 11 passed |
| Native package checks | 2 passed |
| Audit-tool suite | 53 passed |
| Formatting, whitespace, offline locked all-target check and package compilation | Passed |

These are scoped, overlapping executions—not a whole Rust-library result. The
source group's unchanged fork/child-exit and `waitpid` assertions ran normally
and in both sandboxes. Sandbox mounts expose only selected ELF libraries/loader,
an owned writable cwd/temp area, proc/dev and read-only source; no compiler,
Homebrew or full executable tree. Source uses an individual guard shell; history
uses native absence checks with no shell. Five absolute interpreter execution
probes return `ENOENT`; the stdlib directory/shared library are also absent.
This is filesystem-isolation evidence, not exec-trace or universal seccomp proof.

The extracted package does not mount the checkout. Package checks require all
new data/decoder/review inputs and reject restored retired helpers. The archive
contains 468 files, 22,291,358 uncompressed file bytes and 2,112,669 compressed bytes.

No original-method mappings were added: **90 mapped / 851 unresolved**, original
IDs/source pins, accepted reviews and the **4 active / 1 retired / 5 historical**
independent registry remain unchanged. Ordinary audit passes; completion fails
solely for those unresolved obligations. No full Python/native suite, platform
qualification, installation or deployment is claimed. Owned capture/build/package
and sandbox artifacts were cleaned. No restarts, staging, commits or pushes occurred.
