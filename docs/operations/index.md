# Operations

[Documentation index](../index.md)

## Build and integration setup

- [Native packaging](../../packaging/README.md) — development selection, strict
  prebuilt installation, asset resolution and release qualifications.
- [Development checks](DEVELOPMENT_CHECKS.md) — Rust, Python and web recipes,
  build prerequisites, offline caches and disk-backed artifact cleanup.
- [Fixed-endpoint reporting](FIXED_ENDPOINT_REPORTING.md) — advanced manual receiver
  and pinned-provider setup; use the normal agent guide for ordinary invocation.
- [Development OpenSSL selection](WEB_PUSH_BUILD.md) — installer selection rules
  and override/loader boundaries for Web Push builds.

## Coven Web deployment and recovery

- [Coven Web deployment runbook](COVEN_WEB_DEPLOYMENT.md) — reusable configuration,
  scoped build checks, private Serve, rollout and rollback procedures.
- [Coven Web deployment state — 2026-11-21](COVEN_WEB_DEPLOYMENT_STATE.md) — latest
  recorded gateway/helper/native selections, exact rollback backups and activation
  limits; a dated record, not an automatically updated live-state promise.

See [guides](../guides/index.md) for browser workflows,
[architecture](../architecture/index.md) for API/security contracts,
[recovery obligations](../plans/COVEN_WEB_RECOVERY.md) for source/artifact retention,
and [changelog](../changelog/index.md) for historical rollout evidence.
