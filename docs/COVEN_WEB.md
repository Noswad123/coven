# Coven Web: Coven's browser client

Coven Web is Coven's phone-first browser client, not a separate product, agent
manager or agent runner. Source lives in `clients/web/`; Coven owns the neutral
observation API, authentication,
configured endpoints, deadlines and attribution. Co-host static assets and API
behind Tailscale **Serve**, never Funnel. Existing agent processes and the installed
Coven executable are not replaced as a prerequisite.

## First slice and protected behavior

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

## Public JSON contract

All routes are authenticated, same-origin and `Cache-Control: no-store`.

`GET /api/v1/fleet`:

```json
{
  "contract": "coven.fleet-observer.v1",
  "generated_at_ms": 1900000000000,
  "refresh_interval_ms": 3000,
  "stale_after_ms": 15000,
  "endpoints": [{
    "id": "example-host",
    "label": "Example desktop",
    "connection": "reachable",
    "last_attempt_at_ms": 1900000000000,
    "observed_at_ms": 1900000000000,
    "error": null,
    "discovery_status": "observed",
    "unclassified_panes": 0,
    "agents": [{
      "id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "title": "Example project agent",
      "provider": "djinn",
      "project": "example-project",
      "activity": "working",
      "evidence": "kitsune_snapshot",
      "provider_candidates": ["djinn"],
      "observed_at_ms": 1900000000000,
      "output_available": true
    }]
  }]
}
```

Connection is `reachable`, `unavailable` or `unsupported`. Error is null or a fixed
safe code, never upstream text or a command/path. Activity is `working`, `idle`,
`blocked` or `unknown`. The endpoint ID is configured server-side; agent ID is a
gateway-issued lowercase 64-hex opaque reference that includes endpoint and
connection/generation scope. Never infer job success from source `done`.

Evidence is `kitsune_snapshot` or `coven_process_discovery`. Process evidence
provides only bounded provider-family hints, not native identity or activity:
those cards must have activity `unknown` and output disabled. Multiple candidate
families remain one terminal card with explicit ambiguity. Kitsune metadata wins
for the same terminal/four-locator tuple; titles are display data, never recognition
rules. Card identity remains terminal-scoped when evidence changes.

`discovery_status` is `observed`, `unsupported` or `unavailable` independently of
transport health. `unclassified_panes` is a count of unclassified terminal panes,
not an agent count; null means unavailable. Even a completed process scan cannot
establish complete acquisition or prove no other agents exist. Unsupported
capabilities preserve supported metadata with an incomplete-coverage warning.
Malformed descriptors, process failures, mapping drift and deadlines fail refresh
and preserve stale last-known observations instead of publishing fresh emptiness.

`GET /api/v1/agents/<id>/output`:

```json
{
  "contract": "coven.fleet-observer.v1",
  "agent_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "observed_at_ms": 1900000000000,
  "text": "Synthetic example output.\n",
  "truncated": false,
  "attribution": "matching_snapshot_brackets_best_effort"
}
```

Errors use `{"error":{"code":"fixed_safe_code"}}`; unknown/invalid references
are not resolved into paths or arbitrary pane selectors. Output requires a fresh
current reference and snapshot/read/snapshot checks for the expected terminal.
The source lacks an atomic expected-terminal precondition or reliable output
revision. Matching brackets do **not** eliminate ABA races or attest freshness.
The UI must label the best-effort attribution. Production configuration defaults
`allow_best_effort_output` to false; enabling it is an explicit operator choice.

Initial limits: eight endpoints, 128 cards per endpoint, 64 KiB output, bounded
inventory/helper replies and five-second upstream operations, one upstream
operation per endpoint. A slow endpoint cannot stall the cached fleet response.
Poll inventory every three seconds with bounded backoff; mark observations stale
at fifteen seconds or immediately when the last refresh failed. Do not renew
evidence leases by forwarding or caching it. Output is requested only on demand.

Inventory reuses Coven's acquisition/classifier with one absolute five-second
deadline and shared 4 MiB budget across snapshot/process/snapshot reads. The
observer's explicit scan cap is 1,024 panes; the standalone discovery CLI retains
its existing 64-pane cap. Neither classifier rules nor live detection manifests
are changed by this adapter. The method set remains `session.snapshot`,
`pane.process_info` and, for eligible metadata-backed output only, `pane.read`.
Process-only cards reject forced output with `output_unsupported` before transport.

## Configuration and CLI

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
metadata independently, never transcripts or submissions. Shared source health and
sanitized counts such as `1 shared session awaiting association` remain diagnostic
metadata only, with no selection action. Zero fleet observations mean zero root
cards, even when native sharing metadata exists. Host options are configured fleet
endpoint IDs only, with no Unlinked native bucket. Team filters include only verified
chat team metadata and explain unassigned terminal observations. Empty native
metadata, unavailable metadata and absent fleet configuration remain distinct.

Unchanged opaque root nodes preserve keyboard focus. Root-origin views track their
original verified root through its focus reference and close/fence if that mapping
changes; they never follow a replacement chat. Modal focus stays inside the dialog;
closing returns to the original visible root primary (even after it becomes
unavailable) or its Details action. Removed/filtered roots fall back
to Host. Health, incomplete coverage and metadata errors stay separately visible.

Older running endpoints without provenance correctly show **Chat unavailable**.
The currently running old native `f92` lacks that provenance; fixture success does
not establish live one-click chat readiness. A deliberate relaunch with the new
native UI may be needed before verified links become available. The browser does
not restart anything. Deployment/build and relaunch remain separate operator steps.
This frontend policy follows the current BASIC contract at the top of
`CARD_CONVERSATION_NAVIGATION.md`, superseding earlier picker/fallback designs. The
backend's optional verified metadata and bound route remain unchanged; no alternate
association endpoint or inferred linkage is used.

Frontend coverage includes pure projection and real bootstrap-handler tests. The
opt-in `clients/web/browser.test.mjs` uses an installed Firefox and synthetic local
HTTP fixtures for actual DOM, phone/desktop overflow, unavailable-state guards, one-click exact bound
routes, blank filtering and modal focus. In a unique disk-backed `TMPDIR` with
exact-directory cleanup, run
`COVEN_WEB_FIREFOX=/usr/bin/firefox node --test clients/web/*.test.mjs`.
Without that variable only the browser test is explicitly skipped; no dependency
download is performed. The fixture reaps Firefox before deleting its owned profile
and produces no screenshots, snapshots or live chat traffic.

Add explicit operator registrations to the configuration, for example these
synthetic values (not a usable live runtime or session):

```json
"conversations": [{
  "id": "research-example",
  "label": "Example research session",
  "team": "research",
  "allowed_logins": ["owner@example.invalid"],
  "native_socket": "/home/example/.local/run/djinn/native.sock",
  "expected_runtime": "11111111-2222-4333-8444-555555555555",
  "session": "ses_example"
}]
```

The closed entry schema requires all seven fields. At most 64 entries are allowed.
IDs are unique 1-128 byte ASCII tokens (`A-Z`, `a-z`, digits, `_`, `-`, `.`);
team uses the same grammar. Labels are nonempty, at most 256 UTF-8 bytes with no
control characters. Each entry's nonempty, unique `allowed_logins` list (maximum
64) is the ACL for that specific `(team, registered resource)` and must be a
subset of the global outer allowlist. A shared team label does not grant access
to another resource or imply inherited team membership. Both list and read
filter/check this ACL against the exact Serve principal, before native I/O.

The socket must be an absolute local Unix path of at most 100 UTF-8 bytes, with
no control characters or `.`/`..` components. Remote native conversation targets
are **unsupported**: there is no SSH field, URI, proxy or fallback for them. Fleet
SSH configuration is independent. Runtime is a lowercase UUID and session is a
`ses_` identifier with a 1-128 character ASCII alphanumeric/underscore/hyphen suffix.
The runtime never automatically follows a restart. Operator action and a newly
started gateway are required to change registrations; there is no config editor
or reload API in the browser.

An operator must separately start a new Djinn build with its private native
endpoint explicitly enabled, obtain its runtime and intended session identifiers,
and register those exact values. Coven does not start/attach/discover that runtime.
The endpoint must be a same-user `0600` Unix socket in an immediate same-user
`0700` directory. Socket/parent device+inode checks fence both calls. These checks
are **not authentication** against malicious local/same-user processes or races.
Loopback Serve-header forgery by local processes remains the host trust limitation
described below. Do not deploy on an untrusted multi-user host.

Validate with the **newly built**, separately scoped executable using
`/path/to/new/coven observer check --config /path/to/observer.json`. Starting
`/path/to/new/coven observer serve --config /path/to/observer.json` is an explicit
operator startup step, not something this implementation activates. Do not replace
running binaries or restart existing services merely to validate configuration.
The installed older executable does not acquire this behavior without a new build.

`GET /api/v1/conversations` returns only authorized display metadata:

```json
{"contract":"coven.conversations.v1","conversations":[{
  "id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "label":"Example research session","team":"research","host":"Local native"
}]}
```

The reference hashes the full validated configuration generation, explicit target,
team, runtime and session with a fresh random per-process gateway generation.
Configuration IDs, native paths, runtime/session/message IDs are not returned.
References change on gateway restart or any configuration change and are not bearer
authorization: another globally allowed principal cannot use one without its ACL.
No transcript response registry or cache exists to reuse across principals.

`GET /api/v1/conversations/<opaque-id>/messages` returns:

```json
{
  "contract":"coven.conversations.v1",
  "id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "team":"research","source":"djinn.conversation-read.v1",
  "messages":[{"role":"assistant","text":"Synthetic plain text.","truncated":false}],
  "has_older":true,"truncated":false,"omitted_parts":true,
  "projection_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
}
```

The example digest is illustrative, not a valid native projection digest. Unknown
or unauthorized references both return 404 before endpoint selection/connection;
invalid reference grammar returns 400, and outer identity failures return 403.
Bounded native refusals/unavailability return 503 with fixed safe codes only.
Native error messages and transcript data are never logged, cached or persisted by
the gateway. All existing API origin, CSP, no-store and concurrency protections
apply; conversation reads share the two blocking-read slots with terminal output.

The native sequence is `capabilities` then `conversation`, on the same fenced
endpoint, sharing one five-second deadline and 65,536-byte total reply budget.
Both frames use `{version:1,id:UUID,method,params}`. Conversation params are exactly
`{expected_runtime_id:UUID,session_id:"ses_..."}`. Capability
`conversation_read` must exactly advertise contract `djinn.conversation-read.v1`,
20 messages, 8,192 text bytes and 65,536 response bytes. The strict dedicated
decoder rejects duplicate keys, nesting over 32, malformed schemas, mismatched
runtime/session and invalid projection digests, while allowing a single 8 KiB
text leaf. Local filesystem endpoint checks are not interruptible by the deadline.
There is no pagination or retry. Native result/message fields follow
that contract exactly; only user/assistant text is projected into the browser.
SHA-256 is verified over UTF-8 JSON tuple arrays `[id,role,text,truncated]`, matching
JavaScript `JSON.stringify`; it describes only the returned bounded projection,
not complete history or authenticity. Native message IDs are removed after digest
verification, so the browser does not recompute this native digest.

Opening a registered target reads once. Refresh is explicit. No transcript
polling, prefetch, localStorage or HTML/Markdown/ANSI interpretation is provided.
Static registrations remain read-only. Text is cleared immediately on target, binding,
host/team or write-permission changes, close, new reads and refusals;
request-generation checks reject late results,
including close/reopen of the same target. Partial/older/omitted/truncation labels
are separate from availability. An empty authorized list explicitly requires
operator native setup rather than showing fake availability.

Entries containing only whitespace, control characters or default-ignorable Unicode
characters are not rendered as empty message cards. The status reports how many
entries have no displayable text; an all-empty sample has an explanatory empty
state. This filtering is presentation-only: displayed text is preserved exactly,
the validated native projection/digest is unchanged, and no extra private content
is fetched. Native history includes non-text/tool steps, and bounded reads can omit
unsupported or oversized content; a hidden entry is not proof that the assistant
produced no content. Existing partial/truncation/omission warnings remain visible.

The **metadata-only** authorized list is revalidated alongside visible
fleet refreshes (normally every three seconds) and on visibility resume. Matching
references preserve the open view without fetching messages. Removed/changed
references clear the view and invalidate pending reads. Metadata has a 15-second
lease, checked on visible one-second UI ticks and before accepting reads; expiry
or a failed metadata refresh clears authorized metadata and text. Hidden pages
clear conversation state, and returning requires metadata revalidation and an
explicit reopen before another transcript read. Suspended/throttled event loops
can delay the next UI check; this is not a hard real-time revocation guarantee.

A known authentication failure on any API route aborts reads, invalidates pending
callbacks (including deferred response bodies), and clears transcripts, authorized
targets, team choices and fleet metadata. Responses begun before that failure
cannot restore them. Resource/configuration changes are acted on when the next
metadata response observes them, not before. These client-side safeguards cannot
recall already-delivered or copied data and do not replace server-side ACL checks
on every read.

### Shared Follow-Ups

New shared-source rows may advertise `can_submit: true` and
`max_prompt_bytes: 65536`. Missing write metadata means read-only. Source reader
and writer ACLs, current native selected-session grants and the configured control
workspace determine this permission; fleet activity and blocked hints do not.
See [the implementation contract](WEB_SHARING_IMPLEMENTATION.md) for source setup
and native selection/lease requirements. No browser socket, SID, workspace, model
or command selectors are accepted.

`GET /api/v1/csrf` returns `{"contract":"coven.csrf.v1","token":"..."}`.
The unpredictable HMAC-authenticated token is bound to the principal and gateway
epoch and expires in 60 seconds. All responses are no-store. The browser obtains
a fresh token immediately before each deliberate submission and never retries a
mutation automatically.

`POST /api/v1/conversations/<opaque-id>/prompts` accepts only:

```json
{"contract":"coven.conversation-submit.v1","request_id":"11111111-2222-4333-8444-555555555555","execute":true,"text":"Exact follow-up text."}
```

The UUID must be canonical lowercase. The request requires an exact same Origin,
trusted Host/login, same-origin Fetch Metadata when supplied, `X-Coven-CSRF`, JSON
Content-Type, exactly one Content-Length, and no transfer/content encoding.
Authorization, target write permission, token and framing are checked before body
polling; rejected bodies are never drained. Body reads have a two-second deadline
and 400 KiB encoded cap. Strict UTF-8 JSON rejects duplicate/unknown fields. Text
is nonblank, at most 65,536 UTF-8 bytes and permits no controls except tab/CR/LF.
Connections have five-second header and 20-second overall bounds. Two mutation
jobs retain their permits until blocking work finishes, even if HTTP is canceled;
native runtime/session single-flight and attempt suppression remain core-owned.
Discovery/list reads share bounded blocking capacity with other native reads,
without blocking cached fleet or asset responses.

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
Refresh messages never sends a prompt. Native permission/question answering,
terminal input, full history and remote native adapters are outside this scope.

### Optional cross-language acceptance

`tests/observer_gateway/djinn_producer.rs` contains an explicitly ignored opt-in
test, not a silent pass when the producer is missing. With installed Bun and the
Djinn fixture checkout available, run this **inside the usual unique disk-backed
`TMPDIR` and `CARGO_TARGET_DIR` scope with exact-directory cleanup**:

```sh
COVEN_DJINN_CONVERSATION_FIXTURE=/absolute/path/to/djinn-ui/packages/opencode/test/session/conversation-socket.fixture.ts \
  cargo test --offline --locked --jobs 2 --test observer_gateway \
  real_djinn_producer_through_authenticated_http -- --ignored
```

The test starts only a synthetic memory-only producer and a disposable loopback
Coven gateway. It obtains the runtime from native capabilities, explicitly
registers `ses_fixture`, exercises authenticated Host/Origin/Serve-login routes,
and verifies the exact cross-language projection digest and literal hostile control
text. It closes producer stdin, waits for EOF shutdown and endpoint removal, and
reaps both children before temporary-directory cleanup. It neither starts a real
provider/model nor changes live configuration/services. Keep the disk-backed
temporary parent short enough for the 100-byte Unix socket address bound.

`coven observer check --config FILE` validates configuration without contacting
peers. `coven observer serve --config FILE` starts the private gateway and
collectors. `coven observer probe --socket PATH` is a standalone bounded helper
for a configured remote SSH transport, not a general-purpose proxy. It accepts
one bounded JSON request on stdin: `{"operation":"inventory"}` or
`{"operation":"output","terminal_id":"expected-terminal"}`. The helper
discovers pane location from snapshots, never from caller-supplied pane IDs. It
returns a closed `{"result": ...}` or `{"error":{"code":"fixed_safe_code"}}`
envelope with exit status zero; SSH/process failures remain transport failures.

Example configuration (not live device credentials/configuration):

```json
{
  "listen": "127.0.0.1:7341",
  "public_origin": "https://example-desktop.example-tailnet.ts.net",
  "allowed_logins": ["owner@example.invalid"],
  "allow_best_effort_output": false,
  "endpoints": [
    {"id":"desktop","label":"Desktop","socket":"/home/example/.config/kitsune-dev/kitsune.sock"},
    {"id":"laptop","label":"Laptop","socket":"/home/example/.config/kitsune/kitsune.sock",
     "ssh":{"target":"100.64.0.20","helper":"/home/example/.local/libexec/coven-web/coven"}}
  ]
}
```

Configuration is closed-schema and trusted operator input. Only loopback listeners
and HTTPS public origins are accepted. SSH uses fixed argv, strict existing host
trust, batch authentication, no agent/port forwarding or multiplexed sessions;
remote helper paths and socket paths are configured, not browser parameters.
The SSH adapter disables config-file interpretation and uses the current user's
default `~/.ssh/id_ed25519` with `IdentitiesOnly=yes`; configure a directly
resolvable host/IP (not an SSH config nickname), under the same Unix username.
Deploy an immutable, separately scoped build for gateway/helper rather than
changing installed Coven or agent launchers. Inspect binary compatibility first.

## Authentication and deployment boundary

The gateway accepts only exact configured Host/Origin, same-origin Fetch Metadata
where supplied, and an exact allowed `Tailscale-User-Login` from a loopback peer.
Only Tailscale Serve should proxy to this listener. No permissive CORS or generic
cross-origin API is provided. All other HTTP methods and API routes are refused.

**Trust boundary:** Serve strips spoofed identity headers from network requests.
Loopback does not authenticate Serve against other local processes. v1 explicitly
trusts the gateway host's local execution environment; local malicious processes
can forge proxy headers. Do not claim isolation from them or deploy this trust
model on an untrusted multi-user host. Serve identity authorizes the operator at
the gateway, not the full upstream socket: the helper still holds privileged local
socket access, and upstream reads may perform housekeeping.

Before live rollout: confirm allowed user and local-host trust, enable tailnet
HTTPS/Serve with user approval, authorize only intended phone HTTPS access, verify
header forwarding and no direct network listener, and decide whether the qualified
best-effort output is acceptable. No public Funnel, router forwarding or encryption
changes. Validate one existing endpoint at a time, then actual phone/cellular,
offline/reconnect and foreground/background behavior. Fixture success is not live
acceptance. Keep the previous deployment/configuration for rollback.

## HTTP lifecycle and development checks

The HTTP facade deliberately uses Hyper/Tokio rather than a handwritten parser.
No TLS is handled in Coven: Serve owns it. Rejected bodies are never read or
aggregated, keep-alive is disabled, header reads expire after five seconds, and
connections expire after ten. At most 64 connections and two output jobs run
concurrently. An abandoned HTTP response does not release its blocking output-job
permit early. Cached fleet/static requests do not wait for upstream network reads.
Inventory has priority over repeated output requests. Initial request limits are
64 headers/8 KiB accepted header text, a 32 KiB parser buffer, 2 KiB URLs and
2 MiB serialized JSON responses. The UI is dependency-free static ES modules;
development tests use Node 20 or newer, not a Node production service.

```sh
cargo test --offline --locked --lib observer::
cargo test --offline --locked --test observer_gateway --test native_package --test native_install
cargo clippy --offline --locked --all-targets -- -D warnings
cargo fmt --all -- --check
node --test clients/web/*.test.mjs
```

Use the repository `just` recipes for the broad regression gate when available.
`just` was absent in the initial development environment, so equivalent Cargo
commands were used. The full Rust run reached 1,348 passing library tests (seven
ignored), 100 passing CLI tests and multiple passing integration binaries, then
hit a ten-minute command timeout during an existing execution-inspection parity
test. This is incomplete full-suite validation, not a passing full gate. The
equivalent focused checks passed independently at that time. After the client/path
rename, 42 observer tests, four HTTP/CLI tests, nine native-install tests, two
packaging tests and 26 Node tests passed; all-target Clippy, formatting and
whitespace checks passed. This does not complete the full legacy gate.
Native-source packaging tests include
the embedded client assets. Real-DOM mobile-width smoke checks used synthetic
data in an isolated headless browser; they do not establish actual phone access.

## Private installation and operation

The Coven Web layout keeps gateway and remote helper builds in immutable,
hash-verified user-scoped directories under `~/.local/libexec/coven-web/`, separate
from public `coven` launchers. Use `coven-web.service`, with configuration at
`~/.config/coven-web/observer.json` and the same binary's bounded `observer probe`
helper on each remote host. The private deployment was migrated and verified under
these names without changing the API or Serve route. The user service has no new sudo, lingering,
login, sleep or encryption privileges.

```sh
systemctl --user status coven-web.service
journalctl --user -u coven-web.service
```

The gateway should listen **only** on `127.0.0.1:7341`. Its identity headers can be
simulated by a trusted local operator for smoke tests; that is not proof of Serve
authentication. Official Tailscale v1.104.1's TCP proxy preserves public Host and
Origin, strips spoofed identity headers, and supplies the requesting untagged
user's identity. Verify the live path before claiming acceptance.

An administrator runs, on the gateway host:

```sh
sudo tailscale serve --bg --https=443 http://127.0.0.1:7341
```

Complete the HTTPS consent flow if required. Do not use Funnel. The tailnet policy
must separately permit the intended phone to TCP/443 on the gateway; preserve
existing SSH rules. If a policy test previously denied that exact connection,
move it to the phone's accept assertions rather than deleting negative tests
wholesale. Validate the saved policy and then test from the actual phone on
cellular. Output remains operator-disabled unless its attribution limitations
are separately accepted.

Rollback for a deployment using this layout: disable/stop **only**
`coven-web.service`, restore its previous immutable
payload/config if updating, and remove only its Serve HTTPS listener when owned
exclusively by this application (`sudo tailscale serve --https=443 off`). Never
reset unrelated Serve routes or stop Kitsune/Coven agent runtimes. A user service
starts with the user's manager; without lingering it is not a promise of operation
after logout or before user login following encrypted boot. Browser assets and
observations are unavailable when the gateway host is off; HA is out of scope.
