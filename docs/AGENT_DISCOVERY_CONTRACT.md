# Agent discovery contract

## Overview

Production `coven agent discover` is Coven-owned, ephemeral provider-family
recognition over a selected Kitsune endpoint's generic foreground-process cohort.
It is not a complete agent catalog, active-agent detector, sole-ownership transfer
or full status implementation. All 22 literal families are covered; help, server
and maintenance processes count as executable-family hints, not certified native
interactive behavior. Activity and attention remain `unknown`; native identity,
native lifecycle and job completion remain `unestablished`. Authorization is `none`
and `dispatchable` is false, including when a family matches.

No workspace argument or default lookup, storage, adoption, launch, input, stop,
resume, role assignment, native-ID association or receipt/group join is involved.
This is independent of the private persisted `agent inventory` receipt path and
the older Djinn-only process observer/group binding. There is no daemon or saved
roster. Kit remains independently useful without Coven.

## API / Interfaces

### Example usage

Reference syntax only, with explicitly inspected absolute socket and opaque IDs:

```text
coven agent discover --socket ABS_KITSUNE_SOCKET --json
coven agent discover --socket ABS_KITSUNE_SOCKET --pane PANE_ID --expected-terminal TERMINAL_ID --json
coven agent discover --socket ABS_KITSUNE_SOCKET --watch --duration 60 --interval 2 --json
coven agent discover --socket ABS_KITSUNE_SOCKET --watch --duration 60 --interval 2 --present --source coven-discovery --json
```

`--expected-terminal` requires `--pane`. `--duration` and `--interval` require
`--watch`; `--present` requires watch and an explicit `--source`, and source
requires present. Presentation additionally requires that source already be
manually enabled in Kit's generic Integrations configuration. No configuration
is created or changed by discovery. `coven-discovery` is an example routing label,
not a special trusted identity.

One-shot JSON uses `coven.agent-discovery.v1`, `schema_version: 1`. Each scan has
a fresh `scan_id`; target observation IDs are scan-scoped, not native session IDs.
Reports contain selected/resolved endpoint locators and device/inode fences,
the accepted descriptor, target pane/terminal/workspace/tab locators, reception
time, recognition and provider candidates. Matches expose only PID, canonical
provider and rule, never raw process text. No title, cwd, argv, cmdline, native
reference, legacy hint or raw snapshot is returned. Diagnostics are local codes;
peer error text is never echoed. `native_catalog_complete` is always false and
`acquisition_completeness` is always `unknown`, including observed samples and
watch outage samples. Successful admission does not establish complete acquisition.

Human one-shot output lists escaped pane/terminal locators and canonical
provider/PID/rule hints. It shows up to eight matches per pane, explicitly reporting
omissions and directing the caller to `--json` for the full bounded observation.

An observed target has `unknown`, `provider_family` or
`multiple_provider_families` recognition. Every matching process is retained;
provider candidates are deduplicated, not ranked. No job-leader selection,
preferred provider, process-tree inference or one-pane/one-agent assumption follows.
An observed empty cohort is not proof of native absence or completion.

### Capability And Versioning

Kit adds optional `snapshot.process_observation` to the existing source API.
Binary protocol **18 is unchanged**; this is an additive capability, not a new
binary protocol or a guarantee derived from the server's version string:

The exact admitted descriptors are:

```json
{"version":1,"platform":"linux","scope":"pane_foreground_cohort","selection":"unclassified"}
{"version":2,"platform":"macos","scope":"pane_foreground_cohort","selection":"unclassified","argument_policy":"darwin_procargs2_lp64_v1"}
```

Coven requires protocol exactly 18 and one of these exact tuples. On Linux v1,
an optional `argument_policy` is an ignored extension: it is neither validated nor
projected, and changes to it do not change the accepted descriptor. On macOS v2,
the exact string marker is required and participates in bracket comparison.
Old macOS v1 is rejected even with that marker. Missing/null/malformed/unsupported descriptors fail closed as
incompatible (`process_observation_unsupported`); no legacy-hint fallback occurs.
Unknown extension fields are not projected. Both brackets must agree on server
version/protocol and the accepted descriptor fields. Changes require a separately
reviewed supported capability version, not silent reinterpretation of v1.

Windows advertises `platform: windows`, `scope: selected_process`,
`selection: legacy_agent_aware`: Coven rejects it, and Windows acquisition remains
unsupported. macOS support is **source-enabled and provisional, not native runtime
certification**. Descriptor admission uses peer labels, not the Coven host OS;
those labels and returned PIDs grant no local OS/PID authority. Neither tuple is
authenticated platform/live-process attestation, a complete tree/host catalog or a
native process-generation guarantee. Endpoint inode checks are not authentication.

### Generic macOS Acquisition And Data Flow

Kit routes `pane.process_info` through `platform::process_observation_job` to
`macos_observation`, independently of the unchanged legacy macOS detector parser,
argv0 normalization and environment-hint path. No detector retirement follows.
Linux retains its rooted-tree walk filtered by foreground PGID; macOS samples
process-group members. These are different best-effort cohorts, not complete or
atomic process trees.

The macOS source reads the pane child's foreground PGID, lists positive sorted,
deduplicated members, and checks each member's BSD metadata around argument reads.
Visible PID/start-time, membership, width or comm changes omit the member; equality
cannot detect every exec or concurrent argument mutation. Unreadable members can
be omitted, while failed listing can become an empty API cohort. Unavailable or
rejected arguments retain a name-only row with argv/argv0/cmdline absent. Such
omissions are not necessarily detectable by Coven; unknown is not absence.

Argument policy is reviewed against Apple XNU `xnu-11215.81.4`, commit
`d4514f0bc1d3f944c22d92e68b646ac3fb40d452` (`exec_save_path`,
`exec_extract_strings`, `sysctl_procargsx`). Target BSD flags must establish LP64,
not observer pointer width. Reply argv starts at **`4 + round_up(L, 8)`**, where
`L` is executable-path bytes including NUL, **not `round_up(4 + L, 8)`**.
Calculated padding must be zero; the decoder preserves empty argv0 and later
empty operands, consuming exactly argc NUL-terminated strings. It does not
intentionally parse environment bytes after them. Runtime stack mutation or stale
exec data remains possible; the policy is interpretation, not authenticated argv.

Raw argument allocation is capped at **1 MiB**, with at most **six** size/read
attempts. Capacity-filling successful reads are inconclusive and retried with
headroom, not decoded as complete. Group listing also has at most six attempts
and admits at most **256** members. Decoding admits **1–128 arguments**, **4 KiB**
per decoded argument (including lossy UTF-8 expansion), **64 KiB** aggregate argv.
Before export a stricter 64 KiB process-text budget counts name, argv, duplicated
argv0, joined cmdline and a 3,072-byte cwd reserve; failure omits all argument
fields rather than exporting a truncated argument list.

### Classifier V1

`src/agent_processes.rs` is pure lexical policy. Raw structured process text is
used only in memory; there is no filesystem canonicalization/symlink resolution,
PATH lookup, native-store/environment lookup or cmdline/execution-text parsing.
No executable is probed or run. Structured nonempty `argv` is authoritative even
if its first string is empty. Otherwise a present `argv0` masks `name`, including
an empty argv0; only absent argv0 permits name fallback. Only POSIX `/` separates
basenames. Aliases ASCII-case-fold and accept exactly one optional `.js` suffix;
there is no trimming, quote peeling, Unicode normalization or Windows suffix
stripping. These are the exact canonical provider/alias groups:

| Provider | Literal aliases |
| --- | --- |
| pi | pi |
| claude | claude, claude-code |
| codex | codex |
| gemini | gemini |
| cursor | cursor, cursor-agent |
| devin | devin, devin-cli, devin cli |
| agy | agy, antigravity, antigravity-cli |
| cline | cline |
| omp | omp |
| mastracode | mastracode, mastra-code, mastra code |
| opencode | opencode, open-code |
| copilot | copilot, github-copilot, ghcs |
| kimi | kimi, kimi-code, kimi code |
| kiro | kiro, kiro-cli |
| droid | droid |
| amp | amp, amp-local |
| grok | grok, grok-build |
| hermes | hermes, hermes-agent |
| kilo | kilo, kilo-code, kilo code |
| qodercli | qodercli, qoderclicn, qoder, qodercn |
| maki | maki |
| djinn | djinn, djinn-ui |

A direct alias produces `literal-executable`. Otherwise only these wrapper
basenames are supported (ASCII-case-folded); flag spellings are exact:

| Wrapper | Standalone flags | Flags consuming one following nonempty, non-dash value |
| --- | --- | --- |
| node, nodejs | --no-warnings, --trace-warnings, --enable-source-maps | -r, --require, --loader, --import |
| bun | --smol | -r, --require, --preload |
| python, python3 | -u, -B, -E, -I, -s, -S | -W, -X |
| sh, bash, zsh | -l, -e, -u, -x, -f | none |
| fish | --no-config, -N | none |

`--` ends option scanning. Only the first script operand is inspected; unknown
flags or a non-agent first script do not permit scanning later arguments.
Alias scripts produce `literal-wrapper-script`. For node/nodejs/bun only, the
case-sensitive exact script `node_modules/@earendil-works/pi-coding-agent/dist/cli.js`
or a string ending in `/` plus that suffix produces `literal-pi-package-script`.
This suffix check is lexical, without path normalization. No npm/env wrappers,
`bun run`, versioned python basenames, combined/equal-sign flags, shell `-c`,
eval/print/module execution text or inferred executable aliases are supported.
Shell `-e` above is only an option before a script, not evaluated code.

Alias provenance is Kit `src/detect/mod.rs:180-205`, commit
`01d7cef6e98e2ccab216cd669d569c0731abfbce`. The frozen
`tests/fixtures/kitsune_process_policy/reference.json` corpus has 209 legacy cases.
V1 deliberately diverges on structured-field authority, no cmdline parsing or
filesystem resolver, lexical suffixes/wrappers, and all-match/no-leader/no-ranking
semantics. The comparison ledger in `src/agent_processes/tests.rs` is not parity,
provider operational certification or a port of the legacy detector.

## Data Flow And Collection Bounds

One initial `session.snapshot`, one `pane.process_info` per selected pane, and one
final snapshot share an entire-collection deadline and aggregate response budget.
No Kit agent APIs or snapshot agent hints are consumed. Full JSON decoding and
schema/budget admission precede projection; ignored raw fields are not output.

| Limit | Bound |
| --- | --- |
| Raw snapshot pane records | 1,024, including selected-pane inspection |
| Selected panes | 64 |
| Processes per pane | 256 |
| Arguments per process | 128 |
| Name, argv0, each argument | 4 KiB each |
| Total process text, including supplied cmdline/cwd | 64 KiB per process |
| Top-level process-info tty | Nullable string, at most 4 KiB; never projected |
| Aggregate response bytes | 1 KiB through 8 MiB; default 1 MiB |
| `--timeout` | finite, greater than zero, at most 30 seconds; default 3 |

### Edge Cases

Both inventory brackets must preserve selected pane/terminal/workspace/tab
mappings. Endpoint selection/device/inode checks fence reads and their replies.
Any detected endpoint, mapping, capability, schema, budget, timeout or collection
failure discards **all** collected targets, not just the failed pane. This does not
detect every upstream omission or partial acquisition. Failed
counts are null rather than successful zeroes. Matching mappings are not atomic
PID attribution; processes can exit and PIDs can be reused (ABA) between reads.
Reception time is not a native freshness or process-generation attestation.
Native nullable argv/argv0/cmdline/cwd fields are admitted. Unknown extensions,
including a row-level tty field, remain opaque and are never projected.

Read-only requests are **not zero-effect server operations**. Existing Kit
pipeline housekeeping can expire metadata, drain events, synchronize titles and
ensure a default workspace, potentially spawning a shell. Discovery itself does
not request workspace creation or native control. No actual live installed Kit
commands were used in this documentation session.

## Watch And Presentation

Watch is bounded serial foreground sampling, default duration 60 seconds and
interval 2 seconds. Duration is finite `0.1..3600`; interval is finite `0.1..60`.
Each collection is capped by remaining watch time. Elapsed ticks are skipped,
not caught up concurrently. JSON mode emits one JSONL sample per collection then
a `coven.agent-discovery-watch.v1` summary with observed/unavailable/incompatible
counts, maximum targets/sample time, skipped ticks and presentation diagnostics.
Each outage is a fresh unavailable/incompatible sample, never stale positives.
The initial endpoint selection is frozen, including initial selection failure;
future/replacement endpoints are never automatically adopted.

`--present` requires interval at most 2 seconds and collection timeout at most
3 seconds. Optional generic snapshot + `integration.view.open/replace/poll/close`
IO is serial with at most one second per ordinary UI call, bounded by watch time;
best-effort definite-owner cleanup has a separate 200 ms close bound. The generic
view has a five-second TTL. There is no background poll that renews stale positive
rows while collection blocks. A fresh unavailable frame replaces positives with
an unavailable summary; if no fresh frame can arrive, the accepted frame expires.
Age/lease checks stop renewal rather than extend stale observations.

Any uncertain UI operation permanently stops **all** subsequent UI calls, including
close: no failover, resend, reopen or retry-close. Definite failures can attempt
one known-owner close; uncertain cleanup is TTL-only. Core collection/output
continues independently of presentation failure. Delivery is ephemeral, not
lossless, and acknowledgements do not prove physical rendering.

Frames have at most 128 rows (one summary plus 127 matches) and 96 KiB; omitted
matches are explicit in summary and presentation stats. Row IDs are
publisher-lifetime ephemeral hashes, not receipt/session/native identity. Selection
only marks read-only observation details in a later fresh frame; it grants no
focus/navigation, pane binding, native effects or role authority.

| Exit status | One-shot | Watch |
| --- | --- | --- |
| 0 | Observed, even no matches | Completed bounded watch, including all-outage watches |
| 2 | Incompatible evidence/capability | Not an outage exit status |
| 1 | Unavailable/error | Output failure/error |
| 130 | Interrupted | Interrupted |

Watch completion is not successful native observation or successful presentation.
Streaming output failure stops without replay; a failed output channel may not
receive the final summary. Stats describe observations/delivery, not native work.

## Ownership And Next Seam

**No matching Kit detector retirement has happened yet.** The source audit found
legacy detection coupled to hook-owner confirmation, start readiness,
restore/restored-idle behavior, mobile UI and navigation. The internal seam below
separates evidence, not the remaining management ownership. This work adds
no toggle, kill switch or hide controls. It is a functional Coven-owned recognition
path with temporary duplicate legacy policy, not completed sole ownership/full status.

The internal typed **foreground-owner evidence**, **fallback publication**, and
**historical identity** seam is now implemented in Kit; see the optional sibling
[evidence guide](../../kitsune/DETECTOR_EVIDENCE_SEPARATION.md). Historical/screen-only
data cannot mint ownership, takeover or native effectiveness. Restored Idle/name/
reference/pending resume/managed Active remain, but `interactive_ready` needs a
matching probe or accepted fallback, not foreground-only readiness or native proof.
Native input's independent fresh process check remains. This is deliberate public
semantic hardening, not blanket compatibility. No new Config, wire/API/snapshot
fields or external mode were introduced by that earlier evidence seam. The current
generic acquisition cut separately adds the macOS v2 argument-policy descriptor
and Coven admission described above. Historical seam validation and limits remain
in the [migration record](AGENT_MANAGEMENT_MIGRATION.md#state-seam-validation-table),
not current acquisition validation totals.
Do not use an ephemeral family hint as owner confirmation. Status/attention,
mobile/navigation, start/resume and agent-aware restore remain separate transfers.
All five prior Linux hook/configuration retirements stay unchanged; matching
Windows discovery and broader management capabilities remain pending. The
[private hook-file backend](HOOK_FILE_BACKEND_CONTRACT.md) now source-enables
provisional eligible-local-APFS macOS apply for all five file managers; this does
not itself qualify generic process acquisition, receiver privacy,
full status/start/resume/restore, or any non-Linux Kit retirement. See the authoritative
[migration ledger and validation](AGENT_MANAGEMENT_MIGRATION.md).

## Validation And Related Files

Current source includes decoder/acquisition fixtures, exact-tuple admission and
bracket-drift checks, empty-argv/name-only cases, and watch/presentation coverage.
These are not native provider or macOS runtime certification. Actual-source Rust
checks for Apple Silicon and Intel targets are distinct from Apple SDK C compilation,
native linking and execution. Native validation remains blocked by the missing
Apple SDK header `TargetConditionals.h`; no SDK/link/runtime success follows from
Rust target source-checks. Historical validation is recorded separately in the
[migration ledger](AGENT_MANAGEMENT_MIGRATION.md#discovery-validation-table), not
recounted as the current composed cut. This docs-only update ran no tests, builds
or live endpoint commands and generated no test/build artifacts.

Review fixed queued selection loss, source-package fixture omission, protocol
admission, nullable-field handling and typed negative CLI operand handling. Actual
Kit intent-queue semantics are modeled: fresh observed scans poll the displayed
revision before replacement; failed scans clear positives before polling.
No installed-native/provider interactive behavior or macOS/Windows runtime operation is
certified. Full `just check` remains unapproved/not run. Earlier OpenCode results
are historical and non-additive. No live installed Kit configuration was changed.

Implementation: `src/agent_discovery.rs`, `src/agent_discovery/{schema,tests}.rs`,
`src/agent_processes.rs`, `src/agent_processes/tests.rs`,
`src/agent_discovery_watch.rs`, `src/agent_discovery_watch/{presentation,tests}.rs`,
`src/main.rs`; workflow fixtures: `tests/agent_discovery_workflow.rs` and
`tests/agent_discovery_presentation.rs`. Kit capability producers live in
`src/platform/{linux,macos,windows,fallback}.rs`,
`src/platform/{foreground,macos_observation,macos_procargs}.rs`,
`src/api/schema/session.rs` and `src/app/api/{session,panes}.rs` in the separate checkout. Sibling links are navigation,
not core dependencies.
