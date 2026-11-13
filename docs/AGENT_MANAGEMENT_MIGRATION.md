# Agent management migration: ownership and implementation record

## Overview

### Current normal-invocation cut — 2026-11-12

[Normal workflow](NORMAL_AGENT_WORKFLOW.md) implements explicit startup opt-in:
Kit owns a server-only attached helper, Coven owns attributed plain observation
rows, and native agents are invoked normally without a launcher, group or artifact
join. External mode suppresses all legacy agent-status authority/detectors and
automatic resume even if the helper fails; invalid/unreadable startup ownership
configuration fails closed, not back to legacy restore. No automatic installation,
helper restart or live config handoff occurs. Restart Kit, create new panes and
reload providers for runtime changes/rollback. Each concurrent runtime needs its
own initialized, dedicated external workspace and receiver socket. Existing hooks'
explicit socket pins must match the receiver. Installed user config is not rewritten.
Djinn's updated full-TUI source plugin reports bounded volatile native events;
Claude has bounded last-reported activity, and Codex/Copilot/Pi/OpenCode remain
reference-only unknown. Facts expire within 30 seconds independently of the
five-second presentation lease; no pane/PID binding or native attestation follows.
The matrix below retains legacy/default-mode and platform transfer obligations,
not a claim that external mode still runs legacy ownership. Full extraction and
native macOS qualification remain incomplete.

**Confirmed scope: all agent management on all platforms moves to Coven**, including discovery and
status interpretation, hooks/installers, start/resume, and agent-aware restore.
Kitsune retains generic multiplexer operations and optional presentation/messaging;
native runtimes retain execution, policy, credentials and native facts. This is not
a Claude-only target or a proposal to keep management permanently in Kitsune.

The first functional cut implements Coven's `agent_management`, `claude_hooks` and
`agent_management_presentation` modules and retires Kitsune's **new Claude hook
installation on Linux only**. Non-Linux Kitsune installation/recommendation/update
paths remain until matching Coven platform adapters are ready. This does **not**
complete Claude management or all-provider/all-platform extraction.
The subsequent Codex cut adds `codex_hooks` and reference-only `SessionStart`
capture through the same receiver/inventory, retiring new Kit Codex installs,
recommendations and update/bulk candidates **on Linux only**, before provider paths.
It manages both `hooks.json` and `config.toml`, including explicit consent to enable
shared global native hooks, not native trust approval or operational certification.
See [the Codex contract](CODEX_HOOKS_CONTRACT.md). Claude behavior/v2 receipts/errors/
statuses remain unchanged after extracting the shared private `hook_files` engine.
This document is the authoritative migration ownership matrix and next-transfer
queue; older characterization documents remain scoped historical evidence.

The subsequent [Copilot cut](COPILOT_HOOKS_CONTRACT.md) completes scoped new Linux
hook-configuration ownership transfer for global `settings.json`, direct
`hooks.SessionStart[]` command handlers and reference-only startup receipts.
Linux Kit new-install/recommendation/update/bulk paths are retired; non-Linux Kit
paths and all-platform detection/start/resume/restore/status/uninstall/old ingress
remain. Native `github/docs` pin `7c3387d3b513ed43ea26c9b2e4c0e1b12234c010` is
documentation-format evidence only, not native implementation/binary certification.
Native JSONC is deliberately refused: Coven manages strict duplicate-free JSON
only. Copilot shares Claude's single-file v2 runner with per-provider artifact/error
namespaces; Codex retains its separate v1 two-file journal. Native approvals,
sandbox, trust and disable settings are preserved, never granted or changed.

## API / Interfaces

### Shared Linux/macOS File Backend

The [private hook-file guide and platform matrix](HOOK_FILE_BACKEND_CONTRACT.md)
supersedes earlier Linux-only mutation/adapter-absent descriptions, not the
Linux-only Kitsune retirement boundary. All five providers now share source-enabled
Linux/macOS apply through three runners. macOS explicit `--apply` is provisional,
eligible local APFS only and **not natively qualified**. Linux admission and all
receipt versions/native pins/template postimages stay unchanged. Linux and macOS
shared behavior is the current target, not a deferred port.

Every provider ledger row below describing a Linux ownership transfer is a
retirement statement, not denial of provisional macOS file mutation. All Kit
non-Linux installers/assets/status/uninstall/recommendation/update/bulk paths remain
until native qualification. Full status/start/resume/restore transfer remains
pending on both OSes. Generic macOS discovery is now source-enabled/provisional
under the exact v2 LP64 descriptor, not native runtime certification. Receiver,
store and socket privacy now use shared OS admission through `private_access.rs`,
with separate agent descriptor storage and socket-entry policies, not hook
transactions. macOS local-APFS support is provisional and natively unqualified;
see [private storage and endpoints](AGENT_PRIVATE_STORAGE_CONTRACT.md). Linux
receiver startup needs available/permitted `fchmodat2` (6.5 or backport), with no
pathname-following fallback. Other Unix receivers refuse before binding; generic
storage is unchanged and Windows unsupported.

The subsequent [Pi extension cut](PI_EXTENSION_CONTRACT.md) transfers new Linux
reference-extension placement, not full Pi management. `agent extensions`
plan/install/uninstall/rollback uses an explicit existing absolute root and a
separate private v1 two-leaf journal. `report-extension --provider pi --event
session_start` accepts only Coven's exact three-field envelope; `agent report`
remains native JSON ingress for Claude/Codex/Copilot only. Migration explicitly
**drops working/blocked/idle reporting to reference-only capture**. File placement
is not native activation proof, and `hasUI` includes TUI/RPC, not root/TUI proof.
No headless capture, approval/policy change or full Pi transfer is claimed.

The subsequent [OpenCode reference-plugin cut](OPENCODE_EXTENSION_CONTRACT.md)
transfers new Linux explicit existing `plugin` or `plugins` root placement, not
full management. It uses the shared private fixed-two-leaf v1 journal engine with
OpenCode namespaces; Pi API/receipt-v1/generated bytes remain frozen at `887451b`
with [independent compatibility fixtures](tests/pi_extension_frozen_compatibility.rs).
OpenCode's separate strict `coven.opencode-extension.v1` envelope accepts exactly
three strings and `session.created`/`session.updated`; required request event must
equal payload event. Provider/event denotes interpretation, not authenticated origin.
Native `agent report` remains Claude/Codex/Copilot only. **Migration drops working/
blocked/idle to references only**, always unknown. This is a functional reduction,
not full OpenCode management completion.

| Surface | Implemented ownership now | Remaining transfer / target owner |
| --- | --- | --- |
| New Claude hook configuration — Linux | Coven plans/installs/uninstalls/rolls back explicit settings roots; Linux Kitsune refuses new installs with Coven guidance. | Coven is sole owner of new Linux hook configuration; existing roots require explicit opt-in migration. |
| New Claude hook configuration — non-Linux | macOS file apply is source-enabled, provisional eligible local APFS only; Windows unsupported, other Unix preview only. Kitsune preserves production `legacy_install_claude`, assets/result type and recommendation/update/bulk paths. | Native macOS qualification before retirement; do not drop a working path. |
| Claude hook receipts and inventory | Coven private Linux/macOS receiver/store and redacted last-reported inventory, independent of Kitsune; macOS provisional and natively unqualified, other Unix refused, Windows unsupported. | Broader discovery, native association and status capabilities belong in Coven; receipts are not native truth. |
| New Codex hook configuration — Linux | Coven two-file preview/install/uninstall/rollback; Linux Kit refuses new install and excludes recommendation/update/bulk paths. | Explicit legacy migration only; global hooks enablement is not native approval. |
| New Codex hook configuration — non-Linux | macOS provisional local-APFS file apply is source-enabled; Kit legacy installer/assets/result type and recommendation/update/bulk remain. Windows unsupported; other Unix preview only. | Native qualification before retirement, not backend existence alone. |
| Codex receipts and inventory | Same Linux/macOS receiver, reference-only SessionStart capture; always unknown, even fresh. | No working/idle lifecycle or native association is inferred. |
| New Copilot hook configuration - Linux | Coven explicit global settings.json plan/install/uninstall/rollback and exact opt-in legacy migration; Kit install/recommend/update/bulk paths retired. | Scoped new hook ownership complete, not broader management or native certification. |
| New Copilot hook configuration - non-Linux | macOS provisional local-APFS file apply is source-enabled; Kit legacy installer/assets/result type and recommendation/update/bulk remain. Windows unsupported; other Unix preview only. | Native qualification before retirement. |
| Copilot receipts and inventory | Same Linux/macOS receiver, exact SessionStart reference capture; unknown even fresh, no Claude subagent suppression. | No inferred events, camelCase aliases or native lifecycle/association. |
| New Pi extension placement - Linux | Coven explicit existing-root plan/install/uninstall/rollback; Kit new install/recommend/update/bulk paths retired. | File placement only, not native activation or lifecycle parity. |
| New Pi extension placement - non-Linux | macOS provisional local-APFS file apply is source-enabled; Kit preserves full v7 asset/installer/result type and recommendation/update/bulk paths. Windows unsupported; other Unix preview only. | Native qualification before retirement; placement is not activation. |
| Pi receipts and inventory | Same Linux/macOS receiver, strict `coven.pi-extension.v1` envelope via separate report-extension; `session_start` reference only, always unknown. | Discovery/status/lifecycle/start/resume/restore remain pending; upgrade receiver first. |
| New OpenCode reference plugin - Linux | Coven explicit existing-root plan/install/uninstall/rollback; Kit new install/recommend/update/bulk retired. | Placement only; exact v9 opt-in migration, no native activation/lifecycle parity. |
| New OpenCode reference plugin - non-Linux | macOS provisional local-APFS file apply is source-enabled; Kit preserves full v9 asset/legacy installer/result and recommendation/update/bulk paths. Windows unsupported; other Unix preview only. | Native qualification before retirement; placement is not activation. |
| OpenCode receipts and inventory | Same Linux/macOS receiver, strict extension envelope, session.created/updated root DTO references only, unknown even fresh. | Server exact-directory-instance scope, not selected TUI/pane/attach or whole project; no universal resume capture. |
| Optional five-provider inventory presentation | Coven projects its own records with dynamic provider labels through generic Kitsune snapshot + `integration.view.*`. | Kitsune remains agent-neutral; no management authority comes from row selection. |
| Legacy Claude surfaces on all platforms | Kitsune retains all six `IntegrationTarget::ALL` entries, installation status, uninstall and old hook ingress. Non-Linux new installation also remains. | Retire only after platform-matched compatibility/transition coverage; running sessions are not converted by editing settings. |
| Generic Linux/macOS foreground-cohort recognition (all 22 literal families) | Coven production `agent discover`, bounded watch and optional generic presentation; ephemeral PID/provider/rule/locator evidence only, no receipt/group join. Linux v1 and provisional macOS v2 with exact `argument_policy: darwin_procargs2_lp64_v1` are admitted; both require `pane_foreground_cohort`/`unclassified`, protocol 18 unchanged. | Partial scope, not detector retirement/full status or native macOS certification. Linux optional argument_policy is ignored; macOS v1 rejects even with the marker. Windows selected_process/legacy_agent_aware rejects. Acquisition completeness stays unknown. See [discovery contract](AGENT_DISCOVERY_CONTRACT.md). |
| Process/screen fallback, status/attention, navigation, agent APIs and mobile surfaces | Legacy/default-mode compatibility remains. External mode suppresses legacy agent-status authority/detectors; Coven supplies plain integration rows, not the old widgets/focus/mobile model. | Broader adapters and removal gates remain; external mode is implemented, not universal surface replacement. |
| Interactive start/resume | Coven's explicit Djinn UI start/resume uses generic terminal-pinned submission. Kitsune's legacy provider paths remain. | Coven adapters for remaining supported providers; unsupported capabilities stay explicit. |
| Resume policy | Coven pure planner characterizes 16 historical mappings; no new execution authority. | Coven owns provider policy; pure argv equivalence alone cannot replace restore ownership/history semantics. |
| Agent-aware persistence/restore, native-reference ownership/deduplication and deferred launch | Legacy/default-mode compatibility remains; external mode retains history/references without automatic agent resume. | Broader Coven restore adapters remain pending; Kitsune retains generic terminal/session persistence and transport. |
| Other hook/provider integrations | Separate built-in Djinn handling and other applicable sources remain pending. Pi full-v7 and OpenCode full-v9 legacy ingress/lifecycle persist until explicit migration. | Coven-owned adapters/configuration with per-source removal gates, not permanent Kit ownership. |
| Jobs, participant roles and task authority | Coven declarations/task contracts; hook rows have no roles or task authority. | Coven orchestration, separate from native execution and generic presentation. |

### All-source/provider ledger

Every entry in Kitsune's current 22-kind classifier is in scope. These are coverage
groups, not promises that each kind has hooks or resume support:

| Provider group | Transfer status |
| --- | --- |
| Claude | New hook configuration transferred on Linux only; independent receipt inventory supports Linux/macOS, with macOS provisional and natively unqualified. Non-Linux installer/recommendation/update paths and all-platform process/screen fallback, start/resume and restore remain pending. |
| Djinn | Coven observation and explicit interactive/prompt capabilities exist; Kitsune's built-in detection, reporting, start/resume and restore remain pending extraction. |
| Codex | New two-file hook configuration transferred on Linux only; Unix receiver captures SessionStart references only. Independent Linux/provisional macOS literal-family discovery exists; non-Linux installer/recommendation/update paths and full discovery/status/start/resume/restore transfer remain pending. Legacy status/uninstall/old ingress remain. |
| GitHub Copilot | New global hook configuration transferred on Linux only; Unix exact SessionStart references stay unknown. Non-Linux installer/recommend/update/bulk and all-platform detection/status/start/resume/restore/old ingress/uninstall remain. |
| Pi | New Linux reference-extension placement transferred; strict Unix startup-reference capture only. Migration drops working/blocked/idle. Independent Linux/provisional macOS literal-family discovery exists; non-Linux paths and full discovery/lifecycle/status/start/resume/restore/legacy ingress/status/uninstall transfer remain. |
| OpenCode | New Linux reference-plugin placement transferred; strict Unix created/updated root-reference capture only. Migration drops working/blocked/idle. Non-Linux new-install paths, full v9 asset and all-platform legacy CLI/status/uninstall/old ingress remain; broader discovery/status/start/resume/restore pending. |
| Gemini, Cursor, Devin, Antigravity, Cline, OMP, Mastracode, Kimi, Kiro, Droid, Amp, Grok, Hermes, Kilo, QoderCLI, Maki | Independent Linux/provisional macOS literal-family discovery exists; broader discovery/status/start/resume/restore policy transfer pending, with no matching Kit detector retirement. |

Provider labels, hook sources and native references are distinct. Remaining source
owners include each legacy `kitsune:<provider>` producer and its configuration,
not only providers with an external installer. Do not infer hook support from
classifier membership or broad execution support from the resume-planner table.

The provider rows above describe legacy management transfer, not absence of Coven
recognition: [discovery v1](AGENT_DISCOVERY_CONTRACT.md) now recognizes all 22
literal families independently. This is executable-family coverage, not native
interactive/provider certification. Activity/attention remain unknown and native
identity/lifecycle/completion unestablished. It consumes no Kit agent APIs/hints,
stores nothing and joins neither receipts nor groups. Alias provenance is Kit
`src/detect/mod.rs:180-205` at `01d7cef6e98e2ccab216cd669d569c0731abfbce`;
the 209-case legacy process corpus comparison records intentional divergences,
not parity, job-leader selection or ranking.

**Historical pre-normal-workflow cut (before 2026-11-12), not current external-mode behavior:**
no matching detector retirement occurred in that cut; the internal typed foreground/fallback/history
seam is implemented in Kit. See the optional sibling
[evidence guide](../../kitsune/DETECTOR_EVIDENCE_SEPARATION.md). Historical/screen-only
data cannot mint ownership, hook takeover or effective native authority. Restored
Idle/name/reference/pending resume/managed Active remain; `interactive_ready` needs
a matching probe or accepted fallback, not foreground-only readiness or native proof.
Native input retains its independent fresh process check. This deliberately narrows
public semantics beyond a neutral refactor. That earlier seam added no external
mode, Config, wire/API/snapshot-format fields or toggle/kill/hide controls; the
generic acquisition cut separately adds the macOS v2 descriptor. This functional
Coven-owned recognition path temporarily duplicates legacy policy; it is not
completed sole ownership or full status. The five Linux hook/configuration
retirements remain unchanged; macOS native qualification and remaining non-Linux
capabilities, rather than the shared macOS file backend itself, remain pending.

## Data Flow

```text
explicit Claude/Copilot settings / Codex hooks.json + config.toml -> trusted Coven executable's agent report
  -> bounded native JSON projection -> private Coven receiver
  -> private persisted receipt records -> redacted inventory
  -> optional independent generic Kitsune view worker
```

Pi is a separate producer: explicit existing extension root -> Node spawn of the
selected Coven executable's `agent report-extension` -> strict Coven envelope ->
the same private receiver/store/inventory and optional view. No prompt/path/cwd or
event/context object is serialized; no native Pi JSON payload is invented.
OpenCode has its own generated JS producer and envelope on the same route, using
the authoritative canonical anomalyco/opencode release v1.19.5 commit
`810c0eba1a7348036f8bdf61e6632d0c36c6876c`; changed dev `611cee9` is unsupported.
Its named async factory returns event/dispose hooks in the native server's exact
directory instance. That server may be remote; executable/socket must exist there.
Full `properties.info.id` must equal `properties.sessionID`, with parentID only
undefined/omitted. Null/malformed/child DTOs reject, without cache/SDK reads or
enumeration. Conservative validation caps both summary.diffs and permission at
256 entries; created/updated/compacting times are nonnegative safe integers,
archived finite. Capacity is checked before DTO work. The per-factory FIFO holds
32 pending plus one active child, snapshots ID/event before async work, drops
incoming when full and never persists/deduplicates/retries. Quiet resumed sessions
need not emit either event. No installed-native operational certification is claimed.

Core capture does not start a native provider, daemon or Kitsune. Optional UI IO
waits for initialized receiver projection (readiness, not merely a bound socket).
Unavailable Kitsune does not stop receipt capture. No Kit agent API, pane binding,
launch, adoption or role assignment is part of this flow.

### Next transfers

1. Natively qualify the shared macOS local-APFS backend for Claude/Codex/Copilot/Pi/OpenCode,
   separately validate receiver/privacy/provider behavior, and supply remaining
   non-Linux capabilities before retiring Kitsune's preserved macOS/Windows paths. Move remaining
   provider hook/configuration ownership to explicit Coven adapters,
   with independent receivers where supported and conservative legacy migration.
   Separate Djinn and other applicable sources remain. Record exact source ownership and
   retirement behavior for each provider.
2. Build on Linux and source-enabled provisional macOS `agent discover`, without calling recognition detector
   retirement. Build on the implemented typed foreground/fallback/history seam
   and completed scoped API/UI/compatibility checks, retaining the limits below
    for broader retirement beyond the implemented opt-in external mode.
   Then move remaining status interpretation, process/screen fallback,
   attention/navigation and mobile agent model into Coven; supply missing generic evidence first,
   especially Windows tree scope/parent relationships. Do not mislabel today's
   agent-selected Windows singleton as neutral complete evidence.
3. Extend explicit Coven start/resume capabilities per provider using generic
   multiplexer controls or supported native APIs, with separate execution consent.
4. Transfer agent restore/reference ownership and dedupe/deferred-start policy,
   preserving generic Kitsune restoration. Characterize actual runtime import,
   duplicate traversal, rollback and spawn/send failures before retiring those paths.
5. Retire matching legacy ingress/APIs/UI/configuration only as replacements and
   compatibility gates are satisfied; validate supported platforms separately.

These are functional ownership transfers toward **all agent features on all
platforms**, not permanent compatibility ownership or an indefinite test-only backlog.
No scheduler, universal plugin host or core Cargo dependency between Coven/Kitsune
is needed; the generic view adds no native-effects ownership.

## Edge Cases

- A caller-supplied session ID is private reference data, not verified identity or
  resume permission. Receipt order is not native event order. Last-reported
   working/idle/ended for Claude is not current activity, readiness or job completion.
   Codex/Copilot accept only exact SessionStart; Pi only exact session_start in its
   separate envelope; OpenCode only exact session.created/updated in its own envelope.
   All four reference-only providers always report unknown. Provider + NUL + SID
   hashing preserves Claude IDs and separates providers; schema v1 record fields
   stay unchanged, with one aggregate 128-record cap and restart unknown/null age.
- Five providers share that cap and the same five store-v1 fields. Older binaries
  reject new Pi/OpenCode wire/store providers even with schema 1 unchanged:
  upgrade receiver first; storing those records limits
  downgrade. No automatic conversion/reset is supplied.
- Pi migration requires explicit `--migrate-kitsune` and exact v7 bytes/SHA-256
  `cce258cccccd2fdf66549cc89f1eee72b3293f558a7da96b8b87fe9071b3f247`.
  Its immutable original records new-leaf absence and legacy bytes/mode. Install
  writes new first then deletes old; rollback restores legacy first then removes
  new, including after uninstall. Normal uninstall never revives Kit. Bounded
  8 MiB journal/1 MiB leaves, reconstructed images and same-operation legal-prefix
  recovery are private-root safeguards, not global CAS or atomic activation.
  Stop providers/watchers before mutation; no autoload or live authority release.
- New configuration ownership does not release legacy in-memory pane authority,
  rewrite old running sessions or migrate user settings automatically. Hook
  migrations retain their old scripts; Pi/OpenCode explicitly delete only the verified
  legacy extension leaf. Normal Coven uninstall never restores old Kitsune references;
  explicit rollback can restore their exact preimage.
- OpenCode requires exact full-v9 SHA-256
  `7782f028d172b7b56b95aa364f443b8929446a40ce19a505e55aa8d8e405312b`
  plus explicit consent. Shared private two-leaf v1 journaling prevalidates both
  leaves/reconstructed journal before artifacts and reprepares under lock. Install
  writes new before deleting old; rollback restores legacy before deleting new,
  including after uninstall. Immutable original survives reinstall. Pending recovery
  permits only legal prefixes and the same pending operation; no journal paths or
  global CAS. Stop native servers/watchers before apply; restart the native server
  after manually chosen apply, not just TUI attach. No automatic restart or in-memory
  release. Do not double-install in plugin and plugins; no across-root arbitration.
- Copilot migration requires `--migrate-kitsune`, the exact v2 direct handler for
  `ROOT/hooks/kitsune-agent-state.sh` and SHA-256
  `40dee35dc22f1486267789e499deab23f382bd78ee92edd940f47e369421afbe`.
  The script is retained. Guarded rollback restores bytes/mode/absence; normal
  uninstall never revives Kit. Previews create no lock/files; actual single-file
  apply may create a persistent lock before validation. Do not generalize Codex's
  artifact-free prevalidation to Claude/Copilot apply.
- Hook apply is source-enabled on Linux and provisionally on eligible macOS local
  APFS; Kit installer retirement remains Linux-only. Receiver support is Linux/macOS only,
  Windows unsupported and macOS authorization/runtime unvalidated. The provisional
  backend is not native qualification or grounds to remove preserved Kit paths. No all-platform completion,
  full Claude transfer or all-provider certification is claimed.

### Validation record

#### Private storage and endpoint validation

The [private storage contract](AGENT_PRIVATE_STORAGE_CONTRACT.md#validation-and-gates)
records final parent evidence, not tests rerun by this documentation update:

| Focused Linux Rust Scope | Distinct Passed Checks |
| --- | --- |
| Agent management / hook files / generic storage / receipt-presentation units | 59 / 15 / 28 / 5 |
| Codex / Copilot / extension-engine units / CLI | 7 / 6 / 20 / 91 |
| **Unit/bin subtotal** | **231** |
| Management workflow / presentation / private / Codex capture / package integrations | 6 / 3 / 3 / 2 / 2 |
| Five generated end-to-end / five-provider config workflows / frozen Pi integrations | 5 / 57 / 2 |
| **Integration subtotal** | **80** |
| **Total** | **311** |

Agent-management 59 includes Files 14, Unix 24, Store 1 and existing 20. Hook files
15 includes one C-mock test; its 110 scenarios (33 FD + 46 socket + 31 parent),
child umask test prints and historical totals are not extra Rust checks.
Agent-reported ASan/UBSan mock checks ran on Linux, not native macOS ABI.
Strict Linux all-target lint with warnings denied, Rust 1.85 all-target checking,
Linux optimized build, locked package verification and packaged test-target
compilation all passed. Package assertions include actual private-access,
agent-Files and storage-Delivery sources, not broadened generic privacy policy.

Both Apple architecture `--tests` typechecks passed warning-free after the direct
serde dependency/lock update, importing actual Root/private access/agent Files/
storage Delivery/tests, not the full receiver/RPC Unix module. They do not check
Apple SDK C, link or run. No fresh full macOS application build succeeded; the
prior SDK `TargetConditionals.h` block remains. UID-0 ownership mutation was not
exercised; native macOS ACL tests were not run. Final read-only review is ready
within provisional scope with no remaining P0-P2 findings reported, not native
certification or a full-suite-green claim. Current teardown fixtures are clean;
production best-effort cleanup may still leave private secret-bearing temporaries.

The initial four-thread `storage::` run had **27 passed, 1 failed** in
`storage::bootstrap::tests::reservation_failure_matrix_preserves_exact_partials_without_adoption_or_lock_stealing`,
asserting `try_workspace_lock(probe)` at `src/storage/bootstrap/tests.rs:164`.
All **28 passed serially** afterward. Root cause remains unestablished, not proven
unrelated; generic storage logic changed only by pure Delivery-type extraction.
Earlier agent-reported serial workflows sporadically returned
`agent_delivery_uncertain`; final selected serial runs passed. Neither successful
reruns nor final counts establish a cause. No production timing relaxation or
resends were added. Historical full-library failures below remain recorded.

#### Historical hook-file backend validation table

The [historical backend validation table](HOOK_FILE_BACKEND_CONTRACT.md#validation-gates)
records the earlier backend cut, not fresh tests for the 311-check cut above or
tests rerun by this documentation update:

| Focused Linux Rust Scope | Distinct Passed Checks |
| --- | --- |
| Root/backend | 14, including one C-mock test with 33 scenarios |
| Codex / Copilot units | 7 / 6 |
| Shared extension-engine units | 20 |
| All five provider integrations | 64, including 2 frozen-Pi checks |
| `native_package` / CLI | 2 / 91 |
| **Total** | **204** |

Strict all-target lint on Linux, Rust 1.85 all-target checking, Linux optimized
build, locked package verification and packaged test-target compilation all passed.
The C-mock scenarios include source-backed early noowners rejection with zero
capability/ACL queries, not a native two-UID test; do not add 33 to the Rust total.

Both Apple Rust std targets are installed for 1.96.1. The development harness
imports actual Root/platform Rust and tests with locked libc 0.2.186; `--tests`
typechecks passed warning-free for Apple Silicon and Intel. It does not compile
Apple SDK C, link or run. Both full application target attempts failed on default
cc's Apple flags; an ARM retry with correct Clang targeting confirmed missing SDK
`TargetConditionals.h` in ring. Neither application builds nor native backend C
compilation passed. Native SDK ABI/linking, APFS and minimum-OS qualification,
and native receiver/store/socket privacy qualification remain pending, despite
the now-shared source-enabled admission described above.

| Incidental Broader Full-Library Run | Result |
| --- | --- |
| Aggregate | **1,144 passed, 2 failed, 5 ignored; not a pass** |
| Copilot capture projection | `agent_management::tests::copilot_capture_strips_native_fields_before_rpc_store_and_stdout`, `tests.rs:484`, unwrap of `agent_delivery_uncertain`; isolated exact rerun passed. |
| Extension capture projection | `agent_management::tests::extension_capture_projects_only_reference_fields_and_keeps_store_v1`, `tests.rs:318`, unwrap of `agent_delivery_uncertain`; isolated exact rerun passed. |
| Limits | No established root cause or proof the failures are unrelated; no production timeout relaxation or resends added. |

Parent review is ready without blockers within the provisional source scope,
not native certification or a full-suite-green claim. Exact physical-root admission
can safely refuse legacy commands using logical aliases; neither commands nor
user paths are automatically canonicalized. Kit macOS paths remain unretired.
Historical tables below, including 1,318 Kit seam checks, are not new Coven backend
results and are not added to this cut, nor are historical Coven 201/236 totals.

#### State-seam validation table

Current uncommitted Kit source includes `src/terminal/evidence.rs`,
`src/terminal/state/evidence.rs`, pane producer/helpers, App API/actions/headless
gates and both restore branches. These verified scoped agent/parent results were
not rerun by this docs-only update. Listed Kit module filters are disjoint.

| Scope | Reported record |
| --- | --- |
| Kit terminal state | 142 checks passed, including latest private suspension dirty flag |
| Kit pane | 108 checks passed, including 13 producer checks |
| Kit persistence | 58 checks passed |
| Kit App resume guards | 3 checks passed |
| Kit App actions | 95 checks passed |
| Kit `api::` | 264 checks passed, including App API 182 and 15 detector checks |
| Kit headless | 130 checks passed |
| Kit UI | 171 checks passed |
| Kit workspace pure unit | `workspace::tests` 12 plus aggregate 6 passed |
| Kit App input | 329 checks passed |
| Distinct Kit total | **1,318 checks passed** across the disjoint module filters above |
| Coven cross-compatibility | **50 checks passed**: discovery 15, presentation 14, legacy observer 21; separate from Kit total |
| Kit strict `just lint` | Passed across all targets, including final confirmation |
| Final confirmation | State 142, API 264, headless 130 and strict all-target lint reruns passed after the final suspension-history fix |

App API 182, detector subsets (including earlier 13), and producer 13 are included,
not extra checks. No new Coven production code: changes from root `9b36f6c` are
docs-only. Historical Coven 201/Kit 249 discovery results are not added to this cut.

Epochs contain a nonserialized value plus creation Instant. App admission uses the
actual bound runtime token and birth floor, never delivery time. Old queued events
cannot adopt a new runtime. Every actual positive probe publishes; screen/skipped
ticks and legacy `StateChanged` fallback cannot create/refresh proof. Retained
positive time/PGID/epoch are legacy-selected evidence (including env hints and
Windows selection), not native identity, continuous liveness or a new TTL.
Existing six-miss/hook scheduling remains. Reset preserves absence-policy context,
not proof; authoritative wake is separate. Clear is explicit without duplicate Idle.
Reset/sync suspend native authority to Unknown, preserving session/reference/sequence
barriers; admissible reports/replacement intent await fresh appropriate proof
without resend. Unchanged Idle reconfirmation cannot create Done/unseen/SFX;
genuine buffered completion and real PaneDied still complete once. Release uses
pre-release proof for fallback display then invalidates eligibility. Explicit
acceptance drives Idle-to-Idle runtime suppression without fake API events;
rejection plus epoch change does not count as release acceptance.

Latest P1 guard: admitted policy clear/conflicting foreground retires a full-lifecycle
native owner despite a newer unverified hook timestamp. Actual newer foreground
still blocks an older clear; the custom-authority timestamp exception remains.
Private pending-replacement `observed_at` preserves a recognized start received
after the old clear sample for the proper generation, without resend.
The private suspension dirty flag permits reconfirmation only with unchanged
source/reference/state and no accepted native state changes during that window.
Idle -> Reset -> Working -> Idle -> confirmation is genuine completion once;
repeated Idle stays quiet. Stale/foreign/rejected reports do not touch that flag.

Both restore branches seed history, not fabricated process observations. Runtime
import/resume submission is not proof; actor proof is never persisted. Snapshot
schema/capture bytes and Coven's reader/accepted capabilities were unchanged by
that historical seam; the current acquisition cut separately admits macOS v2.
Five Linux retirements and earlier generic metadata are preserved. No fresh
full-suite/live-provider/cross-platform result is claimed. Earlier discovery counts
are historical, not new seam totals or additive evidence.

#### State-seam validation limits

Broader `just test-one workspace::` selected CLI cases whose harness hardcodes
`/tmp`: seven failed ENOSPC with shared temporary storage full, before semantic
validation. Fail-fast left the rest unrun. Refined workspace pure unit checks
passed 18 (included above), not those CLI cases. `workspace::tab::tests` selected
zero tests, not a pass. No full `just check` result is claimed.

The desktop frame golden failure was reproduced in an owned detached temporary
worktree at unchanged Kit HEAD `fabc9f265e3a8faec86b17a0102e4dd649768902`: exact
actual digest `101b79bcc66557ea1049cd77cc79db9746be6ff236360e32e085e40e9c1159c4`
versus old expected `22dc5c...`. This proves the preexisting stale golden. Only the
test was updated, with neutral `No integrations configur...`/not-Agents assertions;
geometry/hyperlinks/cursor/mobile golden coverage remains. The clean worktree was
removed without force/staging/commits; ignored scratch target/build output remains.
Earlier headless title stale native-Agent expectations were corrected to the
existing neutral Integrations contract, preserving API title/no-event-flood checks.
No production UI behavior changed beyond intended evidence hardening. No native
provider, full readiness or cross-platform certification follows from these checks.

#### Discovery validation table

The current composed cut source-enables generic macOS acquisition and Coven
discovery admission; it does not certify native runtime behavior. Earlier Linux
discovery and evidence-seam counts are historical, not current composed totals.

| Scope | Evidence and limits |
| --- | --- |
| Coven admission | Source fixtures cover exact Linux v1/macOS v2 tuples, required macOS marker, rejected old macOS v1, descriptor drift and empty argv/name-only recognition. Linux's optional marker remains an ignored extension. |
| Kit generic acquisition | Decoder and fake-source fixtures cover reviewed LP64 layout, bounds, short/full reads, empty operands and visible member changes. `pane.process_info` uses the isolated generic route; legacy detector parser/hints remain unchanged. |
| Apple source checks | Actual Rust source checks for both Apple architectures are not SDK C compilation, native linking or runtime tests. Native validation remains blocked by missing SDK `TargetConditionals.h`. |
| Completeness and authority | `acquisition_completeness: unknown`; peer labels/PIDs are not local OS/PID authority. Mutable arguments, unreadable members and failed listing/argument reads can leave undetectable omissions. |
| This documentation update | Source/doc consistency inspection only; no builds, tests, live endpoint/provider use, configuration changes or substantial artifacts. No full-suite/native certification claim. Windows remains unsupported. |

See [discovery bounds, versioning and failure semantics](AGENT_DISCOVERY_CONTRACT.md).
Read requests may trigger Kit housekeeping/default-workspace/shell creation, so
read-only is not zero server effects. Earlier OpenCode 236 below remains historical,
unchanged and non-additive; it is not a discovery validation count.

Review corrections include polling queued selections before a fresh observed-frame
replacement (Kit replacement clears intents), clearing failed observations before
polling, packaging the frozen 209-case classifier comparison fixture, exact
protocol-18 admission, nullable native fields and numeric CLI lexer exclusions.
Limits remain explicit: recognition does not establish activity or native ownership,
and the coupled legacy detector has not been retired.

#### OpenCode validation table

| Scope | Current record |
| --- | --- |
| Shared fixed-two-leaf engine | **20 checks passed** across Pi/OpenCode policies. |
| Pi/OpenCode workflows | **14 checks passed**; frozen Pi `887451b` v1 completed/pending receipts and generated bytes have independent fixture coverage. |
| Generated OpenCode producer | **12 Node harness checks passed**, with the harness also passed under Bun; fake native callbacks, not installed OpenCode activation. |
| Kitsune retirement/retained compatibility | **111 integration checks** and **10 OpenCode checks** passed, overlapping filters, plus **6 legacy-asset harness checks**. Do not add overlapping counts. |
| Focused Rust receiver/CLI/provider/workflow/package regressions | **Historical OpenCode cut: 236 checks passed**; the shared 20 and workflow 14 above are subsets, not additive totals. Final integration-target run used one test thread; focused unit runs used four. |
| Lint/MSRV/build/package | Strict all-target lint in both apps, Coven Rust 1.85 all-target checking, optimized build, source-package verification and packaged test-target compilation passed. |
| Full suite | Full `just check` remains unapproved and was not run. |
| Installed-native operational capture, macOS/Windows and full management | Not certified or claimed. |

The Pi producer harness also passed again under Node (7 tests) and Bun. Review
fixed OpenCode's pre-capacity DTO work, bounded both native arrays to 256 entries,
and matched the pinned nonnegative-safe-integer codec. These are deliberately
conservative capture limits, not native-schema universality.

An initial four-thread integration run and a serial repeat failed in the existing
optional-presentation fixture with `agent_delivery_uncertain`. Its report remains
single-send; the fixture now requires raw inventory **and** private persisted-record
proof of the original effect even when an acknowledgement is uncertain. Received
acknowledgements still require the normal committed schema. The corrected fixture
and final selected integration run passed; the uncertainty branch did not recur
during diagnostic reruns, so no cause was established. No production deadline was
relaxed or report resent. These failures are recorded, not counted as passes.

#### Pi validation table

| Scope | Current record |
| --- | --- |
| Rust manager/receiver/CLI/workflow compatibility | **Historical Pi cut: 212 focused checks passed**, including its four provider paths, shared filesystem faults, strict capture/inventory, generated-command workflows, existing CLI and package boundaries. Socket suites used four test threads. Not additive current OpenCode validation. |
| Generated producer callback/spawn checks | **7 Node harness tests passed**; the same harness also passed under Bun. Actual generated TypeScript reached the real Coven receiver through a fake Pi callback. This is not native Pi activation evidence. |
| Kitsune Linux retirement and retained compatibility | **107 integration checks** and **9 `::pi_` checks** passed (overlapping filters, not additive). Non-Linux code paths retained, not executed or certified. |
| Lint/MSRV/build/package checks for Pi cut | Strict all-target lint in both apps; Coven Rust 1.85 all-target check, optimized build, package verification and packaged test-target compilation passed. |
| Live Pi activation/capture and cross-platform operation | Not claimed by this documentation cut. |

Development validation corrected an invalid race-test expectation (external
deletion that exactly matches a legal committed prefix must be resumable), stale
Kitsune Linux CLI expectations, its local integration fixtures' hardcoded `/tmp`
root (without lengthening shared named-session socket paths), and a Bun fixture
entrypoint limitation for a helper filename containing a backslash. Final focused
runs passed after those corrections. Earlier high-parallel receiver runs also
returned `agent_delivery_uncertain` in existing Copilot and new Pi socket tests;
four-thread and isolated reruns passed. A later Pi end-to-end repeat also observed
an uncertain additional-provider acknowledgement. That fixture now permits only
the documented uncertainty error while still requiring inventory and private-store
proof that all four original receipts arrived; it never resends. Uncommitted or
missing records still fail. This timing limitation remains recorded, with no
production timeout relaxation. Runtime selection was also made configurable rather
than hardcoding the validation host's Node path/major.

Earlier results below keep their original historical/Copilot scopes; they are not
new Pi/OpenCode validation or totals to add to the historical Pi 212-check scope.

Historical pre-Copilot scoped validation passed **176 Coven checks** covering both provider hook
managers, the receiver, shared file/journal safeguards, presentation, existing CLI
and selected package/interactive regressions. Generated Claude and Codex commands
reach the real private Coven receiver without Kitsune; tests verify redaction,
offline tolerance, provider-separated IDs and byte/mode-exact rollback.

Historical pre-Copilot Kitsune validation passed **330 focused checks**, including both Linux installer retirements.
Strict all-target lint passed in both apps. Coven also passed Rust 1.85 all-target
checking, optimized build and source-package verification. Receiver files explicitly
require `schema_version: 1`; unknown versions refuse startup rather than resetting
saved references.

Earlier Copilot-cut validation passed **188 focused Coven checks**: provider/native
policy, private receiver/projection, shared filesystem faults, all three hook
managers, three generated-command end-to-end workflows, existing CLI and package
boundaries. Kitsune's `integration` filter passed **103 checks** and its `copilot`
filter passed **8 checks** (overlapping selections, not 111 distinct checks).
Strict all-target lint passed in both applications; Coven also passed Rust 1.85
all-target checking, optimized build, source-package verification and packaged
test-target compilation. Package inventory includes the inert Copilot unit-test
asset and Apache-2.0 license; checkout-only integration suites remain excluded.

Generated **three-provider** commands reach the real Coven receiver end to end.
This is Coven command/transport integration, not native provider execution/delivery
or approval/sandbox certification. Review fixed the HTTPS requirement for HTTP
handlers using `allowedEnvVars` and a non-Linux test expectation. The earlier
176/330 totals above are historical results, not new Copilot counts.

One repeat of the Copilot end-to-end test returned `agent_delivery_uncertain`
during the additional-provider report; the initial run and three subsequent
isolated fresh-fixture runs passed. The cause was not established; this is an
intermittent receipt/deadline validation limitation, not a proven provider bug or
a clean uninterrupted run. No report resend or timeout relaxation was added.

In that historical validation, a first combined run failed because shared `/tmp`
was full. The affected checks
were rerun successfully using owned disk-backed workspace scratch; the failed run
is not treated as a pass. Native Codex trust approval and operational/provider-mode
certification remain separate from these fixture results.

Review fixes include private socket admission, startup interrupt cleanup, directory
publication durability, strict uncertainty handling, exclusive receiver leases,
descriptor-relative config traversal, displaced-image retention, permission
preimages, migration recovery verification, SGID stash normalization, and reserved
lease basenames. These are cooperating-local-filesystem protections, not global CAS
or protection against arbitrary hostile same-user writes through old descriptors.

Full `just check` remains unapproved/not run. Current scoped results are recorded
above, not a full-suite claim. No live provider, user configuration,
automatic install/migration or cross-platform result is claimed. Earlier real generic
bridge validation is separate evidence, not installed-app certification of this adapter.
The Claude fixture's Apache-2.0 license text is retained under
`tests/fixtures/claude_hooks/KITSUNE_LICENSE`.

## Example usage

Reference syntax only; choose inspected absolute paths, a trusted Coven binary,
an existing Coven workspace and an existing private socket parent (exact 0700):

```text
coven agent serve WORKSPACE --socket ABS_COVEN_SOCKET --duration 60 --json
coven agent inventory --socket ABS_COVEN_SOCKET --json
coven agent hooks plan --provider claude --config-root ABS_CONFIG_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --json
coven agent hooks plan --provider codex --config-root ABS_CONFIG_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --json
coven agent hooks plan --provider copilot --config-root ABS_CONFIG_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --json
coven agent extensions plan --provider pi --extension-root EXISTING_ABS_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --migrate-kitsune --json
coven agent extensions plan --provider opencode --extension-root EXISTING_ABS_ROOT --executable ABS_COVEN_BINARY --socket ABS_COVEN_SOCKET --migrate-kitsune --json
```

The receiver runs foreground and writes local receipt state. Hook plan is an
effect-free preview; install/uninstall/rollback also preview unless `--apply` is
explicit. Read the [Claude](CLAUDE_HOOKS_CONTRACT.md),
[Codex](CODEX_HOOKS_CONTRACT.md) or [Copilot](COPILOT_HOOKS_CONTRACT.md) hook contract
before applying. Copilot preserves native disable/approval/sandbox settings and
does not guarantee execution. Codex preview
discloses absent/false → true global hooks enablement and approval **not granted**.
Normal Codex uninstall leaves that shared flag enabled and never restores Kit
references; guarded rollback restores both original files, modes or absence.

## Related files

- [Agent discovery contract](AGENT_DISCOVERY_CONTRACT.md), `src/agent_discovery*`, `src/agent_processes*`, `src/main.rs`
- [Agent management contract](AGENT_MANAGEMENT_CONTRACT.md)
- [Pi extension contract](PI_EXTENSION_CONTRACT.md), `src/pi_extension.rs`, `src/pi_extension/`
- [OpenCode extension contract](OPENCODE_EXTENSION_CONTRACT.md), `src/opencode_extension.rs`, `src/opencode_extension/`, `src/extension_assets.rs`, `src/extension_assets/`
- [Claude hooks contract](CLAUDE_HOOKS_CONTRACT.md)
- [Copilot hooks contract](COPILOT_HOOKS_CONTRACT.md), `src/copilot_hooks.rs`, `src/copilot_hooks/{native,policy}.rs`
- [Codex hooks contract](CODEX_HOOKS_CONTRACT.md), `src/codex_hooks/{policy,native,unix}.rs`, `src/hook_files.rs`
- [App boundaries](APP_BOUNDARIES.md), [roadmap](ROADMAP.md),
  [coding roadmap](CODING_ORCHESTRATION_ROADMAP.md)
- [Extraction characterization](KITSUNE_EXTRACTION_CHARACTERIZATION.md),
  [resume planner](SESSION_RESUME_PLAN_CONTRACT.md)
- `src/agent_management.rs`, `src/agent_management/{store,unix}.rs`,
  `src/claude_hooks.rs`, `src/agent_management_presentation.rs`, `src/main.rs`
- Optional sibling navigation: [Kitsune migration guide](../kitsune/CLAUDE_HOOK_MIGRATION.md).
  Separate checkouts/apps remain independently useful; links are not dependencies.
