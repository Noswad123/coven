# Kitsune Process Policy Reference

Offline development oracle for Kitsune commit
`01d7cef6e98e2ccab216cd669d569c0731abfbce`. This independent Cargo package
is not a production dependency. It acquires no processes, connects to no live
endpoints, and launches no providers, runtimes, shells, PTYs, UI or Windows APIs.
The build launches Git only; checks launch Cargo/compiler tooling only.

## Source Integrity

The build requires an explicit absolute `KITSUNE_REFERENCE_ROOT` and three
explicit absolute supplied source files. There is no sibling, HEAD, environment
default or network fallback. It resolves each pinned commit path to a hardcoded
Git blob ID and compares supplied bytes with `git show` for that object. HEAD
need not equal the commit: it is the pinned blobs, not checkout state, that count.
Files may be supplied separately but must exactly equal their pinned objects.
Git replacement objects are disabled during verification. These local integrity
pins do not authenticate the upstream repository or its author.

Exact, unique start/end markers and expected inclusive line ranges fail closed.
These unchanged contiguous blocks are written into `OUT_DIR` and compiled:

| Source | Inclusive Lines | Git Blob |
| --- | --- | --- |
| `src/detect/mod.rs` | 41-243, 329-614 | `7bbc2ca8113c76da70dcb17a007503811604de91` |
| `src/platform/mod.rs` | 6-19 | `5c168acb23d39efb43085d230c5221e2e753a882` |
| `src/platform/windows.rs` | 77-85, 292-415 | `7f1008326f68bd0da6d31346c3357bfdb2c284ed` |

The compiled selector includes its actual agent-aware descendant classification,
same-agent chain selection, traversal and conversion helpers. No classifier or
selector logic is copied into glue. Glue constructs the real structs and
serializes actual `identify_agent_in_job` and synthetic Windows selection results,
including a second identification of the selected job. Snapshot acquisition and
Windows command-line parsing are intentionally absent: supplied argv is synthetic.

The frozen JSON records full-file SHA256, pinned Git provenance, ranges, exact
boundary markers and block SHA256, but no compiled source bodies. Frozen tests
compare each block's hash and boundary metadata against reviewed constants and
verify the entire artifact SHA256. Source hashes are build-time attestations
anchored by the artifact digest; ordinary tests do not reread source.

Kitsune's Apache-2.0 source remains supplied from a separately licensed checkout,
not vendored in this MIT repository's fixture. The harness still builds unchanged
blocks in `OUT_DIR`; only their hashes and boundary metadata enter the fixture.

## Reproduce

Run from Coven's root with cached dependencies (the independent lockfile pins
resolution). Substitute explicit absolute reference paths if located elsewhere:

```sh
export KITSUNE_REFERENCE_ROOT=/home/jdawson/Projects/kitsune
export KITSUNE_REFERENCE_DETECT=/home/jdawson/Projects/kitsune/src/detect/mod.rs
export KITSUNE_REFERENCE_PLATFORM=/home/jdawson/Projects/kitsune/src/platform/mod.rs
export KITSUNE_REFERENCE_WINDOWS=/home/jdawson/Projects/kitsune/src/platform/windows.rs
export CARGO_TARGET_DIR=/tmp/opencode/kitsune-process-reference-target
cargo run --offline --locked --manifest-path tools/harnesses/kitsune-process-reference/Cargo.toml -- /home/jdawson/Projects/coven/tests/fixtures/kitsune_process_policy/reference.json
cargo run --offline --locked --manifest-path tools/harnesses/kitsune-process-reference/Cargo.toml -- --check /home/jdawson/Projects/coven/tests/fixtures/kitsune_process_policy/reference.json
bash tools/harnesses/kitsune-process-reference/check.sh /home/jdawson/Projects/coven/tests/fixtures/kitsune_process_policy/reference.json
```

`--check` independently captures again and compares bytes, not just parsed JSON.
`check.sh` also checks missing/relative root, missing/relative source and wrong
source bytes fail closed, then runs the harness's repeatability test and recapture.
No tools or dependency downloads are required. Ordinary frozen tests need none of
these environment variables, Git, sibling checkouts, this harness, source files,
the clock, a filesystem sandbox, or any runtime process:

```sh
cargo test --offline --locked --test kitsune_process_policy_reference
```

## Coverage And Limits

209 cases: empty/absent/plain root, all 22 providers at root and as descendants,
23 aliases, five suffixes, 31 runtime-wrapper cases, ten fallback cases, five
noninteractive mode cases and 15 tree scenarios with all 88 permutations.
Tree cases protect leader preference, scoring vs input-order ties, same-agent
chains through plain intermediaries, siblings, mixed nested agents, unrelated
nodes, missing parents and cycles. Unique PIDs are assumed; malformed duplicate
PID snapshots are not characterized.

Important observed behavior is preserved, not corrected: `bash -c codex` is
excluded but combined `bash -lc codex` is recognized. Cmdline fallback merely
splits whitespace and trims token quotes, so `"djinn ui"` can identify Djinn.
Direct Djinn help/version/serve/config/maintenance modes still identify Djinn;
executable family does not establish an interactive session or activity.

**Filesystem behavior is not pure.** The real classifier can canonicalize tokens.
Capture changes cwd to a fresh private `tempfile` directory, creates one inert
file and a relative symlink entirely inside it, and restores cwd before cleanup.
All multi-component fixture tokens are controlled relative paths under `sandbox/`;
other tokens are recognized basenames or single-component misses. The symlink
target is private, is never executable and is never launched. Fixture paths stay
relative and deterministic. There are no probes of real home/store/config paths.
Capture is single-threaded because cwd is process-global; the one harness unit
test performs two captures serially. Random sandbox names never enter output.

Coven's process observer has no public pure classification API reachable by an
integration test. Its private `exact_posix_argv0_basename` is deliberately not
reimplemented or exposed here. This is source characterization, not a claim that
Coven recognizes all these providers, wrappers or Windows selections. Production
source, the root Cargo files, docsroot and the resume harness are unchanged.
