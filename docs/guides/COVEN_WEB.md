# Coven Web browser guide

## Overview

Coven Web is Coven's phone-first browser client, not a separate product, agent
manager or agent runner. Source lives in `clients/web/`; Coven owns the neutral
observation API, authentication,
configured endpoints, deadlines and attribution. Co-host static assets and API
behind Tailscale **Serve**, never Funnel. Existing agent processes and the installed
Coven executable are not replaced as a prerequisite.

### First slice and protected behavior

- Read-only cards from explicitly configured existing Kitsune protocol-18 sockets.
  Combine Kitsune-reported agent metadata with Coven's existing fenced process
  discovery; an empty Kitsune `agents` array is not a complete agent inventory.
- Endpoint health is separate from heuristic agent activity. Idle is not completion;
  blocked is not an authoritative pending approval. No invented native authority.
- Endpoint-scoped opaque agent references. Pane/workspace locations are not identity.
- A failed refresh visibly retains stale last-known cards; a successful empty
  snapshot removes them. Reconnect/replacement invalidates output references.
- Output is explicit and bounded, never prefetched, persisted, logged, rendered as
  HTML or interpreted as a terminal. No service-worker caching or analytics.
- Terminal-only cards offer no prompts, approval emulation, keyboard input, focus, launch, stop, handoff,
  generic RPC, browser-selected socket/host/command or automatic server startup.

This is protocol/security-sensitive additive work. Characterization tests cover
the existing observation method allowlist and bounded wire behavior, colliding
terminal/pane IDs across endpoints, replacement, failed second snapshots,
timeouts, response caps, stale retention versus successful-empty, hostile origins,
missing identity and inert text rendering. Existing adapters retain their semantics.

## API / Interfaces

The [public contract](../architecture/COVEN_WEB_CONTRACT.md) defines DTOs, display
fields, exact bound read routes, identity and authority. A card displays the manual
pane name, plain provider, then complete supported session title; names are not keys.
Operator configuration belongs in the [deployment runbook](../operations/COVEN_WEB_DEPLOYMENT.md),
not browser selectors.

## Data Flow

### Current metadata updates

Visible pages use content-free foreground SSE to invalidate fleet, conversation
links and attention metadata. `ready` requires a full GET resynchronization;
opening a connection alone does not prove authorization. Fleet acquisition still
polls upstream three seconds after work with backoff. Qualified local endpoints can
use [bounded event hints](../architecture/COVEN_WEB_LOCAL_EVENTS.md) to accelerate
normal acquisition and subsequent metadata collection, not to supply display truth
or extend freshness. Topology and supported stripped-title changes can hint;
manual labels, raw-title-only and foreground-process changes may wait for periodic
reconciliation. Remote SSH endpoints remain polling-only; fewer polls, instant
updates and local native grant/decision event coverage are not promised. A shared
server collector supplies list/attention cache snapshots without a per-tab native
poller or a background-alert opt-in requirement.

One dirty coordinator sequences fleet, conversation-list and attention GETs and
coalesces notices arriving during a fetch. Healthy live metadata still gets a
30-second full resync; unavailable/erroring SSE falls back to six-second refreshes
and six-second reconnect attempts. Browsers without EventSource and older servers
remain readable by polling, without a claimed live connection. Metadata leases
and coverage, not transport health, determine available chat/attention actions.
See [events and freshness](../architecture/COVEN_WEB_EVENTS.md) for wire limits.

These updates never automatically read messages, terminal output or decision
details, obtain mutation tokens or POST. Card/request opens and Read/Refresh
gestures remain deliberate; the existing verified notification deep-link navigation
exception is unchanged. No additional toolbar or routine explanatory UI is added.

### Native conversations (new builds only)

The single **Sessions** grid contains only fleet observations. Authorized native
chats never add a third/fallback root card or hide a fleet agent. It is not a claim
of execution readiness or a unique-agent count. Verified cards open the pinned
bound conversation immediately on the first primary click. Unlinked, ambiguous,
stale or unsupported links say **Chat unavailable**, with a concise reason.
Unavailable primary buttons retain keyboard focus with guarded `aria-disabled`;
clicking them does not open a dialog, read messages or submit anything. There is
no picker, manual association, global Shared chats chooser or native fallback card.
Every root keeps a separate Details / terminal output action. Fleet titles,
providers and projects remain intact, never replaced by a source/chat label. Existing
configurations remain valid and default to `"conversations": []` and
`"conversation_sources": []`. Keep `allow_best_effort_output` false for this slice.

Conversation list rows may include `association_status` (`verified`, `unavailable`,
`ambiguous`) and `card: null | {fleet_agent_id, fleet_endpoint_id, binding_id}`.
Agent and binding references are lowercase 64-hex IDs; endpoint IDs are opaque
configured tokens. Only a verified, exact fleet-agent plus endpoint match that is
one-to-one in the authorized visible metadata automatically links a fleet card.
Competing claims never auto-select. Labels, hostnames, providers,
order and single-candidate guesses never establish identity. Missing fields on old
or static rows mean unavailable association. Native sharing can still exist at
the source, but BASIC monitoring cannot honestly open it from an unrelated card.
Both fleet observations and authorized conversation metadata must be fresh.

Merged-card reads use exactly
`GET /api/v1/agents/<fleet_agent_id>/conversations/<id>/bindings/<binding_id>/messages`.
The monitoring UI never falls back to the ordinary native-only read API. It validates
the response against the pinned conversation ID and team. Binding changes fence
pending reads and clear transcript/draft content, including A-to-B-to-A changes.
Prompt POSTs always retain the exact native conversation handle, independently of
display association; unresolved delivery notices survive metadata/auth fences.

There are no page-local association preferences. Metadata expiry/failure, auth
loss, hiding, stale fleet observations, root removal and ID/team replacement clear
affected views and fence pending reads/tokens/posts. Exact binding changes clear
transcript content and drafts without automatically fetching the replacement.
A different conversation closes the old view, never retargeting it. A stale
server-side binding can refuse a read; it never selects the source's latest chat.
Details explains the connection status independently of terminal output capability,
not through a selection action. Native ACL, grant, CSRF, permission and
uncertain-delivery gates remain unchanged. There is no localStorage or persistence.

One global Host/Team toolbar and Refresh action refresh fleet and authorized chat
metadata independently, never transcripts or submissions. Source health and
association coverage remain diagnostic API metadata, not routine explanatory text
in the normal view. Zero fleet observations mean zero root
cards, even when native sharing metadata exists. Host options are configured fleet
endpoint IDs only, with no Unlinked native bucket. Team filters include only verified
chat team metadata and explain unassigned terminal observations. Empty native
metadata, unavailable metadata and absent fleet configuration remain distinct.

Unchanged opaque root nodes preserve keyboard focus. Root-origin views track their
original verified root through its focus reference and close/fence if that mapping
changes; they never follow a replacement chat. Modal focus stays inside the dialog;
closing returns to the original visible root primary (even after it becomes
unavailable) or its Details action. Removed/filtered roots fall back
to Host. Actual API errors remain visible. An unavailable/unsupported host without
any retained card has a concise unhealthy-only warning; healthy host chips and
duplicate coverage notices are not displayed.

The header's single alert toggle replaces the old Private badge and separate
enable/disable toolbar. It reflects passive subscription inspection with on/off/
unknown and busy accessibility state. Healthy states have no helper paragraph.
Real permission, delivery or cleanup problems remain actionable; server-cleanup
uncertainty has a separate exception-only action and is never represented as success.

Chat opens at its latest content/reply area using one outer scroll container.
An explicitly opened pending request takes priority at its heading, with clearance
measured below the rendered sticky header rather than a fixed title height. User
scrolling, touch, keyboard input and form interaction cancel pending follow intents;
metadata refreshes never pull a history reader back down. Initial/explicit read
positioning rechecks view/request/interaction generations and current authorization,
so delayed data cannot move another or closed view. Refresh while reading history
restores position rather than automatically choosing latest. No keyboard is opened
programmatically, and no approval control is automatically focused.

Older running endpoints without provenance correctly show **Chat unavailable**.
Fixture success does not establish live one-click chat readiness. A deliberate relaunch with the new
native UI may be needed before verified links become available. The browser does
not restart anything. Deployment/build and relaunch remain separate operator steps.
This frontend policy follows the current BASIC contract at the top of
`CARD_CONVERSATION_NAVIGATION.md`, superseding earlier picker/fallback designs. The
backend's optional verified metadata and bound route remain unchanged; no alternate
association endpoint or inferred linkage is used.

### Shared Follow-Ups

Receipts use `coven.conversation-submit.v1`, opaque `id`, `request_id`,
`outcome`/`delivery`, fixed diagnostics and optional byte count/digest, never prompt
text or private native identifiers. Accepted means persisted/scheduled, **not
completed**. Missing/invalid HTTP receipts after sending mean uncertainty, not
rejection. Closing, hiding or losing authorization clears drafts/transcripts and
fences late callbacks; it does not cancel or recall native work. A non-content
attempt notice remains. After uncertainty, the composer requires an explicit
message refresh and a separate checked decision before a new prompt, with a new
UUID. There is no Retry action. Drafts are memory-only, not persisted or logged.
Unresolved receipts retain only originating opaque ID, team, request UUID and
attempt-state flags, never prompt or transcript text. Only a successful explicit
read of that exact ID/team, begun after uncertainty, counts as inspection. Reading
another target or a replacement reference does not inspect the original attempt.
Independent targets remain writable; their accepted/unsent receipts or token
failures cannot clear another attempt's uncertainty. A new accepted prompt on the
same target is also not proof of what happened to an earlier uncertain prompt.
The page retains at most 32 unresolved receipts, including inspected attempts;
at capacity it refuses new sends explicitly rather than silently evicting unknown
outcomes. A sanitized global count/uncertainty notice survives navigation, auth
loss, hiding and reference expiry, without exposing old target labels or text.
These safeguards are in-memory for this page only, not durable across reloads or
tabs. Reloading does not establish delivery or make a retry safe; inspect native
history at the source when an original reference is no longer available.
Refresh messages never sends a prompt. Native requests use the separate attention
flow below. Terminal input, full history and remote native adapters remain excluded.

### Actionable Attention

Questions render complete inert text and native single/multiple/custom controls.
Allow once remembers no rule. Deny discloses that it rejects all currently pending
permissions in that session, not future arrivals. A response is separate from a
follow-up. Applied means **native wait settled, not tool completed**. Lost/invalid
receipts are uncertain, never automatic retries. The page retains at most 32
answer-free occurrence/attempt records, does not evict uncertainty, and requires
fresh explicit detail inspection plus a separate deliberate choice for a new
attempt. A different request cannot clear earlier uncertainty. These records are
in-memory, not durable proof across page reloads. Stale grants, ACLs, bindings and
occurrences clear details/drafts and fence late callbacks without retargeting.

### Opt-In Background Alerts

Enable alerts is the only permission/subscription gesture. It registers the embedded
no-store `/service-worker.js` at `/`, with CSP `worker-src 'self'`. No fetch handler,
offline cache or sensitive localStorage is added. Browser permission, service-worker
registration and subscription state are narrow persistence exceptions, not content
storage. Existing subscription ownership is checked; explicit re-enable may remove
an unowned browser endpoint and create a fresh one, never transfer it silently.
Disable alerts removes an owned server subscription and the browser subscription.
Unsupported/denied APIs retain working in-page badges with honest background status.

Local browser unsubscription is attempted before, and independently of, server
config/token/removal success. If it succeeds but server cleanup is unverified, the
page says so and retains only opaque subscription IDs for another deliberate cleanup
click. A failed local removal never claims browser alerts are disabled. Browser setup
promises and config/CSRF/POST/body waits share a 10-second operation deadline, which
releases controls without retrying mutations. Timed-out subscription creation remains
unverified because the server may have committed it. Optional closed `delivery_status`
codes describe delivery separately from subscription-enabled state; arbitrary errors
are never displayed. Optional attention `streams` metadata does not replace exact
public item/verified-root matching and is not rejected by this frontend.

Unresolved creation attempts and ordinary removal candidates are separate bounded,
opaque-ID-only records. Creation uncertainty starts immediately before the subscription
POST, not during CSRF acquisition. A config sample without that ID, a later successful
delete, or a different successful create cannot prove an older blocking create will
not commit afterward. With no ordered cleanup proof in this API, the page retains
that creation notice even after local opt-out and explicit server deletion. Further
cleanup requires another deliberate click; polling never retries it. These records
are memory-only, and reloading is not proof of server cleanup. No endpoint/key blob
or private content is retained in them.

The bounded HTTPS endpoint hash is saved before waiting on local unsubscription, so
a timeout followed by late browser success does not lose the server cleanup ID.
An identity that cannot be established remains unverified rather than becoming a
false fully-disabled claim. Passive inspections capture the push-operation revision;
starting a user operation fences their late success and failure updates.

The server collector works without an open browser page. Encrypted worker payloads
are only `{version:1,alert_id:64hex}`; title/body are constant generic text, without
question/permission/provider content or approval actions. Notification clicks focus
or open only the same origin `/#attention=<alert-id>` and never execute an action.
Fresh bounded metadata must match the stable alert to the exact current occurrence
and verified root before detail opens. Missing/incomplete/revoked references mean
cannot currently verify, never resolved or select the latest request.
Later fresh metadata may verify that same stable alert under a new grant-bound
decision reference. A failed lookup does not consume navigation; after successful
verified opening, dismissal does not reopen the alert on every poll.

HTTPS, browser/platform permissions (including installed-home-screen requirements on
some mobile platforms), native share lifetime, host availability and vendor delivery
all apply. Delivery is advisory. Node worker-VM tests exercise closed-tab handlers;
installed Firefox fixtures exercise real first-card forms without browser permission
prompts or vendor push. Neither is actual device delivery or live native acceptance.

## Edge Cases

Opening a registered target reads once. Refresh is explicit. No transcript
polling, prefetch, localStorage or HTML/Markdown/ANSI interpretation is provided.
Static registrations remain read-only. Text is cleared immediately on target, binding,
host/team or write-permission changes, close, new reads and refusals;
request-generation checks reject late results,
including close/reopen of the same target. Partial/older/omitted/truncation labels
are separate from availability. An empty authorized list explicitly requires
operator native setup rather than showing fake availability.

Entries containing only whitespace, control characters or default-ignorable Unicode
characters are not rendered as empty message cards. Material omissions retain a
compact notice; an all-empty sample has a truthful text-projection empty
state. This filtering is presentation-only: displayed text is preserved exactly,
the validated native projection/digest is unchanged, and no extra private content
is fetched. Native history includes non-text/tool steps, and bounded reads can omit
unsupported or oversized content; a hidden entry is not proof that the assistant
produced no content. Existing partial/truncation/omission warnings remain visible.

The **metadata-only** authorized list is revalidated through SSE invalidations,
periodic/fallback resync and visibility resume. Matching
references preserve the open view without fetching messages. Removed/changed
references clear the view and invalidate pending reads. Metadata has at most a
15-second server lease, shortened by native grant/card/coverage evidence. The
browser subtracts request transit time and never extends the same generation's
deadline on repeat receipt. It is checked on visible one-second UI ticks and
before accepting reads; expiry
or a failed metadata refresh clears authorized metadata and text. Hidden pages
close SSE and clear conversation/request state; returning requires metadata
revalidation and an explicit reopen before another transcript read. Persisted
`pagehide` also clears output and suspends timers. BFcache `pageshow` restarts the
one-second expiry guard and fully resynchronizes metadata, never old content.
Superseded stream callbacks and GET responses cannot restore a paused view.
Suspended/throttled event loops
can delay the next UI check; this is not a hard real-time revocation guarantee.

A known authentication failure on any API route aborts reads, invalidates pending
callbacks (including deferred response bodies), and clears transcripts, authorized
targets, team choices and fleet metadata. Responses begun before that failure
cannot restore them. Resource/configuration changes are acted on when the next
metadata response observes them, not before. These client-side safeguards cannot
recall already-delivered or copied data and do not replace server-side ACL checks
on every read.

## Example usage

Open the private site, filter Host/Team if needed, and click a verified card once
to read its pinned conversation. Use explicit message Refresh for another read;
use Details separately for qualified terminal observations. A follow-up, decision
response, or alert opt-in is always a separate deliberate action.

Frontend coverage includes pure projection and real bootstrap-handler tests. The
opt-in `clients/web/browser.test.mjs` uses an installed Firefox and synthetic local
HTTP fixtures for actual DOM, phone/desktop overflow, unavailable-state guards, one-click exact bound
routes, blank filtering and modal focus. In a unique disk-backed `TMPDIR` with
exact-directory cleanup, run
`COVEN_WEB_FIREFOX=/usr/bin/firefox node --test clients/web/*.test.mjs`.
Without that variable the DOM browser test is explicitly skipped; the separate
native EventSource acceptance test additionally requires `COVEN_WEB_BINARY`.
See the [client test instructions](../../clients/web/README.md#offline-checks).
No dependency download is performed. The fixture reaps Firefox before deleting its owned profile
and produces no screenshots, snapshots or live chat traffic. Fake-EventSource
lifecycle/BFcache tests are not real Firefox EventSource end-to-end acceptance;
the separate opt-in test is not claimed as passed here.

## Related files

- [Guide index](index.md), [API contract](../architecture/COVEN_WEB_CONTRACT.md),
  [deployment runbook](../operations/COVEN_WEB_DEPLOYMENT.md), [history](../changelog/index.md).
- [Client source and acceptance checklist](../../clients/web/README.md),
  [projection and bootstrap](../../clients/web/app.js), [observer](../../src/observer/mod.rs).
