# Kitsune retirement: remaining feature decisions

## Scope and status

Reviewed 2026-11-14. The user approved adoption A1-A5. This backlog contains only
remaining residuals, not all approved work. Retirement may proceed independently of
feature parity; useful outcomes can move to Coven while unwanted behaviors retire.

Only user-facing residuals **not already provided by Coven** are listed. For a
partially implemented capability, the entry names only the missing portion.
Generic multiplexer features staying in Kitsune are not Coven adoption work.
Remove or narrow entries when Coven ships them; do not keep completed work in a
permanent "missing features" backlog.

## Remaining Approved Adoption Work

- **A2: Producer-backed actual native hosting and lifetime qualification.**
  Independently establish that a reported native session is hosted by the exact
  current terminal/runtime, beyond the already-delivered explicit 30-second
  user assertion and its CAS/nonreviving invalidation. No automatic PID/label
  inference or execution authority. See [navigation limits](AGENT_NAVIGATION_CONTRACT.md).
- **A3: Remaining qualified native activity/decision coverage.** Investigate
  supported native activity for Codex/Copilot, which remain reference-only and
  unsupported for activity. Pi decision correlation remains possible work only
  where actual native APIs
  supply suitable request identity; today's APIs do not. No invented blocked bus,
  screen guessing or inferred completion. See [native profile limits](NATIVE_PROVIDER_EVENTS_CONTRACT.md).

## Delivered Implementation Record

A1's 16 explicit interactive adapters, A4 attention/acknowledgement/checked CLI
focus, and bounded A5 selected recovery are delivered, not missing capabilities.
A2 temporary asserted navigation and A3 independent Pi/OpenCode native profiles
are also delivered within their partial scope. See the separate
[A1-A5 status record](FEATURE_ADOPTION_STATUS.md) and its contract links for
commands, durability, skip-only reference vetoes and evidence limits. This does
not mark all adoption fully done or certify installed providers/platforms.

## Deliberately Excluded

- **D1 — Authoritative state from screen patterns or generic terminal activity.**
  Prompts, spinners and text cannot establish native lifecycle, readiness, success
  or permission to send input. Optional diagnostic evidence would need a separate
  non-authoritative contract, not fallback status authority.
  Evidence: Kitsune's [detectors](../../kitsune/src/detect/mod.rs).
- **D2 — "Finished/done" notifications inferred from idle or unseen idle state.**
  Idle can mean waiting, interruption or inactivity. Do not turn it into success
  or a completion sound. Any future notification should describe an actual fact.
  Evidence: Kitsune's [attention logic](../../kitsune/src/app/actions.rs) and
  [mobile status presentation](../../kitsune/src/ui/mobile.rs).
- **D3 — Automatic provider execution merely from restoring a saved layout.**
  Reopening terminal history is not new consent to run an agent. Retire the default
  execution policy; recovery convenience belongs to explicit opt-in A5 instead.
  Keep historical references readable without executing them.
  Evidence: Kitsune's [restore policy](../../kitsune/src/persist/restore.rs).

## Decide later

- **L1 — Compact/mobile attention summaries and terminal selection.** Confirm that
  this is a product requirement. If retained, Coven supplies agent meaning and
  Kitsune only generic compact rendering/routing, not an embedded agent model or
  misleading "done" counts.
  Evidence: Kitsune's [mobile agent consumers](../../kitsune/src/ui/mobile.rs);
  the [normal Coven workflow](NORMAL_AGENT_WORKFLOW.md) lacks this presentation.

Native qualification and the supported OS matrix remain separate in the
[migration contract](AGENT_MANAGEMENT_MIGRATION.md), not extra feature-list entries.

## First retirement cut and remaining boundaries

The first bounded cut rejects legacy `agent.start` under external ownership,
before validation, terminal input or managed-state mutation. It returns
`external_agent_management` even if the Coven helper is unavailable. This closes
competing authority; that cut itself neither implemented A1 nor removed standalone
legacy start. The later Coven A1 adapters are recorded separately above.

Subsequent cuts should retire agent-specific producers and consumers together.
Keep generic PTY/process/terminal/session operations and integration transport in
Kitsune. Preserve old snapshot readers and required migration/uninstall material
separately from execution policy. Return explicit compatibility errors for retired
operations; helper loss must never revive legacy authority.

The target remains a general multiplexer with no agent-aware features when Coven
is absent. Documented feature discontinuation is valid; exact parity must not keep
the legacy implementation indefinitely.
