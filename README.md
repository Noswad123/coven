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
│   ├── records.py         # pure v1 workspace record validators
│   ├── storage.py         # POSIX locks, atomic publication, strict history I/O
│   ├── task_ledger.py     # opt-in ledger, baseline import, durable task receipts
│   ├── checkpoints.py     # exact snapshot/local-scope checkpoint bindings
│   ├── actions.py         # opt-in runtime manifests, single-use claims/outcomes
│   ├── launch.py          # frozen cold-start transport plans/execution
│   ├── workers.py         # help-only native adapter compatibility observations
│   ├── task_attempts.py   # preparation-only execution intent/receipts and facade
│   ├── task_graphs.py     # pure bounded DAG/criteria/source manifests
│   └── execution_v2.py    # explicit graph upgrade, planning review/preparation
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
│   ├── .workspace.lock          # persistent coordination file; never force-delete
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

After explicit task-ledger import, an unsafe/unreadable `state/tasks.json`
projection is diagnosed without following it. Authority and committed receipts
remain readable, but new task changes are blocked and unsafe paths cannot be
automatically repaired or overwritten.

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

The task-file contract describes snapshot-v1. After explicit task-ledger import,
the same task records are validated from replay; an unhealthy `tasks.json`
projection is diagnosed rather than mistaken for authoritative state.

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
explicit task import/recovery are separate operations; refresh never activates or
repairs task authority. Refresh remains subject to managed-path validation.
On first valid mutation/publication in a legacy workspace, Coven may create
`state/.workspace.lock`; this coordination file is not a state migration.

### Safe snapshot-v1 task mutations (default)

```bash
coven task list --json
coven task add "Investigate tool rendering" --owner jamal --json
# Copy a revision token from task list --json; do not literally use the placeholder.
coven task assign core-chatbot jamal --expect-revision '<revision token>' --json
```

Cooperating current Coven processes serialize task add/assign under a persistent
workspace lock, reload task state inside it, and atomically replace the complete
task list. `--expect-revision` rejects a stale whole-task snapshot; without it,
assignment is ordinary serialized reassignment, not an exclusive claim. Task
listing returns a token from its exact bytes without creating a lock. Tokens are
content hashes (or `tasks-missing:v1`), not monotonic or approval revisions.

The task snapshot is authoritative; history and dashboard are separate secondary
writes. A task committed with history/dashboard warnings still exits **0**. Exit
**2** with `outcome: durability_uncertain` means replacement occurred but directory
sync failed. Inspect current task state before retrying either case. There is no
idempotency key yet, so repeating an add after a lost response can duplicate work.

The lock waits up to five seconds and is released by the kernel when its holder
exits. Never delete a live lock file or mix these commands with old binaries or
direct shared-JSON writes. Event/message appends and dashboard render/publication
use the same lock, but goal/lead/checkpoint/runtime operations are not thereby
fully transactional. Generated prompts direct workers through task/message APIs;
custom role prompts remain preserved and may need their old direct-edit guidance
retired. Report unsupported task lifecycle changes as messages instead of editing
task JSON manually.

History parsing now reports malformed/nonobject/interior/truncated-tail records
with a path and line number instead of silently skipping them. Explicit inspection
and repair are required; no automatic truncation or replay. For full scope,
commit outcomes, and recovery instructions, see [STORAGE_CONTRACT.md](STORAGE_CONTRACT.md).

### Opt-in task ledger and durable retries

```bash
coven task list --json
# Substitute the actual token from the output, after inspecting current work.
coven state import-tasks --expect-revision '<snapshot revision>' --json
coven state inspect --json
coven task list --json
coven task add "Investigate rendering" --request-id rendering-1 --json
```

Explicit import preserves the exact old snapshot and activates a task-only,
sequenced ledger. It never interprets old events as complete history or silently
changes existing workspaces. After import, `state/tasks.json` is a projection:
task listing and dashboard rendering use replayed authority instead.

Imported add/assign require `--request-id`. The committed effect and frozen
receipt share one frame, so retrying the **same key with the original arguments
and expected revision** returns the original result without another mutation.
Changing that bound request conflicts. Snapshot-v1 rejects request IDs rather
than promising durable retries. Rejected precommit requests are not cached.

Ledger JSON contains a frozen `result` and live `delivery` metadata. Check
`delivery.outcome`/exit code: secondary warnings still mean committed; exit 2
means publication was observed but durability confirmation failed. A stale or
unreadable projection does not hide a receipt. New requests wait for explicit
recovery, which preserves observed projection bytes before replacement:

```bash
coven state recover-tasks --repair-projection --json
```

`state inspect` is read-only (unlike dashboard-publishing `status`). Interrupted
unactivated imports can be explicitly resumed against the original snapshot or
aborted by preserving/renaming their artifact directory. Corrupt active authority
never falls back to v1 or deletes final frames. See
[TASK_LEDGER_CONTRACT.md](TASK_LEDGER_CONTRACT.md) for formats, recovery commands,
limits, and request/acknowledgement semantics. This is not a whole-workspace
transaction, general task lifecycle, distributed coordinator, or exactly-once
secondary-event delivery protocol.

## Preparation-only task attempts

```bash
coven execution inspect --json
coven execution enable --expect-execution-revision execution-disabled:v1 --request-id enable-1 --json
coven execution enroll core-chatbot --expect-tasks-revision '<task-token>' \
  --expect-execution-revision '<execution-token>' --request-id enroll-1 --json
coven task attempt prepare core-chatbot --intent intent.json \
  --expect-tasks-revision '<current-task-token>' --expect-execution-revision '<current-execution-token>' \
  --request-id prepare-1 --json
coven task attempt list --task core-chatbot --json
```

Use inspected tokens and an explicit JSON intent with task ID, exact assigned
owner, and declarative acceptance criteria; see
[EXECUTION_INTENT_CONTRACT.md](EXECUTION_INTENT_CONTRACT.md) for the schema.
Enable enrolls zero tasks. Nothing changes task bytes/status, upgrades ledger-v1,
or starts a worker. Every result is `dispatchable: false`, authorization `none`.

Selected enrollments retain complete task snapshots and stable source identity.
One immutable prepared attempt per task records source revision, criteria/digests,
owner and predecessor. Fresh reassignment—including the same owner—is guarded
under the task-writer lock until explicit local cancellation:

```bash
coven task attempt cancel '<attempt-id>' --reason "Revise assignment" \
  --expect-execution-revision '<current-execution-token>' --request-id cancel-1 --json
```

Cancellation means **cancelled before submission**, never native termination.
It works without current task/roster validity, preserves the preparation, and
permits a new numbered attempt after reassignment. Changed sources/owners are
diagnostics, not automatic guard release. Import/resume requires no prepared
attempts; changed source identity then requires explicit enrollment rebind.

Execution effects and request receipts share one bounded atomic control file.
Retry the original key/content/revisions: replay precedes mutable source checks
and returns the frozen result even after cancellation. Uncertain delivery exits
2; current revision/state may be unknown while the known receipt is preserved.
Cancellation capacity is reserved before accepting preparations/history growth.
Inspect never creates an absent lock; corrupt current source does not hide valid
frozen attempts. Pending/corrupt execution control fails closed for fresh task
changes—no silent reset or automatic activation repair.

These are cooperating task-API guards, not path reservations, native tool policy,
execution approval, or a scheduler. Readiness and runtime-startup claims remain
independent; future native admission must explicitly revalidate and authorize
attempts rather than automatically executing existing preparations.

## Reviewed task graphs

Execution control stays v1 until an explicit, backed-up upgrade. Cancel existing
preparations first; task snapshots and task-ledger-v1 are never upgraded by it.

```bash
coven execution upgrade --expect-execution-revision '<v1-token>' --request-id upgrade-graphs-1 --json
coven task graph propose --document graph.json --expect-tasks-revision '<task-token>' \
  --expect-execution-revision '<v2-token>' --request-id graph-1 --json
coven task graph inspect --json
coven task graph decide '<graph-revision>' --decision approve --reason "Reviewed criteria" \
  --expect-graph-digest '<graph-digest>' --expect-tasks-revision '<current-task-token>' \
  --expect-execution-revision '<current-v2-token>' --request-id review-1 --json
coven task attempt prepare core-chatbot --graph-revision '<reviewed-revision>' \
  --expect-tasks-revision '<current-task-token>' --expect-execution-revision '<current-v2-token>' \
  --request-id prepare-from-graph-1 --json
```

The graph document selects already-enrolled task IDs, exact owners, criteria and
`depends_on` edges. The bounded DAG rejects duplicate/dangling/self/cyclic edges.
Proposal freezes selected complete task snapshots/enrollment bindings; planning
review owns one immutable criteria revision. Selected tasks become sticky managed
on proposal and cannot bypass review with `--intent`, even after revocation or
removal from a successor. Unmanaged tasks retain standalone preparation.

Covered task changes stale the plan; unrelated whole-source revision drift is
reported separately. Successors require cancellation of managed preparations and
fresh review. `changes-requested` remains available without healthy current source
and does not cancel old attempts. Generic checkpoint approval is not graph review.
Historical manifests/receipts remain readable and old v1 receipt values survive
upgrade unchanged.

**This is not scheduling or execution:** downstream preparation is permitted,
but dependency acceptance is unestablished and every node remains non-dispatchable
with authorization `none`. See [TASK_GRAPH_CONTRACT.md](TASK_GRAPH_CONTRACT.md)
for the document format, upgrade/retry/capacity contract, and exact exclusions.

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

```bash
coven checkpoint inspect plan-approval --json
# Substitute the inspected snapshot token; scope tokens are not write tokens.
coven approve plan-approval --expect-revision '<snapshot revision>' --json
coven suggest plan-approval "Please add QA evidence" --expect-revision '<new snapshot revision>' --json
```

The Markdown file is one decision authority: explicit approve/suggest installs a
versioned inline binding and publishes the whole file atomically under the
workspace lock. Approval is effective only while its checkpoint-local scope
matches current covered bytes. Editing purpose, criteria, custom prose, examples,
whitespace, or line endings yields `approval_stale` on read without rewriting the
file. Legacy bare `Status: approved` is now **`approved_unbound`**, not effective
bound approval, until an explicit decision. `suggest` records `changes_requested`;
historical events cannot revive approval. Dashboard/standup/review/next agree on
effective status rather than trusting presentation or old events alone.

Scope excludes only live decision machinery and valid trailing machine history.
Old human history remains covered; moving machine history into reviewed content,
or adding prose after it, makes that region covered too. Rationale is retained in
structured history comments and decoded by inspect. Ambiguous/malformed machinery
and incompatible notes fail before writes or notifications.

`--expect-revision` protects the exact inspected snapshot, including machinery
and history. Without it, CLI decisions explicitly select the latest snapshot
under the lock; they do not prove an earlier review still matches. Interactive
review and console confirmation submit their displayed revision and never retry
conflicts automatically. Console/review preserve conflict/uncertainty exit codes.

After commit, history/dashboard failures and runtime-notice failures produce
warnings, not rollback. Exit 2 means replacement with uncertain durability and
skips secondary work. Notices run outside the lock, carry decision/scope IDs,
suppress known-superseded decisions, and tell agents to recheck current status.
They are not execution grants. No checkpoint request deduplication is provided.

This binds **checkpoint text only**, not mutable link targets, run plans, task
graphs, repositories, or candidate commits. Dispatch/integration/delivery
authorization still needs explicit policy and enforcement. See
[CHECKPOINT_CONTRACT.md](CHECKPOINT_CONTRACT.md) for exact byte coverage, legacy
behavior, single-file commit boundaries, and limits.

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

## Native worker readiness inspection

```bash
coven worker readiness --adapter djinn --profile research-readonly --json
coven worker readiness /path/to/project --executable /trusted/path/to/djinn
```

This read-only **adapter compatibility diagnostic** creates no worker/session and
grants no permission. It resolves/fingerprints the executable, follows only
advertised help routes, and reports CLI surfaces separately from unverified native
identity, acknowledgement/reconciliation, lifecycle, cancellation, results, and
immutable tool/resource policy. Coven does not load Djinn credentials/configuration
or request native session inspection. Native help runs as trusted code; these
limits are not a purity or filesystem-access guarantee. Ordinary project
directories are accepted; the inspector creates no Coven state, logs, locks,
dashboards, or migrations.

The current `djinn-help-v1` adapter **never declares `compatible` from help**.
Exit 1 means a verified help surface lacks the required session/run route; exit 2
means unknown/probe failure or unverified worker guarantees. Exit 0 is reserved
for a future implemented compatibility contract. Installed Djinn currently
advertises useful native session surfaces but does not establish this bounded
worker contract; its observed readiness is `unknown`, with authorization `none`.

Probes have bounded output/time and supervised cleanup, including caller death.
No bare native status/watch/list command is used: those may persist recovery
observations. Select a trusted executable—help probing is not an OS sandbox.
There is no `coven worker run` or fallback to runtime startup. See
[WORKER_READINESS_CONTRACT.md](WORKER_READINESS_CONTRACT.md) for exact allowed
probes, report semantics, source observations, and native prerequisites.

## Optional reviewed runtime startup

**This authorizes unrestricted runtime startup, not read-only work or task
dispatch.** Current pane adapters cannot sandbox tools or prove worker acceptance.
Native tools may still mutate files according to their independent policy.

```bash
coven action enable-runtime-launch --json
coven action prepare-runtime-launch --json
coven action inspect '<action-id>' --json
coven checkpoint inspect 'runtime-launch-<action-id>' --json
coven approve 'runtime-launch-<action-id>' --expect-revision '<snapshot-token>' --json
coven action submit '<action-id>' --expect-manifest '<manifest-token>' \
  --expect-checkpoint '<approved-snapshot-token>' --expect-decision '<decision-id>' --json
```

Use actual IDs/tokens from output; an optional workspace path precedes the action
ID. Enable is explicit and persistent. It drains admitted cooperating legacy
transports before activation, then blocks legacy launch/restart/bootstrap/pane
delivery paths and agent start/restart/stop. Stop old binaries/direct writers and
inspect old sessions before opting in; activation does not stop running agents.
Existing unactivated workspaces retain their behavior.

Preparation freezes a closed guidance/config/task/roster/prompt inventory and
literal transport plan into an immutable manifest. The dedicated checkpoint
binds that manifest. Submission rechecks current inputs and exact approval under
the workspace lock, consumes one durable claim, then executes only its frozen
plan **outside** that lock. Edits/revocation before claim block startup; after
claim they do not cancel the consumed attempt. No repository/source/toolchain or
filesystem sandbox is implied.

Repeated submissions return the consumed attempt without launching again—even
after failure or a lost response. An uncertain claim never executes. An unresolved
attempt requires inspection and explicit `action acknowledge-unknown` before a
separately prepared/reviewed new action. Nothing automatically retries or deletes
partial resources.

```bash
coven action stop '<action-id>' --json
coven down   # enabled mode: current scoped attempt only, using its frozen backend
```

`transport_completed` means startup CLI commands succeeded, not healthy workers
or accepted tasks. Status is unverified in this mode; action inspection reports
durable transport facts. Stop distinguishes attempted from successful response
and never resets consumption. See
[RUNTIME_LAUNCH_CONTRACT.md](RUNTIME_LAUNCH_CONTRACT.md) for exact inventory,
activation/claim linearization, recovery, and exclusions.

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
In approved-launch mode, only metadata is recorded; legacy pane delivery remains
disabled and a lead record cannot grant startup or worker tool permissions.

## Development checks

Python 3.11+ and local POSIX locking (macOS/Linux) are required. Run the offline regression
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
