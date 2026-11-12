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
peer error text is never echoed. `native_catalog_complete` is always false.

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

```json
{"version":1,"platform":"linux","scope":"pane_foreground_cohort","selection":"unclassified"}
```

Coven requires protocol exactly 18 and exactly this descriptor's version and
field values. Missing/null/malformed/unsupported descriptors fail closed as
incompatible (`process_observation_unsupported`); no legacy-hint fallback occurs.
Unknown extension fields are not projected. Both brackets must agree on server
version/protocol and the accepted descriptor fields. Changes require a separately
reviewed supported capability version, not silent reinterpretation of v1.

Windows advertises `platform: windows`, `scope: selected_process`,
`selection: legacy_agent_aware`: Coven explicitly rejects it. macOS advertises
its own unclassified foreground-cohort metadata but is unsupported by this first
cut, not Linux-compatible by virtue of Unix transport. Windows acquisition is
unsupported. Server-reported Linux metadata is not authenticated platform
attestation, live-process attestation, a complete process tree/host catalog or a
native process-generation guarantee. Endpoint inode checks are not authentication.

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

## Collection Bounds

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

Both inventory brackets must preserve selected pane/terminal/workspace/tab
mappings. Endpoint selection/device/inode checks fence reads and their replies.
Any endpoint, mapping, capability, schema, budget, timeout or partial-cohort
failure discards **all** collected targets, not just the failed pane. Failed
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
[evidence guide](../kitsune/DETECTOR_EVIDENCE_SEPARATION.md). Historical/screen-only
data cannot mint ownership, takeover or native effectiveness. Restored Idle/name/
reference/pending resume/managed Active remain, but `interactive_ready` needs a
matching probe or accepted fallback, not foreground-only readiness or native proof.
Native input's independent fresh process check remains. This is deliberate public
semantic hardening, not blanket compatibility. No new Config, wire/API/snapshot
fields or external mode ship; Coven's reader/accepted capabilities are unchanged.
Scoped validation is complete: **1,318 distinct Kit checks**, separate **50 Coven
cross-compatibility checks**, and strict Kit all-target lint passed. Final
state/API/headless/lint confirmation reruns are in progress after the latest private
suspension flag; state 142 already agent-passed. The
[state-seam table and limits](AGENT_MANAGEMENT_MIGRATION.md#state-seam-validation-table)
record ENOSPC-aborted workspace CLI selection, a zero-match tab filter and
independently reproduced stale golden/test-only corrections, not full-suite or
production UI behavior changes beyond intended evidence hardening.
Do not use an ephemeral family hint as owner confirmation. Status/attention,
mobile/navigation, start/resume and agent-aware restore remain separate transfers.
All five prior Linux hook/configuration retirements stay unchanged; matching
non-Linux discovery and broader management capabilities remain pending. The
[private hook-file backend](HOOK_FILE_BACKEND_CONTRACT.md) now source-enables
provisional eligible-local-APFS macOS apply for all five file managers; this does
not change discovery's explicit macOS rejection or qualify receiver privacy,
full status/start/resume/restore, or any non-Linux Kit retirement. See the authoritative
[migration ledger and validation](AGENT_MANAGEMENT_MIGRATION.md).

## Validation And Files

Historical discovery scope passed **201 focused Coven checks**: 5 classifier, 8 collector,
16 watch/presentation unit, 91 CLI, 15 discovery workflow, 14 presentation workflow,
2 package-boundary and 50 legacy observation/inspection/watch/presenter checks.
Kit passed **249 API checks**, including capability/schema freshness coverage.
Strict all-target lint passed in both apps. Coven also passed Rust 1.85 all-target
checking, optimized build, package verification and packaged test-target compilation.

Review fixed queued selection loss, source-package fixture omission, protocol
admission, nullable-field handling and typed negative CLI operand handling. Actual
Kit intent-queue semantics are modeled: fresh observed scans poll the displayed
revision before replacement; failed scans clear positives before polling.
No installed-native/provider interactive behavior or macOS/Windows operation is
certified. Full `just check` remains unapproved/not run. Earlier OpenCode results
are historical and non-additive. No live installed Kit configuration was changed.

Implementation: `src/agent_discovery.rs`, `src/agent_discovery/{schema,tests}.rs`,
`src/agent_processes.rs`, `src/agent_processes/tests.rs`,
`src/agent_discovery_watch.rs`, `src/agent_discovery_watch/{presentation,tests}.rs`,
`src/main.rs`; workflow fixtures: `tests/agent_discovery_workflow.rs` and
`tests/agent_discovery_presentation.rs`. Kit capability producers live in
`src/platform/{linux,macos,windows,fallback}.rs`, `src/api/schema/session.rs` and
`src/app/api/session.rs` in the separate checkout. Sibling links are navigation,
not core dependencies.
