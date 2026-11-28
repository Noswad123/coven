# Persisted Djinn decoder oracle removal — 2026-11-27

Removed the sole live Python comparison from the persisted Djinn observation
module and deleted `src/djinn_observation/oracle.py` after consumer closure.
All ten module tests pass without Python in checkout and extracted source package,
including the unchanged controlled native-supervisor test. No installed Djinn,
provider, model or real persisted source folder was probed. Production observation,
schema, wire, process and supervisor code is unchanged.

## Complete decoder evidence

All **161 authored raw payloads** and their literal `/selected` request folders
remain in exact indexed order:

| Group | Cases |
| --- | ---: |
| Base | 1 |
| Required-field removals | 39 |
| Field/value mutations | 62 |
| Capability variants | 16 |
| Source variants | 22 |
| Missing/observed empty-source variants | 2 |
| Unavailable envelope | 1 |
| Raw malformed/duplicate/nonfinite JSON | 11 |
| Depth-33 array | 1 |
| Unknown-extension boundaries | 5 |
| Raw 4,300-digit unknown integer | 1 |

Reference outcomes are **23 observed, one unavailable, 127 schema errors,
two unsupported-contract errors, two invalid-JSON errors, two duplicate-key errors,
three nonfinite errors and one nesting-limit error**. Full projected facts and
producer metadata or exact error-only objects are compared, not admission booleans.

Payload strings remain raw through native dispatch. The outer serde envelope never
parses the 4,300-digit input integer; validated unknown extensions are discarded
only by normal projection. Literal NaN/Infinity and overflow `1e999` retain their
distinct errors. Exact lengths, IDs, request folders, per-payload hashes and all
raw input bytes are checked before any native decode, eliminating truncating zip.
Separate invalid-UTF8, 4,301-digit refusal, redaction, finite-profile, source-cap,
character/byte-boundary and post-probe identity assertions remain unchanged.

Independent expectations come from the historical pure Python decoder at
`727edc7aad55ed76e98670e0cee6449d611b2285`, using Python 3.14.8 `-I -B` and an
explicit verified export without bytecode. Thirty source pins include all 25
runner modules. Two captures matched exactly; request-file and stdout bytes and
digests are retained. No native output generated expectations, no decoder was
copied into support code, and optional regeneration has no active-tree fallback.

## Full-module prerequisites and validation

The controlled test is not a supervisor bootstrap: it requires the actual CLI at
`current_exe().parent().parent().join("coven")`. Both validation trees first built
the fresh CLI, then library tests in the same target/profile, and checked the
Cargo-reported paths against that locator. No stale installed binary or override
was used. The [development guide](../operations/DEVELOPMENT_CHECKS.md#unit-supervisor-prerequisites)
records this requirement.

| Gate | Recorded result |
| --- | --- |
| Full persisted-observation module, normal | 10 passed, no skips |
| Full module without Python, checkout | 10 passed, no skips |
| Full module without Python, extracted package | 10 passed, no skips |
| Prior repository / launch / identity / worker / source / history regressions | 24 / 28 / 14 / 11 / 18 / 10 passed |
| Native package checks | 2 passed |
| Audit-tool suite | 53 passed |
| Formatting, whitespace, offline locked all-target check and package builds | Passed |

Normal testing covers 115 distinct selected library tests, not the whole library.
All nine non-oracle test bodies remain byte-identical. The controlled test really
executes the native private supervisor and its owned inert `/bin/sh` script,
verifying arguments, original selector spelling, cwd `/`, stdin EOF, fixture JSON,
non-UTF8 stderr and unchanged script bytes/mtime/directory entries. This does not
qualify installed Djinn or live provider behavior, nor establish the cause of an
earlier unrelated full-library failure.

The sandbox preserves the required binary layout as `/native/deps/coven-tests`
and `/native/coven`, with only the shell, exact ELF dependencies/loaders, proc/dev
and owned writable scratch/cwd. It mounts no checkout, full host root, executable
or library tree, compiler, Homebrew, user home, provider or credentials. Five
absolute interpreter execution probes fail with `ENOENT`; native checks also
require absent stdlib/libpython paths. This is filesystem-isolation evidence,
not execution-trace or universal seccomp proof. Package testing uses its own
freshly built CLI/test pair, not the checkout pair.

Package checks require corpus/provenance/review/Rust helper and reject restoration
of the retired oracle. The 474-file archive contains 25,062,894 uncompressed file
bytes and 2,256,731 compressed bytes. Extracted CLI and library builds passed before
the isolated ten-test run.

Original-method accounting remains **941 requirements, 90 mapped / 851 unresolved**,
with zero new mapping credit. The ledger, original source pins, previous captures,
accepted reviews and **4 active / 1 retired / 5 historical** independent registry
remain unchanged. Ordinary audit passes; completion still fails solely for
unresolved obligations. No full native/Python suite, non-Unix SDK, release build,
installation, deployment, restart, staging, commit or push occurred. Owned capture,
build, producer, package and sandbox artifacts were cleaned after child completion.

See the [removal plan](../plans/PYTHON_REMOVAL.md) for remaining dependencies.
