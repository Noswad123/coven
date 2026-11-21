# Normal-launch sharing and follow-ups deployed, 2026-11-19

Historical record: “current” and “below” refer to the original handoff chronology.
The Conversations workflow was superseded later on 2026-11-19 by
[unified cards](2026-11-19-coven-web-unified-cards.md) and
[BASIC one-click](2026-11-19-coven-web-basic-one-click.md); decisions were added
on 2026-11-20 in [attention](2026-11-20-coven-web-attention-alerts.md).
See [recovery obligations](../plans/COVEN_WEB_RECOVERY.md) before stash cleanup.
[History index](index.md).

This is the current record; the deployment and implementation overlays below are
historical. The previous read-only/manual-registration gap is now addressed for
normal **local** full-TUI launches after one explicit user-level enablement.

### User workflow and scope

1. Finish/save current work, exit the old UI, and relaunch Djinn normally on
   Bumblebee (`djinn --ui` through the existing dev selection). No per-launch socket
   variable or manual session/runtime ID is needed.
2. Open the desired root session in the local Chat view. Its hydrated selection
   publishes a short-lived native grant; leaving/deselecting expires/revokes it.
3. Reload the phone page at `https://bumblebee.tail97624b.ts.net` and use
   **Conversations**, not process-discovery fleet cards. Authorized writable
   conversations have a follow-up composer. Read refresh never sends a prompt.

The new setting is captured at worker startup. Existing processes were not
restarted/signaled and cannot gain the integration by rebuilding or changing the
setting alone. No live-chat or live-model acceptance is claimed until a real new
session is deliberately selected. Permission/question answering, remote native
adapters (including Megatron), mini/headless/explicit HTTP and full-history views
remain outside this slice. A follow-up is not approval of an outstanding request.

### Installed setup

- Gateway: `~/.local/libexec/coven-web/e085732203c643c5/bin/coven`, SHA256
  `e085732203c643c53c070bbca012181a9d548115479905d33c68c6782c050b71`.
  Future public dev Coven launches use `coven-native-vvurzumu`.
- Future native UI launches select bundle
  `f92f874abcea6255d269d19d0b1b3114dbdf737244e3bc70a8a8e45f15031078`
  under `~/.local/libexec/arcana-dev/djinn/bin/.djinn/`. The CLI bytes are unchanged.
- `~/.config/djinn/coven-web.json` is owned/0600, mode `read_write`, registry
  `/run/user/1000/coven-web` (0700). The dotfile-managed parent symlink is preserved;
  the loader validates/fences its trusted canonical directory and no-follow leaf.
- The observer has one `local-djinn` source for team `development`, with the
  existing sole owner as reader/writer. The control workspace is
  `~/.local/state/coven-web/control` (0700), initialized blank without launching
  any agent, provider or tmux runtime. Native working directories remain separate.
- The two fleet endpoints, global allowlist, disabled best-effort terminal output,
  Serve route, public wrappers and Arcana source registry were preserved. Only the
  gateway was restarted. Same-user local processes remain trusted; this is not
  hostile-local-user isolation.

Native authority is now a five-second selected-root/context grant, not knowledge
of a SID. Old references cannot retarget on selection/reload/runtime changes.
HTTP submissions require resource writer permission, exact Origin and CSRF, bounded
strict JSON, and the existing control-workspace admission gate. Lost receipts remain
uncertain; attempts are not automatically retried. The browser retains sanitized
uncertainty independently per originating conversation, without retaining prompt text.

### Validation, rollback and cleanup

- Main-checkout Node suite: 53 passed. Scoped Coven Rust checks passed, including
  68 observer units, 19 prompt workflows, all-target check and release build.
  The two separately invoked actual sharing-producer HTTP tests and the older
  read-producer fixture passed. They use synthetic native services, not user chats.
- Native main-checkout baseline: 219 distinct tests passed, one optional hosting
  test skipped, and package typecheck passed. After the config-alias fix, 22 sharing
  tests and package typecheck were rerun. These counts overlap; do not sum them.
- Compiled normal-TUI sandbox checks passed before/after publication with a
  symlinked config parent, automatic endpoint publication, shared capability and
  no grant until selection. No socket/SID override, transcript or model call.
- Candidate, live loopback and actual Serve HTTPS checks passed for health,
  assets/composer, metadata, CSRF and unauthenticated/unknown-target write refusal.
  There were zero real native grants at rollout; a deliberate UI restart remains
  the activation boundary. Full regression/phone-prompt/model acceptance is not claimed.
- Strict Clippy still reports only the five known unchanged-code style findings.
  All owned test/build/smoke directories and children were cleaned.

Immediate gateway rollback copies are
`~/.config/coven-web/observer.pre-web-sharing.json` and
`~/.config/systemd/user/coven-web.service.rollback-f0904e1d` (both 0600).
Restore only those config/unit contents, reload the user manager and restart only
`coven-web.service`; leave Serve and active native agents alone. Previous binaries
and older rollback copies remain retained.

Source is integrated into main checkouts at Coven `c907fd5` and Djinn `9b33615`
as uncommitted changes. The three new effort worktrees were removed normally after
verified integration and complete recovery snapshots:

| Removed worktree | Repository | Recovery stash object |
| --- | --- | --- |
| `coven-web-sharing-client` | Coven | `85778eed50cffa8a079b642a5e099722a076dbbb` |
| `coven-web-sharing-core` | Coven | `24cc1b6653101617d5e3b8b68c0f539ef1b894d4` |
| `djinn-web-sharing` | Djinn | `0674788474ccbf85fc3bfd436bedf5e89c840507` |

Retain these exact recovery entries until integrated source is committed and no
longer needs recovery. Unrelated worktrees, branches, old recovery stashes and
remote worktrees were not removed. No source commits or pushes were made.
