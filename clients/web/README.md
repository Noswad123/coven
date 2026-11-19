# Coven Web

Coven's read-only, phone-first browser client. The authoritative API is
`docs/COVEN_WEB.md` at the repository root. This directory has no framework,
build step, dependencies, external assets, telemetry or persistent browser storage.
Node fixtures are synthetic tests only, never a fallback production fleet.

## Static Asset Interface

The parent HTTP server needs exactly these same-origin GET asset routes:

| Route | File | Content-Type |
| --- | --- | --- |
| `/` (and optionally `/index.html`) | `index.html` | `text/html; charset=utf-8` |
| `/app.js` | `app.js` | `text/javascript; charset=utf-8` |
| `/styles.css` | `styles.css` | `text/css; charset=utf-8` |

No `model.js` or additional runtime assets are required. `app.js` exports pure
helpers and bootstraps only when `document` exists. Do not embed the tests,
package metadata or this README as public routes. Assets assume root hosting,
not a path-prefix deployment. Use authenticated, no-store responses; do not add
a service worker or public hosting. Deployment remains private Tailscale Serve,
never Funnel or Vercel. The client supplies no identity headers or credentials.

API calls are GET only, same-origin with `cache: 'no-store'`, same-origin
credentials and redirects refused:

- `/api/v1/fleet`: exact `coven.fleet-observer.v1` DTO.
- `/api/v1/agents/<lowercase-64-hex-id>/output`: explicit card-open or Read again
  gesture only. No output prefetching or output polling.

`output_available: false` is authoritative regardless of activity. No additional
configuration endpoint or UI-control API is assumed. HTTP 401/403 explains
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
- Poll after each completed fleet request at the server interval (default 3s),
  with a 1s minimum and 60s ceiling. Failures exponentially back off within that
  ceiling. A 10s request timeout bounds waiting. Requests never overlap.
- Hidden pages stop polling and abort in-flight reads. Resume refreshes inventory
  immediately, including when the prior abort is still settling. Visible ages
  update once a second even after request failures. Browsers may throttle timers;
  every freshness decision uses wall time, not tick counts.
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
text at the shared text sink and selection-ticket races. These are not DOM,
browser, deployment, authentication or upstream integration tests.

## Browser Acceptance

These checks require an operator-approved, same-origin private deployment with
the actual backend. They have not been replaced by fixture success:

- At 320px and phone/desktop sizes, verify readable cards, wrapping long names,
  host filter, unavailable/unsupported hosts, zero hosts and successful-empty
  snapshots. Confirm no horizontal page overflow or fake fleet on first failure.
- Keyboard and screen reader: skip link, card buttons, native dialog title,
  initial Close focus, Tab trapping, Escape, focus return, output scrolling and
  live connection/error announcements. Check zoom and reduced-motion settings.
- Observe normal interval, failed-refresh backoff, age progression, timeout,
  offline/reconnect and background/foreground behavior in the network inspector.
  Verify only same-origin GETs, no overlapping fleet requests, no output prefetch,
  no-store fetches and no new browser storage/service workers.
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
