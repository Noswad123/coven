# Coven-owned group view in Kitsune

## Overview

`coven group present` owns its content and selection meaning within Kitsune's
generic desktop `Integrations` block. **Coven supplies the group/participant model;
Kitsune supplies generic rendering and message transport.** No detected-agent
inventory, native session identity, provider status or execution grant is inferred.

This is distinct from `group publish`, which still supplies terminal-border
annotations. Neither command substitutes for the other when unsupported.

## API / Interfaces

First use a compatible running Kitsune build and manually configure sources:

```toml
[integrations]
sources = ["coven", "build"]
```

Preserve existing configuration; Coven does not edit/reload/restart/install
Kitsune. Source IDs are logical routing, not authentication: at most eight ordered,
unique ASCII IDs of 1–64 bytes (`A–Z`, `a–z`, digits, `_`, `.` or `-`). The user
locally selects an app from a vertical list; the rows below are that app's generic
label/detail text, with no hard-coded Coven/agent semantics. An absent/expired app
shows `Unavailable`, a new lease `Waiting for data`, and an empty frame `No items`.
Empty config shows `No integrations configured`; there is never native-agent
fallback. Other legacy agent APIs/detection, mobile surfaces, restore and hooks
remain during transition. Legacy `integration.install/uninstall` and fixed
`IntegrationTarget` configure compatibility agent hooks, not this provider list.

The obsolete `[external_view]` config is ignored with a diagnostic directing users
to `integrations.sources`, never rewritten automatically. Both repositories use
the new contract together; no published compatibility with that local prototype
is required.

Use an **existing** Coven workspace/group; do not initialize over an application
repository. For declaration/binding setup see [GROUPS_CONTRACT.md](GROUPS_CONTRACT.md).

```text
coven group present WORKSPACE GROUP --socket ABS_SOCKET --source coven [--duration 60] [--json]
```

Workspace, group, socket and source are explicit. Duration is 1–3600 seconds.
Output is pretty ASCII JSON by default, compact with `--json`, with one terminal LF.
The library exports `group_presenter::{Request, arguments}`.

## Data Flow

1. Read Coven's declaration store and freeze its revision. Display one group header
   and at most 127 participant rows from that store—not Kitsune's agent hints.
2. Pin the explicitly selected endpoint, discover protocol 18 and
   `snapshot.integration_views.version == 1`. Validate the complete ordered
   `sources: [{id, epoch}, ...]` list (including entries after the target) and find
   the requested source anywhere, not only first. Each inactive epoch is null;
   discovery exposes no lease tokens and is independent of local UI selection.
3. Open only the requested source's five-second lease, publish a full frame, then
   poll about every 250 ms. `--source coven` does not own/replace the whole block:
   `build` and other producers coexist with independent leases/frames/revisions/queues.
4. Accept only the current epoch/revision and known participant IDs. Re-read the
   local store before publication and after polling. Unknown/stale selections are
   ignored; a store revision change stops the run rather than adopting new state.
5. A valid participant selection publishes a newer frame with read-only details.
   The group header is explanatory; selecting it has no participant-details effect.
6. Duration completion/Ctrl-C attempts one graceful close within the remaining
   budget. Crashes or uncertain delivery rely on lease expiry.

Participant rows show declared keys/roles. Selection does not bind/adopt a pane,
change membership or roles, focus a terminal, read a native artifact, submit input,
launch/resume/cancel a provider, or change local authority files. Kitsune owns
palette, layout, clipping and scrolling; no app-defined scripts/widgets are sent.

The transport uses only `integration.view.open`, `integration.view.replace`,
`integration.view.poll` and `integration.view.close`; there are no aliases for
the obsolete unreleased `view.*` methods. Responses retain `view_opened`,
`view_replaced`, `view_intents` and `view_closed` result types. Existing observation
read allowlists and terminal submission/admission contracts are unchanged. Endpoint
replacement, incompatible discovery and malformed/uncorrelated replies stop the
run; there is no annotation or native-control fallback.

## Edge Cases

See [Kitsune's generic contract](../kitsune/INTEGRATION_VIEWS.md) for exact fields and
bounds. Source/lease/epoch are correlation, not authentication or permission.
Intents are bounded ephemeral `{epoch, revision, item_id}` messages, not commands.

Successful replace/poll alone renews an opened lease; clicks, rejected calls and
local UI selection do not. Close/expiry/removal affects only that provider.
Config reorder preserves retained leases, frames, queues, selection and scroll;
removing another source does not reset Coven. Quiet timers handle expiry without
terminal activity, not with hard-real-time pixel guarantees.

Per provider: 128 items, unique nonblank IDs up to 128 UTF-8 bytes, nonblank labels
up to 160 bytes, details up to 512 bytes, 96 KiB total raw text, 32 queued intents.
Eight providers allow aggregate maxima of 1,024 items, 768 KiB and 256 intents.
IDs may coincide across providers without sharing state. Internal activation routes
the source explicitly; the intent payload stays unchanged. A successful local
draw/server frame queue is not physical client-pixel acknowledgement or permission
to execute. No broker, plugin or scripts are involved.

Poll drains at most once. Lost replies can lose selections; no automatic resend,
reopen or reconnect replay is attempted. A possibly admitted frame or drained poll
is reported uncertain; the lease is left to expire rather than refreshed/retried.
Slow calls may cause server lease rejection even if each fits its three-second
RPC budget. This is fail-closed inspection, not guaranteed delivery or availability.

The report uses `schema_version: 1`, `contract: coven.group-presentation.v1`,
`delivery: ephemeral_ui_only`, `authorization: none`, and `dispatchable: false`.
It reports frame/poll/selection counts, ignored intents, outcome and cleanup state.
Completion exits 0, interruption without delivery uncertainty 130, and operational
failure/uncertainty 2 (including an interruption after a possibly admitted write);
invalid domain arguments use ordinary fixed errors. Peer error text is not echoed.
Local display labels and participant IDs may be sensitive; review before sharing
reports/screenshots. No raw native reference/binding contents are projected.

## Example usage

With the source configured and an existing workspace/group:

```text
coven group present WORKSPACE GROUP --socket ABS_SOCKET --source coven --duration 60 --json
```

Select `coven` locally in the Integrations list, then a participant row to inspect
its declared read-only details. Selecting another app does not stop Coven's lease
or change its management model. This is separate from border annotation priority.

## Validation and next ownership work

Private-socket workflows exercise the real CLI and loss/stale/malformed/endpoint
cases without live apps. An explicit developer test also connects the real Coven
binary to actual Kitsune handlers on a private fixture socket and `TestBackend`,
selects the source header and participant row, and verifies Coven's newer details
frame and close. An independent `build` provider comes first with the same item ID;
its lease/frame/queue remain isolated and survive Coven's close. The workspace files
and native terminal state remain unchanged. This is a fixture integration result,
not an installed-server or cross-platform certification.

Current validation passes **48 focused Coven checks** across presenter workflows,
legacy-method rejection, CLI compatibility, interactive/comparison regressions and
package inventory. Strict all-target Clippy, Rust 1.85 checking, optimized build
and source-package verification pass. Kitsune separately passes 509 focused checks,
strict lint and its normal build; the explicit real-Coven/two-provider roundtrip
also passes. That test is ignored by default and requires explicit
`COVEN_TEST_BIN`, with no sibling/PATH fallback:

```sh
COVEN_TEST_BIN=/absolute/path/to/coven cargo test --offline --locked --bin kitsune app::api::integration_views_roundtrip_tests::integration_views_real_coven_roundtrip -- --exact --ignored
```

Run that recipe from Kitsune with its selected project toolchains. It uses a
direct-handler socket proxy and coordinate helper, not the full production API
server or physical input. Full Kitsune `just check` remains unapproved/not run;
Windows/macOS are not validated.

Next work should extend **Coven's management model and runtime adapters** behind
this generic boundary, then retire matching legacy Kitsune management surfaces.
Do not add Coven-specific agent fields/actions to Kitsune or make more
characterization the sole product milestone. Legacy native restore/failure tests
remain removal gates for those subsystems, not prerequisites for every independent
presentation increment. Future effectful actions require separately specified
Coven authorization and native runtime support; a menu click is not that grant.

## Related files

- [src/group_presenter.rs](src/group_presenter.rs) — request, declaration projection and foreground loop.
- [src/group_presenter/wire.rs](src/group_presenter/wire.rs) — whole-list discovery and response validation.
- [src/observation_socket.rs](src/observation_socket.rs) — explicit method allowlist and endpoint fencing.
- [tests/group_presenter_workflow.rs](tests/group_presenter_workflow.rs) — private-socket workflows.
- [GROUPS_CONTRACT.md](GROUPS_CONTRACT.md), [APP_BOUNDARIES.md](APP_BOUNDARIES.md) — declarations and migration scope.
