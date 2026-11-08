# Coven examples

## Overview

`coven init` seeds inspectable workspace files. The product CLI is native Rust;
it supports blank, embedded `interactive-demo` and custom directory
examples through a relocatable, read-only `AssetBundle`, without Python fallback.
Python source is retained only as a development oracle, not an installed runtime.

## API / Interfaces

Native `init [WORKSPACE]` defaults to cwd and accepts `--name`, `--agents`,
`--opencode-command`, `--multiplexer {herdr,tmux}`, `--example`, `--blank` and
compatibility no-op `--force`. Selectors slug-normalize to a single example directory
name; empty/unknown selections refuse before effects. `--blank` overrides even an
invalid example and ignores invalid unused asset roots. Refresh with absent (`None`)
example metadata also ignores unused roots; present empty metadata is invalid.

Native asset root precedence:

1. `COVEN_EXAMPLES_DIR`: an absolute, authoritative examples root.
2. Otherwise `COVEN_HOME/examples`: the compatibility override, also absolute.
3. Otherwise, relative to the canonical executable's prefix, first existing
   `share/coven/examples`, then `libexec/examples`.
4. Only for an executable actually in a Cargo `target` tree: checkout `examples/`
   as a development fallback.

`COVEN_HOME` is an asset override, not executable selection. The native-only
checkout launcher preserves it and `COVEN_COMMAND` without exporting replacements.
`COVEN_COMMAND` controls generated commands; unset, generation uses canonical `coven`.
Installed `coven` is the direct Rust binary, not a wrapper.

An explicit override also selects the on-disk `interactive-demo`; missing/invalid
overrides do not silently fall back. Without explicit overrides, the built-in demo
uses embedded assets, not an implicitly discovered disk replacement. Root selection
still validates any discovered root. This is not cwd/PATH scanning.

## Example usage

Default example:

```bash
coven init ~/Projects/new-coven --example interactive-demo
```

Build explicitly, then use the normal checkout launcher:

```bash
cargo build --offline --locked --release --bin coven
bin/coven --help
bin/coven init /absolute/path/to/new-demo --example interactive-demo
bin/coven init /absolute/path/to/new-blank --blank --agents Architect,Reviewer
COVEN_EXAMPLES_DIR=/absolute/path/to/examples bin/coven init /absolute/path/to/new-custom --example custom-id
COVEN_NATIVE_BIN=/abs/prebuilt/coven /abs/checkout/bin/coven --help
```

## Data Flow

Each example is a directory with this shape:

```text
examples/<id>/
├── example.json        # name, description, seed message
├── goal.md            # copied to the generated workspace
├── spec.md            # copied to the generated workspace
├── prompt-context.md  # appended to each generated agent prompt
├── tasks.json         # seeded task ids, titles, owners, notes
├── work/README.md     # optional work directory guidance
└── output/README.md   # optional output directory guidance
```

All seven original demo files above remain embedded at native build time. The manifest
supplies seed metadata; goal/spec/guidance are copied, prompt context is appended
to role prompts, and tasks are seeded with roster-resolved owners (missing owners
become unassigned/pending). For the implicit demo, refresh uses the same embedded
prompt-context file; changing those embeds requires rebuilding. Custom examples are
read at runtime before effects. Refresh validates only their manifest and prompt
context, not unrelated seed files. Generated commands are data; init/refresh never
execute them.

Custom `example.json` must be an object. Missing seed files use default goal/spec/
work/output guidance, empty context and `[]` tasks. Missing or falsey `id` defaults
to the normalized directory selector; `seed_message` defaults to `Seeded example: ID`.
Truthy metadata uses finite Python-compatible string coercion. UTF-8 text
normalizes CRLF/CR to LF; goal gains a final LF when absent. Text files are bounded
to 1 MiB (including goal's added LF); manifest/tasks to 8 MiB with existing finite
JSON/value limits. Task/owner validation remains part of init preparation.

To create your own use case, copy
`interactive-demo/`, edit the Markdown and
`tasks.json`, then run:

```bash
COVEN_EXAMPLES_DIR=/absolute/path/to/examples bin/coven init /absolute/path/to/new-workspace --example CUSTOM_ID
```

Use `--blank` when you want only the workspace structure and no example seed.

## Edge Cases

The checkout launcher selects only absolute `COVEN_NATIVE_BIN` or the normal
checkout's prebuilt `target/release/coven`. Missing selection fails with build
guidance, never invoking Cargo, Python, debug builds or PATH fallback. Symlinked/
relocated launchers require explicit native selection. `/abs/...` examples are
placeholders, not automatically discovered paths.

Demo instructions mentioning `demo.py` describe an example deliverable, not a
Coven runtime dependency. Coven does not install Python for that deliverable.

Selected root leaf, example directory and managed asset paths are checked no-follow;
symlinks/wrong types refuse, including normalized trailing separators/dots and
double slashes. Ancestor aliases follow filesystem traversal order (including
symlink-parent/`..` behavior); trusted, stable parents are assumed. These bounded
checks are not a hostile namespace sandbox or blanket no-symlink-ancestor policy.
Native custom-example support does not approve all finite-profile/non-UTF-8
restrictions; explicit compatibility disposition remains a retirement gate.

Native init prepares the whole tree/events/dashboard/safe output before creation,
reserves state exclusively and publishes `coven.json` last, unlike Python's
config-first creation. Reserved names refuse regardless of type; `--force` never
overwrites, resets or adopts partial evidence. No authority activation or runtime
calls occur. Native/native reservation has one winner, but Python initial creation
is unlocked: mixed implementations may both fail without overwriting exclusive
finals. Initialize one destination with one implementation at a time.

Durability covers managed contents/root and only the ancestor chain observed
missing by that invocation, through its containing existing parent; existing
ancestor entries are assumed durable. If A creates the chain then pauses before
reservation, B can see an existing root, win and commit after syncing managed
contents and root but not outer entries, while A loses before chain sync: neither
necessarily fsyncs the outer entries. Native exclusion is not cross-initializer
ancestry durability.
Durably establish destination ancestry first or initialize with one process when
stronger assurance is needed. The deterministic unit pins scope, not crash loss.
See the migration notes for caps, modes, partial diagnostics and exit 0/1/2 rules;
validation is Linux-only with no hostile namespace guarantee.

### Completed locally vs External checklist

An agent verified the extracted source-package release build and staged native
install's help, blank/demo init and refresh with cleared environment/empty PATH.
Independent entrypoint (5), package (2) and install (9) tests pass and cover runtime
guards. Root release/native help, strict all-target Clippy and Rust 1.85.0 all-target
check pass after the two MSRV fixes, without production-policy changes.
[Final local acceptance](../RUST_MIGRATION.md#final-local-acceptance) records **1,225
native passes via split validation**, not one fresh all-command green run, plus
the standalone **941 Python tests in 191.978s** and **22 tool tests in 156.071s**.
Ready for local review and manual native use within documented profiles. macOS and actual Homebrew
install/upgrade have not run, and the formula remains an unpromoted candidate.
Native production cutover does not close non-UTF-8/finite-cap policy or authorize
source-oracle deletion, source push or merge. The full 941-test Python-oracle suite
requires the checkout and development Python tooling, not the installed product
or a standalone Cargo source package. The 67 mapped / 874 unresolved source-audit
IDs gate oracle-removal cleanup, not 874 product features.
No changes have been committed, pushed or merged.

## Related files

- [../RUST_MIGRATION.md](../RUST_MIGRATION.md#native-workspace-init): native API,
  publication, durability limits and validation status.
- `src/init.rs`, `src/storage/bootstrap.rs`, `src/platform/unix/init.rs`: native
  preparation, exclusive reservation and root resolution.
- `src/workspace_assets.rs`, `src/workspace_assets/init.rs`: pure templates and embeds.
- `src/example_assets.rs`, `src/example_assets/tests.rs`: root selection and bounded custom loading.
- `tests/example_assets_workflow.rs`, `tests/native_install.rs`: independent asset and staged relocation coverage.
- `tests/init.rs`, `tests/_init_parity.py`: CLI parity and explicit native deviations.
- `runner/coven.py`: retained development-only Python directory-loading oracle.
- [../packaging/README.md](../packaging/README.md): source vs installed payload and
  staged install into a new prefix whose trusted parent already exists.
