# Observation, repository facts and explicit groups

## Choose the evidence source

These interfaces observe or declare facts; they do not schedule or dispatch tasks.
The installed `coven` product is native Rust. Development snippets using
`target/debug/coven` select an already-built binary from the repository root.
Uppercase names and example paths are placeholders for inspected local values.

| Interface | Evidence or effect |
| --- | --- |
| `agent discover` | Ephemeral provider-family hints from generic foreground cohorts. |
| `observe kitsune` | Snapshot or explicitly selected Djinn-family process evidence. |
| `observe djinn` | Recorded folder metadata/events, not live status. |
| `observe djinn-live` | Explicit private artifact; legacy runner busy/unknown only. |
| `group add` / attach | Local membership/source declarations, not native identity. |
| `group status` | Read-only, revision-scoped projection of declared artifacts. |
| `group publish` | Explicit generic annotation writes, not terminal control. |
| Repository / readiness | Immutable checkout facts / help-only compatibility diagnostics. |

For normal agent invocation/reporting and selected-root integration, prefer the
[normal workflow](NORMAL_AGENT_WORKFLOW.md), not manual artifact grouping.

## Current Kitsune retirement changes the starting point

The 2026-11-16 [app-boundary overlay](../architecture/APP_BOUNDARIES.md#current-retirement-overlay-2026-11-16)
and [migration overlay](../plans/AGENT_MANAGEMENT_MIGRATION.md#current-global-retirement-overlay-2026-11-16)
govern current behavior: snapshots return `agents: []` **globally**. Kit's live
agent detection/control and all six fresh legacy installers retired on every
platform. Old references/names/argv remain data, not an active inventory.
Retirement does not qualify Coven macOS/Windows replacement capabilities.

**Historical snapshot binding:** `group add` without `--process-evidence` still
requires an exact entry in the legacy snapshot agent inventory. Older README and
group-contract quick starts describe that historical path. It cannot enroll from
the empty inventory of a current retired server. Do not treat it as the current
default workflow or infer agents from preserved references.

The supported explicit alternative below is narrowly Djinn-family process binding.
The broader 22-family discovery command has **no discovery-to-group binding or
saved-report import mechanism**. Its locators can be inspected, but discovery
results are not membership, native identity or execution authority.

## Coven-owned provider-family discovery

```text
coven agent discover --socket ABS_KITSUNE_SOCKET --json
coven agent discover --socket ABS_KITSUNE_SOCKET --pane PANE_ID --expected-terminal TERMINAL_ID --json
coven agent discover --socket ABS_KITSUNE_SOCKET --watch --duration 60 --interval 2 --json
coven agent discover --socket ABS_KITSUNE_SOCKET --watch --duration 60 --interval 2 --present --source coven-discovery --json
```

Discovery uses no workspace/default lookup, storage, Kit agent hints, receipt join
or group join. It recognizes all 22 literal families and retains every matching
PID/provider/rule without ranking or leader selection. Raw argv/title/CWD/native
references are not projected. Help/server/maintenance commands can match too.
Activity/attention stay unknown; lifecycle, identity and completion unestablished.

Protocol must be exactly 18 with an admitted generic process descriptor: Linux
v1 or source-enabled/provisional macOS v2 foreground-cohort/unclassified evidence.
macOS requires `darwin_procargs2_lp64_v1`; old macOS v1 and Windows reject.
Peer platform/PID labels grant no local OS authority or runtime certification.
Acquisition completeness remains unknown. Bounded bracket matching is neither
atomic attribution nor PID-ABA protection; omissions may be undetectable.

Watch is bounded/serial (duration `0.1..3600`, interval `0.1..60` seconds).
Presentation requires an already manually enabled generic Integration source,
interval at most two and timeout at most three seconds. Its five-second view
lease never renews stale positives while collection blocks; fresh unavailable
frames clear positives. Uncertain delivery stops UI calls, including close,
without retry/failover while core watch continues. Up to 128 rows contain one
summary and 127 matches with explicit omissions; selection is read-only, no focus.

One-shot exits: 0 observed, 1 unavailable, 2 incompatible. Watch exit 0 means
duration completed even through outages; interruption is 130, output failure 1.
See the [discovery contract](../architecture/AGENT_DISCOVERY_CONTRACT.md).

## Native bounded local observation

Repository inspection/registration, help-only readiness and all three `observe`
routes are native CLI operations. Their accepted Linux implementation profiles
do not establish universal parity, macOS runtime acceptance or packaging approval.
See [native observation details](../plans/RUST_MIGRATION.md#native-repository-readiness-and-persisted-djinn-observation)
and [retirement gates](../plans/RUST_RETIREMENT.md).

Trusted local execution uses bounded Git, advertised help/version routes and one
fixed persisted `session observe` call. Native supervision uses private self-exec
bootstrap in the same binary, not a Python helper. Executable pinning detects
ordinary changes, not authentication or sandboxing. Setup and hashing are not
interruptibly hard-bounded: probe budgets are not universal setup-inclusive
wall-clock guarantees. A trusted executable can ignore requested arguments.

### Persisted Djinn evidence

```sh
coven observe djinn --executable /absolute/path/to/trusted/djinn \
  --folder /absolute/path/to/session-folder --json
```

One explicit trusted executable receives one fixed native call with bounded
output and supervised cleanup. No workspace, PATH selection, help/status fallback,
enrollment or readiness upgrade. The Djinn build must contain `session observe`.
Recorded metadata/events are not live status, job success or authorization.
Coven exits 0 observed, 1 unavailable, 2 incompatible; native Djinn's generated
report exit 0 has a different meaning. See
[persisted observation](../architecture/DJINN_OBSERVATION_CONTRACT.md).

### Legacy Djinn live artifacts are a separate path

```sh
coven observe djinn-live --file /absolute/private/RUNTIME_UUID.json \
  --runtime RUNTIME_UUID --instance INSTANCE_UUID --session SESSION_ID --timeout 1 --json
```

Manually inspect one private artifact and its actual IDs; there is no scanning or
automatic following. This needs producer opt-in at startup of a new feature-bearing
**legacy full-TUI build**, not an old installed UI chosen by `DJINN_UI_BIN`.
No installer/restart/launch/HTTP/folder fallback or enrollment occurs. Fresh samples
establish legacy runner busy ownership only, never idle, health, progress or job
completion; cached reports cannot renew freshness. See
[live observation](../architecture/DJINN_LIVE_OBSERVATION_CONTRACT.md).

Normal native selected-root attachments are **not these artifacts** and are not
`group attach-live`. They are producer-backed local Djinn full-TUI display
associations through the normal receiver, with endpoint/terminal/incarnation pins
and a five-second lease. Heartbeats renew no activity; quiet attachment alone is
UNKNOWN with null activity age. Do not use a pane/PID/artifact to infer automatic
lifecycle claims or terminal/session equivalence. See
[native attachments](../architecture/NATIVE_ATTACHMENT_CONTRACT.md).

### Read the selected Kit endpoint

```text
coven observe kitsune --socket /absolute/path/to/existing-api.sock --json
coven observe kitsune --socket /absolute/path/to/existing-api.sock --process-pane w18:p85 --json
coven observe kitsune --compare-agents --socket ABS_SOCKET --timeout 3 --max-response-bytes 1048576 --json
```

Default observation reads one protocol-18 `session.snapshot`: no discovery,
subprocess, ping, events, autostart or state writes. Current `agents: []` is not
agent absence/failure proof. Historical `done` meant idle/unacknowledged, not
native completion; persisted references have unknown freshness.

`--process-pane` brackets one exact pane's `pane.process_info` with snapshots,
recognizing only literal `djinn`/`djinn-ui` argv0 basenames. Its `observations` are
not agents or participants. Generic brackets ignore malformed legacy metadata
semantics, not malformed JSON or shared resource limits. Default mode retains
strict legacy validation. `--compare-agents` conflicts with `--process-pane`;
observed mismatches still exit 0 and grant no replacement/readiness guarantee.
With current Kit, comparison's legacy inventory side is empty.

Read-only is **not zero server effects**: existing housekeeping may expire
metadata, drain events, sync titles or create a default workspace/shell.
See [snapshot observation](../architecture/KITSUNE_OBSERVATION_CONTRACT.md),
[process evidence](../architecture/KITSUNE_PROCESS_OBSERVATION_CONTRACT.md) and
[comparison](../architecture/KITSUNE_AGENT_COMPARISON_CONTRACT.md).

## Explicitly group a supported running participant

Use an existing Coven workspace, or separately initialize a **new dedicated
directory**, never an app repository or current workspace. Inspect the exact
existing pane and API socket first. This example deliberately uses the supported
Djinn-only process route, not the historical empty-inventory default:

```text
coven group create WORKSPACE auth-review --name 'Authentication review'
coven observe kitsune --socket ABS --process-pane w18:p85 --json
coven group add WORKSPACE auth-review --member backend --role 'Investigator' \
  --socket ABS --pane w18:p85 --process-evidence
coven group list WORKSPACE --json
```

Add reads fresh bracketed evidence itself; it does not import the prior report.
Changed stable evidence requires `--rebind`; optional `--expect-revision
'<current group token>'` fences current declarations. Roles are display-only.
There is no fallback, automatic enrollment/rebind or native execution grant.

Process add promotes `state/groups.json` schema 1 to at least 2 without reducing
schema 3. UUIDs/counters and `groups-v1` CAS format survive; older readers reject
the newer schema. Removal never downgrades. Mutations exit 0 committed/unchanged,
2 uncertain durability, 1 ordinary error; inspect before retrying, as there are no
durable exact-request receipts. List is source-free/read-only.

### Optional static border labels

First manually enable a dedicated annotation source in Kit, preserving config:

```toml
[annotations]
sources = ["coven", "build"]
```

```text
coven group publish WORKSPACE auth-review --socket ABS --source coven --duration 60 --json
```

Each cycle reobserves all process bindings before reservation/write. Missing,
unrecognized or changed evidence stops; same-terminal pane moves can remain valid.
PID reuse and help/maintenance matches remain possible. Pure-process groups use
generic snapshots; mixed/snapshot groups retain strict legacy snapshot-member
checks and cannot silently fall back when retired inventory no longer matches.
Publication still needs a valid annotation descriptor, enabled source and epoch.

Coordinate one dedicated source per workspace. Source order matters: Coven must
precede an already-claiming `build`. After a higher source claims a target, expiry
has **no lower-source fallback** until source-policy reset. Ctrl-C stops refresh,
not agents; static labels expire after 6000 ms, without exit-time clearing.
Spare top-border cells are required (single-pane layouts have no border).
Coven never edits/reloads/restarts Kit; labels confer no terminal control.
See [groups](../architecture/GROUPS_CONTRACT.md) and
[Kit annotations](../../../kitsune/ANNOTATIONS.md) for exact counters/quotas/fences.

Generic Integration rows are separate from border annotations: use the explicit
foreground [`group present`](../architecture/GROUP_PRESENTATION_CONTRACT.md)
workflow with manually configured `[integrations].sources`. Read-only selection
intents are neither native focus nor physical-pixel acknowledgements.

## Attach legacy live evidence and inspect group status

For an existing member only, use actual manually inspected source pins:

```text
coven group attach-live WORKSPACE auth-review --member backend \
  --file /absolute/private/RUNTIME_UUID.json --runtime RUNTIME_UUID \
  --instance INSTANCE_UUID --session SESSION_ID --timeout 1 --json
coven group status WORKSPACE auth-review --timeout 1 --json
coven group publish WORKSPACE auth-review --socket ABS --source coven --live --duration 60 --json
# Later, explicitly remove the declaration if desired:
coven group detach-live WORKSPACE auth-review --member backend --json
```

First attach promotes the same authority to schema 3; all members gain nullable
attachment metadata. Only declarations persist, not activity/TTL. Each attach
freshly reads the source then CAS-checks preflight revision and participant UUID
under lock even without a user token. Changed pins require `--reattach`;
unchanged pins preserve `attached_at`. Changed stable rebind clears attachment;
role/pane/timestamp-only changes preserve it. Detach performs no artifact I/O,
never deletes/controls the producer and never downgrades authority.

Status writes nothing and reads only groups plus declared artifacts. Individual
unavailable rows can still exit 0; whole-content drift, failed recheck or deadline
expiry discards all rows and exits 1. Returned lifetimes must keep decreasing
during delivery/display. Static publication never opens attachments.

Explicit `--live` requires literal `annotations.valid_until_unix_ms: true` every
cycle, even for unknown rows; unsupported servers fail without downgrade/write.
Busy text appends `legacy runner busy`, with a 3000 ms ceiling and absolute expiry
bounded by the original sample through queueing. Unknown/lost/expired sources
yield role-only text/6000 ms TTL. The whole busy frame expires; there is no hidden
static layer or lower-source fallback. Uncertain delivery/rejection stops without
retry or repair. No restart following, idle, progress, readiness or completion.

Clock compatibility and normal monotonic progression are assumptions: observed
regressions stop; unknown skew, suspend and hard-real-time pixel expiry are not
guaranteed. Exact CAS/expiry math belongs to
[group live status](../architecture/GROUP_LIVE_STATUS_CONTRACT.md) and
[live publication](../architecture/GROUPS_CONTRACT.md#opt-in-live-publication).

## Immutable repository registration

Register one existing checkout and independently reviewed full lowercase commit:

```text
coven repository inspect /path/to/orchestration --json
coven repository register /path/to/orchestration \
  --path /path/to/checkout --base '<full lowercase commit ID>' \
  --delivery-target refs/heads/main \
  --expect-repository-revision repository-unregistered:v1 --json
coven repository inspect /path/to/orchestration --verify --json
```

`state/repository.json` immutably pins checkout locators, commit/root tree and
intended target; dirty changes are allowed, excluded and never copied. Default
inspect invokes no Git/lock/fsync. `--verify` uses bounded local Git (2.48+) and
retains stored facts/revision if checkout access fails. Complete checkout
availability is not established. Exact retry retains the original syntactic
request/unregistered token without reopening Git; changed inputs conflict.
Exits are 0 committed/readable, 1 prepublication failure/unavailable inspect,
2 uncertain durability. No reset/replace/unregister, provisioning, task binding,
Git mutation permission or automatic rollout follows. See
[repository registration](../architecture/REPOSITORY_REGISTRATION_CONTRACT.md).

## Help-only native worker readiness

```text
coven worker readiness --adapter djinn --profile research-readonly --json
coven worker readiness /path/to/project --executable /trusted/path/to/djinn
```

This adapter compatibility diagnostic resolves/fingerprints trusted code and
follows advertised help routes, not native status/watch/list execution. It creates
no session, worker or Coven state/lock/log/dashboard. Ordinary project directories
are accepted. Credentials/configuration/policy and sessions are not inspected;
help execution is not a filesystem/network sandbox.

`djinn-help-v1` never declares `compatible` from help: exit 1 is blocked missing
required session/run surface, 2 unknown/probe failure or unverified guarantees;
0 is reserved for a future contract. Authorization remains `none`. Bounded output,
timeouts and supervised cleanup do not establish identity, acknowledgement,
lifecycle, cancellation or immutable tool policy. No `coven worker run` or runtime
startup fallback exists. See [readiness](../architecture/WORKER_READINESS_CONTRACT.md).

## Related source

[`src/main.rs`](../../src/main.rs) routes to `src/{repositories,workers,
djinn_observation,kitsune_observation,agent_discovery,group_commands,group_live,
group_publisher}.rs`. Process supervision lives in
[`src/process.rs`](../../src/process.rs) and `src/platform/unix/process.rs`;
group binding selection is in `src/group_commands/observation.rs`.
Historical validation remains in migration/evidence records, not fresh test totals
or installed native activation claims in this guide.
