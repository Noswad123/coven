# Doctor status and integration compatibility

## Interface

`coven doctor status` reports native build identity, versioned capabilities and
policy-derived defaults. `coven doctor status --json` is the machine interface:

```json
{
  "contract": "coven.doctor-status.v1",
  "version": "0.1.0-dev",
  "executable": "/absolute/canonical/native/coven",
  "capabilities": {
    "integration_runtime": { "versions": [1] },
    "native_event_reporting": { "versions": [1] }
  },
  "defaults": {
    "integration_state_root": "/home/user/.local/state/coven/integration"
  },
  "diagnostics": []
}
```

The closed v1 schema has exactly these fields. `version` is a human application
version, **not** a compatibility signal. Consumers negotiate supported versions
of each required capability. Linux/macOS advertise both v1 capabilities; platforms
without the required privacy implementation advertise empty version arrays.

`executable` is the canonical absolute UTF-8 path of the running native program.
Invoking doctor through a public dispatcher such as Arcana therefore identifies
the selected native payload, not the dispatcher script. This is a pathname
selection, not authentication, a binary digest or a build-generation guarantee.

The integration root follows existing Coven policy: nonempty `XDG_STATE_HOME`
plus `coven/integration`, otherwise nonempty `HOME` plus
`.local/state/coven/integration`. If that policy cannot yield a valid path, doctor
still exits successfully with a null root and a diagnostic code string such as
`integration_home_required` or `integration_invalid_path`. Consumers requiring
integration startup must reject a null root. Native executable-resolution errors
remain command errors.

## Read-only boundary

Doctor reads process environment and native executable identity. It does not read
provider data or Kitsune configuration, connect to sockets, inspect an active
receiver, create state directories, repair permissions, install hooks, or start
subprocesses. Successful inspection is **not live readiness**: a valid default
path may not exist or may later fail runtime privacy admission. No running Coven
or Kitsune session is required. Existing Kitsune routing variables do not select
the returned executable or root.

## Kitsune startup use

With a compatible Kitsune build, the normal opt-in is:

```toml
[integrations]
sources = ["coven"]
```

Unless an explicit runtime block overrides it, Kitsune finds the first executable
`coven` on absolute startup PATH entries and invokes `doctor status --json` once.
The negotiation has a three-second total deadline and a 16,384-byte stdout bound;
stdin is closed, stderr discarded and inherited `KITSUNE_*` variables stripped.
There is no shell expansion or fallback to a later installation after a selected
executable fails negotiation.

Kitsune requires this schema and both v1 capabilities, validates the canonical
native executable and absolute root, then freezes literal runtime/reporter argv.
The helper starts after Kitsune's API is ready. Selecting another Arcana source
or changing PATH does not redirect that server's route; restart the server to
select again. Replacing a pinned executable pathname remains outside this
capability handshake's guarantees.

Failed discovery leaves reporting unavailable and external ownership retained,
with an actionable server-log diagnostic; it never revives legacy agent handling.
The optional `[integrations.coven] auto_start = false` preserves presentation-only
source behavior, disabling the implicit runtime and inferred ownership. Explicit
runtime blocks still take precedence. These ownership choices are startup-only.
Provider configuration and native consent remain separate explicit operations.

See [normal workflow](NORMAL_AGENT_WORKFLOW.md) and
[advanced setup/undo](INTEGRATION_SETUP_CONTRACT.md).
