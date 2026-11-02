# Coven examples

## Overview

`coven init` seeds inspectable workspace files. The Python default CLI can load
example directories; the Rust preview implements complete native init for the
embedded `interactive-demo` and blank profiles only, not SDK-only setup.

## API / Interfaces

Native `init [WORKSPACE]` defaults to cwd and accepts `--name`, `--agents`,
`--opencode-command`, `--multiplexer {herdr,tmux}`, `--example`, `--blank` and
compatibility no-op `--force`. Only aliases slug-normalizing to `interactive-demo`
select a native seed. Empty/custom selections return
`unsupported_init_example_profile` before effects; `--blank` overrides even an
invalid example. There is no dynamic example search or Python fallback.

## Example usage

Default example:

```bash
coven init ~/Projects/new-coven --example interactive-demo
```

Native preview, without changing the installed/default command:

```bash
target/debug/coven init /absolute/path/to/new-demo --example interactive-demo
target/debug/coven init /absolute/path/to/new-blank --blank --agents Architect,Reviewer
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

All seven original files above are embedded at native build time. The manifest
supplies seed metadata; goal/spec/guidance are copied, prompt context is appended
to role prompts, and tasks are seeded with roster-resolved owners (missing owners
become unassigned/pending). Native refresh embeds the same original prompt-context
file, not a separate copied asset. Editing these sources requires rebuilding Rust;
generated commands are data and init/refresh never execute them.

For the **Python default CLI only**, to create your own use case, copy
`interactive-demo/`, edit the Markdown and
`tasks.json`, then run:

```bash
coven init ~/Projects/my-coven --example <id>
```

Use `--blank` when you want only the workspace structure and no example seed.

## Edge Cases

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

## Related files

- [../RUST_MIGRATION.md](../RUST_MIGRATION.md#native-workspace-init): native API,
  publication, durability limits and validation status.
- `src/init.rs`, `src/storage/bootstrap.rs`, `src/platform/unix/init.rs`: native
  preparation, exclusive reservation and root resolution.
- `src/workspace_assets.rs`, `src/workspace_assets/init.rs`: pure templates and embeds.
- `tests/init.rs`, `tests/_init_parity.py`: CLI parity and explicit native deviations.
- `runner/coven.py`: retained Python directory-loading oracle/default.
