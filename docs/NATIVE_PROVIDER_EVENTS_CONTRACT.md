# Native Provider Activity Assets

`coven agent native-extensions plan|install|uninstall|rollback --provider pi|opencode
--extension-root ABS (--from-kitsune | --socket ABS) [--executable ABS]
[--state-root ROOT] [--migrate-kitsune] [--apply] [--json]` explicitly places
assets in one existing, caller-selected trusted extension/plugin directory.
Plan always previews; all other operations preview without `--apply`.
Plan/install default the executable to this Coven executable, never PATH lookup.
`--state-root` belongs only to `--from-kitsune`; generated argv uses the existing
versioned integration route pin and resolves the active receiver per report.

No provider launch, credentials, models, live API calls, automatic activation,
approval changes, provider discovery, or version-based trust is performed.
Use a normal `coven agent run` receiver. An old `agent serve` receiver is not a
fallback for these profiles. Placement is not evidence that a provider loaded it.

## Independent Installation

Pi owns `coven-native-events.ts`, `kitsune-agent-state.ts`,
`coven-pi-native.json`, `.coven-pi-native.lock`, and
`.coven-pi-native-recovery`. OpenCode uses the corresponding `.js` leaves and
`coven-opencode-native` receipt/lock/recovery namespace. Receipts use the existing
bounded, descriptor-relative, two-leaf journal. Their generated-image validation
is separate from all five historical v1 provider installers.

Only exact frozen Kitsune bytes/fingerprints are eligible for migration, and
only with explicit `--migrate-kitsune` consent. Uninstall removes unchanged
managed assets; rollback restores the original pair, including a migrated
Kitsune asset. Edited/unsafe/conflicting leaves and pending recovery fail closed.
Existing engine migration text is historical reference-installer language, not
a statement of this native profile's event capabilities.

`coven-agent-reference.ts/js` and historical receipts are independent unknown
files to this policy and remain untouched. Coexistence may produce a separate
reference-only unknown observation. Prefer the old `agent extensions uninstall`
guarded workflow, with its original pins, before explicitly installing the
native asset. Never silently overwrite an old reference asset or its receipt.

## Closed Wire

Each factory allocates one UUID and increasing positive safe-integer sequence.
The only stdin object is `contract`, `provider`, `profile`, `instance_id`,
`session_id`, `sequence`, `event`, plus `status` or `request_id` when required.
Contract is `coven.provider-events.v1`. Identifiers are nonblank, control-free,
valid Unicode and at most 512 UTF-8 bytes. IDs stay private to this envelope;
prompts, answers, titles, paths, error messages, metadata and raw contexts/events
are never serialized or logged. This is producer-claimed, unauthenticated
observation evidence, not authority or native attachment proof.

The queue holds at most 32 waiting observations and one active child. Each
observation has at most 1900 ms from original callback capture, including queue
time. Spawned children use literal JSON-pinned executable/argv, `shell:false`,
bounded stdin and ignored stdout/stderr. There is no retry, replay, heartbeat or
dynamic status polling. Only child `close` frees its active slot, not `error`,
`exit`, timeout or a kill request. Disposal drops the queue, requests SIGKILL,
and awaits close to reap the owned child; late callbacks are ignored. A child
that never closes cannot trigger another concurrent report.

## OpenCode Qualification

Source qualification is **anomalyco/opencode commit
810c0eba1a7348036f8bdf61e6632d0c36c6876c**, associated with v1.19.5, not all
OpenCode versions and not the local Djinn fork or changed dev APIs.

Verified read-only source URLs:

- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/opencode/src/plugin/index.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/opencode/src/session/status.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/schema/src/session-status-event.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/schema/src/v1/session.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/schema/src/v1/permission.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/schema/src/v1/question.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/opencode/src/question/index.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/plugin/src/index.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/sdk/js/src/gen/sdk.gen.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/sdk/js/src/client.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/sdk/js/src/gen/client/client.gen.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/sdk/js/src/gen/client/types.gen.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/opencode/src/server/routes/instance/httpapi/groups/session.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/opencode/src/server/routes/instance/httpapi/handlers/session.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/opencode/src/server/routes/instance/httpapi/api.ts
- https://raw.githubusercontent.com/anomalyco/opencode/810c0eba1a7348036f8bdf61e6632d0c36c6876c/packages/opencode/src/session/session.ts

The plugin bridge listens to EventV2 events in the exact directory instance,
passes `{id,type,properties:event.data}`, and calls `dispose` on finalization.
Profile `opencode-root.v1` reports only full validated root SessionInfo DTOs
with matching `properties.sessionID === info.id` and absent `parentID`.
Interleaved roots are independent. Known children and deleted sessions stay
excluded for the factory lifetime. Classification retains at most 512 IDs;
unknown IDs at capacity are ignored, never inferred as roots. On-demand lookup
reservations, including failed qualifications, count toward that same bound.

Allowed lifecycle events are `session.created`, `session.updated`,
`session.deleted`. Only qualified roots can emit `session.status` with
exact `busy|retry|idle`, `permission.asked/replied`, or
`question.asked/replied/rejected`. Asked IDs come from `id`; replies/rejection
come from `requestID`, retaining session correlation. Receiver pending state
removes only the matching decision kind/request. Errors are not blocked,
deprecated `session.idle` is not mapped, and cancellation is unsupported.

Resumed sessions do not require a new lifecycle event. An otherwise-valid live
status or decision event for an unknown ID can trigger exactly the pinned
legacy `PluginInput.client.session.get({path:{id},throwOnError:true,signal})`.
This is `GET /session/{id}` with the plugin's directory-aware client, not
session enumeration or a prompt/model request. The server returns full
`Session.Info`; the SDK's default fields response exposes it as `result.data`.
The existing full `root(info,id)` validation must succeed before silently
registering a root. Metadata emits no lifecycle report and consumes no sequence.
Missing SDK support, failed/mismatched/partial DTOs, or children fail closed.

No SDK call occurs during plugin/module initialization. Native status/decision
required fields are checked before admission; malformed events and unsupported
events/cancellation never occupy a lookup slot. Only closed event fields are
retained, never raw callbacks, prompts, answers or metadata DTOs. Native schema
arrays use the existing conservative 256-item validation limit.

One active lookup and at most 32 pending snapshots preserve global live-event
FIFO, including interleaved roots and ask/reply ordering. Same-session events
share the active qualification; a failed lookup drops its queued cohort rather
than retrying those observations. A later new valid callback may try again.
The emitter's separate 32-item queue and single child retain their own bounds.
Each snapshot keeps its original capture time across lookup, validation and
emitter queuing; the entire report budget remains 1900 ms. An abort timer plus
`Promise.race` releases the logical lookup slot even if an SDK ignores abort.
Late resolution cannot register a root or emit a report after timeout,
deletion, child exclusion or disposal. Abort cannot force a non-cooperative
external SDK to finish its underlying IO; no retry is made for that snapshot.

Deletion invalidates pending qualification immediately, even if its DTO is too
incomplete to report deletion. Children and tombstones remain sticky. A valid
full lifecycle DTO can supersede a pending lookup without stale metadata
overwriting classification. Disposal cancels lookup work and drops pending
snapshots before reaping reporter children. OpenCode's directory plugin has no
selected-session switch event: interleaved sessions remain independent, and a
replacement plugin factory inherits no classification or pending work.

This closes the new native profile's avoidable status-first resumed-root gap;
the old reference assets, generators and journals remain frozen. Shared
adoption/migration documentation is coordinator-owned and is not updated here.

## Pi Qualification

Source qualification is **earendil-works/pi commit
6e11a00a01a568ef0ae0df5074acf02fc107c34b** (package version 1.5.2 is not a
release-tag pin):

- https://raw.githubusercontent.com/earendil-works/pi/6e11a00a01a568ef0ae0df5074acf02fc107c34b/packages/coding-agent/src/core/extensions/types.ts
- https://raw.githubusercontent.com/earendil-works/pi/6e11a00a01a568ef0ae0df5074acf02fc107c34b/packages/coding-agent/src/core/session-manager.ts

Profile `pi-context.v1` requires `ctx.hasUI === true` and `ctx.mode` exactly
`tui|rpc`, with an ID from `ctx.sessionManager.getSessionId()`. The pinned
`ReadonlySessionManager` explicitly picks `getSessionId` from `SessionManager`;
there is no uppercase-`ID` alias.
`session_start` emits metadata-only `session.updated`; `agent_start` emits
busy; only `agent_settled` with `ctx.isIdle() === true` emits idle. Missing,
throwing, false or nonboolean idle predicates do not emit idle. `agent_end`
does not prove settlement. JSON/print/headless contexts do not report.

`session_shutdown` cleans queued activity and kills/reaps the child, never
reports deletion. Session replacement clears prior-session pending reports;
quit/reload permanently stop that factory. No producer-checked root claim,
custom `kitsune:blocked` bus, raw UI prompts, UI decision correlation,
completion event, retry state, permission events or question events is claimed.
Pi UI prompt events have no request IDs suitable for this contract.

## Validation Limits

Inert callback/spawn harnesses exercise generated assets without launching
providers, including a real module import of the unmodified generated Pi asset
and positive FIFO delivery on child close before expiry. Pi fixtures expose
only the source-verified `getSessionId` spelling and positively assert metadata,
busy and idle for both TUI and RPC; print/JSON produce no reports.

The receiver workflow imports byte-identical generated assets as `.mjs`, invokes
mock native callbacks, and uses their actual subprocesses to run the real
`report-native` CLI against an actual `agent run` normal receiver. Observations
and a fake Kitsune integration-view endpoint verify redacted Pi metadata,
busy/idle and status-first resumed OpenCode busy/blocked/idle, including matching
decision replies. OpenCode metadata is supplied by a source-faithful mock of the
legacy SDK `session.get`, not a live native API. Deterministic generated-asset
tests also cover lookup deduplication, FIFO, limits, abort-ignoring timeouts,
invalid DTOs, deletion and disposal races, and original observation age.
No imports or exports are rewritten in this roundtrip. Owned reporter children,
receiver and fake endpoint are stopped/joined before fixture cleanup.

Offline install/preview/uninstall/rollback tests use owned temporary roots, not
user provider roots. Source qualification and these tests are not live Pi or
OpenCode loading/activation qualification. Native macOS file mutation remains
provisional under the existing engine contract. No speculative Codex/Copilot
native activity semantics are implemented; their historical assets are frozen.
