# Coven Web: progress and integration handoff

## Current deployment and cleanup, 2026-11-19

This section supersedes the historical worker/reconciliation/deployment records
below. Source now resides in the normal main checkouts:

- `/home/jdawson/Projects/coven`, base `a3f9fc1`, with unstaged/uncommitted feature
  changes and current-main agent-management integration preserved.
- `/home/jdawson/Projects/djinn`, base `88e2042`, with the native reader added while
  preserving the pre-existing attachment/status work. Nothing was committed or
  pushed during reconciliation and rollout.

### Published and running

- `coven-web.service` runs
  `/home/jdawson/.local/libexec/coven-web/f0904e1da389ab1a/bin/coven`.
  Its SHA256 is
  `f0904e1da389ab1a65954dfaeb2e2e38e99fff6a8a37bb5871b15b43c2d80d32`.
- The existing tailnet-only site remains
  `https://bumblebee.tail97624b.ts.net`, proxying only to `127.0.0.1:7341`.
  The Serve route, observer configuration, login policy and two selected fleet
  endpoints were unchanged. Best-effort terminal output remains disabled.
- Future public dev Coven launches select `coven-native-y6fzrkjz`; the old
  `coven-native-agc3h45d` payload is retained. The public wrapper and Arcana registry
  were unchanged; the existing installer atomically switched its dev prefix.
- Future Djinn UI launches select compiled bundle
  `2b75443906a85635e8e28bf0bb4a915f1e952ca012a204c0678c409768aa027e`
  under `~/.local/libexec/arcana-dev/djinn/bin/.djinn/`. The CLI bytes are unchanged.
  The previous `9ac92fd0...` bundle is retained. No existing native agent/UI/Coven
  process was restarted or signaled; only the web gateway was restarted.

**Conversation activation is still explicit.** The new website currently returns
an empty authorized Conversations list: no native target was guessed or registered.
Existing UI processes were started without `DJINN_PROMPT_SOCKET` and cannot acquire
one from binary installation alone. Start a new full TUI with the published UI and
a private socket, obtain its runtime UUID, and deliberately register the exact root
session and allowed logins using [the configuration contract](COVEN_WEB.md). Do not
infer a session from a pane/PID/title or silently follow a runtime replacement.

### Validation of reconciled main

- 39 Node and 273 focused Rust checks passed, including current-main observations
  and presentation regressions, isolation/lifecycle/parity, install/package, and
  the separately invoked real-producer HTTP fixture. Cargo all-target check and
  formatting passed.
- After a two-line lifecycle borrow cleanup, 27 targeted lifecycle/isolation
  checks and the final release build passed. These overlap the earlier scope and
  are not an additional unique-test total. Strict Clippy still reports five
  pre-existing unchanged-code style findings; new lifecycle findings were fixed.
- Djinn: 103 reader/session/instance tests, 85 focused native/attachment tests and
  14 selected-session tests passed; package typecheck and compiled capability
  probes passed. One opt-in full Coven/Kit hosting test was skipped. The existing
  models snapshot and dependencies were reused without dependency installation.
- Isolated candidate, live loopback and actual Serve HTTPS checks passed for page,
  new assets, health, fleet schema, empty Conversations list, unknown reference
  refusal and identity admission. This is not phone/cellular or real-chat acceptance.
- Owned test/build directories and children were cleaned. Full regression and
  physical-device/provider-model acceptance remain separate gates.

### Rollback and removed worktrees

The original unit is retained byte-for-byte at
`~/.config/systemd/user/coven-web.service.rollback-920bef44`, alongside its old
`920bef44922edaf6` payload. Restore only that unit's content, reload the user manager
and restart only `coven-web.service` if rollback is needed. Keep the Serve route and
live agents unchanged. Native/dev selection rollback must preserve the matched
payload and should not terminate existing sessions.

All three effort-owned **local** worktrees were removed normally after source and
index/untracked preservation checks. Named Git recovery stashes remain until the
integrated work is committed and recovery is no longer needed:

| Removed worktree | Repository | Recovery stash object |
| --- | --- | --- |
| `coven-web` | Coven | `50f5b8645b89ca217234c55072c442a2766c2630` |
| `coven-team-isolation` | Coven | `f4ff432c2c614b07b4d60fc60304b54661f5a1b2` |
| `djinn-conversation-view` | Djinn | `6917b18bf4192dd4fe055e55bbf4b3c3469b0ad2` |

Do not pop/drop unrelated stashes; later recovery-snapshot cleanup must identify
only these exact entries after integrated source is committed. The unrelated
`coven-rust-migration`, `coven-worker-handoffs`, both main checkouts, and the remote
Megatron worktree were retained. No branch deletion or forced removal was used.

## Main source reconciliation overlay, 2026-11-19

The complete Coven prototype, web/conversation and agent-directory implementation
has now been reconciled as **unstaged, uncommitted source changes** into
`/home/jdawson/Projects/coven`, on `main` at `a3f9fc1`. Selection used the combined
`coven-web` working tree's delta against `a541e9f`, including prototype commit
`323dbc9` content and all 11 untracked implementation/test/doc files, rather than
copying its older integration files over current main.

Current-main agent-management, native integration and normal-workflow changes are
retained. The closed `PaneRead` method and its transport test were added alongside
the newer guarded-view transport and tests; module declarations were combined.
The local implementation and validation records below describe the earlier worker
baselines, **not test or activation results against this reconciled main**.

Both `/home/jdawson/Projects/coven-web` and
`/home/jdawson/Projects/coven-team-isolation` remain preserved and untouched,
including the former's pending no-commit merge/index. No staging, commit, build,
rollout, live configuration change or worktree deletion was performed during this
reconciliation. Build/test/rollout verification remains with the integration owner;
retain the source worktrees until safe integration has been verified.

Source-only checks on this overlay passed: all 55 files other than this handoff and
the two socket merges match the selected source exactly, including module
declarations. Both socket deltas match the prototype additions exactly, and all 19 main-only files
changed since `a541e9f` remain equal to `a3f9fc1`. Rust formatting, JavaScript and
Python syntax, and diff whitespace checks passed. These are not build, runtime,
test-suite or rollout acceptance results.

## Local implementation overlay, 2026-11-18

The deployment/source-location sections below record the earlier prototype handoff.
That prototype was subsequently preserved on `coven-web-observer` in commit
`323dbc95ccb86f1c49b070f6735a40a3cdd679dd` and pushed. Its original Megatron
worktree was retained. The following new implementation is **local, uncommitted,
and not deployed**:

- `/home/jdawson/Projects/coven-web`: combined Coven implementation. The saved
  prototype has a pending, conflict-resolved **no-commit merge** of committed
  `main` at `a541e9f`, plus the new web and team-directory changes. Automatic
  merge results are staged; new implementation edits include unstaged and untracked
  files. Review both the index and working tree, including new files. No merge
  commit or additional push has been made.
- `/home/jdawson/Projects/coven-team-isolation`, branch `work/team-isolation`:
  retained isolation worker source. Its changes were copied into the combined
  worktree while preserving the web modules.
- `/home/jdawson/Projects/djinn-conversation-view`, branch
  `work/native-conversation-view`: the companion native reader and tests, based on
  committed Djinn `main` at `88e2042`.

The original Coven and Djinn main worktrees have ongoing unrelated, uncommitted
integration changes. They were not edited or copied into these worker worktrees.
**The combined checks cover the named committed baselines plus this implementation,
not those evolving main-worktree edits.** Reconcile those changes before preparing
an installed rollout; do not overwrite either main worktree.

### Delivered first slice

1. [Agent directory isolation](AGENT_DIRECTORY_ISOLATION.md): explicit full-roster
   mapping to distinct, existing Git working-tree roots for legacy team launches.
   Coordination state stays in the control directory. No automatic worktree
   creation/deletion, relocation of running panes, security sandbox or scheduler
   is claimed. Reviewed `runtime_launch` v1 explicitly refuses this new mapping.
2. [Native conversation viewing](COVEN_WEB.md): explicit local socket/runtime/session
   registrations with per-resource Serve-login authorization. A separate
   Conversations list opens bounded, inert user/assistant text. No guessed binding
   from process/pane cards, terminal-output fallback, prompt submission or decision
   response was added. Remote native conversation endpoints remain unsupported.
3. Djinn's opt-in full-TUI private endpoint advertises
   `djinn.conversation-read.v1`. It reads already-active native contexts, limits
   persisted-record acquisition before materialization, and returns at most 20
   messages / 8192 UTF-8 text bytes. Its digest identifies the returned projection,
   not a transactional whole-conversation revision.

### Validation and operational boundary

- Combined Coven: native-conversation units (5), observer units (44), HTTP gateway
  tests (6), isolation acceptance, selected legacy-lifecycle/runtime-launch/refresh
  parity workflows, native install (9) and native package (2) checks passed.
- The separately invoked, default-ignored real Djinn socket producer through the
  authenticated HTTP gateway passed. Its messages are synthetic; it is not live
  native DB/TUI, provider/model, physical browser or tailnet acceptance.
- Final Node suite: 39 passed, including authorization invalidation, metadata
  expiry, deferred responses and keyboard-focus preservation.
- Djinn: 103 focused tests and its package typecheck passed. Independent native,
  isolation and web reviews were performed; identified correctness issues were
  fixed with regression coverage.
- Combined all-target Cargo check, formatting and whitespace checks passed. The
  full regression gate and a fresh strict Clippy run were not performed.
- Owned test/build/fixture directories were cleaned after child shutdown; unrelated
  run directories and shared package caches were preserved.

No installed executable, provider configuration, live agent, systemd unit or
Tailscale Serve route was changed. Activation needs separately reviewed builds,
explicit native endpoint startup, authorized target registration, and a deliberate
gateway rollout. Existing sessions are not retrofitted or restarted automatically.

## Decision and immediate coordination boundary

Coven Web is **Coven's browser client**, not a separate product, repository or
agent-management authority. Coven owns agent identity, inventory, qualified
observations, attention and supported actions. Kitsune is becoming a generic
multiplexer; Djinn/provider integrations supply their qualified native facts.

Bumblebee is actively integrating Djinn, Kitsune and Coven. **Do not expand the
prototype's backend independently of that work.** Its legacy metadata and process
evidence adapters are transitional, not the final canonical agent inventory.
Read the active `AGENT_MANAGEMENT_MIGRATION.md`,
`KITSUNE_RETIREMENT_FEATURE_DECISIONS.md` and `NORMAL_AGENT_WORKFLOW.md` before
deciding what to incorporate. This document records progress; it does not assign
parallel tasks, merge code, or override the migration decisions.

## Source location: important

The implementation is on **Megatron**, not merged into Bumblebee's checkout:

```text
~/Projects/coven-web
branch: coven-web-observer
base:   ec2d227e308f4558bada1c7099d78ff693fb0cc9
```

This is an isolated Git worktree of Coven, not another repository. Changes are
**uncommitted and unpushed**, including untracked client, observer and test files.
`git diff` alone will not capture those new files. Preserve the worktree until a
reviewed transfer is verified; do not copy it over the active integration tree,
reset/clean that tree, or assume an available commit to cherry-pick.

Only this handoff document is being added to Bumblebee's source tree. Its existing
integration edits under `docs/` and `tools/normal-workflow-check/` are untouched.
The deployed executable is a separate immutable payload, not the active Coven
launcher or a source checkout.

## What exists and is running

- Mobile-first cards, host filter, observation ages, stale/unavailable/unsupported
  states, and qualified evidence labels. No analytics, offline cache or transcript
  persistence. Static ES modules; no frontend production server or build dependency.
- A read-only Coven HTTP facade, co-serving the client on loopback behind private
  Tailscale Serve HTTPS. No Vercel, public Funnel, router forwarding or new runner.
- Two explicitly selected existing Kitsune endpoints: Bumblebee's dev default and
  Megatron's stable default. Identical version labels do not make these interchangeable.
- No agent/runtime start, stop, attach, resize, handoff or provider upgrade was
  required. Public Coven launchers and installed agent binaries were not replaced.

Deployment on Bumblebee:

```text
unit:    coven-web.service                 (user service, active and enabled)
config:  ~/.config/coven-web/observer.json  (0600)
listen:  127.0.0.1:7341 only
payload: ~/.local/libexec/coven-web/920bef44922edaf6/coven
```

The same scoped executable is installed on Megatron for bounded SSH reads.
Full artifact SHA256:
`920bef44922edaf6618df449fecf815ae282bbdba8a615ad41a9901581ea3ddc`.
Its manifest records the source base and uncommitted-build status. Retired
prototype units, configuration, payloads and owned cache artifacts were removed
after rename acceptance; they are not rollback artifacts still available today.

Use `tailscale serve status` on Bumblebee for the private URL. The HTTPS listener
proxies port 443 to loopback 7341. Existing SSH permissions remain separate.
At this handoff check, the real HTTPS API returned both endpoints reachable and
one recognized entry on each. **Counts are dynamic, not acceptance constants.**

## Prototype contract and limitations

Contract: `coven.fleet-observer.v1`.

```text
GET /api/v1/fleet
GET /api/v1/agents/<opaque-reference>/output
GET /api/v1/health
```

Only fixed assets and these routes are served. The browser cannot select socket
paths, SSH commands, RPC methods or arbitrary terminal locators. Read transport
permits only `session.snapshot`, `pane.process_info` and eligible `pane.read`.
Helpers accept a closed bounded inventory/output request over stdin; they never
start missing runtimes. Helper and gateway schema versions must match.
Upstream read dispatch can still perform housekeeping; read-only here means no
deliberate control operations, not a guarantee of zero internal runtime effects.

Current inventory combines Kitsune agent metadata with Coven's existing fenced
process-discovery adapter. The empty Kitsune agent list is legitimate during
retirement, not proof of no running agents. A prior Bumblebee sample had six
panes and no agent metadata; process discovery recognized two Djinn-family
targets and left four terminals unclassified. The adapter now handles that case.

- Process evidence supplies family hints only: activity is **unknown**, output is
  unsupported, and it establishes no native identity, request or completion truth.
- Metadata-backed status is explicitly heuristic. Idle is not success, and blocked
  is not an authoritative approval request. No title-based classifier was added.
- Coverage is incomplete even after successful discovery. Unsupported capability
  differs from a failed refresh; failures retain stale observations, not fresh emptiness.
- References currently describe endpoint-scoped terminal observations. They are
  **not canonical Coven agent identities**. A pane should become an optional
  attachment when the client moves to the canonical agent-management contract.
- Stable references survive reorder, other terminal births/removals and layout
  changes; disappearance/reappearance, expiry, replacement and failures invalidate them.

Recent-output infrastructure exists, but deployed `allow_best_effort_output` is
**false**. Current upstream pane reads lack an atomic expected-terminal/incarnation
precondition and reliable output revision. Matching before/read/after snapshots
cannot eliminate ABA races or attest output freshness. Do not enable this merely
to complete a UI demo; obtain explicit acceptance or use a stronger supported
contract. There is no prompt composer, submission, approval, native notification
or generic keyboard-injection feature in this slice.

Authentication requires the exact configured Host/Origin, allowed Serve login and
a loopback peer. Serve strips/replaces network-supplied identity headers. This
explicitly trusts Bumblebee's local execution environment: a local malicious
process can forge proxy headers. The web facade is narrow, but its upstream
account still has privileged socket access. Do not call this multi-user isolation.
Output is not logged, prefetched, cached or rendered as HTML/terminal instructions.

## Changes to review by ownership

| Area in Megatron's worktree | Responsibility / coordination |
| --- | --- |
| `clients/web/` | HTML/CSS/JS, Node tests, mobile presentation; can proceed against fixtures. |
| `src/observer/` | Prototype configuration, cache, adapters, HTTP/auth and bounded transport; Coven integration owner should review/adapt. |
| `src/agent_discovery.rs`, `src/agent_discovery/schema.rs` | Shared fenced acquisition extraction and explicit caller pane limits; shared-core hotspot. |
| `src/observation_socket.rs` and tests | Closed `PaneRead` addition; not arbitrary RPC. |
| `src/observation_wire.rs` | Separately bounded terminal-text decoding; ordinary wire limits unchanged. |
| `src/main.rs`, `src/lib.rs` | `coven observer check/serve/probe` registration; not the final management API design. |
| Cargo files, `justfile`, package/install tests | HTTP dependencies, embedded assets, `test-web`, packaging coverage. |
| `docs/COVEN_WEB.md` | Detailed prototype schema, trust boundaries, limits and operations. |

The HTTP facade uses Hyper/Tokio with connection/header deadlines, no request-body
aggregation, bounded connection/output concurrency and cancellation-safe output
permits. Inventory has priority over repeated output. Existing discovery keeps
its 64-pane CLI limit; the observer uses an explicit 1024-pane ceiling under one
five-second / 4 MiB acquisition budget. Do not silently widen these boundaries.

## Verification: what passed and what did not

After the inventory correction: 42 observer Rust tests, 20 discovery workflow and
33 discovery presentation tests, four real HTTP/CLI tests, nine native-install and
two packaging tests passed. After the naming/path migration: 42 observer tests,
four HTTP/CLI, nine install, two package and **26 Node tests** passed, as did
all-target Clippy, formatting and whitespace checks. Protected discovery behavior
was not changed by the naming migration.

Real HTTPS checks exercised page, assets, API and Serve identity forwarding.
An isolated browser at 390px/320px verified current branding, inventory parity,
modal behavior, output remaining unavailable and no browser persistence. Synthetic
browser checks separately covered hostile output as inert text. The user has
confirmed page visibility; this is not complete actual-Pixel/background acceptance.

**The full regression gate is incomplete.** An earlier broad Rust run passed
1,348 library tests (seven ignored), 100 CLI tests and multiple integration
binaries, then hit a ten-minute command timeout during an existing execution-
inspection parity test. No completed test reported failure, but do not describe
this as a passing full gate. No commits, pushes, PRs or release occurred.

## Suggested next decisions, not work already assigned

1. Integration owner chooses the canonical Coven inventory/observation/attention
   interfaces and qualification scope; do not freeze this prototype as that contract.
2. Agree versioned fixtures for identity, optional hosting attachments, evidence,
   leases, capability refusal, incomplete coverage and access restrictions.
3. Separate client-only changes from shared Coven/backend changes before transfer.
   Use a dedicated worktree based on the active integration branch, not an overwrite.
4. One owner handles the API/schema/adapters and cross-stack acceptance. UI work
   can parallelize against fixtures without editing discovery/provider internals.
5. Verify actual phone/cellular, hidden/resume, stale/offline recovery, saved policy
   tests, expiry/revocation and useful card targeting before calling v1 accepted.
6. Design message/approval controls only through qualified Coven authority. Never
   translate a generic browser approval into blind Enter/key input.

## Read-only operator checks and recovery boundary

```sh
systemctl --user status coven-web.service
journalctl --user -u coven-web.service
tailscale serve status
```

Do not reset Serve configuration, restart agent runtimes or enable output as part
of documentation/UI work. Stop/disable only `coven-web.service` if rollback is
authorized; remove only its own HTTPS listener if exposure is being withdrawn.
Lingering remains off: operation after logout or before user login following
encrypted boot is not guaranteed. The browser is unavailable if Bumblebee is off.
The current model is one private gateway, not high availability.

This handoff contains no private keys, passwords, auth links or backend credentials.
The operator configuration remains private on its host. Recheck live state and
the active integration diff before starting new work.
