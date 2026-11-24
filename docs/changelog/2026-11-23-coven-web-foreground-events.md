# Foreground metadata events — 2026-11-23

Coven Web now uses authenticated Server-Sent Events to invalidate session-card,
conversation-availability and attention metadata on an open page. The
[event/cache contract](../architecture/COVEN_WEB_EVENTS.md) defines bounds,
freshness, provenance and fallback; the [browser guide](../guides/COVEN_WEB.md)
describes user-visible behavior. Background Web Push remains a separate opt-in.

## Delivered scope

- Static content-free `ready`, `fleet` and principal-scoped `metadata` events.
- One shared metadata scheduler/cache instead of native acquisition per tab or
  competing conversation/attention GETs. Web Push consumes the same cache.
- Generation-bound, decreasing freshness, capped by actual evidence leases.
  Repeated GETs, heartbeats and unchanged renewal do not extend an old snapshot.
- Coalesced browser resynchronization, six-second fallback/reconnect and
  thirty-second healthy reconciliation; hidden/BFcache pages suspend safely.
- No automatic message, terminal-output or decision-detail reads, mutation tokens
  or POSTs. Draft, interaction, scroll and exact-reference fences remain required.
- Upstream fleet acquisition still polls; this is not end-to-end native event
  subscription or a promise of instantaneous observations.

## Review and rollout corrections

Review added regressions for shared-source principals, exact list provenance,
shorter renewed leases, irrelevant SSH churn, BFcache restoration, and final Push
admission after reconciliation, cryptography, cancellation and lock waits.

An initial candidate passed fixture tests and HTTPS streaming, but an old/new
metadata comparison found that it hid a valid partial conversation listing. The
gateway and future-dev selection were restored to the old payload before further
work. The fix separates partial metadata provenance from strict card uniqueness;
it retains known authorized rows without making hidden competitors eligible for
binding. Five synthetic regressions and a required predeployment old/new canary
then verified the correction. The earlier `05a6bbd5c4e4e79d` payload remains
retained, not selected or recommended for rollback.

Gateway fixture startup now waits for bounded semantic metadata readiness rather
than assuming TCP readiness means a populated cache. A Firefox DOM fixture also
waits for the transcript response before asserting a cross-process request count.
One earlier live-EventSource failure lacked retained assertions and did not
reproduce; it is not claimed to have an established root cause. Final runner
file-size/subreaper interference was corrected without production source changes.

## Final validation

| Gate | Recorded result |
| --- | --- |
| Focused observer library, including Push | 183 passed |
| HTTP gateway integration | 9 passed, 5 opt-in producer tests ignored |
| Native packaging | 2 passed |
| Full Node suite with installed Firefox and actual release gateway | 146 passed, zero failures/skips, twice |
| Formatting, whitespace, offline locked all-target check and release build | Passed |

The Firefox native EventSource fixture used real gateway embedded assets and an
isolated streaming proxy, including reconnect and no automatic content/action
requests. BFcache behavior uses deterministic lifecycle fixtures, not real browser
history acceptance. The broad library suite and Clippy were not run; earlier
unrelated full-library failures are not superseded by this focused gate. No new
physical-phone, provider/model, live decision or vendor-push acceptance is claimed.

The push-disabled native canary matched the old gateway over five samples and
four publication generations: one conversation from an observed source, null
unverified card, partial attention with one complete stream, zero items and one
unavailable count. Omission/unlinked semantics matched. Candidate freshness was
positive and decreased within a generation. These are dated dynamic observations,
not permanent coverage expectations.

## Published and observed

Gateway SHA256:
`3cec49d2f347e81101f2b6a54345197f227f4e61abfefbd46d2ff9202b2f9ef9`.
Immutable prefix: `~/.local/libexec/coven-web/3cec49d2f347e811`.
Future dev selection: `coven-native-vfpkjm1j`.

Actual Serve HTTPS delivered `ready` in 6 ms, `metadata` in 2.877 seconds and a
heartbeat in 10.007 seconds, incrementally rather than waiting for stream closure.
Three serial reconnects received `ready` in 6–7 ms. Native metadata retained canary
parity after activation. Local fleet was reachable/observed with one agent;
Megatron was offline in Tailscale and timed out before SSH helper execution on
both builds. No remote recovery or helper compatibility failure was inferred.

Only `coven-web.service` restarted. Configuration, Serve, native processes and
selectors, the push key and one subscription binding were preserved. Megatron's
helper remains pinned to `5e3b99d14e57a1b8`; this change needs no helper/native UI
upgrade. Exact rollback and the dated selected state are in
[deployment state](../operations/COVEN_WEB_DEPLOYMENT_STATE.md#rollback).

Private `0600` provenance, canary and rollout reports are retained beside the new
immutable payload. All owned run/build/profile/log artifacts were removed after
child and stream cleanup; no cleanup failure was reported. Unowned artifacts,
including `pn.HC9lGV`, were left untouched. Source remains uncommitted on
`a6d33c3`; no staging, commits, pushes or worktrees were created.
