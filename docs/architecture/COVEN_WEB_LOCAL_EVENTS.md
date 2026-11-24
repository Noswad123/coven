# Coven Web local inventory event hints

## Overview

Local Kitsune events accelerate ordinary fleet acquisition; they are not inventory
truth or native authority. This is the upstream Unix subscription, separate from
the browser's content-free [foreground SSE](COVEN_WEB_EVENTS.md). Periodic
reconciliation remains necessary, and fewer polls are not promised.

Only configured local fleet endpoints (`ssh: None`) qualify, after a successful
normal protocol-18 inventory. SSH helpers and remote endpoints remain unsubscribed;
their protocol and deployment are unchanged. No Kitsune/native-process restart,
configuration write or new browser selector is part of this path.

## API / Interfaces

`LocalEventSource::open(path, deadline, cancel)` returns a `LocalEventStream`.
The stream exposes its frozen endpoint and `next(until, cancel)`, returning either
`Hint::Inventory`, idle `None`, or a bounded failure. Raw generic event frames are
validated and projected to that hint only; they never patch fleet/cache rows.

`EventSocket::subscribe` is a private fixed-purpose `events.subscribe` transport.
It does **not** widen generic `ReadMethod`, which remains snapshot, process-info
and eligible pane-output reads. Its twelve selectors are fixed:

| Family | Selectors |
| --- | --- |
| Pane | `pane.created`, `pane.closed`, `pane.updated`, `pane.moved`, `pane.exited` |
| Tab | `tab.created`, `tab.closed`, `tab.moved` |
| Workspace | `workspace.created`, `workspace.closed`, `workspace.moved` |
| Layout | `layout.updated` |

Selectors use dotted names; received `event` and `data.type` use matching
underscore names, such as `pane_updated`. Consumed identity fields are validated.
Unknown event kinds with valid envelopes do not produce hints. There is no
terminal event family or retired agent-status subscription.

The correlated acknowledgement must contain exactly one of result/error, with
result type `subscription_started` for success. Setup/ACK has a five-second cap,
further bounded by the next inventory due time, including pending hint work.
It cannot postpone due periodic acquisition. ACK is capability negotiation, not
proof of inventory completeness; successful open also schedules reconciliation.

### Stream bounds and endpoint fencing

- Maximum frame: 256 KiB **including newline**, for ACK and event frames.
- Strict UTF-8 JSON, duplicate-key rejection and nesting depth at most 32.
  Ordinary strings are limited to 4096 characters; the event channel has no
  enlarged path or terminal-output string exception.
- Each cooperative read turn is bounded by 32 frames, 512 KiB and 20 ms. A full
  maximum-frame allowance is reserved before consuming another frame. One bounded
  decode is not interruptible, so 20 ms is not a hard preemption guarantee.
- Partial frame assembly has its own five-second deadline across turns.
  Idle deadlines are not EOF; EOF with a partial frame fails as incomplete.
- Selection freezes selected/resolved path and device/inode. Fences surround setup,
  ACK and frames, run during idle at intervals no longer than one second, and
  check EOF. These checks are not authentication against malicious local users.

Missing, replaced or unsafe selected endpoints invalidate fleet evidence rather
than silently trusting the old connection. Ordinary stream failure without an
identity change drops/retries the subscription; inventory remains independently
scheduled. Upstream has no heartbeat or required finite event-stream expiry:
silence is legitimate and is not itself failure. Browser SSE's heartbeat and
rotation rules are separate.

## Data Flow

1. The existing endpoint worker completes fenced inventory before subscribing.
   It owns the only subscription for that endpoint and drops it before joining
   on shutdown; tabs do not create native subscription threads.
2. `NativeEvents` validates frames and emits only inventory hints. It suppresses
   identical `pane_updated` projections using at most 1024 semantic fingerprints,
   covering pane/terminal/workspace/tab identity and projected title, safe manual
   label, provider and project. Exceeding the table bound fails the stream rather
   than growing indefinitely. This is deduplication, not an inventory cache.
3. Topology hints coalesce for 250 ms. At most one extra event-only inventory is
   admitted per three seconds, with a further 250 ms post-acquisition admission
   gap for output opportunities. Overdue periodic work takes priority.
4. Every hint-triggered refresh uses the normal five-second, shared 4 MiB fenced
   snapshot/process/snapshot acquisition and classifier. Existing endpoint
   single-flight and periodic priority over repeated output remain in force.
5. Periodic reconciliation remains three seconds after completed work. Inventory
   failure backoff remains 6/12/24 seconds; endpoint-busy retries remain 100 ms.
   Hints cannot bypass the failure barrier. Extra event acquisition is additional
   bounded work, not a promise to reduce total inventory calls.
6. Reconnect waits are 1/2/4/8/16/30 seconds. Backoff resets after a connection
   survives at least three seconds, not merely after ACK followed by immediate EOF.
   Reconnect history is reconciled by ordinary inventory, never applied as truth.

### Accepted inventory to metadata wake

A meaningful **accepted local inventory** change or failure transition wakes the
existing metadata collector through a generation-counted condition variable.
Meaningful acceptance includes changed inventory/generation/coverage or recovery
from stale/failed evidence. Raw frames and unchanged evidence renewals do not wake
it. Browser fleet notices still occur for accepts, renewals and failures.

There is one owned metadata thread and one weak wake registration, not another
poller per viewer. A wake marks configured logins pending; overdue periodic work
runs first, then accelerated work is round-robin with at most one extra collection
per login per three seconds. Accelerated work does not move the periodic deadline.
Periodic collection remains three seconds after work, important for five-second
native grants even when inventory is unchanged. Metadata publication only notifies
browser streams; it does not feed back into the collector's wake generation.

Paired metadata proof, partial-coverage handling and freshness rules remain those
of [foreground events and metadata](COVEN_WEB_EVENTS.md). Unchanged proof/evidence
renewal never extends an already published generation's deadline. Hints do not
grant sharing, read, submission or decision permission. There are no local native
grant/decision event subscriptions yet, so periodic native checks remain required.

## Edge Cases

- The upstream history is internally bounded to 512 entries, with no wire cursor
  or sequence/gap indication. Replay, reordering and silent history loss cannot
  prove completeness. Continuous connection and ACK do not remove this gap.
- Topology and supported stripped-title changes can supply hints. Manual pane-label
  changes, raw-title-only changes and foreground-process changes are not guaranteed
  to emit subscribed events. Normal inventory can detect them on the periodic
  three-seconds-after-work cadence, not instantly; work/backoff can add delay.
- Duplicate pane updates can settle without repeated acquisition. Other repeated
  topology hints remain bounded by coalescing/admission rather than becoming an
  authoritative event log. Disconnect/replay still requires reconciliation.
- No transcript, terminal content or decision detail is fetched by this feature;
  it issues no POST, native mutation, restart or configuration write. Existing
  deliberate content reads and exact authorization fences are unchanged.

## Example usage

On an already configured qualifying local endpoint, a supported pane change can
produce `pane_updated`. After coalescing, the worker reacquires normal inventory;
only its accepted projection can change the fleet card and wake metadata. A
different title in the event payload cannot become the displayed title by itself.
A silent manual-label change instead waits for periodic reconciliation.

`tests/observer_local_events.rs` exercises a real Rust HTTP gateway with a synthetic
Unix Kitsune peer. Its assertions require hint-driven inventory/publication within
two seconds, before the three-second periodic deadline, retain old data while
acquisition is held, reject raw event payload as truth, reconcile a silent label
change and settle identical replay. This describes test scope, not a test-run
result. Actual Kitsune and phone acceptance, broader tests and release gates are
not established here; no installed activation is claimed.

## Related files

- [Event projection](../../src/observer/local_events.rs),
  [scheduler](../../src/observer/local_event_scheduler.rs),
  [framer/transport](../../src/observation_socket.rs), [wire profile](../../src/observation_wire.rs).
- [Acquisition](../../src/observer/source.rs), [fleet acceptance](../../src/observer/mod.rs),
  [metadata scheduler](../../src/observer/metadata.rs), [wake registration](../../src/observer/events.rs).
- [Gateway fixture](../../tests/observer_local_events.rs),
  [metadata wake tests](../../src/observer/metadata_wake_tests.rs),
  [read-only canary procedure](../operations/COVEN_WEB_DEPLOYMENT.md#local-subscription-canary).
