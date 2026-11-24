# Coven Web deployment state — recorded 2026-11-23

## Overview

This is a **dated snapshot of the latest recorded rollout**, not a live probe or
an automatically updated promise. Recheck private configuration, selected payloads,
backups and service state before operating. The
[runbook](COVEN_WEB_DEPLOYMENT.md) holds reusable procedures; the
[local-events record](../changelog/2026-11-23-coven-web-local-events.md) holds the
latest validation and source-state evidence. The earlier
[foreground-events record](../changelog/2026-11-23-coven-web-foreground-events.md)
retains the initial SSE rollback/canary corrections.

## API / Interfaces

### Selected gateway and retained helper

The gateway selects `~/.local/libexec/coven-web/ad1d49cedf6dbe68/bin/coven`, SHA256
`ad1d49cedf6dbe68e9d858e45f56b19376bd16c023dc5153e95c407275edfa51`.
Future dev Coven selection is `coven-native-zh3kmgjy`. Private `0600` provenance,
canary and rollout reports are retained beside the payload. Native x86-64 ABI,
default OpenSSL 3 loader, help and configuration validation passed.

Megatron's helper remains at `~/.local/libexec/coven-web/5e3b99d14e57a1b8/bin/coven`,
SHA256 `5e3b99d14e57a1b8f39c2e5a80840dab22ba0b1350061e5ffbdf2a53e325dc64`.
Local events and SSE change no helper DTO; no remote update is required. Earlier
remote validation is recorded in the
[complete-title release](../changelog/2026-11-21-coven-web-complete-titles.md), not
rerun against an offline host during this rollout.

The recorded private site is `https://bumblebee.tail97624b.ts.net`, served by
`coven-web.service` through private Serve to `127.0.0.1:7341`, with configuration
at `~/.config/coven-web/observer.json`. It is not public Funnel. Best-effort
terminal output remained disabled. Public launchers, fleet ACLs, Serve, push key
and the one phone subscription were preserved at the latest rollout.

### Retained native selection and configured policy

The 2026-11-21 [native publication record](../changelog/2026-11-21-coven-web-complete-titles.md)
selected managed UI bundle
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

The final candidate passed a push-disabled old/new metadata canary before activation.
Live Serve HTTPS health, assets and incremental SSE passed afterward: first `ready`
arrived in 6 ms, `metadata` in 2.315 seconds, and heartbeat in 10.007 seconds.
The local Kit accepted the exact twelve-selector subscription, and the gateway
retained a persistent Unix connection. No controlled native mutation was made to
measure event-triggered latency on the live host; the synthetic integration
establishes acceleration and snapshot authority.

Local fleet was reachable/observed with one agent. Megatron was offline in Tailscale
and timed out before SSH helper execution, as it did on the old build. The known
remote outage is not repaired or represented as healthy. Local metadata retained
one conversation from an observed source and partial attention with one complete
stream, zero items and one unavailable count. Its card remained unverified/null;
the snapshot did not invent a usable chat link. Final conversation/attention
freshness was positive and bounded by native evidence. Counts and coverage are
dynamic. Only the gateway restarted; no live message, decision or vendor test push
was sent. Existing background notification collection continues normally.

## Edge Cases

### Native activation limits

Running Djinn/Kit/native processes were not signaled. The older complete-title
record observed a running native formatter still emitting shortened titles;
discarded suffixes cannot be recovered by a website refresh. Finish active native
work before any deliberate updated local UI relaunch. Megatron's native UI was
**not updated**. SSE itself requires no native restart. Refreshing the phone loads
the new browser transport, not a new native worker.

OpenTUI 0.4.5's title transport remains roughly 1019 UTF-8 bytes. Coven forwarding
through 4096 UTF-8 bytes is not an end-to-end native title guarantee; no raw
OSC/private API workaround was introduced. Pending native waits belong to their
old process and cannot be adopted by a new UI. Local selected-root grants and
verified bindings are still necessary; remote native conversation/association
support, including Megatron, is not inferred from SSH fleet observations.

Physical-phone acceptance was not performed. Real Firefox EventSource against an
isolated actual gateway passed twice, alongside 223 observer, 43 observation-socket,
one new local-events integration, nine existing gateway and two package checks;
five opt-in producer tests were ignored. These overlapping focused checks do
not supersede earlier unrelated broad-library failures or claim full regression
or Clippy success. Local events accelerate acquisition, but three-second periodic
reconciliation remains for unsupported label/process changes and silent event loss.
Native grant/decision collection retains its separate periodic schedule. Lingering
remains off in the recorded deployment: logout, encrypted boot before login and
host-off availability are not guaranteed, and HA is out of scope.

### Rollback

For this gateway-only release, atomically restore the exact unit backup to the
unit path, reload the user manager and restart **only** `coven-web.service`:

- `~/.config/systemd/user/coven-web.service.local-events-rollback-20261123T191420Z-3cec49d2f347e811`

This restores the preceding SSE gateway `3cec49d2f347e811`. The remote helper stays
at `5e3b99d14e57a1b8`; its wire contract was unchanged by either release.
Observer configuration did not change and must not be replaced as part of this
rollback. If restoring future-dev selection too, use the installer's persistent
publication lock and expected-current selection check to select the retained
`coven-native-vfpkjm1j`; do not clobber a concurrent selection. Private provenance
records that previous selection. Keep all published generations intact.

Older paired gateway/helper rollbacks have different configuration requirements;
see the [complete-title record](../changelog/2026-11-21-coven-web-complete-titles.md)
and do not combine backup generations.

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
