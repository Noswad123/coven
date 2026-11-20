# Actionable attention and opt-in background alerts

Implementation agreement for local Djinn questions/permissions and generic Web Push.
The user approved locked-phone/closed-tab notifications through the browser push
service. Preserve BASIC one-click verified root cards, the user's concise copy,
native policy, existing sharing scopes and no automatic decision retries. No remote
native adapter, swarm mode, remote `always`, or transcript-derived approvals.

## Shared clarifications

- A remote claim settles the captured meaning represented by its digest, or rejects
  synchronously if native meaning differs. Freezing only a separate web wrapper is
  insufficient; never freeze caller-owned objects. Projection failure must not
  escape into legacy native ask.
- Native summary enumeration walks the owned pending map directly, at most 256
  entries per service, without existing list()-then-truncate hydration. The 32-item
  return cap is combined across question/permission. Detail lookup is direct and
  occurrence-conditional. Projection traversal is at most depth 12 / 2048 nodes;
  native request/tool IDs are nonempty, at most 512 UTF-8 bytes, with no controls.
- The canonical digest preimage is UTF-8 JSON.stringify of these tuple arrays,
  NEVER the self-referential details_sha256 field:
  `["djinn.decision-control.v1","question",request_id,occurrence_id,questions.map(q=>[q.question,q.header,q.options.map(o=>[o.label,o.description]),q.multiple,q.custom]),tool? [tool.message_id,tool.call_id]:null]`
  and `["djinn.decision-control.v1","permission",request_id,occurrence_id,permission,patterns,always_patterns,metadata_json,tool? [tool.message_id,tool.call_id]:null,"session_pending"]`.
  Normalize absent question multiple to false and custom to true; normalize absent
  tools to null. metadata_json is canonical JSON with sorted object keys and is
  treated as that exact string in the preimage. Rust verifies the same tuple digest.
- SANITIZED_DETAIL contains kind and the complete question/permission presentation
  fields only. Remove request_id, occurrence_id, details_sha256 and the entire tool
  linkage. They remain in the private reference, not browser response bodies.
- alert_id is stable SHA256 over length-delimited principal, source ID, native runtime,
  SID, kind and occurrence (no daemon/grant/binding epoch). Native IDs supply entropy;
  it is a locator, never authority. Decision id includes current grant/binding and
  daemon/policy epoch. Notification navigation uses current bounded metadata: absent
  under incomplete coverage means cannot currently verify, never proof of resolution.
- Claimed native deferreds settle without depending on successful or unbounded event
  publication. Use guaranteed settlement plus bounded/best-effort publication; a
  hanging event publisher must not strand a question/permission or a denial batch.
- Push subscription disk mutations and DNS/sending run in bounded blocking/background
  jobs, never on the async HTTP request executor. The collector runs without a browser,
  but only the native frontend may keep its own selected-session grant alive.
- Attention additionally returns bounded `streams:[{id:64HEX,complete:bool,alert_ids:[64HEX]}]`
  for each successfully queried authorized native runtime/session (max 64 streams,
  32 IDs each). Stream ID is a stable length-delimited hash of principal, source,
  runtime and SID, never raw identity. Include every observed occurrence in its
  stream even if its card/details are unavailable. A complete matching stream may
  retire a notification ledger entry absent from that stream; unknown/partial or
  absent streams must not be used as proof of retirement. This prevents lifetime
  subscription exhaustion without losing dedup across unselected/unknown contexts.
  Push may also use documented bounded retention for old records, with possible
  re-alert after retention; it must not silently stop after 64 lifetime events.
- Every new push transport attempt requires a fresh positive, authorized eligible
  attention item for that same alert ID. Revoked/resolved or unknown eligibility
  cannot authorize a send/retry. Preserve history on unknown; do not call it resolved.
  Notification read eligibility does not require decision-writer authority or
  displayable full details. Push status may add a sanitized `delivery_status` code;
  the UI must distinguish subscription-enabled from degraded/unverified delivery.

## Authority and lifecycle

- Native user configuration adds optional `decision_control: bool` (default false).
  It is captured at startup, with existing safe parent-alias/0600 checks. Read mode
  can inspect supported decisions; writes require native read_write AND this opt-in.
- Gateway source configuration adds `decision_logins: Vec<String>` (default empty),
  a subset of readers, requiring the configured control workspace. This is separate
  from follow-up submit_logins and independent of model/configuration can_submit.
- A pending native entry gets a fresh immutable occurrence UUID at registration.
  Request ID or text hash is not occurrence identity. Copy/detach/freeze supported
  decision meaning without freezing caller objects. Unsupported/oversized/cyclic/
  accessor-bearing input remains native-only; never break legacy asking just to
  make web projection possible or truncate details and still allow approval.
- Native services own the common synchronous compare-and-claim/remove operation.
  Manual replies, remote replies, cancellation and teardown contend on that map.
  No await/yield separates final entry/session/occurrence/hash/context/grant checks
  from claim. After claim, deferred settlement is interruption-safe even if event
  publication fails. Applied means the native wait was settled, not tool success.
- Permission denial preserves native `session_pending` semantics: synchronously
  capture/claim currently pending permissions in that session, not future arrivals.
  Disclose this scope. Allow once changes no remembered rules. Never expose always.
- Automatic callbacks use the already-active fixed-directory root/implicit-local
  instance; no InstanceStore.load or second worker. Provide a synchronous active-
  context fence at claim if needed. Revoke access without settling native decisions.

## Bounds and native protocol

Use additive capability only on automatic endpoints with real native callbacks:
`decision_control:{contract:"djinn.decision-control.v1",version:1,max_response_bytes:65536,actions:["question_answer","question_reject","permission_once","permission_deny"]}`.
Existing discovery, grants, sharing, association and follow-up schemas stay unchanged.

Keep the existing version-1 framed envelope and runtime pinning. All shared methods
require exact expected_runtime_id/session_id/grant_id, native grant/context fencing,
closed schemas and fixed safe errors. Metadata/detail samples need final synchronous
validity fences after the last await so cancelled/replaced occurrences cannot escape.

`shared_attention`, params exactly the three sharing selectors, returns:
`{type:"shared_attention",contract:"djinn.decision-control.v1",grant_id,session_id,requests:[META],complete:bool,omitted_count:number|null,can_respond:bool}`.
META is exactly `{kind:"question"|"permission",request_id,occurrence_id,details_sha256:string|null,details_available:bool}`.
Bounds: 32 returned requests, at most 256 scanned pending entries per service.
Unknown remaining count is null with complete:false, never a fabricated exact zero.
Unsupported details may have a metadata row but cannot be acted on remotely.

`shared_decision`, params sharing selectors plus kind/request_id/occurrence_id/
details_sha256, returns `{type:"shared_decision",contract:"djinn.decision-control.v1",grant_id,session_id,decision:DETAIL}`.
DETAIL is one complete immutable variant:

```text
Question: {kind:"question",request_id,occurrence_id,details_sha256,
 questions:[{question,header,options:[{label,description}],multiple:bool,custom:bool}],
 tool:null|{message_id,call_id}}
Permission: {kind:"permission",request_id,occurrence_id,details_sha256,
 permission,patterns:[string],always_patterns:[string],metadata_json:string,
 tool:null|{message_id,call_id},deny_scope:"session_pending"}
```

Canonical details digest covers a versioned, deterministic representation of ALL
projected fields and permission deny scope, not mutable caller objects. Each detail
is at most 32768 serialized UTF-8 bytes; bounded traversal/depth/strings BEFORE
serialization. At most 8 questions / 16 options each; question text 4096 bytes,
header/label 256, description 1024. Permission/pattern strings at most 1024 bytes,
at most 32 patterns per array, metadata_json at most 4096. Tool IDs at most 512.
Native IDs are nonempty bounded strings, not browser selectors. Oversize means
details unavailable/use native UI; no truncated actionable patterns or choices.

`shared_decide`, params sharing selectors plus kind/request_id/occurrence_id/
details_sha256/action, uses the envelope UUID as delivery-attempt ID. Closed actions:
`{type:"question_answer",answers:string[][]}`, `{type:"question_reject"}`,
`{type:"permission_once"}`, `{type:"permission_deny",scope:"session_pending"}`.
Answers follow question order: single selection exactly one answer, multiple may
select distinct labels plus at most one custom value if allowed; custom:false
rejects unknown labels. No blank/duplicate answers; 8192 total answer UTF-8 bytes,
4096 per answer, tab/CR/LF allowed but other controls refused. No coercion to yes.

Applied receipt is exactly `{type:"shared_decision_applied",contract:"djinn.decision-control.v1",grant_id,session_id,request_id,occurrence_id,action:ACTION_TYPE}`.
Closed errors add decision_unsupported, decision_unavailable, decision_stale,
invalid_decision, and decision_uncertain to existing runtime/grant/mode/capacity
errors. Only decision_uncertain is uncertain; pre-claim failures rejected. Once
claim may have begun, unexpected failure is uncertain. Reuse runtime-wide bounded
non-evicting mutating-attempt suppression across follow-ups and decision writes.

## Gateway attention and response

Add opaque principal-scoped cached decision references binding configuration/daemon
epoch, exact source/native endpoint/runtime/grant/session, occurrence/kind/hash and
verified card binding. Never accept raw native selectors from the browser. Preserve
the current discovery cap and count omissions honestly. ACLs precede native I/O.
Revalidate card/native/occurrence on detail and response, with no transcript reads.

Observer interfaces agreed for HTTP and push integration:

```text
attention(login:&str) -> Value                         // native metadata only; may block
decision_details(login:&str,id:&str) -> Result<Value> // exact full details; may block
decision_response_allowed(login:&str,id:&str) -> bool // cache/ACL only, no I/O
respond_decision(login:&str,id:&str,attempt:&str,action:&Value) -> Result<Value>
```

attention returns `{contract:"coven.attention.v1",state:"observed"|"partial"|"unavailable",items:[ITEM],omitted_count:number|null,unavailable_count:number,unlinked_count:number}`.
ITEM is `{id,alert_id,conversation_id,fleet_agent_id,fleet_endpoint_id,binding_id,team,kind,can_respond,details_available}`.
All public IDs except fleet_endpoint_id/team are opaque 64-hex strings. Only verified
card targets produce actionable items; report unlinked coverage rather than guessing.
alert_id identifies the same native runtime/session/occurrence/principal across grant
or gateway refresh; it grants no access. A stale alert never selects a newer request.
can_respond intersects native decision permission, reader/decision ACL and control
workspace; never can_submit. Details return `{contract:"coven.decision.v1",id,alert_id,kind,can_respond,decision:SANITIZED_DETAIL}` with raw native selectors/tool IDs removed.

HTTP: GET `/api/v1/attention`, GET `/api/v1/decisions/<id>`, POST
`/api/v1/decisions/<id>/responses`. Closed POST body:
`{contract:"coven.decision-response.v1",attempt_id:UUID,execute:true,action:ACTION}`.
Reuse exact Host/login/Origin/Fetch Metadata/CSRF/framing checks BEFORE body polling,
strict JSON/UTF-8/duplicates/unknown-field rejection; body 64 KiB max / 2-second read.
Keep blocking-job permits through cancellation; control-workspace shared transport
gate and runtime-launch authority refusal before native connection. Single-flight
native runtime/session and global gateway mutating-attempt suppression span both
follow-ups and decisions. No file spooling or raw answer/error logging.

Sanitized response `{contract:"coven.decision-response.v1",id,attempt_id,outcome:"applied"|"rejected"|"unavailable"|"uncertain",delivery:"applied"|"rejected"|"unsent"|"uncertain",action:ACTION_TYPE,diagnostics:[CLOSED_CODE]}`.
Lost receipt is uncertain, never retry automatically. Browser uncertainty is per
originating occurrence/attempt, cannot be cleared by another request, and keeps no
answers. A fresh explicit detail inspection and separate deliberate choice are
needed for another attempt; absence in incomplete metadata is not resolution.

## Push backend (independent of an open page)

Optional gateway `push:{state_directory:"ABS"}`. It owns a verified 0700 directory,
0600 regular single-link no-follow files, atomic bounded state replacement, a persistent
VAPID key and at most 64 subscriptions / 8 per principal. Never commit keys/endpoints.
Derive VAPID subject from the configured public HTTPS origin. Subscription ID is
SHA256(endpoint UTF-8); owner ACL still applies to every operation. Wrong-owner
endpoint reuse is refused, not silently transferred. Native details remain uncached.

`observer::push::PushService::start(config:&Config, collect:Arc<dyn Fn(&str)->Value+Send+Sync>) -> Result<Option<Arc<PushService>>>`.
Methods: `status(login)->Value`, `subscribe(login,&Value)->Result<Value>`,
`unsubscribe(login,id)->Result<Value>`, `registration_allowed(login)->bool` (no I/O).
The collector closure uses Weak<Observer> and `Observer::attention`; no Arc cycle.
Startup with push:None creates no files/threads/outbound requests. Subscribe and
unsubscribe also return before native I/O. Existing reader source policy and explicit
browser opt-in authorize generic notifications, not decision execution.

HTTP GET `/api/v1/push/config` returns `{contract:"coven.web-push.v1",enabled:bool,public_key:string|null,subscriptions:[64HEX]}` for the authenticated principal.
POST `/api/v1/push/subscriptions` body `{contract:"coven.web-push.v1",execute:true,subscription:{endpoint,keys:{p256dh,auth}}}`.
POST `/api/v1/push/unsubscribe` body `{contract:"coven.web-push.v1",execute:true,subscription_id:64HEX}`.
Apply existing mutation CSRF/origin checks, strict closed JSON, encoded body 8192 max,
2-second read and bounded jobs; reject policy/framing before polling. Subscription
responses use the same contract plus subscription_id/subscribed, never secret keys.

Use web-push 0.11 with default features disabled for standard ECE/VAPID construction,
not custom crypto. Its JWT backend is pure Rust; ECE still uses system OpenSSL.
Use a supported hardened TLS sender (e.g. reqwest blocking with rustls, defaults
disabled), no new toolchains or vendored crypto expansion. Keep rust-version 1.85
compatibility or explicitly report dependency blockers. Cargo registry fetch is
allowed for reviewed crates; dependencies reuse shared disk-backed caches.

Only standard HTTPS browser-push vendor hosts on 443: fcm.googleapis.com,
web.push.apple.com and push.services.mozilla.com / its DNS subdomains. Reject
credentials/fragments/invalid URLs, redirects, private/loopback/link-local/multicast/
CGNAT/reserved address resolutions; pin validated public DNS addresses into the TLS
client to prevent re-resolution. Bound DNS/connect/request/body/concurrency/queues.
No arbitrary outbound POST endpoint or proxy inheritance. Validate base64/key sizes
and crypto meaning. Tests use an injected sender; never contact real subscribers.

Collector runs in service-owned threads independently of browser polling, polls
only subscribed authorized principals (round-robin bounded work), collects current
eligible native metadata and never extends a grant. Bounded sender queue/workers,
TTL (e.g. 300s), dedup by subscription+alert_id, bounded retry/backoff for notification
transport only, invalid 404/410 subscription removal and clean service shutdown.
No subscriptions => no native collection or outbound delivery. Incomplete metadata
must not erase observed occurrences or turn unknown into confirmed zero. Config/ACL
revocation stops eligibility. No locks held through native/network I/O.

Encrypted payload ONLY `{version:1,alert_id:64HEX}`. It contains no question, answer,
permission, command, path, title, team, count or authority token. Private notification
state is bounded; disclose failure/coverage rather than promise guaranteed delivery.

## Web UI and service worker

Keep BASIC root cards and add needs-response badges plus a concise attention status.
Poll attention metadata separately while visible; do not poll transcript/details.
Opening the verified card remains one click, with its current requests/form in the
same dialog. Load full decision details on explicit open, render as inert text, show
complete choices/permission patterns and explicit denial scope. Separate follow-up
composer from answer/allow-once/reject; no generic affirmative approval or always.

Add explicit Enable alerts / Disable alerts controls. Never request notification
permission on page load. Register `/service-worker.js` only for opted-in alerts,
scope `/`, no fetch/offline caching. Use PushManager with the gateway public VAPID
key; subscribe/register and unsubscribe require fresh CSRF. Unsupported/denied APIs
fall back to in-page attention without misleading background-enabled status.

The worker accepts only the tiny closed push payload and displays constant generic
title/body ("Coven needs your attention" / "An agent is waiting for your response").
Use alert_id for dedup tag and same-origin navigation `/#attention=<alert_id>`.
Notification click focuses/opens the origin only, NEVER answers/approves. Page
reauthorizes by attention metadata and exact current occurrence before opening its
verified root and request; absent/revoked/changed occurrence shows unavailable.
Same occurrence may be reauthorized under a new grant; never retarget another ask.
No sensitive browser persistence/offline payloads. Subscription permission/state is
the explicit narrow persistence exception. No lock-screen request details by default.

HTTP/API frontend worker owns the service-worker asset, CSP worker-src 'self',
packaging include and native_package expectation. HTTP may wrap Observer and optional
PushService in WebApi with default-denied Api trait methods, preserving old tests.
Keep metadata/native and mutation jobs bounded; alert delivery is advisory and
depends on host/native share/browser permission/platform delivery and connectivity.

## Verification and rollout

Synthetic native waits must prove actual conditional service settlement, CAS/ABA,
local-vs-remote races, cancellation/reload/grant fences, unsupported full details,
question validation, denial cutoff and no remote always. Real-producer HTTP tests
must cover exact detail/response and uncertainty without providers/model calls.
Push tests validate crypto/generic payload, DNS/address policy/redirect refusal,
owner isolation/private state/dedup/retry/410 and collection with no open browser.
Service-worker/UI tests cover permission denial, opt-in only, exact notification
navigation, stale refs, no auto actions, original card/focus and uncertainty behavior.

Deploy default-disabled capabilities, then explicitly enable the sole owner's native
decision setting, gateway decision ACL, and optional private push store with backups.
Browser permission/subscription remains the user's explicit action. Do not restart
active agents; a new native build needs a later deliberate UI relaunch. Do not claim
phone delivery, live native answer or one-click readiness from fixtures alone.
Worktrees created for this effort must be integrated/preserved and removed when done.
