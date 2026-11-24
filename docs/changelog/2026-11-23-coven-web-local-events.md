# Local event-accelerated observation — 2026-11-23

The gateway now subscribes to generic events on configured **local** Kitsune
endpoints, using them as wake-up hints for existing bounded inventory acquisition.
See the [local-event contract](../architecture/COVEN_WEB_LOCAL_EVENTS.md) for
selectors, framing, scheduling and coverage limits. Browser SSE remains the
[separate downstream channel](../architecture/COVEN_WEB_EVENTS.md).

## Delivered scope and limits

- One persistent subscription owned by each eligible local endpoint's existing
  poller; no extra inventory worker or unbounded event queue.
- Fixed generic topology/title subscriptions, correlated acknowledgement and
  endpoint identity checks through reconnect, idle periods and EOF.
- Coalesced, rate-bounded refreshes through the existing fenced snapshot/process
  acquisition. Raw event payloads never become displayed state or authority.
- Meaningful accepted local inventory changes wake the existing shared metadata
  scheduler. Unchanged renewals and metadata publication do not create wake loops.
- Periodic three-second reconciliation remains unchanged, including failure
  backoff and output-admission safeguards. Kit does not reliably emit manual-label,
  raw-title-only or foreground-process changes; its event wire has no usable gap
  cursor and can replay or silently omit history. This does **not** remove polling
  or promise immediate native status, grant or decision updates.
- SSH endpoints, remote helper DTOs, native producers, explicit message/detail
  reads, mutations and background subscription policy are unchanged.

Review corrections bound subscription setup by the next inventory deadline,
preserve identity fencing when a replaced connection immediately reaches EOF,
and make the 512 KiB turn budget explicit without losing deferred frames.

## Validation

| Gate | Recorded result |
| --- | --- |
| Observer library | 223 passed |
| Observation-socket scope | 43 passed |
| New real-gateway/mock-Kit integration | 1 passed |
| Existing gateway integration | 9 passed, 5 opt-in producer tests ignored |
| Native package | 2 passed |
| Full Node suite with actual release gateway and installed Firefox | 146 passed, zero failures/skips, twice |
| Formatting, whitespace, offline locked all-target check and release build | Passed |

Scopes overlap and are not one aggregate full-repository result. Broad library
tests and Clippy were not run. The new synthetic integration verifies accelerated
acquisition before the next periodic deadline, snapshot authority over a differing
event payload, label changes without an event, and identical replay settling.
Adapter/scheduler tests cover malformed framing, partial deadlines, cancellation,
replacement/alias retargeting, storm limits, reconnect, output priority and wake
fairness. No new all-platform, physical-phone or provider/model acceptance follows.

The unchanged actual local Kit acknowledged the exact twelve selectors in 0.6 ms.
A bounded read-only probe received 31 generic frames, then closed cleanly. A
push-disabled old/new canary matched fleet and native metadata over five samples,
16 seconds and five publication generations. It retained one observed-source
conversation with a null/unverified card and partial attention with one complete
stream, zero items and one unavailable count. Freshness was positive and decreased
within each generation. A persistent Unix connection and low CPU use were observed.
No raw event content was retained.

**Live event-triggered acceleration was not measured:** no controlled pane/title
or native mutation was performed. Actual negotiation and connection continuity
are established; acceleration is established by the synthetic integration.

## Publication and recovery

Gateway SHA256:
`ad1d49cedf6dbe68e9d858e45f56b19376bd16c023dc5153e95c407275edfa51`.
Immutable prefix: `~/.local/libexec/coven-web/ad1d49cedf6dbe68`.
Future dev selection: `coven-native-zh3kmgjy`.

Actual Serve HTTPS health, assets, fleet/native parity and incremental SSE passed
after activation. `ready` arrived in 6 ms, `metadata` in 2.315 seconds and heartbeat
in 10.007 seconds. These are stream observations, not measurements of upstream
event latency. Local fleet was reachable/observed with one agent. Megatron retained
its known offline timeout; no remote recovery was attempted.

Only `coven-web.service` restarted. Kit/Djinn/native processes and selections,
configuration bytes, Serve, the push key and one subscription binding were
preserved. The remote helper remains `5e3b99d14e57a1b8`. Previous payloads remain
retained; this release's unit backup restores **`3cec49d2f347e811`**, not the older
pre-SSE payload. Exact backup and prior dev selection are in
[deployment state](../operations/COVEN_WEB_DEPLOYMENT_STATE.md#rollback).

Private `0600` provenance, canary and rollout reports are beside the new payload.
Owned test/build/profile/probe artifacts and processes were cleaned; no cleanup
failure was reported. Unowned `pn.HC9lGV` was preserved. Source remains uncommitted
on `a6d33c3`; no staging, commits, pushes or worktrees were created.
