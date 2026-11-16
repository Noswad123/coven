# Djinn Client Attachments

This is the receiver and checked CLI contract for producer-backed **local Djinn
full-TUI selected-root display associations**. Qualification of actual worker
root metadata and frontend lifecycle belongs to Djinn's producer. Coven accepts
that source-qualified declaration; it does not authenticate a caller, prove
native identity, infer activity, or establish job completion. Inherited pane
environment is only a locator hint, never native-hosting evidence.

## Initial Capture

Run `coven agent attachment-target --from-kitsune --json` once at frontend startup.
`--state-root DIR` may explicitly select the same root as the installed route.
The closed result is:

```json
{
  "contract": "coven.attachment-target.v1",
  "server_epoch": "0123456789abcdef0123456789abcdef",
  "target": {
    "socket": "/canonical/private/kit-api.sock",
    "device": 1,
    "inode": 2,
    "terminal_id": "exact-terminal",
    "runtime_generation": "12345678-1234-1234-1234-123456789abc"
  }
}
```

The receiver epoch comes from normal `observations`, not old `agent serve`.
Capture resolves the active receiver through the existing integration receipt,
current native executable route, ownership and private socket identity fences.
It selects the actual `KITSUNE_SOCKET_PATH`, requires navigation version 1 in
`session.snapshot`, maps exactly one `KITSUNE_PANE_ID` to a terminal in that
snapshot, and requires an exact live `terminal.inspect` with matching pane.
Runtime generation must be a canonical lowercase hyphenated UUID; device and
inode are unsigned 64-bit integers. Unknown capability, missing/duplicate mapping
and nonlive targets fail closed. Capture does not create an unknown state root,
change the PTY, look up a provider, or invoke any shell/native input operation.
The supported CLI route is `--from-kitsune`; bare `--socket` is refused here.

The producer pins this entire target and epoch for the frontend lifetime.
Never resample a replacement terminal, runtime or endpoint after failure.

## Report Frames

`coven agent report-attachment --from-kitsune [--state-root DIR] [--json]`
reads one closed JSON stdin object, at most 8192 bytes. Attach is exactly:

```json
{
  "contract": "djinn.client-attachment.v1",
  "expected_epoch": "0123456789abcdef0123456789abcdef",
  "client_id": "12345678-1234-1234-1234-123456789abd",
  "revision": 1,
  "operation": "attach",
  "instance_id": "12345678-1234-1234-1234-123456789abc",
  "session_id": "actual-selected-root-session",
  "scope": "local_full_tui_root",
  "target": {
    "socket": "/canonical/private/kit-api.sock",
    "device": 1,
    "inode": 2,
    "terminal_id": "exact-terminal",
    "runtime_generation": "12345678-1234-1234-1234-123456789abc"
  }
}
```

Detach is exactly `contract`, `expected_epoch`, `client_id`, `revision`, and
`operation: "detach"`. It must not contain scope, instance, session or target.
UUIDs are canonical lowercase; revisions are positive JS-safe integers, at most
9007199254740991. Session IDs are nonblank, control-free, at most 512 UTF-8 bytes.
Unknown fields, duplicate JSON keys, oversized and excessive-depth frames refuse.
`instance_id` is the actual reporter instance used by Djinn native events, not a
new per-client UUID. `client_id` is the frontend's own stable lifetime UUID.

Every attach CLI invocation validates the current active from-Kit route and
checks the supplied target against the actual Kit API's canonical path and
device/inode. It inspects that exact terminal and compares the original runtime
generation. It never maps the inherited pane again or rebinds a replacement.
Detach still validates route and epoch, but performs no Kit inspection, so an
exited terminal can release its lease. A valid API endpoint/route is still needed.

Each command sends once, without retry, using the private RPC contract
`coven.client-attachments.v1`, method `report`, params equal to the frame **without
`contract`**. The normal four-field contract/id/type result/error envelope is
unchanged. Success is exactly `{"type":"accepted","delivery":"volatile"}`.
Peer error bodies remain private. Old `agent serve` refuses this RPC. Existing
native-event and provider-event input contracts remain unchanged.

## Volatile Leases

The receiver retains at most 128 selected clients and 512 client revision
watermarks, with a 5-second monotonic lease TTL and 600-second watermark retention.
Client selection is one exact instance/session/target tuple at a time. A higher
attach independently declares a freshly root-validated selection. It atomically
supersedes the old selection; detach consumes its revision immediately. Duplicate
and out-of-order frames never renew a lease. Rejected higher revisions admitted
within watermark capacity are consumed and release the old selection rather than
leaving it focusable. Capacity refuses new selections/history, never silently
evicts live replay protection. After 600 seconds, forgotten replay history is not
a permanent anti-replay guarantee.

Identical current targets from multiple clients may focus; any differing current
target for the same observation refuses as ambiguous. There is no newest-client,
newest-SID or PID heuristic. A lease's clock is independent of native status and
decision clocks. Heartbeats change no busy/idle state, native evidence age,
sequence, pending occurrence or acknowledgement. Quiet roots can host with
reported state UNKNOWN, freshness unknown and null activity age.

The exact `(instance_id, session_id)` lookup uses the existing Djinn observation
hash `SHA256("native\0" + instance + "\0" + session)` (the separators are NUL
bytes). If absent, a successful declaration materializes metadata within the
shared 128-native-observation bound.
At capacity, admission reclaims the oldest inactive, unleased observation with
shared retirement bookkeeping. Fresh native facts and all live producer leases,
including conflicting/ambiguous leases, are protected. Detach and expiry do not
erase client revision watermarks; reclamation cannot bypass retained revisions.
Its origin is `djinn_client_attachment` until a genuine native event joins the
same hash and restores `djinn_native_event`.
No lifecycle/status event or native sequence is fabricated. Persisted legacy
receipts are untouched. Observation retirement clears its selected leases;
instance sequence gaps and sequence-history retirement invalidate existing
instance leases. A higher, freshly selected-root heartbeat may requalify after a
gap, with a new association revision. Ended observations/native tombstones refuse
reattachment. Accepting native deletion immediately revokes all matching client
selections without erasing revision watermarks. Every observation-removal path,
including tombstone forgetting, clears selections and counts an actual row
retirement once. Forgetting bounded end history cannot revive an old lease or
association revision; requalification requires a fresh higher-revision attach.
Receiver restart clears leases and changes epoch; late frames fail.

## Checked Navigation

Existing `user_asserted` assertions retain their original 30-second semantics,
CAS/tombstones and priority. They are not relabeled as producer evidence.
`focus.prepare` prefers a valid explicit assertion; otherwise it requires one
unambiguous live producer association. The same closed nine-field
`coven.agent-navigation.v1` result supports the second known pair
`provenance: "producer_reported"`, `native_hosting: "local_full_tui_root"`.
Caller and native origin remain unverified.

The private `association.inspect` control method accepts exactly
`observation_id`, `expected_epoch`, `expected_revision` (nullable for discovery).
It returns the same checked nine-field result. The public library wrapper is
`agent_navigation::association_arguments`; no additional main CLI variant is
needed to discover a revision: `agent observations --from-kitsune --json` includes
sanitized `hosting_state` (`none` or `local_full_tui_root`) and nullable
`association_revision` on native rows. These fields are closed and required,
including explicit nulls; raw sessions, clients, instances and targets are absent.

Producer association revisions remain stable across ordinary heartbeats, not
equal to heartbeat revisions. Membership, selection, target, detach, expiry and
invalidation rotate the association fence. An old revision never revives a lease.
Use the observation ID, receiver epoch and current association revision with
`agent focus OBS --expected-epoch E --expected-revision R --from-kitsune
[--execute] [--json]`. Focus retains exact endpoint/runtime checks, a final
receiver CAS and one checked `terminal.focus`. No acknowledgement, retry, native
input, legacy agent focus or pane-focus fallback occurs. Acceptance means a focus
update was accepted, not pixels displayed or an OS window raised.

The generic view can label a producer-reported selected local full-TUI root. It
never forwards raw identities, target pins or association revisions into generic
UI rows and adds no automatic click action. The receiver does no socket I/O while
holding its presentation projection lock. Frontend lifecycle/worker-root source
qualification is a separate Djinn responsibility, not proven by receiver tests.
