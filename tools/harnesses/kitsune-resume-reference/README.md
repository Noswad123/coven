# Kitsune Resume Reference Capture

Development-only, independent Rust oracle. This package is not a Coven dependency
and does not launch providers, use sockets, or modify Kitsune. The build requires
explicit absolute reference root and source paths; there is no sibling fallback.
It verifies the commit's blob ID and compares the supplied module byte-for-byte
with that Git object before copying it unchanged into `OUT_DIR` and compiling it.
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
