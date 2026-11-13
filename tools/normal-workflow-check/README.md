# Normal Workflow Acceptance

Opt-in, development-only Linux acceptance harness for
[`docs/NORMAL_AGENT_WORKFLOW.md`](../../docs/NORMAL_AGENT_WORKFLOW.md).
Nothing here is installed, automatically run, or imported by Coven production code.
All runtime dependencies are explicit paths. Python 3 uses only its standard library;
Bun imports Djinn's actual built-in `KitsuneAgentStatePlugin` and its native reporter.
The fixture supplies native-shaped event input over stdin, not a provider UI, SDK
runtime, model, or a replacement implementation of the plugin.

## Run

From the Coven checkout, with already-built executables:

```sh
python3 -B tools/normal-workflow-check/check.py \
  --kitsune-bin /absolute/path/to/kitsune \
  --coven-bin /absolute/path/to/coven \
  --djinn-plugin /absolute/path/to/djinn/clients/djinn-ui/packages/opencode/src/plugin/kitsune-agent-state.ts \
  --bun /absolute/path/to/bun
```

Offline dirty-source snapshot build plus the same acceptance checks, qualified on
this development machine:

```sh
python3 -B tools/normal-workflow-check/build-and-check.py \
  --kitsune-source /home/jdawson/Projects/kitsune \
  --coven-source /home/jdawson/Projects/coven \
  --djinn-plugin /home/jdawson/Projects/djinn/clients/djinn-ui/packages/opencode/src/plugin/kitsune-agent-state.ts \
  --bun /home/linuxbrew/.linuxbrew/bin/bun \
  --zig /home/linuxbrew/.linuxbrew/opt/zig@0.15/bin/zig \
  --zig-package-cache /home/jdawson/.cache/zig/p
```

The builder explicitly passes its newly built absolute executable paths to the
same check implementation. There is no implicit sibling lookup. Both source
checkouts are copied including dirty source, excluding `.git`, existing build
outputs, `node_modules`, and local metadata. Builds never mutate either checkout.
Snapshot copying is not an atomic transaction with concurrent editors: rerun after
core work settles to qualify that exact revision.

Rust is selected using `rustup run 1.96.1`, Cargo is `--offline --locked -j 4`,
development debug information is disabled, and incremental compilation is disabled.
The Zig executable and existing offline package cache are explicit; the latter is
passed through `LIBGHOSTTY_VT_ZIG_SYSTEM_DIR`. No toolchains/packages are installed
or downloaded. Cargo/Rustup package/toolchain caches are reused, not copied.
`CARGO_TARGET_DIR`, `TMPDIR`, and Zig local/global caches are run-owned. Kitsune's
hardcoded `vendor/libghostty-vt/zig-out` stays inside its disposable source snapshot.

## Assertions

- Default and presentation-only `integrations.sources` server startup creates no
  live integration lease. Real new PTY panes strip inherited integration routes
  and external-management variables.
- An independently started opt-in server shuts down with a live actual Coven
  helper: Kitsune closes stdin and reaps it, and Coven removes its receiver socket.
- Production `kitsune server` starts the configured literal `coven agent run`
  after API readiness. `/proc` verifies helper argv and direct Kit parentage.
- Actual new PTY pane environment contains the configured source, JSON literal
  reporter argv, and external ownership, replacing deliberately poisoned inherited
  routes. The Bun fixture uses this pane-delivered environment, except its legacy
  Kit socket is redirected to a private listening canary. Native reporter argv
  remains unchanged; the canary detects even unsuccessful legacy fallback attempts.
- Actual `pane.report_agent` rejects legacy status with
  `external_agent_management`, both before and after helper loss.
- Native fixture input goes through the imported Djinn plugin, its real child
  spawn, actual `coven agent report-native` stdin validation/RPC, the actual attached
  receiver/presenter, Kitsune's production handlers/store, and production rendering.
- Real semantic-frame client output contains attributed Coven / Djinn rows with
  working, blocked, idle, and expired-unknown evidence. Assertions match a provider
  label and its adjacent detail line, not unrelated rows or terminal output.
- Permission and question asks block; **both native cancellation event names**
  clear their matching pending decisions and reveal still-fresh working evidence.
- Child and unestablished-root input cannot change the established root's idle
  state. Private native IDs and request IDs do not appear in the checked view rows.
- A real Codex reference-only hook receipt remains unknown; no Codex process runs.
- Real 30-second native evidence expiry occurs with Coven still alive. Presenter
  repainting does not renew the Djinn row's evidence.
- SIGKILL of Coven causes actual five-second Kit lease expiry and rendered
  `Unavailable`, without helper restart or restored legacy status authority.
  Further plugin input after transport loss remains nonfatal; the legacy socket
  canary receives no connection and actual Kit agent inventory remains empty.
  Then actual `server.stop` succeeds.

Each completed gate prints `PASS`. Any failure is nonzero; cleanup is still run.
The two clocks are not shortened or mocked. Typical execution includes at least
30 seconds of real expiry waiting, in addition to source build time.

## Isolation And Cleanup

The harness requires Linux `/proc`, child subreaping, `findmnt`, Unix sockets,
`/bin/sh`, and an existing disk-backed cache parent. It creates a unique private
directory under `${XDG_CACHE_HOME:-$HOME/.cache}/agent-test-runs`, verifies the
filesystem is not RAM-backed, and checks at least 12 GiB free before work. Run roots
and socket parents are mode 0700. Very long cache paths are refused for Unix socket
address limits. No artifacts go to `/tmp` or `/dev/shm`.

Every application starts with an allowlisted isolated environment: private HOME,
XDG directories, config path, session name, API socket, and client socket. Shells
are explicitly `/bin/sh` in non-login mode. Workspace initialization is an explicit
minimal `coven.json` in the private workspace, matching Coven's receiver fixtures.
No hooks/providers are installed; no agent launcher, user service, real user
configuration, or existing user socket is used. No installed app is invoked.

Runtime file sizes are capped at 32 MiB. Build logs are monitored against a 32 MiB
cap; individual build artifacts have a separate 4 GiB limit. All command, build,
API, event, render and cleanup waits are bounded. The small fixture event stream
and latest in-memory frame are bounded; there is no frame history or raw event log.

Cleanup is registered before any build/application children start. Normal success,
failure, SIGINT and SIGTERM stop descendants, including PTY-created process sessions
and orphaned reporter children, reap them via Linux subreaping, verify no owned
descendants remain, and only then delete the exact owned run tree. Cleanup failure
is an error and does not silently remove the tree while children remain. Successful
cleanup prints `CLEANUP confirmed`. SIGKILL/power loss cannot run cleanup; do not
remove another run or saved session by age alone.

## Test Level

This is **actual Linux headless production server startup and rendering**, not a
direct-handler proxy or a source-only startup claim. Startup was reviewed against
Kitsune's `src/main.rs` (`server` dispatch), `src/server/headless.rs` (API readiness,
helper ownership and shutdown), `src/server/socket_paths.rs` (client derivation),
`src/session.rs` (explicit session/socket precedence), and `src/integration/env.rs`
(new-pane environment). It never calls the default auto-detect/client launch path.
The small client speaks protocol 18/bincode 2 semantic frames and fails closed on
version/encoding mismatch. It displays no real terminal and executes no received
notification/clipboard/title payloads. A deliberately wide private sidebar makes
the evidence fields visible rather than asserting clipped strings.

Not certified: macOS/Windows/native-provider UI behavior, real SDK event emission,
physical input/client-pixel acknowledgement, complete retention/queue/replay bounds,
restore-reference policy, malicious endpoint admission, or the full repositories'
unit suites. The cancellation checks prove native-shaped cancellation delivery through the
actual plugin, not that an actual provider UI emits it. Those are separate gates.
