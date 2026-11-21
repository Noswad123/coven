# Unified session cards deployed, 2026-11-19

Historical native-only-card fallback, superseded on 2026-11-19 by
[root-card correction](2026-11-19-coven-web-root-card-correction.md), then
[BASIC one-click](2026-11-19-coven-web-basic-one-click.md). [History index](index.md).

The site now has one **Sessions** grid, shared Host/Team filters, and one refresh
flow. There is no separate Conversations section. Chat-capable cards open the exact
authorized conversation; verified associations merge the chat and fleet observation
into one card, with terminal details secondary. Unlinked chats remain usable as
clearly labeled native-only cards. Unsupported/ambiguous observations show honest
details rather than opening a guessed conversation.

The private association contract and gateway checks are documented in
[CARD_CONVERSATION_NAVIGATION.md](../architecture/CARD_CONVERSATION_NAVIGATION.md). Matching requires
qualified native reporter/SID provenance, a current sharing grant, configured local
Kit endpoint identity, exact terminal/pane/workspace/tab mapping, live terminal
incarnation, bounded leases and bidirectional uniqueness. Pinned bound reads reject
stale clicks instead of following a newly selected session. Existing source ACLs,
CSRF, prompt admission and no-retry/uncertainty semantics are unchanged.

Gateway artifact:
`~/.local/libexec/coven-web/1d0e4b56d4631f4c/bin/coven`, SHA256
`1d0e4b56d4631f4c152356adf7db6419eacad0f28980bf5624274dccea0ce337`.
The exact previous unit is retained at
`~/.config/systemd/user/coven-web.service.rollback-8b1d3f7a`.
Future public dev Coven launches select `coven-native-v8qc2lx_`.

Future native launches select bundle
`e4112eb7ee01153a0d3fcc0c0ce52a1d90af910ea4853345d176b6dbd6868e5d`;
the CLI bytes are unchanged and the preceding bundle remains retained. A compiled
normal-TUI sandbox advertised the new optional association capability. Existing UI
processes were not restarted: the live rollout observed two fleet cards, one
unlinked writable native card and zero merged cards because the running UI lacked
that optional capability. Refreshing the phone immediately gives the unified grid;
a later deliberate Djinn restart enables provenance for merging its existing card.
No restart is required merely to use an unlinked native chat card. Remote native
association/chat, including Megatron, remains unsupported rather than inferred.

Validation: 76 Node tests; all 12 card-link tests within the scoped observer suite;
shared-native, prompt, conversation, HTTP, installation/package, and separately
invoked actual sharing/read-producer fixtures passed. All-target check, formatting
and release build passed. Clippy retained only the five known unchanged warnings.
Native final scoped validation passed 249 distinct tests and package typecheck.
Candidate/installed compiled startup and real gateway HTTPS asset/metadata/refusal
checks passed. Real-browser keyboard/visual and real merged-user-chat acceptance
were not performed. No transcripts or live prompts were used for rollout checks.

Only the web gateway was restarted; configuration, policy, Serve, remote helpers and
active native agents were preserved. All owned build/test artifacts were cleaned.
No new worktrees, commits or pushes were created; changes remain in the normal main
checkouts alongside the previously uncommitted implementation.
