# Session inspection and explicit interaction

## Native Session workflows

Native `session list`, `inspect`, `watch`, `start`, `resume`,
`prompt-capabilities` and `prompt` cover the seven Session workflow routes.
View filtering is part of readers/watch, not another route. The separate pure
`plan-resume` interface characterizes historical argv; it does not execute.
The installed product is native; `target/debug/coven` below explicitly selects
an already-built development binary. Snippets run from the repository root.

Observation, interactive UI submission and native prompt admission are different
operations. None establishes worker readiness, native lifecycle or completion.
Installed provider CLI/version compatibility is unverified; queued input does
not prove the provider started. There is no inferred activity or automatic retry.

## Inspect generic panes, not an authoritative session registry

List/inspect read a selected Kitsune API socket and join matching declared
groups/roles to generic pane/process-family evidence. They do not depend on
Kitsune's retired agent inventory, probe native runtimes, open live artifacts or
write a roster. Group declarations remain qualified historical evidence, not
authenticated session identity.

Use `--group GROUP [--member MEMBER]` and/or `--recognized-only` to narrow the
view **after** the complete bounded observation. Original observed counts remain
explicit. Inspect requires `--pane`; `--expected-terminal` optionally fences it.
See [inspection](../architecture/SESSION_INSPECTION_CONTRACT.md) and
[views](../architecture/SESSION_VIEWS_CONTRACT.md).

Watch refreshes serially in the foreground: default duration 60 seconds, interval
two seconds, optionally one pane and the same view flags. `--json` streams JSONL
samples followed by a scoped summary, not a buffered transcript. There is no
daemon, stored roster or stale-positive fallback. Stream failure stops without
retry; handled interruption exits 130. The streaming adapter returns empty
buffered stdout after emitting samples/summary. See the
[watch contract](../architecture/SESSION_WATCH_CONTRACT.md).

## Preview an explicit interactive UI start or resume

Start/resume support 16 paired source-characterized execution-capable adapters,
separate from the historical planner. Require explicit runtime, absolute binary/
socket paths and exact pane/terminal pins. Resume also requires `--session`;
`--session-kind` defaults to `id`, with `path` accepted only for Pi/OMP.
The [interactive contract](../architecture/INTERACTIVE_SESSION_CONTRACT.md)
owns the adapter table and literal argv policy; arbitrary extra args are refused.

Without `--execute`, these are pure composition previews: no workspace/file/
executable inspection, endpoint contact or process effects. A valid preview
establishes neither available support nor later permission.

Explicit execution submits an unrestricted UI command to a selected existing,
quiescent empty shell prompt through discovered generic `pane.submit_command`.
It uses shared workspace transport admission and rechecks runtime-launch mode;
enabled/invalid/orphaned reviewed-launch authority refuses this different effect.
No readiness probe, adoption, fallback keystrokes or automatic following occurs.
Workspace is control context, not a target-shell cwd change or resource sandbox.
Native credentials, configuration and permissions remain independent.

## Prompt an opted-in native Djinn session

`prompt-capabilities` contacts exactly one explicit private native socket.
`prompt` previews by default; it requires a workspace, runtime UUID, literal
`ses_…` ID, fresh request UUID and absolute prompt-file path. Preview does not
read the file or workspace. With `--execute`, Coven submits through Djinn's
startup-enabled native prompt socket and checks a correlated message receipt.
No Kitsune terminal fallback, inherited approval or permission auto-approval exists.

Acceptance acknowledges native message persistence and scheduling of the ordinary
reply path, not model execution, worker admission or success. A request UUID is
not a durable retry key. Lost acknowledgement does not cancel possible work:
inspect native state and make a separate explicit decision, never blindly resend.
See the [prompt contract](../architecture/SESSION_PROMPT_CONTRACT.md).

## Reference commands for all seven routes

Replace uppercase placeholders with inspected values. These are reference syntax,
not live validation instructions. Capabilities/read/watch **contact endpoints**;
only start/resume/prompt below are pure previews. No execute flags are shown.

```text
target/debug/coven session list WORKSPACE --socket ABS_KITSUNE_SOCKET --json
target/debug/coven session inspect WORKSPACE --socket ABS_KITSUNE_SOCKET --pane PANE --expected-terminal TERMINAL --json
target/debug/coven session watch WORKSPACE --socket ABS_KITSUNE_SOCKET --group GROUP --recognized-only --duration 10 --interval 2 --json
target/debug/coven session start WORKSPACE --runtime djinn --executable ABS_DJINN --socket ABS_KITSUNE_SOCKET --pane PANE --expected-terminal TERMINAL --json
target/debug/coven session resume WORKSPACE --runtime djinn --executable ABS_DJINN --socket ABS_KITSUNE_SOCKET --pane PANE --expected-terminal TERMINAL --session SESSION_SELECTOR --json
target/debug/coven session prompt-capabilities --native-socket ABS_NATIVE_SOCKET --json
target/debug/coven session prompt WORKSPACE --native-socket ABS_NATIVE_SOCKET --expected-runtime RUNTIME_UUID --session ses_SESSION_ID --request-id REQUEST_UUID --prompt-file ABS_PROMPT_FILE --json
```

## Historical pure resume planner

```text
coven session plan-resume --runtime claude --source kitsune:claude --session-kind id --session saved-session --json
```

This inspects literal resume argv for 16 historical provider mappings, without a
workspace, executable lookup, socket, provider launch or execution authority.
Only Pi/OMP accept path references. Reports cannot be imported as execution
inputs. Its historical Djinn `-s` mapping is intentionally distinct from the
interactive adapter's `--session`. See the
[pure planner contract](../architecture/SESSION_RESUME_PLAN_CONTRACT.md).

## Edge cases and related files

Session workflow parsing supports unambiguous long-option abbreviations and
repeated options (last wins), not unknown flags; the pure planner instead requires
exact, nonduplicate flags. Endpoint privacy, no-follow checks and response bounds
remain required. Read-only requests can trigger existing server housekeeping,
including default-workspace/shell creation; they are not zero server effects.

Implementation: [`src/session_inspection.rs`](../../src/session_inspection.rs),
`src/session_views.rs`, `src/session_watch.rs`, `src/interactive_sessions.rs`,
`src/session_prompts.rs` and `src/session_support.rs`; CLI callers are in
[`src/main.rs`](../../src/main.rs). See
[native Session details](../plans/RUST_MIGRATION.md#native-session-completion)
and [retirement gates](../plans/RUST_RETIREMENT.md) for bounded Linux profiles
and separate compatibility/platform obligations, not fresh validation totals.
