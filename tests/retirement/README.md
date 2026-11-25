# Retirement Coverage Ledger

Tracks the frozen original obligations and accepted Session views, records,
checkpoints, task graphs, session prompts, launch, groups and handoff-schema batches.
It also inventories later
supplemental tests and the reviewed active-oracle source. It does not change the
production entrypoint, remove or move runner code, skip existing tests, introduce
test features, or replace migration/release validation. No native-suite coverage
totals or fresh Git-operation authorization are inferred from this ledger.

## Commands

Run from the migration repository, with bytecode disabled:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B tools/retirement_coverage.py --check --summary
PYTHONDONTWRITEBYTECODE=1 python3 -B tools/retirement_coverage.py --json
PYTHONDONTWRITEBYTECODE=1 python3 -B -m unittest discover -s tools/tests -v
PYTHONDONTWRITEBYTECODE=1 python3 -B tools/retirement_coverage.py --require-complete
```

The last command MUST currently exit nonzero. A successful `--check` validates
the inventory, not migration completion. `--json` emits deterministic expanded
method obligations, file counts, direct and transitive imports, and the static
native integration inventory. All commands validate; `--check` is an explicit
spelling for automation, not a weaker inspection mode. The audit is read-only,
stdlib-only and AST-based for Python. It never imports or executes runner code.
Audit unit tests live outside `tests/`. The current AST inventory has **979
methods**: 941 frozen originals plus **38 supplemental methods in three files**
(five wrapper, 21 development-installer and 12 build-environment tests). Inventory
is not execution: the latest batch ran the 40 audit-tool tests, not all 979 methods.

## Frozen Baseline

`coverage.json` stores all original file/class/method names once, grouped by file
and class, with per-file source SHA256, category counts and declarative overrides.
Stable expanded IDs are:

```text
python::tests/test_session_prompts.py::SessionPromptTests::test_capabilities_query_is_metadata_only
rust::tests/session_prompts_workflow.rs::capabilities_are_workspace_independent
```

There are 941 original methods in 44 files: **62 A, 29 B, 0 C, 850 D**.
**90 methods are mapped and 851 methods remain unresolved**. All 15 B prompt
entries have explicit candidate mappings and accepted assertion reviews, including
the original four B entries. Session views' final purity obligation remains D and
unresolved. Mapped means an explicit assertion review with native-test evidence
and, for A, frozen fixtures. Parent and independent review accepted the recorded
batch fragments; this is not global migration or release acceptance. The original
IDs/source pins stay frozen; classifications change only through reviewed batches.
D means no complete substitution has been established, not proof that native
behavior is missing. All original Python tests remain active.

- A proposes frozen inputs/outputs at a known data or codec boundary. Native
  tests must retain mutation independence and every original limit/subcase too.
- B proposes an existing native assertion substitute. Review every original
  assertion and setup/no-effect guard before marking it mapped.
- C contains five independent stdlib Python helper files, separately listed.
  They are development tools, excluded from the candidate native product gate.
- D needs native assertions or a reviewed equivalence mapping. Fault ordering,
  concurrency, purity, package isolation and Python-specific API tests remain
  explicit obligations; they are not silently discarded.

The validator checks current AST-discovered IDs against the disjoint union of
the frozen original list and the explicit supplemental cohort, and
hashes the complete original files. Thus removing assertions without changing a
method name also fails. It validates selector references/overlaps, counts,
candidate native test names, review IDs and all fixture references/digests.
Do not regenerate this baseline to make a failure green. Review a source change
and its coverage disposition, preserve original IDs, and explicitly update only
the corresponding reviewed data. No automatic baseline-update command exists.

Global `reviews` entries are keyed by original ID. `status: "mapped"` requires
nonempty `assertion_review`, `native_tests` and, for A, `fixtures` entries of the
form `{"path":"repository/relative/file","sha256":"..."}`. A missing fixture
fails even while its review is unresolved. D cannot be marked mapped without
first reviewing/reclassifying it. `--require-complete` rejects unresolved A/B/D.
Text claiming an assertion review is not itself proof; human review remains
necessary. The tool cannot establish behavioral equivalence.

Import closure is conservatively file-scoped: imports in setup, methods, helper
modules and conditional branches all apply to every method in that file. It
follows runner package and legacy direct-module aliases, local test helpers,
literal dynamic imports and parseable literal child Python snippets, including
cycles. Nonliteral dynamic imports are reported. It is not a general Python
execution analyzer; generated import names, runtime path manipulation and
unparseable code fragments need manual review. C files fail if their closure
reaches runner, third-party imports or unresolved dynamic import calls.

## Candidate Native Inventory

Seven exact integration targets are whitelisted with **55 current test IDs**.
The previously accepted prompt workflow inventory contains 19, including these
four additions from its earlier 15-ID cohort:

- `all_original_malformed_ack_cases_are_uncertain_without_retry`
- `leaf_socket_replacement_after_capabilities_contacts_neither_submit_endpoint`
- `original_invalid_argument_cases_refuse_without_endpoint_contact`
- `literal_receipt_contract_keeps_prompt_and_peer_extensions_private`

The interactive-session target now lists both `native_interactive_session_workflow`
and `unsupported_adapters_never_contact_or_create_workspace`. The latter was added
in `8aa04e43203b5208d7c08e709f5c8abb9e2cb2bb` and checks unsupported adapter/path
requests without creating a workspace or contacting the fixture. Admitting its
name is not a new original-obligation mapping or a claim that it ran in the audit
repair. The other six inventories are unchanged by this repair, including the
blank-asset-root regression. The candidate gate command remains:

```sh
CARGO_BUILD_JOBS=2 cargo test --offline --locked \
  --test handoff_native \
  --test interactive_sessions_workflow \
  --test session_prompts_workflow \
  --test session_readers_workflow \
  --test task_list_human \
  --test example_assets_workflow \
  --test session_views_vectors
```

This is an independent gate *inventory*, not a replacement for `just check` or
the remaining native tests. The audit does not run these commands. It checks
test-name drift and recursively scans statically named Rust modules/includes,
ignoring comments and checking string literals for Python executables/helper
references. Missing dependencies and dynamic includes fail the inventory.

The Rust check is a small lexical scanner, NOT a Rust AST parser. It does not
prove no Python can execute through crate libraries, macros, computed commands,
external tools, shell scripts or runtime imports. A comment asserting "no Python"
is not evidence. Final acceptance still needs an interpreter-free environment,
runner-free packaged tree, supported Linux/macOS workflows, compatibility
disposition, installation/upgrade and external packaging acceptance. None is
certified by this stage.

## Asset Expectation Provenance

`tests/example_assets_workflow.rs` no longer launches a live Python oracle. Its
literal expected tasks and goal were reviewed from the unchanged
`runner/coven.py::starter_tasks` and `example_goal_text` source at the recorded
HEAD. They were not generated by the native implementation or a new fixture
generator. The fixture checks slug/default/owner/notes policy and universal
newline conversion. Only `created_at` and `updated_at` remain volatile, as in the
previous test; captured fields must now also be strings with a UTC timestamp
shape. Actual CLI init/refresh, exact expected fields, source-byte and
authored-state preservation, unsafe-context refusal and relocation cases remain.
This removes one live oracle dependency; it maps none of the 941 obligations.

## Remaining Scope

The Python helper inventory includes `tests/_*.py` and source-local oracles, but
their internal cases are NOT counted as additional original methods or mapped
obligations. Rust inline oracle call sites and helper scenario assertion IDs
still need a later ledger expansion. Production Python must remain until those
obligations, all 941 originals and the platform/package gates are resolved.
Moving the runner under tests or retaining a copied implementation is not an
acceptable retirement mechanism.

## Session Views Batch

`fixtures/session_views.json` freezes **127 outermost reference calls** across
all loops/subcases in the 19 original `test_session_views.py` methods. There are
46 deduplicated JSON documents. `fixtures/session_views_provenance.json` records
each original ID's assertion obligations and exact case IDs, the reference/test
source hashes, Python version and source commit, and the capture-tool hash.

Capture ran once using isolated Python **3.14.8**, `-I -B`, a cleared environment
and an explicit `--oracle-root`. Both selected reference files were checked
byte-for-byte against commit `e389a08669204d4e51a49ffd13cbe01b90b5d8ba`.
All 19 original methods passed during capture, including their original
assertions and poisoned-call checks. No expected values were generated by Rust.

The corpus SHA256 is
`cff42fcf78013f414ebff64ef93b4287c6aae21ad35f543373a0c70b6905cd8c`.
Only JSON whitespace was compacted when applying the captured data; parsed
content was checked equal to the single capture artifact. Provenance records
both the original capture serialization hash and the applied corpus hash.
Documents use generic JSON-pointer `set`/`remove` deltas from earlier documents,
not a retained selection implementation. Native tests expand those data deltas,
then compare **full serde values** against production selection results.

```sh
CARGO_BUILD_JOBS=2 cargo test --offline --locked --test session_views_vectors
```

Five native tests account for every case, validate the corpus hash/original ID
inventory, assert exact results/refusal codes, full aggregate/context preservation,
input-relative target order and JSON round trips. They check all mutable
descendant addresses are independent, mutate every selected container, repeat
the original alpha/beta nested mutations and prove input/other/fresh results
are unchanged. Both Null-report invalid-selector cases exercise validation
before report access. The 64-character accept and 65-character reject limits
are retained for both group and member selectors.

Ten Python non-string selector cases cannot inhabit Rust `Option<&str>` and ten
non-bool recognized cases cannot inhabit the Rust `bool` argument. The corpus
retains them explicitly with typed-boundary routes, including tagged Python
bytes/object values. An exact rustc-checked function-pointer signature is the
compile-time exclusion evidence. Actual native CLI tests refuse attached and
separated `--recognized-only` value spellings, reject missing group/member
values, and accept absent/bare boolean flag grammar via help. CLI value spellings
are parser tests, **not** fabricated Rust values or a claim that a Python object
can be submitted to the Rust API. No invalid typed case is silently coerced or
omitted. Error acceptance is preserved; Python exception text is recorded as
provenance, not imposed on native diagnostic codes.

For the final purity method, native functional output and a real local sentinel's
bytes/mtime/directory entries are tested. Source inspection of
`src/session_views.rs`, `groups/schema.rs::selector` and `error.rs::require`
finds only validation, cloning, iteration and JSON construction. There is no
native clock/network/process poisoning seam; the local filesystem check is
not a universal no-I/O or no-clock proof. That method remains **unresolved D**.

Optional reference recapture is development-only and emits data to stdout:

```sh
env -i PATH=/usr/bin:/bin HOME=/tmp PYTHONDONTWRITEBYTECODE=1 \
  /usr/bin/python3 -I -B tools/freeze_session_views.py \
  --oracle-root /explicit/historical/reference --emit-patch
```

The argument is required; there is no implicit current production reference.
The utility never writes fixtures or changes the ledger and is never invoked
by native tests or the independent audit. Recapture requires explicit review;
it is not a way to bless a changed native result or bypass original obligations.

## Accepted Fragment Integration

This section records the earlier four-fragment integration (67 mapped / 874
unresolved). The [latest batch](#native-oracle-removal-and-assertion-batch--2026-11-24)
adds three fragments while preserving those original review objects and captures.

The global ledger explicitly incorporates all **61 reviews** from
`reviews/records.json`, `reviews/checkpoints.json`, `reviews/task_graphs.json` and
`reviews/session_prompts.json`, including their unresolved notes. Together with
the unchanged 19 Session views reviews, there are **80 explicit reviews**.
`provenance.accepted_batches` records each accepted fragment's SHA256 and complete
non-review metadata, including capture provenance, fixture/hash references,
native-source hashes and instrumentation limitations where supplied. Historical
fragment wording such as "proposed" or "global_ledger_modified: false" describes
the fragment at submission; explicit parent acceptance is recorded separately.
At batch integration the original 941 IDs, all 44 original source hashes and
frozen fixture contents and hashes were unchanged. The later invocation-only
source-hash transitions below preserve those historical hashes explicitly.

| Batch | Mapped | Unresolved | Categories |
| --- | ---: | ---: | --- |
| Session views (unchanged) | 18 | 1 | 18 A, 1 D |
| Records | 4 | 0 | 4 A |
| Checkpoints | 16 | 2 | 16 A, 2 D |
| Task graphs | 14 | 4 | 15 A, 3 D |
| Session prompts | 15 | 6 | 15 B, 6 D |
| Untouched files | 0 | 861 | 861 D |

Records' final D method is now A; eleven prompt D methods are now B, while the
four original B classifications remain B. Every B candidate list is explicitly
stored from the accepted native review IDs, retaining all original candidate
references. Two prompt fragment IDs contain a nested `::unix::` module qualifier.
Each was checked with `rust_tests` to occur exactly once in its referenced file,
then normalized to the strict `rust::filepath::function` ledger format. No native
reference was dropped and the validator's three-part ID rule is unchanged.

Remaining reviewed cases are deliberately not accepted as mapped:

- Session views: universal poisoned clock/network/process/no-I/O purity.
- Checkpoints: dynamically typed Python API/exception inheritance and direct-script import identity.
- Task graphs: one A method retains `g0076`/`g0077`, whose independent outer map key is unavailable in native replay; three D methods retain source-ingress/strict-JSON/API, direct-script import and universal poisoned-I/O obligations.
- Session prompts: preview poison purity, invalid-argument pre-I/O poison cases, frozen-plan mutation/API disposition, exact-admission no-terminal poison, optional/external gate API disposition, and approved-launch diagnostic/external executor disposition.
- The other 861 original methods have no invented review or candidate mapping.

Prompt acceptance includes the reviewed actual socket-constructor poison for
orphan authority and real SIGINT after all accepted receipt fields are recorded.
Their isolated ignored child helpers are implementation details, not independently
counted obligations or additions to the integration inventory. These scoped
proofs do not discharge the six remaining prompt obligations.

Adding a fragment file alone never changes global classification or status. The
audit reads only explicitly stored global reviews and selectors, not fragment
discovery or acceptance labels. The independent audit tests guard this distinction,
exact batch/category/status counts, all fragment review data and hash provenance,
the fifteen reviewed B mappings, and unchanged original/source/fixture validation.
`--require-complete` must still exit **1** with **874 unresolved obligations**
(one A and 873 D). No Python or native gate is removed, disabled or skipped.

## Accepted Entrypoint Transitions

Parent and independent review approved the eleven invocation-only transitions in
`reviews/legacy_entrypoint_transition.json` after explicit diff review against
`e389a08669204d4e51a49ffd13cbe01b90b5d8ba`. The ledger pins the approved record's
SHA256 in `provenance.accepted_source_transitions`. Each affected file links that
record, retains its captured hash as `original_sha256`, and uses the reviewed
`new_sha256` as its current `sha256`. The record preserves old/new hashes and
affected method IDs. The other 33 file entries and all historical capture/source
and fixture hashes remain unchanged; no baseline or source methods were regenerated.

The old production-wrapper argv prefix is now `sys.executable` plus
`runner/coven.py`, with the old wrapper's `COVEN_HOME` and `COVEN_COMMAND` restored
explicitly. Independent AST comparison normalized only these changes and imports;
assertions, subtests, remaining arguments, cwd, timeouts and fixtures are unchanged.
All 941 IDs, 53 A / 15 B / 0 C / 873 D categories, and 67 mapped / 874 unresolved
statuses remain frozen. This approves neither source deletion nor full retirement.

The read-only audit checks approval, the accepted record digest, both per-file
hashes and method references, then still checks each complete current source hash.
The audit tests include all eleven transitions, missing/mismatched provenance,
and unexpected assertion AST modifications that must fail without rewriting files.

The transition's reported 941 Python passes and five `tests/native_entrypoint.rs`
passes are separate gate evidence. Those five native tests remain outside the
then-current seven-target / 54-ID candidate inventory. The conservative lexical
scanner can flag Python-named trap strings even when they are not executed Python;
this limitation is not suppressed or treated as independent scanner clearance.
No Python fallback script or quoted-command workaround is added to the inventory.

## Main Merge Compatibility

`supplemental_python` explicitly admits the five `WrapperTests` methods from main
commit `649324cda16915a4a5cf8e9746f02c32f6b77f59`, separately from every original
mapping unit, category and review. `original_main_sha256` was verified with
read-only `git show 649324c:tests/test_wrapper.py | sha256sum`; `sha256` pins the
current native-compatible source. Class/method IDs are frozen, not discovered
and accepted automatically. The audit rejects unapproved files or methods,
same-count renames, assertion/source hash drift, duplicates and original-cohort
overlap. Supplemental IDs cannot be placed in original reviews or mappings.
At that integration the supplemental cohort had five methods and the full count
was 946. `--summary` and `--json` now report all three explicit supplemental cohorts
separately from the unchanged 941 originals.

The main Bash/Python fake-runner tests were adapted to real prebuilt Rust
fixtures while retaining all five IDs and their compatibility intent. This is
not an exact-method-AST equivalence claim. Native binary selection now uses
`COVEN_NATIVE_BIN`; `COVEN_HOME` means asset root, not Python binary selection.
Callback precedence is nonempty `COVEN_COMMAND`, then `ARCANA_PUBLIC_COVEN`, then
portable `coven`, preserving literal spelling and quoting. Both native entry paths
exercise init/refresh callbacks; unrelated ARCANA variables do not select them.
Relative/absolute symlinks, cycle/40-hop bounds and prebuilt-only operation remain
explicit compatibility requirements. These tests do not certify full macOS support.

The original `provenance.active_oracle` admission pinned the flat-root callback
fix: remove `PROJECT_DIR`, use `COVEN_DIR/bin/coven`. Its prior bytes and rationale
remain in the separately reviewed transition below. Historical
`oracle_coven_sha256` and all original captures remain unchanged; they describe
their recorded source, not the active checkout. No fixture golden or capture
provenance was regenerated.

All 941 original IDs/source pins, 67 mapped / 874 unresolved statuses and
53 A / 15 B / 0 C / 873 D categories remain unchanged by this integration. The
seven-target candidate native inventory still excludes `tests/native_entrypoint.rs`;
only explicitly listed names are admitted, never inferred from test discovery.
`--require-complete` still fails for the 874 unresolved obligations independently
of native execution success. This metadata does not authorize a merge or commit;
the parent independently reviews supplemental changes before committing.

## Reviewed inventory and active-oracle repair — 2026-11-23

The ordinary audit is restored without changing an original mapping:

- `tests/test_dev_install.py`: 21 supplemental methods, exact source and IDs from
  `1a8d392b4c7fb3f944821cf49a1d01418a1979cf`. These are temporary-filesystem,
  fake-Cargo, publication-race and process/signal fixtures, not pure/no-I/O tests.
- `tests/test_install_dev_environment.py`: 12 supplemental methods, exact source
  and IDs from `b5c9ece62af77f96568c3eb9e9542894154f2ecd`. Eleven use mocked probes;
  the separately gated system-probe method can invoke real rustc/pkg-config and
  inspect system OpenSSL files. These supplemental tests were not executed here.
- Both dynamically load `scripts/install_dev.py` outside the current import
  closure. Its stdlib imports and Unix/external-tool dependencies were reviewed
  manually; an empty static dependency report is not complete closure proof.
- The seven-target native inventory adds exactly the reviewed interactive-session
  test above, from 54 to 55 IDs, without adding any assertion substitution.

The active runner changed substantively in
`c907fd5df98516291bbce9804ff1b52b5baa4654`: directory mappings, Git preflight,
starter text/refresh, launch working directories and reviewed-v1 refusal. This is
**not invocation-only or behavioral equivalence**. Parent acceptance after
independent source/history and gate review is recorded in
[`reviews/active_oracle_directory_isolation.json`](reviews/active_oracle_directory_isolation.json).
It links prior, current and historical SHA256 values and Git commit/blob evidence.

`active_oracle.review` pins that record by hash. The audit rejects missing or
pending approval, changed review bytes, wrong source/hash/historical links, and
changed current source. Removing `active_oracle` cannot disable the gate when
historical oracle provenance is present. The separate original-test transition
registry is unchanged. Git history is manually verified; the read-only stdlib
validator does not invoke Git or execute runner code.

Historical graph capture used a conservative source inventory including the
complete runner. Graph function bodies are unchanged, but that does not make its
historical whole-source hash match the active checkout or establish transitive
behavioral parity. All original fixture/capture bytes and hashes are retained.

Final validation: **38 audit-tool tests passed**, ordinary audit passed, two JSON
reports were byte-identical, and `--require-complete` still exited 1 solely for
the **874 unresolved** obligations. All 44 original entries/source pins, 80 reviews,
15 candidate mappings and original fixture/provenance files remain unchanged.
No Python oracle deletion, fixture recapture, runtime deployment or new native
mapping was performed. See the [dated repair record](../../docs/changelog/2026-11-23-retirement-audit-repair.md).

## Native-oracle removal and assertion batch — 2026-11-24

Accepted `reviews/launch.json`, `reviews/groups.json` and
`reviews/handoff_schema.json` after independent assertion review. Their historical
draft wording describes submission; `provenance.accepted_batches` records parent
acceptance and pins the exact fragment hashes and non-review metadata.

| File | New mapped | Current A / B / D | Current unresolved |
| --- | ---: | --- | ---: |
| `tests/test_launch.py` | 11 | 9 / 2 / 8 | 8 |
| `tests/test_groups.py` | 9 | 0 / 9 / 26 | 26 |
| `tests/test_handoffs.py` | 3 | 0 / 3 / 30 | 30 |

All nine launch A reviews pin both the 298-case corpus and its provenance. Fourteen
new B candidates refer to the reviewed native assertions, not Python-executing
comparison helpers. Global totals are **90 mapped / 851 unresolved**, with
**111 explicit reviews and 29 B candidate entries**. The original ID digest and
all source pins remain unchanged. Audit tests project away only these reviewed
additions to verify every earlier fixed fingerprint, then check the new fragments
separately. They do not replace the old invariant hashes with a new baseline.

Two selected launch methods remain D: Python kwargs/environment-identity semantics
and invoking an actual stop-report validator are not established by transcript
equality. Handoff's mutable-return and dynamically typed API requirements also
remain D. No additional mapping is inferred from the stronger partial evidence.

`reviews/presentation_oracles.json` records removal of seven live Python constructor
sites from existing presentation/dashboard tests, with zero original mappings.
Launch conversion removes one more site and retires `src/launch/oracle.py`.
The new expectations come from explicitly pinned independent reference capture,
not native-generated goldens. Optional regeneration uses a historical reference
tree; it is not an automatic current-runner test dependency.

Validation: 68 focused native tests, 61 interpreter-isolated tests, two package
tests and 40 audit-tool tests passed; extracted-package library tests also compiled.
Formatting and audit checks passed. These scopes overlap; no aggregate full-suite
count is implied. Completion still fails for 851 unresolved obligations, and other
inline/source-local and parent-harness dependencies remain. See the
[batch record](../../docs/changelog/2026-11-24-python-retirement-first-batch.md) and
[current removal plan](../../docs/plans/PYTHON_REMOVAL.md).

## Asset/init and checkpoint oracle removal — 2026-11-25

`reviews/workspace_assets_oracles.json` and
`reviews/checkpoint_inspection_oracle.json` record independently reviewed removal
of three source-level and one integration Python constructor sites. They claim
zero original-method mappings and are not inserted into `accepted_batches` as
assertion-substitution fragments. The global ledger remains byte-identical to the
batch baseline: **90 mapped / 851 unresolved**.

The two asset helpers and checkpoint parent generator were deleted only after
closing their live consumers. Complete frozen data preserves 22 refresh cases,
48 init team rows and all 2,761 checkpoint rows, including historical duplicate
IDs and twelve exact resource refusals. Existing guards, limits and effect
assertions remain active. Source-package checks require replacements and reject
restored retired helpers; optional regeneration selects verified historical source
explicitly rather than an active-tree fallback or copied implementation.

All 16 asset tests, the complete checkpoint integration, two package checks and
40 tool tests passed. Strong interpreter-filesystem isolation covers 15 asset
tests plus the checkpoint integration, not bootstrap's external compiler. All
original source and previous fixture pins remain unchanged. See the
[dated batch](../../docs/changelog/2026-11-25-python-retirement-assets-checkpoints.md)
for exact evidence and limits; the [removal plan](../../docs/plans/PYTHON_REMOVAL.md)
tracks remaining callers.
