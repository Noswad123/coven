# Native Hook Facts V1

## Scope And Qualification

`coven.provider-facts.v1` is a separate volatile, caller-unverified fact
contract, not an ordered native-events profile. Command hooks do not supply
an ordered stream. No instance UUID, sequence, occurrence, decision count,
tool pairing, or current activity state is synthesized. RPC request IDs only
correlate one transport reply.

- `codex-hook-facts.v1`: source-qualified against openai/codex
  `0a94398fa60c4fac0e7659e4213a0a36e613dd46` (`rust-v0.167.1`), including
  `codex-rs/hooks/src/schema.rs`, `events/{model_request,stop,session_end}.rs`,
  `engine/{dispatcher,mod,command_runner}.rs` and `core/src/hook_runtime.rs`.
  This is not live binary certification.
- `copilot-hook-facts.v1`: documentation-qualified only against github/docs
  `7c3387d3b513ed43ea26c9b2e4c0e1b12234c010`,
  `content/copilot/reference/hooks-reference.md`. Not binary certified.

No providers, models, credentials, transcripts, or live endpoints are probed.
Absence of child metadata does not universally prove root identity. Explicit
non-null `agent_id`/`agent_type`, and Copilot `parent_agent_id`, exclude the
fact. Root eligibility and native identity remain unestablished.

## Capture CLI

```sh
coven agent report-fact --provider codex --event PreModelRequest --from-kitsune --best-effort
coven agent report-fact --provider copilot --event Stop --socket /absolute/private/receiver.sock --json
```

Stdin is one raw native JSON object, at most 1 MiB and depth 32, with duplicate
keys rejected at every level. Provider and exact PascalCase event selector
must match `hook_event_name`. CamelCase Copilot payloads are not accepted.
Exactly one endpoint is required. `--from-kitsune` uses existing private route
resolution, optionally with `--state-root`; it launches nothing.

Generated handlers use `--best-effort`, never `--json`. After argument parsing,
hook mode produces **zero stdout bytes**, exits zero even on report failure,
and emits only a fixed bounded stderr diagnostic. No retry or hook decision
output is produced. Operator mode instead reports failures with nonzero exit;
`--json` errors contain fixed codes, never source IDs. These modes conflict.

## Native Selection

Codex: `SessionStart`, `PreModelRequest`, `PostModelRequest`, `PreToolUse`,
`PostToolUse`, `Stop`, `Interrupt`, `SessionEnd`.

Copilot: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`,
`PostToolUseFailure`, `Stop`, `SessionEnd`.

Known keys are event-specific and closed. Documented optional native metadata
is allowed but dropped. Model names, permission modes, transcript paths,
prompts, assistant messages, tool names/inputs/responses/results, token usage,
and errors are never forwarded. Payload file paths are never opened.
`PermissionRequest` is unsupported: rule evaluation is not human-wait proof.

Normalized private RPC params contain `provider`, `profile`, `session_id`,
`event`, `correlation` and only the following event-specific fields:

- Codex turn-scoped events carry `native_turn_id`; tool events also carry
  `native_tool_id`. Correlation is `native_turn` or `native_tool`.
  SessionStart/SessionEnd have correlation `none` and no turn ID.
- Copilot correlation is `none`, with `source_timestamp_ms`, no turn/tool ID.
- SessionStart `source`: Codex startup/resume/clear/compact/fork; Copilot
  startup/resume/new.
- Codex PostModelRequest `status`: completed/failed/cancelled. This describes
  a model request, not job completion.
- SessionEnd `reason`: Codex other; Copilot complete/error/abort/timeout/user_exit.
  Copilot Stop carries reason end_turn.

The RPC envelope uses contract `coven.provider-facts.v1`, method `report`, and
these params, without an inner contract field. The normal receiver revalidates
the closed normalized schema. Acceptance is exactly
`{"type":"accepted","delivery":"volatile"}`. Frozen `agent serve` does not
accept this RPC; the normal `agent run`/integration receiver does.

## Time And Storage

At most 128 per-provider-session records each incorporate their source
watermark, with monotonic 600-second retention and fail-closed capacity.
No legacy receipt is changed. Public IDs hash a dedicated contract domain,
provider and native session ID. Raw session/turn/tool IDs are never public.

Codex has no source time. `last_received_event` means last arrival only;
`ordering` is `unordered`, `time_basis` is `local_receipt_only`, `source_time`
and `freshness` are always `unknown`. `age_ms` measures report arrival age,
not native event age. A newly received delayed fact is not called fresh.

Copilot accepts the explicit UTC ISO8601 subset
`YYYY-MM-DDTHH:MM:SS[.f|.ff|.fff]Z`, years 1970-9999. Calendar/leap dates are
validated; offsets and leap seconds are rejected. Source age at receipt must
be less than 30 seconds; future tolerance is at most 1000 ms. Expiry uses
source age at receipt plus monotonic elapsed, never a new full TTL on late
arrival. Time basis is `source_age_at_receipt_plus_monotonic`, source time is
`caller_supplied_utc`, not authenticated. Within future tolerance initial age
is zero, never negative.

The Copilot watermark is not native event order. Strictly older timestamps
make no change; equal timestamps never renew clocks. Different events at an
equal timestamp produce `ambiguous_same_timestamp`, not an arbitrary winner.
Wall rollback does not clear a newer watermark or manufacture a sequence.
Retention/watermark protection is bounded and lost on restart.

## Public Projection

Rows use origin `provider_native_fact`, capability `native_hook_fact`, state
`unknown`, native identity/job completion/root eligibility `unestablished`,
binding `none`, and caller-supplied unverified trust. Only closed
event/profile/order/time-basis/source-time enums and arrival age are added to
common public fields. No paths, prompts, errors, results, or native IDs appear.
Control reply validation rejects missing fields and substituted private extras
before constructing operator output; the presenter independently validates
fact rows and emits only bounded generic display fields.

Rows merge before the shared 128-row display/summary limit. Fresh native
attention keeps priority. Facts are not attention-pending and do not alter
acknowledgement behavior, ordered native clocks, or attachment leases.
Codex UI says `last received native hook fact: X; source age unknown`, not
working/idle. Copilot freshness qualifies only the fact's source time, never
current state. Stop can force continuation and does not establish settled idle.
SessionEnd is not a persistent deletion tombstone.

## Explicit Manual Handoff

```sh
coven agent native-hooks plan --provider codex --config-root /explicit/codex-root --executable /absolute/coven --from-kitsune --json
coven agent native-hooks plan --provider copilot --config-root /explicit/copilot-root --executable /absolute/coven --socket /absolute/private/receiver.sock --json
```

These are deterministic **inert snippet generators**, not managed installation
previews. They do not read/write the selected root, inspect policy, or attest
trust. `configuration_snippet` is for deliberate manual review/merge only.
Codex uses native matcher groups and shell-quoted absolute commands; Copilot
uses native PascalCase direct exec entries. No async flag is invented.

Managed native-hooks install/uninstall/rollback and `--apply` are **not
implemented**. Sharing hooks.json/config.toml or settings.json with exact-image
reference receipts needs independent versioned receipt/lock/recovery policy;
the reference installer engine is deliberately unchanged. Never manually edit
an image owned by `agent hooks` receipts. Use its explicit unchanged
uninstall/rollback workflow first and review the handoff, or use a fresh,
explicit trusted root. Never delete/invalidate its journal for adoption.

For manual setup, review the snippet hooks block for Codex `hooks.json` or
Copilot `settings.json`; preserve unrelated settings and hooks. Native
disable/trust/allow policy still applies. Codex may require explicit global
`features.hooks = true` consent and native approval; generation grants neither.
Do not automatically override disabled hooks or policy. Copilot sandbox policy
may prevent reaching the local receiver; report failure does not justify
widening permissions. This is local CLI configuration, not cloud-agent
integration or certification of real native event delivery.
