# Current Candidate Process Replay

Dev-only, standalone Cargo crate. This independently compares the **current
Kitsune working-tree source subset** with 209 frozen baseline records. It does
not generate expectations or claim the candidate is the pinned reference.
No root workspace dependency, production edits, reference harness edits, corpus
updates, or vendored raw Kitsune source are needed.

## Offline Commands

Run from `/home/jdawson/Projects/coven/tools/harnesses/kitsune-process-candidate`:

```sh
export KITSUNE_CANDIDATE_ROOT=/home/jdawson/Projects/kitsune
export KITSUNE_PROCESS_FIXTURE=/home/jdawson/Projects/coven/tests/fixtures/kitsune_process_policy/reference.json
cargo run --offline --locked
cargo test --offline --locked -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --offline --locked --all-targets -- -D warnings
```

Both input paths must be explicitly supplied and absolute. There is no sibling
checkout default. Cargo tracks the environment variables, fixture, and all five
source files, including `detect/resolver_tests.rs`, for rebuilds. The dependencies
are pinned cached `serde_json`/`tempfile`, with `sha2` used only at build time;
`Cargo.lock` freezes transitive resolution. No toolchain installation is performed.

## Provenance

The build rejects fixture bytes unless SHA-256 is
`8309440254a20d5dc747d5f1b1b32acb2acfb25586e37260fb57a968533e7e6d`.
It also checks 209 cases, reference schema/repository/harness, baseline commit
`01d7cef6e98e2ccab216cd669d569c0731abfbce`, and all three baseline source
paths, Git blob IDs, and full-file hashes. The fixture checksum checks integrity of
the remaining baseline metadata and expectations without needing a reference
checkout or reading Git objects.

Build outputs live only in Cargo's `OUT_DIR`. `provenance.json`, printed after a
successful replay, records the distinct baseline provenance, actual candidate
root, fixture path/hash, and each current candidate full-file SHA-256 plus
compiled block SHA-256, byte offsets (end exclusive), line ranges, and markers.
Candidate hashes are evidence of what ran, not pins required to equal baseline.

The build extracts unique, ordered, bounded source spans without rewriting:

- `detect/mod.rs`: baseline early Agent/classifier span, including the new
  `PathResolver`, canonicalization function, and resolver-based identification.
- `detect/mod.rs`: helper span starting at `#[cfg(test)]` immediately before
  `normalized_process_name`, ending before the test separator. Keeping the
  attribute is essential for the non-test build.
- `platform/mod.rs`: actual `ForegroundProcess` and `ForegroundJob` types.
- `platform/windows.rs`: actual two-argument `select_pane_foreground_job` adapter,
  stopping before `snapshot_processes`.
- `platform/process_selection.rs`: whole unchanged neutral module and its tests.
- `detect/resolver_tests.rs`: whole unchanged file, attached beneath `detect`
  under `cfg(test)` using `include!` from `OUT_DIR`.

The only generated attachment is test-module scaffolding. The scaffold permits
unused detect helpers and `clippy::collapsible_if` in that module to preserve the
candidate verbatim. No classifier stub, policy copy, test rewriting, or substitute
Windows adapter is used.

## Replay And Limits

Records are deserialized from the SHA-verified frozen fixture embedded at build
time. Every record runs actual `identify_agent_in_job` on the record's complete
job and the actual Windows adapter driving the actual neutral policy. The
complete selected-job projection, including all process fields, identification,
and exact candidate string, is compared as JSON values. All 209 records are
checked before reporting mismatches. Failure output contains only a bounded,
sanitized case name and expected/actual synthetic-data diffs, not source dumps.

Replay uses a private temporary cwd with one inert, non-executable
`sandbox/cursor-agent` file and an internal relative `sandbox/renamed` symlink,
matching baseline setup. Cwd is restored on normal return and unwinding before
the temporary directory is removed. Run tests with the documented single-thread
flag because cwd is process-global. The included source tests use injected
resolvers or lexical matches; they do not acquire processes or mutate cwd.
This is not a security sandbox for arbitrary untrusted candidate Rust code.

This is Linux execution of a source subset, **not a full Windows or application
build**. It covers no PTYs, live process acquisition, WMI, providers, acquisition
cache wiring, screen detection, or platform syscalls. It supplements rather than
bypasses the application checks: baseline `just test-one` / `just lint` remain
blocked by missing Zig, with only the Linux target available. No broader Coven
classification equivalence is claimed.

## Validation Results

Validated offline on the current candidate:

- Main: **209 / 209 cases passed**, both projections exact.
- Tests: **26 passed**: 1 fixture replay, 9 actual resolver tests, and 16 actual
  neutral-selection tests (10 moved tests plus 6 new tests).
- Formatting: `cargo fmt --all -- --check` passed.
- Clippy: `cargo clippy --offline --locked --all-targets -- -D warnings` passed
  with only the documented unchanged-source scaffold allowance above.

Full-file hashes observed during validation:

| Candidate File | SHA-256 |
| --- | --- |
| `src/detect/mod.rs` | `423c3d94185c9a217bc1a32d06c479f83e3635601b95d3e9baaf8443890d046d` |
| `src/platform/mod.rs` | `117785ac8bdec25fc3f8bdb2e99e074895777f6da0c9a663f61bbe8b5b5b6b83` |
| `src/platform/windows.rs` | `bcb68451911283038e55d972b7d460409e8205b209917227bad98fd7c71af102` |
| `src/platform/process_selection.rs` | `84693df2c3ccf6cb9229e6f71b15e43a54a2468ed6f737c5aea10a242be118b3` |
| `src/detect/resolver_tests.rs` | `de6518c198765112b4b910f7a7e8e3eb2e6511172800d32b6d553be63f944a22` |

These are a recorded observation, not enforced candidate pins. Rerun to obtain
fresh full-file and block hashes after candidate changes.
