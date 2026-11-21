# Coven Web: Coven's browser client

Coven Web is Coven's phone-first browser client, not a separate product, agent
manager or agent runner. This forwarding page retains the root README's entry point.

- [Browser behavior and workflows](guides/COVEN_WEB.md)
- [Public API, identity, security, resource limits and lifecycle](architecture/COVEN_WEB_CONTRACT.md)
- [Configuration, build, private Serve, rollout and rollback](operations/COVEN_WEB_DEPLOYMENT.md)
- [Deployment state recorded 2026-11-21](operations/COVEN_WEB_DEPLOYMENT_STATE.md) (not automatically updated)
- [Unresolved source/stash/artifact recovery obligations](plans/COVEN_WEB_RECOVERY.md)
- [Chronological change records](changelog/index.md) and [old handoff navigation](COVEN_WEB_HANDOFF.md)

## First slice and protected behavior

See [the guide](guides/COVEN_WEB.md#first-slice-and-protected-behavior).

## Public JSON contract

See [the API contract](architecture/COVEN_WEB_CONTRACT.md#public-json-contract).

## Configuration and CLI

See [the runbook](operations/COVEN_WEB_DEPLOYMENT.md#configuration-and-cli) and
[native browser behavior](guides/COVEN_WEB.md#native-conversations-new-builds-only).

### Native conversations (new builds only)

See [browser behavior](guides/COVEN_WEB.md#native-conversations-new-builds-only),
[registration](operations/COVEN_WEB_DEPLOYMENT.md#native-conversation-registration), and
[native read contract](architecture/COVEN_WEB_CONTRACT.md#native-conversation-api).

### Shared Follow-Ups

See [API](architecture/COVEN_WEB_CONTRACT.md#shared-follow-ups) and
[delivery workflow](guides/COVEN_WEB.md#shared-follow-ups).

### Actionable Attention

See [API](architecture/COVEN_WEB_CONTRACT.md#actionable-attention) and
[browser workflow](guides/COVEN_WEB.md#actionable-attention).

### Opt-In Background Alerts

See [API](architecture/COVEN_WEB_CONTRACT.md#opt-in-background-alerts) and
[browser workflow](guides/COVEN_WEB.md#opt-in-background-alerts).

### Optional cross-language acceptance

See [the runbook](operations/COVEN_WEB_DEPLOYMENT.md#optional-cross-language-acceptance).

## Authentication and deployment boundary

See [trust boundary](architecture/COVEN_WEB_CONTRACT.md#authentication-and-deployment-boundary)
and [rollout](operations/COVEN_WEB_DEPLOYMENT.md#rollout).

## HTTP lifecycle and development checks

See [HTTP lifecycle](architecture/COVEN_WEB_CONTRACT.md#http-lifecycle),
[development checks](operations/COVEN_WEB_DEPLOYMENT.md#development-checks), and
[historical initial results](changelog/coven-web-initial-prototype.md#initial-development-check-results).

## Private installation and operation

See [the runbook](operations/COVEN_WEB_DEPLOYMENT.md#private-installation-and-operation).
