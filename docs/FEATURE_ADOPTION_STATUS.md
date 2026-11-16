# Approved Feature Adoption Status

Implementation record as of 2026-11-15, not a missing-feature backlog or a claim
that all adoption work is complete. The user approved A1-A5. Remaining product
residuals are tracked in [feature decisions](KITSUNE_RETIREMENT_FEATURE_DECISIONS.md);
platform qualification and legacy retirement remain in the
[migration matrix](AGENT_MANAGEMENT_MIGRATION.md).

The user accepts the current Codex/Copilot/Pi limitations. Richer/full support for
those providers is deferred until actual use warrants revisiting it; it is not a
blocker for the current Djinn workflow or retirement. Existing capture behavior
and its safety limits are unchanged.

| Adoption | Delivered scope | Remaining limits |
| --- | --- | --- |
| A1 | Explicit, preview-first interactive start/resume for all 16 paired source-characterized adapters: djinn, claude, codex, copilot, devin, droid, kimi, mastracode, pi, omp, hermes, opencode, qodercli, kilo, cursor, grok. | Installed CLI/version compatibility is unverified. Enqueued input is not provider startup. No arbitrary extra args, probes, journal or automatic retry in standalone A1. |
| A2, qualified local Djinn | Source-qualified producer association for the loaded, mounted selected root in normal local Djinn full TUI, default integrated worker/local RPC and implicit-local workspace. Exact reporter instance/SID joins a fixed Kit endpoint/terminal/incarnation under a five-second producer lease. Existing explicit 30-second user assertions remain unchanged. | Other clients/runtimes, mini, external attach, remote/explicit workspaces and HTTP transport are not qualified. Display association is caller-unverified, not authentication, physical-pixel proof or provider execution ownership. No PID/newest-SID inference or automatic rebind. |
| A3, partial | Independent opt-in Pi/OpenCode native profiles, including silent resumed-root lookup; separate unordered Codex/Copilot native hook facts through `coven.provider-facts.v1`, `agent report-fact` and inert `native-hooks plan` snippets. | Codex/Copilot public state is always UNKNOWN, not current activity or correlated decisions. Codex is source-qualified, Copilot documentation-qualified, neither binary-certified. Pi paired UI decisions remain NO-GO at the source pin. Native loading/activation and managed native-hook configuration are not qualified/implemented. |
| A4 | Fresh attention lists, occurrence acknowledgements and deliberate checked CLI focus through generic Kit terminal navigation, using an explicit assertion or qualified producer association. | Acknowledgement does not clear decisions/native facts. Generic row clicks remain read-only; expiry means unknown, not done. Focus acceptance is not physical display or native execution proof. |
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
coven agent attachment-target --from-kitsune --json
coven agent report-attachment --from-kitsune
coven agent report-fact --provider codex --event PreModelRequest --from-kitsune --best-effort
coven agent native-hooks plan --provider codex --config-root ABS_ROOT --executable ABS_COVEN --from-kitsune --json
coven agent native-extensions plan --provider pi --extension-root ABS_ROOT --from-kitsune
coven agent native-extensions install --provider pi --extension-root ABS_ROOT --from-kitsune [--apply]
coven session recovery inspect WORKSPACE --json
coven session recovery run WORKSPACE --file ABS_SELECTION_JSON --socket ABS_KIT_SOCKET --expect-recovery-revision TOKEN [--execute]
```

- [Interactive contract](INTERACTIVE_SESSION_CONTRACT.md): Djinn argv preserved; closed native argv for other adapters. Resume kind defaults to `id`; only Pi/OMP accept `path`. Historical pure planner is unchanged.
- [Navigation contract](AGENT_NAVIGATION_CONTRACT.md): unacknowledged fresh attention, then acknowledged fresh attention, then existing fresh/native priority, with stable observation-ID ties, not oldest-first. Exact `terminal.inspect`/`terminal.focus` require capability version 1 and runtime-incarnation UUID, with no legacy fallback. Normal DEC focus traffic is allowed; shell commands and Coven acknowledgement are not implicit.
- [Native profiles](NATIVE_PROVIDER_EVENTS_CONTRACT.md): `opencode` is the other managed provider. Separate policies/journals preserve historical Pi/OpenCode reference bytes and receipts; old reference assets remain reference-only unknown, not updated native assets. No provider hook autoinstall. Existing doctor closed-v1 capabilities are unchanged and do not discover these new profiles.
- [Native attachment](NATIVE_ATTACHMENT_CONTRACT.md): closed capture/report frames, epoch and target pins, private CLI `hosting_state`/`association_revision`, checked focus and bounded lease/revision retention. Generic UI omits raw targets, IDs and association revisions; labels add no automatic click action or acknowledgement.
- [Native hook facts](NATIVE_HOOK_FACTS_CONTRACT.md): exact source pins, closed payload selection and manual snippets. Native handlers emit zero stdout and exit zero on report failure. No fake shared UUID/sequence, tool-name request pairing or current-state inference. Managed native-hooks install/uninstall/rollback is not implemented; old exact-image journals and provider approvals/credentials remain untouched.
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
Paired UI decisions are NO-GO at the pinned source because fire-and-forget async
handler delivery does not establish ordering and immutable occurrence/session IDs
are missing, not merely because events lack a whitelist entry.

The Djinn producer uses the existing selected-load callback, a frontend lifetime
UUID/revision and mounted-only one-second heartbeat/release. Worker generation
fences, actual SDK root metadata and the actual reporter UUID registry qualify each
selection. Quiet qualification is read-only, consumes no entries in the 256-entry
event map and emits no activity; quiet rows stay UNKNOWN with null activity age.
Chat leave, selection change and plugin disposal release; lost release expires.
Conflicting targets are ambiguous. Capture is once per frontend with bounded
transient work, never automatic endpoint/runtime/epoch rebinding. The revocation
race fix clears the owner token before awaiting release.

Codex facts have unknown source age and local receipt age only. Copilot UTC-Z
source time anchors monotonic freshness; equal timestamps never renew and differing
events at the same timestamp are ambiguous. These bounded volatile facts are
separate from reference journals, native-profile status and attachment clocks.

## Recorded Qualification

These are supplied implementation-validation records, not tests rerun by this
documentation update. Counts overlap across layers and must not be aggregated.

| Scope | Recorded result and limits |
| --- | --- |
| Full native Coven suite (before attachment deletion fix) | 71 targets, 1,760 passed, 0 failed, 7 ignored child fixtures exercised by parents; no skips, timeouts or output truncation. Host UID, sealed environment, non-Unix networking seccomp-blocked; offline, one build job, two test threads. |
| Post-deletion-fix targeted Coven gate | Separate gate: 118 management/native/presenter, 4 navigation, 1 attachment integration and 4 facts integration tests passed (127 total), offline with one build job and default test parallelism. Three new deletion/removal/512-tombstone-pressure regressions failed against pre-fix code, then passed with the fix. All-target Clippy with warnings denied and formatting passed. The full 1,760-test suite was not rerun after this fix; counts are not additive. |
| Latest post-deletion-fix broad core gate | Full library: 1,317 passed and 7 parent-exercised fixtures ignored; CLI: 100 passed; attachment/facts integration targets: 5 passed. Total for this separate scoped run: 1,422 passed, 0 failed. The source-built native binary prerequisite was established before rerunning the complete library target. Host UID and sealed environment retained; the other integration targets were not rerun. |
| ACL/tempdir and receiver diagnostics | Seven ACL tests now pass without namespace remapping; group-observation parity tempdirs honor TMPDIR. Prior flaky receiver checks passed 6/6 and one full run, not a proven root-cause fix or all-stress qualification. |
| Coven static checks | Full formatting and Clippy with warnings denied passed. |
| Actual Djinn producer -> actual Coven -> actual Kit | [Seven composed scenarios passed](../tools/normal-workflow-check/README.md#a2-source-native-interop): quiet roots, stable real heartbeat, exact identity join, focus without manual assertion, selection/detach, plugin disposal and lost-renewal expiry. Actual SDK/plugin factory, selected-session load/controller/registry; not a physically rendered Djinn TUI or separate Bun Worker/private-RPC handshake. |
| Focused native facts/source-pin fixtures | 72 tests passed; not real provider activation or binary certification. |
| Djinn producer/TUI | 55 Bun producer and 14 TUI tests passed; both package typechecks passed. These overlap earlier gate regressions. |
| Kit | Prior full-target compilation and regressions stand; no Windows/macOS qualification follows. |

This is source implementation, not installed activation or full legacy extraction.
All three apps need rebuilds and an explicit server restart/new-pane/provider
handoff when the user is ready. Provider configurations are never auto-edited.
Native macOS qualification remains outstanding; no all-22 interactive or Windows
completion claim follows. Historical validation records remain scoped and
non-additive.
