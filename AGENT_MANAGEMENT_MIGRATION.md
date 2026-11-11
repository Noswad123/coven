# Agent management migration: ownership and implementation record

## Overview

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

| Surface | Implemented ownership now | Remaining transfer / target owner |
| --- | --- | --- |
| New Claude hook configuration — Linux | Coven plans/installs/uninstalls/rolls back explicit settings roots; Linux Kitsune refuses new installs with Coven guidance. | Coven is sole owner of new Linux hook configuration; existing roots require explicit opt-in migration. |
| New Claude hook configuration — non-Linux | Kitsune preserves production `legacy_install_claude`, assets/result type and recommendation/update/bulk paths on macOS/Windows. Coven apply refuses unsupported platforms; Unix preview is not equivalent replacement. | Transfer to Coven only when matching platform adapters are ready; do not drop a working path. |
| Claude hook receipts and inventory | Coven private Unix receiver/store and redacted last-reported inventory, independent of Kitsune; Windows unsupported, macOS not validated. | Broader discovery, native association and status capabilities belong in Coven; receipts are not native truth. |
| New Codex hook configuration — Linux | Coven two-file preview/install/uninstall/rollback; Linux Kit refuses new install and excludes recommendation/update/bulk paths. | Explicit legacy migration only; global hooks enablement is not native approval. |
| New Codex hook configuration — non-Linux | Kitsune preserves legacy installer/assets/result type and recommendation/update/bulk paths until matching Coven adapters. | Unix preview is not replacement mutation support. |
| Codex receipts and inventory | Coven Unix reference-only SessionStart capture; always unknown, even fresh. | No working/idle lifecycle or native association is inferred. |
| New Copilot hook configuration - Linux | Coven explicit global settings.json plan/install/uninstall/rollback and exact opt-in legacy migration; Kit install/recommend/update/bulk paths retired. | Scoped new hook ownership complete, not broader management or native certification. |
| New Copilot hook configuration - non-Linux | Kit legacy installer/assets/result type and recommendation/update/bulk paths remain. | Matching Coven platform adapters before retirement; Unix preview is not replacement mutation support. |
| Copilot receipts and inventory | Coven Unix exact SessionStart reference capture; unknown even fresh, no Claude subagent suppression. | No inferred events, camelCase aliases or native lifecycle/association. |
| Optional Claude/Codex/Copilot inventory presentation | Coven projects its own records with dynamic provider labels through generic Kitsune snapshot + `integration.view.*`. | Kitsune remains agent-neutral; no management authority comes from row selection. |
| Legacy Claude surfaces on all platforms | Kitsune retains all six `IntegrationTarget::ALL` entries, installation status, uninstall and old hook ingress. Non-Linux new installation also remains. | Retire only after platform-matched compatibility/transition coverage; running sessions are not converted by editing settings. |
| Process/screen fallback, classification, status/attention, navigation, agent APIs and mobile surfaces | Still Kitsune compatibility, including Claude. Coven also has bounded generic Djinn-family observations and explicit groups. | Move interpretation/model to Coven for every source/provider; retire matching agent-specific Kitsune surfaces. |
| Interactive start/resume | Coven's explicit Djinn UI start/resume uses generic terminal-pinned submission. Kitsune's legacy provider paths remain. | Coven adapters for remaining supported providers; unsupported capabilities stay explicit. |
| Resume policy | Coven pure planner characterizes 16 historical mappings; no new execution authority. | Coven owns provider policy; pure argv equivalence alone cannot replace restore ownership/history semantics. |
| Agent-aware persistence/restore, native-reference ownership/deduplication and deferred launch | Still Kitsune. | Coven owns agent restore policy; Kitsune retains generic terminal/session persistence and transport. |
| Other hook/provider integrations | Pi and OpenCode legacy hook integrations are next pending; Djinn handling remains separate. | Coven-owned adapters/configuration with per-source removal gates. |
| Jobs, participant roles and task authority | Coven declarations/task contracts; hook rows have no roles or task authority. | Coven orchestration, separate from native execution and generic presentation. |

### All-source/provider ledger

Every entry in Kitsune's current 22-kind classifier is in scope. These are coverage
groups, not promises that each kind has hooks or resume support:

| Provider group | Transfer status |
| --- | --- |
| Claude | New hook configuration transferred on Linux only; independent receipt inventory is Unix-only, not macOS-validated. Non-Linux installer/recommendation/update paths and all-platform process/screen fallback, start/resume and restore remain pending. |
| Djinn | Coven observation and explicit interactive/prompt capabilities exist; Kitsune's built-in detection, reporting, start/resume and restore remain pending extraction. |
| Codex | New two-file hook configuration transferred on Linux only; Unix receiver captures SessionStart references only. Non-Linux installer/recommendation/update paths and all-platform discovery/status/start/resume/restore remain pending. Legacy status/uninstall/old ingress remain. |
| GitHub Copilot | New global hook configuration transferred on Linux only; Unix exact SessionStart references stay unknown. Non-Linux installer/recommend/update/bulk and all-platform detection/status/start/resume/restore/old ingress/uninstall remain. |
| Pi, OpenCode | Legacy hook/install integration ownership next pending, along with remaining management surfaces. |
| Gemini, Cursor, Devin, Antigravity, Cline, OMP, Mastracode, Kimi, Kiro, Droid, Amp, Grok, Hermes, Kilo, QoderCLI, Maki | Remaining applicable discovery/status/start/resume/restore policy pending; no transfer claimed here. |

Provider labels, hook sources and native references are distinct. Remaining source
owners include each legacy `kitsune:<provider>` producer and its configuration,
not only providers with an external installer. Do not infer hook support from
classifier membership or broad execution support from the resume-planner table.

## Data Flow

```text
explicit Claude/Copilot settings / Codex hooks.json + config.toml -> trusted Coven executable's agent report
  -> bounded native JSON projection -> private Coven receiver
  -> private persisted receipt records -> redacted inventory
  -> optional independent generic Kitsune view worker
```

Core capture does not start a native provider, daemon or Kitsune. Optional UI IO
waits for initialized receiver projection (readiness, not merely a bound socket).
Unavailable Kitsune does not stop receipt capture. No Kit agent API, pane binding,
launch, adoption or role assignment is part of this flow.

### Next transfers

1. Add matching Coven non-Linux Claude/Codex/Copilot platform adapters before retiring Kitsune's
   preserved macOS/Windows installer/recommendation/update paths. Move remaining
   provider hook/configuration ownership to explicit Coven adapters,
   with independent receivers where supported and conservative legacy migration.
   Pi/OpenCode hook ownership is next pending. Record exact source ownership and
   retirement behavior for each provider.
2. Move discovery/status interpretation, process/screen fallback, attention/navigation
   and mobile agent model into Coven; supply missing generic evidence first,
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
   Codex/Copilot accept only exact SessionStart and always report unknown. Provider + NUL + SID
   hashing preserves Claude IDs and separates providers; schema v1 record fields
   stay unchanged, with one aggregate 128-record cap and restart unknown/null age.
- New configuration ownership does not release legacy in-memory pane authority,
  rewrite old running sessions, remove the old script, or migrate user settings
  automatically. Normal Coven uninstall never restores old Kitsune references;
  explicit rollback can restore their exact preimage.
- Copilot migration requires `--migrate-kitsune`, the exact v2 direct handler for
  `ROOT/hooks/kitsune-agent-state.sh` and SHA-256
  `40dee35dc22f1486267789e499deab23f382bd78ee92edd940f47e369421afbe`.
  The script is retained. Guarded rollback restores bytes/mode/absence; normal
  uninstall never revives Kit. Previews create no lock/files; actual single-file
  apply may create a persistent lock before validation. Do not generalize Codex's
  artifact-free prevalidation to Claude/Copilot apply.
- Hook apply/Kit installer retirement are Linux-only; receiver transport is Unix-only,
  Windows unsupported and macOS unvalidated. Non-Linux preview/refusal is not a
  working replacement for the preserved Kit installer. No all-platform completion,
  full Claude transfer or all-provider certification is claimed.

### Validation record

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

Current Copilot-cut validation passed **188 focused Coven checks**: provider/native
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

Full `just check` remains unapproved/not run. No live provider, user configuration,
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

- [Agent management contract](AGENT_MANAGEMENT_CONTRACT.md)
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
