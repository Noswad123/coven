# Kitsune Resume Reference Capture

Development-only, independent Rust oracle. This package is not a Coven dependency
and does not launch providers, use sockets, or modify Kitsune. The build requires
explicit absolute reference root and source paths; there is no sibling fallback.
It verifies the commit's blob ID and compares the supplied module byte-for-byte
with that Git object before copying it unchanged into `OUT_DIR` and compiling it.
Verification disables Git replacement objects; the pins establish local source
integrity, not upstream or author authentication.
The generator executes that module's constructors and planner, including direct
struct construction to expose validation bypasses. It contains no argv mappings.

From the Coven repository root, capture offline using cached pinned dependencies:

```sh
KITSUNE_REFERENCE_ROOT=/home/jdawson/Projects/kitsune KITSUNE_REFERENCE_SOURCE=/home/jdawson/Projects/kitsune/src/agent_resume.rs CARGO_TARGET_DIR=/tmp/opencode/kitsune-resume-reference-target cargo run --offline --locked --manifest-path tools/harnesses/kitsune-resume-reference/Cargo.toml -- tests/fixtures/kitsune_resume/reference.json
cargo test --offline --test session_resume_reference
```

The committed lockfile pins the offline resolution. Build outputs belong outside
the checkout (the local `target/` is also ignored). The capture is deterministic
and stores no machine paths or timestamps. Its Unix path semantics match Coven.

The matrix covers the 16 official sources crossed with all 16 agents, extended
with unknown/custom source and agent labels, for both ID and path selectors.
Additional boundary/Unicode/control/literal-input cases run against every official
pair plus unknown and custom pairs. `raw_argv` comes from direct construction;
`validated_argv` comes from the real constructor followed by the real planner.
The disposition is Coven's explicit policy delta: reject failed constructors,
blank selectors, and selectors whose trimmed beginning is a hyphen, even for
unknown pairs. Reference-valid selectors otherwise require exact argv parity or
the same unsupported result. Frozen tests need neither Kitsune nor this harness.

## Native Policy Capture

The separate `--native-policy` mode calls the actual `session_ref_from_report`,
`session_ref_from_snapshot`, `is_reserved_native_state_source`,
`normalize_session_start_source`, and `dedupe_key` in that same whole included
module. It does not apply Coven's hardened selectors before calling the reference.
Each record has `name`, `operation`, `input`, and `output`; JSON `null` preserves
`None`, including optional report ID/path inputs. Snapshot successes retain the
persisted source, agent, and session reference. Dedupe inputs deliberately use raw
struct construction, not a validating constructor.

From the Coven root, with the explicit unchanged reference checkout already
available and the lockfile's dependencies cached, run these offline commands:

```sh
export KITSUNE_REFERENCE_ROOT=/home/jdawson/Projects/kitsune
export KITSUNE_REFERENCE_SOURCE=/home/jdawson/Projects/kitsune/src/agent_resume.rs
export CARGO_TARGET_DIR=/tmp/opencode/kitsune-resume-reference-target
cargo run --offline --locked --manifest-path tools/harnesses/kitsune-resume-reference/Cargo.toml -- --native-policy /tmp/opencode/kitsune-native-reference-first.json
cargo run --offline --locked --manifest-path tools/harnesses/kitsune-resume-reference/Cargo.toml -- --native-policy /tmp/opencode/kitsune-native-reference-second.json
cmp /tmp/opencode/kitsune-native-reference-first.json /tmp/opencode/kitsune-native-reference-second.json
cmp /tmp/opencode/kitsune-native-reference-first.json tests/fixtures/kitsune_native_reference/reference.json
cargo run --offline --locked --manifest-path tools/harnesses/kitsune-resume-reference/Cargo.toml -- /tmp/opencode/kitsune-argv-reference.json
cmp /tmp/opencode/kitsune-argv-reference.json tests/fixtures/kitsune_resume/reference.json
sha256sum tests/fixtures/kitsune_native_reference/reference.json tests/fixtures/kitsune_resume/reference.json
cargo test --offline --locked --manifest-path tools/harnesses/kitsune-resume-reference/Cargo.toml
CARGO_TARGET_DIR=/tmp/opencode/coven-reference-policy-target cargo test --offline --locked --test session_resume_reference --test kitsune_native_reference_policy
```

For an intentional, reviewed recapture, pass
`tests/fixtures/kitsune_native_reference/reference.json` as the policy output path,
then update its integration test's SHA only after reviewing the new corpus. No
command above fetches or mutates Kitsune. The build pins commit
`01d7cef6e98e2ccab216cd669d569c0731abfbce`, blob
`0c3b51bd19d73aa0ede31d6775cac35395447273`. The argv corpus remains 1,512
cases with SHA-256
`ddfcbcef2aad3e6e7f46634d7c50afa64ae4f8fe351837bd4f6cdc1aba4ef9fe`.
The policy corpus has SHA-256
`c18f0b9f2871088dfdd0e9aa1ad2987c350333175c7ade72bca4e55e8e186712`.

The 5,282 policy records comprise 3,216 reports, 1,608 snapshots, 420 reservations,
30 startup normalizations, and 8 dedupe keys. Coverage includes all 16 official
sources crossed with all 16 providers, plus custom/unknown rows and columns
(18 x 18), six source/provider whitespace/case variants per official pair, and
24 edge values per kind per official pair. Edges exercise 512/4096-byte limits,
multibyte Unicode, controls, blanks, leading hyphens, and literal path components.

Important reference behavior, not Coven policy:

- Reports prefer valid pi/omp paths, even with invalid IDs; invalid paths fall
  back to valid IDs. ID-only providers ignore paths, including valid paths.
- Snapshots obey the explicit kind: only pi/omp accept paths, and invalid paths
  return `None` without switching to an ID. Paths are not canonicalized.
- Blank and leading-hyphen IDs are reference-valid; empty, over-byte-limit, and
  control-containing IDs are not. No shell or filesystem operations are executed.
- Reservation is exactly eight source/provider pairs, not all 16 official pairs.
  Source/provider matching is exact, without trimming or case folding.
- Startup normalization accepts only `startup`, `resume`, `clear`, `compact`,
  `new`, and `fork`, after Unicode trimming but without case folding.
- Dedupe keys separate source, agent, kind, and literal value using NULs. Raw
  custom/unknown and control-containing inputs still produce keys. This is a
  reference-only characterization, not validation, authorization, or a production
  API; arbitrary NUL-containing fields must not be assumed collision-resistant.

This is deliberately not a lifecycle simulator. `TerminalState` ownership,
sequence handling, release, restore, and provider launch are excluded. There are
no live sockets, providers, reference source mutations, or production integration.
