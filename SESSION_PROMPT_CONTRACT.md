# Explicit Djinn-native interactive prompt admission

This optional integration sends a prompt to an existing Djinn full-TUI session
through its explicitly startup-enabled private socket. It does not use Kitsune,
terminal input, screen detection, a native subprocess or a headless V2 drain.
It does not provide scheduler dispatch or bounded-worker admission.

## Commands

```text
coven session prompt-capabilities --native-socket ABS_SOCKET [--timeout 3] [--json]
coven session prompt WORKSPACE --native-socket ABS_SOCKET --expected-runtime UUID --session ses_ID --request-id UUID --prompt-file ABS_FILE [--execute] [--timeout 3] [--json]
```

`prompt-capabilities` reads metadata from exactly one private native endpoint. It
returns the observed runtime UUID, not permission, readiness or an automatically
adopted session. No default discovery or endpoint launch follows.

Prompt submission defaults to a **pure preview**: validate/compose paths and IDs,
without reading the file/workspace, connecting, starting an app or changing state.
The preview does not contain prompt text/hash and grants no later execution.
Use a fresh canonical UUID for `--request-id`; it is not a durable retry key.
`--session` is a literal native `ses_` ID, never a pane, folder selector or role.

`--execute` requests unrestricted interactive native prompt admission. It may
start model/tool work subject to the selected Djinn runtime's independent policy.
There is no inherited parent approval, permission auto-approval, tool/resource
sandbox, model override or automatic integration. WORKSPACE is existing Coven
control context, not the native session's cwd or resource boundary.

## Required native support

Only a feature-bearing Djinn **full-TUI worker** explicitly started with
`DJINN_PROMPT_SOCKET` supports this v1 endpoint. Existing sessions are not restarted
or retrofitted; mini/headless/V2 and legacy HTTP `prompt_async` are not fallbacks.
Native documentation is in the optional sibling
[`docs/interactive-prompts.md`](../djinn/docs/interactive-prompts.md).

The endpoint must advertise `djinn.interactive-prompt.v1`, exact integer version 1
and a 65536-byte prompt limit. Its runtime UUID must match `--expected-runtime`.
The native handler checks that the selected existing session belongs to its fixed
startup directory and has a recorded agent/model. It preserves that selection and
variant instead of choosing native defaults. A blank chat with only local UI
selection and no recorded native configuration is deliberately unsupported.

The socket's immediate parent must be a non-symlink, same-user directory with
mode 0700. The socket leaf must be a non-symlink, same-user socket with mode 0600.
Selected/canonical paths, socket device/inode and parent device/inode are fenced
before connection/send and after response. These are ordinary local identity
checks, not peer authentication or protection against hostile same-user races.

## Execution and acknowledgement

Execution uses the existing workspace transport gate and rechecks runtime-launch
authority. Enabled, invalid or orphaned approved-launch authority refuses this
different effect. Direct helper calls require the owned live gate token. The
global workspace lock is not held during native I/O. No task/agent/group/approval
record, log, dashboard, launch claim or stored observation is updated; the existing
persistent transport-gate file can be created by admission.

Read one explicit regular prompt file, without following a leaf symlink, once
under a 65536-byte bound. Decode strict UTF-8; reject blank text and controls except
tab, CR and LF. Preserve exact text and newlines: no shell interpretation, `@file`
expansion or template substitutions. Native plugins still follow native semantics.
Report only file path, SHA256 and UTF-8 byte count, not content or peer error text.

One capabilities read and at most one submission share a default 3-second
acceptance budget, configurable in `(0,30]`, and 65536 response bytes. Filesystem
and in-flight native calls cannot be forcibly interrupted by this budget. A lost
acknowledgement does not cancel work that may have been accepted.

The accepted receipt must match request UUID, runtime UUID, session ID, native
`msg_` message ID, submitted-text digest and byte count. It acknowledges completion
of native user-message persistence and scheduling of the ordinary native reply
path—not model start, reply success, worker-policy admission or task completion.
The ordinary instance-scoped runner and folder-capsule assistant-event path remain
native-owned. Concurrent native writers/config changes remain possible; this is
not an immutable configuration or prompt lease.

## Outcomes and uncertainty

JSON reports use `coven.interactive-prompt.v1`, schema version 1:

| Outcome / delivery | Meaning / exit |
| --- | --- |
| `preview` / `not_attempted` | Pure composition; authorization none; 0. |
| `accepted` / `accepted` | Correlated native message receipt; 0. |
| `unavailable` / `unsent` | Preflight/connection failed before a possible mutating send; 1. |
| `rejected` / `rejected` | Valid native pre-admission rejection; 1. |
| `uncertain` / `uncertain` | Possible send/admission or lost/invalid receipt; 2. |

Handled interruption exits 130, retaining an already accounted accepted receipt;
otherwise possible in-flight submission remains uncertain. All reports leave
native worker acceptance/lifecycle/job completion **unestablished**. Explicit
execution authorization is only `interactive_native_prompt_submission_only`.

The native runtime remembers at most 1024 submit UUIDs, without eviction. Duplicate
requests never submit again within that runtime, but return **uncertain** about
the original result. Capacity refuses new requests. This is process-local duplicate
suppression, not durable idempotency, a stored receipt or lost-ack reconciliation.
There is **no automatic retry**, including after a runtime change. Inspect native
state and obtain a separate explicit decision before any further action.

## Offline validation

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -p 'test_session*.py' -q
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -q
```

Fixtures use private sockets only; no real Djinn UI, native model/provider, Kitsune
session, installation or user declaration is touched. The separate cross-language
fixture uses the TS socket producer with injected native callbacks and the real
Coven CLI; it does not validate a deployed full-TUI build.
