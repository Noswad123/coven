# Normal Workflow Acceptance

Opt-in, development-only Linux acceptance harness for
[`docs/NORMAL_AGENT_WORKFLOW.md`](../../docs/guides/NORMAL_AGENT_WORKFLOW.md) and
[`docs/INTEGRATION_SETUP_CONTRACT.md`](../../docs/architecture/INTEGRATION_SETUP_CONTRACT.md).
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

### Sidebar Qualification

Current contract: Spaces/Coven/other sources are persisted, independently
collapsible equal-height peers with one-line item summaries. `prefix+a`, via the
existing customizable `agent_selector`, opens generic local Integration details
with source/item/search/wrapped detail scrolling. Tab/Shift+Tab cycles focus;
arrows/`j`/`k`/PageUp/PageDown navigate or scroll; `/` searches; Esc first blurs
search, then closes. Source/item clicks inspect only; Items Enter explicitly
sends the existing publisher selection intent, while Details Enter activates
nothing. No native focus/acknowledgement/input/start/resume/cancel or old agent
navigator follows. Shared server facts and client inspector focus/scroll differ.

Coven normal `item.label` is human-name-only: provider default (e.g. Djinn),
preferring a valid associated explicit manual pane name. V2/v3 have blank
`presentation.label`; sidebar status is icon/animation only, no words/hashes.
Stable opaque IDs remain internal selection identity, not native session IDs or
display labels; duplicate labels have no ID/suffix disambiguation. Normal v1 is
name-only, exactly `{id,label,detail}` with no metadata. Generic third-party
nonempty captions remain supported. Flat fallback Details retains full
`Status: running`/`Status: neutral` for blank captions and detailed state text.
Fresh Working/running and Blocked/attention are distinct from
neutral Idle/Ended (intentionally identical icons) and expired/history/unordered Unknown. Idle is never success;
generic success/error require explicit source assertions, not normal Coven job
proof. Optional `ui.integration_animations` defaults true; local working spinners
do not refresh facts. See the [wire contract](../../../kitsune/INTEGRATION_VIEWS.md)
for additive descriptor-v1 `supported_versions: [1,2,3]`, lease-pinned strict v1/v2/v3
profiles, limits, null/duplicate/unknown-field rejection, whole-advertisement
validation before open and no uncertain-mutation downgrade. Protocol 18 and binary
wire structs are unchanged. Successful-frame fences include identity/query/focus/
layout/scroll, not animation phase; evidence age remains independent.

### Current Structured-Information Increment

The user-approved concise `prefix+a` feature is a docs/source increment over Kit
HEAD `4f4e700a` (26 dirty source files at handoff) and Coven HEAD `a541e9f`,
preserving the prior uncommitted stale-row filter and the new structured-v3 work.
It adds Kit/Coven JSON DTOs/UI and Coven publication-continuity fencing, not
protocol 18, binary wire structs, Djinn source or native payloads.
Descriptor version 1 advertises `[1,2,3]`; Coven
validates the whole list and opens advertised 3, else 2, else 1 (absent list: 1).
Uncertain mutation never causes downgrade/retry. Group present stays exact v1.

V3 optional non-null `information` has only `attention`, `activity`, `context`
string arrays defaulting to `[]`, each at most four nonblank strings of at most
256 UTF-8 bytes, rejecting controls/bidi. Information text is at most 1,536 raw
bytes/item within the existing 96 KiB/source quota. Required `detail` stays at most
512 bytes and becomes Diagnostics when information is present; `{}` is valid with
honest empty placeholders. V1/v2 reject information atomically without renewal or
partial frame/revision/queue changes. Plain v3 and old v1/v2 keep visible Details.
Neither a 128-row ceiling nor accepted per-line maxima promise all-maxima frames fit.

The inspector name appears once in Items, then meaningful Attention, Current
activity, Working context and collapsed Diagnostics. Details-focus `d` outside
search and a visible scroll-clipped header click toggle it; the footer hints it.
Search covers all information/diagnostics, with a hidden-only match hint and no
automatic expansion or activation. Source/epoch/item change, disappearance and
close/reopen reset expansion; ordinary revisions (including sampled age) preserve
it. Background rendering and successful-frame geometry/hit-map fences remain.
Tab/search/Esc and explicit Items Enter/Details Enter-no-action are unchanged.

Attention is fresh reported counts, not exhaustive inventory/global all-clear.
Activity is reported Working/Idle/Blocked or honest missing/reference-only evidence,
never guessed commands or summaries. Age is the selected fact's age, not receipt
time, coarsely sampled once/second by the owner; it can freeze on publisher stall
until lease expiry, not a continuous TUI clock. No task history, cost or native
prompt scope. `Kit project`/`Kit branch` are not native Djinn CWD proof: project is
associated worktree `repo_name`, else workspace human label, and optional JSON
`NewWorkspaceInfo.git_branch` is cached current provenance in snapshot/event
contexts, never label/path/native inference or projection-time Git/filesystem/
process I/O. The entire name/project/branch bundle shares 300 ms qualification,
post-I/O and pre-send endpoint/generation/unique-pane/workspace/tab/association-
epoch/admission/original-deadline guards. Coherently established association,
generation or endpoint loss clears the entire bundle to default name/no context,
not an otherwise current row; private qualification renews no native clock.

Every candidate, including empty/partial frames, brackets current receiver
projection, name/context lookup and actual pre-send. Temporary projection-lock
contention, changed/odd admission or unavailable lookup evidence defers the candidate,
not fake absence or transient provider-name fallback that loses a name-filtered
selection. Borrowed context never survives its original qualified deadline: the
last shown publication is bounded by the minimum membership/context ceiling and
five-second view TTL. Deferral cannot poll-renew it indefinitely. Genuine monotonic
membership expiry still removes a row; coherent explicit removal clears selection.
Safely Unsent pre-send refresh-needed results may rebuild on the same owner and
candidate revision within the bounded ceiling; attempted/uncertain writes never retry.

Read-only receiver observers (legacy inventory/control observations, `attention.list`,
association inspection and `focus.prepare`) do not invalidate admission. Strict
authority/closed-envelope method classification precedes marking; mutations,
including reports, attachments, acknowledgements and association changes, remain
conservatively fenced. Reads renew neither native evidence nor association clocks.

Frame pressure first marks diagnostic omission, then compacts primary attention/
activity and drops context/evidence-age secondary lines with explicit diagnostic
explanation, preserving manual human name. The bounded profile covers 128
escape-heavy rows with 160-byte labels/full 512-byte diagnostics, not arbitrary
accepted 3,072-byte line combinations losslessly. V1/v2 legacy provider fallback
budgets and the Djinn 5/30-second current/history policy remain unchanged.

**Final structured-information qualification: PASS.** The supplied final observed
results are separate qualifications, not one all-tests invocation:

| Qualification | Observed Result |
| --- | --- |
| Latest unified `--sidebar-only`: filtered Coven agent-management units | 150 passed |
| Latest unified: Coven presentation integrations / guarded transport | Nine / two passed |
| Latest unified: sidebar / normal workflow / adoption | 22 / 32 / four gates passed |
| Latest unified: hosting / actual-source interop | 18 / two passed |
| Latest unified: Bun / Python frame-pump regressions | 26 / four passed |
| Separate earlier new-Kit native validation | 2,591 binary tests passed, five ignored; 193 integration tests across 11 targets; one explicit real-Coven roundtrip passed |
| Independent final review after continuity fix | READY, no P1/P2 findings |

The full Kit suite was not rerun in the latest pipeline: the final production fix
was Coven-only, and the latest built Kit application hash matched the earlier
full-suite build. The final observed binary SHA-256 identities are:

```text
kitsune c59f23baf4c305dfd5362b24b90dd20387a44c83a91a81dfa8362ba324efbfe5
coven   ddc1be6f58c98016558e602d63a7a65cd0009c58ceddbf174ba691a630120b4e
```

Continuity coverage includes a real observer storm every 50 ms with 150 ms Kit
inspection, retaining a live-hosted Unknown row after 30 seconds and its chosen
name/details without `Unavailable`. Actual mixed-source profiles 1/2/3 avoid fake
empty frames; coherent source-filter removal still clears selection, and uncertain
writes are not retried. The bounded newest-complete-frame pump checks established
selection on every frame, with no hidden reselection. Only stale-input-guarded,
read-only selection/focus/toggle operations may retry, never Enter/effectful actions.
Inspector coverage verifies all three main sections, collapsed Diagnostics,
Details-focus `d`, visible-header mouse toggling, hidden search hints without
auto-expansion, name once, expansion surviving 1 Hz age updates and collapse on reopen.

Actual cached branch acquisition is unit-qualified only. Composition accepts a
cached branch value or unavailable; it is not physical real-branch acquisition
qualification. The generic JSON cache field adds no scripts/native heuristics or
protocol-18 change. Djinn source and the 5/30-second current/history filter are unchanged.
No latest Clippy, TypeScript typecheck, full Coven suite/full CI or new physical
native-provider/model qualification is claimed. The baseline Linux
`items_after_test_module` Clippy finding is unchanged and was not rerun.

The run owner confirmed all owned roots from five attempts were removed after
children stopped/reaped, and the run cache was verified empty; no artifacts remain.
No source installs, user-server/provider restarts, config changes, commits, user
endpoints, credentials or provider/model execution occurred. This docs-only
finalization runs static diff checks only. When explicitly approved, rebuild/install
Coven and Kit, then restart the Kit server to pin the new Coven generation; this
increment requires no Djinn rebuild and performs no activation itself.

### Current Stale-Row Increment

Historical previous-feature qualification follows, not current structured-v3
validation. The heading is retained for existing links.

This is a Coven-only follow-up to committed HEAD `a541e9f`; the new filter and
tests are working-tree changes, not the historical qualification below. No new
Kit/Djinn source, API or binary-version changes are needed.

Normal current Djinn rows require any unexpired producer-selected-root lease
(including conflicting nonfocusable targets), a valid manual association matching
the fact, or non-ended native Working/Blocked/Idle activity within its 30-second
grace. Quiet attached Unknown survives query failures; every independently current
second root stays visible, regardless of label or typing. Liveness is not idle.
Hide unhosted ended, lifecycle-only Unknown, expired and historical v1 Djinn rows.
A quiet old reporter without activity disappears five seconds after its last
attachment heartbeat; real old native activity can keep it for at most 30 seconds
after that evidence. Never guess replacements by label/provider SID/PID/CWD or
merge opaque IDs. Hiding is not deletion, stopped/success evidence or acknowledgement.

Filter privately before the 128-item cap. Hidden Djinn history does not contribute
private retirement/omission notices, create an Evidence limits ghost or cost the
128th current slot; non-Djinn notices and public aggregate counts stay unchanged.
Public observations and exported `run_with_projection` retain the unchanged
six-field envelope and history IDs, with no public `_publisher_*` hints. Generic
`prefix+a` sees published items, while CLI observation history remains readable.
Frozen legacy serve and other-provider display policy remain unchanged.
Private membership expires across cached models/name I/O and encoding, with
conservative deadline-floor, admission and epoch fences. The transport pre-send
callback runs after encoding, connection and endpoint fencing. Only safely Unsent
refresh-needed results rebuild on the same owner/candidate revision; attempted or
uncertain writes never retry or close. Membership/name/render/animation checks
renew no attachment or native-evidence clock.

**Final unified qualification: PASS.** The latest source build passed all stages:

| Qualification | Result |
| --- | --- |
| Sidebar / normal workflow / adoption | 19 / 32 / four gates passed |
| Hosting and restart | 18 gates passed |
| Coven presentation integration | Eight tests passed |
| Filtered Coven agent-management units | 140 tests passed |
| Explicit guarded-transport tests | Two tests passed |
| Bun status tests | 26 tests passed |
| Actual-source interop fixtures | Two tests passed |

Independent review found no remaining P1/P2 findings. The final inspector assertion
checks exact item labels: a search for `Djinn` may legitimately match that word in
the details of the current `coven sesh` row and does not itself prove a stale row.

The restart fixture uses actual source factories, registry, observer workers and
SDK bridge: old A and new B share a provider SID but have new factory IDs, while
C is a second live root that must remain current. Actual semantic frames showed
both `? Djinn` and `? coven sesh` while B and C were live, then only
`? coven sesh` after C was released. All three public history IDs remained.
The old reporter's lease loss was observed 4.69 seconds after worker termination,
measured from termination rather than its last heartbeat. The separate real
30-second unhosted activity-expiry gate also passed, as did quiet attached Unknown
and failed-query expired Unknown retention.
Bun inputs remain fixture-controlled source evidence, not physical frontend,
provider or model execution. No live user rows were inspected; no install,
restart, config edit or commit occurred. All attempts stopped/reaped owned children
and removed their exact disk-backed run directories; no artifacts were retained.
Qualification used cached Rust 1.96.1, Zig 0.15.2 and Bun 1.4.4, offline locked Cargo,
one CPU and one build job. Full Kit native tests, the full Coven suite, Clippy and
typecheck were not rerun for this Coven-only increment. Kit's binary hash remains
`d4d29589ad61a390d4eb2b11430423a4553dfa5d58fadea7aa84bf9472c8253a`;
Coven's qualified binary hash is
`4abc9e39cec47b4eafbae9b66351f63c0bbb73959126d6623586c25ada5b4775`.

### Historical Name/Icon/Quiet-Root Qualification

**Historical name/icon/quiet-root baseline: QUALIFIED.** Supplied final results are
all green. These are separate qualifications, not one all-tests command:

| Qualification | Final Result |
| --- | --- |
| Kit native validation, including duplicate/icon/name changes | 2,578 binary tests passed, five ignored; 193 integration tests in 11 targets; one explicitly invoked real-Coven roundtrip passed |
| Latest unified actual-server run | 19 sidebar, 32 normal, four adoption and 16 hosting/status-sampling gates passed |
| Coven checks in that unified run | Six presentation integration tests and 127 filtered agent-management units passed |
| Djinn checks in that unified run | 26 sampling/contract/publication tests across three files and one actual-source interop fixture passed |
| Post-binding-fix full-package typecheck | Installed `tsgo --noEmit --incremental false --pretty false`: exit 0, zero errors |
| Post-binding-fix broader Djinn run | 84 tests across nine files passed, including the added bootstrap test (previous broader count: 83) |
| Independent final review | No P1/P2 findings after the semaphore race fix |

The later compiled Kit hash matched the last unified run. After that unified run,
the only feature change was Djinn type binding: capture Session/Status services
outside, provide them inside `InstanceState`, and explicitly annotate the optional-
signal `NativeStatusSnapshot`. Snapshot creation remains per instance, avoiding an
Effect environment leak. The actual two-instance `Plugin.init/list` bootstrap test
passed with default and external plugins disabled to avoid auth. This qualifies
bootstrap isolation, not live Kitsune-plugin initialization; wiring is typechecked
and the service contract is tested separately. The full unified pipeline was not
rerun after this fix; direct full-package typecheck and targeted/broader Bun tests
qualify that follow-up. The corrected builder's `--typecheck` is a recipe, not a
claim that its full pipeline was rerun after the binding fix.

The Coven frame-budget regression is fixed: independently serialized v1/v2 frames
fit the bound, with all 127 filtered units passing. Qualification used cached Rust
1.96.1, Zig 0.15.2 and Bun 1.4.4, offline builds with one job. All owned children
stopped/reaped; cached run directories were empty after cleanup, no retained artifacts.
Clippy was not run this increment; the prior `src/platform/linux.rs`
`items_after_test_module` finding is unchanged. No full CI/full Coven suite is claimed.

Interop qualifies bounded source callbacks, not a full physical native frontend,
remote support or actual provider/model execution. Other provider profiles and
mobile qualification are unchanged. The source addresses known quiet/resumed
selected-root initial/30-second expiry gaps; the user's two existing Djinn instances
(one Unknown) were not live-inspected and are not claimed fixed or already activated.
Unknown remains valid on failed queries, unmount, references or missing activity
evidence. This documentation update runs no builds, tests or live commands,
installs nothing and edits no configuration. Activation of that historical
cross-app baseline needs new Djinn, Coven and Kit builds, then explicit user
approval for restart/new-pane/provider handoff, unlike the Coven-only follow-up.
Historical counts below remain prior baselines, not these final results.

Qualified name-check coverage preserves fresh association, terminal runtime generation
and endpoint binding, already-set/renamed explicit `pane.manual_label`, provider
fallback on failure, duplicate-name ID-based selection, and no OSC title/CWD/old
`agent_name` identity. Publisher-only private hints must not expose raw targets;
read lookup is bounded to 300 ms without blocking the receiver mutex or creating
associations/focus/input/acks. Name refresh changes no activity or authority.

Qualified quiet-root coverage distinguishes pure qualification and attachment heartbeat
from the separate fresh selected-root query of actual local `Session.Service.get`
and `SessionStatus.Service.get` (`src/plugin/native-status-snapshot.ts`). Explicit
busy/retry/idle, max once/second and 1,900 ms bounds, plus selection/unmount/deletion/
reporter-replacement/ambiguity/new-callback fences are required. Canonical native
idle is native-owned state semantics, not process liveness or sparse SDK absence.
Native status setters commit and publish in per-instance/per-session semaphore order,
protecting initial samples against newer callbacks and concurrent ordering; `get`/
`list` stay unlocked for callback reads. Immutable query-start time is preserved
through query/queue/IPC within one total 1,900 ms budget, not reset at completion,
replayed or aged twice. Only successful fresh native queries renew activity via the existing event contract,
never pending occurrences/acks/outcomes. Failures leave clocks unchanged and Unknown
after 30 seconds. Quiet mounted normal full-TUI recovery does not guarantee every
live Unknown disappears; unmounted/event-only/remote/unavailable profiles still
have no automatic status heartbeat.

**Historical prior status/inspector baseline: PASS (2026-11-17).** Fresh actual Kit
and Coven builds passed **16 composed sidebar gates, 32 normal-workflow gates,
four adoption scenarios, 12 hosting scenarios, five Coven CLI/presentation tests
and 14 presentation-module unit tests**. The hosting run also passed its actual-source
Bun interop test. Inputs were native-shaped fixtures and actual-source adapters,
not authenticated provider/model lifecycle execution. No additional provider/mobile
qualification follows.

Separate final Kit validation passed **2,575 binary tests and all 193 tests across
11 integration targets** (five normally ignored binary fixtures); the opt-in
real-Coven roundtrip passed **1/1 separately**. Regression tests cover held-key
isolation, identical-pixel semantic capture changes, failed frame queues, visible
sidebar animation beside the inspector, and animation stopping on last-App-client
detach and restarting on reattach. All 15 failures from the initial contaminated
run passed with inherited startup settings removed and a sufficiently short
disk-backed temporary root. Kit Clippy still reports the pre-existing
`items_after_test_module` finding in `src/platform/linux.rs`; full CI and the full
Coven suite are not implied. Owned children and build/runtime roots were cleaned.

Build and qualify the **current working-tree** Coven and Kit sources, without
installed apps, providers, user configuration, or user sockets:

```sh
python3 -B tools/normal-workflow-check/build-sidebar.py \
  --coven-source /home/jdawson/Projects/coven \
  --kitsune-source /home/jdawson/Projects/kitsune \
  --zig /home/linuxbrew/.linuxbrew/opt/zig@0.15/bin/zig \
  --zig-package-cache /home/jdawson/.cache/zig/p \
  --sidebar-only
```

To exercise the existing normal-workflow, adoption and hosting gates using the
same owned build, append these optional cached-tool/source arguments:

```sh
  --bun /home/linuxbrew/.linuxbrew/bin/bun \
  --djinn-plugin /home/jdawson/Projects/djinn/clients/djinn-ui/packages/opencode/src/plugin/kitsune-agent-state.ts \
  --djinn-source /home/jdawson/Projects/djinn
```

Optionally append `--typecheck` with `--djinn-source` to use the already installed
full-package tsgo without emit or incremental output. It installs no checker; the
post-binding-fix green typecheck above was invoked directly, separately from the
unified pipeline.

For explicit already-built binaries, run `sidebar.py --coven-bin /absolute/coven
--kitsune-bin /absolute/kitsune` in a dedicated Python process. The builder uses
cached Rust 1.96.1 and Zig 0.15.2, `--locked --offline -j 1`, one short owned target,
private TMPDIR/Zig local cache and existing Cargo/Rustup/Zig global/package caches.
It preserves existing vendor Zig outputs and prints binary SHA-256 identities.
Sources are read directly, not atomically snapshotted against concurrent edits.

`--sidebar-only` still builds actual Kit/Coven and runs the composed sidebar gates,
Coven presentation compatibility/module tests, and any optional suites selected
above. It skips Kit unit/native test builds
and runs, including the ignored real-Coven roundtrip and handoff integration tests.
It is a focused check, not full CI. Omit the flag to request the broader runner.
Legacy two-line coordinate descriptions and captures below record historical
expectations, not the current human-name/icon-only contract. The shared row checker
asserts the compact summary, then uses read-only local inspector navigation to
verify full provenance/freshness details; it closes the inspector afterward.

The independent runtime process uses `check.owned_run` subreaping and fresh
HOME/XDG/config, a private API/client socket pair, and only an owned `bin/coven`
symlink. `sources = ["coven", "build"]` exercises normal actual Coven auto-start,
default receiver discovery and the actual pane reporting environment. Seven
synthetic, inert native Djinn roots pass through the actual reporter CLI and Coven
receiver; no Djinn worker/provider/model lifecycle is claimed. Only `build` is
fixture-published through `integration.view.open/replace/poll`; poll explicitly
renews its lease. No fixture opens or steals the actual Coven publisher's lease.

Protocol-18 semantic frames and structured mouse packets exercise simultaneous
Spaces/Coven/build sections, named Atlas/Birch workspaces, fair body allocation,
independent collapse/re-expansion, source-local wheel scrolling, source scroll
retention, and exact label/detail intent routing using an item ID identical to an
actual Coven row. Coordinates come from the latest rendered frame, not old
selector positions or private App state. Healthy bodies have no status wrapper;
empty live bodies contain only `No items` and otherwise blank cells. Blank/header
clicks cannot activate items. Native IDs remain private. Actual resize checks use
heights 9/12/36; exhaustive 0..30 geometry and up to nine sections remain unit tests.
Compact mode checks numbers, its `m` global launcher, and its bottom toggle, and
requires the exact ordered three-cell headers `▾Sp`, `▾Co`, `▾bu`, not merely
different strings or arbitrary non-ellipsis labels. An actual compact Coven header
click must yield only `▸Co`, free its body, preserve native navigation and the
Coven lease, queue no build intent, and restore the original headers on re-expand.

A clean API `server.stop` and actual restart of only the owned server must preserve
Spaces/Coven collapse preferences, native workspace/focus/history data, and a fresh
Coven receipt generation. Volatile native rows and the old build lease must not
survive. Shutdown verifies the helper is reaped, receiver removed and the exact
receipt generation released before descendant cleanup and exact run deletion.

Without `--sidebar-only`, the builder explicitly executes the normally ignored
`integration_views_real_coven_roundtrip` with `COVEN_TEST_BIN` pointing to its own
new binary. It then executes the full Kit binary suite and all 11 native integration
binaries at two test threads, even if sidebar qualification fails. The ignored
opt-in result is reported separately, never counted as a normal-suite pass. No full
Coven suite or Clippy run is implied. Runtime/build logs and deadlines are bounded.
The outer subreaper reaps exited orphan PTY children during native tests so process
existence assertions do not mistake zombies for live processes.

**Historical peer-sidebar qualification: PASS.** The earlier 2026-11-17 source rebuild and
native execution used cached Rust **1.96.1**, Zig **0.15.2**, `--locked --offline
-j 1`, one owned short target and two Rust test threads. All **nine sidebar
scenarios** passed, including exact compact identities and independent compact
Coven collapse. Expanded body heights were **[11, 10, 10]**, with exact `Coven`
header text and no healthy `Available`/selector preamble. The explicit normally
ignored real-Coven roundtrip passed **1/1 separately**. The full native Kit suite
passed **2,556 binary tests plus 193 tests in all 11 integration binaries**, zero
failures, with five normally ignored binary fixtures. Both binary builds emitted
no warnings; this runner did not execute Clippy or the full Coven suite.

The historical peer-sidebar binary SHA-256 identities were:

```text
coven   679b9ff73f4001c01889c9d1e8ea995aab0b579fee2ed41ae0baa51c2a88c34b
kitsune de00b67eeff68262517f9a686bb78419ec7d32b6029789f7f20103b3be9251e2
SUMMARY 9 sidebar gates passed; compact naming passed
```

Bounded ASCII transcriptions of actual semantic frames follow. Blank/repeated
body rows are omitted, and public opaque row prefixes are elided. `v`/`>` represent
expanded/collapsed carets; `<<`/`>>` represent the bottom chrome toggle. All data
is synthetic; no raw native identities or user data appear.

```text
Expanded                         Compact
menu                             m
v Spaces                  new    vSp
 . w                              1
 . Atlas                         >2
 . Birch                          3
[remaining Spaces body]          [remaining Spaces body]
v Coven                          vCo
Coven / Djinn / [opaque]          Co...
Coven: last-reported WORKING ...  Co...
[remaining Coven rows]           [remaining Coven rows]
v build                          vbu
Build task 0                     No
Build detail 0                   [remaining empty body]
[remaining build rows]
                           <<     >>
```

The expanded sample is taken before the empty-build check; compact mode is
captured after the build publisher has explicitly replaced its rows with `[]`.
`No` is the clipped body-only `No items` message, not a header/status selector.
The compact headers themselves have no ellipsis. Exact frame checks occur before
ASCII conversion and retain all original protocol cells.

The build root `~/.cache/agent-test-runs/8582` and independent runtime root
`~/.cache/agent-test-runs/nw-wn01wd_y` were verified btrfs and removed after all
owned children stopped/reaped. Normal shutdown verified receiver removal and
receipt release; clean restart reused the instance with a fresh receipt generation
and preserved only the intended visual preferences alongside native navigation.
No production code, installed app, user endpoint/config, provider or model was
changed or invoked by this rerun. No commits were made.

Historical bug note: the earlier 2026-11-17 run passed eight sidebar scenarios, the explicit
real-Coven roundtrip (1/1), and the full native Kit suite (2,553 binary tests plus
193 integration tests; five normally ignored binary fixtures). That overall sidebar
qualification failed on actual compact naming: at the four-column compact width,
Spaces, Coven and build all render as the identical `▾ …`. Workspace numbers
`1`, `>2`, `3`, the `m` menu launcher and bottom toggle worked. This was reported to
the Kit coordinator without changing production code. The coordinator's subsequent
`sections::compact_labels` fix is qualified by the green run above. Initial driver
failures were fixture sequence reuse, overlong socket paths and deferred orphan
reaping, not production regressions. The handoff fixture uses a short `c` config
directory to retain its unchanged named-session assertions under disk-backed
TMPDIR. Native resizes at heights 9/12/36 and clean restart/persistence all passed.
That earlier Kit build also reported an unused `workspace_drop_indicator_row`
import at `src/ui.rs:84`; the coordinator subsequently made it test-only. No
production fix was made by this harness.

The earlier, failing compact qualification used these binaries:

```text
coven   679b9ff73f4001c01889c9d1e8ea995aab0b579fee2ed41ae0baa51c2a88c34b
kitsune 202c36e791b679334f0c0b8187f9a0d104be8a0c6d6f7eb6dc08d5eb5a52eef3
```

All owned runtime/build trees were removed after every run, including the failing
compact qualification. Actual helper EOF shutdown, receipt release and reaping
were verified before deletion. No binaries were installed and no commits made.

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

The seven following source-native hosting scenarios describe the historical
qualified attachment baseline, not completed qualification of the new status
sampler. In particular, quiet UNKNOWN/null-age captures below predate fresh native
query sampling; attachment-only renewal still cannot refresh activity. Harness
updates are qualified by the separate final 16 hosting/status-sampling gates above;
the historical captures are preserved, not current icon/name/status expectations.

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
