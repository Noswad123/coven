# coven

## Overview

`coven` is a personal, magic-themed multi-agent workspace command for creating,
tracking, and running local multi-agent workspaces.

See [APP_BOUNDARIES.md](APP_BOUNDARIES.md) for the 2026-10-22 target decision:
independent apps, Coven-owned cross-runtime observation/grouping first, optional
Kitsune annotations as a separate generic presentation path, and Djinn-owned native
execution. Initial Kitsune observation, explicit participant groups/roles, and a
foreground generic-annotation publisher are implemented in Coven, including explicit
opt-in process-evidence binding independent of Kitsune's agent inventory. Explicit
Djinn live attachments, revision-scoped busy/unknown group status and explicit
expiry-bounded `publish --live` are implemented;
native lifecycle, scheduling, extraction and automatic following
are not shipped. See [GROUPS_CONTRACT.md](GROUPS_CONTRACT.md) and
[GROUP_LIVE_STATUS_CONTRACT.md](GROUP_LIVE_STATUS_CONTRACT.md).

Explicit interactive Djinn UI launch/resume is also available through `coven session
start` and `coven session resume` into a selected existing Kitsune pane. Commands
preview by default; `--execute` uses discovered, terminal-pinned generic command
submission and the existing workspace transport gate. No worker readiness, prompt,
scheduling, automatic following or native lifecycle acceptance is inferred. See
[INTERACTIVE_SESSION_CONTRACT.md](INTERACTIVE_SESSION_CONTRACT.md) before using it.

Separate read-only `coven session plan-resume --runtime claude --source
kitsune:claude --session-kind id --session saved-session --json` inspects literal
resume argv for 16 historical provider mappings. It never launches a provider,
looks up executables, contacts a socket, or grants execution authority. Only Pi
and OMP accept path references. Existing execution-capable resume remains
Djinn-only and unchanged. See [SESSION_RESUME_PLAN_CONTRACT.md](SESSION_RESUME_PLAN_CONTRACT.md)
for validation, independent reference characterization, and extraction limits.

Read-only `coven session list WORKSPACE --socket ABS_SOCKET` and `coven session
inspect WORKSPACE --socket ABS_SOCKET --pane PANE` show generic pane/process-family
evidence plus matching declared groups/roles. They do not depend on Kitsune's agent
inventory, launch/probe native runtimes, read live artifacts or infer native activity,
identity, readiness or completion. No authoritative session registry or state writes
are added. See [SESSION_INSPECTION_CONTRACT.md](SESSION_INSPECTION_CONTRACT.md).

List/inspect can select declared targets with `--group GROUP [--member MEMBER]`
and/or `--recognized-only`. Selection happens only after the complete bounded
observation; original observed counts and declaration qualifiers remain explicit.
See [SESSION_VIEWS_CONTRACT.md](SESSION_VIEWS_CONTRACT.md).

`coven session watch WORKSPACE --socket ABS_SOCKET` performs bounded foreground
refreshes (default 60 seconds, two-second interval), optionally targeting one pane
and applying the same view flags. `--json` emits JSONL samples then a scoped watch
summary. No daemon, stored roster, stale positive fallback or native control follows.
See [SESSION_WATCH_CONTRACT.md](SESSION_WATCH_CONTRACT.md).

The optional `coven session prompt` command previews by default and, with explicit
`--execute`, submits to an opted-in Djinn native prompt socket using runtime/session
pins and a correlated message receipt. No Kitsune prompt/keystroke fallback or
permission inheritance is used. See [SESSION_PROMPT_CONTRACT.md](SESSION_PROMPT_CONTRACT.md).

`coven handoff validate/collect/inspect/list` adds explicit structured worker-claim
capture for review. Collection pins the raw input and current task revision/owner,
then archives it without changing task status or trusting reported checks/source.
Every collected result remains unverified. See [HANDOFF_CONTRACT.md](HANDOFF_CONTRACT.md).

The source of truth for a coven workspace is local, inspectable files. The
command creates those files, keeps lightweight state, and launches agent panes
with Herdr by default. Tmux remains available as an opt-in runtime.

## Install

### Native production cutover

The product is the single Rust binary `coven`. The checkout's `bin/coven` is now a
native-only `#!/bin/sh` launcher, not a Python entrypoint. It implements complete
native `init` for blank, embedded `interactive-demo`
and custom-example workspaces, handoff validation/capture/history,
local `goal`, `message`, `messages`, `events` and dashboard-publishing `status`,
bounded snapshot/ledger `task list` (human by default, `--json` optional), read-only
`state inspect`, and
`execution history --json` for retained v1/v2 preparation authority. It also supports
native `execution inspect`,
`task attempt list --task`, `task attempt inspect`, `task graph inspect` and
`task graph export`; see the [inspection contract](RUST_MIGRATION.md#native-execution-inspection-and-graph-export).
Native **`task attempt cancel`** and **`execution recover --confirm-authority` /
`--archive-pending`** now maintain existing v1/v2 authority, including exact legacy
cancellation retries after upgrade. They require no current config, marker, tasks,
roster or runtime inputs. Cancellation records local pre-submission cancellation;
it never stops a process or dispatches work. Enable, enroll/rebind, preparation
(intent and graph), upgrade, graph proposal and both planning decisions are also
implemented natively, without runtime execution. Native repository inspect (including
`--verify`)/register, worker readiness and persisted `observe djinn` are also available;
see [bounded local observation](RUST_MIGRATION.md#native-repository-readiness-and-persisted-djinn-observation).
Native Kitsune/Djinn-live observers and all eight Group routes have also passed
the bounded Linux workflow gate: 79 acceptance cases across ten routes, including
exact authority-byte preservation and reservation/uncertainty checks. These are
observation and annotation operations, not task dispatch or execution permission.
All seven Session routes are now wired natively with interpreter-independent
fixtures; all **78 original command paths are represented**, not certified as full
parity. Compatibility, macOS, packaging and Python retirement remain separate gates in
[RUST_RETIREMENT.md](RUST_RETIREMENT.md).
See the [writer contract](RUST_MIGRATION.md#native-execution-authoring)
and [maintenance contract](RUST_MIGRATION.md#native-execution-cancellation-and-recovery).
Inspection and maintenance default to pretty ASCII JSON (two-space indentation);
`--json` selects
compact JSON, each with exactly one terminal LF. There is no human renderer or
dispatch. Inspection optionally assesses current source under an **existing**
workspace lock (five-second wait, no creation); missing/busy/unsafe locks leave
source uncoordinated. The inspection guard still requires a regular `coven.json` marker;
marker-free history/export and uncoordinated inspection remain supported.
Graph export fully replays retained authority but never loads current source,
configuration, tasks, checkpoints, runtime control or locks. All trust fields remain
authorization `none` and dispatchable `false`; preparation eligibility grants no
execution permission. The separate, unchanged execution-history
reader reports historical reservations and frozen receipts, not current-source
validity, durable mutation permission or execution authorization. V2 also replays
graph proposals/reviews and graph-bound preparations without granting fresh
approval or running graph operations. `action inspect [WORKSPACE] ACTION_ID [--json]`
also validates captured launch control, frozen plans and the dedicated checkpoint
without running transport or checking current runtime inputs. Recorded claims
remain visible despite ordinary bundle/checkpoint diagnostic errors; inspection
is not fresh launch permission or durability confirmation. Standalone
`checkpoint inspect [WORKSPACE] [CHECKPOINT_ID] [--json]` performs its own ordered
workspace/configuration/authority preflight before returning the checkpoint view;
it deliberately has broader dependencies than action inspection. Default output is
pretty ASCII-escaped JSON (two-space indentation and terminal LF); optional `--json`
preserves the existing compact CLI/API bytes, with no added separator spaces.
Native `next [WORKSPACE]` now composes read-only checkpoint guidance after full
workspace and log validation. It neither approves nor notifies agents. See
[checkpoint guidance](RUST_MIGRATION.md#native-next-checkpoint-guidance) for selection
and literal-output limits. The native implementation also has typed
finite task values and a mixed-language-tested library workspace lock primitive.
Library foundations now retain frozen task-ledger receipts for exact historical
lookup and provide guarded atomic mutable-file replacement. They now support the
task writer workflow below, without granting runtime execution permission.
Library selected-ledger confirmation and execution-reservation checks borrow
an existing workspace guard and synchronize validated authority before reporting
their outcome for native task/state writers. Historical inspection commands remain
lock/fsync-free.
Native `state import-tasks WORKSPACE --expect-revision SNAPSHOT_TOKEN` activates
baseline authority marker-last. `state recover-tasks WORKSPACE` requires exactly
one of `--repair-projection`, `--resume-import`, or `--abort-import`; resume also
requires the original snapshot pin. State commands print pretty JSON by default,
or compact JSON with `--json`. Recovery preserves raw projection evidence before
replacement and archives aborted pending imports without deleting them.

Native **`task add` and `task assign` are now available end to end** for existing
snapshot and imported-ledger workspaces, with optional CAS, ledger request replay,
task events, dashboard refresh, and JSON/human delivery outcomes. For example:

```bash
target/debug/coven task add /path/to/workspace "Review parser" --owner operator --json
target/debug/coven task list /path/to/workspace --json
target/debug/coven task assign /path/to/workspace TASK_ID operator --json
```

Use the returned task ID and a declared roster ID. Imported ledgers require a
`--request-id` on every add/assign; snapshots deliberately do not support durable
request deduplication. Known authority-sync uncertainty exits `2`; secondary
event/dashboard/projection warnings do not undo a committed task. See
[RUST_MIGRATION.md](RUST_MIGRATION.md#native-task-addassign-workflow) for retry rules,
bounded profiles and retry/delivery boundaries.

Native `refresh WORKSPACE` now regenerates existing-workspace assets without
executing commands. It preserves authored files, state, logs, checkpoint decisions
and existing role prompts; manifest/starter writes can have partial effects, with
the atomic dashboard published last. Blank, embedded `interactive-demo` and custom
example context are supported; see [example root selection](examples/README.md).
Generated commands default to canonical `coven`,
not a Python checkout wrapper. See the migration notes for recovery limits,
refresh safety deviations and the static/cooperating filesystem model.

The native local workspace loop uses an explicit build path, not the installed CLI:

```bash
cargo build --offline --locked --bin coven
target/debug/coven init /absolute/path/to/new-review --blank --agents operator,reviewer
target/debug/coven goal /absolute/path/to/new-review "Review parser edge cases"
target/debug/coven goal /absolute/path/to/new-review
target/debug/coven message /absolute/path/to/new-review operator reviewer "Please review the parser"
target/debug/coven events /absolute/path/to/new-review --pretty
target/debug/coven messages /absolute/path/to/new-review --raw
target/debug/coven status /absolute/path/to/new-review
```

`goal` joins and strips text; empty text shows the full goal, not a clear operation.
`message` accepts free-form parties and preserves the joined body's whitespace;
it appends locally, without notification or runtime calls. `status` **writes** only
the dashboard (and may create the coordination lock), then rereads it outside the
lock. Message/event visibility can precede an exit-1 failure; there is no receipt,
deduplication, rollback or safe automatic retry. Goal primary sync uncertainty also
exits 1, unlike task/state exit-2 authority uncertainty; confirmed goal secondary
warnings still exit 0. See the [local workspace contract](RUST_MIGRATION.md#native-local-workspace-commands).

**Terminal warning:** all five new command families preserve Python's literal
Unicode/control-text output, including pretty history, goal and status—not only
`--raw`. ANSI, bidi and other controls can render in your terminal. Existing task
human/report escaping and terminal-LF policy are unchanged.

The historical reviewed-runtime full pinned `just check` passed formatting, all-target offline locked Clippy
with warnings denied, **763 native tests** (663 library passed, 46 binary, 54
top-level integration) and **941 Python tests in 190.822s**. Two subprocess-only
library helper entries are ignored in the outer suite and exercised by parent
tests; they are not added to the passed total. Nested child libtest runs are also
excluded. The earlier 659-native process-integration and 586-native execution-authoring
gates remain historical milestones. Maintenance acceptance passes **229 cases**, including
210 CLI cases and 12 mixed-writer races, with process-death lock release also
checked. The execution inspection corpus retains **539 cases** and all eight
parser-refusal/no-effect guards; cancel/recover now use invalid flags in those
guards because their valid command profiles are supported.
Checkpoint inspection and guidance have **580 green CLI
cases**: 403 inspection/workspace cases and 177 guidance cases, including help,
repeated flags, exact operand restoration and zero-effect checks.
Local workspace acceptance has **348 green native CLI
cases** (346 Python-oracle cases plus two native stdout-failure cases), alongside
the current 215 init, 208 lifecycle and 262 task-writer cases. Validation is Linux-only.

The prior complete migration check passed **1,191 native tests** (998 library,
86 CLI and 107 top-level integration across 37 suites), **941 Python tests in
220.389s**, **18 development-tool tests in 27.407s**, formatting, strict offline
locked all-target Clippy and whitespace checks on Rust/Cargo 1.99.1. All integration
suites pass. Three
subprocess-only library helpers are ignored in the outer run and exercised by
parents, not counted twice. See [current validation](RUST_MIGRATION.md#worktree-reconciliation-validation)
for commands, saved evidence and platform qualifications. This is a historical
pre-cutover record. Final local acceptance is **split validation: 1,225 native
passes (1,021 library + 86 CLI + 118 integration across 39 suites)**, not one fresh
all-command green invocation. The final library run had five ignored helpers
exercised by parents, not extra passes. Strict all-target Clippy and Rust **1.85.0**
all-target check pass after the two MSRV fixes, without production-policy changes.
The standalone final Python run passed **941 tests in 191.978s**; **22 development-tool
tests passed in 156.071s**. See [final local acceptance](RUST_MIGRATION.md#final-local-acceptance)
for saved outputs, retry qualifications and package/build evidence.

Execution-authoring acceptance passes **141 cases**, including **128 CLI cases**
and **six uncontrolled shared-lock races**, with an empty child PATH. Python runs
only in the parent as a temporary test oracle, never as native fallback. Fresh
standalone and reviewed two-node flows are covered, not a maximum-size lifecycle
or both race orderings. Targeted activation/upgrade death/uncertainty tests and
coordinator near-limit revocation proofs pass in the full gate (see
[validation](RUST_MIGRATION.md#validation)).

The parent-selected scope is **native production cutover with unchanged Python
source retained as a development oracle**, not deletion of all source oracles or
macOS closure. Neither the launcher nor installed product delegates to Python.
All 941 original Python tests remain active; eleven legacy-wrapper methods now
invoke the Python runner explicitly with the former wrapper environment restored,
preserving assertions, arguments, cwd and timeouts under reviewed hash transitions.
The source-removal audit remains **67 mapped / 874 unresolved of 941 IDs**: this is
a development-oracle cleanup gate, not 874 missing product features or a native
execution coverage count. Explicit native handoff collection writes
only local unverified evidence. Reviewed cold launch, checkpoint approve/suggest/review,
legacy `send` / `agent send` and `down` / `stop` are native with real transport
effects; see [reviewed startup](#optional-reviewed-runtime-startup). The subsequent
12-route legacy lifecycle batch historically brought coverage to **61 represented / 17 absent
of 78 Python paths**; Group/Observe and Session now bring it to **78 represented /
0 absent**, counting aliases; native-only `execution history` is extra,
not replacement coverage or a parity percentage. Checkpoint/action and legacy
Linux workflow gates are accepted within their native profiles, not all compatibility
profiles or macOS. Kitsune/Djinn-live observers (2), groups (8) and sessions (7)
are native within their documented Linux profiles. Preparation-only execution, observations
and readiness do not become dispatch permission.
Unsupported native commands fail closed without delegation.
Product execution is native; full source-oracle deletion, compatibility-policy and
platform closure are not claimed. See [RUST_MIGRATION.md](RUST_MIGRATION.md) for
build/check recipes and limits and [RUST_RETIREMENT.md](RUST_RETIREMENT.md) for the
separate cleanup and release gates. The original full-migration-before-merge
requirement remains: no merge, push or external promotion is authorized.

### Native bounded local observation

Historically, process integration brought coverage from 33/45 to 37 represented /
41 absent; reviewed runtime reached 49/29 and legacy lifecycle reached 61/17.
Current coverage is **78 represented / 0 absent** of 78 (aliases counted, not parity).
Repository's two routes and readiness's
one route have accepted **Linux implementation workflow gates** after the full
gate; profile and macOS acceptance remain open. All **3/3** Observe routes are
native: persisted Djinn, Djinn-live and Kitsune.
The current acceptance batch reports **144 native CLI cases**, using a compiled Rust
producer with no Python children and real disposable Git repositories. Targeted
coverage reports 24 repository, 11 readiness, 10 Djinn and 14 identity tests plus the
actual-binary supervisor bridge; these are different layers, not additive totals.
See [validation](RUST_MIGRATION.md#validation) for the verified full-gate record and
top-level aggregation method.

These are functional CLI commands, not SDK-only interfaces. Repository and persisted
Djinn default to pretty ASCII JSON (`--json` selects compact); readiness defaults to
a human report (`--json` preserves Unicode). Each adds one terminal LF. For example,
replace these **inert reference values** with inspected local paths and a real full
commit ID; use only a trusted executable and disposable checkout for validation:

```bash
target/debug/coven repository inspect /path/to/workspace
target/debug/coven repository inspect /path/to/workspace --verify --json
target/debug/coven repository register /path/to/workspace --path /path/to/disposable-checkout \
  --base FULL_LOWERCASE_COMMIT_ID --delivery-target refs/heads/review \
  --expect-repository-revision repository-unregistered:v1 --json
target/debug/coven worker readiness /path/to/workspace --adapter djinn \
  --profile research-readonly --executable /path/to/trusted/djinn \
  --timeout 3 --max-output-bytes 131072 --json
target/debug/coven observe djinn --executable /path/to/trusted/djinn \
  --folder /path/to/persisted-session --timeout 3 --max-output-bytes 131072 --json
```

Production uses trusted bounded local execution (Git, help/version and one fixed
`session observe` call), supervised through private self-exec bootstrap in the same
binary before Clap—not a Python helper. There are no providers, jobs, worker dispatch
or actual Djinn-instance calls in this validation; tests use controlled Rust fakes.
Executable pinning detects ordinary changes, not authentication or sandboxing.
Synchronous setup and executable hashing are not interruptibly hard-bounded;
probe/observation budgets are not a universal setup-inclusive wall-clock guarantee.
Linux is validated; macOS remains an open gate. External packaging approval and
source-oracle cleanup remain separate from native product execution.

### Native legacy lifecycle

#### Overview

The remaining twelve legacy routes are native: `standup`, `console`, `lead`,
`bootstrap`, `agent status/start/stop/restart`, `plan`, `up` / `start` (alias), and
`restart`. Existing send/down and approved-action behavior remains intact.
`bin/coven` selects only a prebuilt native executable; development build paths below
remain explicit alternatives, not launcher fallbacks.

#### API / Interfaces

Workspace arguments remain optional. Agent selectors use exact persisted IDs, not
display labels. `up --replace` / `start --replace` replaces an existing topology, as does
`restart`. Lead supports show/set/clear and `--message`; console composes the
same services and checkpoint review with borrowed input/output.

#### Data Flow

Legacy lifecycle validates the full batch, takes shared transport admission before
workspace exclusion, reloads mode/state, and releases workspace exclusion before
backend I/O. It preserves topology, reuse versus replacement and agent batch order,
without implicit rollback or retry. Both stdin and stdout must be TTYs to focus;
tmux attach/switch inherits stdio with **no 60-second capture limit**, holding shared
admission throughout attach while workspace I/O remains available.

#### Edge Cases

- **Delayed shell input is not fenced:** generated five-second senders can run after
  creation admission ends, even while exclusive activation admission is held. This
  intentional legacy behavior is not stronger activation quiescence.
- Bootstrap ignores ordinary nonzero delivery exits and counts **attempts**, not
  successful delivery or worker acceptance. Setup/timeout errors stop without retry.
- Approved-mode standup/status reports unverified with zero backend construction;
  historical lead IDs need not be in today's roster. Lead preserves extensions and
  its literal default prompt; set is atomic, clear unlinks and syncs. Committed
  secondary failures warn; postcommit notices run outside workspace exclusion and
  do not duplicate message history.
- Console decisions CAS the displayed revision, default to **No**, and retain the
  maximum checkpoint exit status through EOF/quit. Nested review reuses the same
  borrowed input, never relocking stdin.
- New writer/display profiles include 8 MiB aggregate launch/bootstrap staging,
  1 MiB generated scripts, separate 32 MiB standup row/final-output caps and exact
  reader-profile admission of lead storage bytes. Raw configured-command NUL is
  rejected before normalization/destructive restart; final scripts are validated too.
  Existing readers are unchanged; retirement still needs explicit profile disposition.

#### Example usage

These explicit native examples do not launch tools (replace the inert path):

```sh
target/debug/coven plan /path/to/disposable-workspace
target/debug/coven lead /path/to/disposable-workspace --clear
```

`plan` appends a request event; lead clear mutates metadata. Legacy `up` / `start`,
`restart`, agent start/restart and bootstrap can launch unrestricted tools or send
input. They are **not recommended validation commands against an actual user
workspace**; acceptance used isolated compiled inert backends, not real sessions.

#### Related files

See [the lifecycle contract and module map](RUST_MIGRATION.md#native-legacy-lifecycle-completion)
and [retirement gates](RUST_RETIREMENT.md). The historical lifecycle full pinned `just check` passed
formatting, strict offline locked all-target Clippy, **863 native tests** (752
library + 56 binary + 55 top-level integration across 29 suites) and **941 Python
tests in 189.082s**. Three ignored subprocess-only library helpers are exercised by
parents, not extra passes. New acceptance has **53 cases / 314 native calls /
12 routes / 1503 compiled-backend calls / 151 parent Python oracle replays /
16 TTY-mask cases**; retained reviewed-runtime **16 cases / 176 calls** and all full
suites remain green. Additional units prove a real 61-second foreground child and
execute generated shell on both inert backends with real five-second sleeps.
These are separate evidence layers, not additive test totals. Validation is Linux-only.

### Native Session workflows

#### Overview

Native `session list`, `inspect`, `watch`, `start`, `resume`, `prompt-capabilities`
and `prompt` are wired. View filtering is part of readers/watch, not an eighth route.
The bounded Linux implementation gate is accepted within profile; original-obligation
coverage review and retirement approval are still pending.

#### API / Interfaces

Readers use an explicit workspace and Kitsune socket, optional `--group GROUP
[--member MEMBER]` and `--recognized-only`; inspect requires `--pane` and permits
`--expected-terminal`. Start/resume require `--runtime djinn`, absolute executable/
socket paths, pane and terminal pins; resume also requires `--session`.
Prompt capabilities takes `--native-socket`; prompt requires workspace, native
socket, runtime UUID, `ses_…` session ID, request UUID and absolute prompt-file path.
Start/resume/prompt preview unless `--execute` is explicit. Session, action inspect
and handoff parser compatibility includes unambiguous long-option abbreviations
and repeated options (last value wins), not acceptance of unknown flags.

#### Data Flow

Readers bracket bounded generic process observations, join historical declarations,
then apply views. Watch streams samples and a final summary (JSONL with `--json`),
not a buffered transcript. Start/resume execute through terminal-pinned generic
submission under transport admission. Prompt uses only the opted-in native socket
with correlated receipt checks; there is no terminal fallback or automatic retry.

#### Edge Cases

Previews are pure: they do not load workspace/files/endpoints. Pins grant no
permission; native credentials/configuration remain independent. Empty workspace
normalization, symlink-parent/double-slash no-follow checks and sample/summary EINTR
exit 130 are regression-tested. Stream failure stops without retry; the watch
streaming adapter returns empty buffered stdout after streaming samples/summary.
Observation does not infer identity, readiness, activity or completion; read requests
may cause existing server housekeeping. Privacy/endpoint bounds remain enforced.

#### Example usage

**Reference syntax only, not live validation instructions.** Replace uppercase
placeholders with explicitly inspected values; no execute flags are shown:

```text
target/debug/coven session list WORKSPACE --socket ABS_KITSUNE_SOCKET --json
target/debug/coven session inspect WORKSPACE --socket ABS_KITSUNE_SOCKET --pane PANE --expected-terminal TERMINAL --json
target/debug/coven session watch WORKSPACE --socket ABS_KITSUNE_SOCKET --group GROUP --recognized-only --duration 10 --interval 2 --json
target/debug/coven session start WORKSPACE --runtime djinn --executable ABS_DJINN --socket ABS_KITSUNE_SOCKET --pane PANE --expected-terminal TERMINAL --json
target/debug/coven session resume WORKSPACE --runtime djinn --executable ABS_DJINN --socket ABS_KITSUNE_SOCKET --pane PANE --expected-terminal TERMINAL --session SESSION_SELECTOR --json
target/debug/coven session prompt-capabilities --native-socket ABS_NATIVE_SOCKET --json
target/debug/coven session prompt WORKSPACE --native-socket ABS_NATIVE_SOCKET --expected-runtime RUNTIME_UUID --session ses_SESSION_ID --request-id REQUEST_UUID --prompt-file ABS_PROMPT_FILE --json
```

Capabilities/read/watch contact the selected endpoint; only start/resume/prompt in
these examples are pure previews. Queued input/native acceptance is not worker
admission, model execution or completion.

#### Related files

See [native Session details](RUST_MIGRATION.md#native-session-completion),
`src/{session_inspection,session_views,session_watch,interactive_sessions,session_prompts,session_support}.rs`
and the linked Session contracts above. Independent fixtures cover **11 reader,
15 prompt and one multi-scenario start/resume integration tests**, plus **nine new
parser unit tests**; these are layers within, not additions to, the full totals.

### Native-only staged installation (not cutover)

This stages the native payload; it does not promote an external release or upgrade
an existing installation.

`scripts/install-native.sh` copies a prebuilt executable and examples into a **new,
absent** prefix whose parent already exists. It does not build, run the binary,
alter PATH/default wrappers, copy/delete `runner/`, or upgrade an existing prefix.
Inert reference syntax (absolute paths required):

```text
sh scripts/install-native.sh --binary /absolute/prebuilt/coven --prefix /absolute/new/stage --examples /absolute/examples
```

`--examples` is optional (defaults to source-distribution examples); `--help` lists
the flags. Symlink leaves and `..` are refused; ancestors must be trusted/stable.
Failure after reservation preserves a partial prefix and retries refuse it—no
cleanup or rollback. `packaging/coven.rb` is a local read-only **candidate**, not a
promoted external tap formula; real Homebrew install/upgrade is untested. macOS
was not run (installed SDK unavailable); a CI job being added has not been triggered.
Full migration is required **before merge**; no merge request or retirement approval
is implied. See [pending retirement gates](RUST_RETIREMENT.md).

### API / Interfaces

Build explicitly in this canonical checkout (offline requires cached locked dependencies):

```sh
cd /home/jdawson/Projects/coven-rust-migration
cargo build --offline --locked --release --bin coven
bin/coven --help
COVEN_NATIVE_BIN=/abs/prebuilt/coven /abs/checkout/bin/coven --help
```

The `/abs/...` paths are placeholders for inspected absolute paths. Help is titled
`Manage multi-agent coven workspaces`, with no Python-default qualification.

### Data Flow

The launcher `exec`s an absolute `COVEN_NATIVE_BIN` when set; otherwise it selects
only the normal checkout's `target/release/coven`. It forwards argv and preserves
the caller environment, including `COVEN_COMMAND` (generated-command override) and
`COVEN_HOME` (compatibility asset root: `COVEN_HOME/examples`, not binary selection).
It does not synthesize or overwrite either variable. Installed `bin/coven` is the
Rust executable itself, without a wrapper. See [packaging](packaging/README.md)
for asset precedence and staged installation.

### Edge Cases

Missing/non-executable native selection, relative/empty `COVEN_NATIVE_BIN`, or
same-inode launcher recursion fails explicitly with build guidance. There is no
Cargo invocation, Python, debug-binary or native PATH fallback. Default checkout
discovery follows relative/absolute launcher symlinks, bounded to 40 links;
`readlink` is needed only for symlinks (fixed system paths when PATH is empty).
Relocated copies require an explicit absolute `COVEN_NATIVE_BIN`. `COVEN_HOME`
is still forwarded as an asset root, not restored as the old binary selector.

### Public callback compatibility

Native init, refresh and lifecycle monitor callbacks select nonempty
`COVEN_COMMAND`, then nonempty `ARCANA_PUBLIC_COVEN`, then portable `coven`.
Empty values are unset; `ARCANA_PUBLIC_COMMAND` and `ARCANA_PUBLIC_NUMEN` are
ignored. Public path spelling is shell-quoted without canonicalization or an
executable probe. This also applies to installed direct binaries, and preserves
the Arcana fallback without embedding a source-checkout wrapper path or granting
execution authority. Non-UTF-8 selected values retain domain-profile errors.

### Completed locally vs External checklist

- **Completed locally:** native-only launcher/help and payload boundaries; independent
  entrypoint (5), package (2) and install (9) tests provide runtime-guard coverage.
  After nested harnesses moved to `tools/harnesses/group_publisher` and
  `tools/harnesses/group_live`, an agent verified the 300-file crate build and an
  extracted release install with cleared environment/empty PATH for help, blank/demo
  init and refresh. These scoped results are not a new full-suite total.
- **Local review readiness:** the implementation is ready for local review and manual
  native use within documented profiles. Split validation totals 1,225 native passes;
  strict all-target Clippy, Rust 1.85.0 all-target check and the standalone 941-test
  Python suite pass. The earlier 1,191-native record remains historical; see
  [final acceptance](RUST_MIGRATION.md#final-local-acceptance) for the split-run caveats.
- **External/release checklist:** macOS execution, actual Homebrew clean install/upgrade,
  reviewed formula/release selection and workspace command relocation. The local
  `packaging/coven.rb` candidate has no Python dependency/payload; the external tap
  was not modified or promoted. Its remote `main` URL is not evidence that these
  changes are published and must not be promoted before a reviewed release.
- **Residual policy/cleanup:** non-UTF-8 paths and finite resource/value caps still
  need explicit disposition; native production use does not silently approve them.
  Retained source-oracle removal remains separate. No changes have been committed,
  pushed or merged; no merge or source push is authorized.

### Related files

`bin/coven`, `src/main.rs`, `src/example_assets.rs`, `scripts/install-native.sh`,
`Cargo.toml`, `tests/native_{entrypoint,package,install}.rs`, and
[RUST_RETIREMENT.md](RUST_RETIREMENT.md) define these boundaries.

## Quick start

```bash
/home/jdawson/Projects/coven-rust-migration/bin/coven init ~/Projects/new-coven
cd ~/Projects/new-coven
/home/jdawson/Projects/coven-rust-migration/bin/coven plan
/home/jdawson/Projects/coven-rust-migration/bin/coven up
```

After the explicit release build, the normal checkout launcher can be used by its
path. For an installed command, stage the direct Rust binary as described above.
Symlinked checkout launchers discover their source checkout; relocated copies
require explicit `COVEN_NATIVE_BIN` selection.

## Project layout

This repository currently uses a flat standalone layout:

```text
coven/
├── bin/
│   └── coven              # native-only checkout launcher; never builds or delegates
├── src/                   # production Rust implementation and embedded assets
│   ├── main.rs            # native CLI routing
│   ├── process.rs         # private closed-request observation transport
│   ├── process/identity.rs # shared 1 GiB executable fingerprint and alias checks
│   ├── platform/unix/process.rs # same-binary supervisor, liveness/watchdog and cleanup
│   ├── repositories/     # immutable records, register/retry and bounded read-only Git
│   ├── workers.rs        # help-only readiness; workers/help.rs parses advertised surfaces
│   ├── djinn_observation.rs # fixed persisted-evidence call; schema in djinn_observation/
│   ├── cli_operands.rs    # checkpoint/next + execution readers/maintenance; narrow lexical compatibility
│   ├── execution/mutation.rs # source-free cancellation, retained binding lookup and successor admission
│   ├── execution/mutation/commands.rs # six authoring CLI adapters; document values, not paths
│   ├── execution/mutation/lifecycle.rs # private receipt-first coordinator and activation
│   ├── execution/mutation/{effects,graphs,upgrade,capacity}.rs # effects, backup and escape admission
│   ├── execution/mutation/delivery.rs # preadmitted known-result/uncertainty envelopes
│   ├── execution/confirmation.rs # full typed reread and control/directory synchronization
│   ├── execution/recovery.rs # explicit confirm-authority / whole pending-directory archive
│   ├── storage/maintenance.rs # private marker-independent acquisition of the same persistent flock
│   ├── execution/inspection.rs # replay/existing-lock/source assessment orchestration
│   ├── execution/source.rs # fresh raw snapshot or selected-ledger source under guard
│   ├── execution/inspection/commands.rs # five native inspection/export interfaces
│   ├── execution/inspection/projection.rs # pure binding/graph observations, proposal order
│   ├── checkpoint_workspace.rs # ordered checkpoint inspect, pretty/compact presentation
│   ├── checkpoint_guidance.rs # read-only next selection and literal output
│   ├── checkpoint_guidance/timestamp.rs # Python-compatible guidance chronology
│   ├── init.rs            # prepare-all, exclusive bootstrap, config-last publication
│   ├── storage/bootstrap.rs # private marker-free init reservation
│   ├── platform/          # OS path/descriptor operations
│   └── workspace_assets/  # pure templates; embeds original interactive-demo assets
├── runner/                # retained development oracle only; not installed/packaged
│   ├── coven.py           # explicit Python oracle CLI
│   ├── records.py         # pure v1 workspace record validators
│   ├── storage.py         # POSIX locks, atomic publication, strict history I/O
│   ├── task_ledger.py     # opt-in ledger, baseline import, durable task receipts
│   ├── checkpoints.py     # exact snapshot/local-scope checkpoint bindings
│   ├── actions.py         # opt-in runtime manifests, single-use claims/outcomes
│   ├── launch.py          # frozen cold-start transport plans/execution
│   ├── workers.py         # help-only native adapter compatibility observations
│   ├── kitsune_observation.py # bounded, explicit existing API socket observation
│   ├── groups.py           # explicit participant/role authority and durable counters
│   ├── group_live.py       # declared live-source pins and ephemeral group status
│   ├── group_publisher.py  # foreground, pinned generic Kitsune annotation heartbeats
│   ├── task_attempts.py   # preparation-only execution intent/receipts and facade
│   ├── task_graphs.py     # pure bounded DAG/criteria/source manifests
│   ├── execution_v2.py    # explicit graph upgrade, planning review/preparation
│   └── repositories.py    # immutable checkout/base facts and bounded local verification
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
safety contracts and the deferred worker/composer coding backlog. The near-term
priority is existing-session observation/grouping and generic presentation before
further worker/worktree implementation; initial declarations/publishing are now
implemented, while extraction and native lifecycle/scheduling remain open.

## Current Djinn persisted-evidence observation

The native product supports this route through `coven observe djinn`;
`target/debug/coven` remains an explicit development invocation only.
Current native acceptance uses controlled producers, not an actual Djinn instance.

```sh
coven observe djinn --executable /absolute/path/to/trusted/djinn \
  --folder /absolute/path/to/session-folder --json
```

One explicit trusted binary, one fixed native `session observe` call, bounded
output and supervised cleanup; no workspace, PATH selection, help/status fallback,
group enrollment or readiness upgrade. Recorded folder metadata/events are not
live status, job success or authorization. The binary is not sandboxed and can
ignore arguments. Exits: 0 observed, 1 unavailable, 2 incompatible (unlike native
Djinn's exit 0 for any generated report). See
[DJINN_OBSERVATION_CONTRACT.md](DJINN_OBSERVATION_CONTRACT.md) for fields, bounds,
identity checks and trust limits. Help-only worker readiness remains unchanged.
Requires a Djinn build containing the new command; checkout validation used
`../djinn/target/debug/djinn` without replacing the installed binary.

## Current Djinn live observation

```sh
coven observe djinn-live --file /absolute/private/RUNTIME_UUID.json \
  --runtime RUNTIME_UUID --instance INSTANCE_UUID --session SESSION_ID --timeout 1 --json
```

Use actual explicitly inspected IDs from one selected private artifact; no scanning
or automatic following. Requires producer opt-in at normal startup of a **new
feature-bearing legacy full-TUI build**, not an old installed UI selected by
`DJINN_UI_BIN`. No automatic installer, app restart, launch, HTTP, folder fallback,
group enrollment or readiness upgrade. Fresh source samples establish only legacy
runner busy ownership, never idle, health, progress or job completion. Cached reports
cannot renew freshness. See [DJINN_LIVE_OBSERVATION_CONTRACT.md](DJINN_LIVE_OBSERVATION_CONTRACT.md)
for manual setup, exact selectors/output, expiry, path checks and limitations.
The one-shot observer never enrolls members. To explicitly attach a source to an
existing participant, use the separate [group live workflow](GROUP_LIVE_STATUS_CONTRACT.md).

## Current Kitsune observation

```bash
coven observe kitsune --socket /absolute/path/to/existing-api.sock --json
```

Explicit opt-in selects one existing API socket; no workspace or default endpoint.
The adapter directly requests one existing `session.snapshot` response (protocol
18 only), without discovery, subprocesses, autostart, ping, or events. It writes
no Coven state and creates no group membership. `done` is idle/unacknowledged, not
native completion or job success; native references may be persisted and freshness
is unknown. Nonobserved reports safely return empty agents, not runtime failure
inferences. Exits: 0 observed, 1 unavailable, 2 incompatible.

An optional shadow mode interprets generic foreground-process evidence in Coven,
independently of Kitsune's agent inventory:

```bash
coven observe kitsune --socket /absolute/path/to/existing-api.sock \
  --process-pane w18:p85 --json
```

This selects **one exact pane**, brackets `pane.process_info` with snapshots, and
recognizes only literal `djinn`/`djinn-ui` executable basenames from structured argv.
Its separate report contains `observations`, not `agents` or group participants;
observing alone never enrolls a member. The separate `group add --process-evidence`
path below reads fresh evidence rather than importing that report.
Process brackets use a generic version/protocol/pane-locator projection, ignoring
absent or semantically malformed legacy agent/UI metadata. The entire JSON response
still passes shared syntax and resource limits before projection; malformed JSON
is never accepted. Default observation retains strict legacy validation.
Recognition is executable-family evidence (including help/maintenance modes), not
an authenticated binary, active agent, verified session, or lifecycle fact. No
launch/resume/detection behavior is removed. See
[KITSUNE_PROCESS_OBSERVATION_CONTRACT.md](KITSUNE_PROCESS_OBSERVATION_CONTRACT.md).

Native opt-in comparison characterizes built-in hints against Coven's generic
Djinn-family evidence across one bounded endpoint cohort. Reference syntax only
(replace the uppercase placeholder with an explicitly inspected absolute socket):

```text
coven observe kitsune --compare-agents --socket ABS_SOCKET --timeout 3 --max-response-bytes 1048576 --json
```

It conflicts with `--process-pane`; existing modes are unchanged. Default output
is pretty ASCII JSON, `--json` compact. Mismatches still exit 0 when observed;
comparison grants no active-agent, identity, readiness or replacement guarantee.
No bindings, state writes or built-in disabling follow. See
[KITSUNE_AGENT_COMPARISON_CONTRACT.md](KITSUNE_AGENT_COMPARISON_CONTRACT.md) for
the strict N+2 query contract, privacy, categories and extraction decision matrix.

**Read method does not mean zero server-side effects:** existing Kitsune dispatch
housekeeping may expire metadata, drain events, sync titles, or create a default
workspace (potentially spawning a shell). No live connections were run for this
documentation update. See [KITSUNE_OBSERVATION_CONTRACT.md](KITSUNE_OBSERVATION_CONTRACT.md)
for exact output, bounds, socket privacy checks, and CLI error semantics.

### Group already-running agents and publish optional labels

Use an explicit existing Coven workspace (or separately initialize a **new dedicated
directory** with `coven init /absolute/path/to/new-group-workspace --blank`, never
an app repo or current workspace). Replace `WORKSPACE`, `ABS`, and the pane example:

```sh
coven observe kitsune --socket ABS --json
coven group create WORKSPACE auth-review --name 'Authentication review'
coven group add WORKSPACE auth-review --member backend --role 'Investigator' --socket ABS --pane w18:p85
coven group list WORKSPACE --json
coven group publish WORKSPACE auth-review --socket ABS --source coven --duration 60 --json
```

By default, select current pane IDs from the observed **agent** inventory,
including agents without native references. Alternatively, explicitly bind one
current pane using Coven's Djinn-family process evidence, even if absent from that
inventory:

```sh
coven observe kitsune --socket ABS --process-pane w18:p85 --json
coven group add WORKSPACE auth-review --member backend --role 'Investigator' \
  --socket ABS --pane w18:p85 --process-evidence
```

Use `--rebind` if that member already has different stable evidence, and optionally
`--expect-revision '<current group token>'`. Roles are display-only. Both add modes
read fresh evidence and send no UI writes; publishing is a separate foreground run.
Process add/rebind atomically promotes schema 1 of `state/groups.json` to schema 2
(mixed bindings allowed), never reducing schema 3; older readers reject it.
UUIDs/counters and `groups-v1` token format/CAS
are preserved; removing process members or snapshot rebind never downgrades it.
Default snapshot binding remains unchanged. There is no saved-report import,
automatic enrollment/fallback/rebind, native identity or execution grant.

Before any reservation/write, each publication cycle reobserves all process members
via current panes and bracketed process reads: one initial snapshot + three reads
per process member share a 3-second deadline and aggregate 1 MiB read budget.
Missing/unrecognized/changed evidence stops the cycle; stable same-terminal moves
are allowed. PID reuse remains possible, and help/maintenance modes also match;
this is not active-agent truth. Default annotation writes/6000 ms TTL are unchanged.
All process brackets are generic, even in mixed groups. The initial cycle snapshot
is generic only for pure-process groups; mixed and snapshot-only groups retain
strict legacy validation and snapshot-member native-hint checks. Selection uses
validated member provenance before parsing, with no strict-to-generic fallback.
Process observation/add need no annotation opt-in and ignore descriptor semantics;
publication still strictly requires a valid descriptor (present null is an error),
an enabled source and the expected epoch.
First manually enable a dedicated source in Kitsune using array syntax:
`[annotations]` with `sources = ["coven", "build"]`. Coven must come first if
`build` previously claimed the target. Sources are not authentication; coordinate
one dedicated source per workspace and do not manually reuse the publisher's source.

Ctrl-C stops refresh, not agents; static labels expire after 6000 ms, with **no lower-source
fallback even after expiry** until source-policy reset. Visibility needs spare
top-border cells (no border in single-pane layouts). Coven never edits config or
reloads/restarts Kitsune. See [GROUPS_CONTRACT.md](GROUPS_CONTRACT.md) for exact
schema, rebind/CAS, lifetime, limits, uncertainty, and source-priority caveats.
Kitsune/user retain layout/rendering/clipping/quotas; labels prove no native or job
status and grant no terminal control. The one-shot observer remains read-only.
The historical group-live Python offline milestone passed **763 tests**, including 33 opt-in live-publisher
tests, 24 live-attachment authority tests and 34 workflow/status tests, alongside
the existing live/native observers, group publishers, socket aliases and generic
snapshot contracts.
Live-source validation is limited to 54 focused opencode tests, 3 schema tests
and both package Bun typechecks, not the full UI suite. The
[live observer contract](DJINN_LIVE_OBSERVATION_CONTRACT.md#example-usage) records
cross-TS→Python fixture interop; no real UI/model calls have validated the live slice.
[Live group validation](GROUPS_LIVE_VALIDATION.md)
confirms add/publish, visible labels, process exit, timer-driven TTL expiry and
explicit replacement rebind against installed Linux binaries. Existing native/task
safety contracts are unchanged; native lifecycle/execution was not validated.
Those historical live runs used valid real-server snapshots, not artificial
malformed-legacy payloads. The generic snapshot slice changed no existing stored
schema, protocol, API output or authorization; native observation is a separate
additive command/contract and grants no execution authority.

### Explicit live attachment and read-only group status

For an **existing member**, use actual manually inspected source pins:

```sh
coven group attach-live WORKSPACE auth-review --member backend \
  --file /absolute/private/RUNTIME_UUID.json --runtime RUNTIME_UUID \
  --instance INSTANCE_UUID --session SESSION_ID --timeout 1 --json
coven group status WORKSPACE auth-review --timeout 1 --json
coven group detach-live WORKSPACE auth-review --member backend --json
```

First attach atomically opts the same group authority into schema 3; all members
gain `live_attachment: null|metadata`. Older readers fail closed; reads never
migrate and detach never downgrades. Only declarations are persisted, not activity
or TTL. Every attach freshly reads the pinned source, then CAS-checks the preflight
token and participant UUID under lock, even without `--expect-revision`.
Changed pins need `--reattach`; unchanged pins preserve `attached_at`. A changed
stable `group add --rebind` clears an attachment; role/pane/timestamp-only changes
preserve it. Detach needs no artifact I/O and never deletes or controls the source.

Status reads only groups and declared artifacts, writes nothing, and projects
short-lived busy/unknown legacy-runner evidence, not terminal/session equivalence
or execution permission. Individual unavailable rows still permit exit 0; whole
content drift, failed recheck or overall deadline expiry discards all rows and
exits 1. Callers must continue reducing returned lifetimes during delivery/display.
The static publisher never opens attachments or appends activity; its group/role
labels retain 6000 ms TTL. For explicitly opted-in live labels after attaching
sources, use:

```sh
coven group publish WORKSPACE auth-review --socket ABS --source coven --live --duration 60 --json
```

Live mode requires literal `annotations.valid_until_unix_ms: true` on every cycle
snapshot, even for unknown rows; unsupported servers fail without a write or silent
downgrade. Fresh pinned busy text is `GROUP NAME · ROLE · legacy runner busy`.
It carries a 3000 ms TTL ceiling plus an absolute `valid_until_unix_ms` bounded by
the original sample and continuously decreasing client wall/elapsed budgets through
reads, reservation and sends; Kitsune must enforce it after server queueing.
Unknown/unattached/lost/expired/invalid sources yield role-only text and 6000 ms TTL.
The composed busy frame expires as a whole; later refresh can restore role-only
text, never a hidden static layer, lower-source fallback or exit-time cleanup.
All binding, revision, participant/pin, source/epoch and whole-target durable-counter
fences remain; uncertain delivery or any rejection stops without retry/repair.
No reattachment, restart following, native control, idle, health, provider progress,
readiness or completion is inferred. Attachments do not verify terminal/UI sessions.

Live expiry assumes compatible source/server Unix clocks, no disruptive wall changes
and normal monotonic progression. Observed wall/monotonic regressions stop the run;
unknown skew, arbitrary rollback, suspend and hard-real-time pixel expiry are not
guaranteed. Capability discovery is not a clock-quality attestation. See
[opt-in publication](GROUPS_CONTRACT.md#opt-in-live-publication) for arithmetic,
whole-frame expiry and exact live summary fields; the default summary is unchanged.
See [GROUP_LIVE_STATUS_CONTRACT.md](GROUP_LIVE_STATUS_CONTRACT.md) for exact fields,
expiry math, CAS, shared-pin and clock limitations. Offline tests exercise the real
CLI wrapper with private fixtures; no existing groups or native runtimes were
modified to validate the attachment/status slice. Live-publisher coverage uses
private artifacts/fake endpoints, not installed Kitsune or native agents.

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

The native product supports these init flags (no SDK-only setup step):

```bash
target/debug/coven init /absolute/path/to/new-coven --example interactive-demo
target/debug/coven init /absolute/path/to/blank-coven --blank \
  --name "Review team" --agents "Architect,Reviewer" \
  --opencode-command opencode --multiplexer tmux
```

The optional path defaults to cwd. Defaults are `Clippy,Srikanth,Jamal,Kris,Eric`,
`oc`, `herdr`, and `interactive-demo`; `--multiplexer` accepts `herdr` or `tmux`.
`--force` is accepted but never permits overwrite. Native example selection supports
custom directories through the [asset root policy](examples/README.md), as well as
the embedded demo. `--blank` overrides even an invalid example and ignores unused
asset roots. All seven original demo assets remain embedded at build time; explicit
roots override the demo too. There is no Python fallback. See
[native init](RUST_MIGRATION.md#native-workspace-init) for publication and retry limits.

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

Native init deliberately publishes `coven.json` **last**, unlike Python's config-first
creation. It prepares the complete tree, events, dashboard and safe output before
effects, reserves `state/` exclusively and holds its private persistent flock.
Ordinary `WorkspaceGuard` marker requirements are unchanged. Native/native init
has only one reservation winner; Python initial creation is unlocked, so mixed
initializers may both fail, though exclusive finals prevent overwrite. Initialize
one destination with one implementation at a time; no rollback, adoption or stale
reservation stealing is provided.

Durability is operation-scoped: native init syncs files, managed directories, root,
and the ancestor chain **observed missing by that invocation**, through its containing
existing parent. Already-existing ancestor entries are assumed durable. If A creates
the root chain and pauses before reservation, B can see an existing root, win and
report committed after syncing root but not those outer entries; A then loses before
chain sync. Neither necessarily fsyncs the outer entries. Native exclusion is **not
cross-initializer ancestry durability**. For stronger assurance, durably establish
destination ancestry first or initialize with one process. The deterministic unit
pins this exact scope, not actual crash-loss behavior.

Native success exits 0; pre-marker I/O failure exits 1 and preserves partial evidence
with bounded root/phase/current-target/last-completed/partial-file diagnostics.
Deterministic preflight refusals create no root. A known marker link followed by
sync failure retains informational stdout and exits 2; inspect before retrying.
Stdout I/O failure can still exit 1 after effects. Linux-only validation and the
static/cooperating namespace assumptions apply; see the migration notes for bounds
and modes.

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

The examples below use an explicit native development build path; installed
`coven` and the native-only checkout launcher expose the same routes:

| Workflow | Native interfaces | Boundary |
| --- | --- | --- |
| Fresh standalone preparation | `execution enable`, `execution enroll` / `--rebind-source`, `task attempt prepare --intent` | Local preparation only |
| Reviewed graph preparation | `execution upgrade`, `task graph propose`, `task graph decide --decision approve` / `changes-requested`, `task attempt prepare --graph-revision` | Review is not dispatch approval |
| Inspection and escape | Execution history/inspect, attempt list/inspect/cancel, graph inspect/export, execution recover | Historical/source-free contracts retained |

The read-only interfaces are:

```bash
target/debug/coven execution inspect /path/to/workspace
target/debug/coven task attempt list /path/to/workspace --task core-chatbot --json
target/debug/coven task attempt inspect /path/to/workspace '<attempt-id>' --json
target/debug/coven task graph inspect /path/to/workspace --json
target/debug/coven task graph export /path/to/workspace '<graph-revision>' --json
```

Use actual 32-lowercase-hex attempt IDs/graph revisions. Attempt inspect and export
take one required selector, optionally preceded by a workspace by **arity**, even
if the single selector names a marked path. Graph inspect instead recognizes a
marker-bearing workspace first, then an optional revision (default: graph head).
Inspection fully validates retained history before optional current-source reads;
ordinary source failures preserve frozen facts with native-code diagnostics.
Actual resource/work/output/encoding-profile failures exit 1 without stdout.
Export needs one full replay and remains source-independent even with bad current
files, but corruption anywhere in retained history is fatal. No reader writes or
fsyncs, and no lock or source observation grants freshness or mutation permission.
See [RUST_MIGRATION.md](RUST_MIGRATION.md#native-execution-inspection-and-graph-export)
for filtering, precedence, budgets and graph eligibility semantics.

For an existing disposable workspace with prepared authority (native or Python),
inspect the actual attempt ID and execution revision, then substitute them below.
Maintenance never implicitly enables, enrolls, prepares or starts a runtime:

```bash
target/debug/coven execution inspect WORKSPACE --json
target/debug/coven task attempt cancel WORKSPACE '<actual-attempt-id>' \
  --reason "Revise assignment" --expect-execution-revision '<inspected-execution-token>' \
  --request-id cancel-review-1 --json
# Separately inspect the current revision before explicit durability confirmation:
target/debug/coven execution history WORKSPACE --json
target/debug/coven execution recover WORKSPACE --confirm-authority \
  --expect-execution-revision '<current-inspected-execution-token>' --json
```

Cancellation exits 0 when confirmed, 2 when a known frozen result is retained but
verification/durability is uncertain, or 1 for an ordinary error. Inspect before
blindly choosing a new key; retry only the original key and exact arguments to
retrieve its authoritative frozen result, not fresh permission. Recovery failures
exit 1. Only for a separately inspected disposable pending directory with **no
control entry**, use `target/debug/coven execution recover PENDING_WORKSPACE --archive-pending --json`.
This archives the whole directory, not repair or reset. Post-rename sync failure
exits 1 with empty stdout and a diagnostic retaining the known destination, without
rollback; see the maintenance contract before using it.

**Historical cancellation liveness limit:** an older valid predecessor may have no admissible
native cancellation successor: exact stored bytes including LF must fit 8 MiB,
combined history 1024 entries, prefix/replay work 256 MiB, and finite output 32 MiB.
Python's 512 KiB cancellation reserve does not guarantee these native limits. No
pruning, limit raising or fallback is provided. New native authoring reserves escape
headroom for active attempts and an approved current graph head; it does not
rehabilitate arbitrary historical control. See the writer contract for the scope.

### Fresh native standalone loop

This is **shell-style pseudocode**, not a copy/paste setup script. Build
`target/debug/coven`, choose a new disposable absolute `$WS`, and create the intent
file before preparation. Assign variables from actual inspected output at each
comment; never reuse a stale execution token after a mutation.

```sh
target/debug/coven init "$WS" --blank --agents operator,reviewer
target/debug/coven task add "$WS" "Review parser" --owner operator --json
target/debug/coven task list "$WS" --json
# Set TASK and TASKS from the actual task ID and task revision above.
target/debug/coven execution inspect "$WS" --json
# Set EXEC from the inspected disabled revision (execution-disabled:v1).
target/debug/coven execution enable "$WS" --expect-execution-revision "$EXEC" --request-id enable-1 --json
target/debug/coven execution inspect "$WS" --json
# Set EXEC to the new inspected revision.
target/debug/coven execution enroll "$WS" "$TASK" --expect-tasks-revision "$TASKS" \
  --expect-execution-revision "$EXEC" --request-id enroll-1 --json
target/debug/coven execution inspect "$WS" --json
# Update EXEC; write $INTENT with this exact TASK, owner operator and explicit criteria.
target/debug/coven task attempt prepare "$WS" "$TASK" --intent "$INTENT" \
  --expect-tasks-revision "$TASKS" --expect-execution-revision "$EXEC" \
  --request-id prepare-1 --json
target/debug/coven task attempt list "$WS" --task "$TASK" --json
# Set ATTEMPT from the actual attempt ID; inspect execution again and update EXEC.
target/debug/coven execution inspect "$WS" --json
target/debug/coven task attempt cancel "$WS" "$ATTEMPT" --reason "Revise assignment" \
  --expect-execution-revision "$EXEC" --request-id cancel-1 --json
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
target/debug/coven task attempt cancel '<attempt-id>' --reason "Revise assignment" \
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
Intent/document files are still read and validated before invoking the mutator,
even on replay: preserve their original values, not just their pathnames.
Native authoring reserves cancellation/review-revocation capacity before accepting
history growth; historical Python reserves alone do not guarantee native capacity.
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

### Fresh native reviewed two-node loop

**Shell-style pseudocode:** choose a separate fresh absolute `$WS`. Set `$A`, `$B`
and `$TASKS` from the actual task IDs/revision in `task list --json`. Write `$GRAPH_DOC` using the
[graph document schema](TASK_GRAPH_CONTRACT.md#author-a-bounded-dag), exact task/owner
IDs and explicit criteria, with B depending on A. Prepare no attempts yet.

```sh
target/debug/coven init "$WS" --blank --agents operator,reviewer
target/debug/coven task add "$WS" "Review parser" --owner operator --json
target/debug/coven task add "$WS" "Reconcile findings" --owner reviewer --json
target/debug/coven task list "$WS" --json
# Set A, B and TASKS; write GRAPH_DOC with those exact IDs and owners.
target/debug/coven execution inspect "$WS" --json
# Set EXEC from the disabled revision.
target/debug/coven execution enable "$WS" --expect-execution-revision "$EXEC" --request-id enable-1 --json
target/debug/coven execution inspect "$WS" --json
# Update EXEC.
target/debug/coven execution enroll "$WS" "$A" --expect-tasks-revision "$TASKS" \
  --expect-execution-revision "$EXEC" --request-id enroll-a-1 --json
target/debug/coven execution inspect "$WS" --json
# Update EXEC.
target/debug/coven execution enroll "$WS" "$B" --expect-tasks-revision "$TASKS" \
  --expect-execution-revision "$EXEC" --request-id enroll-b-1 --json
target/debug/coven execution inspect "$WS" --json
# Set EXEC from the current v1 revision.
target/debug/coven execution upgrade "$WS" --expect-execution-revision "$EXEC" --request-id upgrade-1 --json
target/debug/coven execution inspect "$WS" --json
# Update EXEC to v2.
target/debug/coven task graph propose "$WS" --document "$GRAPH_DOC" --expect-tasks-revision "$TASKS" \
  --expect-execution-revision "$EXEC" --request-id graph-1 --json
target/debug/coven task graph inspect "$WS" --json
# Inspect manifest/criteria; set GRAPH, DIGEST and EXEC from this report.
target/debug/coven task graph decide "$WS" "$GRAPH" --decision approve --reason "Reviewed criteria" \
  --expect-graph-digest "$DIGEST" --expect-tasks-revision "$TASKS" \
  --expect-execution-revision "$EXEC" --request-id review-1 --json
target/debug/coven execution inspect "$WS" --json
# Update EXEC; either node may be prepared, even B before A.
target/debug/coven task attempt prepare "$WS" "$B" --graph-revision "$GRAPH" \
  --expect-tasks-revision "$TASKS" --expect-execution-revision "$EXEC" \
  --request-id prepare-from-graph-1 --json
target/debug/coven task attempt list "$WS" --task "$B" --json
target/debug/coven execution inspect "$WS" --json
# Update EXEC. Revocation has no tasks pin and does not cancel B's attempt.
target/debug/coven task graph decide "$WS" "$GRAPH" --decision changes-requested --reason "Revise criteria" \
  --expect-graph-digest "$DIGEST" --expect-execution-revision "$EXEC" --request-id revoke-1 --json
target/debug/coven task graph export "$WS" "$GRAPH" --json
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

## Immutable repository registration

Both routes, including inspect's `--verify`, are also available through the explicit
native `target/debug/coven` path; see [native observation](#native-bounded-local-observation).

Register one existing checkout and an independently reviewed full commit ID;
the optional workspace defaults to `.`. Registration is separate from task/graph
preparation and grants no execution or Git mutation permission.

```bash
coven repository inspect /path/to/orchestration --json
coven repository register /path/to/orchestration \
  --path /path/to/checkout --base '<full lowercase commit ID>' \
  --delivery-target refs/heads/main \
  --expect-repository-revision repository-unregistered:v1 --json
coven repository inspect /path/to/orchestration --verify --json
```

`state/repository.json` pins checkout locators, base commit/root tree, and intended
target immutably. Dirty changes are allowed, excluded, and never copied. Default
inspect is source-free: no Git, lock, or fsync. `--verify` uses bounded local Git
(2.48+) and retains stored facts/revision if the checkout becomes unavailable.
Only the commit/root tree are validated; complete checkout availability remains
unestablished. Exact retries use the original syntactic request and original
unregistered token without reopening Git or the checkout; different inputs conflict. Exit 0 means
committed/readable, 1 prepublication failure or unavailable inspect, and 2 uncertain
durability. There is no reset/replace/unregister, worktree provisioning, task binding,
or automatic M2 rollout. See
[REPOSITORY_REGISTRATION_CONTRACT.md](REPOSITORY_REGISTRATION_CONTRACT.md) for
normalization, conservative Git exclusions, trust limits, and retry semantics.

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

The **native Rust product** supports checkpoint decisions as well as inspection
and inert next-action guidance:

```bash
target/debug/coven checkpoint inspect /path/to/workspace plan-approval
target/debug/coven checkpoint inspect /path/to/workspace plan-approval --json
target/debug/coven next /path/to/workspace
```

Inspect defaults to cwd/`plan-approval`; its first operand selects a workspace only
if the expanded path has a `coven.json` marker. Next instead accepts one explicit
optional workspace, defaulting to cwd. Neither searches ancestors. Inspect's
`--json` is optional and repeatable (including unambiguous prefixes); flags must
not split the positional group. Both retain the bounded UTF-8/finite-value profile.
Next validates the whole messages log and its checkpoint references before events,
then selects the last chronologically sorted nonapproved request. Without one, it
selects the last sorted lowercase `*.md` with exactly `approval_stale`,
`approved_unbound` or `decision_invalid`—not general pending/review discovery.
No selection means a status/dashboard suggestion, not dashboard publication.

**Terminal warning:** next prints literal root/path text; ANSI, bidi and other
controls may render under the existing literal-output policy. Suggested commands
are inert text, not sanitized/quoted execution instructions. Native
`coven` supports `approve`, `suggest` and interactive `review`, including real legacy
text/Enter notifications outside the workspace lock.
These read-only compositions acquire no locks, fsync nothing and write no decisions,
dashboard or events; they grant no approval/execution authority or coherent snapshot.

The following native decision-writing examples use placeholder snapshot tokens:

```bash
target/debug/coven checkpoint inspect /path/to/workspace plan-approval --json
# Substitute the inspected snapshot token; scope tokens are not write tokens.
target/debug/coven approve /path/to/workspace plan-approval --expect-revision '<snapshot revision>' --json
target/debug/coven suggest /path/to/workspace plan-approval "Please add QA evidence" --expect-revision '<new snapshot revision>' --json
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

Here “native” describes the inspected adapter. The Rust product also implements
the inspector through `target/debug/coven worker readiness`, including both output
modes and every flag; unqualified `coven` remains Python until retirement.

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

### Overview

The native product supports the complete reviewed cold-launch workflow below,
not merely inspection or a no-effects framework. Examples use an explicit build path.
Linux implementation acceptance
does not close macOS, finite-profile, packaging or oracle-retirement gates.

**This authorizes unrestricted runtime startup, not read-only work or task
dispatch.** Current pane adapters cannot sandbox tools or prove worker acceptance.
Native tools may still mutate files according to their independent policy.

### API / Interfaces

The reviewed-runtime batch includes five action writers, `approve` / `suggest` /
`review`, `send` / `agent send` and `down` / `stop` (alias). The subsequent
[legacy lifecycle batch](#native-legacy-lifecycle) completes lead/plan/agent/runtime
routes within the Linux native profile; it does not change reviewed-action authority.

### Example usage

**Pseudocode, not an automatic approval script:** replace `WORKSPACE` and all
angle-bracket placeholders with inspected values. Manually review the immutable
plan and dedicated checkpoint before deciding; approval alone never submits.
These commands may start unrestricted tools and are not a suggestion to test
against installed runtimes or existing user sessions.

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

Use actual IDs/tokens from output; an optional workspace path precedes the action
ID. Action inspection defaults to pretty literal-Unicode JSON; `--json` retains
compact ASCII JSON.
`task list` defaults to human output, with optional `--json`. Enable is explicit and persistent. It drains
admitted cooperating legacy
transports before activation, then blocks legacy launch/restart/bootstrap/pane
delivery paths and agent start/restart/stop. Stop old binaries/direct writers and
inspect old sessions before opting in; activation does not stop running agents.
Existing unactivated workspaces retain their behavior.

### Data Flow

Preparation freezes a closed guidance/config/task/roster/prompt inventory and
literal transport plan into an immutable manifest. The dedicated checkpoint
binds that manifest. Submission rechecks current inputs and exact approval under
the workspace lock, consumes one durable claim, then executes only its frozen
plan **outside** that lock. Edits/revocation before claim block startup; after
claim they do not cancel the consumed attempt. No repository/source/toolchain or
filesystem sandbox is implied.

### Edge Cases

Repeated submissions return the consumed attempt without launching again—even
after failure or a lost response. An uncertain claim never executes. An unresolved
attempt requires inspection and explicit `action acknowledge-unknown` before a
separately prepared/reviewed new action. Nothing automatically retries or deletes
partial resources.

```text
target/debug/coven action acknowledge-unknown WORKSPACE '<action-id>' --expect-claim '<claim-id>' --json
target/debug/coven action stop WORKSPACE '<action-id>' --json
target/debug/coven down WORKSPACE
# stop WORKSPACE is an alias for down; enabled mode uses only the frozen current attempt.
```

Acknowledgement accepts uncertainty, not success or permission to replay. Runtime
CLI commands have a 60-second/1 MiB combined-output profile, clean up only their
direct child, and do not roll back server/daemon effects. Nonstandard child descriptors are
closed or CLOEXEC. Frozen launch uses the captured six-key environment and cwd;
legacy delivery preserves all caller environment and caller cwd. Ordered PATH
fallback is allowed only after proven pre-image `ExecFailed` (`Command::spawn`
error), never timeout, output limit, generic `SpawnError` or an executed exit 127.
Observation probes retain their separate <=30-second supervised group-cleanup policy.

The compiled-fixture acceptance has **16 cases / 176 native CLI calls / 12 routes**,
with low and high (>=2048) inherited-FD sentinels on every call and real five-second
delays on both backends. It uses no Python children, actual providers or tmux
sessions; parent Python oracles remain test-only. Unit uncertainty/crash seams are
not universal power-loss proof. Writer capacities and other finite profiles still
require retirement disposition; see [native reviewed runtime](RUST_MIGRATION.md#native-reviewed-runtime-lifecycle).

`transport_completed` means startup CLI commands succeeded, not healthy workers
or accepted tasks. Status is unverified in this mode; action inspection reports
durable transport facts. Stop distinguishes attempted from successful response
and never resets consumption.

### Related files

See `src/actions/mutation/`, `src/launch{.rs,/executable.rs}`,
`src/runtime{.rs,/transport.rs}`, `src/runtime_command.rs`,
`src/checkpoint_decisions/` and `tests/runtime_launch_workflow.rs`, plus
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
