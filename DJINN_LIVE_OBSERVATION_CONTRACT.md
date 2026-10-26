# Djinn live observation contract

## Overview

`coven.djinn.live-observation.v1` reads one explicitly pinned private artifact from
the optional Djinn legacy full-TUI backend worker. Native contract:
`djinn.live-observation.v1`. This is source-first **busy-only** evidence of actual
legacy `Runner.busy` ownership: true means `busy`, false means `unknown`, never idle.
Sparse status maps and persisted session records are not the source. Unobserved
never means idle, stopped, completed, or ready.

Coven does not launch/start/relaunch Djinn, open a listener/HTTP connection, scan
for files, fall back to folders/status/list/watch, or read auth/config. The one-shot
observer performs no group enrollment, readiness upgrade, persisted-adapter
integration, worker dispatch, or state write. The separate explicit
[group live workflow](GROUP_LIVE_STATUS_CONTRACT.md) freshly invokes this reader
for existing-member attachment and read-only status; it never imports saved reports.
The existing [native folder observer](DJINN_OBSERVATION_CONTRACT.md) is separate
and unchanged: persisted `running` is not reinterpreted as current activity.

## API / Interfaces

```text
coven observe djinn-live --file ABS_RUNTIME_UUID_JSON --runtime UUID --instance UUID --session ID [--timeout 1 --json]
```

All four selectors are required and validated before artifact I/O. `--file` is an
explicit absolute UTF-8 path, at most 4096 bytes, without `..`, controls, or a
trailing slash; its basename must be exactly `<runtime UUID>.json`. Runtime and
instance are canonical lowercase RFC 4122 version-4 generation UUIDs. Session is
opaque ASCII `[!-~]{1,128}`, not a folder/native agent lookup. `--timeout` defaults
to 1 second and must be finite in `(0, 1]`: an acceptance deadline, **not** a way
to interrupt blocked filesystem syscalls. `--json` emits compact ASCII-escaped JSON;
otherwise the same report is indented JSON.

The source's closed **14-field** top-level contract is:

| Field | Required value / meaning |
| --- | --- |
| `schema_version`, `contract` | `1`, `djinn.live-observation.v1` |
| `runtime_id` | Pinned worker generation UUID |
| `sequence` | Positive safe integer; ordering metadata only |
| `sampled_at_ms` | Nonnegative safe integer, source wall-clock milliseconds |
| `valid_for_ms` | `3000` |
| `source`, `activity_scope` | `legacy_session_run_state`, `legacy_runner_ownership` |
| `capabilities` | Exactly seven fixed booleans, below |
| `authorization`, `dispatchable` | `none`, `false` |
| `native_lifecycle`, `job_completion` | Both `unestablished` |
| `instances` | Array of exactly `{instance_id, sessions}` objects |

Capabilities: `activity: true`; `idle`, `catalog_complete`, `decision_state`,
`retry_state`, `durable_lifecycle`, `job_completion`: all `false`.
Sessions are exactly `{session_id, activity}`; activity is `busy|unknown`.
Limits: 65,536 bytes, 16 unique instances, 128 sessions globally, unique session
IDs per instance, integers no larger than `2^53 - 1` (not booleans).

The report has exactly these top-level fields (no raw artifact/other IDs):

| Fields | Meaning |
| --- | --- |
| `schema_version`, `adapter`, `adapter_contract` | `1`, `djinn_live`, `coven.djinn.live-observation.v1` |
| `file` | `{selected_path, resolved_path, device, inode}`; unresolved values null |
| `selection` | `{runtime_id, instance_id, session_id}` from explicit selectors |
| `started_at`, `observed_at`, `duration_ms` | UTC receipt times and monotonic read duration |
| `assessment`, `diagnostics` | `observed|unavailable|incompatible`; stable diagnostic codes |
| `activity`, `activity_scope`, `attention` | `busy|unknown`, `legacy_runner_ownership`, `unknown` |
| `freshness`, `expires_in_ms` | `fresh_sample|unknown`; conservative remaining lifetime |
| `source` | Selected validated evidence only, or null |
| `authorization`, `dispatchable` | Always `none`, `false` |
| `native_lifecycle`, `job_completion` | Always `unestablished` |
| `limitations` | Trust, freshness, scope, timeout, and no-authority caveats |

`source` contains only `contract`, `runtime_id`, `sequence`, `sampled_at_ms`,
`valid_for_ms`, `source`, `activity_scope`, `capabilities`, `instance_id`,
`session_id`. It contains no other instance/session IDs or full artifact.

## Data Flow

1. On supported POSIX systems, check the current-UID parent directory's exact
   `0700` mode and nonsymlink leaf, resolve ancestor aliases, and pin device/inode.
2. Require a current-UID regular artifact, exact `0600`, link count 1, within the
   byte cap. Open no-follow/nonblocking; compare FD/path device, inode, size,
   mtime and ctime. Recheck parent alias resolution and identity.
3. Decode strict bounded UTF-8 JSON: reject duplicate keys, nonfinite values,
   excessive nesting (over 32), unknown/missing fields, wrong types and bounds.
   Validate **all** instances/sessions, even unselected ones, before selection.
4. Recheck FD/path stamps and parent identity after decode. Reject changed files,
   wrong runtime, or absent pinned instance/session without following replacements.
5. At final acceptance, reject elapsed timeout and future source timestamps.
   Compute `ageAtEnd = finalWallTimeMs - sampled_at_ms`, then
   `expires_in_ms = floor(3000 - ageAtEnd - elapsedReadMs)`. Nonpositive means stale.
   This deliberately double-counts read latency conservatively, not a new receipt TTL.

Exit **0** means observed, including a valid selected `activity: unknown` entry.
Exit **1** means unavailable (missing target, stale/future sample, unsafe/changed
path, I/O/schema failure, timeout, unsupported platform). Exit **2** means
incompatible/unsupported contract. Invalid CLI arguments are errors before I/O.
Unavailable/incompatible reports retain unknown activity/freshness, `source: null`,
and `expires_in_ms: 0`; no positive cache or inferred stopped state is returned.

## Edge Cases

- Pins do not auto-follow reload/restart. Each actual worker gets a new module
  default-registry runtime UUID; each normal RunState instance initialization gets
  a new instance UUID. Settlement removes only its registration; disposed instances
  drop before automatic cleanup. Missing/rejected registrations remain unknown;
  capacity-rejected handles cannot later acquire ownership.
- Producer startup requires an existing private directory, with no default path
  or `mkdir`. Samples precede async I/O; 1000 ms nonoverlapping heartbeat scheduling
  and 3000 ms TTL do not prove health. First publication uses exclusive linking,
  replacements atomic rename with identity checks; transient two-link files fail closed.
- Publisher errors permanently stop publication with bounded warnings, not runtime
  failure. Close marks stopped first, lets normal teardown proceed, then awaits
  closing in `finally` with a maximum 5-second filesystem wait. Issued operations
  may commit later; only a surviving worker can attempt identity-checked cleanup.
  Old timestamps still expire. No unconditional cleanup or syscall-cancel guarantee.
- Crash-stale/unknown files are not garbage-collected; operator cleanup is manual.
  Same-UID TOCTOU and trusted ancestors remain: modes/stamps are not authentication
  or a sandbox against malicious same-UID writers.
- Shared same-host wall clock and suitable local filesystem are assumptions.
  Rollback can make replay seem younger; stateless sequence checks cannot detect
  all replay. Sequence is ordering metadata, not authentication. Touch/copy mtime
  does not retimestamp JSON. Returned/saved reports must expire without I/O refresh;
  cached-report reads cannot renew freshness.
- Group attachment stores only declared path/pins and UTC declaration time in
  schema 3 of `state/groups.json`, not source activity/sample/sequence/TTL. Its
  `attached_at` has no freshness meaning; commit may outlast the accepted read lease.
  Group status reduces all row lifetimes again after all reads and its content
  recheck; callers must still deduct delivery time. Attachment does not verify a
  terminal/native UI-session association, and shared pins do not prove distinct
  workers. The existing static publisher never reads attachments or appends activity;
  its 6000 ms role-label TTL must not replace the source's 3000 ms lifetime.
  Separate explicit [`publish --live`](GROUPS_CONTRACT.md#opt-in-live-publication)
  uses generic Kitsune `valid_until_unix_ms` discovery/enforcement, original source
  age and decreasing wall/elapsed budgets; unknown is role-only. It never grants
  three fresh seconds from report receipt and assumes compatible local clocks,
  not arbitrary rollback/skew/suspend safety. No activity or lease is persisted.
- `fresh_sample` is not provider progress, health, global activity/idle, readiness,
  lifecycle or completion. Mini/serve/browser/current-V2 execution, decisions,
  retries, durable lifecycle and job completion are unsupported. No HTTP/public
  protocol generation change is needed for this file contract.

## Example usage

The user may manually create an unused private folder **outside app repos**, then
enable publication at normal startup of a **new feature-bearing UI build**:

```sh
mkdir -m 700 "$HOME/djinn-live-private"
DJINN_LIVE_OBSERVATION_DIR="$HOME/djinn-live-private" \
  /absolute/path/to/feature-build/djinn-ui
# Explicitly inspect a chosen UUID-named artifact and copy its actual IDs first.
coven observe djinn-live --file /absolute/private/RUNTIME_UUID.json \
  --runtime RUNTIME_UUID --instance INSTANCE_UUID --session SESSION_ID --timeout 1 --json
```

Placeholders must be replaced. No autodiscovery scanning is performed.
`DJINN_UI_BIN` may select an old installed binary via Djinn: ensure the selected
UI contains the feature. There is no automatic installer; installed binaries stay
untouched. Reading an unenabled existing runtime never relaunches it; do not
restart a user's app to enable observation.

Historical live-observer baseline: full Coven suite **672 passing**, including 27 live tests; producer
**54 focused opencode tests** (including unchanged Runner/RunState/shutdown
regressions), **3 schema tests**, and both Bun typechecks pass. Existing prompt/task
cancellation coverage passed 78 tests with 1 existing skip; not the full UI suite.
The broader Schema test directory has an existing event-manifest mismatch
(55 expected server events versus 58, plus fixed event indices). Its tracked source,
barrel and test files are unchanged; the new artifact schema is not in that public
event inventory. The isolated count test reproduces the failure. It is recorded as
an out-of-scope baseline failure, not treated as a green full Schema suite.

Production TypeScript artifacts -> actual Coven CLI passed **13 fixture cases**,
including busy/unknown, missing/changed instance, stale/future timestamps, idle
rejection, unselected corruption, unsafe permissions and producer close. Source
artifact cleanup was verified. Evidence: `/tmp/opencode/djinn-live-observe-g3qvy1XV/`.
Synthetic owners were used for cross-language fixtures; real SessionRunState was
covered separately in focused tests. No existing app restart or model request;
installed UI/provider activity has not been live-validated.

## Related files

- `runner/djinn_live_observation.py`, `runner/coven.py` — reader and CLI.
- [GROUP_LIVE_STATUS_CONTRACT.md](GROUP_LIVE_STATUS_CONTRACT.md), `runner/group_live.py` — explicit attachment/read-only status; live publication is a separate opt-in publisher, never automatic enrollment.
- `runner/kitsune_observation.py` — shared bounded JSON decoder, not a socket fallback.
- [Producer guide](../djinn/docs/live-observation.md) — optional sibling navigation;
  links source Schema, registry, publisher, RunState, Runner and worker lifecycle.
