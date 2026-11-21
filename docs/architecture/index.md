# Architecture and contracts

Interfaces, data models, ownership and safety guarantees, grouped by domain.
Consult each document's scope, status and evidence limits: inclusion here does not
imply installed activation or all-platform qualification.
Current contracts take precedence over historical migration summaries.

## Contracts by domain

### App ownership and integration bootstrap

- [App boundaries](APP_BOUNDARIES.md): Coven, Kitsune and Djinn responsibilities
  and retirement/presentation boundaries.
- [Doctor status](DOCTOR_STATUS_CONTRACT.md): read-only build identity, defaults
  and versioned startup capability negotiation.
- [Integration setup](INTEGRATION_SETUP_CONTRACT.md): explicit setup previews,
  managed configuration, private bootstrap and guarded undo.
- [Provider environment routes](PROVIDER_ENVIRONMENT_ROUTES.md): explicit
  inherited reporting routes, receipt versions and fixed-route handoff.

### Agent evidence, storage and navigation

- [Agent discovery](AGENT_DISCOVERY_CONTRACT.md): ephemeral provider-family
  recognition over bounded generic foreground-process evidence.
- [Agent receipt management](AGENT_MANAGEMENT_CONTRACT.md): private receiver,
  persisted references, redacted inventory and separate normal observation paths.
- [Agent private storage](AGENT_PRIVATE_STORAGE_CONTRACT.md): descriptor-rooted
  stores, receiver locks, Unix socket privacy and platform admission.
- [Native attachment](NATIVE_ATTACHMENT_CONTRACT.md): selected-root Djinn
  display associations, captured targets, producer leases and ambiguity handling.
- [Native hook facts](NATIVE_HOOK_FACTS_CONTRACT.md): unordered Codex/Copilot
  facts, bounded freshness and inert manual hook planning.
- [Native provider events](NATIVE_PROVIDER_EVENTS_CONTRACT.md): independent
  Pi/OpenCode activity assets, closed events and source qualification limits.
- [Checked agent navigation](AGENT_NAVIGATION_CONTRACT.md): occurrence
  acknowledgements, temporary assertions and generation-fenced CLI focus.

### Provider configuration and file mutation

- [Hook-file backend](HOOK_FILE_BACKEND_CONTRACT.md): shared private mutation
  primitives, filesystem admission, recovery and platform gates.
- [Claude hooks](CLAUDE_HOOKS_CONTRACT.md): explicit settings management,
  generated capture commands and guarded migration/rollback.
- [Codex hooks](CODEX_HOOKS_CONTRACT.md): two-file hook/TOML management,
  shared enablement consent and reference-only startup capture.
- [Copilot hooks](COPILOT_HOOKS_CONTRACT.md): strict-JSON global settings
  management and exact SessionStart reference capture.
- [Pi reference extension](PI_EXTENSION_CONTRACT.md): explicit-root reference
  asset placement, capture envelope and frozen legacy migration.
- [OpenCode reference extension](OPENCODE_EXTENSION_CONTRACT.md): pinned
  server-instance reference plugin, root validation and two-leaf journaling.

### Observation adapters and participant groups

- [Djinn persisted observation](DJINN_OBSERVATION_CONTRACT.md): pinned folder
  evidence inspection, without live activity or execution authority.
- [Djinn live observation](DJINN_LIVE_OBSERVATION_CONTRACT.md): explicit private
  artifact reading for expiry-bounded legacy-runner busy/unknown evidence.
- [Kitsune observation](KITSUNE_OBSERVATION_CONTRACT.md): single-endpoint
  snapshot projection and its server-side housekeeping caveats.
- [Kitsune process observation](KITSUNE_PROCESS_OBSERVATION_CONTRACT.md):
  bracketed one-pane Djinn-family process evidence, not lifecycle detection.
- [Kitsune agent comparison](KITSUNE_AGENT_COMPARISON_CONTRACT.md): opt-in
  comparison of legacy hints with generic process evidence for characterization.
- [Participant groups](GROUPS_CONTRACT.md): declarations, explicit bindings,
  foreground annotations and optional expiry-scoped live publication.
- [Group live status](GROUP_LIVE_STATUS_CONTRACT.md): explicit live attachments
  and revision-scoped busy/unknown status.
- [Group presentation](GROUP_PRESENTATION_CONTRACT.md): Coven-owned content
  over generic Kitsune views, leases and read-only selections.

### Sessions and explicit native interaction

- [Session inspection](SESSION_INSPECTION_CONTRACT.md): read-only pane-target
  inventory and matching declarations after bounded generic observation.
- [Session views](SESSION_VIEWS_CONTRACT.md): pure group/member/recognition
  filtering with original observation counts preserved.
- [Session watch](SESSION_WATCH_CONTRACT.md): serial bounded refreshes,
  callback validation, failure clearing and summary semantics.
- [Pure resume planning](SESSION_RESUME_PLAN_CONTRACT.md): literal historical
  provider argv inspection without executable lookup or launch authority.
- [Interactive sessions](INTERACTIVE_SESSION_CONTRACT.md): preview-first
  start/resume adapters and explicit pinned generic terminal submission.
- [Session prompts](SESSION_PROMPT_CONTRACT.md): explicit Djinn-native prompt
  admission, runtime pins and correlated receipts without retries.
- [Session recovery](SESSION_RECOVERY_CONTRACT.md): selected resume execution,
  durable before-send intent and conservative duplicate/reference vetoes.

### Workspace authority and preparation

- [Workspace records](WORKSPACE_RECORDS.md): managed paths, persisted record shapes,
  configuration authority and editable labels versus stable identity.
- [Snapshot storage](STORAGE_CONTRACT.md): local task authority, cooperative
  locking, atomic replacement and secondary projections.
- [Task ledger](TASK_LEDGER_CONTRACT.md): explicit task-only import,
  immutable frames, replay, committed-request receipts and recovery.
- [Checkpoint decisions](CHECKPOINT_CONTRACT.md): local content-bound decisions,
  snapshot CAS and approval invalidation without execution permission.
- [Execution intent](EXECUTION_INTENT_CONTRACT.md): preparation-only attempts,
  frozen intent, enrollment and pre-submission cancellation.
- [Task graphs](TASK_GRAPH_CONTRACT.md): reviewed DAGs, explicit v2 upgrade
  and graph-bound preparation, not scheduling or native admission.
- [Runtime launch](RUNTIME_LAUNCH_CONTRACT.md): reviewed unrestricted cold
  startup through frozen inputs and one durable single-use claim.
- [Repository registration](REPOSITORY_REGISTRATION_CONTRACT.md): immutable
  checkout/base/tree facts with optional bounded verification, not Git mutation.
- [Worker readiness](WORKER_READINESS_CONTRACT.md): help-only compatibility
  evidence and explicit unknown native guarantees.
- [Worker handoffs](HANDOFF_CONTRACT.md): structured unverified claims and
  pinned local collection without task-status changes or check verification.

### Browser observation, conversation sharing and attention

- [Coven Web contract](COVEN_WEB_CONTRACT.md): browser/API schemas, authentication,
  exact session binding and bounded resource/lifecycle rules.
- [Sharing implementation](WEB_SHARING_IMPLEMENTATION.md): opt-in normal-launch
  native discovery, selected-session grants and follow-up submission boundaries.
- [Sharing core](WEB_SHARING_CORE.md): observer conversation integration,
  bounded blocking jobs, submission receipts and attempt suppression.
- [Card/conversation navigation](CARD_CONVERSATION_NAVIGATION.md): exact verified
  root-card links, metadata-only provenance and ambiguity refusal.
- [Actionable attention](ACTIONABLE_ATTENTION_CONTRACT.md): native decision
  occurrence/meaning fences, explicit responses and opt-in background alerts.
- [Attention core](ATTENTION_CORE_INTERNAL.md): observer decision methods,
  private references, coverage bookkeeping and core submission safety.

## Reading the contracts

The contracts distinguish observation evidence and display leases from native
identity, execution permission and durable workspace authority. Follow the relevant
domain's contract links rather than treating a displayed row, retained receipt,
checkpoint approval or prepared task as permission for another operation.

## Historical and platform limits

Historical Python interfaces, source pins, commands and validation details remain
in the relocated documents. Their presence is not a new compatibility or native
runtime certification claim. Check current scope and platform limits before use.

## Suggested reading order

For normal reporting, use the [normal workflow](../guides/NORMAL_AGENT_WORKFLOW.md),
then doctor/setup and provider contracts above. For implementation review, read
app boundaries, the domain contract, and the separately dated
[adoption status](../changelog/FEATURE_ADOPTION_STATUS.md).

## Related documentation

- [Documentation entry point](../index.md)
- [Active plans and migration gates](../plans/index.md): roadmaps, deferred work,
  recovery obligations and historical evidence, not standing authorization.
- [Adoption status](../changelog/FEATURE_ADOPTION_STATUS.md): delivered scopes
  and recorded qualification limits.
- [Changelog](../changelog/index.md): historical characterization and validation.
