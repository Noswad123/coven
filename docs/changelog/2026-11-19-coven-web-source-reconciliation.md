# Main source reconciliation overlay, 2026-11-19

Historical source-only record, superseded later on 2026-11-19 by
[reconciled deployment](2026-11-19-coven-web-reconciled-deployment.md).
Worktree and uncommitted-state assertions below are not current obligations;
see [recovery review](../plans/COVEN_WEB_RECOVERY.md). [History index](index.md).

The complete Coven prototype, web/conversation and agent-directory implementation
has now been reconciled as **unstaged, uncommitted source changes** into
`/home/jdawson/Projects/coven`, on `main` at `a3f9fc1`. Selection used the combined
`coven-web` working tree's delta against `a541e9f`, including prototype commit
`323dbc9` content and all 11 untracked implementation/test/doc files, rather than
copying its older integration files over current main.

Current-main agent-management, native integration and normal-workflow changes are
retained. The closed `PaneRead` method and its transport test were added alongside
the newer guarded-view transport and tests; module declarations were combined.
The local implementation and validation records below describe the earlier worker
baselines, **not test or activation results against this reconciled main**.

Both `/home/jdawson/Projects/coven-web` and
`/home/jdawson/Projects/coven-team-isolation` remain preserved and untouched,
including the former's pending no-commit merge/index. No staging, commit, build,
rollout, live configuration change or worktree deletion was performed during this
reconciliation. Build/test/rollout verification remains with the integration owner;
retain the source worktrees until safe integration has been verified.

Source-only checks on this overlay passed: all 55 files other than this handoff and
the two socket merges match the selected source exactly, including module
declarations. Both socket deltas match the prototype additions exactly, and all 19 main-only files
changed since `a541e9f` remain equal to `a3f9fc1`. Rust formatting, JavaScript and
Python syntax, and diff whitespace checks passed. These are not build, runtime,
test-suite or rollout acceptance results.
