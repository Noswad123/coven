# README 27cc6ae Evidence Provenance

This is a source-location map, not a relocated README or a new validation report.
The extraction source designated for this record is `README.md` at
`27cc6ae7a28d7ba114736e2cc036aebe6b408659` (2,148 lines). The full 2,148-line
checkout README was read before edits. Its byte-for-byte equality to that revision
was verified before replacement with `git diff --exit-code HEAD -- README.md`;
the original Git blob is `9d20e7bda2630c6c242121b582226d44a43d6262`.

Original line numbers below refer to that extraction text, never the shortened
README. Dates for individual old runs are unspecified unless their canonical
record establishes them. The revision is not a test date or release qualification.

Comparison found the requested validation totals, source pins and important
failure/fixture limits already preserved. The missing material retained here is
their exact README source-location/disposition map, rather than duplicate totals.

## Current documentation

For current contributor commands use [Development Checks](../operations/DEVELOPMENT_CHECKS.md).
For install/upgrade/staging use [Native Packaging](../../packaging/README.md), with
the preserved [prebuilt installer anchor](../../packaging/README.md#install-a-prebuilt-binary).
Canonical contracts and plans remain the detailed evidence owners.

## Historical evidence destinations

| Original README lines | Preservation disposition / canonical location |
| --- | --- |
| 207–227 | Historical discovery/state seam, confirmation reruns, ENOSPC/zero-match limits and Apple source-check qualification: [agent migration, state seam](../plans/AGENT_MANAGEMENT_MIGRATION.md#state-seam-validation-table), its validation limits and discovery table. No duplicate test total extracted. |
| 247–264 | Earlier backend scope vs later private-storage scope, incidental library failures, isolated/serial reruns and unestablished causes: [backend validation](../architecture/HOOK_FILE_BACKEND_CONTRACT.md#validation-gates), [private-storage validation](../architecture/AGENT_PRIVATE_STORAGE_CONTRACT.md#validation-and-gates) and agent migration's private-storage section. |
| 350–363 | Reference-only Pi migration and historical focused/generated-asset evidence: [Pi validation table](../plans/AGENT_MANAGEMENT_MIGRATION.md#pi-validation-table) and [Pi contract](../architecture/PI_EXTENSION_CONTRACT.md). Native activation and receipt-timing limits stay qualified. |
| 376–405 | OpenCode release pin, frozen Pi compatibility, scoped Rust/producer/Kit checks and overlapping counts: [OpenCode contract](../architecture/OPENCODE_EXTENSION_CONTRACT.md) and [validation table](../plans/AGENT_MANAGEMENT_MIGRATION.md#opencode-validation-table). Pins remain there, not copied here. |
| 431–435 | Earlier owner-reported Codex workflow/fault evidence and incomplete-transfer claims: [extraction characterization](KITSUNE_EXTRACTION_CHARACTERIZATION.md) and agent migration. Historical only; see retirement conflict below. |
| 437–460, 469–566 | Native cutover, execution/local-workspace behavior and limits: [Rust migration](../plans/RUST_MIGRATION.md), including its authoring, maintenance and local-workspace contracts. No competing install guide extracted. |
| 461–468 | Bounded Group/Observe gate and Session route representation: [worktree reconciliation](../plans/RUST_MIGRATION.md#worktree-reconciliation-validation), [Session completion](../plans/RUST_MIGRATION.md#native-session-completion), [retirement gates](../plans/RUST_RETIREMENT.md#api--interfaces). Representation is not parity. |
| 567–583 | Reviewed-runtime full gate and maintenance/inspection/checkpoint/local-workspace acceptance layers: [Rust migration validation](../plans/RUST_MIGRATION.md#validation) and [Rust retirement](../plans/RUST_RETIREMENT.md). Nested/helper and internal-case counts are not extra native passes. |
| 585–600 | Prior full migration vs final split acceptance, Python/tool timings, ignored-helper and retry qualifications: [reconciliation](../plans/RUST_MIGRATION.md#worktree-reconciliation-validation) and [final acceptance](../plans/RUST_MIGRATION.md#final-local-acceptance). Not one fresh all-command pass. |
| 602–608 | Authoring CLI/race cases, parent-only Python oracle, two-node/not-maximum-size limits: migration validation and [retirement workflow gates](../plans/RUST_RETIREMENT.md#api--interfaces). |
| 610–635 | Native product vs retained Python oracle, frozen original-ID audit and route progression: [retirement ledger](../plans/RUST_RETIREMENT.md#original-obligation-ledger-not-a-missing-behavior-count) and migration coverage. Merge hold is historical, not current policy. |
| 639–651 | Observation route progression and compiled-producer CLI/targeted layers: [Rust retirement](../plans/RUST_RETIREMENT.md#api--interfaces) and migration validation. Layers are non-additive and Linux-qualified. |
| 745–755 | Legacy lifecycle aggregate, compiled-backend/oracle/TTY layers, real long child and generated-shell sleeps: [historical lifecycle acceptance](../plans/RUST_RETIREMENT.md#historical-legacy-lifecycle-acceptance) and migration validation. |
| 819–823 | Independent reader/prompt/start-resume/parser fixture layers: [Session completion](../plans/RUST_MIGRATION.md#native-session-completion) and retirement Data Flow. Not additions to full totals. |
| 825–924 | Checkout installation, immutable generations, strict prebuilt staging and native launcher: [packaging](../../packaging/README.md). Symlink wording reconciled with actual launcher; no duplicate installer instructions. |
| 926–935 | Public callback precedence, ignored variables, quoting/no probing and non-UTF-8 errors: packaging's [callback section](../../packaging/README.md#public-callback-compatibility), checked against `src/workspace_assets.rs` and its callers. |
| 938–957 | Scoped entrypoint/package/install evidence, split acceptance, unpromoted formula and external gates: packaging checklist, Rust migration final acceptance and Rust retirement. Original no-commit/no-merge wording is historical. |
| 1213–1228 | Group-live offline milestone, focused producer/schema/typecheck limits and separate installed-Linux live runs: [Groups validation](../architecture/GROUPS_CONTRACT.md#validation), [live observer](../architecture/DJINN_LIVE_OBSERVATION_CONTRACT.md#example-usage) and [live group record](GROUPS_LIVE_VALIDATION.md). |
| 1285–1288 | Private artifacts/fake endpoints for attachment/status/live publisher, no existing user group/runtime mutation: [live-status validation](../architecture/GROUP_LIVE_STATUS_CONTRACT.md#validation) and Groups validation. Not installed-native proof. |
| 2088–2093 | Reviewed-runtime compiled fixtures, FD sentinels, real delays, no providers/tmux children and non-universal crash limits: Rust retirement's reviewed-runtime acceptance and migration validation. |
| 2124–2145 | Old development recipes: replaced by the current justfile-based Development Checks guide, with Rust/Python/Node, separate browser acceptance, offline prerequisites and scoped artifact policy. Old Python minimum does not establish full-oracle qualification. |

## Superseded claims and evidence limits

### Historical retirement is not current status

The original discovery seam and hook/extension sections describe temporary
duplicate legacy policy, Linux-only installer retirements and retained controls.
Those are historical cuts, not evidence that current Kit still runs that policy.
The [feature-adoption global retirement record](FEATURE_ADOPTION_STATUS.md#global-retirement-record)
explicitly records the 2026-11-16 global retirement of live detection/control APIs,
provider autoresume and fresh legacy installers across platforms. Remaining static
compatibility, cleanup assets and historical references do not restore live control.
Neither the old scoped counts nor that retirement record certifies native macOS.

### Merge and publication statements are time-scoped

Original lines 634–635 and 938–957 retain the earlier no-merge/no-commit posture.
The [Rust retirement integration update](../plans/RUST_RETIREMENT.md#overview)
records later authorization of a local merge; Rust migration records integrated
validation separately. Do not repeat the earlier no-commit statement as a current
repository fact. Neither record authorizes a new commit, push, release or external
tap promotion during this extraction.

### Evidence must keep its layer

- Focused passes do not erase an incidental broader failure; successful isolated
  reruns do not establish a root cause. Keep the canonical failure/rerun records.
- Source pins characterize supported source, not installed-native activation.
  The exact OpenCode release and frozen Pi pin remain in their contracts.
- Route representation, assertion-level oracle cleanup and release/platform
  qualification are distinct gates; do not derive a parity percentage from routes.
- Offline private/fake endpoints and historical valid real-server snapshots are
  different evidence. The latter do not prove malformed-legacy payload behavior.
- Old saved-output paths are historical references, not a promise that artifacts
  remain available or permission to recreate bulk data in shared temporary storage.

## How to trace a claim

For a historical claim, find its original range above, then read the canonical
record's result **and** limits. For example, the source's 585–600 split-acceptance
discussion belongs to Rust migration's final-local-acceptance record; it is not
today's `just check` result. No test execution is needed to use this map.

## Related records

- [Rust migration](../plans/RUST_MIGRATION.md) and [retirement](../plans/RUST_RETIREMENT.md).
- [Agent-management migration](../plans/AGENT_MANAGEMENT_MIGRATION.md).
- [Feature adoption / global retirement](FEATURE_ADOPTION_STATUS.md).
- [Native packaging](../../packaging/README.md) and [Development Checks](../operations/DEVELOPMENT_CHECKS.md).

This documentation extraction ran no tests, builds, live endpoint calls or
installation. It adds provenance and source-consistent guidance, not new evidence
of a successful check or product/platform qualification.
