# Coven Web

Coven's phone-first browser client. The authoritative API is the
[Coven Web contract](../../docs/architecture/COVEN_WEB_CONTRACT.md); see the
[browser guide](../../docs/guides/COVEN_WEB.md) for usage and the
[deployment runbook](../../docs/operations/COVEN_WEB_DEPLOYMENT.md) for operation.
This directory has no framework,
build step, dependencies, external assets or telemetry. Private content is memory-only;
browser notification permission and push subscription state are narrow exceptions.
Node fixtures are synthetic tests only, never a fallback production fleet.

## Static Asset Interface

The parent HTTP server needs exactly these same-origin GET asset routes:

| Route | File | Content-Type |
| --- | --- | --- |
| `/` (and optionally `/index.html`) | `index.html` | `text/html; charset=utf-8` |
| `/app.js` | `app.js` | `text/javascript; charset=utf-8` |
| `/styles.css` | `styles.css` | `text/css; charset=utf-8` |
| `/service-worker.js` | `service-worker.js` | `text/javascript; charset=utf-8` |

No `model.js` or additional runtime assets are required. `app.js` exports pure
helpers and bootstraps only when `document` exists. Do not embed the tests,
package metadata or this README as public routes. Assets assume root hosting,
not a path-prefix deployment. Use authenticated, no-store responses; do not add
offline caching or public hosting. The opt-in push worker has no fetch handler.
Deployment remains private Tailscale Serve,
never Funnel or Vercel. The client supplies no identity headers or credentials.

API calls are same-origin with `cache: 'no-store'`, same-origin
credentials and redirects refused:

- `/api/v1/fleet`: exact `coven.fleet-observer.v1` DTO.
- `/api/v1/events`: content-free named SSE invalidations; see
  [events/cache contract](../../docs/architecture/COVEN_WEB_EVENTS.md).
- `/api/v1/agents/<lowercase-64-hex-id>/output`: explicit card-open or Read again
  gesture only. No output prefetching or output polling.

`output_available: false` is authoritative regardless of activity. No additional
native authority is inferred from these observations. HTTP 401/403 explains
Tailscale/allowlist/origin access, rather than displaying dummy observations.
Other errors expect `{ "error": { "code": "fixed_safe_code" } }`; server error
text is never displayed. HTTP 403 codes `output_disabled` and `output_unsupported`
are operator/capability errors, not fleet authentication failures. Each has a
specific output message; other codes use a safe generic failure message.

## Observation Rules

- Evidence is either `kitsune_snapshot` (shown as Kitsune metadata) or
  `coven_process_discovery` (shown as Coven process evidence). Process cards must
  have `activity: 'unknown'` and `output_available: false`; invalid claims reject
  the snapshot rather than silently upgrading evidence to activity or capability.
- `provider_candidates` is an array of at most 22 nonempty family hint strings,
  required for process cards and optional for older metadata fixtures. Hints are
  not authoritative provider identity, even when only one family is present.
  The backend owns metadata precedence and one-card-per-terminal acquisition;
  the client does not infer terminals from titles or merge opaque references.
- Endpoint `discovery_status` is `observed`, `unsupported` or `unavailable`;
  `unclassified_panes` is a nonnegative safe integer or null. These fields are
  optional for older metadata-only fixtures, required on endpoints with process
  evidence. Failed discovery immediately stales process cards but does not
  invalidate fresh independent Kitsune metadata. Unclassified terminals are not
  counted as agents. Even observed discovery has unknown scan completeness;
  empty snapshots never establish that no agents are running.
- Failed/invalid fleet responses preserve the prior snapshot and immediately
  disable output. A valid empty response removes prior cards. Duplicate endpoint
  IDs or agent references reject the entire response instead of merging cards.
- Cards sort fresh first, then host label, title and opaque identity. Host
  filtering uses configured endpoint IDs, not labels or browser-selected hosts.
- Freshness checks snapshot, endpoint and agent observation times. Output itself
  also expires. A response cannot renew a lease merely by arriving; snapshots
  older than the accepted generation are rejected. Timestamps more than five
  seconds ahead are not trusted as fresh. Keep gateway and browser clocks aligned.
- One dirty coordinator sequences fleet/list/attention GETs. SSE notices coalesce
  during requests; healthy live metadata resyncs every 30s, otherwise every 6s
  after work. Reconnect waits 6s. Upstream fleet polling/backoff is server-owned.
  A 10s timeout bounds each GET; old transport callbacks are fenced.
- Hidden pages stop polling and abort in-flight reads. Resume refreshes inventory
  immediately, including when the prior abort is still settling. Visible ages
  update once a second even after request failures. Browsers may throttle timers;
  fleet freshness uses wall time, while lease-bearing list/attention metadata uses
  monotonic remaining time minus transit, never tick counts. Same-generation
  receipts cannot extend a lease. BFcache pagehide clears private views/closes SSE;
  pageshow restarts the expiry guard and resynchronizes metadata only.
- Closing, changing references, superseding a read or losing freshness clears
  output and invalidates its selection ticket, in addition to aborting its fetch.
  Reopening the same card cannot accept an earlier request's response.
- Output is bounded to the contract's 64 KiB UTF-8 limit and uses `textContent`.
  No terminal/ANSI/OSC interpreter, HTML parsing, links or remote images. Best-
  effort attribution warnings remain visible even after a successful read.
- Copy requires an explicit clipboard gesture and browser support. No automatic
  clipboard writes or fallback permission tricks. Output is memory-only and
  cleared on close/staleness; copied text is outside the app's control.

## Offline Checks

From the repository root (no installation or API access needed):

```sh
node --test clients/web/*.test.mjs
node --check clients/web/app.js
```

Alternatively, from this directory: `npm test` (no `npm install` required).
Tests cover schema rejection, identity collisions, projection/sorting, stale
retention versus successful empty, output limits/identity/age, literal hostile
text at the shared text sink and selection-ticket races. Bootstrap fixtures also
exercise handlers; `events.test.mjs` uses fake EventSource for dirty coalescing,
lease expiry, reconnect and lifecycle/BFcache behavior, not a real browser stream.

Opt-in acceptance (record actual runs in the [changelog](../../docs/changelog/index.md),
not as a standing claim that these recipes currently pass):

- `browser.test.mjs`: set `COVEN_WEB_FIREFOX` to an installed Firefox for actual
  DOM/overflow/focus and explicit-read forms over synthetic HTTP fixtures.
- `live-events.test.mjs`: on Unix with Node 22+, set both `COVEN_WEB_BINARY` to an
  absolute already-built candidate and `COVEN_WEB_FIREFOX` to an absolute installed
  Firefox. Supply an absolute, verified disk-backed `TMPDIR` under
  `${XDG_CACHE_HOME:-$HOME/.cache}/agent-test-runs`, with checked free space and
  exact-directory cleanup after children stop. Run
  `node --test clients/web/live-events.test.mjs` in that environment. The fixture
  exercises native Firefox EventSource against a disposable actual gateway and
  local proxy, not live services, native sessions, Serve authentication or phone
  delivery. It builds/downloads nothing and skips unless both executable variables
  are set. A skipped test is not end-to-end success.

## Browser Acceptance

These checks require an operator-approved, same-origin private deployment with
the actual backend. They have not been replaced by fixture success:

- At 320px and phone/desktop sizes, verify readable cards, wrapping long names,
  host filter, unavailable/unsupported hosts, zero hosts and successful-empty
  snapshots. Confirm no horizontal page overflow or fake fleet on first failure.
- Keyboard and screen reader: skip link, card buttons, native dialog title,
  initial Close focus, Tab trapping, Escape, focus return, output scrolling and
  live connection/error announcements. Check zoom and reduced-motion settings.
- Observe SSE ready/invalidation, 60s stream rotation, 6s fallback/reconnect,
  30s healthy resync, lease expiry, age progression, timeout,
  offline/reconnect and background/foreground behavior in the network inspector.
   Verify same-origin calls, no overlapping fleet requests, no output prefetch,
   no-store fetches and no content storage/offline cache. Push registration and
   permission requests occur only after Enable alerts, never on page load.
- Delay output A, close/reopen A or open B, replace/disconnect its reference and
  resolve the old response. No old text may appear. Let output age out while
  inventory stays fresh; a new read must require a gesture.
- Verify process-only and mixed-evidence cards, ambiguous family hints, unknown
  activity, no process-card output requests, discovery unsupported/unavailable,
  unclassified terminal counts and incomplete coverage even after observed scans.
- Test disabled/unsupported output 403s without fleet authentication failure,
  mismatched/stale output responses, empty/truncated text,
  hostile HTML/ANSI/OSC text, clipboard success/denial and browser without the
  clipboard API. None of that text should become markup, links or terminal input.
- Confirm real 401/403 guidance, intended tailnet identity forwarding, HTTPS
  phone/cellular access, exact origin checks and absence of public/direct access.
  The operator owns deployment and best-effort-output opt-in. Browser checks
   cannot establish atomic attribution or remove the documented ABA race.

## Requests And Alerts

`GET /api/v1/attention` reads cached bounded request metadata during visible
SSE-triggered or fallback resynchronization.
Needs-response badges never come from blocked/idle heuristics. Unknown/partial
coverage is not confirmed zero. Original verified root cards still open once into
the same conversation dialog; no chooser or extra cards. Explicit card/request
opens use `GET /api/v1/decisions/<opaque-id>` for full inert-text detail, never
background transcript/detail polls. Reader-only requests remain inspectable.

Answers support native ordered single/multiple selections and allowed custom text.
Permission controls are only Allow once and Deny pending session permissions;
denial affects all currently pending permissions in that native session, not future
ones. Follow-ups remain separate. Each response obtains fresh CSRF and posts once
to `/api/v1/decisions/<id>/responses`. Applied means native wait settled, not tool
success. Lost receipts retain bounded answer-free per-occurrence/attempt notices;
another request cannot resolve them. Reinspect the exact request and separately
choose before any new attempt. Closing, stale bindings and auth loss fence callbacks.

Enable alerts explicitly requests browser permission, registers the same-origin
worker and subscribes via browser PushManager using `/api/v1/push/config`. Fresh
CSRF protects subscription/unsubscription POSTs. Disable alerts attempts local
browser removal first, even when server config, CSRF or removal fails. Local and
server results are reported separately. Only opaque subscription IDs remain for
another explicit server-cleanup click; there is no automatic mutation retry.
Unknown creation attempts are tracked separately: absent config or an ordinary
delete receipt cannot prove an older blocking create will not commit afterward.
After a lost creation receipt, browser opt-out still works, but server cleanup
remains unverified for this page even after explicit cleanup succeeds. Further
cleanup requires another click; status polling never retries it. These bounded
records contain only opaque subscription IDs, not endpoints or browser keys.
Alert setup, browser promises and network/body waits have a 10-second deadline;
a timeout after subscription POST means unverified, not proof of non-registration.
Subscription-enabled state is separate from advisory/degraded/unverified delivery.
Passive inspection is fenced by each new user operation, so an old status response
cannot undo a completed Disable action. Identity hashing accepts only bounded HTTPS
browser endpoints and saves the opaque ID before waiting for local unsubscription.
Existing endpoints owned by
another principal are never silently attached; explicit re-enable replaces them.
There is no automatic permission request or subscription on reload. Denied,
unsupported or unverified background state does not disable in-page attention.

The server collector runs independently of an open tab. Worker payloads contain
only a version and opaque alert ID; lock-screen title/body are constant generic
text, without provider content or approval actions. Clicking navigates only to
`/#attention=<alert-id>` on this origin. The page reauthorizes fresh metadata and
the exact verified occurrence; it never chooses a newer request. Missing alerts,
including incomplete coverage, mean cannot currently verify, not resolved. Fresh
metadata can later verify that same stable alert, including a new grant-bound
decision reference. A successfully opened and dismissed alert stays dismissed.

Browser HTTPS/permission/platform support, native sharing policy, grant lifetime,
server connectivity and vendor delivery still apply. iOS may require an installed
home-screen web app. Delivery is advisory, not guaranteed. Synthetic worker-VM
tests prove handler behavior without an open page; they do not prove actual phone
delivery or live native settlement. Real subscription remains an explicit user step.
