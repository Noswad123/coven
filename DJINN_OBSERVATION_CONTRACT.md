# Djinn persisted-evidence observation

## Overview

`coven observe djinn` invokes one explicitly selected trusted binary to inspect
one folder capsule. This first native slice consumes persisted evidence only:
no live activity, native process binding, job acceptance/completion, authorization,
group enrollment, readiness upgrade, or dispatch follows. No Coven workspace is
required or written. Djinn remains independent of Coven and Kitsune.

## API / Interfaces

```sh
coven observe djinn --executable ABS_BINARY --folder ABS_FOLDER \
  --timeout 3 --max-output-bytes 131072 --json
```

Both paths are required, absolute UTF-8, at most 4096 bytes, without parent
components, controls or trailing slash. Executable aliases are resolved and
checked; folder leaf symlinks are refused by Djinn, while ancestor aliases resolve.
There is no PATH selection, discovery, version/help/status probe, retry, fallback,
saved-observation import, or capability inference. No native/UI ID selector exists.
Without `--json` the same report is pretty JSON; compact output is ASCII-escaped.
The selected binary must include the new native contract. Checkout validation used
`../djinn/target/debug/djinn`; installed binaries were not replaced or upgraded.

| Exit | Report assessment |
| --- | --- |
| 0 | `observed`: validated persisted evidence, not permission or success. |
| 1 | `unavailable`: native unavailable, transport/identity/validation failure. |
| 2 | `incompatible`: unsupported native contract/schema version. |

Invalid CLI requests/bounds are errors rather than observation reports. The local
`adapter_contract: coven.djinn.observation.v1` and `schema_version: 1` version
Coven's report, separately from native `djinn.session.observation.v1` version 1.
Djinn's native exit 0 means report generated, even when its assessment is unavailable.

The complete Coven report fields are:

| Field | Contract |
| --- | --- |
| `schema_version`, `adapter`, `adapter_contract` | 1, `djinn`, `coven.djinn.observation.v1`. |
| `executable` | `selected_path`, nullable `resolved_path`, nullable `identity`; identity has `device`, `inode`, `size`, `mtime_ns`, `sha256`. |
| `folder`, `request` | Submitted folder; `{method: session.observe}`. |
| `started_at`, `observed_at`, `duration_ms` | Local receipt interval and monotonic elapsed milliseconds, including hashing. |
| `assessment`, `diagnostics` | Assessment above and bounded safe diagnostic codes. |
| `authorization`, `dispatchable` | Always `none`, false. |
| `runtime_fact_freshness`, `activity` | Always `unknown`. |
| `native_lifecycle`, `job_completion` | Always `unestablished`. |
| `producer` | Null, or native `contract`, `started_at`, `observed_at`, `resolved_folder`, `capabilities`. |
| `observation` | Null, or validated evidence projection described below. |
| `limitations` | Four explicit trust/freshness/policy limitation strings. |

`producer.capabilities` is exactly `{persisted_folder_events: true,
live_status: false, ui_sessions: false, native_store: false}` (actual booleans).
Evidence contains `recorded_native_session_id`, `recorded_created_at`, `sources`,
`event_count`, `last_event`, `last_lifecycle`, and
`consistency: stable_per_file_not_transactional`. Source paths are exactly
`djinn.toml` and `events.jsonl`; each carries bytes and lowercase SHA256, with
events additionally carrying `status: observed|missing`.
`last_event` is null or `{kind, event_id, session_id, recorded_at}`;
`last_lifecycle` is null or `{state, mode, event_id, session_id, recorded_at}`.
See the optional [native contract](../djinn/docs/session-observation.md) for the
complete native JSON and parsing rules; a sibling checkout is not required to run.

## Data Flow

1. Validate input/bounds, resolve the selected executable and fingerprint its
   regular executable file: device/inode/size/mtime and SHA256, maximum 1 GiB per
   hash. Recheck alias/metadata before invocation and identity/alias afterward.
   Hash I/O is outside the subprocess deadline, not a total wall-clock bound.
2. Run exactly `[resolved_binary, "session", "observe", folder, "--json"]`, with
   cwd `/`, null stdin and only inherited `PATH`, `HOME`, `SHELL`, `LANG`, `LC_ALL`,
   `TERM`. Provider keys and Djinn/Kitsune configuration overrides are withheld.
3. Use the shared workers transport with an isolated Python supervisor, bounded
   combined stdout/stderr, private completion/liveness FDs and process-group cleanup
   on all completion paths, caller death and watchdog expiry. Defaults: 3 seconds,
   131072 bytes; accepted bounds: finite `(0, 30]` seconds, integer `[1024, 1048576]` bytes.
   The separate help-probe allowlist and help-only readiness behavior are unchanged.
4. Require native exit 0 and decode stdout with the shared strict UTF-8 bounded
   JSON decoder: duplicate keys rejected recursively, finite numbers, depth 32,
   containers 4096 entries, keys 4096 characters and strings normally 4096
   (8192 for the decoder's designated path/value fields). Unknown fields still
   pass these checks but are not projected. Raw stdout/stderr and exception text
   are withheld; safe codes replace producer errors.
5. Validate native contract, selected folder, normalized resolved path, timestamps,
   fixed capabilities/uncertainty/authority fields and all evidence types/invariants.
   Unknown contract/version is incompatible; malformed known data is unavailable.
   Native unavailable becomes `native_unavailable`, without forwarding its diagnostics.

## Edge Cases

- Native reads only required root `djinn.toml` (65,536 bytes) and optional
  `events.jsonl` (4 MiB). Strict TOML syntax evaluates only optional string native
  ID and creation time, not full manifest/config schema. JSONL allows 65,536 bytes
  per nonblank line, 16,384 records, depth 32 and 4096 entries per container.
- No leaf-root symlink, linked/hardlinked/nonregular artifacts, FIFO or directory
  sources. Anchored no-follow/nonblocking reads and before/after root/file checks
  include an absent events file appearing. Read stability is not atomicity or
  proof against a malicious concurrent writer; FUSE/atime effects remain possible.
- Missing events have null bytes/hash/count/last records; empty present events
  have zero bytes/count and the empty SHA256. Zero records require null last facts.
  Malformed/incomplete/oversize evidence is unavailable, never partial facts.
- IDs must be consistent `agt_` IDs (128-byte cap); event IDs are bounded and
  unique natively. Reject impossible projections: a non-lifecycle last record
  cannot reuse a prior lifecycle's non-null event ID. One-record lifecycle and
  last-record lifecycle metadata must also agree. No synthetic envelope/defaults.
- RFC3339 timestamp text is preserved, including Chrono fractional precision,
  within 64 UTF-8 bytes; receipt times require UTC. Record times are not freshness.
  Last explicit parent lifecycle follows file order, never child status, inferred
  `created`, or assistant-output success. Completion stays unestablished.
- Payload/content/tool args/results/reason/title/model are not returned. Native
  observation reads no `.djinn`, native/UI global stores, background markers,
  logs/cache/database/config/auth; it makes no subprocess/model calls or application
  writes. Existing status/list/watch can still probe/recovery-write: never fallback.
- **Trusted code, not sandboxing or authentication:** the selected binary can
  ignore argv, access files/network and inherited HOME/PATH, or deliberately escape
  process groups. Fingerprints detect ordinary changes, not every execution race.
  Coven does not import files, enroll participants, infer readiness, or dispatch.

## Example usage

```sh
coven observe djinn --executable /opt/djinn/bin/djinn \
  --folder /srv/sessions/review --json
```

Illustrative full report for an unavailable executable (not a live run):

```json
{
  "schema_version": 1, "adapter": "djinn",
  "adapter_contract": "coven.djinn.observation.v1",
  "executable": {"selected_path": "/opt/djinn/bin/djinn", "resolved_path": null, "identity": null},
  "folder": "/srv/sessions/review", "request": {"method": "session.observe"},
  "started_at": "2026-10-24T12:00:00+00:00",
  "observed_at": "2026-10-24T12:00:00.001000+00:00", "duration_ms": 1.0,
  "assessment": "unavailable", "diagnostics": ["executable_unavailable"],
  "authorization": "none", "dispatchable": false,
  "runtime_fact_freshness": "unknown", "activity": "unknown",
  "native_lifecycle": "unestablished", "job_completion": "unestablished",
  "producer": null, "observation": null,
  "limitations": [
    "Persisted evidence only: observation receipt is not event freshness, live activity, native runtime verification, job acceptance or success.",
    "The explicit executable is trusted, not sandboxed or authenticated. Fingerprints and alias checks detect ordinary changes, not every execution race. A selected binary can ignore arguments and access the inherited HOME/PATH.",
    "Executable fingerprint I/O (up to 1 GiB per hash) is outside the subprocess deadline. Provider keys and Djinn/Kitsune configuration overrides are withheld, but this is not native filesystem/network isolation.",
    "Folder/native IDs and file digests describe sourced persisted records, not a live process binding. Per-file stability is not a transactional snapshot. No group membership, worker readiness, policy or execution authority follows."
  ]
}
```

Native unavailable instead retains a validated producer envelope with null
observation and `diagnostics: ["native_unavailable"]`. An observed manifest-only
report has `folder_events.status: missing` and null count/last records. Record
timestamps such as `2026-10-24T12:00:00.123456789Z` retain precision, not freshness.

## Validation

The full Coven offline suite passes **645 tests**, including **28 new native
observation tests**. Existing readiness/transport tests remain green; static review
found no remaining blocking issues. Diff whitespace checks pass.

The newly compiled Rust producer and actual `bin/coven` wrapper passed **8 fixture
cases**, including missing/empty sources, null legacy timestamps, preserved extended
fractional precision, parent lifecycle versus child status, and unavailable results
for mismatched identity, unsupported event schema, partial tail and duplicate keys.
No source bytes, mtime/ctime or directory entries changed. Unusable config/runtime
artifacts were ignored, and no model request was submitted. Local evidence is at
`/tmp/opencode/djinn-native-observe-vD0NEKqJ/`; this is fixture compatibility, not
verification of a running agent, other platforms or an old installed build.

## Related files

- `runner/djinn_observation.py` — validation/projection and local report.
- `runner/workers.py` — fixed observation argv, identity and supervised transport.
- `runner/kitsune_observation.py` — shared bounded JSON decoder, not endpoint selection.
- `runner/coven.py` — CLI arguments, rendering and exits.
- [App boundaries](APP_BOUNDARIES.md), [worker readiness](WORKER_READINESS_CONTRACT.md) — separate authority contracts.
