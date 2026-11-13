# Integration setup and bootstrap

## Overview

The implemented setup surface configures normal native-agent reporting without
manual workspace initialization or socket coordination. Kit stays generic;
Coven owns bootstrap, capture and interpretation. Native macOS qualification and
real provider UI emission remain separate from source and controlled validation.

## API / Interfaces

`coven integration plan` previews the one-time setup without creating files,
locks, directories, sockets, or processes. `coven integration enable --apply`
explicitly enables external ownership. `coven integration disable --apply`
restores an exact managed preimage only when newer user edits will not be lost.
Without `--apply`, enable/disable are previews too.

Optional `--kitsune-config`, `--executable` and `--state-root` select advanced
absolute paths. First-time defaults use Kit's documented startup config selection
(`KITSUNE_CONFIG_PATH`, otherwise `$XDG_CONFIG_HOME/kitsune/config.toml`, otherwise
`$HOME/.config/kitsune/config.toml`), the validated current Coven executable, and
`$XDG_STATE_HOME/coven/integration` (nonempty XDG state value) or
`$HOME/.local/state/coven/integration`. These describe selection, not literal argv
expansion. Paths must be absolute; invalid/empty config overrides refuse. Missing
HOME when needed does not select a shared temporary directory. Debug Kit still
uses the `kitsune` config directory, not `kitsune-dev`. No PATH/sibling-build
guessing or shell expansion. Advanced `--kitsune-config` overrides must name a
`config.toml` leaf; normal setup needs no path arguments.

For a managed configuration, omitted executable/state-root pins use the receipt's
recorded values, not today's invoking binary or environment defaults. Explicitly
different pins refuse, including for disable. An active managed `plan` reports
unhealthy executable/root checks as a warning with `runtime_ready: false`, while
still describing the recorded configuration. This is local preflight, not proof
that a helper is running or provider delivery works. Enable requires readiness.

Setup preserves other sources, comments and unrelated settings. It refuses an
unmanaged runtime instead of adopting it. Explicit provider hook/extension
installation and native consent remain separate, and are reported as such.
Setup performs no live API calls, provider installation, server restart or native
trust override. A selected development executable produces a warning: removing or
rebuilding it affects future starts. Configuration remains pinned to that binary.

## Data Flow

Setup generates one typed specification into both literal command arrays:

- runtime: `COVEN integration runtime --state-root ROOT`;
- reporter: `COVEN agent report-native --from-kitsune --state-root ROOT`.

Source is `coven`, with explicit external ownership. Kit remains generic and
continues providing the real API address in `KITSUNE_SOCKET_PATH`. No API socket,
receiver socket or workspace is manually pinned in generated configuration.
The attached runtime bootstraps its own minimal receiver workspace and invokes
the existing receiver/presenter in process. Stdin EOF remains its lifetime signal.

Each validated exact API address selects a separate bounded private instance key.
The receipt compares the original address and API identity; the hash is not
authentication. Shared registry admission waits cooperatively for up to three
seconds on contention; this is not a setup-inclusive startup deadline. Per-instance
ownership contention refuses immediately. An exclusive instance lock and
active/released receipt fence startup. Reporters never create or repair state,
and must verify the active receipt/API/receiver identities before using the
existing private client.
Clean shutdown releases state only after owned receiver cleanup; crashes or
conflicting/partial state refuse reuse rather than unlinking/adopting a socket.
Protection against stale generations is limited: providers must be restarted in
new panes after a Kit server restart. No process/PID/native-session identity join.

### Reporter route selection

`agent report-native`, `agent report`, and `agent report-extension` accept either
the existing explicit `--socket ABS`, or `--from-kitsune [--state-root ROOT]`.
Environment mode requires the expected Coven source/external ownership context.
If ROOT is omitted by an installed hook, the selected generic reporter argv
provides the already-configured root; no default or other-instance fallback is
permitted. Root overrides must agree with the selected route.

The five hook/extension installers gain the same explicit route selection. Their
new environment-route receipts are versioned and preserve old fixed-route bytes,
receipt readers and guarded uninstall/rollback. Existing fixed routes are not
silently migrated or reinterpreted. Event capability/unknown-state semantics do
not change. Normal native invocation remains unchanged.

## Edge Cases

- **Unmanaged configuration:** any existing runtime without the setup receipt
  refuses, including the earlier manual `agent run` configuration. There is no
  adopt/force flag. Stop affected providers and the Kit server/helper; preserve
  the config and provider preimages. Inspect and deliberately remove the old
  runtime stanza while preserving other settings and, during handoff, avoid
  starting Kit in an unintended ownership mode. Re-run plan/enable, then restart
  Kit and providers in new panes. Fixed-route hooks require their own guarded
  uninstall/reinstall; setup does not rewrite them.
- **Exact undo:** the adjacent `coven-kit-integration.json` journal records original
  bytes/mode/absence and generated bytes. Disable restores that exact preimage,
  not a semantic merge; later user edits (even unrelated settings) refuse. Pending
  enable/disable can continue only against exact recorded images and in the same
  direction. Retained filesystem recovery artifacts require inspection. Never
  discard journals or blindly retry/overwrite to bypass a conflict.
  Disable does not require the recorded executable to exist or the runtime root
  to be healthy: it only performs guarded config preimage undo and does not
  mutate runtime state. Config/receipt safety and exact-image checks still apply.
- **Ownership handoff:** disable only changes config. Stop affected providers,
  inspect any separate provider rollback, restart Kit and open new panes/reload
  providers. Removing rows, config reload or helper failure never returns authority
  live. Historical references do not authorize automatic resume in external mode.
- **Privacy:** no-follow traversal rejects symlink paths; private runtime
  directories require owned exact 0700 mode, files owned single-link 0600, and
  supported ACL/filesystem admission. Existing unsafe ownership/modes/ACLs are not
  repaired. Before enable can change config, read-only root preflight checks ACL
  admission and, for a missing root, the existing creation parent's effective
  write/search access without creating probe files. An active managed plan warns
  on failed readiness instead; disable skips runtime readiness. Hook-file setup
  mutation uses the shared guarded backend. Linux and provisional eligible
  local-APFS macOS support are not native macOS certification;
  see [private storage](AGENT_PRIVATE_STORAGE_CONTRACT.md) and
  [file backend](HOOK_FILE_BACKEND_CONTRACT.md) for platform gates.
- **Bounded state:** at most 64 instance entries are admitted per state root;
  clean release does not delete them. There is no automatic cleanup. Receiver
  paths must fit the platform's `sockaddr_un.sun_path`, including the terminating
  NUL. A too-long default root refuses; a shorter private absolute `--state-root`
  is an optional advanced remedy, selected at setup rather than repeated in hooks.
- **Crash recovery:** release is recorded only after successful receiver shutdown
  and verified owned socket cleanup. Receipt-monitor failure cancels the owned
  receiver, joins workers and runs owned cleanup, but retains unreleased state
  rather than declaring a clean reusable instance. Unreleased receipts, partial
  bootstrap or socket conflicts remain diagnosable but refuse reuse. There is no self-healing,
  socket unlink/adoption or automatic helper restart. Stop the owning server and
  providers, establish that no instance is active, preserve/inspect the exact
  affected instance and recover it deliberately; never blanket-delete state or
  mark a receipt released to force startup. A restart alone may still refuse.
- **Generation limits:** API/receiver identity checks are not an atomic binding
  across the later report or native authentication. Restart providers in new panes
  after every Kit restart, and after adding/changing provider setup; inherited
  routes do not retrofit old processes. Reference-only providers stay unknown.

## Example usage

```sh
coven integration plan
coven integration enable --apply
```

Review first, then restart Kit, open new panes, select **Coven**, and invoke an
updated Djinn full-TUI build normally. For other providers follow the explicit
[hook/extension examples](NORMAL_AGENT_WORKFLOW.md#example-usage). No socket or
workspace argument is needed. Preview undo with `coven integration disable`, then
use `coven integration disable --apply` and complete the restart handoff.

Advanced overrides, when necessary, must be consistent between preview/apply:

```text
coven integration plan --kitsune-config ABS_CONFIG_DIR/config.toml --executable ABS_COVEN --state-root ABS_PRIVATE_ROOT
coven integration enable --kitsune-config ABS_CONFIG_DIR/config.toml --executable ABS_COVEN --state-root ABS_PRIVATE_ROOT --apply
```

Raw `[integrations.runtime]` TOML and fixed `agent run`/`--socket` routes remain
advanced internal interfaces, not normal setup instructions. Any manual snippet
must preserve existing config, ownership and provider preimages and use the same
guarded handoff; it cannot later be silently adopted by setup.

## Validation boundaries

Test pure plans, safe TOML preservation, ownership conflict, idempotence, exact
disable, user-edit refusal, endpoint privacy, per-instance concurrent isolation,
clean reuse, crash refusal, inherited-context failure, all five reporter routes,
old receipt fixtures, and actual generated-setup acceptance through Kit and
Djinn's real plugin with controlled native-shaped input. Runtime setup is not a
replacement for real provider UI emission or native macOS qualification.

## Related files

- [Normal workflow](NORMAL_AGENT_WORKFLOW.md),
  [provider routes and receipt versions](PROVIDER_ENVIRONMENT_ROUTES.md).
- `src/main.rs` — CLI setup and reporter/installer route selection.
- `src/integration_setup.rs`, `src/integration_setup/paths.rs` — TOML and journal.
- `src/integration_runtime.rs`, `src/integration_runtime/private_fs.rs` — bootstrap,
  release fencing and read-only reporter resolution.
- `src/integration_route.rs` — versioned installer pins, not socket addresses.
- Kitsune `src/server/integration_runtime.rs`, `src/integration/env.rs` — attached
  lifetime, actual API address and pane reporter context.
