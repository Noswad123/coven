# Kitsune agent comparison contract

## Overview

Native Coven implements opt-in comparison of Kitsune's built-in hints with Coven's
generic Djinn executable-family evidence at one explicitly selected endpoint.
This **characterizes extraction; it does not complete extraction** or establish
replacement equivalence. Kitsune's built-ins remain enabled and unchanged.
No scheduler is required for this integration work.

## API / Interfaces

Reference syntax only; uppercase values are placeholders, not run-session instructions:

```text
coven observe kitsune --compare-agents --socket ABS_SOCKET [--timeout 3] [--max-response-bytes 1048576] [--json]
```

`--compare-agents` conflicts with `--process-pane`. The library exports
`kitsune_comparison::Request { socket: String, timeout: f64,
max_response_bytes: usize }`, `inspect(&Request) -> Result<serde_json::Value>`
and `arguments(&Request, json: bool) -> Result<TaskOutput>`. `src/lib.rs` exports
the module; `src/main.rs` routes only the explicit comparison flag to it. Existing
default strict observation and one-pane generic process observation stay unchanged.

- Socket selection is explicit and absolute; no default endpoint or discovery.
- Timeout is finite, in **(0, 30] seconds**; default 3 seconds.
- Aggregate response wire budget is **1 KiB–8 MiB** inclusive; default 1 MiB (1048576).
- Output defaults to two-space pretty ASCII-escaped JSON; `--json` selects compact
  ASCII-escaped JSON. The CLI adds one terminal LF; no special human row display.
- Invalid request arguments fail before socket collection, not as observed reports.

The report has `schema_version: 1`, `adapter: kitsune`,
`adapter_contract: coven.kitsune.agent-comparison.v1`, and
`scope: same_endpoint_pane_family_evidence`. Endpoint provenance includes selected
and resolved socket paths plus device/inode when available. Local `started_at`,
`observed_at`, `duration_ms` and `timestamp_basis: local_receipt` are not server time.

Successful `rows` retain the initial pane order. Each row contains:

| Field | Meaning |
| --- | --- |
| `locators` | Nested `pane_id`, `terminal_id`, `workspace_id`, `tab_id`. |
| `kitsune.before`, `kitsune.after` | Each has `listed` boolean, `reported_family`, `status`, `native_reference_present` boolean. |
| `reported_family` | `djinn`, redacted `other` for any other family, or null. |
| `status` | Exactly `idle`, `working`, `blocked`, `done`, or `unknown`. |
| `coven.recognition` | `djinn_executable_family` if matches exist; otherwise `unknown`. |
| `coven.matches` | Ascending PID/family/rule records only: `pid`, `executable_family`, `rule`. |
| `family_comparison` | One of the seven categories below. |

`target_count` is the pane count, not an agent count. `counts` contains all seven
keys, including zeros. An observed empty cohort has `target_count: 0` and zero
counts, not null counts.

| Category | Condition after stable private legacy projections |
| --- | --- |
| `both_djinn` | Pane reports Djinn, is listed in agents, and generic evidence matches. |
| `generic_only` | Generic match without the listed built-in Djinn condition or another family hint. |
| `builtin_only` | Listed built-in Djinn condition holds, but generic evidence does not match. |
| `neither` | Neither listed built-in Djinn nor generic match nor other-family hint. |
| `other_builtin_family` | Another built-in family hint, no generic Djinn match; out of scope, not a regression. |
| `conflicting_family_hints` | Another built-in family hint plus a generic Djinn match. |
| `legacy_changed` | Private pane or matching agent projection changed; takes precedence over all other categories. |

## Data Flow

1. Validate arguments, establish one shared deadline, resolve/select the private
   socket, and create one response budget shared by every frame (including LF).
2. Read a **strict** `session.snapshot {}`: protocol 18, version, full legacy schema
   and all four opaque locators. There is no strict-to-generic fallback.
3. Reject more than **64 panes** with `pane_limit` before any process query.
4. In initial pane order, read one `pane.process_info {pane_id}` per pane.
5. Read a second strict snapshot. Require the same version and full pane-to-four-
   locator mapping. Pane ordering alone may change.
6. Compare private projections, project safe facts, count categories, and recheck
   cancellation/deadline before publishing any rows.

Success uses exactly **N + 2 queries**, including two snapshots for zero panes.
Only `session.snapshot` and `pane.process_info` are allowed. Endpoint selection,
reply endpoint and subsequent endpoint checks must agree throughout. Shared framing
requires correlated IDs, expected result types, LF/EOF and no trailing data.
The deadline includes collection/validation/projection; synchronous filesystem
calls are not a universal interruptible wall-clock guarantee.

Recognition examines only structured `foreground_processes[*].argv[0]`, taking
the substring after the last POSIX `/`. Only exact `djinn` and `djinn-ui` match;
the rule is `exact_posix_argv0_basename`. No trimming, case folding, shell parsing,
name/cmdline fallback, path authentication or mode interpretation occurs.
Missing/empty argv and missing foreground-process lists yield unknown evidence.
Duplicate PIDs or malformed consumed fields reject the response.

`legacy_changed` compares full internal pane/matching-agent projections, including
native reference source/agent/kind/value, name, focus, revision, state-change sequence,
launch-pending, interactive-ready and screen-detection-skipped fields. A change can
therefore be reported even when exported before/after facts look identical. This
is not a bytewise comparison of every unused snapshot field. Raw reference/readiness
fields stay private; inspect the existing strict observer separately if needed,
under its separate disclosure contract, not through this report.

## Edge Cases

- Any collection/read/projection/final-fence failure discards **all** rows:
  `rows: []`, `counts: null`, `target_count: null`; no partial positives or cache.
- Unsupported protocol gives `assessment: incompatible`, exit **2**. Other
  collection failures give `unavailable`, exit **1**. Successful collection is
  `observed`, exit **0**, even with mismatches or `legacy_changed`. CLI interruption
  exits **130**. Diagnostics are safe local codes, never peer error text.
- Shared decoding validates the entire JSON before filtering, including ignored
  extensions: depth 32, bounded strings/collections, valid UTF-8, no duplicate keys,
  malformed JSON or nonfinite floats. Error frames are rejected, not observations.
  Ignored 400- and 4300-digit integers are accepted losslessly; 4301 digits exceed
  the integer profile and are refused. Integers are not converted through `f64`;
  consumed numeric fields retain their own exact integer bounds.
- No argv, cwd, screens, labels, native ID/path values or peer error text are
  exported. Unknown/other family names become `other`. Socket paths and opaque
  locators are intentionally retained provenance, not native references.
- Matching hints are not active-agent detection, identity, readiness, lifecycle,
  job acceptance/completion or worker-policy admission. Help/maintenance modes and
  spoofed executable names match too; missing hints do not prove absence.
  Kitsune `done` means idle and unseen, not task success or runtime shutdown.
- `authorization: none`, `dispatchable: false`; native identity/lifecycle,
  readiness, worker acceptance and job completion remain `unestablished`.
  `runtime_fact_freshness: unknown`, `replacement_equivalence: not_established`.
- Bracketing is not atomic: intervening mapping reversals, process replacement and
  PID reuse remain possible. Private ownership/device/inode checks are not peer
  authentication; parent paths must be trusted and replacement races remain.
- The client performs no writes, locks, workspace access, bindings, automatic
  following/rebinding/adoption, built-in disabling, input requests, probes or
  subprocesses. Reads can still trigger existing Kitsune housekeeping: metadata
  expiry, event drainage, title synchronization or default workspace creation
  (potentially spawning a shell). Zero server-side effects are not promised.

## Example usage

Inert syntax only: `ABS_SOCKET` denotes an explicitly inspected existing API socket.
These examples contact that endpoint if executed; they do not start a session.

```text
coven observe kitsune --compare-agents --socket ABS_SOCKET
coven observe kitsune --compare-agents --socket ABS_SOCKET --timeout 3 --max-response-bytes 1048576 --json
```

A `both_djinn` row means matching family evidence, not permission to replace
Kitsune's detector, bind a participant, resume a session or submit work.

## Extraction decision matrix

| Surface | Current decision / remaining gate |
| --- | --- |
| Panes and process evidence | Generic surfaces already exist; comparison characterizes only Djinn-family hints. |
| Command submission | Generic pinned `pane.submit_command` exists; submission is not worker acceptance. |
| Terminal annotations | Generic opt-in terminal-border annotations exist; broader workspace annotations remain a proposal. |
| Status authority and attention | Blocked: process names cannot replace runtime status, seen/unseen or attention semantics. |
| Native references and automatic restore | Blocked: persisted references, identity, readiness and restore behavior need separate characterization. |
| Provider resume and hook installers | Blocked: runtime/provider policy and installation effects are not covered by family comparison. |
| Windows agent-aware process selection | Blocked: characterize platform-specific selection before claiming generic parity. |

No removal until behavior is characterized per runtime and across supported
platforms. Next small step: characterize generic process selection, then isolate
pure provider policy behind compatibility wrappers, preserving current behavior.
This needs no scheduler and authorizes no removal, hook installation or UI change.

## Validation and provenance

The independent fixture records actual Kitsune **01d7cef6**, protocol-18 snapshot
producer/source inspection, not a speculative replacement schema. Its header pins
six inspected source SHA-256 hashes: `src/app/api/session.rs` and
`src/api/schema/{session,panes,agents,workspaces,tabs}.rs`; see
[the provenance record](tests/support/kitsune_comparison_fixture.rs). Fixtures are
local: no sibling Kitsune source dependency or installed interoperability claim.

Local validation passed **10 comparison unit tests, 29 workflow tests, 21 existing
observer tests, 86 CLI tests, 11 session-reader workflow tests, and the retained
79-case Group/Observe acceptance workflow**. These describe different test layers,
not additive Cargo-suite totals. Formatting, strict all-target Clippy, Rust **1.85**
all-target checking, the release build and dirty-source package verification pass.
This is scoped local evidence, not a full-CI or installed-server interoperability
claim. No real endpoint, UI or provider calls were used.

## Related files

- [APP_BOUNDARIES.md](APP_BOUNDARIES.md) — transition and independent app ownership.
- [KITSUNE_OBSERVATION_CONTRACT.md](KITSUNE_OBSERVATION_CONTRACT.md), [process contract](KITSUNE_PROCESS_OBSERVATION_CONTRACT.md) — separate unchanged modes.
- `src/kitsune_comparison.rs`, `src/kitsune_comparison/tests.rs` — report and units.
- `src/kitsune_observation.rs`, `src/kitsune_observation/schema.rs`, `src/observation_{socket,wire,time}.rs` — schema, transport and bounds.
- `src/{lib,main,session_support}.rs`, `tests/kitsune_comparison_workflow.rs` — wiring, output and independent workflow coverage.
