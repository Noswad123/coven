# Pure provider resume planning

## Scope and interface

`coven session plan-resume` is an inspection-only, Coven-owned characterization
of Kitsune's pure provider resume policy. It does not replace Kitsune's built-ins
or add execution support for these providers.

```sh
coven session plan-resume --runtime claude --source kitsune:claude --session-kind id --session saved-session --json
coven session plan-resume --runtime pi --source kitsune:pi --session-kind path --session /missing/session.jsonl
```

All four selection flags are required. Optional `--executable ABS_PATH` replaces
only `argv[0]`, lexically, without lookup or probing. Nonexistent paths are valid
inspection inputs. There is no workspace operand, socket, timeout, environment
discovery, native-store inspection, provider launch, input submission, `--execute`,
saved-report input, automatic retry, or execution authorization. Flags are exact;
abbreviated and duplicate flags are rejected.

The public native API is
`session_resume_plans::{SessionKind, Request, arguments}`:

```text
SessionKind = Id | Path
Request { runtime: String, source: String, session_kind: SessionKind,
          session: String, executable: Option<String> }
arguments(&Request, json_output: bool) -> Result<TaskOutput>
```

Every request passes validation, including unsupported combinations. The private
plan cannot be deserialized or supplied as authorization to another command.

## Literal mappings

The source must be exactly `kitsune:` followed by the runtime in this table.
There is no trimming, case folding, alias inference, or source authentication.
`VALUE` is one literal argument or the literal suffix of the joined flag, never
shell text. Metacharacters, spaces and Unicode are preserved, not interpreted.

| Runtime | Reference kind | argv |
| --- | --- | --- |
| claude | id | `claude --resume VALUE` |
| codex | id | `codex resume VALUE` |
| copilot | id | `copilot --resume=VALUE` |
| devin | id | `devin --resume VALUE` |
| droid | id | `droid --resume VALUE` |
| djinn | id | `djinn -s VALUE` |
| kimi | id | `kimi --session VALUE` |
| mastracode | id | `mastracode --thread VALUE` |
| pi | id, path | `pi --session VALUE` |
| omp | id, path | `omp --resume=VALUE` |
| hermes | id | `hermes --resume VALUE` |
| opencode | id | `opencode --session VALUE` |
| qodercli | id | `qodercli --resume VALUE` |
| kilo | id | `kilo --session VALUE` |
| cursor | id | `cursor-agent --resume VALUE` |
| grok | id | `grok --resume VALUE` |

The table is a historical source-policy contract, not verification that an
installed provider version accepts these arguments or that a session exists.
Default binary tokens remain unresolved. Explicit absolute overrides are unchecked.

The existing execution-capable **`session resume` remains Djinn-only**, with its
separate `--session VALUE` UI contract. This new historical planner's Djinn `-s`
mapping is intentionally not substituted into that command. Neither command
accepts saved planner reports as execution inputs.

## Validation and outcomes

- Source/runtime: nonblank, at most 512 UTF-8 bytes, no control characters or
  leading hyphen after leading whitespace. Exact well-formed unknown labels are
  unsupported, not normalized into known labels.
- ID: nonblank, at most **512 UTF-8 bytes**, no control characters or leading
  hyphen after leading whitespace.
- Path reference: the same text rules with a **4096-byte** limit and a leading
  POSIX `/`. There is no existence check, canonicalization, traversal or symlink
  resolution; repeated separators, `..` components and trailing spaces are data.
- Executable override: nonblank, at most 4096 UTF-8 bytes, absolute POSIX path,
  no controls, trailing `/`, or `..` component. Root alone is rejected.
- Invalid values return fixed errors without echoing operands:
  `invalid_resume_source`, `invalid_resume_runtime`, `invalid_session_selector`,
  or `invalid_executable_path` (exit **1**).
- Use attached syntax, e.g. `--session=-option`, to submit a nonnumeric hyphen
  operand for domain validation. A separate nonnumeric hyphen token is a CLI
  syntax error (exit **2**); missing/unknown flags or invalid kinds also exit 2.
- A supported valid combination yields `outcome: planned`, literal `argv`, exit
  **0**. Well-formed unsupported source/runtime/kind combinations yield
  `outcome: unsupported`, **omit `argv`**, and exit **2**.

Default human output labels argv as ASCII-escaped JSON data, not a runnable shell
command. `--json` produces compact ASCII-escaped JSON with `schema_version: 1`
and `contract: coven.session-resume-plan.v1`. Library output has no terminal LF;
the CLI adds exactly one. Reports include the supplied source/runtime/kind/value;
do not publish them if session identifiers or paths are sensitive.

Trust fields explicitly remain `reference_trust: caller_supplied_unverified`,
`delivery: not_attempted`, `authorization: none`, `native_lifecycle: unestablished`,
and `readiness: unestablished`. `executable_resolution` is `not_attempted` or
`unchecked_absolute_override`. The report does not export the legacy dedupe key
as a durable identity or idempotency promise. Caller source labels establish
neither provenance nor permission.

## Independent characterization and intentional hardening

Frozen expectations in `tests/fixtures/kitsune_resume/reference.json` were captured
by compiling and executing Kitsune's **unchanged** `src/agent_resume.rs` at commit
`01d7cef6e98e2ccab216cd669d569c0731abfbce`, Git blob
`0c3b51bd19d73aa0ede31d6775cac35395447273`.

The standalone development harness in
[`tools/harnesses/kitsune-resume-reference`](tools/harnesses/kitsune-resume-reference/README.md)
requires explicit root/source paths and verifies supplied source bytes against
the pinned commit/blob. It has no copied argv table, production dependency, or
default sibling checkout path. Frozen tests require neither Kitsune nor schemars.
The harness and its build outputs are excluded from Coven source packages.

The **1,512 cases** cover 648 source/runtime/kind matrix cells plus 864 edge
cases: byte limits, Unicode, blank/option-like/control values, relative paths,
literal metacharacters, and raw constructor bypasses. Each case records both
constructor validity and raw versus constructor-validated planner argv.

This distinction matters: Kitsune's public reference struct allows callers to
bypass constructors, and its pure `plan()` does not revalidate values. Its ID
constructor also accepts blanks and leading hyphens. Coven deliberately rejects
these at its public boundary, even for unsupported pairs. Dispositions explicitly
record `parity` versus `reject_invalid_session_selector`; no blanket behavior
equivalence or automatic-restore compatibility is claimed.

Native workflow tests cover the CLI, fixed diagnostics, safe rendering, unchanged
empty working directories, nonexistent overrides, and an executable sentinel that
must never launch. Reference-module tests and deterministic recapture also pass.
These are source-policy/Linux checks, not macOS/Windows or installed-provider
interoperability certification; remote CI is a separate gate.

### Local validation record

- New slice: **19 tests passed** (6 library, 11 native CLI, 2 frozen-reference
  tests); the latter exercise all 1,512 captured cases.
- Broad run: **1,037 library tests passed, 5 process helpers ignored**, and all
  **86 binary tests passed**. The full `cargo test --offline --locked --all-targets`
  attempt hit the 600-second command limit during the existing
  `execution_maintenance` Python-oracle integration. It is **not a full-suite pass**;
  the remaining integration targets were not certified by that run.
- Focused regression gate: **81 integration tests passed** across resume planning,
  reference parity, interactive sessions, native prompts, session readers/views,
  Kitsune comparison, group observation, and package inventory.
- Strict all-target Clippy, formatting/diff checks, Rust **1.85.0** all-target
  checking, optimized release build, and offline locked package verification passed.
- Independent review found no blocking findings and independently recaptured the
  reference fixture byte-for-byte. Its SHA-256 is
  `ddfcbcef2aad3e6e7f46634d7c50afa64ae4f8fe351837bd4f6cdc1aba4ef9fe`.

No live endpoint, provider, user configuration or Kitsune code was changed. This
record does not certify the older remote-CI failures or an installed integration.

## Next extraction boundary

Keep Kitsune's built-ins and existing restore/report ingestion unchanged. Before
introducing optional delegation behind compatibility wrappers, characterize
generic process selection (including Windows), native-reference persistence and
ingestion, restore sequencing/deduplication, attention/status semantics, and hook
installation effects separately. Test each runtime and supported platform before
removal. No scheduler or mandatory Coven dependency is needed for this work.
