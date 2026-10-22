# COV-02c / COV-03a: checkpoint-local content-bound decisions

This is single-file checkpoint authority, not execution permission. Approval does
not authorize dispatch, repository edits, commits, integration, push, merge, or
deployment, nor transfer a lead's permissions to workers. Linked files, Git
objects, run/task graphs, and external plans are not hashed by this protocol.
The task ledger remains task-only and does not contain checkpoint decisions.

## Inspect and select an exact snapshot

```bash
coven checkpoint inspect plan-approval --json
# Copy the actual snapshot token, not the placeholder below.
coven approve plan-approval --expect-revision '<snapshot revision>' --json
coven suggest plan-approval "Please add QA evidence" \
  --expect-revision '<new snapshot revision>' --json
```

These commands accept an optional workspace path before the checkpoint ID.
Inspect is read-only: it creates no lock, rewrites no checkpoint, and certifies
no filesystem durability. It returns exact UTF-8 content, parsed decision/history,
declared/effective status, binding diagnostics, and two different revisions:

| Token | Purpose |
| --- | --- |
| `checkpoint-snapshot-v1:<digest>` | All original file bytes, domain-bound to the checkpoint ID. Conditional mutation token. |
| `checkpoint-missing-v1:<id>` | Explicit absence, not an empty file. Conditional creation token. |
| `checkpoint-scope-v1:<digest>` | Covered local content only. Tests whether approval is still effective. Not a conditional-write token. |

Approve/suggest without a token explicitly select the **latest** snapshot reloaded
under the lock. They do not prove an earlier external review still matches. The
result exposes `selection: latest` or `conditional`. Scripts protecting reviewed
content must supply the snapshot token. A missing-file decision creates the
default local checkpoint; selecting absence does not approve a previously unseen
external plan.

Interactive review captures each file immediately before displaying it and submits
that exact revision after user input. Console approve/suggest display and ask for
confirmation against the immediate snapshot, not console-startup state. Conflicts
skip the decision; neither interface refreshes tokens and retries automatically.
Console/review retain non-success statuses across later quit/EOF.

## One file and a precise scope

The Markdown stores a versioned `coven-checkpoint-decision:v1` JSON comment in its
header. It binds checkpoint ID, decision ID/action, local scope, reviewed input
snapshot, timestamp, and source. Current `Status:` and any present direct decision
checkboxes must agree. Notes are retained in structured
`coven-checkpoint-history:v1` JSON comments, available decoded through inspect.
Sources/timestamps describe provenance, not an authenticated user or permission.

Scope is a domain-separated hash of the concatenated remaining **original bytes**
after only these exclusions:

1. The unique live header `Status:` line, including its line ending.
2. The unique reserved decision metadata block.
3. Exact recognized direct Approved/Rejected checklist lines under the live
   `## User decision`, before any nested/subsequent heading.
4. Valid machine-history blocks only while they form a **trailer**, with their
   owned separator newlines.

All other bytes are covered: title, ID text, purpose, criteria, links, custom
header/decision prose, old human approval/history sections, examples, code,
whitespace, Unicode, and LF/CRLF distinctions. Fenced, indented, commented, and
nested examples are not live presentation fields. This is a small supported
Markdown dialect, not semantic Markdown normalization or a full CommonMark parser.

Moving a history block into a heading/prose/code span makes it covered and stales
approval. Appending covered prose after old machine history makes those earlier
blocks covered too. Explicit reapproval can bind that updated material. The
formatter verifies scope preservation; it does not invent covered headings or
silently recalculate approval after changing reviewed prose. Missing optional
presentation fields may stay absent; missing status is inserted as machinery.

Unknown versions, duplicate/malformed reserved records, ambiguous live fields, and
unsupported layouts fail closed. Notes cannot inject reserved delimiters, a live
duplicate decision section, or unclosed fences. Current machinery changes affect
the snapshot token even when they do not change scope, so two conditional writers
cannot both win merely because they selected the same scope.

## Effective status and legacy compatibility

- Valid bound approval with matching scope: `approved`.
- Bound approval with covered edits: `approval_stale` without rewriting the file.
- Legacy `Status: approved` without a binding: `approved_unbound`, requiring an
  explicit action before upgraded readers consider it effective approval.
- Bound presentation contradicting status/checklist: `decision_invalid`; an
  explicit decision can repair known presentation, never an automatic grant.
- Change requests remain `changes_requested` after scope edits, with mismatch
  diagnostics; edits do not silently clear rejection.

Dashboard, standup, review discovery, and next-action guidance use this same view.
Inspect/status/refresh do not upgrade legacy decisions or restore old approvals
from events. Stop older readers/writers that only trust `Status: approved`.
Hashes are not monotonic or authentication: same-user editors can forge metadata,
and restoring identical covered bytes can restore the same scope hash. Do not
claim edit-and-revert detection or protection from noncooperating writers.

## Commit and secondary outcomes

Read-only preflight rejects deterministic invalid requests before creating a lock.
Inside the persistent workspace lock, the service reloads records and bytes,
checks the expected snapshot, prepares/validates complete output, and publishes
with temporary-file fsync, atomic replace, and directory fsync.

The checkpoint replacement is authority. Event/message history and a fresh
dashboard are secondary attempts under the held lock. Corrupt observational logs
do not prevent a valid approval/revocation; their append failures become warnings.
No sidecar or event replay participates in approval authority.

| Result | Meaning |
| --- | --- |
| Exit 1 before publication | Rejected/conflicting/busy request; no decision replacement. |
| Exit 0, `outcome: committed` | Checkpoint published; secondary/notification warnings may remain. |
| Exit 2, `outcome: durability_uncertain` | Replacement occurred, directory sync failed; secondary work is skipped. Inspect before retrying. |
| Process death/lost response | Inspect current file; no checkpoint idempotency receipt or exactly-once retry guarantee. |

Results include decision identity/action, selection, before/after snapshot tokens,
scope, warnings, and notice counts. Unconditional retries create another decision
and history entry; a decision ID is correlation, not a durable request key.
Temporary files are never promoted as recovery. Universal power-loss durability
and whole-workspace transactions are not claimed.

Runtime notices happen **after lock release**, never while waiting on Herdr/tmux
or user input. Before each recipient, the service rechecks decision identity and
effectiveness and suppresses notices already known stale/superseded. A race after
that check remains possible: notices say to recheck current authority and are
never grants. `sent` means transport commands succeeded—not that an agent accepted
work or acted correctly. Failed transport/enter/history commands are warnings,
never rollback or ordinary failure of an already committed decision.
