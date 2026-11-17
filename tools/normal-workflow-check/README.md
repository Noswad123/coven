# Normal Workflow Acceptance

Opt-in, development-only Linux acceptance harness for
[`docs/NORMAL_AGENT_WORKFLOW.md`](../../docs/NORMAL_AGENT_WORKFLOW.md) and
[`docs/INTEGRATION_SETUP_CONTRACT.md`](../../docs/INTEGRATION_SETUP_CONTRACT.md).
Nothing here is installed, automatically run, or imported by Coven production code.
All runtime dependencies are explicit paths. Python 3 uses only its standard library;
Bun imports Djinn's actual built-in `KitsuneAgentStatePlugin` and its native reporter.
The fixture supplies native-shaped event input over stdin, not a provider UI, SDK
runtime, model, or a replacement implementation of the plugin.

## Run

From the Coven checkout, with already-built executables:

```sh
python3 -B tools/normal-workflow-check/check.py \
  --kitsune-bin /absolute/path/to/kitsune \
  --coven-bin /absolute/path/to/coven \
  --djinn-plugin /absolute/path/to/djinn/clients/djinn-ui/packages/opencode/src/plugin/kitsune-agent-state.ts \
  --bun /absolute/path/to/bun
```

Offline dirty-source snapshot build plus the same acceptance checks:

```sh
python3 -B tools/normal-workflow-check/build-and-check.py \
  --kitsune-source /home/jdawson/Projects/kitsune \
  --coven-source /home/jdawson/Projects/coven \
  --djinn-plugin /home/jdawson/Projects/djinn/clients/djinn-ui/packages/opencode/src/plugin/kitsune-agent-state.ts \
  --bun /home/linuxbrew/.linuxbrew/bin/bun \
  --zig /home/linuxbrew/.linuxbrew/opt/zig@0.15/bin/zig \
  --zig-package-cache /home/jdawson/.cache/zig/p
```

Add `--acceptance-runs 2` to build once and execute `check.py` twice against the
same binaries. Each invocation creates and cleans a separate fresh owned run with
new configuration, state and servers. Both exit statuses are reported; failures
are not masked and no report/mutation is retried. Snapshot paths and binary SHA-256
identities are printed before acceptance starts.

The builder explicitly passes its newly built absolute executable paths to the
same check implementation. There is no implicit sibling lookup. Both source
checkouts are copied including dirty source, excluding `.git`, existing build
outputs, `node_modules`, and local metadata. Builds never mutate either checkout.
Snapshot copying is not an atomic transaction with concurrent editors: rerun after
core work settles to qualify that exact revision.

Rust is selected using `rustup run 1.96.1`, Cargo is `--offline --locked -j 4`,
development debug information is disabled, and incremental compilation is disabled.
The qualified Zig version is 0.15.2. The executable and existing offline package cache are explicit; the latter is
passed through `LIBGHOSTTY_VT_ZIG_SYSTEM_DIR`. No toolchains/packages are installed
or downloaded. Cargo/Rustup package/toolchain caches are reused, not copied.
`CARGO_TARGET_DIR`, `TMPDIR`, and Zig local/global caches are run-owned. Kitsune's
hardcoded `vendor/libghostty-vt/zig-out` stays inside its disposable source snapshot.

### Startup-Only Runner

To exercise only `startup_modes(args, root)` with freshly built actual Coven and
Kit binaries, run this from the Coven checkout in a dedicated Python process:

```sh
python3 -B - /absolute/path/to/kitsune /absolute/path/to/coven <<'PY'
import sys
from types import SimpleNamespace
sys.path.insert(0, 'tools/normal-workflow-check')
import check
args = SimpleNamespace(kitsune_bin=check.absolute(sys.argv[1]),
                       coven_bin=check.absolute(sys.argv[2]))
with check.owned_run() as root:
    check.startup_modes(args, root)
PY
```

Use `/home/jdawson/.cache` as `XDG_CACHE_HOME` (or leave it unset with that HOME).
The default receiver under `RUN/auto/v/coven/integration/i/KEY/r.sock` is exactly
107 filesystem-encoded bytes at this cache base with the current `nw-` run names.
Longer paths are refused before startup, not shortened with a `/tmp` alias.
The source-only fixture exposes only the supplied Coven binary via a run-owned
`bin/coven` symlink ahead of `/usr/bin:/bin`; it writes no runtime table and does
not override the default state root, only its isolated `XDG_STATE_HOME`.

The runner needs Python's standard library, Linux `/proc`, `findmnt`, `/bin/sh`,
and the two actual binaries, but not Bun or the Djinn plugin. Import the module
as `check` with its directory on `sys.path`: the auto-start case also imports
`bootstrap.instance` and `bootstrap.helper_for`, which import helpers from `check`.
For function extraction, retain those imports and all referenced `check` helpers
and standard-library globals. There is no global child registry: `descendants()`
tracks the process tree and `owned_run()` enables Linux subreaping before launch.
Always enclose the call in `owned_run()` in a dedicated process without unrelated
children; its finalizer stops/reaps **all** descendants before deleting the exact
run directory, including on failure or catchable interruption. Each successful
phase also calls `stop_children()`. `passed()` updates module-global `PASS_COUNT`.
No build, install, live config mutation, or 30-second expiry test runs here.

## Assertions

### A2 Source-Native Interop

The independent A2 gate uses the **actual Djinn source producer**, not handwritten
attachment frames, against current-source Coven and Kit. Run from this checkout:

```sh
python3 -B tools/normal-workflow-check/build-hosting.py \
  --coven-source /home/jdawson/Projects/coven \
  --kitsune-source /home/jdawson/Projects/kitsune \
  --djinn-source /home/jdawson/Projects/djinn \
  --bun /home/linuxbrew/.linuxbrew/bin/bun \
  --zig /home/linuxbrew/.linuxbrew/opt/zig@0.15/bin/zig \
  --zig-package-cache /home/jdawson/.cache/zig/p
```

`build-hosting.py` builds both working trees with Rust 1.96.1, `--locked --offline
-j 1`, one shared run-owned short target, debug info/incremental disabled, private
TMPDIR and Zig local cache, and existing package/toolchain/Zig global caches.
It checks Zig 0.15.2, prints binary SHA-256 identities, and installs nothing.
Existing Kit vendor Zig output is reused, not removed. After interop it retains
the target only long enough to run focused Coven native/attachment/facts/navigation/
recovery, Kit navigation/env/helper lifecycle, and Bun attachment/SDK/transport/
selected-session regressions. Runtime and log limits/cleanup match the other runners.

For existing binaries, use `hosting.py --coven-bin /absolute/coven --kitsune-bin
/absolute/kitsune --djinn-source /absolute/djinn --bun /absolute/bun`.

`hosting.py` reuses `check.owned_run`, environment isolation, the actual new-pane
environment probe and Kit API helpers. Only an owned `bin/coven` is added to PATH;
Kit is headless with `sources = ["coven"]`, its default actual Coven helper and a
private short `XDG_STATE_HOME=RUN/v`. The producer is
Djinn `test/coven/hosting-interop.test.ts`, executed by Bun in its opencode package
using existing dependencies and the shared-effect/instance SDK test pattern.
The fixture is skipped unless explicitly enabled by this runner.

Before starting Djinn, a global-retirement preflight sends **32 typed requests**
to the actual Kit API: `agent.start`, `agent.get`, `agent.focus`, `agent.rename`,
`agent.prompt`, `agent.send_keys`, `agent.view.set`, `agent.view.clear` (both
source-scoped and default/global), `agent.wait`, and prompt-with-wait. Targeted
controls use the real pane ID, terminal ID, an unknown selector and an empty
selector. Every response must be correlated and return exactly the
`feature_retired` error path with the fixed method-specific guidance, not target/argument errors or a default
target fallback. All focus IDs and pane-to-terminal mappings are compared after
each refusal. No `MUST_NOT_ENQUEUE` prompt sentinel may appear in either shell's
recent unwrapped PTY output; real shell file canaries before/after confirm generic
`pane.send_input` still works. The fixture has no managed agent or provider.

The next retirement scenario sends **13 additional typed API requests**:
`agent.list`, `agent.read`, `agent.explain`, `server.agent_manifests`, and
`server.reload_agent_manifests`. Read/explain cover the actual pane and terminal,
unknown, empty, and private selectors. The eleven agent requests require exactly
`feature_retired` with method-specific guidance; the two server manifest methods require exactly
`feature_retired` and the fixed `DETECTOR_MANIFEST_RETIREMENT_MESSAGE`, not either
error interchangeably. Every reply is correlated, with no selector echo. The
Agent and manifest API retirement is global, not dependent on external ownership;
both actual owned server modes and focused Kit tests exercise that distinction. This
intentionally replaces the earlier read-facade and ownership-only manifest
compatibility assertions. `session.snapshot` remains
valid with `agents = []`; focus IDs, pane-to-terminal mappings and any historical
`panes[].agent_session` fields must remain unchanged. No synthetic historical
agent session is injected into the real shell fixture; preservation of nonempty
history is covered by the seeded Kit snapshot/recovery-facing unit tests.

The actual built Kit CLI now refuses **locally**, without probing any server.
The five valid forms are `server agent-manifests`, `server agent-manifests --json`,
`server update-agent-manifests`, `server update-agent-manifests --json`, and
`server reload-agent-manifests`. All must exit 1 with the exact global retirement
message; JSON forms retain their command-specific `cli:server:COMMAND` IDs. Three
`--invalid` forms and `reload-agent-manifests --json` separately remain usage exit 2.
No success/refusal from a real API server is accepted as evidence of local refusal.

The canaries also cover eleven valid Agent CLI forms: list, get, read, live explain,
send-keys, rename, focus, start, prompt-with-wait, wait without a timeout, and attach
with takeover. They require exact local JSON retirement errors and never launch,
attach, resolve readiness or submit input. All six `integration install` target
forms require the existing local human-text `feature_retired` error, not invented
JSON output. Status/uninstall are cleanup-only and are not executed by this gate.

All **26 CLI invocations** (22 retirement refusals plus four usage errors) use
separate CLI-only HOME/XDG roots. Listening Unix
canaries occupy the explicit `KITSUNE_SOCKET_PATH`, its derived `-client.sock`
address, and explicit fallback `KITSUNE_CLIENT_SOCKET_PATH`. They accept/serve
nothing: even a protocol probe queues a connection and fails the check. The gate
requires **zero connections** after each exited command and at the end, plus an
unchanged complete CLI directory tree (bytes, modes, inodes and modification times,
excluding access times). A stale CLI-only catalog environment variable contains an
invalid scheme, with no TCP/HTTP endpoint. Its production consumer no longer exists;
this is no longer a catalog preflight or update-callback-order test.

Before starting actual Kit, the gate positively seeds the dev build's old
`XDG_STATE_HOME/kitsune-dev/agent-detection/remote/codex.toml` and `status.toml`.
Their complete trees must remain unchanged through startup, API/CLI refusals,
source-native hosting and final server shutdown. Static-loader unit tests separately
prove the old cache/status are ignored, local overrides take priority over bundled
rules, and legacy explain fields remain null/false. The composed external shell
fixture does not infer static-rule selection from provider activity. Static rules
remain for explicit offline diagnostics; normal/handoff live detector producers,
inference loops and Agent workflow execution are now deleted in the qualified Kit
source. The gate validates resulting runtime contracts, not a negative inference
from the absence of producer messages in bounded warning-level logs.

The normal hosting, `check.py` startup-mode and `bootstrap.py` setup fixtures no
longer advertise obsolete `[agent_detection] manifest_check = false` or
`[session] resume_agents_on_restore = false` settings. Retired manifest, resume and
CJK agent-filter compatibility diagnostics are covered by focused Kit config tests,
not by writing obsolete knobs into the normal source-native workflow.

Generic `pane.read` still returns actual shell text for visible, recent, recent
unwrapped and detection sources. Generic `pane.process_info` returns only the
owned shell's process observation and the snapshot retains the supported Linux
unclassified foreground-cohort descriptor plus `terminal_navigation.v1`. The
following actual producer-backed focus scenario proves generic navigation still
executes successfully. Read APIs may perform ordinary headless housekeeping; the
controlled shell fixture has no agent timers or active provider. Explicit offline
`agent explain --file` is not a live facade and is unchanged, not exercised here.

`check.py`'s helper-loss phase now expects the explicit `agent.list` `feature_retired`
refusal alongside an empty snapshot inventory. Its native-facts and 30-second
activity-evidence-expiry behavior is unchanged. Running the shorter hosting gate
does **not** qualify or claim execution of that separate full 30-second gate.

Another preflight checks **16 event/installer retirement requests**: six installer
targets, four mixed subscriptions (retired status first/last, valid substring or
semantically invalid regex), and six status waits across real/private/empty pane
selectors. Event requests must return one correlated refusal frame and close,
without initializing a stream or allowing generic-probe errors to win. The four
pane-hook ingress methods alone retain `external_agent_management` with Coven:
report-agent, report-agent-session, clear-authority and release-agent. There is no
either-code acceptance.

The generic metadata scenario subscribes successfully to `pane.updated`, sets a
token, and checks **eight atomic mixed-frame refusals** for retired presentation
fields. Tokens stay unchanged and the refused high sequence cannot consume the
next generic sequence. Duplicate-sequence suppression, an actual short token TTL,
replacement, clear and generic update events remain functional. The supported
`pane.wait_for_output` substring matcher succeeds. A generic `events.wait`
workspace-created matcher explicitly remains `unsupported_event_wait_match`, not
silently counted as working. No retired metadata is used to render agent identity.

After the Coven-hosted scenarios shut down cleanly, a second actual owned Kit
server starts with `sources = []`, no Coven PATH route, no helper and no integration
state. **Fourteen typed requests** cover global Agent/manifest/status/install
retirement plus all four pane hooks, which return `feature_retired` in this mode.
Its generic PTY canary, text read, process inspection and exact-generation focus
work, with an empty agent inventory. It is stopped through its own API before
subreaping/cleanup. No historical native identity or provider process is fabricated
to seed this case; positive history and handoff-reference retention remain focused
Kit tests.

The seven following source-native hosting scenarios assert:

1. Two roots are created by the actual session service through the actual SDK and
   in-process server fetch, in an isolated local test database. Only A is loaded
   via `createSelectedSessionLoad`, with actual metadata and messages hydration.
   The load callback selects the actual attachment observer/controller, using the
   actual native plugin factory/registry/SDK qualification. The real factory UUID
   and root ID hash to Coven's one quiet observation: attachment origin, UNKNOWN,
   null activity age, selected local-root hosting, private `producer_reported`
   exact-target binding. B remains unselected and absent. Raw native IDs/targets
   are absent from observations, generic projection and an actual semantic frame.
2. Real one-second observer heartbeats keep the real five-second lease alive for
   more than six seconds; quiet activity stays null and association revision stable.
3. Actual Coven CLI focus executes against the producer association without any
   user assertion, moving actual Kit focus to the captured terminal generation.
   A direct Kit wrong-generation focus refuses without moving focus.
4. Status/decision inputs through the actual native adapter/reporter and built
   `report-native` join the same observation, changing origin without an identity
   fork. These event inputs are synthetic; actual provider emission is not claimed.
5. Actual load disposal/reload switches A to B; leaving Chat via load disposal
   releases B. Obsolete association revisions refuse focus; A's native pending
   decision and blocked state are preserved, not acknowledged or fabricated.
6. A test-only pause of the observer's real-cadence callback simulates lost renewal
   without detach. After the actual five-second TTL, hosting disappears, old focus
   refuses, and replay of the saved old frontend callback cannot revive the lease.
   No backend/provider restart is performed.
7. A new loaded selection requalifies; actual plugin disposal triggers the worker's
   higher-revision detach while preserving native pending decisions.

Capture and all attachment/native reporting use the **actual built Coven commands
with the actual pane environment**, never a fake process transport/receiver or
fabricated registry UUID/session-get result. Default internal plugins are disabled
and the native factory is registered exactly once manually. Local fixture config
has no external plugins, formatters or LSP. File control is private, bounded and
atomically published; only sanitized assertion patterns are evidence output.

The qualified producer seam is the attachment worker **controller**, not a separate
Bun Worker/private-RPC handshake. The selected-load callbacks are composed directly
as the main TUI composes them, with no Numen state. No physically rendered Djinn Chat,
provider app/model, full UI bundle/install, live endpoint, user API or user config is
used. Hydration uses actual session messages, not the full TUI sync store. Plugin
reload ambiguity, multiple conflicting frontend targets and physical process death
remain separate unit tests; this gate covers actual plugin disposal and renewal
loss instead. Kit's semantic frame is real headless rendering, not physical pixels.
Focus acceptance makes no provider-liveness claim.

Successful shutdown requires actual Kit `server.stop`, the same receipt generation
released, actual helper reaped and receiver removed, then zero owned descendants.
The subreaper stops/reaps every owned child before exact run-directory removal on
success, failure and catchable interruption. No bulk artifacts use `/tmp` or RAM.

Recorded global live-detector/Agent-workflow source-deletion qualification on
**2026-11-16**, using Kit HEAD `92ce12d2` plus the uncommitted deletion diff and
current Coven/Djinn sources: **12/12 composed scenarios**, including all seven
hosting cases. Main-server refusals were **69 `feature_retired` plus four pane-hook
`external_agent_management`**; the no-Coven server added **14 `feature_retired`**.
The 26 local CLI cases made zero canary connections. The composed Bun fixture
passed 1/1, as did 75 Coven regressions, 185 Kit regressions and
39 Bun producer/selected-load regressions passed. The Coven subset includes the
new deleted-attachment selection/retirement bookkeeping regressions. Three
fixture-only Rust tests were ignored by their normal test annotations. No full
repository suite, Clippy run or forced serial test runner was used. The actual
binary SHA-256 identities were:

```text
coven   679b9ff73f4001c01889c9d1e8ea995aab0b579fee2ed41ae0baa51c2a88c34b
kitsune 37be98c3f095532ce98a1fd910c435bcee03b9310760b9938c6b36f3abf46922
SUMMARY 12 A2 composed scenarios passed
SHUTDOWN verified: actual server.stop, helper reaped, same receipt generation released, no owned descendants
CLEANUP confirmed: all owned children stopped/reaped; run directory removed
```

The builder file was unchanged. A one-off Python launcher ran the actual
`build-hosting.py` and appended the following filters to its existing retained Kit
test-binary invocation (alongside `terminal_navigation`, `integration::env::tests`
and `server::integration_runtime::tests`):

```text
retir
app::api::ownership_tests
app::api::manifest_ownership_tests
api::server::tests
api::subscriptions::tests
api::schema::tests
cli::retirement_tests
cli::agent::
cli::server::tests
integration::tests
api::wait::tests
app::api::command_submission::tests
integration_views::tests
process_info
metadata_tokens
pane_report_metadata
mixed_retired_metadata
terminal_maintenance
handoff
```

This single union counts each Rust test once; it is not added to earlier
overlapping retirement gates or the separately reported full-native 2739 executions
and five ignored fixtures. It includes global Agent/event/installer retirement,
typed CLI-helper no-contact guards, all-platform install refusal and explicit
cleanup fixtures, decode-only legacy DTOs, generic output/event subscriptions,
token metadata sequencing/TTL, history/reference and handoff respawn-policy data,
normal/handoff generic terminal maintenance and shutdown, config diagnostics,
and held navigation/integration APIs. The deleted detector/updater/readiness
engines are not selected or claimed as still running. All fixtures use owned
local resources, not remote services or installed providers. The standard command
above always runs the 12 composed scenarios; its unextended Kit unit filter list
is smaller than this recorded retirement run.

These identities qualify that dirty-source build, not subsequent concurrent edits.
The owned btrfs run, including its retained target, was removed after all children
stopped/reaped; no build/test artifacts were retained. The seeded retired-cache tree
was checked unchanged after actual server shutdown as well as after the API/CLI
cases. This source-deletion gate passed on its first run; no production bug surfaced
and no production module, Kit source, Djinn fixture, or other agent's documentation
was edited for this retirement qualification.

### Composed Adoption Runner

`adoption.py` is the small, independent acceptance gate for the current A1/A4/A5
composition and temporary user-asserted navigation. It does not run `check.py`'s
30-second expiry workflow and needs neither Bun nor a provider plugin:

```sh
python3 -B tools/normal-workflow-check/adoption.py \
  --coven-bin /absolute/path/to/coven \
  --kitsune-bin /absolute/path/to/kitsune
```

Run it in a dedicated process: it reuses `check.owned_run`'s subreaper and exact
cleanup of **all descendants**. Both binary arguments must be existing absolute
paths. Kit receives only `sources = ["coven"]`, no runtime table; production
auto-start finds the supplied Coven through an owned `bin/coven` PATH symlink.
The fixture uses private API/client endpoints, a non-login `/bin/sh`, isolated
HOME/XDG roots, and `XDG_STATE_HOME=RUN/v`. The default receiver address is checked
to fit Linux's 107-byte Unix pathname limit before startup. It pins the real
helper's argv and the actual new-pane environment using the existing probe.
Read APIs may perform Kit's ordinary housekeeping, including spawning owned PTY
shells; zero receiver-side effects from reads are not claimed.

The four numbered scenarios are:

1. Synthetic, inert Djinn envelopes enter only the real `report-native
   --from-kitsune` subprocess and owned production receiver. Actual observations,
   attention and rendered rows prove busy/blocked, preview without acknowledgement,
   applied acknowledgement without decision resolution, matching-kind/request-only
   replies, and a newly rearmed ask. Public/generic projections exclude raw private
   IDs. Facts remain fresh under the unchanged 30-second TTL; no expiry wait occurs.
2. Two actual Kit panes advertise `terminal_navigation.v1`. Exact runtime inspection,
   association preview/apply, receiver epoch/revision and generation checks compose
   with Coven's executed focus. A direct stale-generation focus rejects without
   moving focus. Actual server request logs are scanned for terminal-only navigation
   and absence of legacy focus/input fallbacks. Generic DEC focus reports are allowed.
   Association remains `user_asserted`, with `native_hosting: unestablished`: this
   does **not** complete A2 producer attachment qualification.
3. A1's additional-provider `claude` resume adapter previews, then explicitly queues
   **only `/usr/bin/true`** with an inert dummy selector into an owned generic shell.
   `input_queued` proves typed policy/PTY delivery, not native provider startup.
4. A5 inspects a new owned control workspace, previews an explicit two-entry
   selection with the same `codex` reference tuple, then executes using its CAS token.
   Exactly one `/usr/bin/true` command is submitted; the second entry skips. A new
   CLI process with a fresh revision skips the journaled tuple without another
   enqueue or journal mutation. No scheduler, automatic restore or native exactly-once
   guarantee is inferred. Persisted Kit-reference/import vetoes remain separately
   covered by recovery workflow tests; this runner does not fabricate agent APIs.

For a complete offline build plus this gate and focused regressions:

```sh
python3 -B tools/normal-workflow-check/build-adoption.py \
  --coven-source /home/jdawson/Projects/coven \
  --kitsune-source /home/jdawson/Projects/kitsune \
  --zig /home/linuxbrew/.linuxbrew/opt/zig@0.15/bin/zig \
  --zig-package-cache /home/jdawson/.cache/zig/p
```

This smaller builder uses **current working-tree sources directly**, including
uncommitted production changes, not HEAD or installed binaries. It prints exact
commands and binary SHA-256 values. Rust 1.96.1 Cargo builds are `--locked --offline
-j 2`, with one shared owned short target directory, private TMPDIR/Zig local cache,
and existing Cargo/Rustup/Zig global/package caches. Zig must already be 0.15.2.
Kit's pre-existing `vendor/libghostty-vt/zig-out` is reused/retained, not deleted.
There are no toolchain installs or downloads. Concurrent source edits are not
snapshotted atomically; rerun after production changes settle.

The retained target is used to compile and run focused Coven native/navigation,
interactive-session, recovery, native-asset and CLI groups, the two interactive/
recovery workflow binaries, and Kit terminal-navigation, generic command-submission,
integration-view/env and helper lifecycle tests. Unit groups run together at the
Rust test runner's default parallelism; no forced serial guard masking is used.
Only compilation receives the 4 GiB build-artifact limit. Runtime test/application
children receive 32 MiB per-file limits, and build logs are monitored to 32 MiB.
All waits are bounded. On success, actual `server.stop` must reap the helper,
release the same receipt generation and remove the receiver. Failure/interruption
still stops/reaps all owned children before removing the exact runtime/build roots.
No user configuration, live server, provider executable, credential or model is used.

Pi/OpenCode generated native-emitter E2E remains a separate acceptance layer under
`NATIVE_PROVIDER_EVENTS_CONTRACT.md`; this runner adds real Kit/Coven composition,
not a claim of live provider activation or a repeat of that generated-asset test.

### Full Workflow Assertions

The finished setup/bootstrap path is primary. The checks below describe harness
assertions, not a claim that this revision passed the full gate. The targeted
startup-fixture update was syntax-checked only; binary-backed startup and full
integrated acceptance must be run separately against freshly built binaries.

- Actual `integration plan` selects the current built Coven executable and default
  XDG config path without creating files. A separate HOME-only plan checks default
  config selection too. Before/after tree comparisons cover file bytes, types,
  modes, inodes and modification times (not read-access times).
- Plans and unapplied enable/disable preserve an existing complete config tree.
- Actual `integration enable --apply` preserves unrelated settings, source order
  and comments. Generated literal arrays contain only the current executable,
  `integration runtime` / `agent report-native --from-kitsune`, and the state root.
  No API socket, receiver socket or workspace is pinned. Repeated enable is a
  byte/inode/tree-preserving no-op, and setup does not create runtime state.
- After custom-root enable, bare `integration plan` selects the recorded executable,
  root and generated command arrays without creating default-root state or changing
  the config tree.
- Fully default enable/disable also appends a missing Coven source and restores
  the exact source-only preimage, without a state-root/config/executable override.
- The runtime tests use default XDG config selection and current executable
  selection. Only `--state-root RUN/r` is overridden, to keep dynamically generated
  receiver paths below Unix address limits. Kit API/session paths are isolated
  fixture inputs, not generated configuration pins.
- Two simultaneous production Kit servers use the same generated config, state
  root and executable. Both server processes are spawned against a fresh state
  root before waiting for either API or bootstrap readiness. Actual bootstrap creates separate mode-0700 workspaces,
  private mode-0600 receivers, and active generation receipts. `/proc` checks each
  direct Kit-owned `coven integration runtime` child.
- Actual new PTY panes receive the generated reporter/source/external ownership,
  replacing inherited poison. Two real imported Djinn plugin instances use their
  respective unmodified pane environments and report working versus idle to
  different servers, without cross-instance events.
- All five actual provider CLI reporters, `agent report` / `report-extension`, use
  `--from-kitsune` **without** a state-root override. Receipts appear only in the
  selected Kit's rendered view. No provider or installer is run.
- Actual native reporter CLI rejects mismatched root, wrong inherited source and
  another selected executable, without mutating runtime state.
- Generated bootstrap clean shutdown releases its receipt after receiver cleanup
  and helper reaping. A clean actual Kit restart at the same API path reuses the
  released instance directory with a new generation. A primary clean restart
  clears isolation-only volatile rows before the single-root expiry test.
- Default (no sources) and explicitly presentation-only startup, with
  `[integrations.coven] auto_start = false`, create no helper, runtime state, or
  live integration lease. Real new PTY panes strip inherited integration routes
  and external-management variables.
- Source-only `sources = ["coven"]` with no runtime table or auto-start override
  discovers the supplied actual Coven on isolated PATH and starts a direct
  production `integration runtime` helper with the default XDG state root. Its
  exact argv, live lease, receiver socket, and new-pane native reporter argv,
  source, API identity and external ownership replace inherited poison. Actual
  Kit shutdown closes helper stdin, reaps Coven, removes the receiver, and leaves
  the same generation's receipt released.
- The old advanced explicit `agent run` socket/workspace configuration remains
  covered by a cheap independent shutdown test; it is not the primary workflow
  and does not duplicate the 30-second expiry wait.
- Actual `pane.report_agent` rejects legacy status with
  `external_agent_management`, both before and after helper loss.
- Native fixture input goes through the imported Djinn plugin, its real child
  spawn, actual `coven agent report-native` stdin validation/RPC, the actual attached
  receiver/presenter, Kitsune's production handlers/store, and production rendering.
- Real semantic-frame client output contains attributed Coven / Djinn rows with
  working, blocked, idle, and expired-unknown evidence. Assertions match a provider
  label and its adjacent detail line, not unrelated rows or terminal output.
- Permission and question asks block; **both native cancellation event names**
  clear their matching pending decisions and reveal still-fresh working evidence.
- Child and unestablished-root input cannot change the established root's idle
  state. Private native IDs and request IDs do not appear in the checked view rows.
- A real Codex reference-only hook receipt remains unknown; no Codex process runs.
- Real 30-second native evidence expiry occurs with Coven still alive. Presenter
  repainting does not renew the Djinn row's evidence.
- SIGKILL of Coven causes actual five-second Kit lease expiry and rendered
  `Unavailable`, without helper restart or restored legacy status authority.
  Further plugin input after transport loss remains nonfatal and actual Kit agent
  inventory remains empty. A separate plugin instance with an invalid-API route
  points at a private listening legacy canary: it receives no connection, proving
  no legacy socket fallback attempt. The main plugin keeps the real API identity.
  Then actual `server.stop` succeeds.
- After SIGKILL, the active receipt and stale receiver remain unchanged. An actual
  Kit restart remains `Unavailable`; a second runtime CLI diagnoses
  `integration_socket_conflict`, and a reporter diagnoses an inactive generation.
  No automatic socket unlink, repair, state adoption, or sibling-instance fallback
  occurs. A second Kit instance stays live throughout helper loss and crash refusal;
  its active lease, empty native view and instance tree remain untouched. Only final
  harness cleanup removes the crash artifacts after children stop.
- Actual default-config `integration disable --apply` refuses a later user edit
  without changing config or setup receipt. Restoring only that harness-owned edit
  permits exact original bytes, source list, comments and mode restoration.

Each completed gate prints `PASS`, followed by the exact count in `SUMMARY`.
Any failure is nonzero; cleanup is still run.
The two clocks are not shortened or mocked. Typical execution includes at least
30 seconds of real expiry waiting, in addition to source build time.

## Isolation And Cleanup

The harness requires Linux `/proc`, child subreaping, `findmnt`, Unix sockets,
`/bin/sh`, and an existing disk-backed cache parent. It creates a unique private
directory under `${XDG_CACHE_HOME:-$HOME/.cache}/agent-test-runs`, verifies the
filesystem is not RAM-backed, and checks at least 12 GiB free before work. Run roots
and socket parents are mode 0700. Very long cache paths are refused for Unix socket
address limits. No artifacts go to `/tmp` or `/dev/shm`.

Every application starts with an allowlisted isolated environment: private HOME,
XDG directories, session name, API socket, and client socket. The primary path
leaves `KITSUNE_CONFIG_PATH` unset to test default XDG selection. Shells are
explicitly `/bin/sh` in non-login mode. Bootstrap creates the primary minimal
receiver workspaces itself; only the advanced compatibility fixture initializes
its receiver workspace manually.
No hooks/providers are installed; no agent launcher, user service, real user
configuration, or existing user socket is used. No installed app is invoked.

Runtime file sizes are capped at 32 MiB. Build logs are monitored against a 32 MiB
cap; individual build artifacts have a separate 4 GiB limit. All command, build,
API, event, render and cleanup waits are bounded. The small fixture event stream
and latest in-memory frame are bounded; there is no frame history or raw event log.

Cleanup is registered before any build/application children start. Normal success,
failure, SIGINT and SIGTERM stop descendants, including PTY-created process sessions
and orphaned reporter children, reap them via Linux subreaping, verify no owned
descendants remain, and only then delete the exact owned run tree. Cleanup failure
is an error and does not silently remove the tree while children remain. Successful
cleanup prints `CLEANUP confirmed`. SIGKILL/power loss cannot run cleanup; do not
remove another run or saved session by age alone.

## Test Level

This is **actual Linux headless production server startup and rendering**, not a
direct-handler proxy or a source-only startup claim. Startup was reviewed against
Kitsune's `src/main.rs` (`server` dispatch), `src/server/headless.rs` (API readiness,
helper ownership and shutdown), `src/server/socket_paths.rs` (client derivation),
`src/session.rs` (explicit session/socket precedence), and `src/integration/env.rs`
(new-pane environment). It never calls the default auto-detect/client launch path.
The small client speaks protocol 18/bincode 2 semantic frames and fails closed on
version/encoding mismatch. It displays no real terminal and executes no received
notification/clipboard/title payloads. A deliberately wide private sidebar makes
the evidence fields visible rather than asserting clipped strings.

Not certified: macOS/Windows/native-provider UI behavior, real SDK event emission,
physical input/client-pixel acknowledgement, complete retention/queue/replay bounds,
restore-reference policy, malicious endpoint admission, or the full repositories'
unit suites. Installer execution/rollback and fixed-route receipt compatibility
remain separate provider-installer tests, not claimed by these five CLI reporter
checks. The cancellation checks prove native-shaped cancellation delivery through the
actual plugin, not that an actual provider UI emits it. Those are separate gates.

Concurrent server spawning permits real startup overlap but does not deterministically
hold either process at registry creation or admission. Core's checkpoint-coordinated
registry contention, privacy/ACL refusal, and monitor-publication cancellation tests
are separate unit-level gates; this harness does not inject those internal failures.
