# One card grid, verified conversation navigation

## Current BASIC product contract

This supersedes earlier picker/manual-choice fallbacks below. A basic monitoring
card represents one observed agent and, when connected, one exact current native
conversation. Its first primary click opens that pinned chat directly. There is
no first-use picker, manual card association, Shared chats chooser, extra native
fallback card or history selection prerequisite. A missing, ambiguous, stale or
unsupported link is explicitly **Chat unavailable**, with Details separate.

Complete the native association plumbing rather than asking the user to provide
identity. Keep the root fleet-card set unchanged, and do not infer links from
names, hosts, providers, PIDs, order or a single candidate. The existing verified
bound-read API remains authoritative. Swarm/group cards and history navigation
are separate future product modes, not ambiguity in BASIC monitoring.

Normal native startup must tolerate bounded transient readiness/capture failures
until its first successful pinned context. Cancel pending acquisition on shutdown,
never rebind after success, and do not let unchanged one-second heartbeats starve
valid slower qualification. These are source/runtime fixes, not authorization to
retrofit an old compiled worker, restart active agents, or claim live one-click
readiness without a grant-correlated association.

The user-facing model is one session-card grid, not separate fleet and conversation
sections. Fleet observations and native conversation grants remain distinct internal
facts. Never join by hostname, label, provider, PID, array order, or a single candidate.

## Native provenance (optional, backward compatible)

Keep all existing sharing methods and closed grant schemas unchanged. Automatic
endpoints with the new callback advertise a separate capability:
`card_association:{contract:"djinn.card-association.v1",version:1}`.

New metadata-only method `share_association` has exact params
`{expected_runtime_id,session_id,grant_id}` and result
`{type:"share_association",contract:"djinn.card-association.v1",grant_id,session_id,association}`.
Association is null or exactly:

```text
{instance_id, session_id, receiver_epoch, scope:"local_full_tui_root", lease_ms,
 target:{socket,device,inode,terminal_id,runtime_generation}}
```

Use the existing attachment worker's actual unique native reporter qualification
and captured attachment context. Do not originate provenance merely from the common
frontend callback. Cache its successful qualified selection with a monotonic deadline
anchored before qualification, at most five seconds; unchanged heartbeats may keep
an unexpired sample while refreshing it. Changed/deselected selection, reset/close,
reporter removal/ambiguity/replacement or expiry invalidates the association before
awaiting further work. Snapshot access must check the current reporter object and
SID; it never refreshes activity or makes an attachment assertion.

The endpoint rechecks its sharing grant and native fence before and after querying
provenance. SID must match both grant and association. Missing attachment provenance
returns null without disabling native sharing. Existing endpoints without this
capability remain usable through the shared-chat picker. This is qualified local provenance,
not hostile-same-user authentication or execution authority.

## Gateway verification

Apply conversation source/target ACLs before inspecting native or Kit endpoints.
Only query Kit paths explicitly configured as local fleet endpoints (never SSH
endpoints or publisher-selected arbitrary paths). Require:

- exact configured canonical socket/device/inode matching the native target;
- a fresh snapshot whose inventory generation matches the cached fleet observation;
- exact terminal/pane/workspace/tab correspondence, and a live `terminal.inspect`
  with the same terminal runtime generation;
- a current native grant and unexpired qualified provenance;
- uniqueness in both directions among authorized targets: multiple eligible
  conversations for one terminal/card are ambiguous, not a reason to pick one.

Bound all metadata acquisition and do no transcript prefetch. Do not hold shared
state locks during native/Kit I/O. Existing observer/helper wire formats must remain
compatible with the installed remote helper. Native-only grants remain available
when no fleet association can be established.

Conversation list rows optionally add `association_status` (`verified`, `unavailable`,
or `ambiguous`) and `card:null|{fleet_agent_id,fleet_endpoint_id,binding_id}`.
All linkage IDs are opaque; raw targets/SIDs/reporters/paths stay private. Stable
bindings include daemon/policy epoch, authorized conversation, native grant and
provenance, Kit incarnation and fleet ID; refresh must not invalidate an unchanged
live binding merely because I/O has started. Expiry/failure/replacement removes it.

A merged-card read uses an exact bound route:
`GET /api/v1/agents/<agent>/conversations/<conversation>/bindings/<binding>/messages`.
Revalidate the link and pinned conversation before returning content; a stale click
must never resolve whichever session is now selected. Ordinary native-only reads
and exact-conversation prompt admission remain unchanged. New route admission,
blocking-job capacity, no-store headers, ACLs and late-response handling match
existing conversation reads. Do not add generic terminal/RPC selectors.

## Client

The root Sessions grid contains **only fleet observations**. This policy supersedes
the earlier third/native-fallback-card design: two fleet observations plus one
unlinked authorized conversation produce exactly two root cards. Never hide a fleet
agent to avoid duplication. Keep the fleet title, provider and project intact;
conversation/source labels do not replace terminal identity.

Primary click on a verified root opens the exact bound conversation read. An
unlinked root always opens a small conversation-picker dialog, including when zero
or one authorized targets exist. Require explicit choice even for one target. Show
source label, team and the warning that this is not a verified terminal link; do not
guess from labels, host, provider, order or a single candidate. Ambiguous/unlinked
targets may be chosen. Targets verified to another root are disabled with an
explanation, not assigned to the wrong root. Details / terminal output is a separate
sibling button on every root, never nested inside its primary button.

A page-local Map stores only `root opaque endpoint/card key -> {id, team}`. Later
clicks on that same valid root open exactly the chosen native ID/team with
`binding: null`, using the normal native read route. Card and conversation view say
**Selected by you—not a verified terminal link**. This is client navigation only,
not identity, a grant, execution readiness or authority. Selecting the same chat on
another root moves the preference rather than implying both terminals share a
session. Choices are bounded by root count and never persisted; no prompt text,
socket, SID or localStorage is involved.

A future verified association takes precedence. Same-target verification upgrades
the binding and fences outstanding reads/drafts; contradiction clears the manual
choice and closes the old view rather than retargeting it. Lost verification does
not restore a discarded choice. Metadata expiry/failure, authentication failure,
hidden page, removed/replaced references and root removal clear affected choices
and fence pending read/token/post callbacks. Native grant/reference disappearance
never follows the latest selected SID. Existing ACL, CSRF, permission, native
identity and uncertain-attempt-origin protections remain unchanged.

A small global **Shared chats (N)** toolbar button opens the same explicit picker
without assigning a root. It preserves authorized-chat access when fleet is empty
or unavailable. Global selection always uses the exact normal native route, even
if verified metadata exists. Global conversation validity is independent of root
projection/filter membership. Root-origin views track their original root separately
and must still resolve its same chosen/verified target; they never follow a new one.

Use one host/team filter area and refresh flow. Retain exact conversation handles,
metadata leases, keyboard focus, inert rendering, prompt uncertainty and independent
authorization state. Hosts are configured fleet endpoint IDs only; there is no
unlinked-native host bucket. Team filters include only explicit verified or
user-chosen chat team metadata and explain unassigned terminal observations.
Binding changes fence pending merged-card reads; no automatic
transcript reads/posts on metadata refresh. No separate visible Conversations list
or duplicate refresh section remains. Preserve useful metadata-only source
coverage/error messages. Modal focus stays inside the active dialog. On close,
resolve the original root primary (even if its choice disappeared), Details action,
or global toolbar opener; fall back to Host if that root is no longer visible.

Tests include same-label agents, distinct same-directory SIDs, source ACL exclusion,
competing runtimes/aliases, Kit endpoint/incarnation replacement, stale click and
A-to-B-to-A, global-picker fallback, explicit manual choice/move/conflict,
metadata without transcript reads, focus retention,
filters and unchanged follow-up/uncertain-delivery protections. Existing runtimes may
need a later deliberate restart for new provenance, but explicit picker navigation
works immediately without one. No automatic root match is claimed before verified
provenance is available. Remote native conversation support remains separate.
