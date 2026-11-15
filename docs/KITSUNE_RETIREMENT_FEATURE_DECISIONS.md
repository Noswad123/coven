# Kitsune retirement: remaining feature decisions

## Scope and status

Reviewed 2026-11-14. These are **recommendations**, not implemented capabilities or
approved implementation commitments. Retirement may proceed independently of
feature parity; useful outcomes can move to Coven while unwanted behaviors retire.

Only user-facing residuals **not already provided by Coven** are listed. For a
partially implemented capability, the entry names only the missing portion.
Generic multiplexer features staying in Kitsune are not Coven adoption work.
Remove or narrow entries when Coven ships them; do not keep completed work in a
permanent "missing features" backlog.

## Recommend adopting in Coven

- **A1 — Guarded interactive launch/resume adapters beyond Djinn.** Add actual
  provider execution support with explicit consent and unsupported results where
  necessary, not another discovery interface or resume-argv preview.
  Evidence: Kitsune's [`App::start_agent`](../../kitsune/src/app/agents.rs) submits
  provider commands; Coven's [interactive adapter](../src/interactive_sessions.rs)
  restricts executable interactive sessions to Djinn.
- **A2 — Reliable native-session-to-terminal association.** Let users identify the
  current terminal hosting a reported session. Independently establish the mapping
  and invalidate it on replacement, exit or stale evidence. Never treat a label or
  PID hint as authenticated native identity or execution permission.
  Evidence: Kitsune's [agent API](../../kitsune/src/app/api/agents.rs) targets panes
  and checks foreground runtime; Coven's [normal observations](NORMAL_AGENT_WORKFLOW.md)
  remain unbound.
- **A3 — Native activity/decision coverage for reference-only integrations.**
  Investigate genuine events for Codex, Copilot, Pi and OpenCode. Report working,
  waiting or idle only where native interfaces support those facts, with bounded
  freshness and explicit unknown states. No screen-guess fallback.
  Evidence: legacy [Pi](../../kitsune/src/integration/assets/pi/kitsune-agent-state.ts)
  and [OpenCode](../../kitsune/src/integration/assets/opencode/kitsune-agent-state.js)
  report richer activity/decisions; the [current migration contract](AGENT_MANAGEMENT_MIGRATION.md)
  limits these Coven integrations to references. This is not a promise of every
  provider's lifecycle capabilities.
- **A4 — Actionable cross-session attention navigation.** Prioritize waiting
  sessions, acknowledge attention and deliberately jump to their terminals.
  Coven owns interpretation; use generic Kitsune focus/presentation operations.
  Terminal jumps depend on A2. Acknowledgement is not job completion.
  Evidence: Kitsune's [agent view](../../kitsune/src/app/agent_view.rs) orders by
  priority/seen state and its [agent API](../../kitsune/src/app/api/agents.rs)
  focuses targets; Coven's [group-view selections](GROUP_PRESENTATION_CONTRACT.md)
  supply read-only details, not this workflow.
- **A5 — Opt-in restart recovery of selected saved native sessions.** Include
  duplicate suppression, imported-running-session precedence, deferred launches,
  reservation rollback and command-enqueue failure handling. Historical references
  alone never authorize execution; consent belongs to the recovery operation.
  Evidence: Kitsune's [restore](../../kitsune/src/persist/restore.rs) and
  [resume](../../kitsune/src/app/agent_resume.rs) state machines; broader transfer
  remains open in the [migration contract](AGENT_MANAGEMENT_MIGRATION.md).

## Recommend deliberately dropping

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
competing authority; it neither implements A1 nor removes standalone legacy start.

Subsequent cuts should retire agent-specific producers and consumers together.
Keep generic PTY/process/terminal/session operations and integration transport in
Kitsune. Preserve old snapshot readers and required migration/uninstall material
separately from execution policy. Return explicit compatibility errors for retired
operations; helper loss must never revive legacy authority.

The target remains a general multiplexer with no agent-aware features when Coven
is absent. Documented feature discontinuation is valid; exact parity must not keep
the legacy implementation indefinitely.
