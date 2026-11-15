# Approved Feature Adoption Status

Implementation record as of 2026-11-14, not a missing-feature backlog or a claim
that all adoption work is complete. The user approved A1-A5. Remaining product
residuals are tracked in [feature decisions](KITSUNE_RETIREMENT_FEATURE_DECISIONS.md);
platform qualification and legacy retirement remain in the
[migration matrix](AGENT_MANAGEMENT_MIGRATION.md).

| Adoption | Delivered scope | Remaining limits |
| --- | --- | --- |
| A1 | Explicit, preview-first interactive start/resume for all 16 paired source-characterized adapters: djinn, claude, codex, copilot, devin, droid, kimi, mastracode, pi, omp, hermes, opencode, qodercli, kilo, cursor, grok. | Installed CLI/version compatibility is unverified. Enqueued input is not provider startup. No arbitrary extra args, probes, journal or automatic retry in standalone A1. |
| A2, partial | Explicit user-asserted, 30-second volatile association to an exact Kit endpoint, terminal and runtime generation, with CAS and nonreviving invalidation. | Not independently proven native hosting or a fully verified binding. Producer-backed attachment/lifetime qualification remains. No automatic association or PID/label inference. |
| A3, partial | Independent opt-in `agent native-extensions` assets for pinned Pi context activity and OpenCode root-qualified lifecycle/status/decisions, through the normal receiver. Unknown resumed OpenCode roots can be established silently by bounded native metadata lookup. | Codex/Copilot remain reference-only with unsupported native activity. Invalid/unavailable root metadata fails closed; OpenCode cancellation is unsupported. Pi has no root claim or decision correlation. Native loading/activation is not qualified. |
| A4 | Fresh attention lists, occurrence acknowledgements and deliberate checked CLI focus through generic Kit terminal navigation. | Focus uses A2's temporary assertion, not proven hosting. Acknowledgement does not clear decisions/native facts. Generic row clicks remain read-only; expiry means unknown, not done. |
| A5, bounded | Explicit selected-session recovery with whole-batch CAS/capacity, durable before-send intent, duplicate blockers and imported/snapshot-reference skip vetoes. | Per-workspace journal plus one point-in-time endpoint inventory, not global exactly-once. No scheduler, autostart or saved consent; queued input is not native resume proof. |

## Commands And Contracts

These are reference forms; uppercase values must be explicitly selected. Mutations
require the shown opt-in flag on the current invocation, never a saved preview.

```text
coven session start WORKSPACE --runtime RUNTIME --executable ABS_BINARY --socket ABS_KIT_SOCKET --pane PANE --expected-terminal TERMINAL [--execute]
coven session resume WORKSPACE --runtime RUNTIME --executable ABS_BINARY --socket ABS_KIT_SOCKET --pane PANE --expected-terminal TERMINAL --session SELECTOR [--session-kind id|path] [--execute]
coven agent attention list --socket ABS_RECEIVER_SOCKET --json
coven agent attention acknowledge OBS --socket ABS_RECEIVER_SOCKET --expected-epoch E --expected-revision R [--apply]
coven agent associate OBS --socket ABS_RECEIVER_SOCKET --kitsune-socket ABS_KIT_SOCKET --terminal TERM --expected-epoch E [--expected-revision R] [--apply]
coven agent focus OBS --socket ABS_RECEIVER_SOCKET --expected-epoch E --expected-revision R [--execute]
coven agent native-extensions plan --provider pi --extension-root ABS_ROOT --from-kitsune
coven agent native-extensions install --provider pi --extension-root ABS_ROOT --from-kitsune [--apply]
coven session recovery inspect WORKSPACE --json
coven session recovery run WORKSPACE --file ABS_SELECTION_JSON --socket ABS_KIT_SOCKET --expect-recovery-revision TOKEN [--execute]
```

- [Interactive contract](INTERACTIVE_SESSION_CONTRACT.md): Djinn argv preserved; closed native argv for other adapters. Resume kind defaults to `id`; only Pi/OMP accept `path`. Historical pure planner is unchanged.
- [Navigation contract](AGENT_NAVIGATION_CONTRACT.md): unacknowledged fresh attention, then acknowledged fresh attention, then existing fresh/native priority, with stable observation-ID ties, not oldest-first. Exact `terminal.inspect`/`terminal.focus` require capability version 1 and runtime-incarnation UUID, with no legacy fallback. Normal DEC focus traffic is allowed; shell commands and Coven acknowledgement are not implicit.
- [Native profiles](NATIVE_PROVIDER_EVENTS_CONTRACT.md): `opencode` is the other managed provider. Separate policies/journals preserve historical Pi/OpenCode reference bytes and receipts; old reference assets remain reference-only unknown, not updated native assets. No provider hook autoinstall. Existing doctor closed-v1 capabilities are unchanged and do not discover these new profiles.
- [Recovery contract](SESSION_RECOVERY_CONTRACT.md): schema 1, at most 16 selected entries and 128 explicit imports. Actual `session.snapshot` raw pane `agent_session` hints from all panes are evaluated before any mutating frame. Exact source/runtime/kind/literal matches yield `existing_reference_present`, with `native_running: unestablished`. Stale hints conservatively skip; absence is not global absence proof.

## Evidence Limits

A5 uses the private A1 `preview`/`execute_with_guard` seam to durably synchronize
intent, state directory and workspace before first send. Timeout/cancellation after
the callback but before send is known-unsent. Queued, uncertain and intent records
block restarts; reservations are rolled back only for known-unsent/rejected
delivery. Actual SIGKILL regression tests exercise restart barriers, not power-loss
certification.

Generated native assets have passed a generated-asset -> actual `report-native` ->
normal receiver roundtrip with mock native callbacks and source-verified APIs.
This is not real provider loader/activation qualification. OpenCode requires a
validated pinned root DTO, from a lifecycle event or on-demand native SDK lookup,
and matching native permission/question requests, not cancel. Lookup itself emits
no lifecycle/status report and does not reset the observation-age budget.
Pi uses exact `getSessionId`, `hasUI === true`, TUI/RPC metadata, `agent_start`
busy and settled plus native `isIdle() === true` idle; no custom blocked bus.
Native macOS qualification remains outstanding; no all-22 interactive or Windows
completion claim follows. The composed Linux gate passed four real Kit/Coven
scenarios using only inert command execution and synthetic native events. Focused
native/CLI tests and Clippy pass. The attempted broader suite did not complete;
namespace-limited ACL failures and intermittent receiver timing failures remain
recorded separately, not waived by the focused gate. Historical
validation counts in other documents are scoped records, not additive totals or
new tests run by this documentation update.
