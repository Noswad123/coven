# Current deployment and cleanup, 2026-11-19

Historical heading: this is **not current deployment state**. Normal-launch sharing
superseded manual activation later on 2026-11-19 in
[sharing and follow-ups](2026-11-19-coven-web-sharing-follow-ups.md).
See [dated latest state](../operations/COVEN_WEB_DEPLOYMENT_STATE.md) and
[recovery obligations](../plans/COVEN_WEB_RECOVERY.md). [History index](index.md).

This section supersedes the historical worker/reconciliation/deployment records
below. Source now resides in the normal main checkouts:

- `/home/jdawson/Projects/coven`, base `a3f9fc1`, with unstaged/uncommitted feature
  changes and current-main agent-management integration preserved.
- `/home/jdawson/Projects/djinn`, base `88e2042`, with the native reader added while
  preserving the pre-existing attachment/status work. Nothing was committed or
  pushed during reconciliation and rollout.

### Published and running

- `coven-web.service` runs
  `/home/jdawson/.local/libexec/coven-web/f0904e1da389ab1a/bin/coven`.
  Its SHA256 is
  `f0904e1da389ab1a65954dfaeb2e2e38e99fff6a8a37bb5871b15b43c2d80d32`.
- The existing tailnet-only site remains
  `https://bumblebee.tail97624b.ts.net`, proxying only to `127.0.0.1:7341`.
  The Serve route, observer configuration, login policy and two selected fleet
  endpoints were unchanged. Best-effort terminal output remains disabled.
- Future public dev Coven launches select `coven-native-y6fzrkjz`; the old
  `coven-native-agc3h45d` payload is retained. The public wrapper and Arcana registry
  were unchanged; the existing installer atomically switched its dev prefix.
- Future Djinn UI launches select compiled bundle
  `2b75443906a85635e8e28bf0bb4a915f1e952ca012a204c0678c409768aa027e`
  under `~/.local/libexec/arcana-dev/djinn/bin/.djinn/`. The CLI bytes are unchanged.
  The previous `9ac92fd0...` bundle is retained. No existing native agent/UI/Coven
  process was restarted or signaled; only the web gateway was restarted.

**Conversation activation is still explicit.** The new website currently returns
an empty authorized Conversations list: no native target was guessed or registered.
Existing UI processes were started without `DJINN_PROMPT_SOCKET` and cannot acquire
one from binary installation alone. Start a new full TUI with the published UI and
a private socket, obtain its runtime UUID, and deliberately register the exact root
session and allowed logins using [the configuration contract](../operations/COVEN_WEB_DEPLOYMENT.md#configuration-and-cli). Do not
infer a session from a pane/PID/title or silently follow a runtime replacement.

### Validation of reconciled main

- 39 Node and 273 focused Rust checks passed, including current-main observations
  and presentation regressions, isolation/lifecycle/parity, install/package, and
  the separately invoked real-producer HTTP fixture. Cargo all-target check and
  formatting passed.
- After a two-line lifecycle borrow cleanup, 27 targeted lifecycle/isolation
  checks and the final release build passed. These overlap the earlier scope and
  are not an additional unique-test total. Strict Clippy still reports five
  pre-existing unchanged-code style findings; new lifecycle findings were fixed.
- Djinn: 103 reader/session/instance tests, 85 focused native/attachment tests and
  14 selected-session tests passed; package typecheck and compiled capability
  probes passed. One opt-in full Coven/Kit hosting test was skipped. The existing
  models snapshot and dependencies were reused without dependency installation.
- Isolated candidate, live loopback and actual Serve HTTPS checks passed for page,
  new assets, health, fleet schema, empty Conversations list, unknown reference
  refusal and identity admission. This is not phone/cellular or real-chat acceptance.
- Owned test/build directories and children were cleaned. Full regression and
  physical-device/provider-model acceptance remain separate gates.

### Rollback and removed worktrees

The original unit is retained byte-for-byte at
`~/.config/systemd/user/coven-web.service.rollback-920bef44`, alongside its old
`920bef44922edaf6` payload. Restore only that unit's content, reload the user manager
and restart only `coven-web.service` if rollback is needed. Keep the Serve route and
live agents unchanged. Native/dev selection rollback must preserve the matched
payload and should not terminate existing sessions.

All three effort-owned **local** worktrees were removed normally after source and
index/untracked preservation checks. Named Git recovery stashes remain until the
integrated work is committed and recovery is no longer needed:

| Removed worktree | Repository | Recovery stash object |
| --- | --- | --- |
| `coven-web` | Coven | `50f5b8645b89ca217234c55072c442a2766c2630` |
| `coven-team-isolation` | Coven | `f4ff432c2c614b07b4d60fc60304b54661f5a1b2` |
| `djinn-conversation-view` | Djinn | `6917b18bf4192dd4fe055e55bbf4b3c3469b0ad2` |

Do not pop/drop unrelated stashes; later recovery-snapshot cleanup must identify
only these exact entries after integrated source is committed. The unrelated
`coven-rust-migration`, `coven-worker-handoffs`, both main checkouts, and the remote
Megatron worktree were retained. No branch deletion or forced removal was used.
