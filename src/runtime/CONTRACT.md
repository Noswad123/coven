# Protected Runtime Behavior

`tests.rs` characterizes the Python `command_send`, `send_to_agent_pane`,
`notify_agents`, and legacy down command paths without invoking a real backend.

- Select exact persisted agent IDs; `all` preserves roster order, not display names.
- Admit through a shared transport gate before workspace exclusion. Recheck actual
  control, orphan bundles, and location after admission; a previous load is not authority.
- Approved mode rejects legacy delivery. Down delegates to
  `actions::mutation::stop_current`, without reading broken mutable configuration.
  Frozen consumed-head plan selection remains owned/tested by actions mutation.
- Validate workspace, dashboard rendering, full logs, encoded records, and aggregate
  log capacity before lookups/effects. Hold shared admission through lookup,
  text/Enter, and successful-send history; history requires workspace exclusion.
  Release workspace exclusion before every native lookup/text/Enter/stop call;
  reacquire and revalidate for each delivered agent's history, and separately for
  dashboard publication. Post-delivery publication failures report known delivery
  and never trigger retries.
- Herdr sends text then Enter; tmux sends one text/Enter command to the first pane.
  Missing panes are counted, not errors. Failed Enter never records delivery or retries.
- Successful user delivery records message, `message.sent`, then
  `agent.message.sent`; dashboard refresh and INFO counts follow the batch.
  Command bodies have no terminal newline; the CLI adds exactly one.
- Checkpoint notice transport neither takes workspace exclusion nor writes history.
  The coordinator's identity check remains before admission; this API rechecks mode
  after admission, not checkpoint identity. Notice history stays with the coordinator.
- Legacy down selects only the human-name `coven-<slug>` session/workspace. It does
  not use frozen namespaces or stop unrelated workspaces. Invalid authority cannot
  fall back to legacy behavior.
- Native commands use an absolute PATH-selected executable without canonicalizing
  the selected alias or collapsing symlink-parent traversal. Inherit all Unicode
  ambient environment variables, not the frozen-launch six-key environment.
  Capture the caller's `std::env::current_dir()` once when constructing the legacy
  backend. Workspace identity is separate: never use the workspace as either
  native execution cwd or the base for relative/empty PATH entries. Both selection
  and execution use that captured caller cwd even if the process later changes cwd.
  Frozen launch retains its own frozen cwd, unchanged.
  Reuse `launch::executable` for ordered candidates from the captured environment
  and caller cwd. Only `ExecFailed` proves the selected image never ran and permits the
  next PATH candidate; exit 127, other exits, timeout, output limit, and generic
  spawn/setup/capture failures never authorize fallback. Candidate lookup and
  spawn attempts share a single 60-second deadline per backend command.
- Native profile differences from Python: null stdin, 60-second timeout, 1 MiB
  combined output budget, and strict safe Herdr IDs. Timeout/capture errors do not
  prove an effect did not occur; never retry automatically or use the probe runner.

Parent wiring is intentionally outside this module's scope: expose `pub mod runtime`
and declare `mod runtime_command`, `mod transport_gate`, and `mod launch` in the
crate root. Existing helpers used here must remain crate-visible:
`actions::validate_workspace_control`, `launch::executable`,
`workspace`, `workspace_assets::slug`, `logs`, `task_delivery`, and `task_time`.
The shared executable selector/run API is crate-visible; launch and legacy
transport use the same ordered fallback implementation with different captured
environments.

General fake-backend acceptance fixtures and CLI routing belong to the parent
integration batch. Injected runners use test HOME for workspace control data,
not native cwd. The caller-cwd regression compiles an inert executable and runs
native send/down in an isolated exact-test child with relative PATH and a workspace
decoy. Tests never invoke real tmux, Herdr, or a provider.
