# Plans and migration records

These documents describe direction, remaining decisions and migration gates.
Migration records also carry historical implementation and validation evidence,
not timeless authorization or a reliable summary of all current behavior. Current
[architecture contracts](../architecture/index.md) are authoritative for behavior.
Historical approvals are not standing instructions to execute work.

## Roadmaps, migrations and recovery

- [Agent-management migration](AGENT_MANAGEMENT_MIGRATION.md): ownership matrix,
  transfer sequence, platform gates and historical provider-cut evidence.
- [Coding orchestration roadmap](CODING_ORCHESTRATION_ROADMAP.md): coding workflow
  priorities, worker/composer requirements and implementation milestones.
- [Kitsune retirement feature decisions](KITSUNE_RETIREMENT_FEATURE_DECISIONS.md):
  residual adoption work, accepted limitations, exclusions and deferred decisions.
- [Product roadmap](ROADMAP.md): orchestration direction, product stories and
  proposed worker, lifecycle, synthesis and presentation slices.
- [Rust migration](RUST_MIGRATION.md): native cutover implementation record,
  module map, compatibility limits and historical acceptance evidence.
- [Rust retirement](RUST_RETIREMENT.md): development-oracle cleanup, original
  obligations and separate compatibility, platform and distribution gates.
- [Coven Web recovery obligations](COVEN_WEB_RECOVERY.md): exact-object source
  recovery checks, snapshot retention and ownership-gated artifact cleanup.

## How to use these plans

Read the current contract first, use the relevant plan to identify remaining work,
and consult the [changelog](../changelog/index.md) for recorded evidence. Narrow or
remove completed backlog entries rather than treating every historical gap as open.

## Scope and status

- An implemented milestone is not installed activation or all-platform qualification.
- Source characterization, scoped test results and native runtime qualification are
  different evidence; do not combine overlapping historical totals.
- Deferred work and previously approved operations require their current scope and
  authorization to be checked anew. No plan grants automatic execution permission.

## Suggested reading order

For agent ownership work, compare the
[migration matrix](AGENT_MANAGEMENT_MIGRATION.md) with
[app boundaries](../architecture/APP_BOUNDARIES.md) and the
[adoption record](../changelog/FEATURE_ADOPTION_STATUS.md). Use the residual feature
decisions to distinguish missing work from explicitly deferred or excluded work.

## Related documentation

- [Documentation entry point](../index.md)
- [Architecture contracts](../architecture/index.md)
- [Historical changes and validation](../changelog/index.md)
