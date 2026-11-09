# Kitsune policy extraction: characterization and gates

## Decision

Keep Kitsune's production behavior unchanged while establishing independent
reference evidence in Coven. The next safe production seam is **separating
acquisition from interpretation inside Kitsune**, not routing more execution
through Coven or deleting existing integrations.

The earlier [resume planner](SESSION_RESUME_PLAN_CONTRACT.md) covers literal argv
policy only. This slice adds **5,491 source-backed characterization cases**:

| Corpus | Cases | Captured behavior |
| --- | ---: | --- |
| `tests/fixtures/kitsune_process_policy/reference.json` | 209 | Actual job classifier and Windows descendant selection, using synthetic data on Linux. |
| `tests/fixtures/kitsune_native_reference/reference.json` | 5,282 | Report/snapshot reference conversion, native-state reservation, startup-source normalization, and reference-only dedupe. |

There is **no new public command, production policy change, ingestion path,
executor, mandatory dependency, or authorization**. Frozen cases are historical
source evidence, not a native session registry or a promise of restored sessions.
Ordinary tests consume local fixtures without a sibling checkout, Git, network,
provider, live endpoint, process acquisition or reference compiler harness.

## Provenance and reproduction

The source baseline is Kitsune commit
`01d7cef6e98e2ccab216cd669d569c0731abfbce`.

The [process harness](tools/harnesses/kitsune-process-reference/README.md) verifies
explicitly supplied files against pinned Git objects, then compiles unchanged
source blocks in `OUT_DIR`. Glue adapts input structures and serializes results;
it does not substitute classification or selection policy.

| Kitsune source | Git blob | Compiled inclusive lines |
| --- | --- | --- |
| `src/detect/mod.rs` | `7bbc2ca8113c76da70dcb17a007503811604de91` | 41–243, 329–614 |
| `src/platform/mod.rs` | `5c168acb23d39efb43085d230c5221e2e753a882` | 6–19 |
| `src/platform/windows.rs` | `7f1008326f68bd0da6d31346c3357bfdb2c284ed` | 77–85, 292–415 |

Unique boundaries, exact ranges, full-file hashes and block hashes are checked.
The fixture retains provenance hashes/markers, **not vendored source bodies**;
source comes from the separately licensed Kitsune checkout.

The [reference harness](tools/harnesses/kitsune-resume-reference/README.md) adds
`--native-policy OUTPUT.json`, executing the whole unchanged `src/agent_resume.rs`
module (blob `0c3b51bd19d73aa0ede31d6775cac35395447273`). Existing argv capture
and its 1,512-case corpus remain byte-for-byte unchanged.

Both harnesses require explicit source paths, operate offline with pinned cached
dependencies, disable Git replacement objects, and have no sibling-path default.
Their pins establish local integrity, not upstream/author authentication. Only
explicit development recapture needs source access and compiler/Git tooling.

Fixture SHA-256 values:

```text
process: 8309440254a20d5dc747d5f1b1b32acb2acfb25586e37260fb57a968533e7e6d
native:  c18f0b9f2871088dfdd0e9aa1ad2987c350333175c7ade72bca4e55e8e186712
argv:    ddfcbcef2aad3e6e7f46634d7c50afa64ae4f8fe351837bd4f6cdc1aba4ef9fe
```

Native fixture tests:

```sh
cargo test --offline --locked --test kitsune_process_policy_reference --test kitsune_native_reference_policy --test session_resume_reference
```

The harness READMEs document explicit recapture commands. Byte-exact recaptures,
source-verification refusal cases, and unchanged reference-module tests are
separate from frozen semantic checks. Schema mutation tests reject wrong types,
missing/unknown fields, malformed selectors/process records, and inconsistent
snapshot identity **independently of fixture checksum checks**.

### Local validation

- **67 focused Coven tests passed**, including all 16 new characterization tests,
  existing argv/reference/CLI checks, interactive-session regression, comparison
  workflows and package inventory.
- All **7 unchanged reference-module tests**, the process harness repeatability
  test and **5 fail-closed source-verification checks** passed. Independent
  recaptures matched the frozen corpora; the original argv corpus stayed unchanged.
- Strict all-target Clippy, Rust **1.85.0** all-target checking, root and harness
  formatting, shell syntax, diff whitespace checks and offline locked package
  verification passed. The source package excludes development harnesses and
  these checkout-only corpora/tests.

This slice did not rerun the full legacy-oracle suite or remote CI. Windows/macOS
acquisition, installed providers and actual restore/lifecycle interoperability
remain unvalidated. No production source, Kitsune code, live endpoint or user
configuration was changed.

## Process-selection findings

`pane.process_info` (`src/app/api/panes.rs:193–239`) uses the platform foreground
job collector. It does not directly call the agent classifier—but that does not
make acquisition agent-neutral on every platform:

- **Linux:** foreground-PGID evidence from the pane/PGID-rooted process trees,
  filtered by group membership; not an unrestricted host-wide member scan.
- **macOS:** foreground process-group enumeration, with group membership checks.
- **Windows:** a snapshot-based descendant selector already calls the agent
  classifier. One recognized descendant wins; a single same-agent ancestry chain
  selects its topmost recognized candidate. Same-agent siblings, mixed-agent
  descendants, or plain descendants fall back to the pane root. A missing root
  yields no selected job. Output is a **singleton**, with the selected PID used
  as the group-like ID—not an OS terminal foreground PGID.

The current wire payload omits parent relationships, selection basis and discarded
descendants. Coven therefore **cannot take over Windows selection from today's
payload**. Agent policy would remain in Kitsune even if the final classifier moved.

The classifier is not filesystem-pure: path-token interpretation can call
`canonicalize`. Capture confines this to a private temporary cwd, controlled
relative paths and one internal symlink to an inert file. No real home/store/config
paths or acquired process data are used. OS acquisition and Windows command-line
parsing are not tested by synthetic selection on Linux.

The 209 cases cover all 22 labels, aliases, suffixes, runtime wrappers, field
precedence, leader preference, priority ties and 88 tree-order permutations.
Observed quirks remain evidence, not corrections: combined `bash -lc` can match
where `bash -c` does not; cmdline fallback is limited whitespace tokenization;
help/version/maintenance modes can still match. None establishes interactivity,
authenticated identity, activity, readiness or completion.

Coven's existing `exact_posix_argv0_basename` remains deliberately narrow:
literal `djinn`/`djinn-ui` argv[0] basenames only. This slice does not claim broad
recognition parity, expose a new classifier API, or change existing reports.

## Native-reference findings

The 5,282 cases directly exercise the reference functions without applying
Coven's stricter selector validation first:

| Operation | Cases | Protected distinction |
| --- | ---: | --- |
| Report conversion | 3,216 | Pi/OMP prefer valid path, then valid ID; other providers ignore paths. |
| Snapshot conversion | 1,608 | One explicit kind/value; invalid path does not fall back to an ID. |
| Native-state reservation | 420 | Exactly eight source/provider pairs, not all 16 official pairs. |
| Startup normalization | 30 | Six exact values after Unicode trimming; no case folding. |
| Dedupe | 8 | Literal source/provider/kind/value separation, no canonicalization or authorization. |

Reservation covers Claude, Codex, Copilot, Devin, Droid, QoderCLI, Cursor and
Grok. This is not the full-lifecycle-hook set or a producer-authentication rule.
Reference ID constructors accept blanks and leading hyphens; `None` conversion
outcomes remain null rather than being rewritten as Coven validation errors.

The **restore/state call graph was inspected, not executed by these fixtures**:

- API label normalization precedes reference conversion; state/session/release
  consumers apply additional ownership and sequence rules. Invalid reference
  conversion can become `None`, which release treats differently from a scoped
  valid reference. Do not silently replace conversion failure with a release.
- `TerminalState::foreground_agent_confirms_session_owner`
  (`src/terminal/state.rs:1544–1555`) also calls `agent_resume::plan(...).is_some()`.
  Planner support is therefore an **ownership predicate**, not only argv policy.
- `pane_restore_startup` (`src/persist/restore.rs:736–779`) reserves literal keys
  before runtime construction. Duplicate panes suppress saved presentation
  history and rehydrated reference identity, not merely duplicate argv.
- Reservation is shared across the restore pass in snapshot workspace/tab/layout
  order. Imported runtimes participate before their no-new-launch branch.
- Runtime construction, theme/geometry waits, shell startup, command enqueue,
  deferred failures and provider readiness are distinct stages. A dedupe string
  alone certifies none of them.

Terminal ownership, sequence consumption, pending lifecycle reports, release
effects, actual restore/history replay, reservation rollback and deferred launch
remain **unexecuted gates**, not equivalent behavior established by this corpus.

## Next production seam and removal gates

### Local candidate: preserving seams implemented

The following local Kitsune candidate now implements the first two internal
seams, without moving classification into Coven or changing its existing API:

- `detect::identify_agent_in_job_with_resolver` threads one mutable path resolver
  through the classifier. The original entry point retains filesystem
  canonicalization and all fallback/ranking behavior.
- Private `platform::process_selection` holds the actual descendant/tree policy
  with an injected classification callback, compiled for Windows production and
  host unit tests. Windows retains acquisition/cache and supplies its existing
  classifier through a thin adapter.
- Nine resolver tests and six additional selector tests were added; all ten
  existing selector tests moved with the actual implementation. Duplicate-PID
  behavior is additionally protected, without changing the unique-PID baseline
  fixture schema or adding malformed-graph hardening.

The separate [candidate replay tool](tools/harnesses/kitsune-process-candidate/README.md)
compiles current source without rewriting policy and compares it against the
unchanged independent baseline. **209/209 records match**, and **26 source-subset
tests pass** (replay plus 25 actual source tests). Full-file/block hashes distinguish
the candidate from the pinned reference. The original reference harness and corpus
were not relaxed or regenerated to accept changed source.

The 20 focused Coven frozen-reference/package tests also pass, as do Coven's
strict all-target Clippy, candidate-subset Clippy and both repositories' formatting/
diff checks. Candidate-subset lint allowances are documented in the replay tool;
they are not evidence of a clean full Kitsune application lint.

**This is not full-application or Windows validation.** Kitsune's normal test and
lint recipes stop in its build script because Zig is missing; only the Linux Rust
target is installed. Formatting, independent review and source-subset tests/lint
pass, but the full `just check` and Windows compile/cache/acquisition gates remain
open. See Kitsune's optional sibling note
[`PROCESS_POLICY_EXTRACTION.md`](../kitsune/PROCESS_POLICY_EXTRACTION.md).
Local changes in both repositories remain uncommitted. No live apps or user
configuration were changed.

### Remaining gates

1. **Validate the preserving resolver/selection candidate in the full app** once
   Zig is available. Retain canonicalizing adapter coverage and run the actual
   relocated tests in normal cross-platform CI, not only source-subset tooling.
2. **Validate Windows wiring/acquisition**, then design an
   additive generic process-tree evidence surface with parent relationships and
   explicit acquisition scope/completeness semantics; do not relabel the current
   agent-selected singleton as a complete tree.
3. **Characterize the actual restore/state machine before moving it.** Cover
   sequencing, scoped/unscoped release, ownership takeover, imported runtimes,
   duplicate history/identity suppression, immediate rollback and deferred failure.
   Do not copy it into a second lifecycle simulator in Coven.
4. Before optional delegation/removal, validate each affected runtime and platform,
   retain Kitsune's independent operation, and treat hooks/installers and status/
   attention as separate effects and authority boundaries.

No scheduler, daemon, automatic following or native admission framework is needed
for these steps. No built-in is ready for removal on the strength of this corpus.
