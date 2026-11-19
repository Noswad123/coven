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
- No prompts, approval emulation, keyboard input, focus, launch, stop, handoff,
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
