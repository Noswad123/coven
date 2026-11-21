# Coven documentation

Start with the task you need to perform. Current guides, architecture contracts
and operations procedures are separate from plans and historical evidence.
Moving a document does not qualify a platform, activate an integration, or approve
an operation; retain each document's stated scope and limits.

## Find what you need

| Audience / task | Start here |
| --- | --- |
| Users: use the browser client, configure normal reporting or isolate working directories | [Guides](guides/index.md) |
| Implementers and reviewers: understand interfaces, ownership and safety boundaries | [Architecture](architecture/index.md) |
| Operators: build and operate documented integrations | [Operations](operations/index.md) |
| Contributors: find proposed work, migration gates and remaining decisions | [Plans](plans/index.md) |
| Reviewers: trace dated implementation, characterization and validation evidence | [Changelog](changelog/index.md) |

## Where to start

Use a guide or operations procedure for a task, consult its architecture contract
for exact behavior, and use plans/changelog records for rationale and evidence.
Current contracts are authoritative for current behavior; historical migration
notes do not override them.

## Documentation conventions

- Give each topic one authoritative home. Link to that home instead of duplicating
  its instructions or maintaining competing contracts.
- Keep changes and validation records dated, newest first. Preserve the scope,
  platform, source revision and limitations of earlier evidence; do not relabel it
  as fresh qualification.
- Plans and history are not standing instructions. Historical approvals do not
  authorize new builds, installation, configuration changes, provider execution,
  staging, commits, releases or deployment.
- Keep only this index directly under `docs/`. Documents live in their category
  folders; links point directly to their authoritative destinations, not forwarding
  pages. When moving a section, update its incoming links and fragments together.
- Commands, paths and headings in retained historical content preserve their
  original context; they are not automatically current operating instructions.

## Suggested first reads

For local workspace coordination, start with
[workspace basics](guides/WORKSPACE_BASICS.md), then
[task workflows](guides/TASK_WORKFLOWS.md) and
[preparation workflows](guides/PREPARATION_WORKFLOWS.md).

For ordinary native-agent invocation, begin with
[the normal workflow](guides/NORMAL_AGENT_WORKFLOW.md), then consult
[doctor compatibility](architecture/DOCTOR_STATUS_CONTRACT.md) if startup
negotiation needs explanation. To review migration progress instead, use
[plans](plans/index.md) and the [adoption record](changelog/FEATURE_ADOPTION_STATUS.md).

For phone-based monitoring, start with the [Coven Web guide](guides/COVEN_WEB.md).
For deployment and recovery, use the [operations index](operations/index.md).

## Project entry point

- [Root README](../README.md): concise project overview, installation entry points
  and a local-files-only quick start.
- Each category index links to its canonical documents and their stated limits.
