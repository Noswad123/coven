# Workspace records and managed paths

Use this as a map of the file-backed workspace, not a replacement for the
[storage contract](STORAGE_CONTRACT.md) or the opt-in authority contracts it links.
For creation and day-to-day commands, see
[workspace basics](../guides/WORKSPACE_BASICS.md).

## Recognize authority before editing or recovering

Ordinary workspaces use snapshot-v1: `state/tasks.json` is the task authority.
Observational event/message logs are not a replay ledger, and `dashboard.md` is
regenerable. New workspaces declare `source_of_truth = "workspace-files"`;
older declarations mentioning events remain descriptive and do not change
authority. `state/.workspace.lock` is a persistent coordination inode, not state
or ownership truth. Never delete it to force-unlock a workspace.

Only explicit [task-ledger import](TASK_LEDGER_CONTRACT.md) changes task authority.
After activation, replayed ledger records are authoritative and `tasks.json` is a
projection. An unhealthy projection is diagnosed, not treated as authoritative
state. Corrupt active authority never silently falls back to snapshot-v1.
Init/refresh do not import or repair it.

Goal and other state retain their individual file authorities. Checkpoint Markdown
has its own [content-bound decision protocol](CHECKPOINT_CONTRACT.md), not event
replay. Optional [execution control](EXECUTION_INTENT_CONTRACT.md) records local
preparation/receipts, never execution permission. These are not one aggregate
workspace transaction.

## Keep managed paths safe

Checkpoint IDs and persisted roster IDs must be nonempty single components made
of ASCII letters, digits, underscores, or hyphens. Traversal, absolute paths,
whitespace, and nonstring IDs are rejected rather than sanitized. Safe legacy IDs,
including uppercase names and `_`, retain their spelling and identity.

Runner-managed control files/directories must not be symlinks, even to an internal
or nonexistent target. This includes metadata, state, logs, checkpoints, roster
notes, prompts, runtime manifests, and `work/README.md` / `output/README.md`.
A user-selected root alias resolves normally. Unrelated artifact links under
`work/` and `output/` are not recursively inspected or removed.

Checkpoint actions, refresh/bootstrap, and runtime batches preflight managed
targets before writes or runtime actions. Invalid checkpoint IDs in consumed
history, unsafe lead IDs, and unreadable checkpoint text cause errors rather than
partial approval/notification. Bootstrap reads every starter before creating
missing prompts or sending any. Fix reported input explicitly; unsafe history is
not automatically renamed or sanitized.

After ledger import, an unsafe/unreadable `tasks.json` projection is diagnosed
without following it. Authority and committed receipts remain readable, but new
changes are blocked; unsafe paths cannot be automatically repaired or overwritten.

These are static path/ID/readability checks, not hard-link protection, a multi-file
transaction, or a sandbox against concurrent filesystem replacement or agent tools.
Operation-specific contracts can impose additional checks.

## Understand the retained v1 record shapes

Commands and workspace-aware mutation/runtime helpers validate current metadata,
tasks, agent state, and lead before effects. Invalid JSON/encoding, required
fields, known-field types, and duplicate IDs produce source-specific errors. A
cached team object does not bypass validation; present invalid files need explicit
inspection/repair, not automatic replacement.

The format is **v1**, selected by `coven.json.version = 1`. Missing version means
legacy v1 without backfilling. Unsupported explicit versions are rejected. State
keeps its list/object shapes:

| File | Structural contract |
| --- | --- |
| `coven.json` | Object with nonblank `name` usable for runtime naming and a nonempty, unambiguous roster of safe IDs. Known runtime fields must be valid nonblank strings; optional `example` is a string or null. |
| `state/tasks.json` | List of objects with unique safe `id` and string `title`/`status`; optional `owner` is a safe ID or null; `notes` and timestamps, when present, are strings. |
| `state/agents.json` | List of objects with unique safe `id` and string `status`/`role`. |
| `state/lead.json` | Object with safe `agent` and optional string `set_at`/`until`/`instructions`, or JSON null. |

The task-file row describes snapshot-v1. After explicit ledger import, the same
task records are validated from replay, not from an unhealthy projection.

Absent task/agent files retain legacy empty-list defaults; absent/null lead means
no lead. **Present malformed files never become empty defaults.** Unknown extension
fields survive mutations. Existing ID spelling, arbitrary string statuses, and
opaque timestamp strings are retained. Historical owners/state agents need not
belong to today's roster. These structural checks do not enforce task lifecycle,
referential reconciliation, locking, or crash-consistent updates; use the storage
and task-authority contracts for those operation boundaries.

## Follow configuration and name conventions

`coven.json` holds workspace metadata and roster. `coven.toml` can override the
runtime scalars `multiplexer` and `opencode_command`, and its
`[runtime.agent_working_directories]` table replaces the entire JSON directory map.
Generated TOML agent/role/prompt/checkpoint sections remain descriptive, not live
customization surfaces. `COVEN_MULTIPLEXER` takes precedence for runtime selection.
See [directory isolation](../guides/AGENT_DIRECTORY_ISOLATION.md) for exact mapping,
starter-refresh and reviewed-launch v1 restrictions.

`tmux/coven.toml` is a generated optional-runtime manifest, not replacement metadata
authority. Both persisted JSON runtime fields and live TOML overrides are validated
before use: a valid override cannot hide malformed JSON. Init also parses both
generated TOML documents before creating files, including when display names
contain Unicode.

Optional `coven.json.display_names` maps exact IDs to nonblank labels. JSON is the
label authority; generated TOML labels are descriptive. Missing labels fall back
to IDs, and refresh does not backfill them. Labels may duplicate but are never
selectors. See [names versus identity](../guides/WORKSPACE_BASICS.md#keep-labels-separate-from-identity)
before changing metadata or choosing owner/lead/runtime selectors.

## Related files

- [Storage contract](STORAGE_CONTRACT.md): authority table, locks, commits, recovery.
- [Task-ledger contract](TASK_LEDGER_CONTRACT.md): activation and projection health.
- [Checkpoint contract](CHECKPOINT_CONTRACT.md): current local decision authority.
- [Record validators](../../runner/records.py) and [native workspace loading](../../src/workspace.rs).
