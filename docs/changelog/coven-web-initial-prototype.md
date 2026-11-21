# Coven Web initial prototype handoff (before 2026-11-18)

Historical record; the source gives no exact date for this original handoff.
All “current”, “running”, source-location and next-step statements below refer to
that prototype. Source status was superseded by [local implementation](2026-11-18-coven-web-local-implementation.md)
on 2026-11-18 and [reconciled deployment](2026-11-19-coven-web-reconciled-deployment.md)
on 2026-11-19. Later capabilities do not retroactively change its limits.
[History index](index.md).

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

## Initial development check results

The following additional evidence was originally in `docs/COVEN_WEB.md`, under
“HTTP lifecycle and development checks”. It describes initial development, not
the later committed source or latest deployment gate.

The original lifecycle documentation said header reads expired after five seconds
and connections after ten. The later `connection` implementation uses a twenty-second
overall deadline for both reads and mutations; the ten-second figure is an initial
historical limit, not the current contract.

Use the repository `just` recipes for the broad regression gate when available.
`just` was absent in the initial development environment, so equivalent Cargo
commands were used. The full Rust run reached 1,348 passing library tests (seven
ignored), 100 passing CLI tests and multiple passing integration binaries, then
hit a ten-minute command timeout during an existing execution-inspection parity
test. This is incomplete full-suite validation, not a passing full gate. The
equivalent focused checks passed independently at that time. After the client/path
rename, 42 observer tests, four HTTP/CLI tests, nine native-install tests, two
packaging tests and 26 Node tests passed; all-target Clippy, formatting and
whitespace checks passed. This does not complete the full legacy gate.
Native-source packaging tests include
the embedded client assets. Real-DOM mobile-width smoke checks used synthetic
data in an isolated headless browser; they do not establish actual phone access.
