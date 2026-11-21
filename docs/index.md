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
- Legacy files directly under `docs/` are forwarding pages that preserve existing
  README and handoff links. Prefer canonical category paths in new documentation.
- Commands, paths and headings in retained historical content preserve their
  original context; they are not automatically current operating instructions.

## Suggested first reads

For ordinary native-agent invocation, begin with
[the normal workflow](guides/NORMAL_AGENT_WORKFLOW.md), then consult
[doctor compatibility](architecture/DOCTOR_STATUS_CONTRACT.md) if startup
negotiation needs explanation. To review migration progress instead, use
[plans](plans/index.md) and the [adoption record](changelog/FEATURE_ADOPTION_STATUS.md).

For phone-based monitoring, start with the [Coven Web guide](guides/COVEN_WEB.md).
For deployment and recovery, use the [operations index](operations/index.md).

## Project entry point

- [Root README](../README.md): unchanged project entry point; legacy documentation
  links are supported by forwarding pages.
- Each category index links to its canonical documents and their stated limits.
