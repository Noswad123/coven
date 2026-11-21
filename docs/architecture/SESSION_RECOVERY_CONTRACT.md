# Explicit Session Recovery (A5)

Recovery is an explicit selection executor, not a startup service or automatic
restoration mechanism. It submits only selected, typed provider resume commands
through the A1 interactive preflight and callback seam. It never invokes the
provider executable itself, imports transcripts or credentials, changes cwd,
enrolls workers, or modifies groups, tasks, history or native references.

## CLI And API

```text
coven session recovery inspect WORKSPACE [--json]
coven session recovery run WORKSPACE --file ABS_SELECTION_JSON --socket ABS_SOCKET --expect-recovery-revision TOKEN [--execute] [--json]
```

`inspect` returns `recovery_revision`, including for an absent journal/workspace.
`run` defaults to read-only preview. Both are lock-free and make no endpoint
contact or workspace writes in preview mode. Missing executables and endpoints
are valid preview inputs. Preview tokens/reports contain no execution authority.
The exact current token is required for both preview and execution. Execution
also requires the current invocation's `--execute`; no consent is persisted.

Public Rust entry points are `session_recovery::inspect(&Path, bool)` and
`session_recovery::run(&Request, bool)`, returning `Result<TaskOutput>`.
`Request` contains `workspace: PathBuf`, `file: PathBuf`, `socket: String`,
`expect_recovery_revision: String`, and `execute: bool`. JSON reports have
`schema_version: 1` and `contract: "coven.session-recovery.v1"`. Text output is
pretty JSON, deliberately retaining the same explicit limitations and outcomes.
Exit codes: 0 preview/queued/skipped, 1 deferred/rejected/known-unsent, 2
unsupported/uncertain/persistence failure/batch stopped, 130 interrupted.
Validation, admission, CAS and capacity errors use the existing CLI error path.

## Closed Selection Schema

```json
{
  "schema_version": 1,
  "entries": [
    {
      "key": "selected-one",
      "reference": {
        "source": "kitsune:djinn",
        "runtime": "djinn",
        "kind": "id",
        "value": "literal-saved-session"
      },
      "executable": "/absolute/path/to/djinn",
      "pane_id": "w1:p1",
      "expected_terminal_id": "terminal-1"
    }
  ],
  "imported_references": []
}
```

All shown fields are required except `imported_references`, which defaults to
empty. Unknown fields at every level, duplicate JSON keys (including escaped
equivalents), malformed JSON, nonfinite numbers, more than 16 levels of JSON
nesting, and files larger than 1 MiB are refused. Parsing uses the existing
`values::Profile::Authority` routines before closed typed deserialization.
There must be 1..16 entries with unique nonblank keys of at most 128 bytes.
No raw argv, environment, prompts, authorization or permission fields exist.
Imported references have exactly the same four-field reference shape, at most
128 tuples. Null is not an empty list.

References require the exact matching source `kitsune:<runtime>`. `kind` is
`id` or `path`; path references are supported only by A1's Pi/OMP adapters.
Source/runtime strings are bounded to 512 bytes, ID values to 512 bytes and
path values to 4096 bytes. Values are literal, nonblank, contain no controls,
and cannot begin with an option after leading whitespace. Absolute paths are
lexically checked, bounded to 4096 bytes, with no trailing slash or `..`
components. Keys and target identifiers contain no controls; identifiers also
pass the existing session-opaque validator.

All selected entries pass typed A1 preview before admission or endpoint contact.
Unsupported runtime/kind combinations produce explicit `adapter_unsupported`
rows and are never submitted. The current A1 adapter set is djinn, claude,
codex, copilot, devin, droid, kimi, mastracode, pi, omp, hermes, opencode,
qodercli, kilo, cursor and grok. This is source-characterized policy, not live
provider/version qualification.

The duplicate key is the exact `(source,runtime,kind,literal value)` tuple,
independent of executable and target. Only the first selected occurrence can
be considered. The entire imported-reference list is evaluated before any
launch: every matching selected tuple is skipped even if its import appears
later in the JSON document. Imports are skip-only vetoes, never authority.
Distinct reference tuples sharing a pane or terminal are rejected before any
send, even when another veto would skip one of them.

## Present Reference Veto

Explicit execution performs one additional read-only `session.snapshot` after
transport admission and the initial locked CAS/whole-selection capacity check,
before any mutating command. Preview performs no such read. A new recovery-only
parser reads the optional `snapshot.panes[].agent_session` shape
`{source, agent, kind, value}` from **all** panes before considering any selected
submission. `agent` maps literally to the selection's `runtime`; no source or
provider aliases are inferred. References match the exact
`(source,runtime,kind,literal value)` tuple, independently of selected order,
target pane, terminal and executable. Explicit `imported_references` vetoes still
apply to the whole batch and take precedence in reporting.

A matching snapshot hint yields `outcome: "existing_reference_present"`,
`delivery: "not_attempted"` and `native_running: "unestablished"`. It creates no
journal attempt or stored permission. Every run report/entry also keeps
`native_running` unestablished. These hints may be **stale persisted history**,
including references associated with external panes. They conservatively skip;
they do not authenticate a provider, session identity or currently running UI.
An absent optional field means no hint, not proof that an equivalent session is
absent locally or elsewhere. A present null, missing required member, invalid
kind/value/type, or out-of-profile reference fails closed rather than silently
discarding the hint. Valid differing source/provider/kind/value tuples do not
match. Unknown unrelated snapshot/pane fields are ignored after bounded wire
validation, without narrowing native integer extensions.

The read has a three-second deadline, no retry, a 1 MiB response budget
(including the required terminal newline), and exactly one EOF-terminated JSON
frame. It validates the correlated request ID, exclusive result/error branch,
`session_snapshot` result type, protocol 18 and command-submission descriptor
version 1. It accepts at most 1024 panes with unique pane/terminal IDs. All four
locator IDs use the existing nonblank printable/non-whitespace profile, at most
4096 characters. Hint source/provider strings and ID values are at most 512
bytes; absolute path values at most 4096 bytes, using the selection's literal
text/path checks (but without relabeling a different hint source/provider).
The existing native wire decoder bounds depth to 32, object members and array
items to 4096, general strings to 4096 characters and path-like strings to 8192
characters, with its Authority parser's finite/duplicate-free scalar and node
bounds. Consumed reference fields have the tighter byte limits above.

Endpoint selection, the returned read endpoint and metadata fences must agree.
The same sampled endpoint is re-fenced in each A1 before-send callback so a
visible replacement cannot inherit the sampled veto inventory. Existing
same-user path-race limitations still apply. Read/validation/fence failure stops
the whole batch before any queue attempt, reports
`unattempted_reference_read_failed` for every selected entry and leaves the
journal unchanged. No workspace lock is held across network I/O. No
`agent.list`, `agent.get`, process inspection, readiness inference or provider
invocation is used. Existing Kit read requests may themselves trigger its
normal server-side housekeeping; zero receiver-side effects are not promised.

This compatibility parser is deliberately separate from A1's generic PTY
locator schema. It is a skip-only veto, not execution authority, and can be
retired when the legacy optional field is replaced by a supported native import
inventory. The inventory is sampled once, not continuously or atomically with
later commands; it cannot prevent changes by other tools after the read.

## Journal And Ordering

Only `state/session-recovery.json` is recovery-owned persistent state:

```json
{
  "schema_version": 1,
  "revision": 2,
  "attempts": [
    {
      "entry": {
        "key": "selected-one",
        "reference": {
          "source": "kitsune:djinn",
          "runtime": "djinn",
          "kind": "id",
          "value": "literal-saved-session"
        },
        "executable": "/absolute/path/to/djinn",
        "pane_id": "w1:p1",
        "expected_terminal_id": "terminal-1"
      },
      "outcome": "queued"
    }
  ]
}
```

The state schema is closed, duplicate-free, depth/size bounded as above, with
an unsigned 64-bit revision and at most 128 attempt records. A missing file
means revision zero and no attempts, without creating an initial file. The
opaque token hashes the workspace path and exact present/absent file bytes.
Existing workspace paths are canonicalized without mutation. Unknown/newer
schemas, malformed state and unsafe managed paths fail closed; no repair or
reset is attempted. A full journal refuses further attempts, including
deferrals; no records, especially blockers, are automatically evicted.

Execution takes shared transport admission **before** any workspace lock and
uses one interrupt guard for the batch. It rechecks CAS under the existing
workspace guard and checks capacity for all eligible unique selected references
before endpoint contact. This deliberately happens before the snapshot veto:
insufficient journal capacity refuses the entire batch even if a later snapshot
might have skipped some entries. Callback-time CAS/capacity checks remain.
A1 performs executable, endpoint and target preflight. Its
`before_send` callback takes a short workspace lock, rechecks CAS and duplicate
blocking, appends `submission_intent`, and atomically replaces and synchronizes
the journal using the existing storage pattern. Directory preparation syncs
both `state/` and the workspace parent under the guard, including when admission
created `state/`. Only confirmed durable success
permits the callback to return success. The lock is released before any
mutating network send; cooperating competing selections with the same token
cannot both reserve. The transport admission remains held across the batch.

The callback's durable intent is the crash-safe conservative reservation. After
delivery, another short locked CAS records the result in that same attempt.
`queued`, `uncertain` and `submission_intent` block the tuple on later
invocations/restarts. `rejected`, `unsent` and `deferred` retain an outcome but
do not block a new explicitly consented execution with a fresh token. Missing,
unavailable or busy targets are not automatically retried. Explicit server
rejections remain visibly `rejected` rather than being called queued.

Callback persistence failure or uncertainty prevents send and stops the batch.
Post-send persistence failure stops the batch without rolling back any prior
queued command; the prior durable intent remains a blocker (or a successfully
replaced queued/uncertain outcome remains blocking). Lost acknowledgements and
interruptions stop remaining entries as unattempted. A crash before send may
leave a false-positive intent blocker; safety takes precedence over liveness.
No automatic resolution/removal command is supplied. Existing atomic-store
assumptions apply: cooperating local filesystem users, not hostile same-user
path races, distributed durability, or protection against manual journal deletion.

## Honest Scope

There is no qualified A2 native binding inventory in this implementation.
The explicit imported-reference list and the optional point-in-time Kit pane
references are unauthenticated, conservative skip-only vetoes, not running
status. Journal duplicate blocking is workspace-scoped; the snapshot veto covers
only the panes returned by that one pinned endpoint at that point in time.
Absence of a tuple does **not** show that an equivalent session is not running
elsewhere, including another workspace or another tool. This journal plus
sampled veto is not a global native exactly-once guarantee. Queued input is
not proof of native startup/resume, identity, readiness, worker acceptance or
completion. No live provider, credentials, models or endpoints are involved in
the regression fixtures.
