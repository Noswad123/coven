# coven

Coven is a personal, magic-themed command-line tool for coordinating local
multi-agent workspaces. Goals, tasks, messages, checkpoints and worker handoffs
live in inspectable local files.

It also provides opt-in native-agent reporting, bounded session observation and
a phone-first browser client. Coven owns coordination and observation; native
providers own their execution. Kitsune supplies optional generic terminal and
presentation interfaces. See the [app boundaries](docs/architecture/APP_BOUNDARIES.md).

**[Documentation index](docs/index.md)**
[Normal agent workflow](docs/guides/NORMAL_AGENT_WORKFLOW.md)
[Coven Web](docs/guides/COVEN_WEB.md)
[Plans](docs/plans/index.md)

## Choose your workflow

| I want to… | Start here |
| --- | --- |
| Invoke native agents normally with opt-in Coven reporting | [Normal agent workflow](docs/guides/NORMAL_AGENT_WORKFLOW.md) |
| Monitor sessions, open verified chats and enable optional alerts on my phone | [Coven Web guide](docs/guides/COVEN_WEB.md) |
| Create a local coordination workspace | [Workspace basics](docs/guides/WORKSPACE_BASICS.md) |
| Prepare reviewed work without dispatching it | [preparation workflows](docs/guides/PREPARATION_WORKFLOWS.md) |
| Manage tasks | [Task workflows](docs/guides/TASK_WORKFLOWS.md) |
| Inspect sessions | [Session workflows](docs/guides/SESSION_WORKFLOWS.md)|
| group participants | [observation and groups](docs/guides/OBSERVATION_AND_GROUPS.md) |
| use explicit runtime controls | [runtime workflows](docs/guides/RUNTIME_WORKFLOWS.md) |
| Build, deploy or recover an installation | [Operations](docs/operations/index.md) |
| Understand interfaces and safety guarantees | [Architecture](docs/architecture/index.md) |
| See what changed or what remains planned | [Changelog](docs/changelog/index.md) and [plans](docs/plans/index.md) |

## Install

The product is the native Rust `coven` binary. Python is retained for development
and compatibility checks, not as a runtime fallback.

From a development checkout, `just install` builds a locked release and atomically
selects a new immutable development payload. It retains earlier payloads and does
not replace public launchers, change provider configuration or restart agents.
Build prerequisites, selection paths and overrides are documented in
[native packaging](packaging/README.md#install-the-development-checkout).

For a trusted prebuilt binary, use the separate
[staged installation procedure](packaging/README.md#install-a-prebuilt-binary).
It requires a new prefix and does not perform in-place upgrades. Platform and
external-release qualifications are documented separately; installation is not
proof of native provider activation.

## Quick start: local files only

With a native `coven` selected on your PATH, choose a fresh directory:

```sh
coven init ./my-coven --blank --agents operator,reviewer
coven goal ./my-coven "Review parser edge cases"
coven message ./my-coven operator reviewer "Please review the parser"
coven task list ./my-coven --json
coven status ./my-coven
```

These commands create and update local workspace files; they do not launch agents
or notify a running provider. `status` writes the dashboard—it is not a read-only
probe. Init refuses reserved existing files; `--force` does not permit overwrite.
See [workspace basics](docs/guides/WORKSPACE_BASICS.md) for examples, generated layout,
refresh preservation and partial-failure handling.

Normal native-agent reporting does not require this workspace-creation workflow.
Use the [normal agent guide](docs/guides/NORMAL_AGENT_WORKFLOW.md) for that setup.

## Important boundaries

- Display labels, process matches and observation rows are not native identity,
  execution permission, readiness or completion proof.
- Task preparation and graph approval do not dispatch workers. Explicit runtime
  commands can launch unrestricted tools; review their documented effects first.
- Preview, read, configuration mutation and execution are distinct operations.
  Uncertain delivery is not permission to retry automatically.
- Provider setup, sharing and alerts are opt-in. Platform support, source
  qualification and installed activation have separate limits; consult each
  feature's guide and contract rather than assuming universal support.

## Repository layout

```text
src/        Native command implementation
clients/    Browser client
docs/       Guides, architecture, operations, plans and changelog
examples/   Workspace examples and assets
packaging/  Installation and packaging instructions
tests/      Native, compatibility and client-support fixtures
tools/      Development and qualification harnesses
runner/     Retained Python development oracle
```

## Development

Use [development checks](docs/operations/DEVELOPMENT_CHECKS.md) for Rust, Python and
web test recipes, toolchain prerequisites, offline builds and scoped artifact
cleanup. The [plans index](docs/plans/index.md) separates product direction from
migration and retirement gates. Historical test results live in the
[changelog](docs/changelog/index.md), not in this project overview.

## License

[MIT](LICENSE).
