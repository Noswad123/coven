# Coven Web recovery obligations

## Overview

This tracks unresolved **verification and retention obligations**, not an assertion
that historical worktrees, uncommitted deltas or stash entries still exist. The
documentation-split baseline is Coven `aea93a6`, with complete-title source committed.
Old handoff statements that Coven changes remain uncommitted on `2d1fb8d`, `1e88ca2`,
`c907fd5` or `a3f9fc1` are historical, not the current source baseline. Djinn's
present source/commit state and the availability of private recovery objects were
not checked in this documentation-only change.

## API / Interfaces

Recovery operates on exact source snapshots, named object IDs and owned artifacts,
not live agents. The original stash-object tables remain authoritative evidence
in their individual historical change-sets (not duplicated here):

| Recovery set | Exact object table | Required review |
| --- | --- | --- |
| Initial web, team isolation, native reader | [2026-11-19 reconciled deployment](../changelog/2026-11-19-coven-web-reconciled-deployment.md#rollback-and-removed-worktrees) | Coven `coven-web` / `coven-team-isolation`, Djinn `djinn-conversation-view` snapshots |
| Shared-source and follow-up implementation | [2026-11-19 sharing](../changelog/2026-11-19-coven-web-sharing-follow-ups.md#validation-rollback-and-cleanup) | Coven client/core and Djinn sharing snapshots |
| Native decisions, attention and push | [2026-11-20 attention](../changelog/2026-11-20-coven-web-attention-alerts.md#validation-and-cleanup) | Coven core/client/push and Djinn native-decision snapshots |

## Data Flow

Before any separately authorized recovery-snapshot cleanup:

1. Identify the exact repository/object and verify whether the recorded snapshot
   still exists; do not use changing stash ordinals as identity.
2. Verify integrated tracked **and untracked/index** source against committed
   successors. The prototype was preserved in
   `323dbc95ccb86f1c49b070f6735a40a3cdd679dd`; worker reconciliation used the full
   delta and 11 untracked files, not just a tracked diff. See the
   [local overlay](../changelog/2026-11-18-coven-web-local-implementation.md) and
   [reconciliation evidence](../changelog/2026-11-19-coven-web-source-reconciliation.md).
3. Confirm recovery is unnecessary in **both** repositories, including native
   companion changes and unrelated edits preserved during integration. The fact
   that Coven full titles are committed does not alone verify every older snapshot.
4. Remove only specifically identified, approved recovery entries after that
   review. Do not pop/drop unrelated stashes, force-remove worktrees, delete
   branches, or overwrite an evolving main checkout.

The historical local effort worktrees were reported removed normally after
preservation/integration checks. The original Megatron worktree and unrelated
worktrees/branches were retained in the records; neither their present existence
nor a need to delete them is established here. Historical source paths must not
be treated as newly assigned working directories.

## Edge Cases

### Unattributed run artifact

The [2026-11-20 pane-hierarchy record](../changelog/2026-11-20-coven-web-pane-hierarchy.md)
reported an unattributed 119 MiB disk-backed directory
`~/.cache/agent-test-runs/pn.HC9lGV`. Its creator could not be confirmed and it
was left untouched, including at the [2026-11-21 wrap-up](../changelog/2026-11-21-coven-web-complete-titles.md).
No readable active process reference at that time did not establish ownership.
Any future cleanup must first identify its owner and repeat the active-use check;
do not delete by age/name or blanket-delete the cache. Its present existence was
not verified here. All **owned** run/build/browser/IPC artifacts were reported
cleaned after child shutdown in the corresponding records.

### Private deployment recovery is separate

Retain matched immutable payloads and private configuration/unit/native-selection
backups until rollback is no longer needed. Their recorded paths, payload hashes
and supersession dates remain in [history](../changelog/index.md); immediate latest
rollback is extracted in [dated deployment state](../operations/COVEN_WEB_DEPLOYMENT_STATE.md#rollback).
Retired pre-rename prototype artifacts were already removed and must not be
assumed available. Recovery must not reset unrelated Serve routes, signal live
agents or silently enable best-effort output.

## Example usage

For a future cleanup review, start with one exact object table above, establish
repository/ownership and committed successor coverage, then record the decision
for that object only. This plan performs no Git mutation or artifact deletion
and does not authorize either.

## Related files

- [Plans index](index.md), [handoff history](../changelog/index.md),
  [runbook](../operations/COVEN_WEB_DEPLOYMENT.md),
  [dated state](../operations/COVEN_WEB_DEPLOYMENT_STATE.md).
