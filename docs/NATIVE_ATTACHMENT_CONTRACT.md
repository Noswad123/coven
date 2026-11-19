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
reported state UNKNOWN, freshness unknown and null activity age when no fresh
native status evidence exists. Attachment heartbeat alone is never activity.

The quiet-root increment keeps `qualify` pure, then separately samples the selected
root's actual local `Session.Service.get` and `SessionStatus.Service.get` via
`src/plugin/native-status-snapshot.ts`. Explicit busy/retry/idle readings are
independent native evidence, not hosting inference. The closed native service's
canonical idle default is native-owned semantics, not process liveness; sparse SDK
status-list absence is not a fallback. Sampling is at most once/second within
one total 1,900 ms query/queue/IPC budget, preserving immutable query-start time
rather than restamping completion or replaying a sample as fresh. Native status
setters commit and publish under a per-instance/per-session semaphore; unlocked
`get`/`list` support callback reads. This orders concurrent setters and protects
initial samples against newer callbacks. Selection change/unmount, deletion, reporter replacement/ambiguity and
newer native callbacks cancel or fence stale results. Only a successful fresh
query renews activity through the existing native event contract, not permission/
question pending occurrences, acknowledgements or outcomes. Failure updates no
activity; the 30-second ceiling still yields Unknown. This recovers quiet mounted
normal full-TUI roots, not all live Unknown causes. Unmounted/event-only/remote/
unavailable profiles have no automatic status heartbeat; alive never implies idle.

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

## Current View Membership

Normal Djinn publication keeps any unexpired selected-root producer lease, even
conflicting targets that refuse focus, or a valid manual association matching the
fact, or fresh non-ended native Working/Blocked/Idle activity within 30 seconds.
A quiet attached Unknown remains visible even when native queries fail. Each
independently current root stays visible, including an idle second root; typing
and human labels are not selection criteria and liveness never implies idle.

Unhosted ended, lifecycle-only Unknown, expired and historical v1 Djinn rows are
hidden, not deleted, stopped, successful or acknowledged. A quiet old reporter
without activity disappears five seconds after its last lease heartbeat; real
native activity may keep it for at most 30 seconds after that evidence. No label,
provider SID, PID or CWD replacement heuristic or opaque-ID merge is allowed.
CLI observations retain history, while generic `prefix+a` inspects published rows.

This filter is private to the normal publisher and runs before its 128-item cap.
Hidden Djinn history contributes no private limits/retirement notice or lost slot;
public aggregate retirement counts and non-Djinn notices remain unchanged. Public
observations and exported `run_with_projection` keep their six-field envelope and
retained history IDs, without `_publisher_*` hints. Legacy serve and other-provider
display policies do not change. Cached models, name I/O and encoded frames cannot
extend membership: deadline-floor, admission and epoch checks reach the transport
pre-send boundary after encoding, connection and endpoint fencing. Only safely
Unsent refresh-needed frames rebuild on the same owner/candidate revision;
attempted or uncertain writes never retry or close. These checks and rendering
renew no leases or evidence clocks. No new Kit/Djinn source/API/version is needed.

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

The generic view uses a human name only: provider default (e.g. Djinn), preferring
a valid associated explicit generic `pane.manual_label`, already set or renamed
now. OSC title/CWD/old `agent_name` are not manual identity. Fresh association,
exact endpoint binding and live terminal runtime generation must pass; failure
uses the provider default. Duplicate names have no ID/suffix disambiguation.
Stable opaque row IDs remain internal selection identity, not session IDs or
display labels. Normal v1 is exactly `{id,label,detail}`; v2 adds blank
`presentation.label` and unchanged status enum for icon/animation-only sidebar
status. Idle/Ended intentionally share the neutral icon; detailed states and full
`Status: running`/`Status: neutral` remain in the `prefix+a` inspector. Nonempty
generic third-party captions still render.

Publisher-only private hints are not public raw targets. Name lookup is a bounded
300 ms read with no blocking receiver mutex; it creates no association/focus/input/
acknowledgement and refreshes no activity or evidence authority. The generic view
never forwards raw identities, target pins or association revisions into generic
UI rows and adds no automatic click action. The receiver does no socket I/O while
holding its presentation projection lock. Frontend lifecycle/worker-root source
qualification is a separate Djinn responsibility, not proven by receiver tests.

## Current Increment Record

The Coven-only stale-row increment is tracked in the
[gate record](../tools/normal-workflow-check/README.md#current-stale-row-increment).
Existing user rows were not live-inspected; this is not installed activation and
does not require new Kit/Djinn builds.

Historical name/icon/quiet-root review and qualification are **QUALIFIED**. Final Kit passed
2,578 binary tests (five ignored), 193 integration tests across 11 targets and one
explicit real-Coven roundtrip. Separately, the latest unified run passed 19 sidebar,
32 normal, four adoption, 16 hosting/status-sampling gates, six Coven presentation
integration tests, 127 filtered agent-management units, 26 Djinn tests in three files
and one actual-source interop fixture. After the subsequent type-binding-only fix,
full-package installed tsgo exited 0 with zero errors and broader Djinn validation
passed 84 tests in nine files. Independent review confirmed no P1/P2 after the
semaphore race fix. The unified pipeline was not rerun after the binding fix;
see the [gate record](../tools/normal-workflow-check/README.md#sidebar-qualification)
for separate results and bootstrap/interop limits.

Owned children stopped/reaped; cached runs were empty and no artifacts retained.
No full CI/full Coven suite, new provider/mobile/remote or actual model qualification.
The source addresses quiet/resumed selected-root initial and expiry gaps, but the
two existing user Djinn instances were not live-inspected; their current Unknown
cause and resolution are not established. No binary/wire version change, install
or configuration edit; activation of that historical cross-app baseline requires
new Djinn/Coven/Kit builds and explicit user approval for restart/new-pane/provider
handoff, unlike the Coven-only follow-up.
