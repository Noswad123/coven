# Coven Web public contract

## Overview

Coven serves browser assets and neutral observation APIs behind private Tailscale
Serve. Fleet observations do not grant native authority. Browser workflows are in
the [guide](../guides/COVEN_WEB.md); operator configuration is in the
[runbook](../operations/COVEN_WEB_DEPLOYMENT.md).

## API / Interfaces

### Public JSON contract

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
      "pane_name": "Research",
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

`pane_name` is optional: absent/null supports older remote helpers. A supplied
string is at most 160 UTF-8 bytes, without Unicode controls, U+061C, U+200E/F,
U+2028/29, U+202A-E or U+2066-9. It comes only from the matched Kitsune pane's
manual `label`, never a tab/workspace name, CWD, title or provider guess. Missing
or empty names display **Unnamed pane**. Names are inert text, including literal
`OC | ` prefixes and HTML-like characters; they carry no identity or authority.

Each card body has three ordered lines: manual pane name, plain provider, and
the existing session title. Only that third title removes the known leading
legacy `OC | ` decoration for Djinn. Long values wrap without changing backing
data. Provider display uses the reported provider, otherwise a single process
family hint, **Multiple providers** for ambiguity, or **Unknown provider**.
This display-only fallback does not verify identity or enable any capability.
Evidence, family disclaimers, project/team and coverage diagnostics stay in
Details, alongside chat/output status. Card, chat and Details headings use the
same pane name. Renames and duplicate names never change opaque keys, bindings,
read routes, availability, or trigger transcript reads/submissions. Native session
metadata titles are unchanged. Upgrade readers before remote helpers; release and
deployment remain separate operator steps.

Session-title candidates are forwarded completely through 4096 UTF-8 bytes,
without the former 512-byte shortening or added ellipsis. Existing control removal
and outer-whitespace cleanup remain; literal `...` in a supplied title is retained.
Unsupported values within the wire profile have an explicit unavailable placeholder,
not a sliced value claiming to be complete. Other display-field limits and the
global 4096-character wire guard/response budgets are unchanged.

The web title line has no ellipsis, line clamp or clipped-height rule. A title
already shortened by a running native application cannot be reconstructed by the
gateway. Updated Djinn source removes its 40-character application cutoff and emits
the session title directly; existing running instances require a deliberate updated
UI relaunch. OpenTUI's current native title path also has an approximately 1019-byte
transport ceiling, so this is not a claim of end-to-end 4 KiB Djinn OSC fidelity.
Ordinary titles within that transport limit are not deliberately shortened.

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

### Native conversation API

Association metadata and exact bound-read behavior are specified in the
[browser guide](../guides/COVEN_WEB.md#native-conversations-new-builds-only).
Monitoring must use the pinned fleet-agent/conversation/binding route, never fall
back to the native-only route below.

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

Receipt shapes and uncertainty behavior are in the
[guide](../guides/COVEN_WEB.md#shared-follow-ups). Accepted is not completed;
lost/invalid receipts are uncertain and never authorize an automatic retry.

### Actionable Attention

The authoritative details and native/public shapes are in
[ACTIONABLE_ATTENTION_CONTRACT.md](ACTIONABLE_ATTENTION_CONTRACT.md), including its
shared clarifications. This is additive to BASIC one-click cards, not a chooser,
third card or transcript-derived approval system.

- `GET /api/v1/attention` returns `coven.attention.v1` metadata with `state`, opaque
  exact-occurrence items, nullable omissions and unavailable/unlinked counts.
  Only visible pages poll metadata; no background transcript/detail reads.
- `GET /api/v1/decisions/<64hex>` explicitly reads complete sanitized details
  inside the existing conversation dialog. Raw native IDs, paths and tool linkage
  never cross this boundary. Unsupported/oversized detail requires the source UI.
- `POST /api/v1/decisions/<64hex>/responses` accepts only
  `{contract:"coven.decision-response.v1",attempt_id:UUID,execute:true,action:ACTION}`.
  Actions are `question_answer` with ordered `string[][]` answers,
  `question_reject`, `permission_once`, or `permission_deny` with
  `scope:"session_pending"`. Never generic yes, remembered/always or remote policy edits.
- Decision source ACL `decision_logins` is separate from follow-up `submit_logins`
  and native model/configuration `can_submit`. Readers may inspect without write
  authority. Native read_write plus explicit startup `decision_control` and exact
  current grant/verified binding/occurrence are required to respond.
- Mutation checks reuse Host/login/Origin/Fetch Metadata/CSRF/framing gates before
  polling the body. Decision body cap is 64 KiB and two seconds; answers are bounded
  to 8192 UTF-8 bytes total and 4096 per answer. Strict duplicate/unknown fields,
  invalid UTF-8 and unsupported union variants are refused. Blocking jobs retain
  permits through cancellation; the core owns transport gates and native fencing.

### Opt-In Background Alerts

Optional gateway `push:{state_directory:"ABS"}` enables the separate server-owned
collector; absence creates no push state, sender or collector. It never renews native
sharing grants. Source reader eligibility permits generic alerts, not native action.
No deployment/configuration or browser permission change is performed by the UI
until the user's explicit gesture.

- `GET /api/v1/push/config` returns `coven.web-push.v1`, `enabled`, public VAPID key
  and principal-owned opaque subscription IDs. Disabled means false/null/empty.
- `POST /api/v1/push/subscriptions` accepts closed
  `{contract:"coven.web-push.v1",execute:true,subscription:{endpoint,keys:{p256dh,auth}}}`.
- `POST /api/v1/push/unsubscribe` accepts closed
  `{contract:"coven.web-push.v1",execute:true,subscription_id:64hex}`.
- Both mutations use fresh CSRF, existing mutation origin/identity/framing checks,
  an 8192-byte/two-second body bound and bounded blocking disk jobs. Endpoint/key
  blobs come only from browser PushManager, never native selectors. Subscription
  ID is SHA256(endpoint UTF-8); the backend still enforces owner ACL independently.

## Data Flow

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

### HTTP lifecycle

The HTTP facade deliberately uses Hyper/Tokio rather than a handwritten parser.
No TLS is handled in Coven: Serve owns it. Rejected bodies are never read or
aggregated, keep-alive is disabled, header reads expire after five seconds, and
connections expire after twenty seconds overall. At most 64 connections and two output jobs run
concurrently. An abandoned HTTP response does not release its blocking output-job
permit early. Cached fleet/static requests do not wait for upstream network reads.
Inventory has priority over repeated output requests. Initial request limits are
64 headers/8 KiB accepted header text, a 32 KiB parser buffer, 2 KiB URLs and
2 MiB serialized JSON responses. The UI is dependency-free static ES modules;
development tests use Node 20 or newer, not a Node production service.
The connection deadline includes header admission and applies to reads and mutations
(`src/observer/http.rs`, `connection`). The original ten-second initial limit is
preserved as historical evidence in the
[prototype record](../changelog/coven-web-initial-prototype.md#initial-development-check-results).

## Edge Cases

### Authentication and deployment boundary

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

The [guide's edge cases](../guides/COVEN_WEB.md#edge-cases) specify visibility,
metadata expiry, callback fencing and clearing behavior; already-delivered/copied
data cannot be recalled. Static registration ACLs and socket fencing are specified
in the [runbook](../operations/COVEN_WEB_DEPLOYMENT.md#native-conversation-registration).

## Example usage

The fleet, output, conversation and prompt JSON above use synthetic values, not
usable live references. All calls require the same authenticated origin; browser
code cannot choose a native socket, session or command. See the
[configuration example](../operations/COVEN_WEB_DEPLOYMENT.md#configuration-and-cli)
for operator-owned targets.

## Related files

- [Architecture index](index.md), [guide](../guides/COVEN_WEB.md),
  [operations](../operations/index.md), [dated state](../operations/COVEN_WEB_DEPLOYMENT_STATE.md).
- [Observer/cache](../../src/observer/mod.rs), [DTOs](../../src/observer/model.rs),
  [configuration types](../../src/observer/config.rs), [HTTP facade](../../src/observer/http.rs),
  [client](../../clients/web/app.js), [CLI caller](../../src/main.rs).
