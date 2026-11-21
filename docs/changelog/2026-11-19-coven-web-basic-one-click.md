# BASIC one-click UI and approved copy cleanup deployed, 2026-11-19

Historical record: “current” below refers to this UI policy change, superseding the
[picker correction](2026-11-19-coven-web-root-card-correction.md) on 2026-11-19.
The Private badge was subsequently replaced on 2026-11-20 by
[quiet UI](2026-11-20-coven-web-quiet-ui.md). [History index](index.md).

This is the current record and supersedes the picker/manual-choice fallback below.
Basic root cards now open their one exact verified conversation on the first click.
There is no picker, manual binding, Shared chats chooser or third native fallback
card. Unlinked, stale, ambiguous or unsupported observations show **Chat unavailable**
with Details separate; no identity is guessed to make the card look ready.

The user confirmed concurrent HTML edits and requested deletion of elements marked
"Delete This". Those paragraphs and the marked footer were removed. "Agent
Management", "Private", the shorter coverage text and the retained heading were
preserved. Orphan footer/paragraph CSS was removed and shell bottom padding supplies
phone/desktop breathing room without an empty footer.

The gateway now runs `~/.local/libexec/coven-web/2bc5cde074cbb2d3/bin/coven`, SHA256
`2bc5cde074cbb2d33c0eaee0f4e1cf6bb258f8d7c0ff5234d460228189db2a96`.
Future dev Coven selection is `coven-native-3ba7akbi`; the exact preceding unit is
`~/.config/systemd/user/coven-web.service.rollback-26e48daf`. Configuration, source
ACLs, Serve and native processes were not changed by the gateway deployment.

Native startup now has bounded read-only initial readiness/target-capture recovery
until its first pinned context. Unchanged heartbeats no longer starve slow valid
qualification; true selection/context changes still revoke immediately. Future UI
launches select bundle
`de541eb6dc173317ff1eec142b5448b3e78a0a4538110a6e05ede4a67e17be99`.
Its CLI bytes are unchanged; older bundles remain retained. These changes cannot
retrofit the old compiled worker. The last live metadata showed two roots, one
unlinked shared chat and zero verified links: one-click live reading is **not yet
established** until a deliberate updated-UI relaunch supplies provenance. No active
agent was restarted or signaled. Refreshing the phone loads the new UI contract;
it does not itself activate native matching.

Validation: 89 Node checks including real Firefox, eight gateway and two packaging
tests passed; three producer opt-ins were not rerun for this UI-only release.
Formatting, syntax, release build and candidate/live HTTPS asset/copy/metadata
checks passed. Native final combined scope passed 151 tests and typecheck; an
initial receipt-timing failure needed an unchanged-budget rerun and is not claimed
as a root-cause fix. The overlapping thread suite passed 11 tests again. Network-
denied compiled normal-startup checks passed before/after native publication with
automatic capabilities and no grant before selection, without dependency reification,
model calls or user chats. This is not real selected-session/phone chat acceptance.
All owned test/build/browser/helper artifacts were cleaned. No worktrees, source
commits or pushes were created; existing unrelated edits were preserved.
