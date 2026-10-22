# coven

`coven` is a personal, magic-themed multi-agent workspace command for creating,
tracking, and running local multi-agent workspaces.

The source of truth for a coven workspace is local, inspectable files. The
command creates those files, keeps lightweight state, and launches agent panes
with Herdr by default. Tmux remains available as an opt-in runtime.

## Install

Homebrew tap install:

```bash
brew tap Noswad123/jamal-arcana
brew install coven
```

Or install directly from the tap:

```bash
brew install Noswad123/jamal-arcana/coven
```

The tap formula currently tracks `https://github.com/Noswad123/coven` on the
`main` branch. Publish this repository there before using the tap from another
machine. After the first tagged release, update the formula to a tag tarball and
SHA like the other Jamal Arcana tools.

The Homebrew formula installs `bin/coven`, `runner/`, and `examples/` together
under Homebrew's Cellar so the wrapper can find the Python runner and example
seeds.

Local development can still use the repository wrapper directly:

```bash
/Users/jdawson/Projects/coven/bin/coven --help
```

## Quick start

```bash
/Users/jdawson/Projects/coven/bin/coven init ~/Projects/new-coven
cd ~/Projects/new-coven
/Users/jdawson/Projects/coven/bin/coven plan
/Users/jdawson/Projects/coven/bin/coven up
```

For a shorter command, add this project's `bin/` directory to `PATH` or symlink
`bin/coven` into `~/.local/bin`.

## Project layout

This repository currently uses a flat standalone layout:

```text
coven/
├── bin/
│   └── coven              # executable wrapper
├── runner/
│   ├── coven.py           # main CLI and workspace runner
│   └── records.py         # pure v1 workspace record validators
├── examples/
│   ├── README.md          # example authoring notes
│   └── interactive-demo/  # default seeded workspace example
├── ROADMAP.md             # orchestration roadmap and Coven/Djinn boundary
└── README.md
```

There is no `tools/coven/` tree in this project.

## Roadmap

See [`ROADMAP.md`](./ROADMAP.md) for the lead-agent orchestration roadmap,
including the intended Coven/Djinn split: Coven owns decomposition, scheduling,
worker lifecycle projection, and synthesis; Djinn owns Djinn-native worker
execution, policy, tools, and transcripts.

See [`CODING_ORCHESTRATION_ROADMAP.md`](./CODING_ORCHESTRATION_ROADMAP.md) for
the prioritized safety repairs and the staged worker/composer coding workflow.

## Generated workspace layout

`coven init` creates a file-backed workspace with this shape:

```text
new-coven/
├── coven.json                  # workspace metadata
├── coven.toml                  # coven agents/checkpoints/runtime config
├── goal.md                     # shared goal
├── spec.md                     # example/spec notes when seeded
├── dashboard.md                # human-readable state projection
├── agents/                     # per-agent role and notes files
├── prompts/                    # generated agent prompts and starter prompts
├── state/
│   ├── agents.json
│   ├── tasks.json
│   └── lead.json               # present when a lead is set
├── logs/
│   ├── events.jsonl            # append-only event log
│   └── messages.jsonl          # append-only message log
├── checkpoints/
│   ├── plan-approval.md
│   └── pre-delivery-review.md
├── work/                       # in-progress artifacts
├── output/                     # final/demo artifacts
└── tmux/
    └── coven.toml              # generated tmux workspace manifest for optional tmux runtime
```

## Examples

By default, `init` seeds the `interactive-demo` example:

```bash
coven init ~/Projects/new-coven --example interactive-demo
```

Create an empty workspace instead:

```bash
coven init ~/Projects/new-coven --blank --agents orchestrator,architect,implementer,reviewer
```

### Initialization safety

`init` creates a workspace; it does not reset or adopt an existing one. It refuses
any existing `coven.json`, `coven.toml`, `goal.md`, `spec.md`, `dashboard.md`, or
reserved directory (`agents`, `prompts`, `state`, `logs`, `checkpoints`, `work`,
`output`, `tmux`). Even empty reserved directories and dangling symlinks are
rejected. Unrelated project files are allowed and left untouched.

The deprecated `--force` flag remains accepted for compatibility but **does not
permit overwriting**. Use `coven refresh` to regenerate an existing workspace's
generated files, or choose a fresh directory. No destructive reset command is
provided. Seed validation happens before creation; an I/O failure during creation
leaves partial files for inspection rather than deleting them or retrying a reset.
Initialization is not a multi-file transaction or a filesystem sandbox.

New agent IDs are normalized to lowercase slugs; duplicate normalized IDs are
rejected. Example task owners and `task add --owner` / `task assign` names resolve
to the roster's IDs (for example, `Jamal` resolves to `jamal`). Explicit task-owner
commands reject unknown agents. Example owners omitted from a custom roster leave
their tasks unassigned.

Starter examples live under `examples/`. See `examples/README.md` for the
example directory format.

### Managed path safety

Checkpoint IDs and persisted roster IDs must be nonempty single components using
ASCII letters, digits, underscores, or hyphens. Coven rejects traversal, absolute
paths, whitespace, and nonstring IDs instead of silently renaming them. Safe
legacy roster IDs, including uppercase names and `_`, retain their identity.

Runner-managed control files and directories must not be symlinks, even if the
link points inside the workspace or its target does not exist. This covers
metadata, state, logs, checkpoints, roster notes, prompts, runtime manifests, and
the `work/README.md` / `output/README.md` guidance files. A user-selected workspace
root alias is resolved normally; unrelated artifact links under `work/` and
`output/` are not recursively inspected or removed.

Checkpoint actions, refresh/bootstrap, and runtime batch operations preflight
their managed targets before writes or runtime actions. Invalid checkpoint IDs
in consumed log records, unsafe lead IDs, and unreadable checkpoint text cause a
clean error rather than partial approval or notification. Bootstrap reads all
starter prompts before creating missing prompts or sending any of them. Fix the
reported input explicitly; Coven does not sanitize unsafe history automatically.

These are static path/ID/readability checks plus the structural contracts below,
not multi-file transactions, hard-link protection, or a sandbox against concurrent
filesystem replacement or tools run by an agent.

### Persisted record contracts

Commands and workspace-aware mutation/runtime helpers validate the current
workspace metadata, tasks, agent state, and lead before effects. Invalid JSON,
encoding, required fields, known-field types, or duplicate IDs produce a clear
error identifying the source record. A cached team object does not bypass this
validation. Invalid current files must be inspected and repaired explicitly;
Coven does not overwrite them to recover automatically.

The current format is **v1**, selected by `coven.json.version = 1`. Missing version
means legacy v1 without backfilling; unsupported explicit versions are rejected.
State files keep their existing list/object shapes:

| File | Structural contract |
| --- | --- |
| `coven.json` | Object with nonblank `name` usable for runtime naming and a nonempty, unambiguous roster of safe IDs. Known runtime fields must be valid nonblank strings; optional `example` is a string or null. |
| `state/tasks.json` | List of objects with unique safe `id` and string `title`/`status`; optional `owner` is a safe ID or null; `notes` and timestamps, when present, are strings. |
| `state/agents.json` | List of objects with unique safe `id` and string `status`/`role`. |
| `state/lead.json` | Object with safe `agent` and optional string `set_at`/`until`/`instructions`, or JSON null. |

Absent task/agent files retain the legacy empty-list default; absent/null lead
means no lead. Present malformed files never become empty defaults. Unknown
extension fields survive mutations. Existing ID spelling, arbitrary string
statuses, and opaque timestamp strings are retained; old owners or state agents
need not belong to today's roster. Task transitions, referential reconciliation,
locking, and crash-consistent updates are not enforced by these structural checks.

Both persisted JSON runtime fields and live TOML overrides are checked before
use; a valid override cannot hide a malformed JSON field. Initialization also
parses both generated TOML documents before creating files, including Unicode
display names.

### Agent display names versus identity

New workspaces retain trimmed `--agents` input as display names while deriving
stable IDs as before: `--agents "Alice 🚀,Dr Oom"` creates IDs `alice,dr-oom`.
Optional `coven.json.display_names` maps exact IDs to nonblank labels, for example:

```json
{"display_names": {"jamal": "Jamal — Senior Developer", "eric": "QA"}}
```

Add that field to existing metadata rather than replacing the whole file. Legacy
workspaces without labels display their IDs, and refresh never backfills names.
Labels appear alongside IDs in dashboard/standup and agent/lead status, with
Markdown/control characters escaped for the surface. Labels may be duplicated,
but are **not selectors**: owners, lead records, logs, filenames, tmux targets,
and native routing always use IDs. Case/normalization-ambiguous ID rosters are
rejected, not silently renamed. Generated TOML display names are descriptive;
JSON remains their authority.

## Common commands

```bash
coven status       # render and print dashboard.md
coven next         # show the recommended next user action
coven standup      # summarize agents, tasks, checkpoints, and lead
coven events       # print event log, or use --raw for JSONL
coven messages     # print message log, or use --raw for JSONL
coven refresh      # regenerate generated prompts and runtime manifests
coven bootstrap    # paste starter prompts into running agent panes
```

### Refresh preservation

`coven refresh` only regenerates `dashboard.md`, `tmux/coven.toml`, and
`prompts/<agent>.start.txt`, and creates missing `prompts/<agent>.md` files.
Existing role prompts are preserved; starter prompts and the manifest are
generated outputs and should not hold irreplaceable notes.

Refresh leaves task/agent/lead state, metadata/config, checkpoint decisions,
roster notes, both logs, and work/output artifacts unchanged. It does not append
a refresh event, rename legacy task IDs, restore missing guidance/checkpoint
files, or recover approval from historical events. Initialization, refresh, and
future explicit migration/recovery are separate operations; no migration command
is supplied yet. Refresh remains subject to the managed-path validation above.

## User interaction

```bash
coven review
coven console
coven approve plan-approval
coven suggest pre-delivery-review "Please include QA evidence before final approval."
coven send jamal "Please continue core-chatbot and update the logs."
coven send all "Pause and summarize current status."
```

`coven review` walks pending checkpoint files and lets you approve, request
changes, skip, or quit. `coven console` is a small interactive shell for status,
standup, agents, send, approve, suggest, and review operations.

### Current checkpoint decisions

The current `Status:` metadata before the first real H2 heading in a checkpoint
file is authoritative for its displayed decision. `suggest` sets it to
`changes_requested`, unchecks the current approval, and checks “Rejected / needs
changes.” Explicit approval reverses those checkboxes. Prior rationale and
feedback stay in the file; historical approval events do not grant current
approval or revive it during refresh. `next` discovers requests in both logs and
checks current file status, not whether approval happened at some point.

Decision edits are limited to metadata and the direct checklist under the live
`## User decision` section. Fenced examples and nested historical sections are
preserved. Ambiguous layouts (duplicate current fields/sections/checklist labels
or unclosed code fences) are rejected before saving or notifying agents. This
also applies to newly submitted notes: close code fences and quote/fence example
decision sections rather than introducing another live H2 decision heading.
Existing contradictory legacy prose is not silently repaired; make an explicit
decision after inspecting it.

These decisions are not revision-bound execution grants. Dispatch enforcement,
concurrent writes, and crash-consistent decision/log updates remain future work.

## Runtime lifecycle

`coven` launches and manages a Herdr workspace for the generated coven by
default. Agent panes run `oc` when it is available and automatically fall back to
`opencode` otherwise. Override this with `coven init --opencode-command ...`.

```bash
coven plan
coven up
coven up --replace
coven down
coven restart

# aliases
coven start
coven stop
```

To use tmux instead of Herdr for a workspace, initialize with tmux:

```bash
coven init ~/Projects/new-coven --multiplexer tmux
```

Or edit `coven.toml` later:

```toml
[runtime]
multiplexer = "tmux" # or "herdr"
```

Configuration authority today: `coven.json` holds the workspace name and roster.
Only `[runtime].multiplexer` and `[runtime].opencode_command` in `coven.toml`
override the corresponding JSON values; `COVEN_MULTIPLEXER` takes precedence over
both for runtime selection. Generated TOML agent/role/prompt and checkpoint
sections are descriptive, not live customization. This scaffold does not yet
rebuild workspace state from the event log.

Target a specific agent window:

```bash
coven agent status jamal
coven agent send jamal "Please summarize your blocker."
coven agent stop jamal
coven agent start jamal
coven agent restart jamal
```

## Coven lead

Set one agent to lead until the goal is achieved or a critical blocker requires
the user:

```bash
coven lead srikanth
coven lead
coven lead clear
```

The lead is recorded in `state/lead.json`, shown in the dashboard/standup,
logged, and sent to the lead's runtime pane when it is running.

## Development checks

Python 3.11+ is required (the runner uses `tomllib`). Run the offline regression
suite from the repository root:

```bash
python3 -m unittest discover -s tests -v
bash -n bin/coven
git diff --check
```

Tests use disposable workspaces, exercise the real CLI wrapper, and do not start
Herdr, tmux, OpenCode, or model sessions.

## License

MIT © Jamal Dawson
