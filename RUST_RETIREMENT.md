# Native production cutover and development-oracle retirement

## Overview

The parent selected **native production cutover with Python source retained as a
development oracle**: one Rust `coven`, no runtime Python runner or fallback.
The checkout's `bin/coven` is now a native-only shell launcher; installed `coven`
is the direct Rust binary. This is not full source deletion or macOS closure.
All 25 Python source oracles and tracked runner bytecode remain in the checkout,
excluded from the Cargo source package and native installed payload.
The user requires the **entire migration before merge**, not a partial merge or
moving the runner elsewhere. No merge request or automatic approval is implied.

Integration update: the user has now explicitly authorized a **local merge to
main**. This supersedes the earlier merge hold, not the recorded compatibility,
macOS, external packaging or development-oracle cleanup qualifications. It does
not authorize a push, release publication or external Homebrew promotion.

### Completed locally vs External checklist

- **Completed locally:** native launcher/help, positive Cargo package allowlist and
  native installed payload. Independent entrypoint (5), package (2) and install (9)
  tests cover runtime guards. After nested harnesses moved to
  `tools/harnesses/group_publisher` and `tools/harnesses/group_live`, an agent
  verified the 300-file crate build and extracted release install with cleared
  environment/empty PATH for help, blank/demo init and refresh. No production
  modules were sacrificed to make packaging pass.
- **Local acceptance:** **1,225 native passes** from split validation (1,021 library,
  86 CLI, 118 integration across 39 suites), not a single fresh all-command green
  run. Five ignored helpers are exercised by parents, not double-counted. Strict
  all-target Clippy and Rust **1.85.0** all-target check pass after the two MSRV
  fixes, without production-policy changes. The final standalone Python suite
  passed **941 tests in 191.978s**; **22 tool tests passed in 156.071s**. See
  [final local acceptance](RUST_MIGRATION.md#final-local-acceptance) for retries and
  saved evidence. Ready for local review/manual native use within documented profiles;
  the prior 1,191-native record remains historical.
- **External:** macOS has not run; actual Homebrew clean install/upgrade is untested.
  The local native-only formula is a candidate; the external tap is unchanged and
  unpromoted. Its remote `main` URL cannot be promoted until a reviewed release.
- **Policy and cleanup:** non-UTF-8 paths and finite caps still need explicit
  disposition. The 941-ID source audit remains 67 mapped / 874 unresolved; that
  blocks development-oracle removal, not native execution of 874 alleged features.
  The migration is being committed for the user-authorized local main merge.
  No push, source deletion or external promotion is authorized by that request.

The command-path audit baseline is 78 Python paths: 27 represented, 51 absent,
plus native-only `execution history`. The six execution writers brought that
to 33 represented / 45 absent. Repository inspect/register, worker readiness and
persisted `observe djinn` brought it to 37 represented / 41 absent. Reviewed runtime
adds 12 routes (five action writers, three checkpoint decisions and four legacy
paths, including the stop alias), bringing it to 49 represented / 29 absent.
The final twelve legacy routes brought it to **61 represented / 17 absent of 78**:
standup, console, lead, bootstrap, agent status/start/stop/restart, plan,
up/start alias and restart. The accepted ten-route Group/Observe batch brought it
to 71 represented / 7 absent: eight Group routes plus Kitsune/Djinn-live.
All seven Session routes are now wired, bringing current representation to
**78 represented / 0 absent of 78**.
Aliases count; native-only history remains extra.
Representation is not full flag, default-output,
profile or platform parity: `task list` now defaults to human output (`--json`
optional); `action inspect`
now defaults to pretty literal-Unicode JSON, with optional compact ASCII `--json`.

## API / Interfaces

These nine **whole-workflow gates** describe implementation evidence, not isolated readers/SDK checkpoints.
Support every existing path/flag or obtain explicit approval for its removal;
parser refusal is not parity. There are **0 absent routes**, but original-obligation
review, compatibility, platform and distribution gates remain open.

| Gate | Current status | Required acceptance before retirement |
| --- | --- | --- |
| [x] Execution authoring (6 routes) | Implemented and validated within the native profile | Fresh standalone and reviewed graph flows, enroll/rebind, both preparation/decision variants, exact replay, cancellation/recovery/export preservation, activation/upgrade death and uncertainty, near-limit cancellation/revocation headroom |
| [x] Legacy workspace/runtime/agent (0 missing) | All 16 previously missing routes represented; Linux native workflow accepted within profiles | Full topology/reuse/replacement/agent order, bootstrap attempts, lead, standup/status, console and foreground attach accepted; compatibility/macOS/oracle/packaging remain open; delayed shell input is not activation-fenced |
| [x] Checkpoint decisions (3/3 represented; 0 missing) | Linux native workflow accepted within profile after full gate | Exact scope/CAS, displayed-snapshot review, committed warnings and actual legacy text/Enter notices accepted; finite profiles/macOS/oracle/packaging remain open |
| [x] Action launch (5/5 represented; 0 missing) | Linux native workflow accepted within profile after full gate | Enable/prepare/submit/acknowledge-unknown/stop, durable claim before actual frozen transport, no replay, drift/crash/uncertainty and frozen stop accepted; profile/macOS/oracle/packaging remain open |
| [x] Repository (2/2 represented; 0 missing) | Linux implementation workflow accepted after targeted acceptance and full gate | Inspect with/without verify, register, immutable exact-byte facts, source-free original-request retries, pinned Git, real-Git acceptance and publication uncertainty accepted on Linux; profile/macOS acceptance still required for retirement |
| [x] Worker readiness (1/1 represented; 0 missing) | Linux implementation workflow accepted after targeted acceptance and full gate | Human/Unicode JSON, eight allowlisted help/version surfaces, eleven unknown semantic capabilities, native supervision/cleanup accepted on Linux; profile/macOS acceptance still required, no execution inference |
| [x] Observe (3/3 represented; 0 missing) | Bounded Linux implementation workflow accepted after full gate | Persisted Djinn, Kitsune and Djinn-live retain explicit pins, bounds, unavailable/incompatible output and no stale fallback; profile/macOS/oracle/packaging gates remain open; observation is not readiness or execution authority |
| [x] Group (8/8 represented; 0 missing) | Bounded Linux implementation workflow accepted after 79-case Group/Observe batch and full gate | Declaration/rebind, attachment/status, static/live publication, revisions, expiry, whole-target counters and uncertain delivery accepted within profile; compatibility/macOS/oracle/packaging remain open |
| [x] Session (7/7 represented; 0 missing) | Implemented on Linux within profile with independent native fixtures | Start/resume, list/inspect/views/watch, prompt-capabilities and prompt; explicit target pins, pure preview/execute boundaries, correlated delivery and no retry; exact original-obligation review/profile/macOS/retirement acceptance still pending |

Execution acceptance has 141 green cases (128 CLI and six uncontrolled shared-lock
races) with empty child PATH. Python is a **temporary parent test oracle**, not a
production fallback. Two-node flows are not maximum-size lifecycle coverage, and
each race does not establish both orderings. Targeted native fault/capacity tests
pass, including death after backup publication and a real coordinator refusal at
the escape-headroom boundary. The historical full pinned gate passed **586 native tests**
(507 library, 32 binary, 47 integration), formatting, strict Clippy and **941 Python
regressions**; see [migration validation](RUST_MIGRATION.md#validation). This completes
the execution-preparation gate, not the remaining workflow or packaging/oracle retirement gates.

Current acceptance reports **144 native CLI cases** with a Rust-compiled producer,
no Python children and real disposable Git; targeted coverage reports **24 repository,
11 readiness, 10 Djinn and 14 identity tests**, plus the actual-binary supervisor
bridge. Closed stdio/FD churn, malformed bootstrap, delayed descendants and reported
cleanup-fault tests were added. These are different layers, not new full-suite totals.
The preceding process-integration full gate passed **659 native tests** (569 library,
37 binary, 53 integration) and **941 Python tests in 195.379s**; retain it as historical.
The historical reviewed-runtime full pinned gate passed formatting, offline locked all-target Clippy
with warnings denied, **763 native passed (663 library + 46 binary + 54 top-level
integration)** and **941 Python tests in 190.822s**. Library also reports two ignored
subprocess-only helper entries exercised by parent tests, not added to the total.
The 28 top-level integration suite results are summed once; nested libtest calls (including the
`process_supervision` library rerun/children) and internal CLI cases are not added.
See [migration validation](RUST_MIGRATION.md#validation). Checked action, checkpoint,
Repository and Worker readiness rows mean accepted Linux implementation workflows, not closure
of macOS, compatibility profiles, packaging, cutover or oracle-retirement gates.
That milestone's coverage was **49 represented / 29 absent**; no production default changed.

Reviewed-runtime compiled-fixture acceptance reports **16 cases / 176 native CLI
calls / 12 routes**, with low/high (>=2048) FD sentinels on every call and real
five-second delays on both backends. It uses no Python children, actual providers
or tmux sessions; parent Python oracles remain. Unit uncertainty/crash seams are
not universal power-loss proof. Former unsupported-writer tests now use invalid
flags for supported routes, preserving all no-effect guards rather than omitting
coverage. Independent review reports no remaining findings after corrections.

### Historical legacy lifecycle acceptance

The legacy-lifecycle full pinned `just check` passed formatting, strict offline locked all-target
Clippy, **863 native tests (752 library + 56 binary + 55 top-level integration in
29 suites)** and **941 Python tests in 189.082s**. Three ignored subprocess-only
library entries are exercised by parents. Nested helpers/reruns and internal CLI
cases are not added. The saved full output and aggregation are recorded in
[validation](RUST_MIGRATION.md#validation); no full tests rerun for docs.

New acceptance: **53 cases / 314 native CLI calls / 12 routes / 1503 compiled-backend
calls / 151 parent Python oracle replays / 16 TTY-mask cases**. No Python children
in fake-backend PATH, actual tmux/Herdr/provider/user sessions or installation.
Separate units prove a real 61-second inherited-stdio foreground child, all PTY
masks and low/high-FD CLOEXEC. Admission lasts through attach while activation
waits and workspace I/O remains available. One generated-shell test executes two
backend cases using inert `/bin/sh`, compiled fake tools, special paths and real
five-second sleeps; Herdr strips final LF through command substitution, tmux retains
file bytes. Delayed input still occurs while exclusive admission is held after
creation admission ends: intentional legacy behavior, not activation quiescence.
The reviewed **16-case / 176-call** acceptance and all full suites remain green.

Current coverage is **78 represented / 0 absent**, not a parity percentage.
Prioritize the original-obligation coverage audit, explicit profile disposition,
macOS, external Homebrew install/upgrade and final cutover validation. Checked workflow
rows mean Linux acceptance within profiles, not all compatibility or macOS gates.

## Data Flow

Current worktree reconciliation checks are recorded in
[migration validation](RUST_MIGRATION.md#worktree-reconciliation-validation).
The useful main-checkout init/refresh additions have been selectively adapted,
not merged over the existing implementation. The Group endpoint-byte preservation
and actual-binary supervisor closed-stdio blockers are fixed. Historical pre-cutover Linux
validation passed **1,191 native tests (998 library + 86 CLI + 107 top-level
integration tests in 37 suites)**, **941 Python tests in 220.389s** and **18 development-tool
tests in 27.407s**, plus formatting, strict offline locked all-target Clippy and
whitespace checks on Rust/Cargo 1.99.1. Three ignored subprocess-only library
helpers and nested libtest runs are not added to the total. The **79-case, ten-route
Group/Observe batch** and actual-binary supervisor bridge pass in that full run.
That run accepted bounded Linux implementation workflows, not macOS, all compatibility
profiles, packaging, oracle retirement or production default cutover. Independent
Session coverage includes 11 reader tests, 15 prompt tests, one multi-scenario
start/resume test and nine new parser units; these are not extra full-suite passes.
Empty-workspace normalization, symlink-parent/double-slash no-follow handling and
sample/summary EINTR exit 130 corrections are tested. Remaining retirement gates
must still close before deleting the retained source oracles.

The launcher `exec`s absolute `COVEN_NATIVE_BIN`, or only the normal checkout's
`target/release/coven` when unset. Missing native selection fails with explicit
build guidance; no Cargo invocation, Python, debug-binary or PATH fallback exists.
Default checkout launchers follow relative/absolute symlink chains, bounded to
40 links, before selecting the single release path. Relocated wrapper copies
require explicit native selection. Neither
`COVEN_COMMAND` nor `COVEN_HOME` is overwritten: they retain generated-command and
`COVEN_HOME/examples` asset meanings, respectively. Installed binaries have no wrapper.

### Original-obligation ledger (not a missing-behavior count)

`tests/retirement/coverage.json` retains **941 original Python method IDs**:
**67 mapped / 874 unresolved**. Mechanism categories are **A: 53, B: 15,
C: 0, D: 873**, not 874 missing implementations or proof of equivalence. Preserve
every original assertion, subtest, setup/no-effect guard, failure boundary and
ordering requirement; exact reconciliation still needs review.

Session-view evidence retains **127 captured calls / 46 deduplicated documents**,
with **107 native value cases / 20 typed-boundary cases**. Typed-interface substitution
is explicit, not invented runtime error parity. Mapping acceptance is assertion-scoped;
no overall gate becomes green from fixture execution or mapping labels. This
inventory now gates development-oracle removal, not product native-execution
coverage or installation.
Recorded `tools/retirement_coverage.py --check --summary` passes;
`--require-complete` deliberately exits **1** while obligations remain unresolved.
These tools are development-only; relocating/deleting the oracle is not coverage.

All 941 original Python tests remain active. Eleven legacy-wrapper methods now
invoke `sys.executable` plus `runner/coven.py` explicitly, recreating the old
wrapper's environment exports. Independent AST review accepted invocation-only
changes with identical assertions, remaining argv, cwd, timeouts and fixtures.
The approved transition record preserves original/current source hashes and all
941 IDs. Hash-transition approval does not shrink the baseline or authorize
production source deletion.

Separate product release checks from development-oracle removal:

- [ ] **Source cleanup:** convert necessary Python-backed oracle assertions into native fixtures/builders
  and native failure/coordination tests. Preserve canonical bytes, replay semantics,
  raw-read contracts and refusal/no-effect coverage; do not simply delete coverage.
- [ ] **Source cleanup:** audit all 941 original IDs to exact assertion-level native evidence and obtain
  independent/parent review; retain unresolved obligations rather than claiming
  coverage from route counts. Human task listing, parser abbreviation/repeats for
  Session/action inspect/handoff and custom examples are now implemented.
- [ ] Explicitly disposition non-UTF-8 paths,
  finite-value/resource profiles and other documented restrictions:
  implement compatibility or record an explicitly approved restriction/removal.
  The user's requirement of full migration is **not** implicit approval of static
  known restrictions.
  This includes the new readiness limit of 65536 unique commands and flags per
  surface: 65537 can fit the byte cap but not the common Value array profile, so
  both renderers return failed-observation/unknown exit 2, not silently lossy JSON.
  Reviewed-runtime fresh writers also cap claims at 128, canonical plans at 128 KiB,
  bundles/projected control at 4 MiB, and reserve 128 KiB/2048 nodes per pending
  report; checkpoint files remain 1 MiB. These are writer-only restrictions, not
  retroactive limits on historical inspection/retry/acknowledgement/stop.
  Legacy lifecycle additionally caps aggregate launch scripts + agent IDs at 8 MiB,
  and bootstrap texts + IDs + relative paths at 8 MiB, checked before append;
  generated scripts are 1 MiB each. Standup staged rows and complete output have
  separate 32 MiB caps. Lead publishes only exact storage bytes admitted by the
  `LegacySnapshot` reader (8 MiB and existing node/depth/container/string profiles).
  These writer/display restrictions leave existing readers unchanged and still
  require explicit final retirement disposition.
- [ ] Validate macOS and install/upgrade behavior; Linux-only evidence is insufficient
  for the supported POSIX distribution. Preserve documented filesystem assumptions.
  macOS was not run: installed SDK unavailable. A new CI job being added has not
  been triggered and is not validation evidence.
- [x] Make the checkout entrypoint and native payload non-delegating. The shell
  launcher selects only a prebuilt native binary; the installed command is the
  Rust binary. Scoped empty-PATH/cleared-environment checks are recorded above;
  final parent full validation and external upgrade verification remain open.
- [ ] **Source cleanup, not product cutover:** remove **all 25 retained `runner/*.py` modules and tracked runner bytecode**
  only after all paths/flags are supported or explicitly approved for removal.
  Check remaining imports, tests and package manifests for runner dependencies.
  Standard-library Python development tools are a separate category, not a runtime
  dependency; retain them only when clearly development-only and non-delegating.
- [ ] Update the **external Homebrew formula (not in this repository)** to install
  the native binary/assets without `runner/` or Python-runner install dependencies.
  Verify a clean install and upgrade do not retain/select an old Python entrypoint.
  The external formula was located/read only; local `packaging/coven.rb` is a
  candidate, not promoted. Actual Homebrew install/upgrade remains untested.
- [ ] Verify generated commands after relocation, including existing workspace
  prompts/manifests and `COVEN_COMMAND` overrides. Native init/refresh default to
  canonical `coven`; legacy checkout-wrapper paths need an explicit migration path,
  not an assumption that moving the binary repairs authored/preserved files.
- [x] Record final local validation after the MSRV corrections: split native results,
  standalone Python, tool tests, strict Clippy and Rust 1.85.0 check pass, with retry
  qualifications preserved in the linked final acceptance record. Public docs
  describe native selection. This does not close external release checks.

## Edge Cases

- Example instructions mentioning `demo.py` describe a **deliverable**, not Coven
  delegating to its Python runner. Retain those instructions as example deliverable
  content, outside the no-Python-runner guarantee; do not install Python merely for
  them. If the intended release promises Python-free example execution too, obtain
  an explicit replacement/removal decision before making that release promise. No example rewrite is
  authorized by this checklist.
- Custom examples are native through the [asset root policy](examples/README.md):
  absolute authoritative `COVEN_EXAMPLES_DIR`, compatibility `COVEN_HOME/examples`,
  installed prefix assets, then Cargo-target-only development fallback. The demo
  remains embedded unless explicitly overridden. This closes the custom-loading
  implementation gap, not finite-profile/relocation/platform approval.
- `scripts/install-native.sh` stages only a prebuilt binary and examples into a new
  absent prefix, never copies/deletes a runner or changes the default wrapper.
  Partial prefixes are preserved with no rollback/adoption/upgrade. All **25 retained
  Python source oracles and tracked runner bytecode remain** in the checkout;
  source deletion has not occurred, but product execution is native-only.
- Cargo's positive package allowlist includes production Rust, embedding assets and
  development `*oracle*.py` inputs, never `runner/`, `.pyc`, target outputs or interim
  harness trees. Development Python files are not installed. A buildable source
  package is not standalone full-oracle test closure: that suite requires the checkout.
  Python 3.14.8 and the repository's Python development/audit tools are requirements
  for those checks, not a product runtime dependency.
- Execution preparation/review, retained receipts, observations and readiness never
  become dispatch grants because their language changed. Native reviewed cold launch
  now performs **actual startup effects** and can start unrestricted tools, not task
  dispatch, read-only enforcement or worker acceptance. Exact checkpoint approval
  alone does not execute: fresh durable action consumption is still required.
- Keep source-free historical read/cancel/recover behavior independent of current
  config, task state and upgrade backups. Writer-only escape admission must not
  tighten historical acceptance or pretend older controls have been rehabilitated.
- Observation uses **trusted bounded local execution** for Git/help/version/fixed
  `session observe`, not Python helpers. Private bootstrap
  runs before Clap in the same single binary. Parent EOF and budget+1 watchdog
  are armed before target launch; leader status and pipe EOF are both required.
  Cleanup kills the unreaped supervisor PGID before reap, including normal/nonzero
  descendants with closed pipes; reaper contexts refuse and cleanup faults produce
  SpawnError without transcript disclosure. Probe limits are <=30 seconds; this
  ownership policy is unchanged, not reused for effectful runtime transport.
- Captured runtime CLI commands instead use 60 seconds/1 MiB combined output, own/reap only
  the direct child and leave server/daemon effects alive; no rollback is implied.
  Nonstandard child descriptors are closed/CLOEXEC. Frozen launch uses the six-key captured
  environment and cwd; legacy transport retains all caller environment and caller
  cwd. Ordered PATH fallback requires proven pre-image `ExecFailed` (spawn error),
  never timeout, output limit, generic `SpawnError` or executed nonzero 127. Relative,
  empty/unset PATH and symlink/`..` resolution preserve the appropriate cwd/order.
  Foreground tmux attach/switch is separate: inherited stdio, no 60-second cap,
  both stdin/stdout TTY required for focus, shared admission held through wait.
- Bootstrap ordinary nonzero exits are intentionally ignored; counts/events mean
  attempts, not delivery or worker acceptance. Setup/timeout errors stop without
  retry. Full topology reuse/replacement and agent order have no implicit rollback.
  Raw configured-command NUL rejects before normalization/destructive restart;
  final scripts also validate. Lead preserves extensions/default prompt, atomically
  sets or unlinks+syncs clear; secondaries warn after commit, notices run unlocked
  with independent admission and no duplicate history. Approved standup/status
  stays unverified with zero backend construction; historical lead needs no current
  roster membership. Console uses borrowed I/O for nested review, displayed-revision
  CAS, default-No confirmation and maximum checkpoint status through EOF/quit.
- Observation's synchronous entropy/tempfile/FD-scan/self-exec setup and filesystem/hash I/O are
  not interruptibly hard-bounded. Per-probe and per-observation budgets are checked,
  not universal setup-inclusive deadlines; cleanup has a separate <=1-second reap
  allowance. Static/cooperating process groups are assumed, not `setsid`-escape
  containment. Linux is validated; macOS remains open.
- Shared executable metadata/alias checks and capped 1 GiB complete fingerprints
  detect ordinary changes, not authentication/sandboxing. Git is pinned too;
  ordinary change makes live observation unavailable but cannot invalidate
  source-free inspect or exact known retries. Git >=2.48 probes hash only raw base
  commit/root tree, not full closure. Ring 0.17.14 (default features off) supplies
  cached independent SHA-1; native C build dependencies are not runtime Python.
  Existing SHA-256 domains stay unchanged; lockfile additions are dependency-only.
- Preserve Python 3.14 timestamp-consumption quirks: two alleged trailing-garbage
  cases are accepted by the oracle, not proof of stricter native rejection.
- Standalone readiness tests use a Python fake target as development coverage, not
  Python-absence proof. The separate compiled Rust producer batch establishes
  interpreter-independent children. No actual Djinn instance calls were validated.

## Example usage

Build and invoke the native checkout launcher explicitly:

```sh
cargo build --offline --locked --release --bin coven
bin/coven --help
COVEN_NATIVE_BIN=/abs/prebuilt/coven /abs/checkout/bin/coven --help
sh scripts/install-native.sh --binary /abs/prebuilt/coven --prefix /abs/existing-parent/new-prefix --examples /abs/checkout/examples
```

Paths are placeholders; the stage prefix must be absent with an existing trusted
parent. The installer never builds, upgrades or rolls back partial installation.
Help is titled `Manage multi-agent coven workspaces`. Use inspected tokens/IDs in the README's
[standalone](README.md#fresh-native-standalone-loop) and
[reviewed graph](README.md#fresh-native-reviewed-two-node-loop) flows. Those examples
are labeled pseudocode where manual document/variable setup is required. The
[bounded observation examples](README.md#native-bounded-local-observation) show
functional repository/readiness/persisted-Djinn CLI commands with inert reference
selectors; real-Git validation belongs in disposable local checkouts. After
release validation, repeat accepted workflows through the installed `coven` with Python absent;
unsupported-route refusal alone is not a successful workflow smoke test.
The README's [reviewed cold-launch pseudocode](README.md#optional-reviewed-runtime-startup)
uses explicit native paths and manually inspected placeholder pins; it is not
automatic approval or a recommendation to validate against real installed runtimes.

## Related files

- [README.md](README.md): user-visible native availability, examples and current install.
- [RUST_MIGRATION.md](RUST_MIGRATION.md): code map, limits, validation and coverage matrix.
- `src/main.rs`, `src/execution/mutation/`: native routing and preparation writers.
- `src/actions/mutation/`, `src/launch{.rs,/executable.rs}`, `src/runtime{.rs,/transport.rs}`,
  `src/runtime_command.rs`, `src/platform/unix/runtime_command.rs`,
  `src/transport_gate.rs`, `src/checkpoint_decisions/`: effectful reviewed startup,
  legacy transport, separate process ownership and checkpoint decisions.
- `tests/runtime_launch_workflow.rs`, `tests/_runtime_launch_parity.py`,
  `tests/support/runtime_backend_fixture.rs`: compiled inert-backend workflow acceptance.
- `src/runtime/lifecycle{.rs,/create.rs,/staging.rs}`, `src/runtime/bootstrap.rs`,
  `src/runtime/presentation{.rs,/standup.rs}`, `src/lead{.rs,/prompt.rs}`,
  `src/console{.rs,/services.rs}`: completed legacy lifecycle and presentation.
- `src/runtime_foreground.rs`, `src/platform/unix/runtime_foreground.rs`,
  `src/platform/unix/runtime_child.rs`, `src/workspace_assets.rs`: foreground/FD
  ownership and shared generated scripts; no new dependency in this batch.
- `tests/legacy_lifecycle_workflow.rs`, `tests/_legacy_lifecycle_parity.py`,
  `tests/support/legacy_backend_fixture.rs`: new compiled inert acceptance.
- `src/process.rs`, `src/process/identity.rs`, `src/platform/unix/process.rs`:
  shared executable identity and real production supervisor.
- `src/repositories/`, `src/workers{.rs,/help.rs}`,
  `src/djinn_observation{.rs,/schema.rs}`: repository, help-only readiness and persisted
  evidence; not the still-required live observers.
- `tests/process_integrations.rs`, `tests/_process_integrations_parity.py`,
  `tests/process_supervision.rs`, `tests/workers_readiness_native.rs`: compiled
  producer/real-Git CLI acceptance, actual-binary bridge and development-only targets.
- `tests/execution_lifecycle.rs`, `tests/_execution_lifecycle_parity.py`: native
  workflow harness and temporary parent oracle to retire without losing assertions.
- `bin/coven`, `runner/`: native-only checkout launcher and retained development-oracle inventory.
- `tests/native_entrypoint.rs`, `tests/native_package.rs`, `tests/native_install.rs`:
  independent non-delegation, source-payload and installed-runtime guards.
- `scripts/install-native.sh`, `packaging/coven.rb`: staged native installer and
  unpromoted local formula candidate; see [staged usage](README.md#native-only-staged-installation-not-cutover).
- `tests/retirement/coverage.json`, `tools/retirement_coverage.py`,
  `tests/session_views_vectors.rs`: original IDs, audit and independent frozen vectors.
- `src/workspace_assets{,/init}.rs`, `examples/README.md`, `examples/interactive-demo/`:
  embedded assets, generated commands and example deliverable instructions.
- `Cargo.toml`, `justfile`: native binary and full validation recipes.
