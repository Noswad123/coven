# Retirement audit inventory and provenance repair — 2026-11-23

Restored the ordinary read-only retirement audit after three forms of committed
source drift. This is maintenance of an integrity gate, **not progress in original
assertion substitution or permission to remove Python**.

## Changes

- Registered 21 development-installer and 12 build-environment Python methods as
  explicit supplemental cohorts. Existing wrapper IDs remain unchanged. Current
  inventory: **941 originals + 38 supplemental = 979 methods**.
- Added exactly `unsupported_adapters_never_contact_or_create_workspace` to the
  existing interactive-session native inventory. Seven targets now list 55 IDs;
  this is static inventory, not a new native execution or mapping result.
- Reviewed the active runner's committed directory-isolation delta: 161 inserted
  and 32 deleted lines, five added functions and twenty changed functions. This
  changes configuration, generated text, preflight and launch behavior, not merely
  invocation spelling.
- Added a separate, hash-pinned active-oracle transition record and fail-closed
  validation for source path, prior/current/historical hashes, review integrity
  and parent acceptance after independent review. Dropping the active-oracle key
  while historical provenance exists no longer disables source checking.

The [accepted transition](../../tests/retirement/reviews/active_oracle_directory_isolation.json)
records verified Git commits/blobs and its limited integrity scope. It does not
claim current behavioral parity for every original test. The original fixture
captures still describe historical source; none were regenerated.

## Preserved obligations

All 44 original ledger entries/source pins, 80 review entries, 15 candidate mappings,
accepted batch/test-transition records and eight fixture/provenance files remain
unchanged. Original categories remain **53 A / 15 B / 0 C / 873 D**. Status remains
**67 mapped / 874 unresolved**. No supplemental ID was promoted into an original
mapping or category C.

Frozen expanded original-ID SHA256 remains:
`a48118da016436fea96b135e5e6a799eaab6d249f617a901da1a288b05f10b0a`.

Dynamic installer-module loading remains outside the static import closure and is
documented as manually reviewed. Stdlib source imports do not imply no filesystem,
process, signal or external-tool effects. The native inventory remains lexical,
not proof that no library/runtime dependency can execute Python.

## Validation

- `python3 -B -m unittest discover -s tools/tests -v`: **38 passed** against the
  actual approved manifest; no substituted approval status.
- `tools/retirement_coverage.py --check --summary`: passed.
- Two `--json` CLI reports were byte-identical.
- `--require-complete`: expected exit 1 solely for 874 unresolved obligations.
- Frozen source/fixture invariants and diff whitespace checks passed.

Tests reject missing/pending/tampered transition evidence, changed source,
supplemental renames/removal/assertion drift, cohort overlap and invented original
mappings. This run did not execute all 979 Python methods, the supplemental
installer tests, a Rust gate or a native application. No new all-suite, platform,
behavioral-equivalence or retirement-completion claim follows.

Owned disk-backed test artifacts were cleaned; no cleanup failure remained.
Existing uncommitted SSE/local-event work was preserved. No runtime source,
configuration, service, deployment, fixture capture, staging, commit or push was
changed by this repair.

See the [ledger guide](../../tests/retirement/README.md) and
[remaining retirement gates](../plans/RUST_RETIREMENT.md#original-obligation-ledger-not-a-missing-behavior-count).
