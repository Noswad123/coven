# Guides

[Documentation index](../index.md)

## Local workspace coordination

- [Workspace basics](WORKSPACE_BASICS.md) — initialize safely, understand generated
  files, exchange local messages and preserve edits during refresh.
- [Task workflows](TASK_WORKFLOWS.md) — default snapshot changes, optional ledgers,
  conditional mutations and explicit recovery.
- [Preparation workflows](PREPARATION_WORKFLOWS.md) — standalone task attempts and
  reviewed graph loops without worker dispatch.
- [Runtime workflows](RUNTIME_WORKFLOWS.md) — checkpoint decisions, legacy lifecycle,
  reviewed cold startup and lead configuration, with explicit effects.

For worker results, the [handoff contract](../architecture/HANDOFF_CONTRACT.md)
explains validation and collection of explicitly unverified claims; collection
does not approve the work or change task status.

## Native agents, observation and browser access

- [Coven Web](COVEN_WEB.md) — browser cards, bound conversations, follow-ups,
  requests, alerts and stale/uncertain states.
- [Normal agent workflow](NORMAL_AGENT_WORKFLOW.md) — opt-in native-agent invocation
  and integration handoffs without changing ordinary provider commands.
- [Agent directory isolation](AGENT_DIRECTORY_ISOLATION.md) — explicit legacy-team
  working-directory mappings and their limits.
- [Session workflows](SESSION_WORKFLOWS.md) — bounded session readers, separate
  resume planners and explicit native start/resume or prompt admission.
- [Observation and groups](OBSERVATION_AND_GROUPS.md) — persisted/live evidence,
  process discovery, explicit participant groups and optional presentation.

See [architecture](../architecture/index.md) for contracts,
[operations](../operations/index.md) for deployment, [plans](../plans/index.md)
for outstanding work, and [changelog](../changelog/index.md) for dated evidence.
