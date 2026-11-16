# Kitsune retirement: remaining feature decisions

## Scope and status

Reviewed 2026-11-15. The user approved adoption A1-A5. This backlog contains only
remaining residuals, not all approved work. Retirement may proceed independently of
feature parity; useful outcomes can move to Coven while unwanted behaviors retire.

Only user-facing residuals **not already provided by Coven** are listed. For a
partially implemented capability, the entry names only the missing portion.
Generic multiplexer features staying in Kitsune are not Coven adoption work.
Remove or narrow entries when Coven ships them; do not keep completed work in a
permanent "missing features" backlog.

## Remaining Approved Adoption Work

- **A2: Other-client/runtime and remote-hosting qualification.** The qualified
  normal local Djinn full-TUI/default-worker/implicit-local selected-root producer
  is delivered, not remaining work. Other clients/runtimes, mini/external attach,
  remote/explicit workspaces and HTTP transports need their own source/lifetime
  qualification. No PID/newest-SID inference, automatic rebind or execution
  authority. See [attachment scope](NATIVE_ATTACHMENT_CONTRACT.md).

These attachment modes are not merely missing test runs. The current Djinn
producer deliberately excludes them; support requires an additional producer or
routing implementation, followed by qualification. "Explicit workspace" here
means Djinn's native workspace-placement/routing identifier, not ordinary project
directories, Coven control workspaces, or Kitsune workspaces/tabs.

## Accepted Provider Limitations — Deferred

The user accepts current Codex, Copilot and Pi limitations and wants richer/full
support revisited if those providers enter the actual workflow. This work is
deferred, not a blocker for the current Djinn workflow or Kitsune retirement.

- **A3: Current-state and correlated-decision coverage beyond unordered facts.**
  Codex/Copilot unordered native hook facts are delivered; current working/idle/
  blocked/ended state and paired decisions are not. Pi paired UI decisions remain
  NO-GO at its source pin: fire-and-forget async delivery lacks ordering guarantees
  and immutable occurrence/session IDs are missing. Requalification needs actual
  native guarantees, not invented IDs, tool-name pairing or a whitelist change.
  No screen guessing or inferred completion. See [hook facts](NATIVE_HOOK_FACTS_CONTRACT.md)
  and [native profile limits](NATIVE_PROVIDER_EVENTS_CONTRACT.md).

## Operational Follow-Up

Managed native-hooks configuration (install/uninstall/rollback) is not implemented.
Today's deterministic `agent native-hooks plan` snippets require deliberate manual
review/merge with exact-image reference-journal ownership respected. This is a
configuration follow-up, not missing Codex/Copilot fact-capture capability. Native
approvals, credentials and provider configuration are never auto-edited.
It is not required for the current Djinn workflow; revisit it with provider
adoption rather than expanding that scope now.

## Delivered Implementation Record

A1's 16 explicit interactive adapters, A4 attention/acknowledgement/checked CLI
focus, and bounded A5 selected recovery are delivered, not missing capabilities.
A2 qualified local Djinn producer attachment and unchanged temporary asserted
navigation, plus A3 independent Pi/OpenCode profiles and unordered Codex/Copilot
native hook facts, are also delivered within their stated scopes. See the separate
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

The [next Kitsune control-surface cut](../../kitsune/AGENT_CONTROL_RETIREMENT.md)
refuses prompt/key input, rename, focus, legacy view mutation and effectful get
reconciliation under external ownership. Wait and prompt-with-wait stop at their
initial guarded get. Generic terminal controls and historical data remain; the
standalone implementation and transitional list/read/explain facades are not
deleted yet. This does not expand the deferred provider-support scope.

The subsequent read/detector cut refuses the live list/read/explain facades and
manifest status/reload APIs externally. Generic snapshots still succeed with an
empty agent inventory and intact historical pane references. Background and CLI
updater admission stops before detector I/O, and queued updates cannot reset
runtimes. Explicit offline file explanation and standalone implementations remain
until their source-removal cut; they are not reasons to resume richer providers.

Subsequent cuts should retire agent-specific producers and consumers together.
Keep generic PTY/process/terminal/session operations and integration transport in
Kitsune. Preserve old snapshot readers and required migration/uninstall material
separately from execution policy. Return explicit compatibility errors for retired
operations; helper loss must never revive legacy authority.

The target remains a general multiplexer with no agent-aware features when Coven
is absent. Documented feature discontinuation is valid; exact parity must not keep
the legacy implementation indefinitely.
