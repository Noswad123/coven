# Explicit interactive Djinn UI start/resume

This is a small interactive-control slice, not scheduling, task dispatch, native
worker admission, lifecycle supervision or an automatic replacement for existing
Kitsune/Coven agent commands. Kitsune stays a generic multiplexer; Coven constructs
native arguments and consumes only the public JSON API. Djinn code is unchanged.

## Preview by default; submit only explicitly

```text
coven session start WORKSPACE --runtime djinn --executable ABS_BINARY --socket ABS_SOCKET --pane PANE --expected-terminal TERMINAL [--execute] [--json]
coven session resume WORKSPACE --runtime djinn --executable ABS_BINARY --socket ABS_SOCKET --pane PANE --expected-terminal TERMINAL --session SELECTOR [--execute] [--json]
```

Without `--execute`, this validates/composes a preview only: no workspace/executable
inspection, API contact, process execution or application-state write. Normal
Python import/CLI/locale setup is not a filesystem sandbox. An uninitialized path
or absent executable can be previewed; that establishes no usable native fact.
No preview file/report grants later execution permission.

Start constructs `[ABS_BINARY, "--ui"]`. Resume constructs
`[ABS_BINARY, "--session", SELECTOR]`. Djinn's selector can denote a native UI id,
folder/name/path; it is a literal requested selector, not verified session or
execution identity. Arguments are never shell-interpreted by Coven. No prompt,
model override, permission auto-approval, hidden help/status/watch call, installed
binary fallback or native-worker-readiness probe is generated.

Executable/socket paths must be explicit absolute local paths without `..`/trailing
slash. The CLI expands the workspace path to an absolute control context; `..` is
not accepted in that context. Identifiers
must be exact, nonblank and whitespace-free. Paths/selector/identifiers reject
control characters, invalid UTF-8 and values above 4096 UTF-8 bytes. Resume selectors
also reject an initial `-`. Start has no selector. Runtime is explicitly `djinn`.

`--execute` is an explicit request to submit an **unrestricted interactive native
UI command** into a quiescent empty shell prompt. Native configuration, credentials,
permissions and tools remain Djinn's; no read-only/resource/tool sandbox or task
authorization follows. WORKSPACE is an existing initialized Coven control context,
not an implicit target-shell cwd change or resource boundary.

## Workspace authority and target fencing

Execution validates the workspace and acquires the existing shared persistent
`state/.runtime-transport.lock` gate before rechecking actual runtime-launch
authority. Enabled, invalid or orphaned approved-runtime-launch mode fails closed;
its cold-launch claims cannot authorize this different operation. Direct library
execution requires the current owned gate token too. No gate inode is reset/deleted,
no launch claim consumed, and no metadata/agent/task/dashboard/history is updated.
The gate persists, as for legacy transports. Activation cannot publish while this
admitted command operation is still performing I/O.

The shared gate is held through API I/O, **not** the global workspace lock. A single
API budget defaults to 3 seconds, accepts `(0, 30]`, and covers two sequential
snapshots and one mutation under a shared 1 MiB response budget. Gate acquisition,
filesystem calls and native application behavior are not hard wall-time bounded.

Before writing, Coven:

1. Checks that the explicitly selected native path is a regular executable, without
   invoking it. Metadata is rechecked before submission; it is not build/CLI-version
   attestation, authentication or protection from hostile same-user path races.
2. Selects one existing user-owned/private Unix API socket, with canonical parent
   path/device/inode fences. No default discovery, autostart or private TUI socket.
3. Reads generic snapshot identities and requires exact integer
   `command_submission.version: 1`. It does not consume Kitsune's legacy agent schema.
4. Reads again, requiring the same current pane/terminal/workspace/tab mapping and
   capability. Changed target or lost support stops before any write; no following,
   rebind, adoption, write probe or unguarded-input fallback.
5. Calls only `pane.submit_command` with the exact pane/terminal pin and native argv.
   The server rechecks its current terminal and observed available foreground shell
   immediately before queuing one command input frame.

The read-only observation transport and annotation publisher keep their existing
method allowlists. New controlled submission is separate. Socket/OS observations
are not peer authentication, an atomic snapshot or a foreground/prompt lease.
Caller and server must share the intended local filesystem namespace. Executable
metadata is inspected from the caller's view; it does not pin a server-side binary.

## Results, uncertainty and exits

JSON reports use `schema_version: 1`, contract `coven.interactive-session.v1`, exact
requested argv/target/control context, safe diagnostics and explicit limitations.
Peer error messages, terminal text, credentials and native output are not projected.

| Outcome / delivery | Meaning / exit |
| --- | --- |
| `preview` / `not_attempted` | Composition only; authorization `none`; exit 0. |
| `input_queued` / `queued` | Correlated `pane_command_submitted` ack matches exact pane, terminal and argv; exit 0. |
| `unavailable` / `unsent` | Failed validation/preflight/connection before a possible mutating send; exit 1. |
| `rejected` / `rejected` | Correlated valid API error; exit 1, no repair/retry. |
| `uncertain` / `uncertain` | Possible partial send, lost/malformed/mismatched ack or interruption before ack accounting; exit 2. |

Handled interruptions exit 130 and preserve any already recorded queue
acknowledgement; before-send is unsent and unrecorded in-flight outcomes are
uncertain. There are no durable admission receipts or exactly-once guarantees.
SIGKILL/crash cannot yield a receipt. Reinvocation is not a safe retry: inspect
the native UI/process after uncertainty and make any further action explicitly.

Execution reports authorize only `interactive_terminal_submission_only` and leave
native lifecycle, native worker acceptance and job completion **unestablished**.
Even a valid ack is not proof that Djinn started, the selector existed/resumed, the
UI is ready, model work succeeded, or later input is safe. No activity/group
enrollment, attachment or result facts are synthesized from it.

## Compatibility and limits

Legacy `agent start/stop/restart`, `up`, runtime-launch manifests, task attempts and
help-only worker readiness are unchanged. In particular, this command does not make
the blocked/unknown native worker contract compatible. Existing mutable native
configuration and independent permissions still apply to any launched UI.

This requires a feature-bearing Kitsune server advertising the descriptor, not
merely an updated CLI or protocol 18. Old/incompatible servers are refused before
submission. See the optional sibling [generic API guide](../kitsune/COMMAND_SUBMISSION.md).
No installed server, native UI, model job, provider, install or upgrade is started by
the implementation's offline tests.
