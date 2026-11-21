# Workspace basics

A Coven workspace is a local file-backed coordination area: a shared goal,
agent roster, tasks, messages, checkpoints, and generated prompts/dashboard.
Creating or refreshing one does not launch agents or execute generated commands.
Task-ledger import and execution preparation are separate, explicit opt-ins.

## Create a workspace without launching anything

Commands using `target/debug/coven` in this guide assume the repository root as
the working directory and an already-built native binary. Replace the absolute
example path with a fresh destination. No build or runtime command is needed in
this workflow:

```bash
target/debug/coven init /absolute/path/to/new-review --blank --agents operator,reviewer
target/debug/coven goal /absolute/path/to/new-review "Review parser edge cases"
target/debug/coven goal /absolute/path/to/new-review
target/debug/coven message /absolute/path/to/new-review operator reviewer "Please review the parser"
target/debug/coven events /absolute/path/to/new-review --pretty
target/debug/coven messages /absolute/path/to/new-review --raw
target/debug/coven status /absolute/path/to/new-review
```

`--blank` creates no seeded tasks or `spec.md`. `message` appends local history;
it does **not** notify a runtime. `status` is a **write**, not a read-only probe:
it republishes `dashboard.md` and may create the persistent coordination lock.
For read-only task/authority inspection, use `task list --json` or
`state inspect --json`; see [task workflows](TASK_WORKFLOWS.md).

Installed `coven` exposes the same routes. With no workspace argument, commands
normally use the current directory. For `goal` and `message`, the first item is
treated as a workspace only if its expanded path has a `coven.json` marker;
otherwise it remains part of the command text. Use a successfully initialized,
explicit path when following the examples.

## Choose blank or example content

```bash
target/debug/coven init /absolute/path/to/new-demo --example interactive-demo
target/debug/coven init /absolute/path/to/blank-team --blank \
  --name "Review team" --agents "Architect,Reviewer" \
  --opencode-command opencode --multiplexer tmux
```

The optional init path defaults to the current directory. Defaults are agents
`Clippy,Srikanth,Jamal,Kris,Eric`, command `oc`, multiplexer `herdr`, and example
`interactive-demo`. The name defaults to the resolved root basename, including
when `--name` is empty. Multiplexer choices are `herdr` and `tmux`; choosing one
records configuration, not a launch request.

The demo's seven original assets are embedded. Custom example directories and
explicit roots follow the [example asset-root policy](../../examples/README.md);
explicit roots override the demo too. There is no Python fallback. `--blank`
overrides even an invalid example and ignores unused asset roots.

New agent IDs are normalized lowercase slugs. Duplicate normalized IDs are
rejected. Example owners resolve to roster IDs; a seeded owner missing from a
custom roster leaves the task unassigned. Explicit `task add --owner` and
`task assign` reject unknown agents rather than silently leaving them unassigned.

## Know the generated layout

```text
new-coven/
├── coven.json                  # workspace metadata
├── coven.toml                  # agent/checkpoint/runtime configuration
├── goal.md                     # shared goal
├── spec.md                     # seeded example/spec notes, when present
├── dashboard.md                # regenerable human-readable projection
├── agents/                     # roster role and notes files
├── prompts/                    # role prompts and generated starter prompts
├── state/
│   ├── agents.json
│   ├── tasks.json
│   ├── .workspace.lock          # persistent coordination inode; never force-delete
│   └── lead.json               # present when a lead is set
├── logs/
│   ├── events.jsonl             # observational append-only history
│   └── messages.jsonl           # observational append-only history
├── checkpoints/
│   ├── plan-approval.md
│   └── pre-delivery-review.md
├── work/                       # in-progress artifacts
├── output/                     # final/demo artifacts
└── tmux/
    └── coven.toml              # generated optional tmux runtime manifest
```

This is the initial layout, not a requirement to create optional authority
files manually. See [workspace records](../architecture/WORKSPACE_RECORDS.md)
for managed-path rules, record defaults, and authority conventions.

## Initialize safely; do not reset with init

Init refuses existing `coven.json`, `coven.toml`, `goal.md`, `spec.md`,
`dashboard.md`, or any reserved directory: `agents`, `prompts`, `state`, `logs`,
`checkpoints`, `work`, `output`, `tmux`. Even empty reserved directories and
dangling symlinks are rejected. Unrelated project files are allowed and untouched.

The deprecated `--force` option is accepted but **never permits overwrite**.
Choose a fresh destination, or use `refresh` for an existing workspace's generated
assets. Init does not adopt partial files, reset, roll back, automatically resume,
or steal a stale reservation. Seed validation precedes creation; an I/O failure
during creation leaves partial evidence for inspection.

Native init prepares the complete tree, events, dashboard, generated TOML and safe
informational output before effects. It exclusively reserves `state/`, holds a
private persistent flock, and publishes `coven.json` **last**. Ordinary workspace
guards still require a valid marker. Native/native init has one reservation
winner, but Python initial creation is unlocked: mixed initializers can both fail
and leave partial evidence. Exclusive final-file creation prevents overwrite, not
mixed-initializer serialization. Use one implementation/process per destination.

Durability is operation-scoped, not cross-initializer ancestry durability. Native
init syncs files, managed directories, root, and the ancestor chain **observed
missing by that invocation**, through its containing existing parent. It assumes
already-existing ancestor entries are durable. If one initializer creates the
chain and pauses, another can see it as existing, win reservation, and sync only
its own scope; the loser may stop before syncing those outer entries. Neither
necessarily syncs that ancestry. Durably establish ancestry first or initialize
with one process when stronger assurance is needed.

Success exits 0. Pre-marker I/O failure exits 1 and preserves partial files with
bounded root/phase/current-target/last-completed diagnostics. Deterministic
preflight refusals create no root. Known marker publication followed by root-sync
failure exits 2 with informational stdout retained; inspect before retrying.
Parser errors can also exit 2, and stdout failure can exit 1 after effects.
This is not a multi-file transaction, filesystem sandbox, or universal power-loss
guarantee. Native validation is Linux-only; static/cooperating namespace limits
and bounded profiles are detailed in the
[native init notes](../plans/RUST_MIGRATION.md#native-workspace-init).

## Use local goal, message, and history commands

`goal` joins words with spaces and strips the result. Empty/whitespace-only text
shows the **full** goal; it is not a clear operation. Goal show and history reads
create no lock, fsync nothing, and mutate nothing.

`message` requires sender, recipient, and body. Parties are free-form text, not
roster selectors. The joined body's whitespace is preserved rather than stripped.
Message append, event append, and dashboard publication are separate effects.
Message/event bytes may already be visible when the command exits 1; there is no
receipt, deduplication, rollback, or safe automatic retry. Inspect before repeating.

Goal primary sync uncertainty also exits 1, unlike task/state authority uncertainty
(exit 2). Confirmed goal replacement with secondary history/dashboard warnings
still exits 0. Status writes only the dashboard, apart from possible lock creation,
then releases the lock and rereads it; another writer may intervene before output.
Failed publication, reread, or stdout can produce exit 1 after visible effects.

Pretty history validates the entire selected log; malformed/nonobject/truncated
records require explicit inspection and repair. `--raw` reads UTF-8 text without
JSON/record validation, but does not bypass workspace/path preflight. `--raw` wins
over `--pretty`. These local families have no `--json` option.

**Terminal warning:** goal, message/history, and status preserve literal Unicode
and control text, including pretty output, not only `--raw`. ANSI, bidi, and other
controls can render in the terminal. Task human/report escaping is unchanged.
See the [local command notes](../plans/RUST_MIGRATION.md#native-local-workspace-commands)
for exact parsing, output, and bounded-input behavior.

## Keep labels separate from identity

`--agents "Alice 🚀,Dr Oom"` retains trimmed display names while deriving IDs
`alice,dr-oom`. Existing metadata may add a label map without replacing the file:

```json
{"display_names": {"jamal": "Jamal — Senior Developer", "eric": "QA"}}
```

Legacy workspaces without labels display IDs; refresh does not backfill names.
Labels accompany IDs on dashboard/standup and agent/lead status, escaped for the
surface. Labels may duplicate, but are **not selectors**. Owners, lead records,
logs, filenames, tmux targets, and native routing use IDs. Ambiguous normalized or
case-ambiguous rosters are rejected, not renamed. Generated TOML labels are
descriptive; JSON remains their authority.

## Refresh generated assets without changing authored work

From inside an initialized workspace, these installed-CLI commands are useful:

```bash
coven status       # render and print dashboard.md; writes
coven next         # recommend a next user action; does not approve or notify
coven standup      # summarize agents, tasks, checkpoints, and lead
coven events       # pretty event history; --raw prints JSONL text
coven messages     # pretty message history; --raw prints JSONL text
coven refresh      # regenerate assets; no command execution
coven bootstrap    # runtime boundary: paste starters into running agent panes
```

Refresh regenerates only `dashboard.md`, `tmux/coven.toml`, and
`prompts/<agent>.start.txt`, and creates missing `prompts/<agent>.md`. Existing
role prompts survive; generated starters/manifest must not hold irreplaceable
notes. Generated commands default to canonical `coven`, not a checkout wrapper.

Refresh preserves task/agent/lead state, metadata/config, checkpoint decisions,
roster notes, both logs, and work/output artifacts. It appends no refresh event,
renames no legacy task IDs, restores no missing guidance/checkpoint files, and
recovers no approval from events. It never activates or repairs task authority.
Blank, embedded-demo, and custom-example contexts are supported.

Managed targets are preflighted, but manifest/starter writes may have partial
effects; the atomic dashboard is published last. Inspect failures rather than
assuming rollback. A first valid mutation/publication may create
`state/.workspace.lock`; this is coordination, not a state migration. Refresh and
bootstrap are distinct: only bootstrap crosses into runtime prompt delivery.
See [refresh safety](../plans/RUST_MIGRATION.md#native-state-lifecycle-and-existing-workspace-refresh).

## Continue with focused workflows

- [Task workflows](TASK_WORKFLOWS.md): snapshot CAS, opt-in ledger, and recovery.
- [Preparation workflows](PREPARATION_WORKFLOWS.md): explicit attempts and reviewed graphs.
- [Workspace records](../architecture/WORKSPACE_RECORDS.md): paths, defaults, and records.
- [Storage contract](../architecture/STORAGE_CONTRACT.md): authoritative commit boundaries.
- Implementation: [init](../../src/init.rs), [refresh](../../src/refresh.rs), and
  [record validation](../../runner/records.py).
