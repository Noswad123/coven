# COV-05a: help-only native adapter readiness

`coven worker readiness` is an adapter compatibility observation, **not a worker
ready signal, policy attestation, or execution grant**. It creates no worker,
task attempt, session, result, claim, or persistent authority.

```bash
coven worker readiness --adapter djinn --profile research-readonly --json
coven worker readiness /path/to/workspace --executable /trusted/path/to/djinn
```

The directory is probe cwd/display context, not an admitted resource scope. It
need not be an initialized Coven workspace. Snapshot-v1, imported task ledgers,
and approved-launch mode are not modified. Even corrupt Coven records can be
diagnosed independently. No workspace locks, logs, dashboards, controls, caches,
or migrations are created by the inspector.

## Report and exit semantics

The Coven report uses `schema_version: 1`, adapter contract `djinn-help-v1`, and
`authorization: none`. This versions **Coven's report**, not a hypothetical native
Djinn worker protocol. It includes:

- selected/resolved executable path, regular-file identity and SHA256;
- observed native version, or null when unavailable/unverified;
- advertised CLI surfaces, commands, flags, and source probe IDs;
- per-probe argv, observation time, status, return code, bounded output sizes and
  digests, and elapsed time; native output is withheld from diagnostic excerpts;
- separate semantic capability states, stable blocker codes, and next steps;
- explicit `configuration_inspected: false`, `policy_inspected: false`, and
  `sessions_inspected: false` context.

| Exit | Assessment |
| --- | --- |
| 1 | `blocked`: a successfully observed help surface does not advertise the session/run route this adapter requires. |
| 2 | `unknown`: probe failure, missing/changing executable, unsupported help, or unverified worker guarantees. |
| 0 | Reserved for a future implemented compatibility contract; this help-only adapter never emits `compatible`. |

Invalid CLI requests/bounds are errors, not capability observations. A successful
help exit, role named “read only,” background/foreground flag, or version string
cannot establish policy enforcement, provider authentication, acceptance, or
reconciliation. Query failure is never evidence of native absence/stopped state.
Reports are not cached permissions and must not authorize later execution.

## Allowlisted observations

The first probe is root `--help`. Only advertised parent routes are followed:

```text
--help
session --help
session run --help
session status --help
session watch --help
ask --help
agents --help
--version                    # only when root Options explicitly advertises it
```

No bare run/status/watch/list command, `doctor ui`, configuration/policy/tool
inspection, session initialization, model/provider call, or fallback submission
is invoked. Help parsing follows bounded Clap-style sections, not arbitrary prose
or role claims. Unknown output/schema/version does not create a guessed adapter.
The inspector does not assume the roadmap's proposed `djinn agent worker run`
exists. It does not invent a native capability JSON endpoint.

The selected binary must be trusted. It is resolved and fingerprinted before and
after probing, with metadata checked before each invocation. This detects ordinary
changes; it does not authenticate a build or prevent malicious same-user races.
An arbitrary executable can ignore `--help` and mutate state. Process-group
cleanup is not a filesystem/network sandbox or protection from deliberate group
escape. The false context flags describe operations Coven did not request, not
an attestation of arbitrary native code's purity.

## Bounded subprocess lifetime

Each native argv is passed directly, without shell interpretation, through a
static isolated Python supervisor (`-I -S`). The supervisor cannot import modules
from probe cwd. Native stdin is null and native environment is limited to
`PATH`, `HOME`, `SHELL`, `LANG`, `LC_ALL`, and `TERM`; values are not put in reports.
Arbitrary provider tokens are not forwarded.

Default per-probe budget is 3 seconds and 128 KiB combined stdout/stderr. Accepted
bounds are `(0, 30]` seconds and `[1024, 1048576]` bytes. Executable fingerprinting
streams at most 1 GiB; it is separate from subprocess timeouts. Output is bounded
while reading, not after accumulating an unlimited result.

The live supervisor pins its process-group identity through cleanup. It reports
native exit through a private FD, with both private descriptors excluded from
native inheritance. Parent cleanup signals the whole group on every completion
path before reaping the supervisor, including success/nonzero exits whose
descendants closed output pipes. Return code is the **observed native code**, not
the supervisor's cleanup signal; an unobserved native exit stays null.

Parent-liveness EOF handles readiness-caller SIGTERM/SIGKILL. An independent
supervisor watchdog (`probe timeout + 1 second`) covers native execution and
post-completion waiting even if parent cleanup stalls. Cleanup/reaping attempts
are bounded; OS cleanup failures report a failed observation. Custom parent
SIGCHLD/reaper contexts are rejected before spawn rather than guessing child
ownership. These are local POSIX cooperating-process guarantees, not universal
kernel or malicious-code containment claims.

## Current native evidence and required next contracts

The separate [persisted-folder observation contract](DJINN_OBSERVATION_CONTRACT.md)
now consumes `djinn session observe ABS_FOLDER --json` through an explicit trusted
binary. This is bounded, non-application-writing folder evidence, not the required
per-attempt admission, immutable accepted policy, live lifecycle or result-binding
contract. The help-only readiness inspector does not invoke it, change its help
allowlist, or upgrade any capability; it still never declares `compatible`.

The inspected installation advertises session/run/status/watch, ask, and agent
description help. It does not advertise a version flag, so native version remains
null. The installed help-only probe returns `unknown`, with no authorization.

Source inspection of Djinn at `a040dde4b0c1b0c9d87c7c6fe303100b12385d2d` found real
native sessions and enforced role tool allowlists, but no complete bounded-worker
admission contract. This source reference is **not** a claim that the fingerprinted
installed binary is that build. Native configuration and credentials were not
inspected, and no native session/model was started.

Important source-level constraints behind the blockers:

- Folder-session initialization also creates/reuses UI bindings; it is not a
  read-only readiness check. Folder, native agent, UI session, background-run PID,
  and pane IDs are different identities.
- Background session run reports process spawn and paths, not an attempt-bound
  native acceptance receipt with recoverable caller identity.
- Status/watch/list can persist stale-run recovery observations. Inspection-style
  names and ledger “readiness” are not side-effect-free execution inspection.
- A lifecycle enum containing `cancelled` does not establish a cancel command.
  Session deletion is not cancellation or observed termination.
- Configured role tool allowlists are genuinely enforced, but empty lists select
  defaults; an immutable per-attempt accepted policy/digest is not a CLI contract.
  Defaults and shell do not establish bounded read/resource containment.

A future native adapter needs explicit evidence for:

1. Supported versioned capability/admission schema and compatibility negotiation.
2. Stable native execution identity and persisted caller task-attempt correlation.
3. Attempt-bound submission acknowledgement and lost-ack lookup/idempotency.
4. Typed lifecycle/permission-pause facts through a documented non-mutating query.
5. Result/transcript pointers bound to the admitted attempt.
6. Cancellation acknowledgement separate from observed termination.
7. Accepted immutable per-attempt tool and resource/path policy with enforcement.

Help-only observations leave all those semantic capabilities `unknown`. No role
configuration tweak can make this adapter declare compatibility. Actual dispatch
also needs Coven's durable task-attempt model and its own scoped authorization;
an existing `runtime_launch` claim cannot be reused. Full COV-04/COV-05 remain open.
