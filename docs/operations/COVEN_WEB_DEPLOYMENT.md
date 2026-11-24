# Coven Web deployment runbook

## Overview

This is the reusable configuration, build and private-operation procedure, not an
installed-state assertion. See [deployment state recorded 2026-11-21](COVEN_WEB_DEPLOYMENT_STATE.md)
for exact selected payloads and immediate rollback backups, and the
[contract](../architecture/COVEN_WEB_CONTRACT.md) for security/resource boundaries.
Do not deploy or restart native processes merely to validate documentation.

## API / Interfaces

### Configuration and CLI

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

### Native conversation registration

Static registrations remain supported and read-only. Normal-launch shared sources
are a separate opt-in integration; see [the browser guide](../guides/COVEN_WEB.md)
and the related sharing contracts. Do not infer static targets from fleet cards.

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
described in the [contract](../architecture/COVEN_WEB_CONTRACT.md#authentication-and-deployment-boundary).
Do not deploy on an untrusted multi-user host.

Validate with the **newly built**, separately scoped executable using
`/path/to/new/coven observer check --config /path/to/observer.json`. Starting
`/path/to/new/coven observer serve --config /path/to/observer.json` is an explicit
operator startup step, not something this implementation activates. Do not replace
running binaries or restart existing services merely to validate configuration.
The installed older executable does not acquire this behavior without a new build.

## Data Flow

### Development checks

The browser assets are embedded in native-source packaging; no Node production
service or frontend build dependency is required. Web Push adds an OpenSSL build
dependency; see [development OpenSSL selection](WEB_PUSH_BUILD.md) before choosing
a scoped build environment. These commands are validation recipes, not evidence
that a full gate has passed. Use unique disk-backed `TMPDIR`/`CARGO_TARGET_DIR`
locations, check space, and clean the exact owned directories after child shutdown.

```sh
cargo test --offline --locked --lib observer::
cargo test --offline --locked --test observer_gateway --test native_package --test native_install
cargo clippy --offline --locked --all-targets -- -D warnings
cargo fmt --all -- --check
node --test clients/web/*.test.mjs
```

Historical initial-development counts and incomplete broad-suite results are
preserved in [the prototype record](../changelog/coven-web-initial-prototype.md#initial-development-check-results).
Later scoped results and known failures remain in their dated change records.

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

### Rollout

Foreground SSE uses the same private Serve origin and needs no new public route
or push configuration. On the approved proxy path, verify incremental delivery of
`/api/v1/events` (`text/event-stream`, no-store): initial `ready`, 10-second comment
heartbeats and invalidations must not wait in a proxy buffer until stream close.
If an additional proxy is present, disable response buffering for this route and
allow the 60-second body lifetime/65-second admitted connection bound. Ordinary
requests remain bounded to 20 seconds. Do not weaken identity/origin checks or
enable caching to accommodate streaming.

Check stream rotation/reconnect, hidden-tab and BFcache return, six-second fallback
when SSE is blocked/unavailable, and absence of automatic message/output/detail
GETs or POSTs. Two streams per login/16 global can cause intentional 429 fallback
with many tabs. A buffered or connected stream is not proof of fresh metadata;
see [event/cache semantics](../architecture/COVEN_WEB_EVENTS.md) and
[opt-in acceptance scope](../../clients/web/README.md#offline-checks).

Before live rollout: confirm allowed user and local-host trust, enable tailnet
HTTPS/Serve with user approval, authorize only intended phone HTTPS access, verify
header forwarding and no direct network listener, and decide whether the qualified
best-effort output is acceptable. No public Funnel, router forwarding or encryption
changes. Validate one existing endpoint at a time, then actual phone/cellular,
offline/reconnect and foreground/background behavior. Fixture success is not live
acceptance. Keep the previous deployment/configuration for rollback.

### Local subscription canary

For a separately approved candidate gateway, canary one already configured local
protocol-18 endpoint without restarting Kitsune/native workers, changing their
configuration or upgrading remote helpers. First confirm ordinary inventory works;
only then may the gateway open the fixed read-only `events.subscribe` channel.
Use existing bounded diagnostics or an isolated synthetic peer to inspect setup:
the twelve fixed selectors, correlated `subscription_started` ACK, and ACK deadline
bounded by five seconds and the next inventory due time. Do not add a browser RPC
selector, log raw frames or treat ACK/connection health as metadata freshness.

Observe naturally occurring supported topology/title changes, or use the synthetic
fixture; do not create/close/move panes, send prompts, answer decisions or perform
any native action as a canary test. Confirm only completed normal fenced inventory
changes fleet data, and meaningful accepted local changes wake paired metadata.
Check continued periodic reconciliation during silence, unchanged lease expiry,
bounded reconnect, and no automatic transcript/output/detail reads or POSTs. Missing,
replaced and unsafe sockets must invalidate evidence; exercise those cases in
isolated fixtures, never by replacing a live socket. SSH endpoints must stay
unsubscribed. Upstream silence has no heartbeat/finite-expiry requirement and is
not a fault by itself; browser SSE heartbeats are a different channel.

See [local hint semantics and fixture scope](../architecture/COVEN_WEB_LOCAL_EVENTS.md).
The real-gateway/mock-Unix fixture is not actual Kitsune or phone acceptance. Record
candidate tests, release gates and actual deployment evidence separately when
available; this procedure does not assert a live rollout or authorize an action test.

### Private installation and operation

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

## Edge Cases

### Rollback and availability

Rollback for a deployment using this layout: disable/stop **only**
`coven-web.service`, restore its previous immutable
payload/config if updating, and remove only its Serve HTTPS listener when owned
exclusively by this application (`sudo tailscale serve --https=443 off`). Never
reset unrelated Serve routes or stop Kitsune/Coven agent runtimes. A user service
starts with the user's manager; without lingering it is not a promise of operation
after logout or before user login following encrypted boot. Browser assets and
observations are unavailable when the gateway host is off; HA is out of scope.

For an update rollback, preserve matched gateway/helper schema compatibility and
restore the exact paired backups in the [dated state](COVEN_WEB_DEPLOYMENT_STATE.md#rollback).
Native UI selection rollback is separate, must preserve matched payload/config,
and must not implicitly terminate active work. Installing a native bundle does
not retrofit running workers, grants or pending waits.

## Example usage

Prepare a private operator configuration using the synthetic schema above, then
validate it with the separately scoped candidate's `observer check`. Serving,
publishing a helper, enabling private Serve, changing policy and deliberately
relaunching a native UI are separate approved steps, never browser side effects.

## Related files

- [Operations index](index.md), [dated deployment state](COVEN_WEB_DEPLOYMENT_STATE.md),
  [browser guide](../guides/COVEN_WEB.md), [contract](../architecture/COVEN_WEB_CONTRACT.md),
  [recovery obligations](../plans/COVEN_WEB_RECOVERY.md), [history](../changelog/index.md).
- [Configuration](../../src/observer/config.rs), [HTTP startup](../../src/observer/http.rs),
  [CLI](../../src/main.rs), [client acceptance](../../clients/web/README.md).
