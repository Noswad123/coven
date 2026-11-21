# Runtime and checkpoint workflows

## Choose the effect deliberately

Coven's installed product and checkout launcher are native Rust. Examples using
`target/debug/coven` select an already-built development binary explicitly; they
are not launcher fallbacks. Run those examples from the repository root.
`WORKSPACE`, uppercase names and angle-bracket tokens are placeholders to replace
with inspected values. Do not execute these reference examples unchanged:
placeholder spelling does not make an effectful command safe.

There are three different boundaries here:

| Operation | What it establishes |
| --- | --- |
| Checkpoint inspect / `next` | Read-only local evidence and inert guidance, not approval. |
| Checkpoint approve / suggest / review | A content-bound local decision, possibly followed by legacy pane notices. |
| Legacy transport or reviewed action submission | Real process/input effects, not a read-only worker sandbox or task acceptance. |

Do not validate launch/send/stop examples against actual user sessions. Native
tools retain independent credentials, policy and filesystem effects. Neither
reviewed startup nor a role named “read only” restricts what those tools can do.
For ordinary agent invocation/reporting, prefer the
[normal agent workflow](NORMAL_AGENT_WORKFLOW.md).

## Native legacy lifecycle

The native legacy routes include `standup`, `console`, `lead`, `bootstrap`,
`agent status/start/stop/restart`, `plan`, `up` / `start`, and `restart`, alongside
`send` / `agent send` and `down` / `stop`. Workspace operands are optional where
supported; use explicit paths when operating from the repository root.
Agent selectors are exact persisted IDs, not display labels.

Legacy mode uses Herdr by default. Generated agent panes use `oc` when available
and fall back to `opencode`; initialize with `--opencode-command` to override.
The following are **effectful reference commands**, not a validation recipe:

```text
coven plan WORKSPACE
coven up WORKSPACE
coven up WORKSPACE --replace
coven down WORKSPACE
coven restart WORKSPACE
# Aliases:
coven start WORKSPACE
coven stop WORKSPACE
```

`up --replace` / `start --replace` replaces existing topology, as does `restart`.
Reuse versus replacement and persisted batch order are preserved. `plan` does
not launch a tool, but does append a request event: it is not a pure preview.

To select tmux at initialization, use a new destination:

```text
coven init /absolute/path/to/new-coven --multiplexer tmux
```

Or deliberately edit the existing workspace's runtime selection:

```toml
[runtime]
multiplexer = "tmux" # or "herdr"
```

`coven.json` owns workspace name and roster. In `coven.toml`,
`[runtime].multiplexer` and `[runtime].opencode_command` override their JSON
counterparts; `[runtime.agent_working_directories]` replaces the entire JSON
directory mapping, not individual entries. `COVEN_MULTIPLEXER` takes precedence
for runtime selection. Generated TOML agent/role/prompt/checkpoint sections are
descriptive, not live customization. Workspace state is not rebuilt from the
event log. See [directory isolation](AGENT_DIRECTORY_ISOLATION.md) for full-roster
mapping validation, required starter refresh and reviewed-launch v1 restrictions.

### Individual panes and starter prompts

```text
coven agent status WORKSPACE jamal
coven agent send WORKSPACE jamal "Please summarize your blocker."
coven agent stop WORKSPACE jamal
coven agent start WORKSPACE jamal
coven agent restart WORKSPACE jamal
coven bootstrap WORKSPACE
```

Start/restart/bootstrap may launch unrestricted tools or deliver input. Bootstrap
preflights all starter prompts before delivery, but counts **attempts**, not
successful delivery or worker acceptance. Ordinary nonzero delivery exits are
ignored; setup/timeout errors stop the batch without retry.

Lifecycle takes shared transport admission before workspace exclusion, reloads
mode/state, and releases workspace exclusion before backend I/O. There is no
implicit rollback or retry. Both stdin and stdout must be TTYs for focus;
interactive tmux attach/switch inherits stdio without the captured-command
60-second limit. Admission remains held through attach, while workspace I/O can
continue.

Delayed generated five-second shell senders are **not fenced** after creation
admission ends. They can still run while exclusive activation admission is held;
this legacy behavior is not stronger activation quiescence.

Writer/display profiles bound aggregate launch/bootstrap staging, generated
scripts and standup output. Raw configured-command NUL is rejected before
normalization or destructive restart. See the
[native lifecycle details](../plans/RUST_MIGRATION.md#native-legacy-lifecycle-completion)
for exact caps and compatibility dispositions rather than treating these checks
as a sandbox.

## Review and communicate with users

These examples select an existing workspace explicitly:

```text
coven review WORKSPACE
coven console WORKSPACE
coven approve WORKSPACE plan-approval
coven suggest WORKSPACE pre-delivery-review "Please include QA evidence before final approval."
coven send WORKSPACE jamal "Please continue core-chatbot and update the logs."
coven send WORKSPACE all "Pause and summarize current status."
```

`review` walks pending checkpoint files: approve, request changes, skip or quit.
`console` offers status, standup, agents, send, approve, suggest and review.
Console decisions display a revision, default confirmation to **No**, and CAS
that displayed revision. Nested review reuses borrowed input rather than
relocking stdin. Review/console preserve the maximum checkpoint exit status
through quit/EOF and do not retry revision conflicts automatically.

### Inspect before deciding

```bash
target/debug/coven checkpoint inspect /path/to/workspace plan-approval
target/debug/coven checkpoint inspect /path/to/workspace plan-approval --json
target/debug/coven next /path/to/workspace
```

Inspect defaults to cwd and `plan-approval`. Its first operand selects a workspace
only when the expanded path contains a `coven.json` marker. `next` takes one
optional explicit workspace, defaulting to cwd. Neither searches ancestors.
Inspect's positional group must not be split by flags; repeatable `--json` and
unambiguous option prefixes retain their supported parser behavior.

Both readers validate local inputs but acquire no locks, fsync nothing and write
no decisions, events or dashboard. They provide neither a coherent snapshot nor
fresh execution authority. `next` validates the whole message log/checkpoint
references before events, selects the last chronologically sorted nonapproved
request, then falls back to sorted lowercase `*.md` files with `approval_stale`,
`approved_unbound` or `decision_invalid`. It is not general pending discovery.
No selection produces a dashboard/status suggestion, not dashboard publication.

**Terminal warning:** `next` prints literal paths/root text. ANSI, bidi and other
controls may render. Suggested commands are inert text, not sanitized or quoted
execution instructions.

### Practical checkpoint CAS recipe

Inspect the content and substitute the actual **snapshot** token at each step.
Scope tokens measure approval effectiveness; they are not conditional-write tokens.
Keep the placeholders distinct after the first mutation:

```bash
target/debug/coven checkpoint inspect /path/to/workspace plan-approval --json
# Substitute the inspected snapshot token; scope tokens are not write tokens.
target/debug/coven approve /path/to/workspace plan-approval --expect-revision '<snapshot revision>' --json
target/debug/coven suggest /path/to/workspace plan-approval "Please add QA evidence" --expect-revision '<new snapshot revision>' --json
```

The Markdown file is the decision authority. Approve/suggest atomically replaces
the complete file under the workspace lock with a versioned inline binding.
Without `--expect-revision`, the writer deliberately selects the latest snapshot
under lock; it does not prove that previously reviewed content still matches.

Approval remains effective only while covered checkpoint-local bytes match.
Purpose, criteria, prose, examples, whitespace and line endings are covered;
editing them produces `approval_stale` without rewriting the file. A legacy bare
`Status: approved` is `approved_unbound`, not effective bound approval. `suggest`
records `changes_requested`; old events cannot revive approval. Ambiguous or
malformed decision machinery and incompatible notes fail before writes/notices.

Only live decision machinery and valid trailing machine history are excluded
from scope. Old human history remains covered. Moving machine history into prose
or adding prose after it makes that region covered too. Inspect decodes rationale
from structured history. The exact byte rules belong to the
[checkpoint contract](../architecture/CHECKPOINT_CONTRACT.md).

After commit, history/dashboard or notice failures warn rather than roll back.
Exit 2 means replacement with uncertain durability; secondary work is skipped.
Inspect before retrying: checkpoints have no request deduplication. Notices run
outside the lock, carry decision/scope IDs, suppress known superseded decisions
and instruct agents to recheck current authority. Delivery is not acceptance.

This binds checkpoint text **only**, not mutable link targets, run plans, graphs,
repositories or candidate commits. Separate policy/enforcement is still required
for dispatch, edits, integration and delivery.

## Optional reviewed runtime startup

The native product supports explicit reviewed **cold launch**, not merely plan
inspection. This mode authorizes one unrestricted runtime startup attempt, not
read-only research, worker acceptance or task dispatch. It freezes neither the
whole filesystem nor native tools, source, credentials or provider configuration.

### Manually bind and review one frozen plan

**Pseudocode, not an automatic approval script.** Replace `WORKSPACE` and every
angle-bracket placeholder with inspected output. Review both the immutable plan
and its dedicated checkpoint before deciding; approval alone never submits.
These commands can start unrestricted tools, not inert preview sessions.

```text
target/debug/coven action enable-runtime-launch WORKSPACE --json
target/debug/coven action prepare-runtime-launch WORKSPACE --json
target/debug/coven action inspect WORKSPACE '<action-id>'
target/debug/coven checkpoint inspect WORKSPACE 'runtime-launch-<action-id>' --json
target/debug/coven approve WORKSPACE 'runtime-launch-<action-id>' --expect-revision '<snapshot-token>' --json
target/debug/coven checkpoint inspect WORKSPACE 'runtime-launch-<action-id>' --json
target/debug/coven action submit WORKSPACE '<action-id>' --expect-manifest '<manifest-token>' \
  --expect-checkpoint '<approved-snapshot-token>' --expect-decision '<decision-id>' --json
```

Enable is explicit and persistent, with no disable/reset command. Stop old
binaries/direct writers and inspect old sessions first. Activation drains admitted
cooperating legacy transport, but does not stop agents already running. Enabled
mode blocks legacy launch/restart/bootstrap/pane delivery and agent start/restart/
stop; unactivated workspaces keep their behavior. Status/standup is unverified in
enabled mode, not a native health query.

Preparation captures a closed guidance/config/task/roster/prompt inventory and
literal transport plan. The dedicated checkpoint binds that immutable manifest.
Submission rechecks current inputs and exact approval under the workspace lock,
consumes one durable claim, then runs **only the frozen plan outside the lock**.
Edits/revocation before claim block startup; after claim they do not cancel it.
Changing the dedicated checkpoint scope is not a way to change the frozen plan;
prepare and review a new action instead.

Action inspect validates retained control/bundle/checkpoint evidence without
running transport or checking current runtime inputs. Its default is pretty
literal-Unicode JSON; `--json` selects compact ASCII JSON. Inspection is not
fresh launch permission or durability confirmation.

### Uncertain attempts and operator stop

Repeated submissions return the consumed attempt without launching again, even
after failure or a lost response. An uncertain claim **never executes**. An
unresolved attempt requires inspection and explicit acknowledgement before a
separately prepared/reviewed new action:

```text
target/debug/coven action acknowledge-unknown WORKSPACE '<action-id>' --expect-claim '<claim-id>' --json
target/debug/coven action stop WORKSPACE '<action-id>' --json
target/debug/coven down WORKSPACE
# stop WORKSPACE is an alias for down; enabled mode uses only the frozen current attempt.
```

Acknowledgement accepts uncertainty, not success, stopped-process proof or replay
permission. Consumption is permanent; nothing automatically retries or deletes
partial resources. Other scoped containers may still be running. Stop uses the
recorded namespace/backend and distinguishes attempted from successful response;
`down` does not sweep all historical attempts or reset consumption.

Captured runtime commands have a 60-second/1 MiB combined-output profile and
clean up only their direct child, not server/daemon effects. Frozen launch uses
captured cwd and the six-key environment; legacy delivery preserves caller cwd
and environment. Ordered PATH fallback is permitted only after proven pre-image
`ExecFailed` (spawn error), never timeout, output overflow, generic `SpawnError`
or an executed exit 127. Observation probes instead retain their separate
at-most-30-second supervised group-cleanup policy.

`transport_completed` means required startup CLI commands succeeded—not healthy
workers, an accepted task or completion. See the
[runtime launch contract](../architecture/RUNTIME_LAUNCH_CONTRACT.md) for exact
inventory, activation/claim boundaries, outcome codes and recovery exclusions.

## Lead metadata is not pane delivery

```text
coven lead WORKSPACE srikanth
coven lead WORKSPACE
coven lead WORKSPACE clear
```

The lead is recorded in `state/lead.json`, shown in dashboard/standup and logged.
Legacy mode can send the notice to a running lead pane. Approved-launch mode
records metadata only: legacy pane delivery stays disabled. A lead record grants
neither startup permission nor worker tool permissions; it is not a scheduler.

Lead preserves extension fields and its literal default prompt. Set is atomic;
clear unlinks and syncs. Historical lead IDs need not be in today's roster.
Committed secondary failures warn; postcommit notices happen outside workspace
exclusion and do not duplicate message history.

## Related files and implementation limits

- [Native migration details](../plans/RUST_MIGRATION.md#native-reviewed-runtime-lifecycle)
  and [retirement gates](../plans/RUST_RETIREMENT.md) separate implementation
  profiles from macOS, packaging and source-oracle cleanup acceptance.
- [`src/runtime.rs`](../../src/runtime.rs), `src/runtime/`,
  [`src/runtime_command.rs`](../../src/runtime_command.rs): legacy effects/admission.
- `src/actions/mutation/`, [`src/launch.rs`](../../src/launch.rs): frozen claims/plans.
- `src/checkpoint_decisions/`, [`src/checkpoint_guidance.rs`](../../src/checkpoint_guidance.rs):
  local decisions and inert guidance.
- [`tests/runtime_launch_workflow.rs`](../../tests/runtime_launch_workflow.rs):
  controlled workflow fixtures, not installed-provider certification.
