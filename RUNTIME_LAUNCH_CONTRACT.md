# COV-03b: reviewed, single-use runtime startup

This optional mode authorizes **one concrete unrestricted COLD runtime startup
attempt**, not read-only research or task dispatch. Current pane adapters cannot
sandbox tools or establish native worker acceptance/session identity. OpenCode
can still read mutable files and use whatever tools its independent policy
allows—including mutation. Do not use this feature as a read-only guarantee.

Other capabilities (repository edits/commits, integration, push/merge/deployment)
receive no additional Coven grant here. Local checkpoint approval alone remains
insufficient: this boundary additionally verifies a frozen manifest and consumes
one durable claim. It supports cooperating upgraded Coven processes on one local
POSIX host, not authentication, hostile same-user edits, or distributed control.

## Explicit activation and review

Stop old binaries/direct-file writers and inspect/stop old sessions first. Mode
activation drains admitted cooperating legacy transports; it does not terminate
already-running agents or force older software/native commands to obey policy.

```bash
coven action enable-runtime-launch --json
coven action prepare-runtime-launch --json
coven action inspect '<action-id>' --json
coven checkpoint inspect 'runtime-launch-<action-id>' --json
coven approve 'runtime-launch-<action-id>' --expect-revision '<snapshot-token>' --json
coven action submit '<action-id>' --expect-manifest '<manifest-token>' \
  --expect-checkpoint '<approved-snapshot-token>' --expect-decision '<decision-id>' --json
```

Substitute actual IDs/tokens; an optional workspace path precedes the action ID.
Enable is persistent and idempotent: it does not reset identities or old claims.
There is no disable/reset/reactivation command. Unactivated workspaces retain
their existing behavior. Invalid control or orphan action artifacts fail closed.

Enabled workspaces reject legacy up/start/restart, agent start/restart/stop,
bootstrap and pane delivery, including workspace-aware shared helpers. Status
reports unverified runtime health rather than querying an unrelated human-name
container. Lead metadata can still be recorded, but legacy delivery is disabled.
Checkpoint decisions still commit even when their optional pane notices cannot
be delivered. Use action inspect for recorded transport facts, not worker health.

## One control authority, immutable prepared inputs

```text
state/runtime-launch-control.json      # activation + consumed claims/outcomes
state/actions/<action-id>.json         # immutable, bounded reviewed manifest
state/.runtime-transport.lock          # persistent admission gate, never force-delete
checkpoints/runtime-launch-<id>.md     # dedicated local decision authority
```

Activation allocates action-specific workspace/epoch IDs and binds canonical
local root, filesystem device, and inode. Copies/moves require future explicit
reactivation; no implicit remapping is performed. Task-ledger IDs/import remain
independent and unchanged.

The closed input inventory captures exact bytes/absence of:

- `coven.json`, `coven.toml`, `goal.md`, `spec.md`;
- task snapshot, agent state, and lead state;
- each roster agent's notes, role prompt, and starter prompt.

It additionally captures the authoritative task view/revision (including imported
task identities where applicable), ordered roster, effective multiplexer and
normalized command, `COVEN_MULTIPLEXER` selection, and transport environment from
`PATH`, `HOME`, `SHELL`, `LANG`, `LC_ALL`, and `TERM`. Arbitrary environment secrets
are not copied. A pending task import or unhealthy task projection must be
resolved first. Startup prompts must exist and be nonblank UTF-8.

Control revision and activation/location are bound separately by the manifest.
Generated dashboards, logs, the dedicated approval, mutable application source,
native provider configuration, binaries, shell startup files, and arbitrary
workspace artifacts are **not** frozen by this inventory. This is not registered
repository/base isolation, a pinned toolchain, a frozen filesystem, or a sandbox.

Preparation publishes the immutable bundle before its dedicated checkpoint.
An interrupted preparation can leave an unapproved orphan, never an executable
grant. The checkpoint covers action/identity/manifest digest and a readable
frozen transport plan with an unrestricted-runtime warning. Submission requires
its effective bound approval, exact snapshot and decision identity, and exact
generated action scope; editing/reapproving a different dedicated scope is not a
way to change the action. Prepare a new bundle instead.

The manifest does not contain its eventual approval snapshot/decision. The claim
records those after review, avoiding circular authority. Current files and
environment selections must still match the captured inventory at claim time.
Goal/lead writes, task mutations, and prompt refresh participate in the same
workspace lock. Quiesce manual/uncooperating edits; no filesystem-race sandbox
is claimed.

## Activation gate and claim linearization

Legacy transports take a shared persistent transport gate, recheck actual mode
after admission, and keep admission through their native operation. First
activation needs its exclusive gate. It cannot publish/return while a previously
admitted legacy operation can still issue native effects. Nested helpers reuse
validated tokens; a cached team flag is not authority.

The transport gate is acquired **before** the workspace lock. Legacy I/O may
hold its shared gate but never a global workspace lock just to wait for runtime
commands. Activation never holds the workspace lock while waiting for that gate.
Acquisition is bounded; busy activation fails instead of stealing admission.
Both lock files persist and kernel ownership is released on process death.

For a fresh action, submission takes the workspace lock, reloads authority and
inputs, verifies manifest/plan/approval, and atomically records consumption and
the in-flight slot in one control replacement with file/directory fsync. Only
after durable claim publication does it release the lock and execute the
**claim's frozen plan**, never live configuration or delayed starter-file reads.

- Input changes or revocation winning the lock before claim prevent startup.
- Changes/revocation after durable claim do not cancel that frozen attempt.
- Claim directory-sync uncertainty means **no execution**, even if a consumed
  claim is observed later. A retry never opportunistically starts it.
- Two submitters consume once. Repeated same-reference submissions return the
  existing claim/outcome without another executor call, even if inputs, approval,
  or prepared artifacts later change. Changed consumed references conflict.

## Transport and outcomes

The plan contains exact shell program strings, literal starter text, ordered
steps, cwd/environment and typed Herdr output handles. The planner/executor does
not re-read prompt/config files or ambient selection variables. The runtime
itself may read mutable files after launch. A fixed five-second startup delay is
not a readiness acknowledgement.

tmux uses an exact action-scoped session, control window and agent windows. Herdr
uses an action-scoped workspace label and typed created handles. Existing scoped
containers are not attached/adopted/replaced. All required CLI responses are
checked, with per-command timeouts. Partial resources are preserved, not cleaned
up or retried. Successful transport means neither task acceptance nor healthy
OpenCode execution.

| Outcome | Meaning |
| --- | --- |
| `transport_completed`, exit 0 | Required startup CLI commands succeeded. |
| `transport_failed_or_partial`, exit 1 | Attempt consumed; transport failed/partial and effects may be uncertain. |
| `claimed` or `outcome_unknown`, exit 2 | Consumed attempt unresolved; inspect, do not relaunch it. |
| `durability_uncertain`, exit 2 | Control publication/confirmation uncertain; startup was not attempted by that claim call. |

Outcome recording uses the same control authority, outside runtime I/O. If
transport ran but recording failed, the response includes the observed report
and says `outcome_unknown`; a lost response/crashed process never resets the
claim. History append failure warns after durable outcome; logs do not authorize
or reconstruct execution.

```bash
coven action inspect '<action-id>' --json
coven action acknowledge-unknown '<action-id>' --expect-claim '<claim-id>' --json
```

An unresolved claim blocks new preparation. Explicit acknowledgement records
accepted uncertainty—not proof that a process died or stopped—and allows only a
separately prepared/reviewed **new action**. Old consumption stays permanent.
Terminal attempts also require a new action for another startup. Other scoped
containers may still be running; fresh approval is not automatic cleanup.

## Operator stop

```bash
coven action stop '<action-id>' --json
coven down       # enabled mode: current scoped attempt only
```

Stop uses the recorded plan's exact namespace/backend, not current mutable
runtime config or a human-name legacy container. It does not reset consumption
or prove native workers terminated. Historical attempts can be stopped explicitly
by ID; down does not sweep every previous container.

Reports distinguish `stop_command_attempted` from `stop_command_succeeded` and
uncertain effects. A failed/permission-denied tmux presence query is not evidence
of absence. Nonzero or timed-out stops are not silently successful. Stops do not
start, attach, replace, or retry anything.

General task dispatch, read-only tools, native worker identity/lifecycle, mutation
scope grants, integration/delivery authorization, and distributed recovery remain
separate roadmap work. Bounded JSON/retained manifests and control claims can grow;
no pruning, claim reset, or universal power-loss durability is promised.
