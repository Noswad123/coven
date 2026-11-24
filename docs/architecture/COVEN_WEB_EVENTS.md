# Coven Web foreground events and metadata

## Overview

Foreground Server-Sent Events (SSE) invalidate browser metadata; they do not stream
transcripts, terminal output or decision details. Authenticated GETs remain the
source of display data. Opt-in background Web Push is separate, although both
consumers use the same principal-scoped metadata cache.

See the [public contract](COVEN_WEB_CONTRACT.md), [browser guide](../guides/COVEN_WEB.md)
and [runbook](../operations/COVEN_WEB_DEPLOYMENT.md). These are implementation
semantics, not installed activation or rollout evidence.

## API / Interfaces

### Event stream

`GET /api/v1/events` accepts only that exact route, without queries or a body.
Admission uses the trusted Serve boundary: loopback peer, exact configured Host,
allowed login, matching Origin when supplied, and permitted Fetch Metadata
(`Sec-Fetch-Site` absent, `same-origin` or `none`). Loopback does not authenticate
Serve against other processes within the trusted local-host boundary.

Content-Length must be absent or exactly `0`; transfer/content encoding and
duplicate Content-Length are refused. Rejected bodies are not polled or drained.
An admitted response is `text/event-stream`, no-store, with the ordinary security
headers. No browser-supplied native selector or CSRF token is added.

| Named event | Meaning |
| --- | --- |
| `ready` | First frame; perform full metadata resynchronization. |
| `fleet` | Fleet evidence was accepted, renewed or failed; reread fleet. |
| `metadata` | A principal's paired snapshot was published; reread list and attention. |

Every named event has exactly the same static data:

```text
event: ready
data: {"contract":"coven.events.v1"}

```

There are no event IDs, personal fields, resource references, replay history or
Last-Event-ID recovery semantics. Heartbeats are comments (`: heartbeat`) every
10 seconds. The body expires after 60 seconds even while notifications continue.
Only admitted SSE receives a separate 65-second connection bound, including a
blocked writer; ordinary connections retain their 20-second overall deadline.
Header admission remains bounded to five seconds.

Admission is atomic: at most 16 streams globally and two per login, within the
existing 64 HTTP connections. Capacity refusal is 429 `observer_busy`; unavailable
event support is 503 `events_unavailable`. Streams consume no blocking-read permit.
Dropping a stream releases its registration.

### Cached metadata and freshness

`GET /api/v1/conversations` and `GET /api/v1/attention` read paired, transcript-free
snapshots from the service also used by Web Push. They perform no native refresh
for the request and occupy no blocking native-read slots. Each adds:

```json
{"freshness":{"generation":"opaque-epoch:42","remaining_ms":12000}}
```

The example is illustrative. Treat `generation` as an opaque epoch/counter token,
not a resource identity, timestamp, authority or cursor. Both documents from one
publication share a generation; separate GETs can straddle publications.
`remaining_ms` is an integer in `0..15000`, reduced by monotonic elapsed time and
the earliest applicable native grant, card-binding and fleet-coverage lease.
Collection work spends the lease; HTTP receipt never starts a fresh one.

Missing, invalidated or expired snapshots return `state: "unavailable"`, empty
display arrays and zero remaining time. That means **unknown**, not proof of no
conversations or pending requests. Existing coverage/omission fields still apply.
Partial discovery with known authorized rows is not a missing snapshot: preserve
those rows and their partial attention/stream coverage with bounded freshness.
An incomplete scan with no usable rows remains unavailable. Metadata provenance
tracks authorized candidate identities and coverage separately from the strict
card-uniqueness proof. Unavailable competitors still prevent verified card links;
preserving diagnostic rows never grants read, submission or decision authority.

## Data Flow

1. Fleet collection still polls every three seconds after work, with bounded
   failure backoff. Accepts, failures and unchanged evidence renewals notify
   viewers. [Local inventory hints](COVEN_WEB_LOCAL_EVENTS.md) can accelerate
   normal fenced acquisition and wake the shared metadata collector after meaningful
   accepted local changes; SSE itself does not create native instant-change hooks.
2. One owned metadata thread schedules every configured allowed login fairly by
   next-due time. Each login becomes due three seconds after its collection work
   completes; slow work can delay later collections. No tab or push opt-in is
   required, and tabs do not create independent native pollers.
3. Collection acquires list and attention as a pair. The list's exact metadata proof
   travels with its rows; fleet evidence precedes card acquisition, and card
   revision comes from the actual commit. Attention and final cache admission
   check those provenance tokens, not a later unrelated list.
4. Proof equality describes semantic identity/coverage, remaining stable across
   unchanged evidence renewals. It does not extend the published snapshot's own
   deadline. Reads may shorten a lease; an invalidated generation cannot revive.
   Freshly collected evidence gets a new publication generation.
5. Publication wakes only that login's metadata streams; fleet wakes all streams.
   Per-stream dirty bits and a capacity-one wake channel coalesce changes instead
   of retaining an unbounded event queue. Slow clients lose intermediate notices,
   not the obligation to reread current state.
6. The browser drains dirty fleet, conversation-list and attention GETs in order
   through one coordinator. Clearing a bit before awaiting its GET lets events
   during a fetch coalesce into a follow-up, without overlapping drains.

Web Push takes a final cache-only sample at network admission. Copied lease time,
validation and lock waits spend freshness too. Nonblocking job/subscription/stop
checks and expiry gate transport, including retries. This is not guaranteed
delivery and cannot retract a send already admitted to transport. Collection
observes sharing status; it never begins or renews native sharing grants.

## Edge Cases

- EventSource `open` is not authorization proof. Validated `ready` triggers full
  GET resynchronization; live status requires successful fleet and fresh
  lease-bearing conversation/attention responses.
- Errors close/fence the stream, trigger metadata GETs and schedule reconnect in
  six seconds. Transport errors alone are not HTTP authentication failures.
  Transport/auth generations fence old callbacks and deferred responses.
- Healthy live operation still resynchronizes every 30 seconds. Otherwise fallback
  GETs run six seconds after the drain completes. A one-second UI guard ages
  leases, clears expired private views and drops the live claim.
- Older servers or browsers without EventSource retain readable polling
  compatibility. Missing freshness uses the legacy 15-second client lease and
  never qualifies for a live-metadata claim.
- Hidden pages close SSE, stop metadata refreshes, clear conversation/request
  private views and abort in-flight reads. Return resynchronizes metadata, not
  messages. Persisted `pagehide` also clears output and suspends timers; BFcache
  `pageshow` restarts the expiry guard and resynchronizes. Nonpersisted exit
  destroys the coordinator.
- SSE and routine resynchronization never fetch messages, output or decision
  details, obtain mutation tokens or POST. Explicit card/request opens and reads
  remain deliberate. Existing notification deep-link navigation still verifies
  the exact alert/root before opening content; SSE adds no such exception.
- Browser throttling can delay guards. Events and leases promise neither immediate
  revocation nor recall of delivered/copied data. Server ACL and exact-reference
  checks remain required for each deliberate read or mutation.

## Example usage

Open the private site and let `ready` resynchronize metadata. Later `fleet` notices
update cards; `metadata` updates links and attention badges. Open a verified card
deliberately to read messages. Disconnecting SSE leaves six-second fallback reads,
not an automatic transcript read or mutation retry.

## Related files

- [Event body](../../src/observer/events.rs), [HTTP facade](../../src/observer/http.rs),
  [metadata scheduler/cache](../../src/observer/metadata.rs).
- [Fleet publisher](../../src/observer/mod.rs), [paired collection](../../src/observer/attention.rs),
  [card provenance](../../src/observer/cardlinks.rs),
  [grant proof](../../src/observer/conversations.rs), [push consumer](../../src/observer/push.rs).
- [Browser coordinator](../../clients/web/app.js),
  [test scope](../../clients/web/README.md#offline-checks), [architecture index](index.md).
