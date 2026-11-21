# Redundant title prefix removed, 2026-11-20

Historical record; later on 2026-11-20 [pane hierarchy](2026-11-20-coven-web-pane-hierarchy.md)
changed headings, and [complete titles](2026-11-21-coven-web-complete-titles.md)
published the native title change on 2026-11-21. [History index](index.md).

Coven Web now removes one exact leading `OC | ` decoration from known Djinn
display titles in cards, chat headings and Details. Backing metadata, session names,
opaque IDs and bound navigation are unchanged. Other providers and non-leading
literal uses are preserved. Live metadata showed two decorated Djinn titles and
zero decorated captions after this presentation helper.

Gateway: `~/.local/libexec/coven-web/92d1b9c340d43598/bin/coven`, SHA256
`92d1b9c340d4359821d2fe4c5d2cbe88a41838746d11452b5659bad43b431ec4`.
The exact old unit is `~/.config/systemd/user/coven-web.service.rollback-3fde7b16`;
future dev selection is `coven-native-j4z66wpw`. 129 Node tests (including Firefox),
eight gateway and two package tests passed; five producer opt-ins were not rerun
for this presentation-only release. Real HTTPS assets/metadata checks passed.
The phone subscription/VAPID key, policy and running native agents were preserved.
Refresh the phone page; no native restart or alert re-enablement is required.

Djinn source at `c9398c4` additionally emits unadorned named-session/plugin titles
and uses Djinn rather than OpenCode for home/default titles. Existing truncation,
disable preference and stored session names are unchanged. TUI validation passed
231 tests with one skip and package typecheck. Existing fixtures warned about
hardcoded `/tmp/opencode/state/kv.json` reads; no data was created there. That native
source fix is uncommitted and was not built/published or activated in this slice;
it takes effect in a future native build. All owned test/build artifacts were
cleaned; no worktrees, commits or pushes were created.
