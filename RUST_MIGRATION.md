# Coven Rust migration

## Overview

Status: **native production cutover; Python source retained as a development oracle**.
Native workspace init, goal/message/history/status, checkpoint inspect/next guidance, task add/assign, state import/recovery, existing-workspace refresh,
evidence capture and authority readers, all five execution inspection/export routes,
preparation cancellation, both explicit execution-recovery modes, and all six
runtime-free execution authoring routes (enable, enroll/rebind, upgrade, intent/graph
prepare, graph propose and approve/changes-requested decide), repository inspect
(including verify)/register, worker readiness, persisted `observe djinn`,
Kitsune/Djinn-live observation, all eight Group routes, all seven Session routes,
human-default task listing, custom example assets, reviewed
cold-launch action writers, checkpoint approve/suggest/review, legacy send/agent
send and down/stop, standup, console, lead, bootstrap, agent status/start/stop/restart,
plan, up/start alias and restart are implemented. All 78 original command paths
are represented; native-only execution history is extra, not a parity percentage.
The parent selected native product execution without deleting all 25 retained
Python source oracles. `bin/coven` is native-only; installed `coven` is the direct
Rust binary. No unsupported command delegates to Python. Source cleanup, full
compatibility-policy approval and macOS closure are not claimed. The original
full-migration-before-merge requirement remains; no merge, source push or external
promotion is authorized. Building alone does not install, migrate workspace/state
or activate authority.

Migration development is being consolidated in the dedicated `rust-migration`
worktree (`../coven-rust-migration` from the main checkout). Check `git worktree
list` and current uncommitted work before starting another slice. The overlapping
main-checkout init/refresh implementations were not transplanted: this worktree's
config-last bootstrap, embedded assets and stronger generated-file safety remain
canonical. Only useful parser, work-budget and regression-test additions were
adapted; the main checkout's changes remain preserved separately.

## One command, one replacement

### Main-branch integration

The local-main merge retains the migration's canonical native init/refresh and
shared assets rather than layering the older parallel template implementations.
Main's flat-root Python-oracle fix is retained. Its public callback identity
feature is shared by native init, refresh and runtime monitor generation:
nonempty `COVEN_COMMAND`, then nonempty `ARCANA_PUBLIC_COVEN`, then portable
`coven`. Empty values are unset; unrelated Arcana variables do not select callbacks.
Selected spelling is shell-quoted without canonicalization or executable probing.

The native launcher still executes only an explicit absolute prebuilt binary or
the one checkout release path. Default checkout symlink chains are followed with
a 40-link bound; ordinary nonsymlink invocation remains tool-free with empty PATH.
`COVEN_HOME` remains an asset root, not a Python-runner or binary selector.
The five main-only wrapper tests are retained as explicit supplemental tests
against prebuilt Rust fixtures. The original retirement baseline remains 941
methods (67 mapped / 874 unresolved); total Python discovery is now 946 methods.
No remaining release/platform or source-oracle-cleanup gate is silently closed
by the user-authorized local merge. No push or external formula promotion is made.

Integrated validation passed formatting, strict all-target Clippy, Rust 1.85.0
all-target checking, all native regression targets (1,228 unique passes, five
subprocess-only helpers ignored), 946 Python tests in 192.483s and 29 audit-tool
tests in 257.093s. Native results combine the completed full regression run with
the eight-test entrypoint rerun after replacing a transient write-open script
fixture with an existing prebuilt inert executable; no production assertion or
timeout was relaxed. The complete regression output is saved at
`/home/jdawson/.local/share/opencode/tool-output/tool_1bb52e65d001K26PqvTY7WzwgR`.
The retirement audit separately preserves 941 original plus five explicitly
reviewed main-branch supplemental methods; it does not redefine mapping totals.

The Rust crate defines exactly one binary target, **`coven`**. Development and
parity tests invoke its explicit Cargo/build path, never the installed command
or an automatically selected implementation. There is no second native command
or compatibility alias to maintain.

Python remains a checkout-only development oracle, not a second product or runtime
fallback. The native source-package allowlist excludes `runner/`, bytecode, build
outputs and interim trees while retaining production Rust and compile-time inputs,
including development `*oracle*.py` files; those files are not installed. The full
oracle regression suite requires the checkout, not a standalone Cargo source package.
See [RUST_RETIREMENT.md](RUST_RETIREMENT.md) for separate source-cleanup and release gates.

## API / Interfaces

```sh
cargo build --offline --locked --release --bin coven
bin/coven --help
COVEN_NATIVE_BIN=/abs/prebuilt/coven /abs/checkout/bin/coven --help
```

Absolute paths above are placeholders. Offline builds need the locked dependency
cache. Help is titled `Manage multi-agent coven workspaces`.

## Data Flow

The `#!/bin/sh` checkout launcher `exec`s absolute `COVEN_NATIVE_BIN` when set,
otherwise only its normal checkout's `target/release/coven`. Missing selection
fails with explicit build instructions: no automatic Cargo, Python, debug or PATH
fallback. It forwards argv/environment without overwriting `COVEN_COMMAND` or
`COVEN_HOME`. The former overrides generated commands; the latter means the asset
root `COVEN_HOME/examples`, not executable selection. Relocated/symlinked launchers
require explicit absolute native selection; installed binaries need no wrapper.

## Completed locally vs External checklist

- **Local scoped evidence:** five independent entrypoint, two package and nine
  install tests cover native runtime guards. Nested harnesses that caused Cargo to
  omit production modules moved to `tools/harnesses/group_publisher` and
  `tools/harnesses/group_live`; production modules remain under `src/`. An agent
  verified the 300-file crate build and extracted release install with empty
  PATH/cleared environment for help, blank/demo init and refresh.
- **Local acceptance:** split validation totals **1,225 native passes**; strict
  all-target Clippy and Rust **1.85.0** all-target check pass after the two MSRV
  import/let-chain fixes, with no production-policy changes. The implementation is
  ready for local review and manual native use within documented profiles. See
  [final local acceptance](#final-local-acceptance), not the historical 1,191-native
  run, for current results and qualifications.
- **External:** macOS has not run; actual Homebrew clean install/upgrade is untested.
  `packaging/coven.rb` is a native-only candidate, not a promoted external tap.
  Its remote `main` URL cannot be promoted until a reviewed release. No source push
  or merge is authorized. No changes have been committed, pushed or merged.
- **Separate cleanup/policy:** all 941 original Python tests remain active. The
  audit's 67 mapped / 874 unresolved IDs concern development-oracle removal, not
  874 missing features or product native-execution coverage. Non-UTF-8 and finite
  value/resource restrictions require explicit disposition, not silent approval.

### Final local acceptance

Final local evidence is **split validation**, not a single fresh all-command green
invocation:

- The initial full `cargo test --no-fail-fast` run passed **86 CLI tests and 118
  integration tests across 39 suites**, but had a library-test flake. Saved output:
  `/home/jdawson/.local/share/opencode/tool-output/tool_1b8cf6da8001vCV3PzWpPztZn5`.
- After test-only corrections, the final complete library run passed **1,021 tests,
  with 5 ignored and 0 failed**. Saved output:
  `/home/jdawson/.local/share/opencode/tool-output/tool_1b960241a0014zirTOTvdLveg4`.
  Combined native passes are **1,021 + 86 + 118 = 1,225**. Ignored subprocess helpers
  are exercised by parents; nested runs/helpers are not counted twice.
- Lease tests were isolated so unrelated raw-fork descriptors could not retain an
  OFD lease. A subsequent library retry exposed a Git fixture's `maintenance.lock`
  disappearing during background maintenance; fixture Git commands now disable
  `maintenance.auto` and `gc.auto`. These are test-only changes; all original
  snapshot assertions remain. They do not alter production policy.
- The final standalone Python suite passed **941 tests in 191.978s**. A prior run
  concurrent with heavy builds failed one pre-existing 0.2-second descendant test;
  the final pass was obtained after load subsided, without loosening its timeout,
  changing assertions or skipping tests. **22 development-tool tests passed in
  156.071s**. The audit passes with **67 mapped / 874 unresolved**, unchanged.
- Independent native entrypoint **5**, package **2** and installer **9** tests pass
  (evidence layers within the native results, not additions to the aggregate).
  Actual Cargo packaging verified **300 files**, excluding runner/bytecode/target
  payloads while retaining all production modules. Root and extracted-source
  release builds/install checks pass; `env -i PATH= /abs/bin/coven --help` shows
  `Manage multi-agent coven workspaces`. Installed blank/demo init and refresh pass.
- Strict all-target Clippy and Rust **1.85.0** all-target check pass after the
  `crate::execution` import and nested-`if` replacement for let-chains. The moved
  harness commands were verified: publisher **177** and live **25** tests pass;
  these separate harness results are not added to the 1,225 native total.

All legacy source remains unchanged. The eleven reviewed invocation-only test
retargets preserve all 941 original tests and accepted AST/hash-transition
provenance. This evidence supports local review/manual native use within profiles,
not unconditional compatibility, macOS parity, source deletion or external release.

## Edge Cases

The launcher rejects empty/relative overrides, missing/non-executable files and
same-inode recursion. Native product execution and a native payload do not imply
full source deletion, standalone-package oracle test closure or platform acceptance.
Detailed filesystem, encoding, output and resource risks remain documented below.

## Example usage

For a new, absent prefix with an existing trusted parent:

```sh
sh scripts/install-native.sh --binary /abs/prebuilt/coven --prefix /abs/existing-parent/new-prefix --examples /abs/checkout/examples
/abs/existing-parent/new-prefix/bin/coven --help
```

The installer refuses upgrades/adoption and preserves partial failures without
rollback. See [packaging/README.md](packaging/README.md).

## Related files

`bin/coven`, `src/main.rs`, `src/example_assets.rs`, `Cargo.toml`,
`scripts/install-native.sh`, `tests/native_{entrypoint,package,install}.rs` and
[RUST_RETIREMENT.md](RUST_RETIREMENT.md).

## Native commands available

```sh
cargo run --offline --locked --bin coven -- init WORKSPACE --example interactive-demo
cargo run --offline --locked --bin coven -- init WORKSPACE --blank
cargo run --offline --locked --bin coven -- handoff validate FILE --json
cargo run --offline --locked --bin coven -- handoff collect WORKSPACE --file FILE --expect-sha256 HEX --expect-tasks-revision TOKEN --json
cargo run --offline --locked --bin coven -- handoff inspect WORKSPACE COLLECTION_ID --json
cargo run --offline --locked --bin coven -- handoff list WORKSPACE --task TASK_ID --json
cargo run --offline --locked --bin coven -- task list WORKSPACE --json
cargo run --offline --locked --bin coven -- task add WORKSPACE "Task title" --owner AGENT --json
cargo run --offline --locked --bin coven -- task assign WORKSPACE TASK_ID AGENT --json
cargo run --offline --locked --bin coven -- state inspect WORKSPACE --json
cargo run --offline --locked --bin coven -- state import-tasks WORKSPACE --expect-revision SNAPSHOT_TOKEN --json
cargo run --offline --locked --bin coven -- state recover-tasks WORKSPACE --repair-projection --json
cargo run --offline --locked --bin coven -- state recover-tasks WORKSPACE --resume-import --expect-revision SNAPSHOT_TOKEN --json
cargo run --offline --locked --bin coven -- state recover-tasks WORKSPACE --abort-import --json
cargo run --offline --locked --bin coven -- refresh WORKSPACE
cargo run --offline --locked --bin coven -- goal WORKSPACE "Shared goal"
cargo run --offline --locked --bin coven -- message WORKSPACE SENDER RECIPIENT "Message body"
cargo run --offline --locked --bin coven -- messages WORKSPACE --pretty
cargo run --offline --locked --bin coven -- events WORKSPACE --raw
cargo run --offline --locked --bin coven -- status WORKSPACE
cargo run --offline --locked --bin coven -- execution history WORKSPACE --json
cargo run --offline --locked --bin coven -- execution inspect WORKSPACE
cargo run --offline --locked --bin coven -- execution enable WORKSPACE --expect-execution-revision TOKEN --request-id KEY --json
cargo run --offline --locked --bin coven -- execution enroll WORKSPACE TASK_ID --expect-tasks-revision TOKEN --expect-execution-revision TOKEN --request-id KEY --json
cargo run --offline --locked --bin coven -- execution upgrade WORKSPACE --expect-execution-revision TOKEN --request-id KEY --json
cargo run --offline --locked --bin coven -- task attempt prepare WORKSPACE TASK_ID --intent FILE --expect-tasks-revision TOKEN --expect-execution-revision TOKEN --request-id KEY --json
cargo run --offline --locked --bin coven -- task attempt prepare WORKSPACE TASK_ID --graph-revision REV --expect-tasks-revision TOKEN --expect-execution-revision TOKEN --request-id KEY --json
cargo run --offline --locked --bin coven -- task graph propose WORKSPACE --document FILE --expect-tasks-revision TOKEN --expect-execution-revision TOKEN --request-id KEY --json
cargo run --offline --locked --bin coven -- task graph decide WORKSPACE REV --decision approve --reason TEXT --expect-graph-digest DIGEST --expect-tasks-revision TOKEN --expect-execution-revision TOKEN --request-id KEY --json
cargo run --offline --locked --bin coven -- task graph decide WORKSPACE REV --decision changes-requested --reason TEXT --expect-graph-digest DIGEST --expect-execution-revision TOKEN --request-id KEY --json
cargo run --offline --locked --bin coven -- task attempt list WORKSPACE --task TASK_ID --json
cargo run --offline --locked --bin coven -- task attempt inspect WORKSPACE ATTEMPT_ID --json
cargo run --offline --locked --bin coven -- task attempt cancel WORKSPACE ATTEMPT_ID --reason TEXT --expect-execution-revision TOKEN --request-id KEY --json
cargo run --offline --locked --bin coven -- execution recover WORKSPACE --confirm-authority --expect-execution-revision TOKEN --json
cargo run --offline --locked --bin coven -- execution recover WORKSPACE --archive-pending --json
cargo run --offline --locked --bin coven -- task graph inspect WORKSPACE --json
cargo run --offline --locked --bin coven -- task graph export WORKSPACE GRAPH_REVISION --json
cargo run --offline --locked --bin coven -- action inspect WORKSPACE ACTION_ID --json
cargo run --offline --locked --bin coven -- action enable-runtime-launch WORKSPACE --json
cargo run --offline --locked --bin coven -- action prepare-runtime-launch WORKSPACE --json
cargo run --offline --locked --bin coven -- action submit WORKSPACE ACTION_ID --expect-manifest MANIFEST --expect-checkpoint APPROVED_SNAPSHOT --expect-decision DECISION_ID --json
cargo run --offline --locked --bin coven -- action acknowledge-unknown WORKSPACE ACTION_ID --expect-claim CLAIM_ID --json
cargo run --offline --locked --bin coven -- action stop WORKSPACE ACTION_ID --json
cargo run --offline --locked --bin coven -- approve WORKSPACE CHECKPOINT --expect-revision SNAPSHOT --json
cargo run --offline --locked --bin coven -- suggest WORKSPACE CHECKPOINT "Requested changes" --expect-revision SNAPSHOT --json
cargo run --offline --locked --bin coven -- review WORKSPACE CHECKPOINT
cargo run --offline --locked --bin coven -- send WORKSPACE AGENT "Literal message"
cargo run --offline --locked --bin coven -- agent send WORKSPACE AGENT "Literal message"
cargo run --offline --locked --bin coven -- down WORKSPACE
cargo run --offline --locked --bin coven -- stop WORKSPACE
cargo run --offline --locked --bin coven -- checkpoint inspect WORKSPACE CHECKPOINT_ID
cargo run --offline --locked --bin coven -- checkpoint inspect WORKSPACE CHECKPOINT_ID --json
cargo run --offline --locked --bin coven -- next WORKSPACE
cargo run --offline --locked --bin coven -- repository inspect WORKSPACE --verify --json
cargo run --offline --locked --bin coven -- repository register WORKSPACE --path CHECKOUT --base FULL_COMMIT --delivery-target refs/heads/review --expect-repository-revision repository-unregistered:v1 --json
cargo run --offline --locked --bin coven -- worker readiness WORKSPACE --adapter djinn --profile research-readonly --executable TRUSTED_EXECUTABLE --timeout 3 --max-output-bytes 131072 --json
cargo run --offline --locked --bin coven -- observe djinn --executable ABS_TRUSTED_EXECUTABLE --folder ABS_FOLDER --timeout 3 --max-output-bytes 131072 --json
```

The compiled `target/debug/coven` can also be invoked directly, including with
Python absent from PATH. Production has no Python helper or fallback. It now uses
trusted bounded local subprocesses for Git, help/version and fixed persisted-session
observation, plus actual reviewed cold-launch and legacy runtime transport effects.
Cold startup can start unrestricted tools; it is not task dispatch, sandboxing or
worker acceptance. Observation/readiness and execution preparation remain non-dispatching.
Python is needed only for development fixtures, oracle tests and audit tools,
not native product execution. Groups, all Session routes and Kitsune/Djinn-live observers
are native. Unknown commands/flags reject at parsing before effects; there is
**no automatic Python fallback**.

Handoff commands preserve [HANDOFF_CONTRACT.md](HANDOFF_CONTRACT.md): syntax-valid
handoffs remain worker claims, collected history remains unverified, authorization
none and dispatchable false. Historical reads report current task binding not
checked; collection checks only its explicit current local context. No source/check/
native reference is followed or verified. Reads create no lock, directory,
dashboard, repair, migration marker or runtime state.

## Structure and dependencies

```text
src/
  main.rs            Thin explicit CLI; escaped reports and exact literal workspace output
  process.rs         Private ProbeRequest/ProbeRunner/ProbeResult observation transport
  process/identity.rs Shared executable selection, metadata/alias checks and SHA256 pinning
  platform/unix/process.rs Same-binary private bootstrap, watchdog, drain and group cleanup
  repositories.rs    Inspect/register CLI adapters and ASCII JSON delivery
  repositories/{record,registration,git,timestamp}.rs Immutable raw authority, retry, Git and chronology
  workers.rs         Help-only readiness CLI and unverified capability report
  workers/help.rs    Strict advertised help/version parsing and declaration limits
  djinn_observation.rs Fixed persisted session.observe adapter and ASCII report
  djinn_observation/schema.rs Closed bounded producer contract validation
  cli_operands.rs     Narrow checkpoint/next and execution-reader/maintenance operand lexing
  workspace_commands.rs Local goal/message writers and dashboard-publishing status
  workspace_commands/history.rs Read-only pretty/raw event and message history
  workspace_commands/python.rs Finite Python truthiness and str/repr presentation
  logs.rs            Private shared strict JSONL admission and guarded append
  unicode.rs         Frozen Unicode 16 printability shared by dashboard and Python repr
  handoff.rs         Typed identifiers, revisions, claims, enums, closed schemas
  strict_json.rs     UTF-8, duplicate-key, finite-number and depth safeguards
  canonical.rs       Closed-domain v1 evidence encoding and SHA256
  archive.rs         Historical archive verification, bounded list/filter reports
  collection.rs      Pinned unverified capture with locked context revalidation
  read_fs.rs         Bounded read-only file/managed-path operations
  values/            Finite compatibility values, duplicate profiles and encoders
  records.rs         Extension-preserving validated workspace/task views
  tasks.rs           Preflight, captured task/state reports and authority fences
  state_commands.rs  State inspection/import/recovery CLI admission and known-outcome delivery
  init.rs            Complete staged blank/embedded/custom-example init CLI service
  example_assets.rs  Relocatable read-only AssetBundle, explicit roots and bounded custom files
  session_inspection.rs Bracketed generic inventory and historical declaration joins
  session_views.rs   Pure selection after full bounded observation
  session_watch.rs   Foreground streaming samples and summary, no buffered replay
  interactive_sessions.rs Pure start/resume plans and explicit pinned generic submission
  session_prompts.rs Native-only capabilities/prompt, correlated receipts and no retry
  session_support.rs Shared empty-workspace normalization, transport admission and output
  storage/bootstrap.rs Private exclusive state reservation and config-last publication
  platform/unix/init.rs Filesystem-order missing-root resolution and bootstrap identity
  refresh.rs         Prepare-all, guarded existing-workspace asset regeneration
  workspace_assets.rs Frozen pure templates and embedded interactive-demo context
  workspace_assets/init.rs Initial templates and all seven original embedded example assets
  task_mutation.rs   End-to-end snapshot/ledger add/assign, receipt and delivery policy
  task_admission.rs  Prepublication response/list capacity and bounded warning codes
  task_delivery.rs   Guarded delivery/dashboard; shared read-only checkpoint-log references
  task_time.rs       UTC ISO metadata and hyphenated command/event UUIDs
  workspace.rs       Neutral ordered workspace loading and optional-root selection
  ledger.rs          Complete bounded selected task-ledger replay/projection health
  ledger/confirmation.rs Explicit locked selected-history synchronization
  ledger/activation.rs Baseline planning, marker-last activation, resume/archive
  ledger/repair.rs    Confirmed authority, raw projection backup and explicit replacement
  ledger/publication.rs Private typed no-overwrite artifact publication
  ledger/writer.rs   Guarded task-frame commit and fresh selected projection publication
  execution.rs       Historical execution replay state, v1 rules and report boundary
  execution/v2.rs    Upgrade bridge, graph reviews and v2 preparation replay
  execution/graphs.rs Pure bounded graph validation, refreezing and source matching
  execution/guards.rs Confirmed reservation checks under a borrowed writer guard
  execution/mutation.rs Source-free cancellation, receipt-before-CAS and exact-byte successor admission
  execution/mutation/commands.rs Six authoring CLI adapters; strict document-value bindings
  execution/mutation/lifecycle.rs Private receipt-first coordinator, activation and delivery
  execution/mutation/effects.rs Enable/enroll/standalone preparation successor effects
  execution/mutation/graphs.rs Typed graph proposal/review/preparation effects and reducers
  execution/mutation/upgrade.rs Exact raw v1 backup, mode/readback/sync before replacement
  execution/mutation/capacity.rs Writer-only native cancellation/revocation escape headroom
  execution/mutation/delivery.rs Preadmitted confirmed/uncertain frozen-result delivery
  execution/confirmation.rs Full typed reread and control/execution/state/root synchronization
  execution/recovery.rs Explicit authority confirmation and whole pending-directory archival
  storage/maintenance.rs Crate-private marker-independent acquisition of the shared persistent lock
  execution/inspection.rs Ordered replay/existing-lock/source assessment and output admission
  execution/source.rs Fresh raw snapshot or independently selected ledger under borrowed guard
  execution/inspection/commands.rs Five native inspection/export wrappers and selector timing
  execution/inspection/projection.rs Pure binding/graph projections in proposal order
  actions.rs         Captured launch-control/bundle/checkpoint inspection
  actions/mutation/  Immutable preparation, durable single-use submission, acknowledgement and frozen stop
  actions/mutation/capacity.rs Writer-only claim/plan/control and pending-report headroom
  launch{.rs,/executable.rs} Frozen plan execution/stop and ordered pre-image PATH fallback
  runtime{.rs,/transport.rs} Legacy send/down and unlocked checkpoint text/Enter notices
  runtime_command.rs Direct-child runtime transport; separate from observation probes
  platform/unix/runtime_command.rs Descriptor hygiene, capture and direct-child cleanup
  runtime_foreground.rs Separate inherited-stdio foreground request/runner, no capture deadline
  platform/unix/runtime_foreground.rs TTY query and direct-child foreground wait
  platform/unix/runtime_child.rs Shared child-only CLOEXEC hygiene, preserving exec-error handshake
  runtime/lifecycle{.rs,/create.rs,/staging.rs} Legacy topology/reuse/restart and aggregate staging
  runtime/bootstrap.rs Whole-batch prompt admission and immediate unchecked-nonzero attempts
  runtime/presentation{.rs,/standup.rs} Approved-aware unverified status and bounded standup
  lead{.rs,/prompt.rs} Extension-preserving atomic set, unlink/sync clear and unlocked notices
  console{.rs,/services.rs} Borrowed-I/O loop and displayed-revision checkpoint composition
  transport_gate.rs Shared legacy admission/exclusive activation before workspace exclusion
  checkpoint_decisions/ Scope-preserving decisions, displayed-snapshot review and unlocked notices
  actions/authority.rs Pure all-claim validation, reference lookup and work budget
  actions/plans.rs   Pure deterministic tmux/Herdr frozen-plan validation
  checkpoints.rs     Pure byte-oriented checkpoint views and framed scope hashes
  checkpoint_workspace.rs Ordered standalone inspect preflight and pretty/compact text
  checkpoint_guidance.rs Read-only next selection and complete literal output
  checkpoint_guidance/timestamp.rs Python 3.14 ISO timestamp guidance sort keys
  storage.rs         Shared workspace guard and immutable evidence publication
  storage/replacement.rs Guarded atomic mutable-file replacement
  output.rs          Bounded safely escaped CLI presentation
  platform/          Compile-gated OS open flags and unsupported-platform refusal
```

Serde/serde_json provide typed JSON, clap the CLI, sha2 the existing digest
algorithm, and Unix libc the descriptor/lock operations. Ryu supplies shortest
ties-to-even float digits; the compatibility encoder separately applies Python's
formatting policy. TOML parses the existing workspace runtime configuration.
Tempfile creates exclusive private same-directory evidence temps. All are
conventional cached crates, not an external agent,
interpreter/daemon dependency or a new plugin framework.

Git raw-object SHA-1 integrity is independently recomputed using cached **ring
0.17.14**, with default features disabled. The minimal SHA-1 crate was not cached;
no online Cargo resolution was used. Ring adds native C/crypto build dependencies,
not runtime Python. SHA-256 object hashing and all existing SHA-256 digest domains
are unchanged. Cargo.lock adds the required dependencies only, without unrelated
upgrades. SHA-1 integrity is not authentication.

Current filesystem support is POSIX Linux/macOS, matching existing Coven scope;
only Linux was executed here. Non-Unix filesystem commands fail closed. The pure
model/codec modules remain platform-independent. No Windows support is claimed.

## Byte identity and current limits

Raw input, typed values, canonical bytes and hashes are separate representations.
The native reader accepts existing Python-produced handoff archives, verifies
raw-input hash/count, captured binding, exact canonical envelope plus LF, content
ID/domain and final filename. It never recomputes historical task SHA against
today's task file or uses a current roster to manufacture verification.

The canonical encoder is **only for the accepted closed handoff/archive domain**:
sorted fixed schema keys, compact separators, literal UTF-8, bounded integer
version/byte-count fields. It is intentionally private to those typed models.
This handoff encoder is **not** the generic task-value codec. The separate
`values` model preserves bounded decimal integers without a binary64/Serde-number
round trip, distinguishes floats (including signed zero), and preserves insertion
order. Its legacy snapshot profile is last-wins without moving an existing key;
its authority profile rejects duplicates. Canonical encoding sorts keys/compact/
no LF; storage encoding preserves order with indent=2 and a terminal LF. The
general codec is used by bounded authority validation and admitted task/baseline writers. Never
silently adopt Serde/JCS encoding under existing v1 digest domains.

Frozen launch plans use an additional private sorted/spaced mode equivalent to
Python `json.dumps(sort_keys=True, ensure_ascii=False)`, with default `, ` / `: `
separators and no LF. This is separate from compact digest encoding, insertion-order
storage and report presentation. Existing canonical/storage/compact behavior is
unchanged; plans are compared and size-checked in their own encoding domain.

The first native preview deliberately refuses historical artifact reporting
under non-UTF-8 POSIX paths (`non_unicode_path_unsupported`). Python can represent
those paths through surrogateescape. A differential case captures this known
gap; Rust does not substitute a lossy name or label the archive corrupt. It must
be resolved, or explicitly restricted by a separately approved compatibility
decision, before default-entrypoint parity is claimed.

CLI JSON presentation need not match whitespace/key order, but structured values,
trust fields, raw hashes/counts and success/refusal behavior must match. Existing report untrusted
text, bidi characters, DEL/C1 and non-BMP content are safely escaped for display;
canonical archive bytes use the separate UTF-8 encoder. The new local workspace
families instead preserve literal Python text; see the terminal warning below.

## Snapshot task preview and shared-lock foundation

`task list WORKSPACE` defaults to human output; `--json` selects the existing
structured envelope. Both validate the known workspace managed paths, JSON
team/agent/lead records, checkpoint paths/text and TOML runtime fields. It retains
the complete task values, unknown nested extensions, exact IDs, arbitrary legacy
status strings, missing versus null owner, and uninterpreted timestamp strings.
It does not impose owner-roster membership or add defaults to task records.

The snapshot success envelope remains exactly `revision` and `tasks`. Revisions hash the
original bytes; missing and present-empty snapshots differ. No metadata is made
authoritative, and no read creates a lock, event, dashboard or recovery artifact.
CLI empty workspace means current directory and quoted `~/...` uses HOME; library
Path APIs remain literal. Named-user expansion and HOME-absent expansion explicitly
refuse instead of selecting a literal tilde directory or doing implicit user lookup.

Native task listing still refuses runtime-launch control/action artifacts before
reading and at final return; the state-inspection route described below validates
launch control instead. A task-authority marker selects the complete native reader
below. An absent marker plus ledger directory is unresolved import: native task
listing conservatively refuses it, while `state inspect` reports snapshot
`import_pending: true`, including `source_error` without invented tasks/revision
when source is malformed. These checks are not an atomic authority snapshot and
cannot be reused as mutation authorization.

## Read-only task-ledger authority

A present marker must validate: exact schemas/types/IDs, selected raw manifest
digest, source presence/mode/length/digest/revision, exact epoch entries, strict
source task JSON and contiguous canonical final frames. The reader checks complete
envelope bytes/LF, payload hash/length, baseline/source task equality, predecessor
chain, request uniqueness/bindings, add slug/collision/owner/title/status rules,
assign preservation of every other field/extension, and frozen before/after
revisions/result sequences. Hash-consistent but semantically invalid effects are
not accepted. A bad final frame never yields a last-good result or snapshot fallback.

Active task-list output retains the established ledger envelope: authority,
revision/tasks, workspace/epoch/sequence/frame digest and projection health. State
inspection has the same active envelope with narrower existing preflight: current
agent/lead/projection contents are not parsed as task state. Both skip projection
path/type validation in active preflight, classifying it through health instead.
Current roster cannot re-resolve historical owners; timestamps have no new meaning.

Projection health is raw-byte current/missing/diverged/unreadable with separate
snapshot hashes, never authority or a repair trigger. Sequence zero preserves the
original baseline bytes; absent source expects `[]\n`, not absence. After mutation,
expected projection uses the existing Python-compatible indented storage encoding.
Observed projections are stream-compared/hashed, including readable inputs above
the frame-size bound. Diagnostic text uses safe native codes, not OS/Python prose.

Marker bytes are rechecked after replay and before captured command report return.
Snapshot-to-marker, marker change/disappearance, runtime-authority publication and
pending-state transitions refuse. A frame appended outside the captured filename
set can leave a fully validated prefix: there is no latest-head, linearizable,
authentication or durability-confirmation claim. No lock, fsync or repair is done.

Native restrictions: 8 MiB individual authority inputs/frames, 4096 final frames,
256 MiB aggregate final-frame bytes, 16384 entries per enumerated directory
(including inert temps), existing value/reconstruction/response limits, and 256 MiB
streamed projection observation. Over-budget projections are diagnostic unreadable
with an unsupported-profile code rather than unbounded hashing. Exact recognized
regular temp names are inert/type-checked across epoch/frame directories; they are
never promoted or deleted.

Python supports some raw source encodings or numeric spellings outside the finite
UTF-8 native profile. Native retains raw bytes for digests/projection but still
requires supported parsing and source/frame comparison; unsupported input is not
rewritten, accepted via hashes alone or replaced by a projection. The read view now
retains validated historical receipts for the pure library lookup below. Mutation
retry coordination and delivery are supplied by the task workflow below, not
inferred from lookup, inspection or the explicit synchronization API alone.

### Frozen task-ledger receipts (library only)

`ledger::RequestKey` and `RequestBinding` have validated, private representations;
bindings can be parsed from compatibility values or built with explicit add/assign
arguments. `View::lookup` returns an immutable `FrozenResult`, a missing captured
key, or `request_key_binding_conflict`. Only complete successful replay constructs
frozen results. There is no new lookup CLI, mutation, acknowledgement or recovery
surface from lookup itself and no public unchecked result constructor.

Binding equality compares canonical bytes, ignoring only object insertion order.
Raw selector case/whitespace, notes/title spelling, operation and null versus
concrete expected-revision tokens remain part of identity. Historical alias
equivalence in an effect does not make different request bindings interchangeable.
Lookup does not compare an old expected revision to the current head: replay still
validates its CAS at the frame's original position. Later assignments cannot rewrite
an earlier result's complete task, owner, after_revision or historical sequence.
Large integers, floating signed zero and unknown extensions retain `values` identity.
Borrowed results are immutable and owned clones cannot mutate retained receipts.

Receipt retention replaces the request-ID set as the sole uniqueness authority.
Each receipt is inserted only after all frame/effect/result checks; every selected
frame, including a corrupt tail after a matching key, validates before a view is
returned. Existing marker checks remain, but do not authenticate append-only history
or detect every removed suffix, pin the latest head, or make an unlocked prefix
linearizable. Captured lookup itself takes no filesystem/root/guard/current-state
arguments. Explicit fencing is separate; known historical results remain known if
the marker later changes or becomes unreadable.

Missing authority/pending import and a missing key inside a valid active view are
different conditions. `None` from lookup means only absence from that captured
history—not current receipt absence, permission to execute, fresh CAS success or
durability acknowledgement. A private test-only route proves hit/conflict handling
precedes a fresh-request continuation; that seam alone is not a writer or durable
retry pipeline. Native task-command coordination remains a separate contract.

A separate 256 MiB retained-canonical-data limit counts UTF-8 key bytes plus
canonical binding and frozen-result bytes with checked accumulation. Raw history
remains independently capped at 256 MiB and 4096 frames. This is not a heap/peak
memory bound; retaining receipts adds per-frame result storage and encoding work,
without cloning the whole accumulated map on each frame.

Known-result policies must not be silently unified during later writer work:
Python ledger confirmation can fail hard during a current-authority reread outside
its sync-uncertainty wrapper, whereas execution preserves known results after
verification failure. Pure lookup adds no delivery fields and changes neither
policy. Historical `result.outcome: committed` is not a new fsync confirmation.

General JSON preview bounds: 8 MiB input, depth 64, 262144 parsed value nodes,
16384 object members, 65536 array items, 1 MiB decoded strings, 8192-byte number
tokens, 4300 integer digits excluding sign, 32 MiB encoded/final ASCII output.
Checkpoint/TOML reads are bounded to 1 MiB and checkpoint scans to 4096 entries.
These are explicit native restrictions: legacy Python can read some nonfinite,
surrogate-containing, deeper/larger extensions that this finite scalar profile
refuses. They are not silently rewritten or misrepresented as current authority.
Parser node budget is not a promise about arbitrary programmatically built values;
encoding separately enforces depth/container/string/output limits.

`storage::WorkspaceGuard` underpins explicit native handoff capture.
Explicit `acquire` may create a persistent mode-0600 lock; `existing` cannot create
a missing lock/directory. The token is root/PID/thread owned and not Send/Sync,
uses the same kernel flock/inode as Python, never steals/unlinks/replaces it, and
releases on normal exit or process death. The destructor avoids explicitly
unlocking a parent's inherited open-file description after fork. Task and handoff
reads do **not** acquire this token. Acquiring it never activates authority or
dispatches work. Explicit baseline/recovery and task commands use it inside their
separate pipelines below. No automatic recovery or dispatch is enabled.

### Guarded mutable replacement (library only)

`storage::atomic_replace_bytes(guard, relative, bytes)` is a low-level transport,
now used by task snapshots, ledger projections and dashboard publication. The matching root/PID/thread guard
proves coordination only—not CAS, approval, receipt absence, execution permission
or whole-ledger confirmation. Parent directories must already exist and managed
paths must remain safe. Both `.workspace.lock` and `.runtime-transport.lock`
basenames are rejected in every directory, preserving the persistent flock inode.

An exclusive same-directory `.<target>.<random>.tmp` receives all bytes, then
descriptor mode application, descriptor mode verification and file fsync. Existing
07777 mode bits are preserved; absent targets receive 0600. Mode is applied after
writing because writes may clear special bits. A successful chmod can also silently
clear SGID; descriptor verification rejects any mismatch before publication, leaving
the old target intact. Ownership, ACLs and xattrs are not part of preservation.

After path/parent recheck, atomic rename is the visibility boundary, followed by
parent-directory sync. Pre-rename errors leave old bytes/mode/inode intact (or the
target absent). Successful rename plus failed parent sync returns known-new
`Delivery::DurabilityUncertain`; there is no post-rename reread, rollback or claim
that nothing happened. Cleanup touches only the owned temp and never masks the
primary outcome. Inert orphans are neither adopted nor broadly removed.

The independent 32 MiB byte-transport cap does not raise domain limits or native
reader profiles. Existing immutable evidence publication retains its 8 MiB cap and
hard-link no-overwrite semantics. Ledger frames/manifests/baselines, the initial
task-authority marker and action bundles must never use mutable replacement.
No immutable artifacts use mutable replacement. The ledger-specific immutable
baseline protocol below supplies marker-last activation and pending recovery;
selected-history confirmation remains a separate already-active operation.
Local ordinary rename-error semantics and static cooperating paths
are assumed; this is not a hostile-path sandbox, distributed unknown-rename
protocol or universal power-loss guarantee.

### Locked selected-ledger confirmation (SDK only)

`ledger::confirm_selected(workspace, expected_view, borrowed_guard)` validates the
matching root/PID/thread guard before authority I/O, then fully rereads selected
history and compares **both** revision and head-frame digest. A marker-only fence,
earlier receipt or matching task-state hash is insufficient. It rereads the selected
manifest before synchronization, preserving Python's pre-sync hard-failure boundary.
Native additionally binds the raw manifest digest to the fresh validated marker
metadata and requires Boolean source_present; this is deliberately stricter than
Python confirmation-time truthiness under concurrent tampering.

Files synchronize in order: task marker, manifest, baseline source if present,
then every frame from zero through the current sequence. Directories follow:
frames, epoch, task-ledger, state, workspace root. Nothing is published or repaired,
and projection/configuration/events/other epochs/execution authority are not read
or synchronized. There is no post-sync replay or new latest-head guarantee.

The must-use `LedgerConfirmation` distinguishes Confirmed from
SynchronizationUncertain. Failed replay/head verification or manifest reread remains
a hard error. During synchronization, operational path-metadata, open, descriptor-
metadata, file-fsync or directory-sync errors stop immediately as uncertainty;
unsafe paths, detected nonregular descriptors and other policy errors remain hard.
Historical FrozenResult values are not rewritten or wrapped in a new delivery
envelope. Confirmed assesses selected-file synchronization, not publication,
execution permission, or a completed durable task-command retry.

### Confirmed execution reservation checks (SDK only)

`execution::guard_task_mutation(workspace, task_id, borrowed_guard)` and
`guard_authority_change(workspace, borrowed_guard)` consume complete typed replay
state, never report fields or frozen preparation receipts. Internal typed extraction
precedes report projection without changing report values, replay order, Work
charges or V2 prefix-capacity validation. Historical `history()` still acquires no
lock and performs no fsync; the SDK checks are explicit, separate operations.

Task-ID validation occurs first, then matching-guard validation before authority
I/O. A missing execution directory/control returns DisabledObserved without fsync;
pending, null, malformed or unsafe authority never becomes disabled. Active
authority requires a second complete replay matching authority ID and revision,
then fsync of control.json and directories execution, state and root. Only afterward
does derived live attempt state determine a task-specific or authority-wide block.
Confirmation is mandatory even for unrelated tasks, no attempts, all-cancelled
history, approval-only graphs, or an already known prepared blocker.

Only DisabledObserved or ActiveConfirmedClear is successful. A reservation returns
`prepared_attempt_blocks_mutation`; operational synchronization failure returns
`execution_synchronization_uncertain`; current-authority verification failures and
unsafe paths remain errors. Thus an uncertain cancellation cannot clear a guard.
Graph revocation does not cancel prepared attempts; approval alone is not a task
reservation. No current source/task/roster/projection or upgrade backup is read.

The must-use result describes a locked observation, not a transferable permission
token, fresh CAS, receipt-absence proof, approval or task/execution authorization.
Future domain code must retain the same guard and perform its own receipt/head/CAS
and publication checks. DisabledObserved is administrative absence observation,
not durable absence or anti-rollback proof. Two active-authority reads each retain
the existing per-replay bounds; no combined CPU/peak-memory ceiling is implied.

Neither API acquires, replaces or releases the borrowed lock. Obtaining a native
WorkspaceGuard through public `acquire`/`existing` requires a regular coven.json
marker, unlike Python's low-level stores; it does not parse configuration. That
restriction and inspection's existing-lock policy remain unchanged. Only the private
execution-maintenance acquisition below omits the marker requirement.
These confirmation APIs themselves publish nothing or select a new authority.
The SDK baseline and native task protocols call them explicitly; no provider/runtime
request is implied by a reservation check or confirmed historical receipt.
Static cooperating filesystem
assumptions remain; no hostile namespace sandbox or universal crash durability is
claimed.

### Baseline activation and pending recovery

`ledger::import_tasks(workspace, snapshot_pin, borrowed_guard)` now activates a
strictly validated baseline. `resume_import` explicitly resumes only a complete,
unambiguous pending baseline, and `abort_import` archives a pending directory
without deleting it. These effectful library operations now underpin the explicit
state CLI below; the primitives themselves do not deliver events or dashboards,
mutate execution/control authority or dispatch runtime work.

Import validates the matching guard and confirmed execution authority-change gate,
then refuses any marker entry before classifying pending state. It rejects an
existing ledger directory, validates the original snapshot pin and strict task
records, captures raw source bytes/mode, and constructs frame zero, manifest and
marker with fresh UUIDv4-shaped IDs from platform OS entropy. IDs require a complete
successful 16-byte entropy read; no timestamp/PID/fallback generator is used.
All generated final JSON bytes, including the fully wrapped frame and LF, pass
the existing Authority parser and 8 MiB limit **before the first ledger mkdir**.
This catches envelope-added depth/nodes that source-only parsing or encoding alone
would miss. Deterministic profile rejection is intentionally earlier than Python's
interleaved first-import construction.

Directories are created in order task-ledger, epoch, frames, syncing each parent.
Native requests `0700`, subject to umask and inherited special bits, rather than
Python's default mkdir permissions. Optional raw backup, frame zero and manifest
are then published immutably. Private typed artifact roles derive fixed ledger
paths from validated IDs; they do not expose arbitrary authority destinations.
Temporary names are target-derived and reader-recognized. Mode application and
descriptor verification follow writing, then file fsync, exclusive hard link,
parent sync and best-effort cleanup of only the owned temp. Backup mode bits are
preserved exactly; other artifacts receive 0600. Mode/owner/ACL limitations remain
as documented above; silent SGID loss fails before linking, stronger than Python's
unverified chmod. Two directory entries are reserved in reader-enumerated parents,
counting inert temps, while `state` gets no new global enumeration limit.

After publishing artifacts, complete on-disk epoch validation runs again and must
produce sequence zero. Workspace-root sync precedes publication of the **sole
activation marker last**. The marker hard link is the visibility/selection boundary;
its parent sync determines activation durability. Before it, artifacts alone never
become task authority. No source/projection bytes, mode or inode are rewritten.
An absent source remains absent after activation; projection repair is separate.
Public `read()` still selects a marker, fully validates its epoch and fences the
original marker before returning; the preactivation candidate type stays private.

`ActivationReport` exposes the known validated view and separate
ActivationDurability, Confirmed or SynchronizationUncertain. Successful marker link
always returns that known result, even after final directory-sync failure, without
a fallible rediscovery read. `into_view()` discards this operation's durability
classification, so callers should inspect it first. Confirmed describes the
operation's synchronization, not continuing/global authority or execution permission.

Before activation, `ImportFailure` exposes a fixed cause and operation-local
ImportProgress. NoNewArtifacts is not proof that no earlier artifacts exist. Any
already-present marker rejects as NoNewArtifacts, even if its active epoch directory
exists or the marker is malformed/unsafe. Only after marker absence is established
does an observed ledger path count as Pending; first successful mkdir also records
Pending before parent sync. A known linked file updates this invocation's relative
last_published, not a global latest-artifact claim. An uncertain pre-marker sync
stops without activating. A false uncertainty flag does not certify pre-existing
artifacts. Existing finals are never adopted, overwritten or rolled back.

Resume uses the same guard/execution gate, validates the original pin, requires no
marker and bounds **all raw root entries** to exactly one UUID-named epoch. It does
not ignore temp-looking names at that root, choose newest, regenerate missing
finals, or promote orphan temps. It hashes the existing raw manifest rather than
rewriting its formatting, fully validates the epoch, requires sequence zero, and
compares original pin plus exact current source presence/bytes to the validated
backup. Backup mode must match recorded mode; changing only the current source's
mode does not change its byte revision. Resume syncs manifest, frame zero, optional
backup, then frames/epoch/ledger/state/root before linking the marker last.

Abort intentionally does not parse source/epochs or consult execution reservations.
Any marker entry blocks it. A safe pending directory is renamed whole to a fresh,
definitely absent `state/task-ledger-aborted-<id>` destination. Rename failure is an
error; rename success followed by state-sync failure returns the absolute known
path with ArchiveDurability::SynchronizationUncertain, never rollback or a false
“nothing happened” failure. Confirmed covers the archive rename synchronization,
not independent durability of every pre-existing archived file. The state CLI
retains this known archive path and durability classification. Destination absence
plus rename assumes cooperating writers/static paths, not hostile atomic
no-replace semantics.

Existing native JSON/source/frame/history/directory profiles remain mandatory: the
writer cannot activate data its own reader refuses. Raw source and every artifact
are bounded to 8 MiB; complete wrapped parser bounds are enforced, and root resume
enumeration is bounded. Directory access modes and special-bit inheritance remain
OS policy, not an exact cross-platform mode claim. Old binaries/direct-file editors
must cooperate or be stopped operationally. No hostile namespace sandbox, reviewer
authentication, anti-rollback, universal power-loss guarantee or automatic recovery
is implied; all live tests here use disposable local data.

## Explicit unverified handoff collection

`handoff collect` is the first native write command: it captures **local evidence
only**, not accepted execution/results. It requires explicit raw SHA256 and task
revision pins, validates the selected handoff once, and retains its bytes even if
the worker changes the file later. It never follows references or runs checks,
Git, native apps or tools. Fixed reports remain `collected_unverified`,
authorization none, dispatchable false and `local_artifact_capture_only`.

The collector has a dedicated context path, not task-command preflight. It checks
strict depth-16/duplicate-free JSON for roster (64 KiB) and actual legacy snapshot
(1 MiB), or selects/replays active task authority without reading its projection.
Pending/corrupt task authority refuses. Expected/current/claimed revision and exact
task/owner/current roster membership must agree before effects and again under
the shared exclusive flock. Full-task hash includes all extensions using the
existing `coven.handoff-task.v1` domain and compatibility codec. Separate corrupt
runtime/execution controls neither authorize nor block evidence capture. Prepared
attempts remain reserved, and all task/ledger/control/log/dashboard bytes stay put.

Native context uses the bounded finite value profile as well as byte/depth caps:
some Python-valid contexts, such as a 65,537-item extension array, are explicitly
unsupported. This is a tested preview restriction, not a corrupt-authority or
snapshot-fallback classification. Existing historical reads remain independent.

Directory preparation, publication and confirmation all require a matching owned
`WorkspaceGuard`. New output dirs request mode0700; each dir and its parent are
synced, including existing parents on retry. A mode0600 temp is created in the
destination directory, written/synced, then exclusively hard-linked to the final
content ID. No overwrite/rename fallback. A pre-link failure leaves no final; a
post-link directory-sync failure retains final bytes and returns
`durability_uncertain`/exit2. Cleanup only attempts this operation's own temp;
orphan temps stay inert. Entry/byte accounting reserves both links before publish.

Exact existing bytes are observed under the guard before directory preparation.
A private-field `ObservedEvidence` token preserves that fact for confirmation.
Later operational read/open/metadata/file-sync/dir-sync failures return existing
uncertainty, not ordinary rejection; mismatched bytes and unsafe paths still fail
without replacement. Identical Python/Rust retries preserve ID, bytes, inode and
mtime. No timestamp/index/secondary log or authority transaction is added. Context
must still be current on recollection; this is not frozen task-ledger command
receipt replay or permission to repeat an uncertain native task.

## Historical execution-control v1/v2 reader

`execution history WORKSPACE --json` is a new native preview report, not a port of
Python's current-source execution inspection or mutation delivery envelope. It
requires only an existing directory, not current `coven.json`, task, checkpoint or
runtime files. It loads only managed `state/execution/control.json`. Missing
directory/control means disabled; a directory without control is pending and
refuses. Present null, malformed authority, unsafe types, unreadable data and
unsupported schemas never become disabled or an empty reservation report.

The reader fully replays v1 enable, enrollment/source rebind, preparation and
cancellation. It checks exact schemas and literal integers, generation/history,
unique request keys, every expected prefix revision with final extensions, retained
source identities/rosters/task records, ledger task-content digests, complete
enrollment/preparation snapshots, domain-separated task/intent hashes, predecessor
and numbering rules, unique identities and the sole permissible frozen results.
Snapshot revisions retain their raw-byte meaning: parsed historical tasks cannot
reconstruct an unavailable original snapshot's raw hash. Final canonical length
plus LF and 512 KiB per prepared attempt must fit Python's 16 MiB authority budget.

Reports retain enrollments, attempts, receipts and extensions. Cancellation changes
derived attempt state but never rewrites the earlier preparation receipt's
`state: prepared`. Sorted `reserved_task_ids` lists only still-prepared tasks;
enrollment, staleness or unrelated current-file changes cannot grant or release a
reservation. Status active means valid v1/v2 historical authority, not active execution.
Both active/disabled reports say authorization none, dispatchable false,
assessment historical_authority_only and current_source/durability not_assessed.

Reads do not create/acquire a lock, fsync, publish, repair, archive or load current
sources. The observed revision is not a latest/linearizable, authenticated or
durability-confirmed view. Historical reservation observation is **not** a reusable
mutation guard: writers still need receipt-before-mutable-state ordering, matching
lock ownership and durable authority confirmation. Native cancellation and explicit
recovery use the separate maintenance protocol below; enable/enroll/prepare,
upgrade and graph review use the separate authoring coordinator. This execution
authority still grants no runtime admission; the separate reviewed cold-launch
action protocol below supplies its own durable claim boundary. Native task writes
call the separate confirmed guards rather than treating history as permission.
Task and handoff readers/collectors retain their existing independence from this
execution history path. Unsupported or malformed schemas refuse rather than
returning partial legacy state or an empty reservation report.

### Complete v2 historical graph/review replay

V2 validates its complete embedded v1 authority before any graph-era history:
no active legacy preparations, unchanged identities/extensions, exact upgrade
metadata and first/unique upgrade request. Raw source SHA is shape-validated, not
reconstructed from canonical values; the exact backup path and mode are retained
data only. Missing, corrupt, unsafe or FIFO backups are never opened. Original
legacy bindings, record schemas, receipts, cancellations and attempt order carry
forward directly as validated state, not by reparsing a legacy report.

All v2 requests bind the exact prior revision, starting at the validated legacy
revision. Prefixes include the embedded authority, upgrade and final extensions.
Every v2 prefix, not only the final state, must retain 512 KiB per prepared attempt
and 512 KiB for an approved current graph head within Python's 16 MiB limit.
Embedded v1 keeps its original final-only reserve policy.

Pure graph helpers enforce closed schemas, 1..64 unique nodes, at most 256 edges,
no duplicate/dangling/self/cyclic dependencies, inclusive 1 MiB canonical document
and manifest limits, and exact task/node/manifest digest domains. Persisted node
and dependency order stays intact; lexical topology is validation, not a rewrite.
Proposal replay re-freezes the document against the retained **full** source and
replayed enrollment bindings, including current retained task extensions rather
than an enrollment's older snapshot. A self-consistent manifest alone is not enough.

Historical approval and graph preparation match all selected nodes against their
entry's retained source. Unrelated full-source revision drift may still be current;
selected snapshot, authority, enrollment or roster drift cannot. Graph family/head,
successor, globally unique decision IDs and latest decision binding are checked.
Managed task IDs remain sticky across removed/superseded nodes, so standalone
preparation cannot bypass graph rules. Dependencies do not gate preparation.
Revocation never cancels preparations; cancellation never revokes review.
V2 intent hashes include graph_binding, including explicit null for standalone
unmanaged preparations; legacy v1 records are not given new fields or digests.

Schema2 reports add graph_id, current_graph_revision, graphs and sorted
graph_managed_tasks to the existing historical envelope. Graph review states and
source_match inside frozen receipts are historical facts only: no fresh approval,
preparation eligibility, current-source match, durable writer guard or execution
admission is inferred. Graph mutation and upgrade require the separate writer protocol.

Native profile: 8 MiB raw input, the shared finite value/output bounds, at most
1024 total entries across legacy plus v2, and conservative prefix preflight
`(whole_control_canonical_length + 32) * total_entries <= 256 MiB` before either
version clones/encodes history prefixes. Standalone v1 keeps its existing profile.
All reducer canonical comparisons and hash encodings, including graph helpers and
embedded legacy replay, charge one shared 256 MiB aggregate accepted-work budget.
A refusal may perform one further encoding capped at 32 MiB before charging;
this is neither a
strict CPU/memory ceiling nor a linear replay algorithm. Final compact and escaped
ASCII reports each have their own 32 MiB bound and finish before stdout is written.

Timestamp acceptance is deliberately narrower than Python's `fromisoformat`:
ASCII `YYYY-MM-DD[T or space]HH:MM:SS[.1..6 digits](Z|+/-HH:MM)`, years 0001..9999,
Gregorian calendar/leap-day checks, hours/offset hours 00..23, minutes/seconds/offset
minutes 00..59. Spelling including `-00:00` is preserved; no chronology is invented.
Other Python-valid ISO forms return unsupported_execution_timestamp_profile.
These limits are explicit preview restrictions, not a claim that refused Python
authorities are corrupt, nor a reason for fallback or persisted-value rewriting.

## Native execution inspection and graph export

### Overview

All five read-only native commands below are available. Three boundaries stay
distinct: unchanged `execution history`
reports frozen authority without current source; inspection fully validates history
then optionally assesses current source under an existing lock; graph export uses
one complete replay and no current-source dependencies. None dispatches, writes,
fsyncs, repairs, confirms durability or grants continuing freshness/authorization.
No dependency, production Python, `bin/coven`, runtime/provider, fallback, install,
main integration, push, persisted state or schema change is included.

### API / Interfaces

```text
execution inspect [WORKSPACE] [--json]
task attempt list [WORKSPACE] [--task TASK_ID] [--json]
task attempt inspect [WORKSPACE] ATTEMPT_ID [--json]
task graph inspect [MARKER_WORKSPACE] [GRAPH_REVISION] [--json]
task graph export [WORKSPACE] GRAPH_REVISION [--json]
```

The five `execution::*_arguments` interfaces return `TaskOutput`. Default output
is ASCII-escaped JSON with two-space indentation; `--json` is compact with no
separator spaces. Main adds exactly one terminal LF. Complete output admission
precedes stdout; there is no human renderer. Authorization remains `none` and
dispatchable remains `false`, including eligible graph/node projections.

Attempt list validates `--task` **after full inspection**, filters only `attempts`,
and removes `enrollments`; it does not narrow history/source/graph validation.
Attempt inspect and export use positional arity: one operand is always the required
selector in cwd, even if it names a marked path; two mean workspace then selector.
Their selector must be 32 lowercase hex digits and validates **before root access**.
Attempt inspect returns the minimal inactive envelope for a valid unknown ID; active
inspection requires a known attempt, returning `attempt` without `attempts` or
`enrollments`. Export requires active v2 and a known graph, returning frozen
`manifest`, `graph_digest`, captured `execution_revision` and the two trust fields.

Graph inspect instead selects an optional marker-bearing workspace first. Excess
operands fail before inspection; inactive envelopes and the v1 upgrade refusal
precede bad-revision validation. Omitted/empty revision means current head; no head
returns `graph: null`. Active v2 output contains only `execution`, selected `graph`
and the two trust fields. Execution inspect and attempt list default workspace to cwd.

The existing Unicode-Nd negative-prefix lexical adapter extends only to these five
added routes/arguments, not all commands. Typed parsers restore exact operands and
`--task` values (including attached values); the last repeated task flag wins.
Repeated flag indices and operand-group timing remain intact: selector-group
`--json` belongs before or after the group, not between operands or before a trailing
terminator after operands. No global arbitrary-hyphen acceptance is enabled.

### Data Flow

1. Resolve an existing root. The initial control observation fully replays retained
   history. Disabled/pending/unavailable states return minimal envelopes with exit 0
   and no source reads; present corrupt/null control never becomes disabled.
2. Active inspection makes a **second complete replay outside the coordination-error
   catch**. Its failure is hard, not downgraded to unavailable source.
3. Try only `WorkspaceGuard::existing`, waiting up to five seconds. Never create a
   lock or directory, steal a lock or replace its inode. The inherited guard still
   requires a regular `coven.json` marker without parsing its contents. Marker-free
   history/export and uncoordinated inspection remain valid; marker-free coordinated
   source capture is not added.
4. Under the guard, replay execution again before source capture; a disabled
   transition returns minimal disabled. Load the full workspace (including config,
   checkpoint and runtime-launch control validation), discard its earlier tasks,
   then independently select/replay the current task ledger or freshly capture raw
   snapshot bytes and validated tasks. Revision and tasks come from that same fresh
   observation. Missing versus empty raw snapshots retain distinct revisions.
   Active ledger capture uses authority, not projection health; pending import
   refuses source assessment. Strict identity/revision/tasks/roster checks follow.
5. Ordinary source/config errors retain the captured frozen facts with bounded
   native-code diagnostics. Missing/busy/unsafe existing locks yield uncoordinated
   source, never a freshness grant. Other coordination failure reclassifies authority
   once more: return an established inactive envelope, or retain the prior captured
   active view, never pairing a new execution revision with old source.

Export bypasses that assessment entirely. It fully replays all retained history
before selecting a graph; corrupt **anywhere** in history is fatal, even beyond the
selected proposal. Current config/tasks/checkpoints/runtime control/locks and upgrade
backups are not opened, so bad current files do not prevent a valid frozen export.

### Edge Cases

- Binding precedence is source unavailable, authority changed, task missing, task
  digest changed, frozen owner missing, whole-source revision changed, then current.
  Enrollment uses its latest binding; every attempt uses its frozen preparation,
  including cancelled attempts. Unrelated task changes can therefore stale an
  attempt's whole-revision binding without changing its selected task.
- Graph matching uses selected inputs: unrelated whole-revision drift can leave
  `source_binding.status: current` with `whole_revision_changed: true`.
  Graph arrays follow proposal order, not the historical BTree map's lexical order;
  frozen node/dependency arrays and decisions retain their order. Only current-head,
  approved, source-current graphs are preparation-eligible. A node with an active
  prepared task is ineligible. Dependencies remain `unestablished` or `not_required`
  regardless of eligibility; they are not accepted execution results.
- Revoking review never cancels a preparation; cancellation never revokes review.
  `affected_preparations` includes cancelled graph-bound attempts. These projections
  are preparation-only observations, not writer permits or dispatch admission.
- Ordinary unsupported config versions/multiplexers, nonfinite/surrogate source
  values and other source errors are diagnostics, not lost frozen facts. Explicit
  resource/work/output/encoding-profile refusals are hard exit 1 with no stdout,
  including nested ACTION runtime-control input, encoding and work limits. The
  reviewed downgrade bug is fixed: real oversized/BOM/UTF-16 runtime-control cases
  exercise fatal input/encoding behavior, with a private fault seam for action work.
- Inspection makes at most **four execution reads**, each with its own existing
  **256 MiB replay-work budget**. A separate **shared 256 MiB assessment budget**
  spans source, all bindings, all graphs and final ASCII output; it is not reset per
  graph. Existing **8 MiB control / 1024 combined history-entry** limits remain.
  Graph bounds remain **64 nodes / 256 edges / 1 MiB manifest**; encoded and final
  ASCII output each remain bounded to **32 MiB**. These are accepted-work/profile
  bounds, not absolute CPU, heap, peak-memory or filesystem-time guarantees.

### Example usage

Use an existing disposable workspace and actual IDs from inspection; do not create
or delete coordination files just to obtain a source assessment:

```sh
target/debug/coven execution inspect WORKSPACE
target/debug/coven task attempt list WORKSPACE --task TASK_ID --json
target/debug/coven task attempt inspect WORKSPACE '<attempt-id>' --json
target/debug/coven task graph inspect WORKSPACE --json
target/debug/coven task graph inspect WORKSPACE '<graph-revision>'
target/debug/coven task graph export WORKSPACE '<graph-revision>' --json
```

### Related files

`src/main.rs` and `src/cli_operands.rs` route/restore arguments;
`src/execution/inspection.rs` orchestrates reads, existing-lock assessment and output;
`src/execution/source.rs` captures fresh source; `inspection/commands.rs` selects
wrappers; `inspection/projection.rs` derives pure observations. The latter two are
under `src/execution/` (there is no `src/execution/projection.rs`). Existing
`execution.rs`, `execution/{v2,graphs}.rs`, `workspace.rs`, `tasks.rs`, `ledger.rs`
and `storage.rs` supply private replay, validation and locking contracts.
`tests/execution_inspection.rs` runs `tests/_execution_inspection_parity.py`;
module-local source, projection and state-machine tests cover private seams.
See [EXECUTION_INTENT_CONTRACT.md](EXECUTION_INTENT_CONTRACT.md) and
[TASK_GRAPH_CONTRACT.md](TASK_GRAPH_CONTRACT.md) for retained preparation contracts.

## Native execution cancellation and recovery

### Overview

Native maintenance supports cancellation across v1, v2 and retained legacy-v1
receipts after upgrade, plus both explicit recovery modes. It maintains local
preparation authority only: no native process stop, submission or dispatch follows.
Authorization remains `none`, dispatchable `false`. Current workspace config,
`coven.json` marker, tasks, roster, checkpoints and runtime controls are not loaded
or used as guards. In particular, a preparation does not block its own cancellation.
No events, dashboard, task projection or current-source files are written.

Enable, enroll, preparation in **both intent and graph modes**, upgrade and graph
mutations are implemented by the separate authoring coordinator below. Repository
commands use the separate bounded observation/registration workflow below. This
maintenance milestone changed no dependencies, production Python,
`bin/coven`, runtime/providers, fallback, installation or default-entrypoint cutover
are changed. All other command contracts remain unchanged.

### API / Interfaces

```text
task attempt cancel [WORKSPACE] ATTEMPT_ID --reason TEXT --expect-execution-revision TOKEN --request-id KEY [--json]
execution recover [WORKSPACE] --confirm-authority --expect-execution-revision TOKEN [--json]
execution recover [WORKSPACE] --archive-pending [--json]
```

Cancellation uses positional arity: one operand is the required attempt ID in cwd;
two mean workspace then ID. IDs are 32 lowercase hex digits. Reason is nonblank,
at most 16 KiB UTF-8, and preserved without trimming or normalization. Keys are
1..128 ASCII bytes, start alphanumeric and otherwise allow alphanumeric, `_`, `-`,
`.` and `:`. The revision participates in the versioned request binding.

Recovery requires exactly one mode. Confirm requires a syntactically active v1/v2
revision **semantically before lock acquisition**; this is not parser-enforced
conditional flag admission. Archive rejects any revision **under the lock**.
Repeated value flags use their last values. All flag occurrences remain visible
to operand-group validation: a later repeat cannot hide an earlier misplaced flag.
The narrow Unicode-Nd negative-prefix adapter restores exact operands/option values;
it is not global arbitrary-hyphen acceptance. Flags belong outside a multi-operand
group, not between its operands or before a trailing terminator after operands.

Output is ASCII-escaped, two-space pretty JSON by default, compact with `--json`,
with exactly one terminal LF. Cancellation returns frozen `result` plus live
`delivery`: confirmed new/replayed receipts exit **0**, known-result uncertainty
exits **2**, ordinary errors exit **1**. Recovery success exits **0**; confirmation
or archive failure exits **1**, not cancellation's known-receipt exit 2.

### Data Flow

1. Cancellation resolves an existing root and fully reads/replays authority **before
   semantic binding admission**. Binding validation precedes request-key validation;
   deterministic invalid inputs and disabled authority refuse before the first lock.
2. Crate-private `WorkspaceGuard::execution_maintenance` waits up to five seconds
   and may create `state/` and its persistent `.workspace.lock`, without requiring a
   marker. It uses the same Python/native flock inode and root/PID/thread ownership,
   path/type safety and process-death release as other writers. Never delete or
   replace that inode. Public `acquire`, `existing` and inspection marker policies
   are unchanged; this is not a new public marker-free general writer API.
3. Under that guard, fully replay again, then look up the retained receipt **before
   current CAS or live-attempt eligibility**. The retained binding's schema selects
   the comparison namespace, including v1 cancellation retries after v2 upgrade.
   A matching key returns the original frozen result; changed reason, revision,
   operation or attempt ID conflicts. Corrupt history anywhere cannot shortcut to a
   matching receipt. A fresh key requires exact current CAS and a prepared attempt;
   a second fresh cancellation is rejected.
4. Derive the effect/result with the existing checked reducer, append one entry and
   generation, and serialize canonical control plus LF. **Exact stored bytes,
   including LF**, must fit 8 MiB, parse under `Profile::Authority` and pass complete
   typed reader replay before publication. Both pretty/compact, committed/uncertain
   output variants are admitted before replacement, even for the unrequested form.
5. Mode-verified same-directory atomic replacement publishes control. Existing
   `07777` mode bits are restored and verified after writing; absent targets receive
   verified `0600`. No ownership, ACL or xattr preservation is claimed. Known rename
   uncertainty returns immediately with the retained result, **without a confirmation
   reread**, rollback or blind retry.
6. Confirmed replacement and exact receipt retry use the private borrowed-guard
   confirmation helper: completely reread typed authority, match UUID and revision,
   then fsync control, execution directory, state and root, in that order. Prepared
   attempts are allowed; this is not the task-writer reservation guard. Any post-known
   read/profile/unsafe-path/sync failure, including failed retry confirmation,
   preserves the frozen result with exit 2, `current_revision: null`, retained
   `known_control_revision`, and `attempt_state: null`.

Explicit recover-confirm checks exact CAS under the lock and uses that same full
reread/sync sequence, without changing generation or bytes. It returns no
known-receipt uncertainty envelope on failure. Frozen cancellation results are not
confirmed reservation release or transferable permission: later task/state writers
must independently confirm execution authority under their own held guard.

### Edge Cases

- **Historical cancellation liveness remains bounded.** An older valid predecessor
  can have a cancellation successor refused by the **8 MiB exact stored-byte limit,
  1024 combined legacy/v2 history entries, 256 MiB prefix/replay-work profile, finite
  value profile or 32 MiB output limit**, despite Python's 512 KiB per-preparation
  reserve within its 16 MiB authority policy. There is no pruning, limit raising or
  fallback. New native authoring reserves escape headroom as described below;
  historical reads/cancel/recover keep their accepted limits unchanged and do not
  rehabilitate arbitrary older control. Work bounds apply per replay, not as one
  command-wide CPU/heap/latency guarantee.
- After uncertainty, inspect before blindly choosing a new key. The original key
  with exact original arguments retrieves its authoritative historical result,
  not fresh permission or proof of current authority/durability.
- Archive requires a safe pending `state/execution` directory and **no control
  entry whatsoever**: present null, malformed, symlink or other unsafe control
  blocks it. Absent pending state is an error. It does not parse or promote orphan
  files, restore authority, reset state or clean up evidence.
- Archive checks a fresh absent `state/execution-pending-archive-<uuid>` target,
  renames the **whole directory**, then syncs state. This assumes a static/cooperating
  namespace; it is not atomic no-replace protection against a hostile target race.
  A post-rename sync failure exits **1 with empty stdout** and a diagnostic retaining
  the known archive path and stating no rollback occurred. It must not be confused
  with task-ledger abort's successful exit-2 uncertainty delivery.

### Example usage

Use an **existing disposable workspace with prepared authority** (native or Python). Replace
every placeholder with actual inspected IDs/revisions; do not auto-enable, enroll,
prepare, upgrade or start a runtime as part of maintenance:

```sh
target/debug/coven execution inspect WORKSPACE --json
target/debug/coven task attempt inspect WORKSPACE '<actual-attempt-id>' --json
target/debug/coven task attempt cancel WORKSPACE '<actual-attempt-id>' \
  --reason 'Revise assignment' --expect-execution-revision '<inspected-execution-token>' \
  --request-id cancel-review-1 --json
# To retry this request, retain that exact ID, reason, token and key.
target/debug/coven execution history WORKSPACE --json
target/debug/coven execution recover WORKSPACE --confirm-authority \
  --expect-execution-revision '<current-inspected-execution-token>' --json
```

Only for a separately inspected disposable pending directory with **no control**:

```sh
target/debug/coven execution recover PENDING_WORKSPACE --archive-pending --json
```

### Related files

`src/execution/mutation.rs` owns cancellation and `mutation/delivery.rs` preadmits
known-result delivery. `execution/confirmation.rs` and `execution/recovery.rs`
implement full confirmation and explicit recovery. `execution.rs` and
`execution/{v2,graphs}.rs` supply typed replay/bindings/reducers; `storage.rs`,
`storage/maintenance.rs` and `storage/replacement.rs` supply the same persistent
guard and mode-verified publication. `src/main.rs` and `src/cli_operands.rs` route
and restore arguments. Module-local fault tests and
`tests/execution_maintenance.rs` / `tests/_execution_maintenance_parity.py` cover
CLI parity and mixed-writer behavior. No current-source loader is in this pipeline.

## Native execution authoring

### Overview

All six remaining runtime-free execution writer routes are implemented. Fresh
standalone and reviewed two-node graph workflows can start with native init/task
add, without Python/SDK bootstrap. This is preparation authority, not an agent
executor: authorization stays `none`, dispatchable `false`, dependencies do not
gate preparation, and review revocation does not cancel existing attempts.
Python source remains only as a development oracle; the launcher and installed
product are native-only. Source deletion still requires separate cleanup approval.

### API / Interfaces

| Route | Required inputs / variants |
| --- | --- |
| `execution enable [WORKSPACE]` | Execution CAS and request key; initially `execution-disabled:v1` |
| `execution enroll [WORKSPACE] TASK_ID` | Task/execution CAS and request key; optional `--rebind-source` |
| `execution upgrade [WORKSPACE]` | V1 execution CAS and request key; no active preparations |
| `task attempt prepare [WORKSPACE] TASK_ID` | Exactly one of `--intent FILE` / `--graph-revision REV`; task/execution CAS and request key |
| `task graph propose [WORKSPACE] --document FILE` | Task/execution CAS and request key; v2 required |
| `task graph decide [WORKSPACE] REV` | `--decision approve` / `changes-requested`, `--reason`, `--expect-graph-digest`, execution CAS and request key; task CAS required only for approve, forbidden for changes-requested |

CAS flags are `--expect-execution-revision` and `--expect-tasks-revision`; the key
flag is `--request-id`. Default output is pretty ASCII JSON, `--json` compact,
each with one terminal LF. Frozen `result` and live `delivery` are distinct:
confirmed new/replayed receipts exit 0, known-result uncertainty 2, ordinary service
errors 1 (parser errors can also exit 2). Keep exact original arguments for retries.

### Data Flow

1. Intent/document adapters read and strictly validate the selected **file first**,
   before invoking the mutator, even for receipt replay. Bind parsed values, not
   pathname or formatting; missing/unreadable retry files still fail at this boundary.
2. Initial enable preflight uses `source::observe` with a zero snapshot ID to perform
   **full workspace validation before lock creation**. Under the persistent shared
   guard, fresh capture repeats workspace preflight and independently selects ledger
   authority or raw snapshot tasks/revision using actual generated IDs. Preflight
   tasks are discarded rather than paired with a later revision.
3. Fully replay under the guard. Retained receipt schema controls binding comparison,
   including v1 retries after upgrade. Lookup precedes fresh CAS, source, graph and
   backup work. A matching key returns its original result; conflicting values refuse.
   Fresh requests derive effects through typed reducers, not report projections.
4. Before activation directories, upgrade backup or control replacement, serialize
   canonical control **plus LF**, enforce 8 MiB, parse `Profile::Authority`, fully
   replay with measured Work, admit native escape headroom, and preadmit both pretty/
   compact and confirmed/uncertain output variants.
5. Activation exclusively creates the execution directory with normal `0777`/umask
   semantics, syncing state/root; no pending-directory adoption. Control uses
   mode-verified atomic replacement (existing `07777` bits; verified default `0600`).
   Upgrade first preserves exact raw v1 bytes/mode and source SHA, exclusively writes
   a fresh backup, verifies readback/mode and syncs backup through the root chain.
   It needs no current source. Failed backups remain inert; no orphan adoption.
6. Known replacement uncertainty immediately retains the result with exit 2; a
   confirmed replacement or known retry performs full authority reread and the
   control/execution/state/root sync chain. **Any post-known confirmation failure**
   preserves the result with exit 2. Upgrade retry lookup needs no backup access.

### Edge Cases

- Writer-only admission reserves one escape for every active attempt plus the
  currently approved graph head: 512 KiB per closed cancellation/revocation entry
  with worst-case reason escaping, plus history entries, parser nodes, prefix
  capacity and actual replay Work within 256 MiB. Both current output forms are
  independently bounded. See private `capacity.rs` for the proof, not a new public
  resource guarantee.
- A real approval revocation **spends** its reserve; another fresh changes-requested
  decision on an already-revoked head is history growth and must preserve all other
  escapes (including A's cancellation). Historical control is not automatically
  rehabilitated. Existing historical read/cancel/recover acceptance is unchanged.
- Hard profiles remain explicit: 8 MiB stored control including LF, 1024 combined
  legacy/v2 entries, 256 MiB replay Work, 1 MiB documents, 64 graph nodes/256 edges,
  and 32 MiB output, plus finite JSON limits. Refusal is not permission to prune,
  raise caps, rewrite old authority or fall back to Python.
- Pending/corrupt authority fails closed. Incomplete activation/backup evidence is
  retained for inspection. Static/cooperating local paths and Linux-only validation
  remain the scope, not hostile-path or universal crash/power-loss guarantees.

### Example usage

See the README's [fresh standalone loop](README.md#fresh-native-standalone-loop)
and [reviewed two-node loop](README.md#fresh-native-reviewed-two-node-loop). They
explicitly label setup as pseudocode and require inspected tokens/IDs. Rebinding
uses the enrollment route with `--rebind-source`, not implicit source adoption.

### Related files

`src/main.rs` routes `execution/mutation/commands.rs`; its `WriteOptions` and
`PreparationInput` feed private `lifecycle.rs`, `effects.rs`, `graphs.rs`,
`upgrade.rs` and `capacity.rs`. `execution/source.rs`, `execution/{v2,graphs}.rs`,
`confirmation.rs` and `storage/replacement.rs` supply fresh source, typed replay,
confirmation and publication. `tests/execution_lifecycle.rs` and its temporary
`_execution_lifecycle_parity.py` parent oracle cover CLI workflows; module-local
tests own activation/upgrade faults and coordinator capacity proofs. See also
[EXECUTION_INTENT_CONTRACT.md](EXECUTION_INTENT_CONTRACT.md) and
[TASK_GRAPH_CONTRACT.md](TASK_GRAPH_CONTRACT.md).

## Runtime-launch action inspection and checkpoint views

`action inspect [WORKSPACE] ACTION_ID [--json]` requires only an existing root; one
operand means an action ID in the current directory, while two supply an explicit
workspace (including quoted empty workspace and tilde expansion). No coven.json
marker or current task/config/runtime preflight is required. The command validates
the complete launch control before selecting a claim, then reads only that action's
bundle and dedicated `runtime-launch-<id>` checkpoint. It neither scans other
bundles nor follows frozen inventory paths, environment values or shell strings.
Default output is pretty literal-Unicode JSON; optional `--json` retains compact
ASCII output.
Both have one terminal LF. `task list` now defaults to human output with optional
`--json`; both default-output implementation gaps are closed, not retirement gates.

Control validates all claims, including unselected ones: exact schemas and literal
integers/booleans, UUID/reference tokens, head membership, unique claim IDs, status/
acknowledgement/report rules, and every frozen plan. Actual resolved root/device/
inode must match persisted identity. Platform code alone acquires OS identity;
persisted arbitrary decimal integers are compared without truncation. Root aliases
resolve normally; copies/moves never silently rebind launch authority. Non-UTF-8
actual roots explicitly refuse. A missing control disables only without an actions
directory; present null, orphan bundles, unsafe paths and corrupt controls refuse.

Plans are rebuilt exactly for tmux or Herdr, preserving ordered agents, literal
commands/starters/environment, typed handles/output selectors, argv, steps and
integer sleep values. No executor, executable lookup, sleep or environment capture
exists in this module. Plan validation is structure, not permission. Terminal
reports follow Python's intentionally permissive schema: step counts/indices and
handle inventories are not inferred from plan length or backend. Acknowledged
terminal claims and late results on an older claim remain valid.

Bundles validate their own identity and plan while keeping `inputs` an opaque
finite object. They need not match today's activation/control revision for
inspection. Manifest/control token labels are not hash preimage domains: each
hashes sorted compact UTF-8 values without LF. An internal classifier compares
only validated consumed manifest/checkpoint/decision references, with no optional
evidence/current-source callbacks. It is not a durable retry or submit API.

Successful output preserves Python's inspection envelope and `outcome: inspected`;
exit is zero even when a retained claim is claimed, outcome_unknown or failed.
Full control and selected claim remain independently visible despite ordinary
missing/corrupt/unsafe bundle or checkpoint artifacts. Existing diagnostic string
slots use stable native codes rather than Python OS/exception prose:
`action_bundle_missing`, `action_bundle_unsafe_path`, `action_bundle_unavailable`,
`action_bundle_invalid`, and checkpoint unsafe_path/unavailable/invalid equivalents.
Missing checkpoint is a successful missing view, not a diagnostic. These fields
are not a fresh approval, readiness, coherent latest snapshot, authentication or
durability claim. No new grant fields or durable mutation envelopes are invented.

The pure checkpoint API ports Python's exact byte scanner, not a general Markdown
parser. It preserves LF/CRLF/bare-CR offsets, fences, comments, list nesting,
reserved three-line blocks, direct checkboxes and history separator/trailer rules.
Only specified presentation/machinery spans are excluded, and remaining original
bytes are concatenated and length-framed once for the scope hash. Snapshot hashes
cover every original byte; missing and empty differ. Contradiction takes precedence
over stale scope; stale changes-requested remains rejected. Decided-at is scalar
text, not an ISO timestamp requirement. Linked external content is never read.
The standalone command below adds Python's broader workspace preflight separately;
action inspection continues using only the narrow pure checkpoint reader.

Native profile: raw action authority at most 8 MiB, UTF-8 without BOM, existing
finite value limits, compact canonical authority at most 16 MiB, sorted/spaced plan
at most 8 MiB, selected checkpoint at most 1 MiB. Python accepts some BOM/UTF-16/32
authority encodings; native returns `unsupported_action_encoding_profile`, even for
optional bundles, rather than labeling this profile gap ordinary corruption.
One command-scoped 256 MiB accepted serialization budget covers control/plans/
reports/bundle/final compact and escaped output. A refusal may encode one additional
bounded value before charging; this is not a CPU/peak-memory ceiling. Compact and
escaped reports each have an independent 32 MiB bound, both complete before stdout.
Library inspection charges compact output only; CLI also charges ASCII presentation.

Resource/profile errors anywhere are fatal with no success stdout—even inside
malformed checkpoint machinery or an optional bundle. Ordinary malformed optional
content remains diagnostic. No lock acquisition/inspection, fsync, transport,
publication, recovery, current input/task/projection validation or runtime/provider
call is performed. Existing task/runtime fences remain unchanged.

## Standalone workspace-aware checkpoint inspection

### Overview

Stage16 (uncommitted work after `52d376a`) completes native inspection presentation
and adds read-only next guidance, not decision writers or execution authority.

### API / Interfaces

`checkpoint inspect [WORKSPACE] [CHECKPOINT_ID] [--json]` reuses the pure byte reader
only after ordered workspace validation. No operands means cwd and plan-approval.
The first operand selects a workspace **only** when its expanded spelling has an
existing or symlinked `coven.json`; otherwise all original operands are checkpoint
selectors in cwd. This is marker-based, not a two-operand arity heuristic or ancestor
search. Marker directory/symlink types can select a workspace and fail managed-path
validation later. Root aliases resolve, while marker contents are not selection data.

Default output is safely ASCII-escaped JSON with two-space indentation and one
terminal LF. `--json` retains existing compact CLI bytes (no separator spaces) and
one terminal LF. `checkpoint_workspace::inspect_text_arguments(items, json)` adds
presentation without changing `inspect_arguments` or `inspect_json_arguments` and
their compact byte/profile contracts. The library text functions exclude the LF
that main adds; canonical/hash/storage encodings are unchanged.

### Data Flow

Preflight follows the existing Python order: known managed directories and task
marker path; selected ledger replay; managed files; conventional checkpoint text
and sorted actual-name scan; persisted team and TOML overrides/effective team;
environment multiplexer and complete launch-control validation; repeated paths,
ledger and checkpoint text checks plus roster paths; third independent ledger
read or legacy tasks; agent/lead records. Only then are excess operands or an
invalid selected ID rejected. Missing selected checkpoints succeed after preflight.
Unrelated recognized checkpoint files require valid UTF-8 text, not valid decision
grammar. Known log/prose/prompt paths are type-checked without parsing their content.

Pending ledger evidence without a task marker is allowed with a valid snapshot,
unlike the task-list preview's conservative pending fence. Active authority is fully
validated without projection path/type/content or health reads. A corrupt selected
ledger never falls back. All three ledger reads remain distinct observations;
there is no added cross-pass/final equality or pending/runtime fence. Each reused
`ledger::read` retains its own existing internal marker checks. This is not an
atomic, latest or coherent workspace snapshot or a reusable mutation guard.

The narrow launch-control-only seam validates every claim/plan/report and actual
workspace location, including orphan-control rules, but never opens bundles or
dedicated checkpoints or constructs action output. Valid unresolved claims do not
block checkpoint inspection. Existing action inspection remains configuration-
independent; existing task runtime/pending fences and pure checkpoint semantics
are unchanged. Execution authority is not added as a dependency.

TOML runtime overrides apply trimmed strings to an effective team and revalidate it;
environment selection cannot rescue invalid persisted or effective configuration.
Nested TOML dates/times are checked against Python calendar/year/clock/timezone
constraints (including rejection of leap second 60 and year zero). The cached
parser's signed-i64 integer bounds and rejected positive floating overflow are
explicit `unsupported_checkpoint_toml_scalar_profile` restrictions, not a claim that
Python rejects those ignored scalars. Ignored TOML NaN/inf literals and parser-
accepted negative overflow are not subjected to the finite JSON policy. Numeric
refusal classification depends on the pinned parser diagnostics; some malformed
overflow-prefixed integer tokens can receive the same unsupported-profile code.
Both implementations still refuse those tokens. No alternate parser/dependency or
value rewriting is introduced, and full TOML grammar equivalence is not claimed.

### Edge Cases

`--json` is optional and repeatable, matching Python's flag-free pretty mode.
Unambiguous long-option prefixes (`--j`, `--js`, `--jso`) are supported for this
command. The flag belongs before or after the operand group, not between operands
or before a trailing terminator following operands, matching Python parser errors
before workspace loading. Every occurrence is retained with Clap `Append` for this
positional-group check; a later repeat cannot erase an earlier misplaced flag.
Tilde expansion precedes the marker probe but never rewrites a retained
checkpoint selector; named-user expansion and HOME-absent implicit lookup remain
explicitly unsupported. Non-UTF-8 paths and directory names refuse without lossy
conversion, including unrecognized names that Python could ignore.

Only `checkpoint inspect` and `next` use Python 3.14 argparse's lexical negative
operand rule: the **prefix** regex `-\.?\d`, not float parsing. Thus `-1anything`
and `-.٥` are operands too. `\d` is frozen Unicode 16 Nd (760 code points in 71
ranges). Collision-free lexer tokens restore the original `OsString` and argument
indices before service use: no numeric/path normalization, broad unknown-flag
acceptance or change to other command parsers. `--` retains its ordinary meaning.

Limits are per read/pass: 8 MiB finite JSON, 1 MiB checkpoint/TOML text, 4096 total
directory entries per checkpoint scan, existing ledger replay limits and action
control encoding/work limits. Conventional checkpoint files are checked before
the bounded sorted scan. Compact and escaped pretty/compact presentation finish within
32 MiB before stdout. Repeated passes do not imply an aggregate CPU/peak-memory
ceiling. No locks, fsync, publication, repair, approval, task/control mutation,
transport, runtime/provider call or Python fallback is added.

### Example usage

```sh
target/debug/coven checkpoint inspect WORKSPACE plan-approval
target/debug/coven checkpoint inspect WORKSPACE plan-approval --json --j
target/debug/coven checkpoint inspect # cwd, plan-approval, pretty JSON
```

### Related files

`src/main.rs` routes `src/checkpoint_workspace.rs`; `src/cli_operands.rs` preserves
the narrow lexical contract. `src/workspace.rs`, `src/checkpoints.rs` and
`src/values/` supply ordered preflight, pure byte inspection and finite encoding.
See [CHECKPOINT_CONTRACT.md](CHECKPOINT_CONTRACT.md) for decision scope semantics.

## Native next checkpoint guidance

### Overview

`next` is a thin read-only composition of workspace, log and checkpoint readers.
It reports a recommendation, never grants approval, CAS or execution authority,
restores a decision from history, or supplies a coherent/latest workspace snapshot.

### API / Interfaces

`next [WORKSPACE]` accepts one explicit optional workspace (cwd by default), not
inspect's marker-based first-operand selection. It resolves the selected root
without ancestor search. `checkpoint_guidance::next_arguments(&Path)` returns
`TaskOutput` containing complete literal text including terminal LF. There is no
`--json` interface. Main writes those bytes without adding another LF and flushes.

### Data Flow

1. Run full ordered `workspace::load` preflight, with the same distinct ledger
   observations, configuration/runtime-control checks and limitations as inspect.
   This is not the task-list pending/runtime fence or execution-history assessment.
2. Read the **whole messages log**, then validate all its explicit checkpoint
   references, before reading the **whole events log** and its references. Every
   present `checkpoint` field must be a valid string ID with a safe managed path
   and UTF-8 text if present—even on non-request records. Whole-batch parsing
   precedes reference checks; corrupt tails never yield valid-prefix guidance.
3. Requests have a truthy checkpoint and the case-sensitive substring `request`
   in Python `str(type)`, including container values and object key strings. Retain
   duplicates and stable chronological ordering; message ties precede event ties.
   Timestamp keys match Python 3.14 `datetime.fromisoformat(str(ts))`: naive means
   UTC, aware normalizes to UTC, invalid or UTC overflow sorts at datetime.min UTC.
   Calendar/basic/week dates, Unicode separators and permissive fractional/offset
   forms follow that compatibility profile, **not** execution authority's narrower
   timestamp grammar. Finite non-string values use compatible stringification.
4. Inspect every sorted request's effective checkpoint status and select the
   **last nonapproved** request. Only if none remains, scan sorted lowercase
   `*.md` names and select the **last** with exactly `approval_stale`,
   `approved_unbound` or `decision_invalid`. This is not generic pending-file or
   interactive-review discovery; missing/changes-requested checkpoints can matter
   through requests but are not fallback statuses.
5. Emit `Next action`, `Reason`, `Review file` and `Suggested` lines for a selected
   checkpoint, otherwise the status/dashboard suggestion. Omit workspace from the
   suggested command only when the resolved root equals canonical cwd.

### Edge Cases

- Root/path output is **literal**, including ANSI, bidi and other controls under
  the current literal-output policy, not a terminal sanitizer or shell-quoting API.
  Suggested `coven approve` is inert text. Native `approve`, `suggest` and interactive
  `review` are supported by the product and explicit development build paths,
  including real legacy transport notices. Guidance itself
  neither invokes those writers nor grants their authority.
- Existing UTF-8 path, finite-value and strict log caps remain: 8 MiB/log,
  65,536 physical LF lines, object records and terminal LF; checkpoint text 1 MiB,
  each checkpoint scan 4096 entries. Admit **all** output within 32 MiB before
  stdout; profile/refusal errors emit no success prefix. This is not an aggregate
  CPU/heap ceiling or proof against concurrent namespace replacement.
- No locks, fsync, repair, decisions, dashboard/events writes, runtime/network
  calls or fallback. Linux-only validation and static/cooperating namespace scope
  remain; no power-loss/exhaustive proof or change to init's ancestry-sync limit.

### Example usage

```sh
target/debug/coven next /absolute/path/to/workspace
# From an existing workspace (not ancestor discovery):
target/debug/coven next
```

Inspect the named file and current status before deciding what to do; copying the
suggestion does not authorize a native mutation or notification.

### Related files

`src/checkpoint_guidance.rs` composes `src/workspace.rs`, `src/checkpoints.rs` and
the shared read-only `task_delivery::checkpoint_log_records` helper in
`src/task_delivery.rs` (also used by refresh validation). `src/logs.rs` owns strict
whole-log admission; `src/checkpoint_guidance/timestamp.rs` owns sort keys.
`src/main.rs` and `src/cli_operands.rs` own routing, literal output and narrow
operand compatibility. These read compositions add no authority boundary bypass.

## Native task add/assign workflow

The native binary now supports a complete task loop in existing workspaces:

```sh
target/debug/coven task add WORKSPACE "Review parser" --owner operator --notes "Check edge cases" --json
target/debug/coven task list WORKSPACE --json
target/debug/coven task assign WORKSPACE TASK_ID other-agent --json
target/debug/coven task list WORKSPACE --json
```

Use the returned task ID (slug collisions append `-2`, `-3`, etc.) and declared
roster IDs, not display labels. Native init can create a fresh workspace; no
reinitialization is required for an already-created workspace. Human add/assign output is
the default; `--json` selects the established structured envelope. Marker-based
optional workspace selection, title-word joining, assignment's ignored extra
operands, last-wins repeated flags and long-option prefixes follow the characterized
Python CLI. Parser behavior is checked against Python 3.14.8, not every Python
release; use `--` for option-like literal operands. Human output escapes controls
and non-ASCII with JSON-style spellings; machine values and persisted bytes retain
their exact text. JSON carries warnings in its envelope; human warnings use stderr.

`--expect-revision` is optional CAS against the whole current task authority token
from `task list --json`. Snapshot CAS hashes exact source bytes, including whitespace;
ledger CAS uses the ledger revision rather than its independent head-frame digest.
Snapshot workspaces reject `--request-id` because they have no durable request
deduplication. Existing imported ledgers require it on every add/assign:

```sh
target/debug/coven task add WORKSPACE "Review parser" --owner operator --request-id review-parser-1 --json
```

Retries must reuse the original raw title/notes/selectors, optional revision token,
and request ID. Do not replace an original pin with the current revision on retry:
that changes the binding. A replay can return the old task/owner/revision while
delivery reports the captured current head and projection health. It is not fresh
assignment, worker acceptance, task completion, or runtime execution permission.

### Authority and commit order

Workspace loading is now a neutral internal adapter, mechanically extracted from
the checkpoint wrapper. It retains ordered path/config/runtime-control validation,
all three ledger observations, native TOML limits and existing checkpoint behavior.
Loaded state is data, not a cached mutation permit. New task writes accept valid
runtime-launch control, refuse invalid control/orphan authority and never inspect
bundle children or execute frozen plans. **Native task listing keeps its conservative
runtime/actions and pending-import fences; state inspection now validates launch
control without executing it and can diagnose pending imports.** Consequently write
support in a valid runtime-controlled workspace does not imply native list/runtime
parity there; normal snapshot and active-ledger loops above are supported.

Snapshot mutations preflight a disposable draft, CAS and both event/message logs
before lock creation. They reload and reselect authority under the shared guard,
discard the unlocked draft, recheck CAS/logs, create the fresh effect and confirm
the target's execution reservation before replacement. A queued snapshot writer
cannot overwrite a projection after import wins the lock. Assigning to the same
owner is still a fresh mutation and is not exempt from reservation blocking.

Ledger mutation first checks raw receipt identity/conflict, then selected-history
confirmation. A hit returns before projection admission, fresh CAS, current owner
resolution or execution checks, with no projection/event/dashboard backfill.
For a miss, the service checks projection **before CAS**; the low-level fresh writer
independently reloads revision/head, rechecks receipt absence, checks CAS **before
projection**, then confirms the target's execution guard. This intentional ordering
matches the existing two-layer protocol.

The complete canonical frame is parsed and applied before immutable publication.
This preserves canonical key order in newly replayed tasks/projections while
untouched baseline records retain their insertion order. Raw history bytes, frame
count, retained receipts and directory capacities are admitted before linking.
Events use the original draft's nested task order, not a substituted canonical
receipt object. Snapshot records retain storage order, unknown extensions, missing
versus null fields and numeric identity. New task/event metadata uses tested UTC
ISO `+00:00` timestamps with microseconds omitted only at zero; add makes separate
created/updated calls. Snapshot/event UUIDs are hyphenated; ledger mutation IDs use
32 lowercase hex digits.

### Outcomes, secondaries and bounded output

Confirmed snapshot/ledger commits attempt secondary delivery independently:
projection first for ledger, then event, then dashboard. Failure does not roll back
authority, suppress later attempts, or change a committed exit to failure. Native
warning prose uses bounded static codes in the existing fields, not Python's OS
exception text. Snapshot logs are validated before commit; malformed observational
log contents do not gate ledger authority or receipt replay.

Known snapshot replacement uncertainty returns its task/revisions/mutation ID,
exit `2`, and no secondaries. A known receipt with uncertain confirmation returns
uncertain replay delivery; an uncertain confirmation with no receipt refuses a
fresh effect. A newly uncertain ledger frame follows Python's **fresh authority
reread and matching lookup** policy. Failed reread/missing/conflicting receipt stays
a hard error, rather than substituting execution's cached-proposal policy. No
secondary delivery runs on replay or authority uncertainty. Exit `1`, broken output
or interrupted transport is therefore not universal proof of no effect; inspect
state or reuse the exact durable request key instead of blindly changing keys.

Exit `0` means confirmed commit/replay, including secondary warnings. Structured
known authority uncertainty exits `2`; parser errors also exit `2` but have no
mutation JSON on stdout. Ordinary validation/CAS/busy/guard failures exit `1`.
Fresh proposals admit actual snapshot storage (8 MiB plus legacy-reader roundtrip)
or complete frame (8 MiB plus real Authority parse/apply), and both the result and
next full task-list compact/ASCII response before publication. An 8 KiB reserve
is proved sufficient for five warnings, 128-byte safe codes and bounded projection
metadata. Late projection-health I/O failure becomes bounded unreadable diagnostics
and preserves a confirmed result. Capacity refusal happens before the fresh
authoritative effect; stdio transport failure remains inherently observable later.

Events are compact literal-UTF-8 JSONL objects with correct backend-specific
metadata, finite last-wins parsing, no truncated final line, and no dedup/tail
repair. Native limits are 8 MiB total/file, 65536 physical LF lines including blanks,
and existing per-record value bounds. The complete new line and projected growth
are checked before append. Under the guard, event delivery creates an absent safe
`logs/` using Python's default mkdir policy, then revalidates directory/file paths;
snapshot preflight itself remains noncreating. Appends use private-mode, regular,
nofollow/nonblocking descriptors and file fsync. Partial append is left as evidence
with a warning; no truncation or rollback is attempted.

Dashboard capture/render/publication remain under the same guard, read authoritative
tasks rather than repairing a projection, and preserve Python Markdown bytes—raw
task titles/statuses intentionally remain unescaped. Only display labels receive
Python printability/HTML/Markdown escaping, using a provenance-tagged frozen Unicode
16.0.0 property table, not runtime Python. Goal text uses universal newline handling;
checkpoint rows use effective status. Render failure preserves the old dashboard.
Goal/checkpoint reads are capped at 1 MiB; dashboard output at 32 MiB. Projection or
dashboard sync uncertainty after a confirmed task is a secondary warning, not new
task-authority uncertainty.

No automatic import/repair, broader lifecycle commands, runtime/provider
call, installed entrypoint change or Python fallback is included. Explicit native
state import/recovery and refresh are described below. Input/path/TOML/log/output
and Linux-only execution qualifications remain explicit preview limits, not claims
of complete application parity.

## Native state lifecycle and existing-workspace refresh

### API / Interfaces

`state inspect [WORKSPACE]` is read-only; `--json` selects compact JSON instead of
the default pretty JSON. Both modes safely escape presentation text. It validates
workspace paths/configuration and launch control, replays selected task authority,
and reports projection health without repairing it. Pending imports retain
`import_pending: true`; malformed source yields `source_error`, not invented tasks.
It does not parse current agent/lead contents as task authority or acquire a lock.

`state import-tasks [WORKSPACE] --expect-revision SNAPSHOT_TOKEN` requires an exact
snapshot token. `state recover-tasks [WORKSPACE]` requires exactly one mode:

- `--repair-projection`: explicit repair from active ledger authority.
- `--resume-import --expect-revision SNAPSHOT_TOKEN`: activate only a complete,
  unambiguous pending baseline against the original snapshot pin.
- `--abort-import`: preserve a pending import by renaming its directory whole.

The revision option is rejected for repair/abort. These state commands also accept
`--json`; default output is pretty JSON, not a human summary. Omitted workspace
means cwd. `refresh [WORKSPACE]` likewise defaults to cwd, but has no `--json` flag.
It regenerates only `tmux/coven.toml`, starter prompts, missing role prompts and
`dashboard.md` in an existing workspace. New workspace init is described separately below.

### Data Flow

State writers preflight before lock creation, then repeat admission under the
shared workspace guard. Import/resume call the confirmed execution authority-change
gate and publish the activation marker last. Abort preserves the pending directory
without parsing its source or consulting execution reservations. Confirmed activation
or repair independently attempts projection/dashboard delivery, without an event;
primary uncertainty skips secondary effects.

Projection repair synchronizes fully validated selected authority **before reading
the projection**. A byte-current projection needs no backup. Otherwise it captures
raw bytes and absence distinctly, creates an exclusive new child under
`state/task-recovery/`, and durably publishes `tasks.bytes` when present plus
`metadata.json`. Corrupt JSON, non-UTF-8 and empty files are preserved verbatim;
missing input is recorded as missing, not converted into an empty backup. Before
atomic replacement it replays authority again, compares revision **and** head digest,
and rechecks exact raw projection bytes/presence. Unsafe source paths are refused,
not followed. Partial/final backups are retained and reported at workspace-specific
paths; they are never silently adopted or deleted.

Refresh prepares every output and validates inputs before creating the first lock,
discards that preparation, then freshly prepares under the guard. Both logs are
fully parsed and every explicit `checkpoint` field is validated, even on records
that are not checkpoint requests; history never restores a decision. Publication
order is manifest → exclusively created missing role prompts → starters → atomic
dashboard last. Existing role prompts, authored metadata/config/prose, task/agent/
lead state, logs, checkpoint bytes, roster notes and work/output artifacts remain
unchanged (apart from possible first creation of the persistent coordination lock).
Refresh never activates/repairs authority or executes the generated commands.

### Edge Cases

- Recovery's **raw backup cap is 32 MiB**, independent of JSON/baseline limits;
  larger source projections refuse with `unsupported_recovery_size` before creating
  recovery artifacts. This is not a JSON validity requirement.
- New private SDK ledger/recovery directories request `0700`, filtered by inherited
  umask and retaining inherited SGID. Existing directories are not chmodded. New
  authority and projection-recovery backup files receive verified `0600`; import
  raw-source backups instead preserve source mode bits where applicable. Atomic
  replacement preserves existing target mode bits, or uses `0600` for a missing
  target. Ownership, ACLs and xattrs are not promised preserved.
- Known primary results, preserved paths and durability classification survive late
  health/report/render failures through compact known-metadata output. Primary
  synchronization uncertainty exits **2**; ordinary errors exit **1**, including
  failures leaving pending/backup artifacts. Confirmed operations with secondary
  projection/dashboard warnings still exit **0**: warnings do not mean rollback.
  Parser errors also exit 2; failed stdout/stderr delivery can exit 1 after effects.
- Refresh is **not a multi-file transaction**. Manifest/starters are in-place writes;
  I/O failures can leave partial outputs and report `refresh_outputs_may_be_partial`
  with exit 1. Even dashboard directory-sync uncertainty is a refresh error, not a
  task-authority uncertainty envelope.
- Hardlinks on generated files selected for overwrite are refused in preflight and
  on the opened descriptor **before truncation** (`unsafe_generated_hardlink`). This intentionally
  tightens Python behavior. Existing role prompts are preserved; atomic dashboard
  replacement does not truncate a linked inode.
- Ordinary-path template bytes match Python. For roots containing apostrophes,
  percent signs or backslashes, generated root labels use literal `printf` data
  arguments, never untrusted format/escape interpretation. The default generated
  `COVEN_COMMAND` is canonical `coven`; an explicit nonempty environment override
  is shell-quoted. There is no Python fallback or executable lookup during refresh.
- Refresh supports blank, embedded `interactive-demo` and custom example context
  through [AssetBundle root selection](examples/README.md). It validates the custom
  manifest/context only, not unrelated seed files. Absent example metadata ignores
  unused invalid roots; empty metadata still refuses.
  Each generated prompt/manifest is capped at **1 MiB**; non-dashboard outputs have
  a separate aggregate **32 MiB / 4096-output** budget. Dashboard output has its own
  **32 MiB** cap. Existing workspace/JSON/log/checkpoint limits still apply.
- Each refresh preparation also caps aggregate checkpoint-reference work at
  **256 MiB of validated raw UTF-8 bytes**, shared across messages then events.
  Repeated references, including non-request records, are charged on every reread;
  CRLF is not normalized for accounting. Exact-limit input is admitted and overage
  returns `unsupported_refresh_checkpoint_work_profile`. The disposable and locked
  preparations each start a fresh budget. A deterministic refusal creates no first
  lock or generated outputs. Other checkpoint-history consumers retain their
  existing profiles; this is not caching, replay or decision authority.

These protocols assume a static/cooperating namespace and local filesystem, not
hostile-swap immunity, a sandbox or universal power-loss durability. Validation is
Linux-only; macOS execution has not been established. The original state/refresh
slice did not change installation; current native-only entrypoint/payload policy
is documented in the overview.

### Example usage

Use a previously created disposable workspace and actual inspected tokens:

```sh
target/debug/coven state inspect WORKSPACE
target/debug/coven task list WORKSPACE --json
target/debug/coven state import-tasks WORKSPACE --expect-revision '<snapshot token>' --json
target/debug/coven state inspect WORKSPACE --json
target/debug/coven refresh WORKSPACE
# Only when explicitly needed after inspecting projection health:
target/debug/coven state recover-tasks WORKSPACE --repair-projection --json
```

For interrupted unactivated imports, choose resume with the original snapshot pin
or abort, not both. Neither repairs incomplete pending artifacts automatically.

### Related files

CLI routing is in `src/main.rs`; coordination in `src/state_commands.rs` and
`src/refresh.rs`; pure assets in `src/workspace_assets.rs` and its embedded files.
`src/workspace.rs`, `src/task_delivery.rs`, `src/ledger/{activation,repair}.rs` and
`src/platform/unix.rs` supply validation, delivery, backup and descriptor policies.
`tests/state_refresh.rs` and `tests/_state_refresh_parity.py` cover the CLI loop.
See also [TASK_LEDGER_CONTRACT.md](TASK_LEDGER_CONTRACT.md) and
[STORAGE_CONTRACT.md](STORAGE_CONTRACT.md) for retained authority/storage contracts.

## Native workspace init

### Overview

Native init is implemented end to end, not SDK-only: it creates the complete seeded
`interactive-demo`, custom-example or blank snapshot workspace without Python, runtime calls or
executing generated commands. It does not activate ledger/execution authority.
The product CLI is native. The original init slice did not itself change installation;
the current native-only launcher and payload policy are described above.

### API / Interfaces

```text
coven init [WORKSPACE] [--name NAME] [--agents CSV]
  [--opencode-command COMMAND] [--multiplexer {herdr,tmux}]
  [--example EXAMPLE] [--blank] [--force]
```

Use the explicit native build path. WORKSPACE defaults to cwd; defaults are roster
`Clippy,Srikanth,Jamal,Kris,Eric`, command `oc`, multiplexer `herdr`, example
`interactive-demo`, and name from the resolved root basename (also for empty
`--name`). Unambiguous long-option prefixes and repeated options are accepted;
the last value wins. `--force` is a compatibility no-op, never reset permission.
Init uses the shared Python 3.14 negative-digit-prefix compatibility adapter, so
values such as `-1team`, `-.5suffix` and Unicode decimal-digit prefixes retain
their literal spelling. Unknown flags still refuse; attached values and `--`
retain their normal meaning, and invalid multiplexer choices cannot be hidden by
a later repeated valid choice.
`--blank` overrides example selection, including invalid selections and unused
invalid asset roots. Otherwise selectors slug-normalize to an example directory;
empty/unknown selections refuse before effects. The demo remains embedded by
default; explicit roots also override it. See [examples](examples/README.md) for
absolute authoritative `COVEN_EXAMPLES_DIR`, compatibility `COVEN_HOME/examples`,
installed prefix roots and Cargo-target-only development fallback, plus defaults,
newline/coercion rules and bounded no-follow leaf checks.

`init::arguments(&Path, &init::Options)` returns the shared `TaskOutput` delivery
shape. Bootstrap reservation is private; ordinary `WorkspaceGuard` still requires
a regular workspace marker. No reader/guard marker-admission rule is weakened.

### Data Flow

1. Resolve the root without effects: existing aliases, missing components and `..`
   are processed in filesystem order (symlinks before a following `..`), not by
   lexically collapsing the path. Existing tilde/Unicode-path restrictions apply.
2. Refuse all 13 reserved top-level names, of any type, including empty directories
   and dangling symlinks, before other semantic option checks: `coven.json`,
   `coven.toml`, `goal.md`, `spec.md`, `dashboard.md`, `agents`, `prompts`, `state`,
   `logs`, `checkpoints`, `work`, `output`, `tmux`. Unrelated files are preserved.
   CLI syntax errors still precede service preflight.
3. Prepare and validate the entire tree, generated TOML, state, initial events,
   dashboard and safely escaped informational output before any creation. All seven
   original `examples/interactive-demo/` assets remain embedded; custom assets are
   loaded read-only through the selected root, with no Python fallback. Refresh
   uses embedded context for the implicit demo, selected context otherwise.
   Missing custom-roster owners become unassigned/pending.
4. Create missing root directories; exclusively reserve `state/`, create and hold
   its persistent private flock inode, recheck reserved names and identity, then
   exclusively create managed directories and final files. Sync files, managed
   directories and the operation's creation chain as qualified below.
5. Write/sync an owned config temp, exclusively hard-link complete `coven.json`
   **last**, then sync root. This deliberately differs from Python's config-first
   creation for safety; failure-artifact parity is not promised. Cleanup attempts
   only the owned config temp; no overwrite, reset, rollback, adoption, automatic
   resume or stale reservation stealing occurs. Partial state/lock/finals remain.

Initial JSON uses Python-compatible indent-2 ASCII plus LF; events are compact
literal-Unicode JSONL (`workspace.created`, plus seeded goal/task events). Blank
mode has no `spec.md` or seeded tasks. The lock is coordination, not new authority.
Canonical native `COVEN_COMMAND` policy is unchanged: default `coven`, explicit
nonempty environment override shell-quoted, no executable lookup. Unsafe root
labels remain literal `printf` data arguments, not format strings.

### Edge Cases

- Normal files request `0666` and directories `0777`, subject to umask and OS
  inheritance; the persistent lock requests `0600` under umask. Dashboard mode is
  applied and verified as `0600`. No executable files are generated.
- Admission caps: **32 MiB aggregate stage, 4096 files, 1 MiB text/TOML/dashboard,
  8 MiB JSON and event log**, plus existing finite-value/output/input caps. Normalized
  ASCII agent IDs are at most **245 bytes**, keeping `<id>.start.txt` within 255
  bytes. Duplicate normalized IDs refuse. Deterministic refusals create no root.
- Success exits **0**. Pre-marker publication failure exits **1**, keeps partial
  evidence and emits bounded root, phase, current target, last-completed target and
  partial-file diagnostics. Known marker-link success followed by root-sync failure
  exits **2** with informational stdout retained (`init_marker_durability_uncertain`),
  not a false “nothing happened” failure. Parser errors also exit 2; stdout I/O
  failure can exit 1 after effects. Inspect before retrying; `--force` cannot repair.
- Native/native creation has one exclusive reservation winner. Python's initial
  creation is **unlocked**: mixed initializers can both fail and leave mixed partial
  evidence; exclusive finals prevent overwrite, not shared bootstrap serialization.
  Initialize a destination with one implementation at a time.
- **Operation-scoped durability, not cross-initializer ancestry durability:** sync
  covers files, managed directories, root, and the chain **observed missing by this
  invocation**, through its containing existing parent. Already-existing ancestor
  entries are assumed durably established. A can create a new root chain and pause
  before reservation; B sees an existing root, wins reservation and reports Committed
  after syncing root (and managed contents), but not the outer entries A created.
  A then loses before its chain sync, so neither necessarily fsyncs those outer
  entries. Native exclusion does not close this exposure. Callers needing stronger
  assurance must durably establish destination ancestry first or initialize with
  one process. A small reorder does not eliminate the race without a new protocol
  or unconditional ancestor sync, which would regress the execute-only-parent case.
  A deterministic unit pins this exact scope; no actual crash-loss test is claimed.
- Static/cooperating local namespace assumptions remain: no hostile namespace
  guarantee, multi-file transaction or universal power-loss guarantee. Validation
  is Linux-only, not established macOS execution or Windows support.

### Example usage

Choose fresh disposable destinations; these commands do not run a demo or runtime:

```sh
target/debug/coven init /absolute/path/to/new-demo --example interactive-demo
target/debug/coven init /absolute/path/to/new-blank --blank --name 'Review team' \
  --agents 'Architect,Reviewer' --opencode-command opencode --multiplexer tmux
target/debug/coven task list /absolute/path/to/new-demo --json
target/debug/coven state inspect /absolute/path/to/new-demo --json
target/debug/coven refresh /absolute/path/to/new-demo
```

### Related files

`src/main.rs` routes `src/init.rs`; `src/init/tests.rs` covers staging, partial
publication and operation-scoped ancestry sync. `src/storage/bootstrap.rs` and its
tests own reservation/marker delivery; `src/platform/unix/init.rs` and its tests own
path resolution and descriptor identity. `src/workspace_assets{,/init}.rs` embed
the original example files; `src/task_delivery.rs` formats the in-memory initial
dashboard without weakening ordinary capture. `tests/init.rs` and
`tests/_init_parity.py` exercise CLI parity and explicit native differences. See
[examples/README.md](examples/README.md) for the bounded example profile.

## Native local workspace commands

### Overview

Native `goal`, `message`, `messages`, `events` and `status` are local file operations,
not notifications, runtime admission or execution. Python production code,
`bin/coven`, dependencies, runtime/fallback behavior, installation and
default-entrypoint integration are unchanged.

**Terminal warning:** all five families preserve Python's literal Unicode/control
text, not terminal-sanitized text. ANSI, bidi and other controls can render in pretty
history, goal and status output as well as `--raw`. Python `repr` escapes inside
container values are compatibility formatting, not a terminal safety boundary.
Existing task human/report escaping and terminal-LF behavior remain unchanged.

### API / Interfaces

```text
coven goal [ITEM ...]
coven message ITEM [ITEM ...]
coven messages [WORKSPACE] [--pretty] [--raw]
coven events [WORKSPACE] [--pretty] [--raw]
coven status [WORKSPACE]
```

Use the explicit native build path. Goal accepts zero or more items; message accepts
one or more at parser level. Both select the first item as a workspace only when its
expanded path has a `coven.json` marker; otherwise they use cwd and retain all items.
Message requires at least three remaining items (sender, recipient, body) **before**
full workspace loading. Parties are free-form, not roster selectors. Body words join
with single spaces without stripping. Goal words join with single spaces and Python
stripping; empty/whitespace-only text means **show**, never clear.

History/status workspace defaults to cwd. Pretty history is the default; `--pretty`
is accepted, and `--raw` wins when both are supplied. History accepts unambiguous long
flag prefixes and repeats. These families add no `--json` interface.

### Data Flow

- All routes use general ordered workspace preflight: managed paths, configuration,
  checkpoint text, runtime-launch control, pending snapshot or selected active task
  authority, and agent/lead records. They do not inherit task-list's runtime/actions
  or pending-import fences, check execution reservations, or execute frozen plans.
  Preflight is not an atomic workspace snapshot or transferable mutation permit.
- Goal show reads the **whole** UTF-8 goal, converts CRLF/bare CR to LF and adds one
  extra LF, even if the file already ends in LF. Goal set atomically replaces exact
  `# Goal\n\n{text}\n` bytes under the guard. Known rename plus sync failure
  (`Delivery::DurabilityUncertain`) exits **1**, skips both secondaries and does not
  roll back—not task/state's exit-2 policy. Confirmed replacement independently
  attempts `goal.updated` and dashboard publication under that guard; either warning
  still exits **0**.
- Message encodes ordered `ts`, `from`, `to`, `body` fields, then validates events
  and messages before lock creation. It reloads workspace state and revalidates
  events under the guard; guarded append revalidates the message log and capacity.
  Message append and `message.sent` share that guard. It releases the guard **before**
  a separate dashboard preflight/lock/publication epoch. Append, event or dashboard
  failure exits **1** with no false success banner, even if message/event bytes are
  already visible. Partial evidence stays; no receipt, deduplication, automatic retry,
  truncation or rollback is offered.
- Status publishes only `dashboard.md` under the lock (apart from possible first
  creation of the persistent lock), then releases it and rereads text for output.
  It does not parse log contents: corrupt JSONL content alone does not block status,
  while unsafe managed log paths still do. Publication uncertainty or failed later
  output reread exits **1**; another writer may change the dashboard before reread.
- Pretty history validates the entire selected log and admits the complete output
  before printing any records. Finite legacy fields use Python truthiness and
  `str`/container `repr`, not an all-string schema or substituted JSON rendering.
  Raw history bypasses JSON, depth and LF-record validation, reading UTF-8 text with
  universal newlines instead. Missing/empty cases retain Python output differences.
  Goal show and history reads create/acquire no lock, fsync nothing and mutate nothing.

`main.rs` writes the new families' complete literal bytes exactly, without an added
terminal LF, and explicitly flushes stdout. A buffered broken pipe can therefore
produce nonzero exit **after** visible file effects. Exit 1 is not proof of no effect;
inspect files before deciding whether to repeat a message.

### Edge Cases

| Profile | Native bounds |
| --- | --- |
| Strict pretty history / append | 8 MiB per log, 65,536 physical LF-terminated lines including blanks; finite legacy value caps |
| Raw history | Separate 32 MiB UTF-8 input and 32 MiB complete-output bounds; no JSON/depth/LF-record profile |
| Goal | 1 MiB stored/read text including generated header/LF; joined input admission up to 32 MiB before stripping; complete output 32 MiB |
| Message parties | Each free-form string checked against 1 MiB before copying; body/record also obey finite string/encoding caps |

Strict logs require object records and a terminal LF, reject malformed or truncated
tails, and never silently repair. Existing shared JSON limits (depth, nodes,
containers, strings, finite numbers) still apply; raw bypasses only log parsing,
not general workspace preflight. Goal's stripped set text must fit the 1 MiB
stored-file cap after its nine header/LF bytes. These are preview acceptance limits,
not claims that larger Python inputs are corrupt.

The private JSONL implementation was mechanically extracted to `src/logs.rs`;
the frozen Unicode 16 table moved unchanged to `src/unicode.rs` for dashboard labels
and Python repr. Old task/refresh policies and protocol IDs are unchanged. Linux-only
validation and static/cooperating local namespaces remain the scope; no universal
power-loss proof is claimed. Native init's accepted operation-scoped ancestry
durability limitation remains unchanged.

### Example usage

From the repository root, select a fresh disposable destination:

```sh
cargo build --offline --locked --bin coven
target/debug/coven init /absolute/path/to/new-local --blank --agents operator,reviewer
target/debug/coven goal /absolute/path/to/new-local "Review parser edge cases"
target/debug/coven goal /absolute/path/to/new-local
target/debug/coven message /absolute/path/to/new-local operator reviewer "Please review the parser"
target/debug/coven events /absolute/path/to/new-local --pretty
target/debug/coven messages /absolute/path/to/new-local --pretty
target/debug/coven messages /absolute/path/to/new-local --raw
target/debug/coven status /absolute/path/to/new-local
```

This complete init → goal → message → history → status loop neither launches nor
notifies agents. Inspect after a failure instead of blindly retrying an append.

### Related files

`src/main.rs` routes `src/workspace_commands.rs` and
`src/workspace_commands/{history,python}.rs`. `src/workspace.rs` supplies ordered
preflight; `src/logs.rs`, `src/task_delivery.rs`, `src/storage/replacement.rs` and
`src/task_time.rs` supply guarded delivery and metadata. `src/unicode.rs` is the
unchanged shared printability table. Existing task/state/refresh outcome policies
remain separate from this family's delivery policy.

## Native reviewed runtime lifecycle

### Overview

Twelve new represented routes complete the native reviewed cold-launch batch:
five action writers, three checkpoint decisions and four legacy transport paths
(including the `stop` alias). Checkpoint and action workflow gates are accepted
on Linux within the native profile. At that milestone legacy workspace/runtime/agent
covered **4/16** previously missing routes; the subsequent completion below closes
the other twelve. This is actual effectful cold
startup, not a no-effects framework, task dispatch or worker-readiness acceptance.
No dependencies were added in this batch; production Python and `bin/coven` are
unchanged. Nothing here authorizes staging, commits, installation or cutover.

### API / Interfaces

```text
action enable-runtime-launch [WORKSPACE] [--json]
action prepare-runtime-launch [WORKSPACE] [--json]
action submit [WORKSPACE] ACTION_ID --expect-manifest TOKEN
  --expect-checkpoint APPROVED_SNAPSHOT --expect-decision DECISION_ID [--json]
action acknowledge-unknown [WORKSPACE] ACTION_ID --expect-claim CLAIM_ID [--json]
action stop [WORKSPACE] ACTION_ID [--json]
approve [WORKSPACE] [CHECKPOINT] [--agent AGENT] [--message TEXT]
  [--expect-revision SNAPSHOT] [--json]
suggest [WORKSPACE] CHECKPOINT TEXT... [--agent AGENT]
  [--expect-revision SNAPSHOT] [--json]
review [WORKSPACE] [CHECKPOINT] [--agent AGENT] [--message TEXT]
send [WORKSPACE] AGENT TEXT...
agent send [WORKSPACE] AGENT TEXT...
down [WORKSPACE]
stop [WORKSPACE]  # alias of down
```

Action selectors use arity, not workspace-marker inference. Legacy/checkpoint
operand selection retains its narrow marker-based compatibility rules. Action
writers return pretty JSON by default, compact with `--json`; action inspection
now defaults to pretty literal-Unicode JSON (its compact `--json` remains ASCII).
Approve/suggest have human/default or ASCII
JSON reports; review is interactive and binds the displayed snapshot. Main appends
one terminal LF. Checkpoint committed-with-warning remains exit 0; uncertain
replacement exits 2 and skips notices/secondary work. Action claimed/unknown/
durability-uncertain outcomes exit 2, failed/partial transport exits 1; completion
describes CLI transport, not worker health or task success.

### Data Flow

1. **Activation/admission:** persistent shared legacy transport admission precedes
   workspace exclusion. Exclusive activation drains admitted cooperating sends
   before enabling reviewed mode; no lock upgrade or automatic reset. Legacy paths
   recheck enabled mode after admission. Workspace exclusion is released before
   native lookup/text/Enter/down or frozen-plan I/O; shared admission may remain held.
2. **Prepare/review:** publish an immutable captured inventory/plan bundle before
   its dedicated checkpoint. Decisions preserve exact checkpoint scope, CAS the raw
   snapshot under lock, and perform secondary history/dashboard plus freshness-checked
   notices after publication. Positive legacy notification uses actual text + Enter;
   enabled mode does not silently bypass transport gates. Interactive review never
   refreshes/retries a stale displayed revision automatically.
3. **Submit:** check exact manifest, approved checkpoint snapshot/decision/scope,
   captured current inputs and control binding before publishing one durable claim.
   Only a fresh confirmed consumption creates the private execution permit. Release
   the workspace lock, then execute only the frozen plan, including five-second
   per-agent delays. Drift/revocation before claim refuses; after claim it cannot
   change or cancel that attempt. Acknowledged/consumed retries never execute again.
4. **Outcome/recovery:** retain observed completed/failed/partial or unknown outcomes.
   Uncertain claim publication never executes; death after claim leaves it consumed.
   Explicit acknowledgement accepts uncertainty without success inference or reset.
   Stop uses recorded frozen scope/backend, not current mutable configuration, and
   never grants replay or removes consumption. There is no automatic rollback.

### Edge Cases

- **Separate process ownership:** observation probes retain <=30-second configurable
  budgets and same-binary supervised process-group cleanup. Runtime CLI commands
  instead use **60 seconds and 1 MiB combined output**, own/reap only the direct
  child and intentionally leave native daemon/server effects alive. Failure is not
  proof of no effects. Inherited nonstandard child descriptors are closed/CLOEXEC,
  including lock descriptors; low/high-FD sentinel tests cover this boundary.
- **Environment and selection:** frozen launch/stop use the captured six-key
  environment (`PATH`, `HOME`, `SHELL`, `LANG`, `LC_ALL`, `TERM`) and frozen cwd.
  Legacy sends/down/notices preserve all caller environment and caller cwd, not
  workspace cwd. Relative/empty PATH components resolve against the applicable cwd;
  absent PATH uses `/bin:/usr/bin`. Symlink/`..` traversal remains in filesystem
  order. Ordered candidates advance **only** on proven pre-image `ExecFailed`
  (`Command::spawn` error), never timeout/output limit/generic `SpawnError`/executed
  nonzero 127. Lookup and fallback attempts share one command deadline.
- **Writer-only admission:** `actions/mutation/capacity.rs` admits at most 128 claims,
  128 KiB canonical plan, 4 MiB bundle/projected control, with 128 KiB and 2048 nodes
  reserved per pending report. Captured inputs are at most 2 MiB aggregate/768 KiB
  per file; checkpoints retain 1 MiB. Known retries, historical inspection,
  acknowledgement and stop are not retroactively tightened by fresh-writer limits.
  These finite profiles still require explicit compatibility disposition at retirement.
- **Limits of evidence:** trusted/cooperating paths/tools, default SIGCHLD/no competing
  reaper and static descriptor limits are assumed. Synchronous spawn/FD-scan/filesystem
  setup and uninterruptible kernel waits are not universally hard-bounded. Unit fault/
  uncertainty/crash seams are not universal power-loss proof. No actual provider,
  installed runtime or tmux session was used for acceptance; unrestricted startup
  remains possible when an operator deliberately submits against real tools.

### Example usage

See the README's [reviewed startup pseudocode](README.md#optional-reviewed-runtime-startup).
All IDs, manifest/snapshot/decision/claim tokens require manual inspection and
substitution. Review does not automatically submit; checkpoint approval, execution
preparation and observations cannot substitute for fresh durable action admission.

### Related files

`src/actions/mutation{.rs,/submission.rs,/inputs.rs,/capacity.rs}` owns capture and
single-use claims; `src/launch{.rs,/executable.rs}` executes frozen plans.
`src/runtime{.rs,/transport.rs}`, `src/runtime_command.rs`,
`src/platform/unix/runtime_command.rs` and `src/transport_gate.rs` own effectful
transport, FD hygiene and admission. `src/checkpoint_decisions/` and
`src/checkpoints/formatter.rs` own decisions/review/notice semantics.
`tests/runtime_launch_workflow.rs`, `tests/_runtime_launch_parity.py` and
`tests/support/runtime_backend_fixture.rs` provide compiled inert-backend acceptance;
module-local tests retain fault, race and oracle characterizations. See
[RUNTIME_LAUNCH_CONTRACT.md](RUNTIME_LAUNCH_CONTRACT.md) for the existing contract.

## Native repository, readiness and persisted Djinn observation

### Overview

Four complete CLI routes now represent repository inspect/register, worker readiness
and persisted `observe djinn`. Verification is a mode of inspect, not a fifth route.
They use the real production native supervisor, not an SDK-only transport or Python
helper. Repository/readiness **Linux implementation workflow gates are accepted**
after targeted acceptance and the full gate; profile and macOS gates remain open.
Observe is only **1/3** complete. Djinn-live and Kitsune are
not ported. None grants execution, job, policy or worker authority.

### API / Interfaces

```text
repository inspect [WORKSPACE] [--verify] [--json]
repository register [WORKSPACE] --path CHECKOUT --base FULL_COMMIT
  --delivery-target refs/heads/NAME --expect-repository-revision TOKEN [--json]
worker readiness [WORKSPACE] [--adapter djinn] [--profile research-readonly]
  [--executable PATH] [--timeout SECONDS] [--max-output-bytes BYTES] [--json]
observe djinn --executable ABS_PATH --folder ABS_FOLDER
  [--timeout SECONDS] [--max-output-bytes BYTES] [--json]
```

Workspace defaults to cwd. Readiness supports only the shown adapter/profile,
selects explicit executable or caller PATH `djinn`, and needs an existing directory,
not a loaded workspace configuration. Its selector preserves filesystem resolution
order through symlink/`..` components (the P1 selector correction). Persisted Djinn
requires explicit absolute UTF-8 selectors; it has no PATH/default-folder discovery.
Both configurable probes default to 3 seconds and 131072 combined output bytes;
timeout must be finite, greater than zero and at most 30 seconds, output 1024..1048576.
These routes accept repeated flags with the last value winning and unambiguous long
flag abbreviations; validation still applies to the resulting selectors/bounds.

Repository and persisted Djinn default to two-space pretty ASCII-escaped JSON;
`--json` selects compact ASCII JSON. Repository's compact separators retain Python's
spaces. Readiness defaults to human text, while `--json` emits Unicode JSON. Main
adds exactly one terminal LF. Readiness exits 1 only for healthy advertised evidence
missing required session/run routes (`blocked`); failed/changed/unverified evidence
is `unknown`, exit 2. It never reports compatible. Persisted Djinn exits 0 observed,
1 unavailable, 2 incompatible. Repository exits 0 readable/confirmed, 1 unavailable
or prepublication error, 2 known-result durability uncertainty.

Private `ProbeRequest`, `ProbeRunner`, `ProbeResult` and `ProbeStatus` separate domain
allowlists from native transport. Shared `platform::FileMetadata` records device,
inode, size and nanosecond mtime. Fingerprints stream SHA-256 with a shared **1 GiB
per-hash cap**, checking path and descriptor metadata. Git is pinned too: alias and
metadata before each probe, final complete hash and alias recheck. This detects
ordinary changes, including content changes with restored mtime, not authentication,
race-free execution or a hostile-program sandbox.

### Data Flow

- **Repository:** read a duplicate-free, closed-schema record at
  `state/repository.json` (at most 65536 bytes). Revision is SHA-256 of
  `coven.repository-registration:v1\0` plus the **exact original record bytes**,
  not reserialized JSON. Default inspect is source-free: no Git, checkout, lock or
  fsync. Explicit `--verify` observes the original selector, retaining frozen record
  and revision on ordinary checkout/Git-pin failure, with unavailable observations.
- **Registration:** compare immutable normalized original request **before Git**.
  Exact retry uses the original expected token, never opens the checkout or runs
  Git, and confirms known bytes under the shared persistent workspace lock.
  Lock/confirmation failure preserves the known result with
  `delivery.outcome: durability_uncertain`, exit 2; confirmed uses the literal label
  `durably_committed`. Changed arguments conflict. Fresh registration requires
  `repository-unregistered:v1`, observes once before locking and again under lock,
  compares frozen facts, then publishes and confirms exact bytes. No source/task/
  runtime gates, events, dashboard, ref writes or index writes are added.
- **Git:** trusted caller-PATH Git >=2.48, clean environment and closed read-only
  options disable optional locks, hooks, fsmonitor, maintenance, replacements, lazy
  fetch and protocols. Each observation gets its own 20-second checked budget
  (fresh registration has **two**, not one shared budget), each Git probe at most
  3 seconds and 1 MiB combined output. Check split-index/config, promisor packs,
  alternates and linked-worktree common-directory storage **before status**.
  Existing linked worktrees are allowed subject to exclusions; worktree-specific
  config, includes, filters and unsupported extensions/storage are not. Independently
  hash raw base commit and root tree with Git framing for SHA-1/SHA-256 and validate
  root-tree structure; this is **not full transitive object closure** or complete
  checkout availability. HEAD/dirty observations are ephemeral, not frozen authority.
- **Readiness:** root help gates advertised session/ask/agents help; session help
  gates run/status/watch help; root **Options** gates `--version`. All eight surfaces
  are allowlisted; no bare execution or status/watch invocation follows help. All
  eleven semantic capabilities remain unknown. Missing routes are blocked only
  when all relevant observation is healthy; probe/parse/identity failure dominates
  as unknown. Native transcripts are withheld; reports retain bounded provenance.
- **Persisted Djinn:** pin one trusted executable, invoke only
  `session observe ABS_FOLDER --json` from `/` with filtered environment, rehash,
  then validate bounded producer schema and exact folder. There is no help fallback,
  live status, actual-instance validation, enrollment or readiness upgrade. Persisted
  IDs/timestamps and digests do not establish live identity, freshness or job success.

The transport self-execs the **same single `coven` binary** into private bootstrap
**before Clap**. An anonymous bounded request and private liveness socket gate the
bootstrap. It launches the target in its cooperating process group only after
arming parent-EOF monitoring and an independent watchdog at probe budget + 1 second.
Completion requires **both target leader status and stdout/stderr EOF**. Exact
combined output capacity is accepted; one additional byte yields output-limit.
Normal and nonzero completion also clean descendants that already closed pipes.
The caller kills the **unreaped supervisor PGID before reaping**, never signals a
reused PGID after reap, and rejects SIGCHLD-reaper/auto-reap contexts. Cleanup faults
become `SpawnError`, not successful observations; adapters do not leak transcripts.

### Edge Cases

- Budgets are not a universal setup-inclusive hard wall clock. Synchronous entropy,
  tempfile, FD enumeration, self-exec setup and filesystem/hash I/O are not
  interruptibly hard-bounded. Executable hashing is outside subprocess deadlines
  and Git's observation budget. Drain/scan/acceptance checks enforce their budgets
  when control returns; cleanup has a separate at-most-one-second reap allowance.
- Containment assumes static/cooperating paths and process groups. A trusted target
  can ignore arguments, access files/network or escape with `setsid`; this is not
  containment of malicious descendants. Filtered environment is not isolation.
- Each readiness surface admits at most **65536 unique commands and 65536 unique
  flags**. A 65537-declaration help payload can fit 1 MiB yet exceed the common
  `Value` array profile: it is a **failed observation / unknown / exit 2 in both
  renderers**, not silent JSON loss or human-only success. This explicit native
  restriction still requires explicit compatibility disposition for release approval.
- Git-pin ordinary-change failure makes live verification unavailable; source-free
  inspect and exact known retries remain independent. Registration is immutable,
  not a rebind API or a task/execution grant.
- Timestamp consumption quirks follow the Python 3.14 oracle. Two strings reported
  as trailing-garbage cases are actually accepted by that oracle; do not claim
  blanket strict trailing-junk rejection or silently tighten retained authority.
- Linux validation does not close the macOS gate. POSIX implementation is not
  evidence that all supported-platform acceptance is complete.

### Example usage

The README's [native bounded local observation](README.md#native-bounded-local-observation)
examples exercise actual commands with every mode/flag shown above. Paths and full
commit selectors there are **inert reference values**, not runnable fixture IDs.
For real-Git validation use a fresh disposable local checkout with Git >=2.48 and
an independently inspected full lowercase commit ID; register only in a disposable
Coven workspace. Repeat the **identical original request/token** for retry, not the
returned revision. No provider, worker or actual Djinn instance is needed for the
controlled Rust-producer acceptance batch.

### Related files

- `src/main.rs`, `src/lib.rs`: CLI routing and early private supervisor entry.
- `src/process.rs`, `src/process/identity.rs`, `src/platform/{mod,unix}.rs`,
  `src/platform/unix/process.rs`: transport, shared metadata/pinning and supervision.
- `src/repositories{.rs,/record.rs,/registration.rs,/git.rs,/timestamp.rs}`:
  immutable authority, known-result confirmation, bounded Git and retained chronology.
- `src/workers{.rs,/help.rs}`, `src/djinn_observation{.rs,/schema.rs}`:
  help-only and persisted-evidence projections.
- `tests/process_integrations.rs`, `tests/_process_integrations_parity.py`,
  `tests/process_supervision.rs`, `tests/workers_readiness_native.rs`: actual CLI,
  temporary oracle, actual-binary supervisor bridge and development fixtures.
- [Repository contract](REPOSITORY_REGISTRATION_CONTRACT.md),
  [readiness contract](WORKER_READINESS_CONTRACT.md),
  [persisted Djinn contract](DJINN_OBSERVATION_CONTRACT.md): underlying semantics;
  native profile and validation qualifications above remain explicit.

## Native legacy lifecycle completion

### Overview

The twelve remaining legacy routes are implemented and the **Linux native workflow
gate is accepted within the documented profiles**, not all legacy compatibility or
macOS. Prior send/agent send/down/stop and approved-action behavior remain protected.
Coverage is now **61 represented / 17 absent of 78 Python paths** (aliases count),
plus native-only `execution history`; this is not a parity percentage.
No new dependencies or production Python edits accompany this batch. There is no
staging, commit, push, installation or default-entrypoint cutover.

### API / Interfaces

```text
standup [WORKSPACE]
console [WORKSPACE]
lead [WORKSPACE] [show|status|set AGENT|AGENT|clear|remove|unset] [--clear] [--message TEXT]
bootstrap [WORKSPACE]
agent status [WORKSPACE] [AGENT...]
agent start|stop|restart [WORKSPACE] AGENT...
plan [WORKSPACE]
up [WORKSPACE] [--replace]
start [WORKSPACE] [--replace]  # alias of up
restart [WORKSPACE]
```

Selectors retain exact persisted IDs and narrow legacy operand selection. Lead
`--clear` takes precedence; absent/empty `--message` uses the literal default
prompt. Safe historical lead/state IDs need not be current roster members; explicit
agent selections must all validate against today's roster before backend creation.

### Data Flow

1. Preflight the full workspace/operand/script or prompt batch before effects.
   Shared transport admission precedes workspace exclusion, with fresh mode and
   state rechecks. Native backend I/O occurs outside workspace exclusion. Lifecycle
   preserves complete tmux/Herdr topology, reuse/replacement, restart and ordered
   agent-batch effects; partial effects are not implicitly rolled back.
2. Focus requires **both stdin and stdout TTY**. tmux attach/switch uses a distinct
   inherited-stdio foreground runner with no capture/output budget or 60-second cap.
   Shared admission remains held through attach, so activation waits (and may time
   out); unrelated workspace I/O remains lock-free during the foreground wait.
   Only the direct CLI is owned/reaped; servers/daemon effects remain alive.
3. Bootstrap stages all existing/custom/missing starter text before creation or
   sending, and revalidates the batch under exclusion. Ordinary nonzero text/paste/
   Enter exits are intentionally ignored: counts/events describe **attempts**, not
   delivery or worker acceptance. Setup, timeout and capture errors stop, without retry.
4. Lead set preserves unknown extensions and atomically publishes exact preadmitted
   storage bytes. Clear **unlinks and syncs the parent**, never writes JSON null.
   Known primary sync uncertainty exits 1 and skips secondaries/notices; committed
   secondary failures warn and continue with exit 0. Notice admission is independent
   after releasing workspace exclusion; activation can suppress it without rollback.
   Message history is written once, not again through public send.
5. Standup/status returns approved-mode **unverified** without constructing a backend.
   Console shares the native services and borrowed input/output with nested review;
   it never relocks stdin. Confirmation defaults to No, decisions bind the displayed
   revision with no refresh/retry, and maximum checkpoint status survives EOF/quit.

### Edge Cases

- **Delayed shell input is not fenced.** Generated backend-owned senders preserve
  the real five-second legacy delay. Tests release shared creation admission, hold
  exclusive admission, and still observe delayed input. Activation drains admitted
  calls, not already-scheduled shell input; do not infer stronger quiescence.
- Captured backend calls retain 60 seconds/1 MiB combined output, null stdin,
  caller cwd/environment, pre-image-only PATH fallback and direct-child ownership.
  Foreground execution is deliberately separate from capture and observation probes.
  Shared child hygiene applies CLOEXEC to low/high nonstandard descriptors without
  corrupting the exec-error handshake or changing the parent's flags.
- New incremental staging profiles: **8 MiB aggregate scripts + agent IDs** for
  launch drafts (including full-roster validation for a selected agent), and **8 MiB
  aggregate starter texts + IDs + relative paths** for bootstrap, checked before
  append. Each generated script is at most **1 MiB**. Standup retains at most
  **32 MiB staged rows**, independently of its **32 MiB complete output** cap.
- Lead exact storage bytes are parsed as `LegacySnapshot` and structurally checked
  before publication, including the 8 MiB input and node/container/string/depth
  profiles. Pretty expansion or metadata-added nodes cannot publish an unreadable
  candidate. The actual timestamp/reloaded candidate is admitted again under lock.
- Raw configured-command NUL is rejected **before normalization**, before a
  destructive restart; final generated scripts are validated too. Existing reader
  limits remain unchanged. These new writer/display caps require explicit final
  retirement compatibility disposition; staging caps are not universal heap bounds.
- Shared Python-compatible formatting has one implementation (visibility refactor),
  not duplicated formatter code or duplicate test counts. Literal/control output
  remains surface-specific; no sandbox, worker readiness or universal crash guarantee.

### Example usage

Use explicit `target/debug/coven` paths as in the README's
[legacy examples](README.md#native-legacy-lifecycle). Plan appends an event; lead
clear mutates metadata. `up`/`start`, restart and agent start/restart may launch
unrestricted tools; bootstrap sends input. Do not validate them against actual user
workspaces or installed runtimes merely to reproduce the acceptance batch.

### Related files

- `src/main.rs`, `src/cli_operands.rs`: CLI route/operand compatibility.
- `src/runtime/lifecycle{.rs,/create.rs,/staging.rs}`, `src/runtime/bootstrap.rs`:
  topology, admission and bounded whole-batch drafts.
- `src/runtime{.rs,/transport.rs}`, `src/transport_gate.rs`: preserved legacy effects.
- `src/runtime_foreground.rs`, `src/platform/unix/runtime_foreground.rs`,
  `src/platform/unix/runtime_child.rs`: foreground wait/TTY and shared FD hygiene.
- `src/runtime/presentation{.rs,/standup.rs}`, `src/lead{.rs,/prompt.rs}`,
  `src/console{.rs,/services.rs}`, `src/checkpoint_decisions/review.rs`: presentation,
  metadata, notices and borrowed-I/O review.
- `src/workspace_assets.rs`: shared pure launch-script generation and NUL admission.
- `tests/legacy_lifecycle_workflow.rs`, `tests/_legacy_lifecycle_parity.py`,
  `tests/support/legacy_backend_fixture.rs`: compiled inert CLI acceptance.

## Validation

Use the existing installed Rust toolchain; this slice was exercised with Rust/
Cargo **1.96.1**. The manifest declares Rust 1.85 compatibility but that compiler
version has not been independently run. Cargo.lock is generated offline.

```sh
just test-rust     # Native unit/integration tests, including Python oracle
just test-python   # All retained Python regressions
just check         # rustfmt check, strict clippy, both test suites
```

The historical full pinned `just check` passed rustfmt, all-target offline locked Clippy with
`-D warnings`, **586 native tests** (507 library + 32 binary + 47 integration), and
**941 Python tests**.

Current process-integration acceptance reports **144 native CLI cases** with a
compiled Rust producer, no Python children, and real disposable Git. Targeted
results report **24 repository, 11 readiness, 10 Djinn and 14 identity tests** plus
the actual-application-binary supervisor bridge. These are separate coverage layers,
not additive full-suite totals. Supervisor tests were added for closed stdio, FD
churn, malformed private bootstrap, delayed descendants and reported cleanup faults,
alongside exact/+1 combined caps, parent death/stoppage, leader/EOF ordering and
normal/nonzero descendant cleanup. No actual Djinn instance/provider/job was called.

The preceding process-integration full pinned gate passed **659 native tests
(569 library + 37 binary + 53 top-level integration)** and **941 Python tests in
195.379s**; this is historical evidence, not the current total.

**Historical reviewed-runtime full pinned `just check`: PASS — 763 native passed (663 library + 46 binary
+ 54 top-level integration), plus 941 Python tests in 190.822s (`OK`).** Rustfmt and
offline locked all-target Clippy with `-D warnings` also passed. This verifies the
working tree based on HEAD `e389a08`, including the uncommitted execution-authoring
and process-integration work plus reviewed runtime; it is not evidence of a
committed release or cutover.
The complete saved output was inspected through its final Python `OK`; no expensive
tests were rerun for this documentation update.

That historical aggregation uses the final result of each top-level Cargo suite: library **663
passed / 2 ignored** out of 665 entries, binary 46, and 28 integration suites
totaling 54 (zero doc-tests). The ignored `closed_stdio_entry` and
`inherited_descriptors_entry` are subprocess-only fixtures exercised by parent
tests, not two extra passes or omitted workflow coverage. Nested filtered
libtest invocations are excluded, including the library rerun and isolated children
inside `tests/process_supervision.rs`, and mode/privilege child runs elsewhere.
CLI acceptance cases within a harness are not additional Cargo tests. This avoids
double-counting the actual-binary supervisor bridge's child output.

**Historical legacy-lifecycle full pinned `just check`: PASS — 863 native passed (752 library + 56
binary + 55 top-level integration across 29 suites), and 941 Python tests in
189.082s (`OK`).** Formatting and offline locked all-target Clippy with warnings
denied passed. Saved output:
`/home/jdawson/.local/share/opencode/tool-output/tool_1ac64b61f001wobNiDPT9BLMgQ`.
The library's top-level result is **752 passed / 3 ignored** (755 entries). Its
`closed_stdio_entry`, `inherited_descriptors_entry` and foreground `isolated_entry`
helpers are exercised by parents, not extra passes. Sum only the final top-level
results; exclude nested libtest children/reruns and internal CLI cases. Doc-tests
contribute zero. The complete output ends with Python `OK`; this docs-only update
does not rerun full tests or imply a release/default cutover.

### Worktree reconciliation validation

The selective main-checkout ports pass their focused gates: **215 init CLI cases**,
**208 state/refresh CLI cases**, four new init parser tests, exact/over checkpoint
work bounds and a Python-held/native-refresh descriptor/wait-state handshake that
changes tasks before release and verifies a freshly rendered dashboard. A real
`RLIMIT_FSIZE=128`/ignored-`SIGXFSZ` test preserves native partial
`state/agents.json` plus reservation/lock, versus Python's config-first partial
`coven.json`; both refuse reset attempts without changing evidence. Failure-artifact
equality is deliberately not claimed.

The initial reconciliation run passed focused checks but exposed two integration
blockers. Both are now fixed without weakening the assertions:

- Group reservation now clones the selected declaration's first endpoint directly
  from ordered authority under the lock, after CAS and whole-target validation.
  Native report conversion can reorder JSON keys; it must not choose persisted
  counter bytes. Existing counter representations, arbitrary-precision endpoint
  integers, declarations and revision semantics remain unchanged. The workflow
  compares complete native/Python authority bytes for all six endpoint-key orders.
- The actual-binary supervisor could lose captured stdout under closed-stdio/FD
  churn: a pipe writer could occupy FD 1, where `dup2(1,1)` retains `CLOEXEC` and
  exec closes stdout. Null stdin and both ends of each explicit output pipe are now
  owned high-numbered CLOEXEC descriptors before `Command` maps stdio. Tests retain
  every closed-stdio mask, exact output, churn, spawn failure and cleanup check,
  with added deterministic low-slot pipe allocation/EOF/CLOEXEC assertions.

**Historical pre-cutover complete Linux validation: PASS — 1,191 native tests (998 library +
86 CLI + 107 top-level integration tests across 37 suites), all 941 Python
regressions in 220.389s, and 18 development-tool tests in 27.407s.**
The preceding Group/Observe milestone was 1,086 native / 941 Python in 207.460s;
that result remains historical. Three subprocess-only library helpers remain ignored in
the outer suite and exercised by parents. Nested libtest reruns, helper processes
and internal CLI cases are excluded from the total; doc-tests contribute zero.
The final Rust output ends with successful doc-tests:
`/home/jdawson/.local/share/opencode/tool-output/tool_1b7a1d1a2001GMqI85WV31aS2C`.
Its outer library result is 998 passed / 3 ignored (1001 entries), CLI is 86 passed,
and the 37 top-level integration results sum to 107 passed. Nested library/helper
reruns (including process supervision) and internal acceptance cases are not added.
Formatting, offline locked all-target Clippy with warnings denied, and whitespace
checks also pass. Commands were the existing format/lint/Python recipes plus
`PYTHONDONTWRITEBYTECODE=1 CARGO_BUILD_JOBS=2 cargo test --offline --locked --no-fail-fast`.
Rust/Cargo **1.99.1** was used. The pipe implementation remains compatible with
the declared 1.85 API profile; that compiler itself was not independently run.

Group/Observe acceptance now reports **79 cases across all ten routes** (eight
Group plus Kitsune/Djinn-live), including all six endpoint-key permutations and
the retained reservation race/uncertainty checks. This closes the bounded Linux
implementation gates, not compatibility-profile disposition or retirement.
The actual-binary process-supervision bridge passes in the same full run.

The pipe fix uses atomic `pipe2(O_CLOEXEC)` on Linux; non-Linux POSIX uses
`pipe` followed by CLOEXEC setup before high-FD duplication. That fallback retains
a concurrent fork/exec inheritance window during initial flag setup; macOS was
not exercised. Extra transient descriptor pressure fails closed on allocation
errors. Production process ownership, watchdog, output bounds and cleanup policy
are unchanged. Independent scoped review found no material issues; descriptor-
exhaustion fault injection and explicit mixed-order two-member endpoint tests remain
optional strengthening, not claimed coverage.

Three earlier fixture failures were corrected without production changes: stable
manifest-root lookup for Kitsune Python oracles and owner-writable private Git
copies. A signal test uses real thread-directed `raise(SIGINT)` so the handler
completes before fixture EOF, and prints child exit status on failure; production
signal ownership is unchanged. These checks cover the preserved uncommitted
migration tree, not a committed release, macOS validation or default cutover.

Legacy lifecycle acceptance reports **53 cases, 314 native CLI calls, all 12
routes, 1503 compiled-backend calls, 151 parent Python oracle replays and 16
TTY-mask cases**. Native children have no Python in fake-backend PATH; no actual
tmux/Herdr/provider/user sessions are used. Separate unit evidence includes an
actual **61-second foreground inherited-stdio child**, all four PTY masks,
low/high-FD CLOEXEC, shared admission held through attach, activation waiting and
workspace I/O availability. Generated-shell proof is **one test / two backend
cases**, actually executed via inert `/bin/sh` and compiled fake tools at special
paths, with a real five-second sleep each. It checks Herdr command substitution
strips final LF whereas tmux preserves file bytes. Delayed input still occurs
after shared creation admission ends while exclusive admission is held: intentional
legacy behavior, **not activation fencing**. These layers are not additive Cargo
test totals. The retained reviewed **16-case / 176-call** batch and every full
suite remain green. Reviews fixed lead unreadable-candidate admission, raw NUL
restart and aggregate staging/output amplification; existing readers are unchanged.

The 586/941, 659/941 and 763/941 totals above remain historical. Repository and worker readiness
have accepted Linux implementation workflow gates, not full platform/profile or
retirement acceptance. Standalone
readiness fixture tests intentionally use a Python fake **target** as development
coverage, not proof of Python absence. The separate compiled-Rust-producer CLI batch
establishes interpreter-independent children. The parent Python oracle and retained
runner-backed assertions still require retirement without losing coverage. All
current evidence is Linux-only; macOS acceptance remains open.

Reviewed runtime acceptance reports **16 compiled-fixture cases, 176 native CLI
calls and all 12 new routes**. Every call carries low and high (>=2048) inherited-FD
sentinels; both backends exercise real five-second delays. Children use compiled
inert fixtures without Python, actual providers or tmux sessions. Parent Python
oracles remain development-only. Coverage includes positive legacy checkpoint
text/Enter, committed warnings, exact scope/CAS, durable consumption, no replay,
current-input drift, uncertainty/crash seams and frozen stop. Unit uncertainty
seams are not a universal power-loss guarantee. Independent review reports no
remaining findings after the lock-scope, unresolved-name, FD, failed-interpreter
fallback and legacy caller-cwd corrections.

`tests/_action_parity.py` and the checkpoint-workspace refusal corpus now use
invalid flags for newly supported writers; all no-effect guards are preserved,
not dropped as inconvenient coverage. Full-gate harness success is one top-level
test, not another 16 or 176 tests added to the native total.

Execution authoring has **141 green acceptance cases**, including **128 CLI cases**
and **six uncontrolled shared-lock races**, with empty child PATH (Python parent
oracle only, not fallback). These cover fresh standalone and reviewed two-node
flows, not maximum-size lifecycle coverage or both lock orderings. Targeted native
activation/upgrade process-death and uncertainty tests and real-coordinator near-limit
revocation proofs pass. Historical inspection/cancel/recover/export remain
protected; these layers are not additive native-test counts.

Critical architecture, publication and capacity reviews found no material code
defects. Activation faults cover mkdir and both directory-sync boundaries. A real
test-process death after complete backup publication leaves v1 authoritative and
the evidence intact; retry uses a new backup UUID. The capacity coordinator test
permits a real approved-head revocation but rejects repeated growth that would
consume active preparations' escape room. These are targeted failure/interleaving
proofs under the existing filesystem assumptions, not universal crash survival or
both winner orders for every race.

Previously delivered maintenance coverage includes **104 execution tests** and
**10 storage-maintenance tests**, retained within the expanded native total. Maintenance acceptance
passes **229 cases**, including **210 CLI cases**, **12 mixed-writer races**, and a
process-death lock-release check. These describe different test layers, not additive
native-test counts. Coverage includes v1/v2,
legacy receipt retry after upgrade, source independence, both recovery modes,
prepublication limits, late known-result failure, mode/inode preservation and
native/Python lock ordering. The admitted warning-slot regression exercises both
output modes, fresh and replayed receipts, exact-16-KiB adversarial control/multibyte
reasons and overlong diagnostics. Architecture, publication, recovery/locking and
parser reviews found no material findings. Local process-death/fault tests are not
power-loss proof or hostile-namespace guarantees.

The prior inspection-focused execution suite passed **76 unit tests**, including
**16 source / four projection tests**. The retained inspection
corpus still passes **539 total = 534 CLI + five oracle
state-machine cases**. **Eight parser-refusal/no-effect guards**
are counted separately; the two old cancel/recover guards now use invalid flags,
because valid cancellation/recovery is supported. Other unsupported routes remain
parser refusals.
It covers all five routes, parser/selector timing, proposal order, current versus
whole-revision binding, source-independent export, mixed-lock behavior, full-tree
preservation and empty runtime PATH. Resource regressions include nested action
input/encoding refusals across all four source-assessing commands, export against
the same unsupported current files, and the private action-work fault seam. Real
cross-module tests protect these limits from being downgraded into ordinary source
diagnostics. Architecture, coordination, projection and parser reviews found no
remaining material findings after corrections. These are bounded local tests,
not universal crash-durability, freshness or hostile-namespace guarantees.

Checkpoint inspection and guidance have **403 + 177 = 580 green native CLI cases**.
Expanded cases cover exact negative-prefix root restoration, token collisions,
repeated flags, terminators, early help without workspace reads, generic parser
diagnostics without internal tokens, and explicit non-UTF-8 profile refusals.
Timestamp coverage has five unit tests, including a **364,346 exact-key Python 3.14
oracle** (213,597 nonminimum keys), 1 MiB long-input cases and a 60-second oracle
deadline. The unignored Unicode Nd oracle compares all **1,112,064 Unicode scalars**,
pinning 760 Nd code points/71 ranges. A 2,059-argument parser test verifies exact
restored values, unchanged indices and every repeated flag. Independent architecture,
parser/read-safety and timestamp reviews found no remaining actionable code issues.
These tests characterize the bounded native profile, not exhaustive interpreter
equivalence or hostile-namespace safety.

Local workspace acceptance adds **348 green native CLI cases**: 346 Python-oracle
cases plus two native buffered-stdout failure cases. They cover exact literal
text/newlines, full-tree effects and preservation, history formatting/profile
boundaries, snapshot/pending/active/runtime-control behavior, offline command loops
and mixed Python/native flock writers. Private units cover complete message writes
followed by sync failure, actual partial event writes, independent goal secondaries,
fresh admission at the second dashboard lock epoch, maximum message-party strings,
known goal/dashboard uncertainty, and full-output admission. Explicit flushing is
tested with both a deferred-error writer and real pipes closed before child spawn.
Independent architecture, persistence and history reviews found no remaining
actionable issues after corrections; these are bounded local tests, not universal
crash-durability or hostile-namespace guarantees.

Native init has **215 green real CLI cases**, including complete tree/byte/mode
comparisons, every flag, 104 reserved-name/type/force combinations, path and
environment selection, offline execution without Python/tools, native-created
workspace interoperability, and native/mixed-language contention. The Python-only
oracle run has 194 cases. Normalization changes only validated clock/event-ID
fields and root paths, never arbitrary timestamp/UUID-looking literals.

Private units cover pre-effect filename/staging/output admission, 56 actual
non-marker partial-write/file-sync faults, reservation and marker publication
faults, cleanup failures, inode/owner checks and marker-known uncertainty. Tests
also exercise an unprivileged execute-only ancestor, real Python marker publication
while native holds its reservation, and the exact operation-scoped ancestry-sync
interleaving described above. These are targeted fault characterizations, not an
exhaustive filesystem or power-loss proof. Independent architecture/persistence
reviews accepted the bounded contract; cross-initializer ancestry durability
remains an explicit limitation, not a corrected race.

The native suite includes a build/CLI-name regression test and an
integration harness with **109 handoff parity
cases** and one explicit deferred non-UTF-8 reporting case. Characterizations cover
otherwise-valid duplicate keys and escaped-equivalent keys, required null versus
missing, literal integer versions, Unicode/byte boundaries, source-state claims,
frozen canonical bytes, Python-produced literal-Unicode archives, unknown fields,
hash/binding/filename/encoding/LF corruption, historical reads despite corrupt
current tasks, full-cohort filtering, 256/257 entries, exact/over byte limits,
inert temps, symlink/FIFO refusal and effect-free unsupported commands.

The independent CPython value corpus adds **17,464 byte/bit cases**: every finite
exponent region with selected significands, seeded random binary64 patterns,
fixed/scientific boundaries, extremes/subnormals, exact decimal midpoints and
perturbations, negative zero, bounded large integers, Unicode, duplicate order,
canonical bytes and indented storage bytes. Python's integer digit limit is pinned
to 4300 in the oracle. This is strong characterization, not exhaustive binary64 or
arbitrary-token proof. A frozen decimal-tie regression catches the real difference
found with Rust's standard Debug float digits versus CPython's even candidate.

The snapshot CLI oracle adds **43 cases**, including broader preflight, exact-byte
revision, extensions, missing/invalid records, explicit nonfinite/scalar refusals,
tilde/empty argument behavior, ledger/runtime refusal before snapshot access and
no filesystem mutation. Mixed Python/Rust lock tests establish both directions of
exclusion, aliases, unchanged inode and kernel release after killing the owner.
Fork/exec lifetime corner cases, macOS execution and network-filesystem behavior
have not been independently established by this test set.

The task-ledger oracle adds **94 authority/corruption/effect cases** against real
Python-produced imports/histories. It includes noncanonical and absent baselines,
CAS-bound historical requests, Unicode/float/big-int extensions, roster drift,
projection modes (including FIFO/symlink and a 9 MiB readable projection), wrong
field/type/identity/mode/digest/chain/sequence/directory/envelope data, duplicate
requests, semantic effects with recomputed hashes/revisions, strict backup keys,
unsupported raw encoding, and invalid final frames. Native unit tests exercise
captured-command transitions, exact temp recognition, frame/history/directory
boundary checks and overflow. Reads leave bytes/modes/namespace/lock inode unchanged.

Collection adds **16 mixed Python/native producer cases**: native-first and
Python-first exact archive bytes/IDs, existing inode/mtime preservation, concurrent
collectors, active ledger with bad/FIFO projection, unrelated FIFO controls,
real prepared reservation preservation, stale/hash/owner/roster/duplicate/pending/
corrupt/path/capacity refusals and an explicit out-of-native-profile context.
Internal fault fixtures cover mkdir/temp/write/partial-write/file-sync/link/
pre/post-link directory-sync/cleanup, replay read/open/metadata faults, authority
changes before locked capture, wrong guard, integrity/unsafe-path refusal and
exact/over byte reservations. CLI uncertainty maps to exit2. Fault injection is
internal to tests, never activated by production environment variables.

The original execution corpus has **120 oracle cases**: 36 accepted reports,
70 corruption/path/capacity refusals and 14 Python-valid native-profile refusals
(the previously refused valid v2 history is now accepted). Real Python histories cover
snapshot/ledger source rebind, cancellation/reprepare, multiple reservations and
frozen preparation receipts after cancellation. Re-signed downstream prefixes
isolate semantic corruption from hash failure. Tests retain numeric/Unicode values,
move identities, ignore malformed/missing/FIFO current files, reject unsafe managed
paths, retain v2 reservations, and establish success without Python on PATH.
They snapshot namespace/bytes/modes/inodes/mtimes on success and refusal. Native
unit tests cover exact work/history/capacity limits and timestamp calendar bounds;
a valid small control with DEL text exercises final ASCII expansion refusal with
empty stdout and no writes. Source inspection, rather than syscall tracing, also
establishes the absence of network/runtime/fsync operations in this isolated path.

The v2 corpus adds **175 cases**: 56 accepted reports, 117 semantic/schema/path
refusals and 2 Python-valid native-profile refusals. It covers actual backed-up
Python upgrades, mixed-era receipts/attempts, standalone/null graph bindings,
snapshot/ledger proposals, repeated reviews, downstream-first graph preparation,
revocation/cancellation independence, successors and sticky membership, unrelated
source drift, full-source refreezing, and invalid early reserve prefixes followed
by valid final states. Semantic corruptions re-sign all downstream prefixes;
representative uniqueness/head/managed-task/reserve fixtures repair corresponding
receipts and pin both oracle diagnostics and native error codes. Backups/current
files and lock paths may be absent, malformed, unsafe, FIFO or held without becoming
reader dependencies. Success/refusal snapshots retain bytes, namespace, modes,
inodes and mtimes; success without Python on PATH remains covered.

Focused graph units exercise exact node/edge/size limits, lexical topology without
array normalization, digest vectors, stale enrollment snapshot refreezing and
source-match reason ordering. Extraction units carry actual cancellation identities,
immutable receipts and predecessor state. Fixed work totals (54,929 v1 bytes and
81,919 through v2 upgrade for one fixture) pin the shared meter, with exact
precharged MAX_WORK success/one-over refusal; deliberately resetting the embedded
meter was verified to fail the regression. CLI work-profile fixtures separately
cover conservative combined preflight, not this private shared-meter invariant.

Action inspection adds **132 cases**: 54 accepted parity, 53 control refusals,
16 native-profile refusals, 4 resource-policy refusals and 5 unsupported-CLI refusals.
Real Python enable/prepare/review/claim/outcome fixtures never execute runtime submit.
Cases cover both plan backends, all-claim validation, stale/opaque bundles, claimed/
unknown/terminal and acknowledged late outcomes, consumed-reference independence,
current-file/lock FIFO and held-lock independence, optional artifact diagnostics,
copy/move refusal and actual device/inode plus 2^64 truncation traps. Recorded
claims are duplicated in output; a dedicated DEL fixture proves the escaped report
can exceed 32 MiB even when control alone and compact output fit, with empty stdout.
Independently calculated work formulas and precharged command seams pin exhaustion
inside optional bundle validation, compact output and ASCII output without huge
allocations. Plan units cover sorted/spaced exact limits and frozen Python vectors.

Pure checkpoint inspection adds **2,761 byte-level oracle cases**: 2,212 accepted
views and 549 rejected inputs, including 12 exact resource refusals. These cover
framed digest vectors, Unicode status/trim/lowercase, line endings, scanner and
machinery precedence, nested examples, trailers/internal history, presentation
contradiction and stale scope. Parser resource errors retain their classification
through action inspection; ordinary malformed JSON remains a diagnostic retaining
the known claim. Existing storage/numeric corpora also protect unchanged codec modes.

The original standalone checkpoint workspace corpus had **280 CLI cases**: 101 accepted,
150 semantic/preflight refusals, 13 parser-boundary refusals, 15 explicit profile
refusals and one permission-sensitive marker-selection probe. The permission probe
ran here and pinned cwd selection/first configuration error; it explicitly skips
when privilege makes denial ineffective. Cases cover marker/empty/tilde ambiguity,
failure order, full managed inventories, team/TOML/environment/state, sorted Unicode/
dotfile/name edges, pending import, active authority with unsafe projections, valid
runtime control and unusable bundle children, ignored execution control, held locks,
no-Python PATH, unchanged bytes/modes/inodes/mtimes and old-command policy separation.
Unit tests compare managed inventories to Python constants, test exact 4096/4097
scan boundaries and controlled authority changes between independent ledger reads.
An independent nested TOML scalar/runtime-field corpus protects calendar and numeric
profile behavior; parser grouping tests preserve semantic arity checks after preflight.

Frozen receipt lookup adds **95 direct-library oracle cases**: 25 results/misses,
16 changed-binding conflicts, 8 invalid keys, 38 invalid bindings, 2 absent/pending
authorities and 6 corrupt-tail read refusals. Python-produced histories preserve
old bindings/results after later owner changes; raw aliases, stale original CAS,
object order, extensions, numeric identity, clones, poisoned current files and
marker changes are characterized without CLI or writer activation. The corpus
has a 60-second isolated child-process watchdog with parent-owned fixture cleanup
so accidental FIFO reads cannot hang the suite. Units pin multi-receipt accounting,
exact budget/overflow, programmatic string bounds, raw float version spellings and
the pre-existing replay validation precedence.

Mutable replacement adds a **120-combination fault matrix** across existing/absent
targets, permission modes, I/O boundaries and cleanup outcomes, plus a silent-SGID-
loss regression using the real descriptor verifier. Failures before rename preserve
old bytes/mode/inode; post-rename parent-sync failure retains new bytes as uncertain.
Tests cover wrong ownership tokens, unsafe paths/FIFOs, both lock names, missing
parents, exact/over transport cap and inert orphans. A local mixed Python/Rust test
performs 65 Python and 64 Rust writes under the same persistent flock, checks whole
payload observations from both writers, 16 exclusion checks, preserved special
mode bits and unchanged lock device/inode. The SGID regression simulates the kernel
result; no administrator/nonmember-group setup or crash/power-loss test is claimed.

Locked authority confirmation adds **95 direct-library cases** with independently
pinned counts: 24 ledger confirmations, 30 active confirmed-clear reservation
checks, 2 disabled observations, 6 reservation blocks, 31 invalid-authority refusals,
1 native-profile refusal and 1 absent ledger. Three Python-first and three native-
first lock-order scenarios verify append/prepare/cancel serialization, exclusion
even after SDK errors, borrowed-lock lifetime and persistent lock identity.
Fixtures and native probes have bounded watchdogs and process-group cleanup.

Ledger units trace the exact selected files and leaf-to-root directories for
present/absent baselines and multiple frames. Every sync-stage path/open/descriptor/
fsync fault checks outcome and immediate stop; pre-sync reread failures remain hard.
Independent revision/head checks, changed manifest digest/type, corrupt tails and
wrong-root-before-I/O are covered. Execution units inject all sync faults across
matching/unrelated/authority-wide checks in clear/prepared/cancelled and V2 approved/
graph-prepared/revoked states, plus second-read cancellation/upgrade/disappearance/
corruption. Existing 54,929/81,919 work counters and 295 report-parity cases continue
passing. Synchronization may change storage durability; no namespace/content/mode/
inode/mtime mutation is expected, and atime is not used as an invariant.

Baseline activation/recovery adds **133 cross-language SDK cases** and six flock
ordering checks: native artifacts are read by Python, pending baselines resume in
both directions, and Python task follow-ups accept native imports. Cases preserve
raw source bytes/mode/inode, exact canonical frame/manifest/marker bytes, digests,
numeric/Unicode extensions, absent projection, strict source parsing, execution
reservations, raw-manifest recovery, backup-mode/source-pin distinctions, ambiguous
or incomplete pending artifacts and exact nondestructive archive transformations.
The explicit import/recovery CLI is covered separately by the lifecycle corpus below.

The private publisher's fault matrix covers **144 combinations** (four artifact
roles × two target states × nine boundaries × two cleanup outcomes), plus capacity,
collision, SGID verification and exact-8-MiB frame cases. Activation units inject
every phase, distinguish pending versus known active/archive uncertainty, and prove
the real candidate validator rejects five already-published corruptions—including
a valid sequence-one tail that fails first import's sequence-zero gate. Fixed-ID
Python vectors and source-valid/wrapped-invalid node/depth fixtures protect planning.
Four isolated umask/SGID-parent probes pin twelve exact directory access-mode checks
without changing the parent test process's umask. Complete/short/failed entropy
reads and UUIDv4 shape are independently tested. Crash/power-loss behavior and a
real nonmember-group SGID setup are not claimed.

Task add/assign adds **262 real CLI cases** across both backends: add/list/assign/list,
JSON and human output, exact storage/frame/projection/event/dashboard bytes,
dynamic UUID/time format and allocation, CAS, aliases, parser ordering/repeats,
strict reader/output profiles, prepared guards, receipt-first replay/no backfill,
secondary warnings, missing event parents, runtime controls and mixed Python/native
writers/import races. Native invocations run with no Python/runtime executables
on PATH. A deterministic native-first assignment/Python preparation test on both
backends proves lock contention, committed owner change, stale preparation CAS,
no attempt/receipt and stable lock inode; prepare-first is also covered.

Private fault tests pin snapshot pre/post replacement, confirmation hit/miss,
uncertain-frame matching/missing/corrupt/conflicting rereads, exact backend error
precedence, fresh guarded drafts and independent later secondary effects. A maximal
admitted receipt plus five warnings/health metadata renders below the output cap.
Low-level append tests exercise actual partial writes/fsync errors and descriptor
flags, symlink/FIFO/device refusal. Dashboard golden bytes and all Unicode scalars
are compared against the version-pinned oracle. The neutral workspace extraction
retains the checkpoint/workspace corpus (now 403 cases) and 43-case task-reader
corpus. `task remove` and
`task complete` are unsupported in both Python and Rust, not outstanding port work.

State lifecycle/refresh adds **208 green CLI cases**, including **7 explicit native
safety cases**, alongside the retained **262 task-writer cases**. Coverage includes
state presentation, import/resume/abort/repair, raw evidence preservation, refresh
preservation and partial effects, hardlink-before-truncate refusal, literal root
labels and workspace-specific preserved paths. Independent state/refresh reviewers
approved the corrected hardlink, conditional printf and preserved-path behavior.
Private fault tests cover authority/backup/replacement uncertainty and late rendering
without erasing known primary outcomes. These are local validation results, not
hostile-swap or cross-platform guarantees.

The historical legacy-lifecycle full gate passed **941 Python tests in 189.082s**;
the historical reconciliation gate passed all 941 in 220.389s. They are not all black-box CLI
tests; a passing Python suite alone is not proof of Rust coverage. Exact original
assertion/fault/coordination obligations still need review, not just these passes.

## Native Session completion

### Overview

All seven routes are native: `session list`, `inspect`, `watch`, `start`, `resume`,
`prompt-capabilities` and `prompt`. Views are flags on readers/watch, not another
route. Independent fixtures pass without a Python child or live user runtime;
the Session implementation gate is **implemented on Linux within profile**.
This does not close original-obligation review or certify retirement. The
authoritative inventory retains 941 original method IDs: 67 mapped, 874 unresolved.
These are source-oracle cleanup obligations, not product feature counts.

### API / Interfaces

`session_inspection::Request` carries workspace/socket, optional pane/terminal,
runtime, timeout, byte budget and selection flags. Readers default to runtime
`djinn`, timeout 3 seconds and 1 MiB response budget; accepted budget is 1 KiB–8 MiB.
Watch defaults to 60 seconds / 2-second interval (duration 0.1–3600 seconds,
interval 0.1–60 seconds). Inspect requires a pane; a terminal pin requires a pane.
`interactive_sessions::Request` adds operation/executable/execute and resume selector;
`session_prompts::Request` instead carries the native socket, runtime/session/request
pins and prompt file. Timeout is positive, finite and at most 30 seconds.
Native Session, action inspect and handoff parsing accepts unambiguous long-option
prefixes and repeated flags/values (last value wins); unknown flags still refuse.

### Data Flow

Readers take complete bounded generic pane/process observations before pure view
selection, preserving original counts/declaration qualifiers. They do not open live
attachments, mutate groups, create a session registry or infer native activity.
Watch streams and flushes samples and one scoped summary through its writer; its returned
`TaskOutput.stdout` is empty. No stale positive fallback, output retry or daemon exists.
Start/resume construct pure plans, then only with `--execute` acquire shared legacy
transport admission and submit through discovered, terminal-pinned generic control.
Prompt capabilities uses the explicit native socket; prompt preview performs lexical
validation only. Execute reads the bounded prompt and uses native capability/runtime/
session pins plus a correlated acceptance receipt, without terminal input fallback.

### Edge Cases

Previews never touch workspace/files/endpoints; pins are not grants or native
permission inheritance. Empty workspace means cwd. The reviewed empty-workspace,
symlink-parent/double-slash no-follow and sample/summary EINTR exit-130 regressions
are covered. Ordinary stream failure stops without summary retry; returned buffered
stdout remains empty. Endpoint privacy and response/prompt bounds still apply.
Queued input is not startup/resume/readiness; accepted prompt is not model execution,
worker-policy admission or completion. Uncertainty never authorizes automatic retry.
Parent aliases are trusted, metadata checks are not authentication, filesystem calls
are not interruptible hard deadlines and read requests may cause server housekeeping.

### Example usage

See [all seven placeholder command examples](README.md#native-session-workflows).
These are reference syntax, not instructions to contact live user endpoints;
capabilities/read/watch do I/O, while start/resume/prompt without execute are pure.
See [staged installer usage](README.md#native-only-staged-installation-not-cutover)
for actual `--binary`, `--prefix`, optional `--examples` flags. The installer requires
a prebuilt executable and absent prefix with existing parent; it never builds,
selects the default wrapper, copies/deletes the runner or performs an upgrade.

### Related files

- `src/main.rs`, `src/cli_operands/tests/session.rs`: seven-route routing and nine new parser units.
- `src/{session_inspection,session_views,session_watch,session_support}.rs`,
  `src/{observation_socket,observation_time}.rs`: readers, selection, streaming,
  admission, cancellation and output bounds.
- `src/interactive_sessions.rs`, `src/session_prompts{.rs,/endpoint.rs,/protocol.rs}`:
  pure plans, pinned submission and native prompt correlation.
- `tests/session_readers_workflow.rs` (**11 tests**), `tests/session_prompts_workflow.rs`
  (**15 tests**), `tests/interactive_sessions_workflow.rs` (**one multi-scenario test**)
  and `tests/support/`: interpreter-independent workflow fixtures, not additional totals.
- `tests/session_views_vectors.rs`: five native tests over 127 captured calls,
  46 deduplicated documents, 107 value cases and 20 typed-boundary cases.
- `scripts/install-native.sh`, `packaging/coven.rb`, `tests/native_install.rs`:
  native-only staged installation/candidate formula, not external release promotion.

The original-obligation ledger records **941 IDs: 67 mapped / 874 unresolved**;
categories **A 53 / B 15 / C 0 / D 873** describe mechanisms, not missing behaviors.
Recorded audit `--check --summary` passes, while `--require-complete` deliberately
exits 1 for source-oracle cleanup. Eleven reviewed legacy-entrypoint transitions
retain original/current hashes and all 941 IDs; only invocation prefixes and former
wrapper environment exports changed, not assertions, cwd, timeout or remaining argv.
Independent AST review accepted those transitions, not source deletion. See
[retirement obligations](RUST_RETIREMENT.md#original-obligation-ledger-not-a-missing-behavior-count).

Native production execution has cut over without deleting all 25 Python source
oracles or tracked bytecode. Neither is installed or included in the Cargo package.
The external formula was located/read only;
`packaging/coven.rb` is unpromoted and actual install/upgrade untested. macOS was not
run because the installed SDK was unavailable; a new CI job being added has not
been triggered. Finite-profile/non-UTF-8 restrictions require explicit compatibility
approval or implementation, not presumed approval from the demand for full migration.
Full migration must precede merge; neither staging assets nor moving a runner counts.

## Migration coverage and next gates

| Surface | Current implementation | Next gate |
| --- | --- | --- |
| Handoff schema, validate, historical inspect/list | Native product CLI | Explicit encoding/profile disposition and platform validation |
| Handoff collection/publication | Native bounded unverified capture | Close profile limits/cross-platform validation |
| Finite values, ordered extensions, records, snapshot task human/JSON listing | Native bounded product CLI | Close stated acceptance limits before full legacy parity |
| Persistent workspace flock, evidence/ledger immutable publication and mutable replacement | Native guarded primitives used by task/baseline pipelines | Integrate remaining writer families without changing their policies |
| Existing-workspace refresh CLI | Native bounded generated-asset refresh with custom context; no runtime effects | Close profile and cross-platform gaps |
| Workspace init CLI | Complete native blank/embedded/custom-example CLI; staged config-last exclusive bootstrap | Close profile/cross-platform gaps; retain operation-scoped ancestry durability qualification |
| Local goal/message/history/status | Native bounded goal/message writers, pretty/raw history and dashboard-publishing status | Close supported-profile/cross-platform gaps; preserve literal output, separate lock epochs and family-specific exit-1 failure policy |
| Task-ledger read/replay, frozen receipt lookup, selected confirmation and projection diagnostics | Native bounded readers/primitives used by task/state commands | Close supported-profile and cross-platform limits |
| Task-ledger baseline import, explicit resume and pending archival | Native bounded SDK and state CLI with pinned activation/known outcomes | Close remaining profile/cross-platform gates |
| Task add/assign, commit projection, events and dashboard | Native snapshot/active-ledger commands with CAS/replay/delivery; task list defaults to human | Close documented profiles and original-obligation review before default parity |
| Manual projection repair | Native raw-preserving SDK and explicit state CLI | Close stated raw-size/profile and cross-platform limits |
| Execution v1/v2 historical replay, graph reviews and confirmed reservation checks | Native bounded SDK/read-only preview; cancellation/recovery integrated separately | Close profile/cross-platform limits; preserve independent writer confirmation |
| Preparation cancellation and execution recover (confirm/archive) | Native complete bounded commands across v1/v2/legacy retry; source-free shared flock and known-result delivery | Preserve historical limits; arbitrary older control has no native escape guarantee |
| Execution enable/enroll, full preparation (intent + graph), upgrade and graph mutations | All six native routes implemented and previously full-gate validated; receipt-first coordinator, exact backup and writer-only escape admission | Preserve acceptance; close profiles/cross-platform and retirement oracle gates, not dispatch |
| Runtime-launch action inspection and pure checkpoint inspection | Native bounded read-only preview | Close profile/encoding limits before default parity |
| Standalone checkpoint inspection and ordered workspace preflight | Native pretty-default/optional compact JSON; repeated flags and narrow Python lexical compatibility | Close TOML/path/resource and cross-platform restrictions before default parity |
| Next checkpoint guidance | Native bounded read-only composition; stable request chronology and exact fallback statuses | Close supported-profile/cross-platform gaps; retain literal output and no authority grant |
| Current-source execution/attempt/graph inspect and export CLI | All five native routes complete; pretty/compact JSON, existing-only coordinated assessment; export is source-independent full replay | Close documented profile/cross-platform gaps; preserve history/source/export boundaries and no grants |
| Checkpoint approve/suggest/interactive review (3 routes) | Linux native workflow accepted within profile; exact scope/CAS, displayed-snapshot review, committed warnings and real unlocked legacy notices | Close finite-profile/macOS/oracle/packaging gates; approval is not automatic execution permission |
| Repository inspect/verify/register (2 routes) | Native complete bounded CLI; raw immutable authority, source-free exact retries, independently hashed Git objects and pinned Git | Linux implementation workflow accepted after full gate; close profile/macOS/oracle gates |
| Worker readiness (1 route) | Native human/Unicode JSON; eight help/version surfaces, eleven unknown capabilities, actual same-binary supervision | Linux implementation workflow accepted after full gate; disposition 65536-declaration profile and macOS/oracle gates |
| Persisted observe djinn (1 of 3 Observe routes) | Native pretty/compact ASCII JSON; explicit pinned producer and fixed persisted-evidence call; full gate passed on Linux | Retain trust/profile/platform limits; persisted evidence is not live readiness |
| Reviewed cold-launch action writers (5 routes) | Linux native workflow accepted within profile; enable/prepare/submit/acknowledge-unknown/stop, immutable plan, durable claim, no replay and frozen stop | Close writer-profile/macOS/oracle/packaging gates; preserve direct-child ownership, unknown effects and no automatic retry |
| Kitsune/Djinn-live observers and all eight Group routes | Native bounded Linux workflow accepted after 79-case ten-route acceptance and full gate; original authority bytes, counters, expiry and uncertainty preserved | Close profile/macOS/oracle/packaging gates; observations/annotations never grant execution authority |
| Legacy workspace/runtime/agent | All 16 previously missing paths represented; Linux native workflow accepted within profiles, including the final 12-route lifecycle batch | Close compatibility profiles/macOS/oracle/packaging gates; retain delayed-input non-fencing, foreground admission, attempt counts and no rollback/retry |
| Session start/resume/list/inspect/watch/prompt-capabilities/prompt (7 routes; views are reader flags) | Implemented Linux workflow within profile with independent native fixtures | Exact original-obligation review, profile/macOS acceptance; pins/pure previews/correlated delivery/no retry retained |
| Packaging/default executable/Python retirement | Native-only checkout launcher and installed binary; positive source-package allowlist; final split local validation passes; Python source retained only as development oracle | Local review, explicit profile disposition, macOS and external tap install/upgrade/promotion; source-oracle deletion is a separate gate in RUST_RETIREMENT.md |

The audit baseline contains **78 Python command paths: 27 represented and 51
absent**, plus one native-only history route. The six writers brought it to 33/45;
process integrations brought it to 37/41. Reviewed runtime adds **12 routes**:
five action writers + three checkpoint decisions + send/agent send/down/stop alias.
That milestone reached 49/29. Legacy lifecycle added the final **12 legacy routes**,
reaching 61/17. Group/Observe brought it to 71/7; Session now brings current coverage
to **78 represented / 0 absent**, not full parity or an effort percentage.
Counts include aliases; native-only history is extra. Counts alone do not certify
flags/defaults/profiles. Native `task list` now defaults to human output (`--json`
optional); `action inspect` has pretty
default output and optional compact `--json`. Use the
[retirement checklist](RUST_RETIREMENT.md) to close complete workflow families,
not another reader/SDK micro-checkpoint. Inspection cannot substitute for writer
receipt/publication/confirmation protocols. Current-source assessment remains
separate under an **existing lock**,
not a property inferred from historical replay. Checkpoint/action native Linux
workflow acceptance retains actual transport effects, not notification-free substitutes.
Next review the final split local evidence and obtain explicit profile disposition,
macOS and external Homebrew install/upgrade/promotion results. The **941-ID original-obligation
audit** remains a separate development-oracle removal gate.
Group/Observe, Session and legacy
lifecycle acceptance does not close compatibility or macOS. Native execution readers,
checkpoint inspection/next guidance, local workspace
commands, init, refresh and state import/recovery do not authorize runtime execution
by themselves. Do not invent task remove/complete port work: Python
has neither command. Remaining writers must preserve persistent flock inode/order,
same-directory publication/modes/fsync, receipt-before-mutable-state lookup,
corrupt-authority refusal, prepared-task guards and each family's uncertainty policy
(including task/state exit 2 versus local workspace exit 1). No old
preparation, observation, collected claim or runtime-launch receipt may become a
new execution grant because its implementation language changed.
