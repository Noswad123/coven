# Checked Agent Navigation

Coven's public CLI attention controls are ready: `agent observations`,
`agent attention list`, and `agent attention acknowledge OBS --expected-epoch E
--expected-revision R [--apply]`. Each accepts `--socket ABS` or
`--from-kitsune [--state-root DIR]`, and `--json`. Acknowledgement previews by
default and changes only acknowledgement bookkeeping, not pending/native facts.

Acknowledgement applies to current fresh decision occurrences, fenced by receiver
epoch and attention revision, not to all future requests with the same ID. View
priority is unacknowledged fresh attention, then acknowledged fresh attention,
then existing fresh/native priority, with stable observation-ID ties, not
oldest-first. Expiry is unknown, not done. The private control APIs are
`observations`, `attention.list` and `attention.acknowledge`; generic row clicks
remain read-only. See the [A1-A5 implementation record](FEATURE_ADOPTION_STATUS.md).

## Temporary Assertions

`agent associate OBS --kitsune-socket ABS --terminal TERM --expected-epoch E
[--expected-revision R] [--apply]` asserts a `user_asserted` navigation target.
It is **not independently proven native hosting**. A live shell may outlast its
native provider. A2 producer attachment qualification remains pending; A2 is not
fully implemented. No PID, name, label, provider reference, or old group binding
automatically creates an association.

The receiver keeps at most 128 volatile records, including invalidated and removed
tombstones. Capacity exhaustion refuses new records until receiver restart; it
does not silently evict CAS history. Records bind receiver epoch, opaque observation
ID, endpoint device/inode, exact terminal ID, and inspected runtime generation.
Assertions expire after 30 monotonic seconds. Repaints, status, reads and focus do
not renew assertions. Observation expiration, end, eviction or sequence gaps
invalidate associations. Gap invalidation conservatively invalidates all assertions.
Invalid records cannot revive without another explicit assertion with the exact
current revision. A missing revision means create only; replacement requires the
existing opaque revision. Receiver restart clears all records and changes epoch.

`agent dissociate OBS --expected-epoch E --expected-revision R [--apply]` previews
by default. Its applied result returns a new tombstone revision for subsequent CAS.
All association previews are read-only, though associate preview contacts Kit for
checked inspection. Neither public generic UI projection nor persisted group
schemas receives raw navigation targets. Explicit private CLI results contain
sanitized terminal IDs but no native SIDs or provider payloads. There are no new
clickable auto-focus or acknowledgement actions in the generic presenter.

## Checked Focus

`agent focus OBS --expected-epoch E --expected-revision R [--execute]` is read-only
unless `--execute` is supplied. Execution prepares fresh receiver metadata, checks
Kit's `snapshot.terminal_navigation.version == 1`, inspects the exact terminal,
checks pinned endpoint identity and runtime generation, then repeats receiver CAS
immediately before sending one `terminal.focus` with `terminal_id` and
`expected_runtime_generation`. The receiver performs no Kit I/O in its loop.
The private assertion RPC validates closed input shape and pins; its assertions
remain caller-supplied, not authenticated claims of completed inspection.

Runtime generation is Kit's fresh runtime-incarnation UUID, not a PID, provider
session ID or persisted detector epoch. See Kit's generic
[Terminal Navigation v1](../../kitsune/TERMINAL_NAVIGATION.md) contract.

Kit checks exact ID, current live attachment and generation atomically before
focus. Same-terminal/same-generation movement follows the current locator. Changed
generation, endpoint replacement, stale CAS, exited/detached terminals, missing
capability and unknown methods refuse navigation. There is no fallback to
`pane.focus`, `agent.focus`, aliases, shell commands or provider commands.

Uncertain delivery is never retried and never acknowledges attention or submits
input. Success means focus update accepted, not pixels displayed or an OS window
raised. Generic DEC focus reports are allowed for terminals enabling them.
This is not a lease: native state can change after preparation or acceptance.
Same-user endpoint checks are race fences, not authentication. No native-hosting
guarantee is made, even after successful focus.
