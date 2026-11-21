# Root-card conversation correction deployed, 2026-11-19

Historical picker policy, superseded later on 2026-11-19 by
[BASIC one-click](2026-11-19-coven-web-basic-one-click.md). [History index](index.md).

This supersedes the native-only root-card fallback below, which duplicated the
user's existing session card. Root cards now come **only from fleet observations**:
the live rollout's two observations plus one shared chat produce exactly two root
cards. Chat availability no longer appends a third tile.

Verified root links still open their pinned conversation directly. An unlinked
root opens an explicit conversation picker, even if there is only one candidate.
The user may choose an exact authorized ID/team for that root; subsequent clicks
reuse that page-local choice while valid. It is labeled as user-selected, not a
verified terminal association. Choices move rather than alias between roots, and
expiry, revocation, reference replacement, hiding or conflicting verified metadata
clears/fences them. A small Shared chats button preserves access without fleet
rows; it does not create another root card. No session is chosen by label/order.

Gateway artifact: `~/.local/libexec/coven-web/26e48daf7be7b788/bin/coven`, SHA256
`26e48daf7be7b7888fd57e132c896618576c6f240462b863031c7e1ddf29d981`.
Future dev selection is `coven-native-uvp_ofhc`. The previous unit is retained at
`~/.config/systemd/user/coven-web.service.rollback-1d0e4b56`.

Validation passed 85 Node tests, including installed Firefox real-DOM phone/desktop
coverage, eight gateway tests and two packaging tests. Three producer opt-ins were
not rerun for this frontend-only change. Syntax, formatting, release build and live
HTTPS assets/metadata checks passed. Only the gateway was restarted: **no Djinn
restart is needed for this interaction**, and no native/backend association was
forged. Source/configuration policies, Serve and active agents were preserved.
Owned test/build/browser artifacts were cleaned; no worktrees, commits or pushes
were created. Refresh the phone page to load the correction.
