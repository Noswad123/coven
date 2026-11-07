# Read-only session-target list and inspection

```text
coven session list WORKSPACE --socket ABS_SOCKET [--runtime djinn] [--timeout 3] [--max-response-bytes 1048576] [--json]
coven session inspect WORKSPACE --socket ABS_SOCKET --pane PANE [--expected-terminal TERMINAL] [--runtime djinn] [--timeout 3] [--max-response-bytes 1048576] [--json]
```

Optional `--group GROUP [--member MEMBER]` and `--recognized-only` apply a pure
post-observation view, never a narrower scanner/authority boundary. Selected counts
are distinguished from original observed counts; aggregate declaration counts keep
their full-observation scope. Default no-filter reports are unchanged. See
[SESSION_VIEWS_CONTRACT.md](SESSION_VIEWS_CONTRACT.md).

The bounded foreground `session watch` command reuses these same inspections and
view options, without a daemon/state cache or permission to control native apps.
JSON watch output is JSONL samples followed by a watch-summary, not one inspection
object. See [SESSION_WATCH_CONTRACT.md](SESSION_WATCH_CONTRACT.md).

These commands observe **existing pane targets on one explicitly selected Kitsune
API endpoint** and display any matching Coven declarations. They do not enumerate
saved/native/headless sessions, prove interactive-agent identity, or maintain a new
session registry. One pane containing both Djinn CLI and UI processes is one target,
not two workers. No pane/agent count establishes distinct native executions.

Kitsune's legacy `agents`, `agent`, `agent_status`, `agent_session`, titles, tokens,
names and readiness flags are not interpreted. All JSON still passes existing
UTF-8/duplicate-key/nesting/collection/string/size safety checks, including unused
fields. Generic consumed fields and process schemas must remain valid.

## Process evidence, not lifecycle

The default/only interpreter is `djinn`, using the existing exact POSIX argv[0]
basename rule for `djinn` and `djinn-ui`. Output contains only canonical
PID/executable-family/rule matches, never raw argv, cmdline, terminal contents,
credentials or peer diagnostic messages. Substrings, process names, launcher
arguments, Bun/Node wrappers and missing argv cannot establish a match.

A valid nonmatch is **observed unknown**. It does not establish absence, an empty
shell prompt, failure or shutdown. A match can be a help/maintenance command or a
renamed/untrusted program; it is not binary authentication or verified UI mode.

Activity and attention remain `unknown`; native identity/lifecycle, worker
acceptance and job completion remain `unestablished`. `authorization: none`,
`dispatchable: false`, `native_catalog_complete: false` are explicit. There is no
native timestamp/freshness attestation or cached receipt lease. A read report is
not permission to launch, send, resume, stop or perform model/tool work.

Live attachments are **not opened**, even for matching members. A declared
attachment is shown as a boolean only. Use the separate `coven group status`
contract for explicit, expiry-scoped legacy-runner busy/unknown source evidence.
Native folder/status/watch/help probes and task/launch controls are not queried.

## Bounded read sequence and consistency

- WORKSPACE must have the existing groups workspace structure: regular `coven.json`,
  nonsymlink state directory, safe/valid `state/groups.json` if present. Missing
  group authority is valid/uninitialized, not an instruction to migrate/create it.
  Other task/action/roster stores are not health assertions or prerequisites.
- Socket is explicit, absolute and local; there is no default, discovery/autostart,
  private TUI socket, fallback endpoint or native executable argument. Existing
  user-owned/private Unix-socket checks and canonical parent/device/inode fences apply.
- One acceptance budget is `(0,30]` seconds, default 3. One aggregate API response
  allowance is `[1024,8388608]` bytes, default 1 MiB including frame terminators.
  Declaration files have their existing separate 1 MiB parser bound.
- List reads one generic snapshot, one `pane.process_info` per pane, and a final
  generic snapshot: **N+2 requests**, no extra per-pane snapshot fan-out. At most
  64 pane targets are accepted; larger lists fail `pane_limit`, never silently
  truncate. Inspect can select one target from a larger valid snapshot.
- Inspect selects only an exact current pane and optionally verifies a supplied
  terminal ID before its process query. A missing/replaced target fails; there is
  no alias/native-reference lookup or following to another pane.
- List requires its entire pane/terminal/workspace/tab mapping cohort to match
  before/after. Inspect requires its selected mapping to match; unrelated pane
  changes do not invalidate that selected observation. Array order, focus, revision
  and unused legacy metadata are not identity. Version/protocol context is fenced.
- Group declarations are read before and after observation; content token and schema
  version must match. Counter-only updates do not change declaration content and
  remain allowed. Joins use the final snapshot's declaration-location context.
- Endpoint/deadline are fenced after all reads/projection. Transport/schema failure,
  relevant drift or timeout yields no partial positive target/role list. No cached
  result is substituted. A failed view has `target_count: null`, not a claim of zero
  native sessions; a valid empty pane view has count 0 but still no complete catalog.

Matching mapping samples are **not an atomic native snapshot or process lease**.
PID reuse, ABA, same-UID pathname interference and changes after the last check
remain possible. Filesystem calls are not hard wall-time interruptible. Existing
Kitsune API dispatch may perform housekeeping/default workspace creation, so a
read-only client is not a guarantee of zero server-side housekeeping effects.

## Declaration joins and stale bindings

Roles join by exact stored endpoint path/device/inode **and** current terminal ID.
A same-terminal move can retain its declaration; the old pane-at-binding is only
historical display context, never used as a new action locator.

Each association identifies group/participant/member/role and its provenance:

| `binding_check` | Meaning |
| --- | --- |
| `matching_process_evidence` | Current canonical PID/family/rule sample matches the stored declaration; PID reuse/native identity caveats still apply. |
| `process_evidence_changed` | Declared target terminal exists, but process evidence differs; no automatic rebind or native attribution follows. |
| `legacy_evidence_not_revalidated` | Snapshot-bound metadata is a historical declaration; forbidden legacy runtime metadata was not consulted to revalidate it. |

Any stored native reference is shown only as **`declared_native_reference`**. It
is not a current native identity, verified resume selector, acceptance receipt or
permission. A null reference is not proof that no native session exists.

Same-path declarations with changed endpoint identity or a terminal not present in
the final pane snapshot are reported separately under `declarations.unmatched` as
`endpoint_identity_changed` / `terminal_not_observed`. They are not followed or
modified. Different-endpoint declarations are counted separately, not called stale.
Valid declarations for another target during inspect count as outside that
inspection, not missing native sessions. No declaration change is inferred from
current legacy-agent labels or an observed-unknown process classification.

## Report, exits and no writes

JSON uses `schema_version: 1`, contract `coven.session-inspection.v1`, explicit scope,
request/endpoint, local receipt timestamps and limitations. Local receipt times do
not make native facts current. Human list output is a compact target/declaration
table; inspect without `--json` pretty-prints the same structured report.

Exit 0: observed, including empty/nonmatch views. Exit 1: unavailable, including
invalid/changed group authority, target drift, transport/byte/time failures.
Exit 2: incompatible protocol (currently only public protocol 18 is supported).
Invalid CLI options/values fail validation before observation. No peer error text
or corrupt authority body is echoed as a diagnostic.

No state, cache, history, dashboard, binding, annotation, gate or lock is created,
acquired or updated. Reads remain available in approved-runtime-launch mode without
consulting its control or claiming its permissions. Existing launch/submission and
worker-readiness contracts are unchanged; new observation cannot authorize them.

## Validation

Offline tests use private sockets/authorities and forbid subprocesses, native
readiness/observers/live artifacts and mutating transports. They cover exact
requests, generic decoder independence, fixed bounds, mapping/content/schema/counter
drift, optional terminal pins, process declaration states, historical references,
unchanged records/locks, endpoint aliases/replacement, privacy and empty/unknown
semantics. Full suite: `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -q`.
