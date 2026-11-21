# Pane name / provider / session-title hierarchy deployed, 2026-11-20

Historical record; payload selection was superseded on 2026-11-21 by
[complete titles](2026-11-21-coven-web-complete-titles.md). [History index](index.md).

Card body order is now the explicit short pane name, plain provider, then the
existing long session title. Pane names come only from the exactly matched Kit
`panes[].label`, with the existing safe-display policy (160 UTF-8 bytes, no controls
or prohibited bidi/separator characters). Missing/invalid names display Unnamed
pane; titles, providers, CWD, tab/workspace labels and PIDs are not name fallbacks.
Chat/Details headings use the pane name. Long title cleanup is presentation-only,
and manual labels (including literal OC text) are preserved. Evidence, ambiguity,
project/team and coverage diagnostics remain in Details, not normal card text.

The additive helper DTO field defaults to None and is omitted when absent. The new
reader accepts older helpers. Reader/helper rollout was coordinated because older
strict readers cannot accept a present new field. All opaque IDs, binding proofs,
activity/output rules and decision/alert authorization remain independent of names.

Gateway and Megatron helper now use immutable prefix
`~/.local/libexec/coven-web/75bb7f3a5b8b0cc5/bin/coven`, SHA256
`75bb7f3a5b8b0cc5d229cb7b096f84a26fc5f3034c9289d4ea19003411cedcdc`.
The remote ABI/hash/default-loader probe passed with system OpenSSL 3. The old
`920bef44922edaf6` helper remains retained. Only the remote helper pin and gateway
ExecStart changed; restore both `observer.pre-pane-names.json` and
`coven-web.service.rollback-92d1b9c3` for paired rollback. Future dev Coven selection
is `coven-native-10x2s1ps`.

Live metadata reported one local agent with one manual label, and one remote agent
with no reported manual label. The latter falls back to Unnamed pane; the viewer
does not assert that another short tab/workspace title is its pane name. No pane
was renamed on the user's behalf.

133 Node tests including real Firefox, 153 observer tests, eight gateway tests,
two packaging tests, syntax/formatting/all-target check and release build passed.
Five producer opt-ins were not rerun for this presentation-only change. Candidate
and actual Serve HTTPS assets/metadata passed. Name rename/clear preserves exact
card bindings and routing. Only the gateway restarted; remote/native/Kit agents,
policy, Serve, VAPID key and the phone subscription were preserved. All owned
artifacts were cleaned, with no worktrees, commits or pushes created. Refresh the
phone page; no native restart or alert re-enablement is needed. Source remains
uncommitted on top of the user's `2d1fb8d` commit.

An unattributed 119 MiB disk-backed run directory,
`~/.cache/agent-test-runs/pn.HC9lGV`, was found at wrap-up. No readable active
process references were found, but its creator could not be confirmed (the final
validator's recorded run was a different, already-cleaned directory). It was left
untouched rather than deleted based on age or naming. Any later cleanup must first
identify its owner and repeat the active-use check; do not blanket-delete the cache.
