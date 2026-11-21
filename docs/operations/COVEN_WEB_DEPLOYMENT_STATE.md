# Coven Web deployment state — recorded 2026-11-21

## Overview

This is a **dated extraction of the latest recorded rollout**, not a live probe or
an automatically updated promise. Recheck private configuration, selected payloads,
backups and service state before operating. No deployment was performed for this
documentation split. The [runbook](COVEN_WEB_DEPLOYMENT.md) holds reusable procedures;
the [complete-title record](../changelog/2026-11-21-coven-web-complete-titles.md)
holds full provenance, validation scope and historical source-state evidence.

## API / Interfaces

### Selected gateway and helper

Gateway and matching Megatron helper were selected at
`~/.local/libexec/coven-web/5e3b99d14e57a1b8/bin/coven`, SHA256
`5e3b99d14e57a1b8f39c2e5a80840dab22ba0b1350061e5ffbdf2a53e325dc64`.
Future dev Coven selection was `coven-native-rr9vz35i`. Private provenance and
rollout reports were retained beside the payload. Remote hash, x86-64 ABI,
default OpenSSL 3 loader, help and bounded-inventory checks passed.

The recorded private site is `https://bumblebee.tail97624b.ts.net`, served by
`coven-web.service` through private Serve to `127.0.0.1:7341`, with configuration
at `~/.config/coven-web/observer.json`. It is not public Funnel. Best-effort
terminal output remained disabled. Public launchers, fleet ACLs, Serve, push key
and the one phone subscription were preserved at the latest rollout.

### Native selection and configured policy

Local future Djinn launches selected managed UI bundle
`cfc7e72931097d77b6ab5e3339247e084a6116b78260173e372a1cb5bb8e75af`
under `~/.local/libexec/arcana-dev/djinn/bin/.djinn/`. Native CLI SHA256 remained
`4d7f8f1ebd49cab7f16eb145185eda18d9d53281521060856ac4c560ee9c98b4`.
Named sessions use the supported renderer API without the old prefix or formatter.

The retained setup from [normal-launch sharing, 2026-11-19](../changelog/2026-11-19-coven-web-sharing-follow-ups.md#installed-setup)
and [attention activation, 2026-11-20](../changelog/2026-11-20-coven-web-attention-alerts.md#deployment-and-activation-boundary)
records:

- `~/.config/djinn/coven-web.json`: owned/0600, `read_write`, registry
  `/run/user/1000/coven-web` (0700), dotfile-managed parent preserved, and
  `decision_control:true` captured at native worker startup.
- One `local-djinn` source, team `development`, with the sole owner explicitly
  authorized separately as reader, submitter and decision responder. Control
  workspace `~/.local/state/coven-web/control` (0700), initialized blank without
  launching agents/providers/tmux; native working directories remain separate.
- Push state at `~/.local/state/coven-web/push` (0700), with owned regular
  single-link private key/state/lock files (0600). Browser opt-in is separate.

These are retained configuration records, not evidence of a currently selected
native grant, verified chat link, decision availability or vendor delivery.

## Data Flow

The latest candidate and live loopback/Serve HTTPS health, assets and metadata
passed. Both fleet endpoints were reachable/observed; the final sample had one
local manually named agent and zero remote agents. Counts are dynamic, not coverage
guarantees. Only the gateway restarted; no live message, decision or push was sent.

## Edge Cases

### Native activation limits

Running Djinn/Kit/native processes were not signaled. The active local metadata
still emitted the old 45-byte decorated/truncated title; the website cannot recover
discarded suffixes. Finish active native work before a deliberate updated local UI
relaunch. Megatron's native UI was **not updated** and needs its normal separate
native update. Refreshing the phone changes browser assets, not native workers.

OpenTUI 0.4.5's title transport remains roughly 1019 UTF-8 bytes. Coven forwarding
through 4096 UTF-8 bytes is not an end-to-end native title guarantee; no raw
OSC/private API workaround was introduced. Pending native waits belong to their
old process and cannot be adopted by a new UI. Local selected-root grants and
verified bindings are still necessary; remote native conversation/association
support, including Megatron, is not inferred from SSH fleet observations.

Physical-phone/interactive-native title acceptance was not performed. The focused
passing checks in the latest record are not a passing full regression gate; its
two named earlier unrelated library failures remain outside that scope. Lingering
remains off in the recorded deployment: logout, encrypted boot before login and
host-off availability are not guaranteed, and HA is out of scope.

### Rollback

For the latest paired gateway/helper rollback, atomically restore **both** exact
private backups to their original unit/config paths before reloading the user
manager and restarting **only** `coven-web.service`:

- `~/.config/systemd/user/coven-web.service.full-title-rollback-20261121T092445-5e3b99d14e57a1b8`
- `~/.config/coven-web/observer.json.full-title-rollback-20261121T092445-5e3b99d14e57a1b8`

They restore paired gateway/helper pin `75bb7f3a5b8b0cc5`; older payloads were
retained. The prior payload's full hash and earlier rollback chain remain in
[pane hierarchy, 2026-11-20](../changelog/2026-11-20-coven-web-pane-hierarchy.md).
Do not restore just one side of an incompatible gateway/helper pair.

The preceding matched native bundle
`08a998a3848af9d86eb4882604969787ff5bea49826d3ec4b66e2f19c1f9d106`
was retained for atomic `.djinn/current` selection rollback. Native startup-policy
rollback is separate: an attention-capability rollback also requires restoring
its startup config, with the pre-attention backups documented in the
[attention record](../changelog/2026-11-20-coven-web-attention-alerts.md#deployment-and-activation-boundary).
Never reset unrelated Serve routes or implicitly restart active native agents.
Withdraw only this application's exclusively owned HTTPS listener if exposure is
being removed; see the [runbook](COVEN_WEB_DEPLOYMENT.md#rollback-and-availability).

## Example usage

Before an approved operation, compare read-only service/Serve status with this
dated record. Do not treat an older record's “current” heading or its rollback pin
as the latest selection. Reconfirm backup existence and contents before restoring;
this document does not execute or authorize a rollback.

## Related files

- [Operations index](index.md), [runbook](COVEN_WEB_DEPLOYMENT.md),
  [contract](../architecture/COVEN_WEB_CONTRACT.md), [guide](../guides/COVEN_WEB.md),
  [recovery](../plans/COVEN_WEB_RECOVERY.md), [all rollout records](../changelog/index.md).
