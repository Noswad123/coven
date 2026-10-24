# Kitsune observation contract

## Overview

`runner/kitsune_observation.py` implements initial, opt-in observation of one
caller-selected **existing** Kitsune API socket. It is a compatibility bridge,
not extraction of Kitsune's built-in agent behavior or a native lifecycle or
reliability attestation. Grouping and foreground annotation publishing are separate
implemented paths; see [GROUPS_CONTRACT.md](GROUPS_CONTRACT.md).

The default adapter sends only the existing `session.snapshot` read method. The
opt-in `--process-pane` mode uses a separate
[process observation contract](KITSUNE_PROCESS_OBSERVATION_CONTRACT.md) and report;
the default behavior documented here is unchanged. **This does
not promise zero server-side effects:** although Kitsune's snapshot builder takes
`&self`, its existing dispatch pipeline expires metadata, drains internal events,
synchronizes titles, and may ensure a default workspace in an empty session,
potentially spawning a shell. The adapter cannot prevent that housekeeping.

## API / Interfaces

```python
inspect(socket_path: str, *, timeout: float = 3.0,
        max_response_bytes: int = 1048576) -> dict
```

CLI surface:

```text
coven observe kitsune --socket ABS_API_SOCKET [--timeout SECONDS] [--max-response-bytes BYTES] [--json]
```

There is no workspace argument, default endpoint, environment-based selection,
discovery, CLI/subprocess invocation, autostart, ping, event subscription, or
generic method dispatch. No snapshots are stored and no Coven state, logs, locks,
dashboards, or group membership are written. Display/observation grants no input,
start, resume, or stop authority.

Arguments are rejected before connection: socket path must be absolute, without
NUL, surrogates, `..` components, or trailing slash, and at most 4096 encoded bytes;
timeout must be finite in `(0, 30]`; response budget must be an integer in
`[1024, 8388608]`. Python caller errors raise `ValueError`; the CLI converts these
to `StorageError` (exit 1). CLI argument-parsing errors retain usual exit 2.

### Result envelope

| Fields | Meaning |
| --- | --- |
| `schema_version`, `adapter`, `adapter_contract` | `1`, `kitsune`, `coven.kitsune.observation.v1`; a Coven-local output contract, not a new external schema. |
| `endpoint` | Caller-selected path, resolved path, device and inode (unknown values remain null). Locators, not authenticated server identity. |
| `request` | `{"method": "session.snapshot"}`. |
| `assessment`, `diagnostics` | `observed`, `unavailable`, or `incompatible`; bounded diagnostic codes, not raw server error text. |
| `started_at`, `observed_at`, `duration_ms` | Local UTC timestamps and elapsed monotonic time; `observed_at` is also set on failure. |
| `authorization`, `dispatchable` | Always `none`, `false`. |
| `runtime_fact_freshness`, `server_snapshot_timestamp` | Always `unknown`, null; receipt time does not date native facts. |
| `agents`, `pane_count`, `limitations` | Projected agents, total snapshot pane count, and explicit housekeeping/identity/freshness caveats. Nonobserved reports retain `[]` and null. |

Each agent has provenance `kitsune.session.snapshot`, endpoint-scoped `locators`
(`pane_id`, `terminal_id`, `workspace_id`, `tab_id`), `raw_status`, `activity`,
`attention`, `revision`, `focused`, `state_change_seq` (default 0), and flags
`launch_pending`, `interactive_ready`, `screen_detection_skipped` (default false).
Optional `agent_kind`, `name`, and `native_session` preserve available source
values. Native references retain `source`, `agent`, `kind` (`id` or `path`), and
`value`; they may be persisted references, not evidence of a running session.
Titles, cwd, tokens, layout, and full snapshots are not returned.

`done` maps to activity `idle` and attention `unacknowledged`; `blocked` maps to
`needs_attention`. Other statuses retain their activity and attention `unknown`.
`native_lifecycle` and `job_completion` are always `unestablished`. No native
deduplication, participant identity, group membership, success inference, or
acknowledgement follows from labels or pane presence.

CLI exits: **0** observed (including a valid empty inventory), **1** unavailable,
**2** incompatible. None establishes runtime success, failure, or shutdown.

## Data Flow

```text
explicit existing API socket -> one session.snapshot request
Kitsune -> bounded validated response -> Coven-local observation envelope
```

The adapter uses AF_UNIX directly, sends a generated request id and empty params,
and requires one newline-terminated response followed by EOF, with no trailing
data and a matching id. The overall deadline covers connection, send, receive,
and a final post-validation deadline check, rather than restarting per chunk.
It does not retry or fall back to another endpoint.
One connection uses the shorter filesystem-byte encoding of the selected and
resolved pathname. This avoids parent-symlink expansion overflowing the Unix-socket
address limit without changing the canonical endpoint identity. Each fence also
re-resolves the selected parent, rejecting alias retargeting.

Only snapshot protocol **18** is accepted. This is an adapter compatibility limit,
not a universal cross-app lifecycle requirement. Required inventories and known
fields are validated; additive fields are allowed within bounds. Agents must
match corresponding pane locators/status/reference fields; duplicate pane or
terminal ids within either inventory are rejected.

Independent optional path (implemented separately, not required by observation):

```text
user-enabled annotation producer -> generic bounded annotations -> Kitsune
```

See [APP_BOUNDARIES.md](APP_BOUNDARIES.md) for user/Kitsune control of presentation.

## Edge Cases

- Socket leaf must be a real Unix socket owned by the current uid with no
  group/other permission bits (normally mode 0600), not a symlink. Parent aliases
  are resolved (for example macOS `/var`); parent directories remain trusted.
  Device/inode are checked before/after connection and after receipt. These checks
  do not authenticate the peer, prevent all path races, or establish stable server
  identity. Native references and names may be sensitive; handle output accordingly.
- Default response budget is 1 MiB, maximum 8 MiB. UTF-8 and JSON must be valid;
  duplicate keys, nonfinite numbers, surrogate strings, excessive nesting (>32),
  objects/arrays over 4096 entries, and overlong strings are rejected. Strings
  generally allow 4096 characters, selected path/value fields 8192; each top-level
  inventory allows at most 1024 entries.
- Missing/unsafe/replaced sockets, I/O/timeouts, server errors, framing, decoding,
  or schema failures yield `unavailable`; a valid non-18 protocol yields
  `incompatible`. Agents remain safely empty, not cached active observations.
  These assessments do not infer native runtime failure or absence of sessions.
- A received snapshot can contain stale native facts. Pane loss and empty lists
  do not prove termination; built-in source authority is not reliability attestation.

## Example usage

Only after explicitly choosing an existing, trusted API socket:

```bash
coven observe kitsune --socket /absolute/path/to/existing-api.sock --json
coven observe kitsune --socket /absolute/path/to/existing-api.sock \
  --timeout 2 --max-response-bytes 1048576
```

Examples are not live smoke-test instructions; no live connections were made for
this documentation update. Offline fixture validation is owned by the implementation.

## Validation

The focused observation/CLI suite passes 31 tests using private fake endpoints,
including fragmented responses, deadline/framing failures, native-reference and
status semantics, workspace preservation, and the standalone wrapper. The full
Coven offline suite now passes **599 tests**, including process observation/group
workflows and 10 socket-alias transport regressions. Offline fixtures query no live
endpoint. Diff whitespace checks pass. Separate
[live group validation](GROUPS_LIVE_VALIDATION.md) exercised the transport fix and
installed Linux workflow; neither establishes universal runtime compatibility or
native lifecycle guarantees.

## Related files

- [runner/kitsune_observation.py](runner/kitsune_observation.py) — implementation
  (standard-library imports only; no local types or runtime adapters).
- [runner/coven.py](runner/coven.py) — CLI integration.
- [tests/test_kitsune_observation.py](tests/test_kitsune_observation.py) — private
   fake-endpoint usage and adversarial fixtures.
- [tests/test_observe_commands.py](tests/test_observe_commands.py) — CLI, wrapper,
  explicit selection, authority independence, and workspace preservation.
- [README.md](README.md), [APP_BOUNDARIES.md](APP_BOUNDARIES.md).
- Optional sibling navigation, not a dependency:
  [Kitsune inspection](../kitsune/AGENT_TRACKING_INSPECTION.md),
  [snapshot builder](../kitsune/src/app/api/session.rs),
  [API types](../kitsune/src/api/schema/session.rs),
  [dispatch](../kitsune/src/app/runtime.rs),
  [API handling](../kitsune/src/app/api.rs).
