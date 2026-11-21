# Local implementation overlay, 2026-11-18

Historical worker-baseline record; source reconciliation and rollout superseded
these locations/statuses on 2026-11-19. See [source reconciliation](2026-11-19-coven-web-source-reconciliation.md),
[reconciled deployment](2026-11-19-coven-web-reconciled-deployment.md), and
[history index](index.md).

The deployment/source-location sections below record the earlier prototype handoff.
That prototype was subsequently preserved on `coven-web-observer` in commit
`323dbc95ccb86f1c49b070f6735a40a3cdd679dd` and pushed. Its original Megatron
worktree was retained. The following new implementation is **local, uncommitted,
and not deployed**:

- `/home/jdawson/Projects/coven-web`: combined Coven implementation. The saved
  prototype has a pending, conflict-resolved **no-commit merge** of committed
  `main` at `a541e9f`, plus the new web and team-directory changes. Automatic
  merge results are staged; new implementation edits include unstaged and untracked
  files. Review both the index and working tree, including new files. No merge
  commit or additional push has been made.
- `/home/jdawson/Projects/coven-team-isolation`, branch `work/team-isolation`:
  retained isolation worker source. Its changes were copied into the combined
  worktree while preserving the web modules.
- `/home/jdawson/Projects/djinn-conversation-view`, branch
  `work/native-conversation-view`: the companion native reader and tests, based on
  committed Djinn `main` at `88e2042`.

The original Coven and Djinn main worktrees have ongoing unrelated, uncommitted
integration changes. They were not edited or copied into these worker worktrees.
**The combined checks cover the named committed baselines plus this implementation,
not those evolving main-worktree edits.** Reconcile those changes before preparing
an installed rollout; do not overwrite either main worktree.

### Delivered first slice

1. [Agent directory isolation](../guides/AGENT_DIRECTORY_ISOLATION.md): explicit full-roster
   mapping to distinct, existing Git working-tree roots for legacy team launches.
   Coordination state stays in the control directory. No automatic worktree
   creation/deletion, relocation of running panes, security sandbox or scheduler
   is claimed. Reviewed `runtime_launch` v1 explicitly refuses this new mapping.
2. [Native conversation viewing](../guides/COVEN_WEB.md#native-conversations-new-builds-only): explicit local socket/runtime/session
   registrations with per-resource Serve-login authorization. A separate
   Conversations list opens bounded, inert user/assistant text. No guessed binding
   from process/pane cards, terminal-output fallback, prompt submission or decision
   response was added. Remote native conversation endpoints remain unsupported.
3. Djinn's opt-in full-TUI private endpoint advertises
   `djinn.conversation-read.v1`. It reads already-active native contexts, limits
   persisted-record acquisition before materialization, and returns at most 20
   messages / 8192 UTF-8 text bytes. Its digest identifies the returned projection,
   not a transactional whole-conversation revision.

### Validation and operational boundary

- Combined Coven: native-conversation units (5), observer units (44), HTTP gateway
  tests (6), isolation acceptance, selected legacy-lifecycle/runtime-launch/refresh
  parity workflows, native install (9) and native package (2) checks passed.
- The separately invoked, default-ignored real Djinn socket producer through the
  authenticated HTTP gateway passed. Its messages are synthetic; it is not live
  native DB/TUI, provider/model, physical browser or tailnet acceptance.
- Final Node suite: 39 passed, including authorization invalidation, metadata
  expiry, deferred responses and keyboard-focus preservation.
- Djinn: 103 focused tests and its package typecheck passed. Independent native,
  isolation and web reviews were performed; identified correctness issues were
  fixed with regression coverage.
- Combined all-target Cargo check, formatting and whitespace checks passed. The
  full regression gate and a fresh strict Clippy run were not performed.
- Owned test/build/fixture directories were cleaned after child shutdown; unrelated
  run directories and shared package caches were preserved.

No installed executable, provider configuration, live agent, systemd unit or
Tailscale Serve route was changed. Activation needs separately reviewed builds,
explicit native endpoint startup, authorized target registration, and a deliberate
gateway rollout. Existing sessions are not retrofitted or restarted automatically.
