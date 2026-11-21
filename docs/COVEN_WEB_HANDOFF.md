# Coven Web: progress and integration handoff

## Complete supported titles deployed, 2026-11-21

Coven now forwards complete supported titles through 4096 UTF-8 bytes and renders
them on the third card line, below the short pane name and plain provider. It does
not slice titles or add ellipses; literal trailing dots remain intact. Title-only
bounds do not widen other display fields, wire response budgets, or authorization.
Invalid/oversized candidates within the wire profile have explicit unavailable
labels; the existing global wire guard can reject larger inputs before projection.
Firefox checks cover complete 4096-byte ASCII/emoji titles at 320px, 390px and
desktop widths without clipping or horizontal overflow.

Gateway and matching Megatron helper use immutable payload
`~/.local/libexec/coven-web/5e3b99d14e57a1b8/bin/coven`, SHA256
`5e3b99d14e57a1b8f39c2e5a80840dab22ba0b1350061e5ffbdf2a53e325dc64`.
Future dev Coven selection is `coven-native-rr9vz35i`. Private provenance and
rollout reports are retained beside the payload. Remote hash, x86-64 ABI, default
OpenSSL 3 loader, help and bounded-inventory checks passed. Candidate and live
loopback/Serve HTTPS health, assets and metadata passed. Both endpoints were
reachable/observed; the final sample had one local agent with a manual pane name
and zero remote agents. These are dynamic counts, not a guarantee of coverage.

For paired rollback, atomically restore both exact private backups before reloading
the user manager and restarting only `coven-web.service`:

- `~/.config/systemd/user/coven-web.service.full-title-rollback-20261121T092445-5e3b99d14e57a1b8`
- `~/.config/coven-web/observer.json.full-title-rollback-20261121T092445-5e3b99d14e57a1b8`

They restore the previous paired gateway/helper pin `75bb7f3a5b8b0cc5`; old
payloads remain retained. ACLs, native configuration, Serve, push key and the one
phone subscription were preserved. No live message, decision or push was sent.

Local Djinn future launches now select managed UI bundle
`cfc7e72931097d77b6ab5e3339247e084a6116b78260173e372a1cb5bb8e75af`.
Named sessions pass their complete stored title to the supported renderer API,
without the old `OC | ` prefix or 40-character formatter. The native CLI is
byte-identical (SHA256
`4d7f8f1ebd49cab7f16eb145185eda18d9d53281521060856ac4c560ee9c98b4`).
The preceding matched bundle
`08a998a3848af9d86eb4882604969787ff5bea49826d3ec4b66e2f19c1f9d106`
remains intact for an atomic `.djinn/current` selection rollback. Existing
dependencies and the models snapshot were reused without installation/downloads.

Only the gateway restarted. Running Djinn/Kit/native processes were not signaled,
and Megatron's native UI was not updated. The active local metadata still emitted
the old 45-byte decorated/truncated title: discarded suffixes cannot be recovered
by the website. Finish current native work before deliberately relaunching the
updated local UI. Megatron needs its normal native update separately. OpenTUI
0.4.5 still limits native title transport to roughly 1019 UTF-8 bytes; the 4096-byte
Coven contract is not an end-to-end native title guarantee. No raw OSC/private API
workaround was introduced.

Validation passed: 135 Node tests (real Firefox, no skips), 156 observer tests,
eight gateway tests (five opt-ins ignored), two packaging tests, offline locked
all-target check/release build, formatting and whitespace. Native validation passed
234 TUI tests (one skipped), package typecheck and compiled/staged capability
probes. The broader library suite was not rerun: earlier unrelated failures in
`djinn_observation::tests::controlled_real_producer_supervisor_and_source_independence`
and `integration_runtime::unix::tests::normal_slow_commit_acknowledges_after_commit_without_retry`
remain outside this passing focused gate. Physical-phone/interactive-native title
acceptance was not performed.

Source integrity was checked across both builds/publications. No source commits,
staging, pushes or worktrees were created. All owned disk-backed run/build/browser
artifacts were cleaned after child shutdown; the unattributed `pn.HC9lGV` directory
and other unowned data were left untouched. Source remains uncommitted on Coven
`2d1fb8d` and Djinn `c9398c4`.

## Pane name / provider / session-title hierarchy deployed, 2026-11-20

Card body order is now the explicit short pane name, plain provider, then the
existing long session title. Pane names come only from the exactly matched Kit
`panes[].label`, with the existing safe-display policy (160 UTF-8 bytes, no controls
or prohibited bidi/separator characters). Missing/invalid names display Unnamed
pane; titles, providers, CWD, tab/workspace labels and PIDs are not name fallbacks.
Chat/Details headings use the pane name. Long title cleanup is presentation-only,
and manual labels (including literal OC text) are preserved. Evidence, ambiguity,
project/team and coverage diagnostics remain in Details, not normal card text.

The additive helper DTO field defaults to None and is omitted when absent. The new
reader accepts older helpers. Reader/helper rollout was coordinated because older
strict readers cannot accept a present new field. All opaque IDs, binding proofs,
activity/output rules and decision/alert authorization remain independent of names.

Gateway and Megatron helper now use immutable prefix
`~/.local/libexec/coven-web/75bb7f3a5b8b0cc5/bin/coven`, SHA256
`75bb7f3a5b8b0cc5d229cb7b096f84a26fc5f3034c9289d4ea19003411cedcdc`.
The remote ABI/hash/default-loader probe passed with system OpenSSL 3. The old
`920bef44922edaf6` helper remains retained. Only the remote helper pin and gateway
ExecStart changed; restore both `observer.pre-pane-names.json` and
`coven-web.service.rollback-92d1b9c3` for paired rollback. Future dev Coven selection
is `coven-native-10x2s1ps`.

Live metadata reported one local agent with one manual label, and one remote agent
with no reported manual label. The latter falls back to Unnamed pane; the viewer
does not assert that another short tab/workspace title is its pane name. No pane
was renamed on the user's behalf.

133 Node tests including real Firefox, 153 observer tests, eight gateway tests,
two packaging tests, syntax/formatting/all-target check and release build passed.
Five producer opt-ins were not rerun for this presentation-only change. Candidate
and actual Serve HTTPS assets/metadata passed. Name rename/clear preserves exact
card bindings and routing. Only the gateway restarted; remote/native/Kit agents,
policy, Serve, VAPID key and the phone subscription were preserved. All owned
artifacts were cleaned, with no worktrees, commits or pushes created. Refresh the
phone page; no native restart or alert re-enablement is needed. Source remains
uncommitted on top of the user's `2d1fb8d` commit.

An unattributed 119 MiB disk-backed run directory,
`~/.cache/agent-test-runs/pn.HC9lGV`, was found at wrap-up. No readable active
process references were found, but its creator could not be confirmed (the final
validator's recorded run was a different, already-cleaned directory). It was left
untouched rather than deleted based on age or naming. Any later cleanup must first
identify its owner and repeat the active-use check; do not blanket-delete the cache.

## Redundant title prefix removed, 2026-11-20

Coven Web now removes one exact leading `OC | ` decoration from known Djinn
display titles in cards, chat headings and Details. Backing metadata, session names,
opaque IDs and bound navigation are unchanged. Other providers and non-leading
literal uses are preserved. Live metadata showed two decorated Djinn titles and
zero decorated captions after this presentation helper.

Gateway: `~/.local/libexec/coven-web/92d1b9c340d43598/bin/coven`, SHA256
`92d1b9c340d4359821d2fe4c5d2cbe88a41838746d11452b5659bad43b431ec4`.
The exact old unit is `~/.config/systemd/user/coven-web.service.rollback-3fde7b16`;
future dev selection is `coven-native-j4z66wpw`. 129 Node tests (including Firefox),
eight gateway and two package tests passed; five producer opt-ins were not rerun
for this presentation-only release. Real HTTPS assets/metadata checks passed.
The phone subscription/VAPID key, policy and running native agents were preserved.
Refresh the phone page; no native restart or alert re-enablement is required.

Djinn source at `c9398c4` additionally emits unadorned named-session/plugin titles
and uses Djinn rather than OpenCode for home/default titles. Existing truncation,
disable preference and stored session names are unchanged. TUI validation passed
231 tests with one skip and package typecheck. Existing fixtures warned about
hardcoded `/tmp/opencode/state/kv.json` reads; no data was created there. That native
source fix is uncommitted and was not built/published or activated in this slice;
it takes effect in a future native build. All owned test/build artifacts were
cleaned; no worktrees, commits or pushes were created.

## Quiet UI, header alerts toggle and latest-first positioning, 2026-11-20

The frontend polish is deployed. Routine verified-link/no-linked-request text,
healthy native metadata explanations, host-health chips and the static coverage
paragraph are removed. Actual errors, unavailable empty hosts, permission scope
warnings and uncertain-delivery notices remain visible only when relevant.

The old Private badge is now one accessible header alerts toggle, reflecting
on/off/unknown/checking without a healthy helper paragraph. Passive inspection and
opt-in/opt-out semantics are preserved; server-cleanup uncertainty has an exception-
only action. The existing phone subscription and VAPID key were unchanged.

Chat opens at latest content/reply controls, or at an explicitly opened request's
meaning. One outer message scroll context allows scrolling up for history without
metadata/late-response jumps. The sticky heading clearance is measured dynamically,
including long wrapped titles; no approval control is automatically focused. Scroll
effects recheck view, request, interaction and authorization generations.

Gateway: `~/.local/libexec/coven-web/3fde7b16ed6e4e65/bin/coven`, SHA256
`3fde7b16ed6e4e6553c36817b031bc2ae2247d5d468a07924b2e282ab64f2ce9`.
Future dev selection is `coven-native-tcf57kxp`. Exact previous unit backup:
`~/.config/systemd/user/coven-web.service.rollback-a01e9a08`.

127 Node tests including real Firefox passed at phone/desktop widths, with long
history, 320px header fit, both message/request response orders, wrapped permission
heading clearance, stale-callback and focus checks. Eight HTTP and two package
tests passed; five producer opt-ins were not rerun for this UI-only change. Syntax,
formatting, release build and actual Serve HTTPS asset/metadata checks passed.
Physical-phone keyboard behavior remains device acceptance, not a Firefox claim.

Only the gateway restarted. Native agents, backend contracts, access policy,
configuration, Serve, subscription bindings (one before/after) and private key
were preserved. No live prompt, decision or vendor test push was submitted. All
owned browser/build/test artifacts were cleaned; no worktrees, commits or pushes
were created. Source changes are in the main checkout at `b5c9ece`, uncommitted.
Refresh the phone page; no native relaunch or alert re-enablement is required.

## Actionable attention and background alerts deployed, 2026-11-20

This is the current feature record. Existing verified BASIC cards now support
native question/permission attention inside the same dialog. Follow-up messages
remain separate from answering questions or releasing permission waits. Supported
remote actions are question answer/reject and permission allow-once/session-pending
rejection; **never always allow**. Native policy and exact occurrence CAS remain
authoritative; alert dismissal is not a response.

The contract is [ACTIONABLE_ATTENTION_CONTRACT.md](ACTIONABLE_ATTENTION_CONTRACT.md).
It covers immutable bounded meaning, fresh occurrence IDs, shared native claims,
session-scope/projector/transaction fences, source decision ACLs, opaque refs,
pre-body CSRF/admission, uncertainty, private subscription state and advisory push.
Generic encrypted notifications contain only an opaque alert ID. The service worker
opens/focuses the private origin and reauthorizes that exact occurrence; it never
answers a request and caches no offline details/transcripts.

### Deployment and activation boundary

- Gateway: `~/.local/libexec/coven-web/a01e9a08dc2fb590/bin/coven`, SHA256
  `a01e9a08dc2fb5908655716875cc31c436b545866f8c9f08aaf8521e47e8bc35`.
  Future dev Coven payload is `coven-native-xer_v6tu`. System OpenSSL 3 loads without
  an LD_LIBRARY_PATH override; normal native-Linux dev installation now prefers
  validated system OpenSSL when no explicit selection/cross configuration exists.
- Future UI bundle:
  `93500f8ee43243f0fd6d37e375b2b77ee1554bcf48b2cff47b9ba55d57c76a69`.
  Native CLI bytes are unchanged. Previous bundles/payloads remain retained.
- `~/.config/djinn/coven-web.json` adds `decision_control:true`, preserving the
  dotfile-managed parent, read_write mode and registry. The existing sole owner is
  explicitly in local-djinn's new decision_logins, independently of submit_logins.
- Optional push is configured at `~/.local/state/coven-web/push`: directory 0700,
  private key/state/lock files owned, regular, 0600 and single-link. No keys or
  endpoints were printed or committed. Browser registration remains explicit opt-in.
- Only the gateway was restarted. Global login policy, two fleet endpoints, source
  sharing configuration, control workspace, wrappers, Arcana registry and Serve
  route were preserved except the explicitly added decision/push policy fields.

At final verification, push was configured with **zero browser subscriptions** and
attention was **unavailable**, not confirmed empty, for the older active native
worker. Real phone delivery and real native response acceptance are not claimed.
Finish current native prompts locally before a deliberate UI relaunch: pending
waits belong to the old process and cannot be adopted by a new UI. Then open the
shared Chat, refresh Coven Web on the phone, and explicitly Enable alerts/allow the
browser permission. Push collection is independent of an open browser page, but
requires a running gateway and eligible live native share. Unsupported/process-only
cards and unselected sessions do not gain decision authority or notification coverage.

Rollback backups (0600) are `~/.config/djinn/coven-web.pre-attention.json`,
`~/.config/coven-web/observer.pre-attention.json`, and
`~/.config/systemd/user/coven-web.service.rollback-2bc5cde0`. A native UI rollback
must also restore its startup config. Never reset unrelated Serve routes or restart
active agents as an implicit rollback action.

### Validation and cleanup

Main-source validation passed 121 Node checks (Firefox + service-worker VM), 12
installer tests, 258 independent focused Rust tests including all five explicit
producer opt-ins, and 416 unique native tests across 22 files with both package
typechecks. Tests include actual synthetic Question/Permission waits settled via
HTTP, scope/move/transaction races, no remote always, encrypted push decryption,
vendor/DNS-pinned TLS, subscription ownership, ledger/queue recovery and opt-out.
The management display-lease assertion needed serial reruns; this timing limitation
is recorded in manifests rather than claimed fixed. Clippy has only the five known
unchanged style warnings. Compiled network-denied startup probes advertised exact
capabilities before/after native publication with no selected actor or model work.
Candidate and actual Serve HTTPS assets, metadata, push-config and pre-body refusal
checks passed. No live approvals, native detail reads, browser subscriptions or
vendor pushes were performed by agents. Physical-phone delivery remains user acceptance.

All owned build/test/browser/IPC/audit artifacts were removed after child shutdown.
Four effort worktrees were integrated, preserved and removed without force. Recovery
stash objects remain until integrated source is committed and recovery is unnecessary:

| Removed worktree | Repository | Recovery stash object |
| --- | --- | --- |
| `coven-attention-core` | Coven | `8e4a312af215310e7e8fb546f3725d972ee12384` |
| `coven-attention-client` | Coven | `12d22387909aca7fa271f360d991e4393e756bd2` |
| `coven-attention-push` | Coven | `00d5ed98746083fa9031d6ce73600f2001f6cbb3` |
| `djinn-native-decisions` | Djinn | `5ca1ce31f59fb8e198be8511368339d85454e7ec` |

Original main checkouts remain at Coven `1e88ca2` and Djinn `db13571` with the
implementation uncommitted. Unrelated worktrees, branches and old recovery stashes
were preserved. No source commits or pushes were made.

## BASIC one-click UI and approved copy cleanup deployed, 2026-11-19

This is the current record and supersedes the picker/manual-choice fallback below.
Basic root cards now open their one exact verified conversation on the first click.
There is no picker, manual binding, Shared chats chooser or third native fallback
card. Unlinked, stale, ambiguous or unsupported observations show **Chat unavailable**
with Details separate; no identity is guessed to make the card look ready.

The user confirmed concurrent HTML edits and requested deletion of elements marked
"Delete This". Those paragraphs and the marked footer were removed. "Agent
Management", "Private", the shorter coverage text and the retained heading were
preserved. Orphan footer/paragraph CSS was removed and shell bottom padding supplies
phone/desktop breathing room without an empty footer.

The gateway now runs `~/.local/libexec/coven-web/2bc5cde074cbb2d3/bin/coven`, SHA256
`2bc5cde074cbb2d33c0eaee0f4e1cf6bb258f8d7c0ff5234d460228189db2a96`.
Future dev Coven selection is `coven-native-3ba7akbi`; the exact preceding unit is
`~/.config/systemd/user/coven-web.service.rollback-26e48daf`. Configuration, source
ACLs, Serve and native processes were not changed by the gateway deployment.

Native startup now has bounded read-only initial readiness/target-capture recovery
until its first pinned context. Unchanged heartbeats no longer starve slow valid
qualification; true selection/context changes still revoke immediately. Future UI
launches select bundle
`de541eb6dc173317ff1eec142b5448b3e78a0a4538110a6e05ede4a67e17be99`.
Its CLI bytes are unchanged; older bundles remain retained. These changes cannot
retrofit the old compiled worker. The last live metadata showed two roots, one
unlinked shared chat and zero verified links: one-click live reading is **not yet
established** until a deliberate updated-UI relaunch supplies provenance. No active
agent was restarted or signaled. Refreshing the phone loads the new UI contract;
it does not itself activate native matching.

Validation: 89 Node checks including real Firefox, eight gateway and two packaging
tests passed; three producer opt-ins were not rerun for this UI-only release.
Formatting, syntax, release build and candidate/live HTTPS asset/copy/metadata
checks passed. Native final combined scope passed 151 tests and typecheck; an
initial receipt-timing failure needed an unchanged-budget rerun and is not claimed
as a root-cause fix. The overlapping thread suite passed 11 tests again. Network-
denied compiled normal-startup checks passed before/after native publication with
automatic capabilities and no grant before selection, without dependency reification,
model calls or user chats. This is not real selected-session/phone chat acceptance.
All owned test/build/browser/helper artifacts were cleaned. No worktrees, source
commits or pushes were created; existing unrelated edits were preserved.

## Root-card conversation correction deployed, 2026-11-19

This supersedes the native-only root-card fallback below, which duplicated the
user's existing session card. Root cards now come **only from fleet observations**:
the live rollout's two observations plus one shared chat produce exactly two root
cards. Chat availability no longer appends a third tile.

Verified root links still open their pinned conversation directly. An unlinked
root opens an explicit conversation picker, even if there is only one candidate.
The user may choose an exact authorized ID/team for that root; subsequent clicks
reuse that page-local choice while valid. It is labeled as user-selected, not a
verified terminal association. Choices move rather than alias between roots, and
expiry, revocation, reference replacement, hiding or conflicting verified metadata
clears/fences them. A small Shared chats button preserves access without fleet
rows; it does not create another root card. No session is chosen by label/order.

Gateway artifact: `~/.local/libexec/coven-web/26e48daf7be7b788/bin/coven`, SHA256
`26e48daf7be7b7888fd57e132c896618576c6f240462b863031c7e1ddf29d981`.
Future dev selection is `coven-native-uvp_ofhc`. The previous unit is retained at
`~/.config/systemd/user/coven-web.service.rollback-1d0e4b56`.

Validation passed 85 Node tests, including installed Firefox real-DOM phone/desktop
coverage, eight gateway tests and two packaging tests. Three producer opt-ins were
not rerun for this frontend-only change. Syntax, formatting, release build and live
HTTPS assets/metadata checks passed. Only the gateway was restarted: **no Djinn
restart is needed for this interaction**, and no native/backend association was
forged. Source/configuration policies, Serve and active agents were preserved.
Owned test/build/browser artifacts were cleaned; no worktrees, commits or pushes
were created. Refresh the phone page to load the correction.

## Unified session cards deployed, 2026-11-19

The site now has one **Sessions** grid, shared Host/Team filters, and one refresh
flow. There is no separate Conversations section. Chat-capable cards open the exact
authorized conversation; verified associations merge the chat and fleet observation
into one card, with terminal details secondary. Unlinked chats remain usable as
clearly labeled native-only cards. Unsupported/ambiguous observations show honest
details rather than opening a guessed conversation.

The private association contract and gateway checks are documented in
[CARD_CONVERSATION_NAVIGATION.md](CARD_CONVERSATION_NAVIGATION.md). Matching requires
qualified native reporter/SID provenance, a current sharing grant, configured local
Kit endpoint identity, exact terminal/pane/workspace/tab mapping, live terminal
incarnation, bounded leases and bidirectional uniqueness. Pinned bound reads reject
stale clicks instead of following a newly selected session. Existing source ACLs,
CSRF, prompt admission and no-retry/uncertainty semantics are unchanged.

Gateway artifact:
`~/.local/libexec/coven-web/1d0e4b56d4631f4c/bin/coven`, SHA256
`1d0e4b56d4631f4c152356adf7db6419eacad0f28980bf5624274dccea0ce337`.
The exact previous unit is retained at
`~/.config/systemd/user/coven-web.service.rollback-8b1d3f7a`.
Future public dev Coven launches select `coven-native-v8qc2lx_`.

Future native launches select bundle
`e4112eb7ee01153a0d3fcc0c0ce52a1d90af910ea4853345d176b6dbd6868e5d`;
the CLI bytes are unchanged and the preceding bundle remains retained. A compiled
normal-TUI sandbox advertised the new optional association capability. Existing UI
processes were not restarted: the live rollout observed two fleet cards, one
unlinked writable native card and zero merged cards because the running UI lacked
that optional capability. Refreshing the phone immediately gives the unified grid;
a later deliberate Djinn restart enables provenance for merging its existing card.
No restart is required merely to use an unlinked native chat card. Remote native
association/chat, including Megatron, remains unsupported rather than inferred.

Validation: 76 Node tests; all 12 card-link tests within the scoped observer suite;
shared-native, prompt, conversation, HTTP, installation/package, and separately
invoked actual sharing/read-producer fixtures passed. All-target check, formatting
and release build passed. Clippy retained only the five known unchanged warnings.
Native final scoped validation passed 249 distinct tests and package typecheck.
Candidate/installed compiled startup and real gateway HTTPS asset/metadata/refusal
checks passed. Real-browser keyboard/visual and real merged-user-chat acceptance
were not performed. No transcripts or live prompts were used for rollout checks.

Only the web gateway was restarted; configuration, policy, Serve, remote helpers and
active native agents were preserved. All owned build/test artifacts were cleaned.
No new worktrees, commits or pushes were created; changes remain in the normal main
checkouts alongside the previously uncommitted implementation.

## Blank-entry presentation fix deployed, 2026-11-19

The gateway now runs `~/.local/libexec/coven-web/8b1d3f7aa9b7121f/bin/coven`
(SHA256 `8b1d3f7aa9b7121fc3a4c5872f7d626ce1895ea2de640d4b4f359dc8d3872420`).
The browser hides entries without displayable text, reports their count, preserves
real text and omission/truncation labels, and explains all-empty samples. This is
presentation-only; native projection/digest/API and sharing policy are unchanged.
Refresh the browser; **no Djinn restart is needed** for this fix. Only the gateway
was restarted, with an exact unit backup at
`~/.config/systemd/user/coven-web.service.rollback-e0857322`. Future dev Coven launches
select `coven-native-obx75bs4`; the previous payload is retained.

Validation: 56 Node tests, eight HTTP gateway tests and two native-package tests
passed; three opt-in gateway tests were not rerun for this frontend-only change.
Syntax, formatting, whitespace, offline release build, candidate and real Serve
HTTPS assets/health/metadata checks passed. Observer configuration and Serve were
unchanged; no live prompts/model calls or native restarts were performed. Owned
build/test artifacts were cleaned. No worktrees, source commits or pushes were
created for this fix.

## Normal-launch sharing and follow-ups deployed, 2026-11-19

This is the current record; the deployment and implementation overlays below are
historical. The previous read-only/manual-registration gap is now addressed for
normal **local** full-TUI launches after one explicit user-level enablement.

### User workflow and scope

1. Finish/save current work, exit the old UI, and relaunch Djinn normally on
   Bumblebee (`djinn --ui` through the existing dev selection). No per-launch socket
   variable or manual session/runtime ID is needed.
2. Open the desired root session in the local Chat view. Its hydrated selection
   publishes a short-lived native grant; leaving/deselecting expires/revokes it.
3. Reload the phone page at `https://bumblebee.tail97624b.ts.net` and use
   **Conversations**, not process-discovery fleet cards. Authorized writable
   conversations have a follow-up composer. Read refresh never sends a prompt.

The new setting is captured at worker startup. Existing processes were not
restarted/signaled and cannot gain the integration by rebuilding or changing the
setting alone. No live-chat or live-model acceptance is claimed until a real new
session is deliberately selected. Permission/question answering, remote native
adapters (including Megatron), mini/headless/explicit HTTP and full-history views
remain outside this slice. A follow-up is not approval of an outstanding request.

### Installed setup

- Gateway: `~/.local/libexec/coven-web/e085732203c643c5/bin/coven`, SHA256
  `e085732203c643c53c070bbca012181a9d548115479905d33c68c6782c050b71`.
  Future public dev Coven launches use `coven-native-vvurzumu`.
- Future native UI launches select bundle
  `f92f874abcea6255d269d19d0b1b3114dbdf737244e3bc70a8a8e45f15031078`
  under `~/.local/libexec/arcana-dev/djinn/bin/.djinn/`. The CLI bytes are unchanged.
- `~/.config/djinn/coven-web.json` is owned/0600, mode `read_write`, registry
  `/run/user/1000/coven-web` (0700). The dotfile-managed parent symlink is preserved;
  the loader validates/fences its trusted canonical directory and no-follow leaf.
- The observer has one `local-djinn` source for team `development`, with the
  existing sole owner as reader/writer. The control workspace is
  `~/.local/state/coven-web/control` (0700), initialized blank without launching
  any agent, provider or tmux runtime. Native working directories remain separate.
- The two fleet endpoints, global allowlist, disabled best-effort terminal output,
  Serve route, public wrappers and Arcana source registry were preserved. Only the
  gateway was restarted. Same-user local processes remain trusted; this is not
  hostile-local-user isolation.

Native authority is now a five-second selected-root/context grant, not knowledge
of a SID. Old references cannot retarget on selection/reload/runtime changes.
HTTP submissions require resource writer permission, exact Origin and CSRF, bounded
strict JSON, and the existing control-workspace admission gate. Lost receipts remain
uncertain; attempts are not automatically retried. The browser retains sanitized
uncertainty independently per originating conversation, without retaining prompt text.

### Validation, rollback and cleanup

- Main-checkout Node suite: 53 passed. Scoped Coven Rust checks passed, including
  68 observer units, 19 prompt workflows, all-target check and release build.
  The two separately invoked actual sharing-producer HTTP tests and the older
  read-producer fixture passed. They use synthetic native services, not user chats.
- Native main-checkout baseline: 219 distinct tests passed, one optional hosting
  test skipped, and package typecheck passed. After the config-alias fix, 22 sharing
  tests and package typecheck were rerun. These counts overlap; do not sum them.
- Compiled normal-TUI sandbox checks passed before/after publication with a
  symlinked config parent, automatic endpoint publication, shared capability and
  no grant until selection. No socket/SID override, transcript or model call.
- Candidate, live loopback and actual Serve HTTPS checks passed for health,
  assets/composer, metadata, CSRF and unauthenticated/unknown-target write refusal.
  There were zero real native grants at rollout; a deliberate UI restart remains
  the activation boundary. Full regression/phone-prompt/model acceptance is not claimed.
- Strict Clippy still reports only the five known unchanged-code style findings.
  All owned test/build/smoke directories and children were cleaned.

Immediate gateway rollback copies are
`~/.config/coven-web/observer.pre-web-sharing.json` and
`~/.config/systemd/user/coven-web.service.rollback-f0904e1d` (both 0600).
Restore only those config/unit contents, reload the user manager and restart only
`coven-web.service`; leave Serve and active native agents alone. Previous binaries
and older rollback copies remain retained.

Source is integrated into main checkouts at Coven `c907fd5` and Djinn `9b33615`
as uncommitted changes. The three new effort worktrees were removed normally after
verified integration and complete recovery snapshots:

| Removed worktree | Repository | Recovery stash object |
| --- | --- | --- |
| `coven-web-sharing-client` | Coven | `85778eed50cffa8a079b642a5e099722a076dbbb` |
| `coven-web-sharing-core` | Coven | `24cc1b6653101617d5e3b8b68c0f539ef1b894d4` |
| `djinn-web-sharing` | Djinn | `0674788474ccbf85fc3bfd436bedf5e89c840507` |

Retain these exact recovery entries until integrated source is committed and no
longer needs recovery. Unrelated worktrees, branches, old recovery stashes and
remote worktrees were not removed. No source commits or pushes were made.

## Current deployment and cleanup, 2026-11-19

This section supersedes the historical worker/reconciliation/deployment records
below. Source now resides in the normal main checkouts:

- `/home/jdawson/Projects/coven`, base `a3f9fc1`, with unstaged/uncommitted feature
  changes and current-main agent-management integration preserved.
- `/home/jdawson/Projects/djinn`, base `88e2042`, with the native reader added while
  preserving the pre-existing attachment/status work. Nothing was committed or
  pushed during reconciliation and rollout.

### Published and running

- `coven-web.service` runs
  `/home/jdawson/.local/libexec/coven-web/f0904e1da389ab1a/bin/coven`.
  Its SHA256 is
  `f0904e1da389ab1a65954dfaeb2e2e38e99fff6a8a37bb5871b15b43c2d80d32`.
- The existing tailnet-only site remains
  `https://bumblebee.tail97624b.ts.net`, proxying only to `127.0.0.1:7341`.
  The Serve route, observer configuration, login policy and two selected fleet
  endpoints were unchanged. Best-effort terminal output remains disabled.
- Future public dev Coven launches select `coven-native-y6fzrkjz`; the old
  `coven-native-agc3h45d` payload is retained. The public wrapper and Arcana registry
  were unchanged; the existing installer atomically switched its dev prefix.
- Future Djinn UI launches select compiled bundle
  `2b75443906a85635e8e28bf0bb4a915f1e952ca012a204c0678c409768aa027e`
  under `~/.local/libexec/arcana-dev/djinn/bin/.djinn/`. The CLI bytes are unchanged.
  The previous `9ac92fd0...` bundle is retained. No existing native agent/UI/Coven
  process was restarted or signaled; only the web gateway was restarted.

**Conversation activation is still explicit.** The new website currently returns
an empty authorized Conversations list: no native target was guessed or registered.
Existing UI processes were started without `DJINN_PROMPT_SOCKET` and cannot acquire
one from binary installation alone. Start a new full TUI with the published UI and
a private socket, obtain its runtime UUID, and deliberately register the exact root
session and allowed logins using [the configuration contract](COVEN_WEB.md). Do not
infer a session from a pane/PID/title or silently follow a runtime replacement.

### Validation of reconciled main

- 39 Node and 273 focused Rust checks passed, including current-main observations
  and presentation regressions, isolation/lifecycle/parity, install/package, and
  the separately invoked real-producer HTTP fixture. Cargo all-target check and
  formatting passed.
- After a two-line lifecycle borrow cleanup, 27 targeted lifecycle/isolation
  checks and the final release build passed. These overlap the earlier scope and
  are not an additional unique-test total. Strict Clippy still reports five
  pre-existing unchanged-code style findings; new lifecycle findings were fixed.
- Djinn: 103 reader/session/instance tests, 85 focused native/attachment tests and
  14 selected-session tests passed; package typecheck and compiled capability
  probes passed. One opt-in full Coven/Kit hosting test was skipped. The existing
  models snapshot and dependencies were reused without dependency installation.
- Isolated candidate, live loopback and actual Serve HTTPS checks passed for page,
  new assets, health, fleet schema, empty Conversations list, unknown reference
  refusal and identity admission. This is not phone/cellular or real-chat acceptance.
- Owned test/build directories and children were cleaned. Full regression and
  physical-device/provider-model acceptance remain separate gates.

### Rollback and removed worktrees

The original unit is retained byte-for-byte at
`~/.config/systemd/user/coven-web.service.rollback-920bef44`, alongside its old
`920bef44922edaf6` payload. Restore only that unit's content, reload the user manager
and restart only `coven-web.service` if rollback is needed. Keep the Serve route and
live agents unchanged. Native/dev selection rollback must preserve the matched
payload and should not terminate existing sessions.

All three effort-owned **local** worktrees were removed normally after source and
index/untracked preservation checks. Named Git recovery stashes remain until the
integrated work is committed and recovery is no longer needed:

| Removed worktree | Repository | Recovery stash object |
| --- | --- | --- |
| `coven-web` | Coven | `50f5b8645b89ca217234c55072c442a2766c2630` |
| `coven-team-isolation` | Coven | `f4ff432c2c614b07b4d60fc60304b54661f5a1b2` |
| `djinn-conversation-view` | Djinn | `6917b18bf4192dd4fe055e55bbf4b3c3469b0ad2` |

Do not pop/drop unrelated stashes; later recovery-snapshot cleanup must identify
only these exact entries after integrated source is committed. The unrelated
`coven-rust-migration`, `coven-worker-handoffs`, both main checkouts, and the remote
Megatron worktree were retained. No branch deletion or forced removal was used.

## Main source reconciliation overlay, 2026-11-19

The complete Coven prototype, web/conversation and agent-directory implementation
has now been reconciled as **unstaged, uncommitted source changes** into
`/home/jdawson/Projects/coven`, on `main` at `a3f9fc1`. Selection used the combined
`coven-web` working tree's delta against `a541e9f`, including prototype commit
`323dbc9` content and all 11 untracked implementation/test/doc files, rather than
copying its older integration files over current main.

Current-main agent-management, native integration and normal-workflow changes are
retained. The closed `PaneRead` method and its transport test were added alongside
the newer guarded-view transport and tests; module declarations were combined.
The local implementation and validation records below describe the earlier worker
baselines, **not test or activation results against this reconciled main**.

Both `/home/jdawson/Projects/coven-web` and
`/home/jdawson/Projects/coven-team-isolation` remain preserved and untouched,
including the former's pending no-commit merge/index. No staging, commit, build,
rollout, live configuration change or worktree deletion was performed during this
reconciliation. Build/test/rollout verification remains with the integration owner;
retain the source worktrees until safe integration has been verified.

Source-only checks on this overlay passed: all 55 files other than this handoff and
the two socket merges match the selected source exactly, including module
declarations. Both socket deltas match the prototype additions exactly, and all 19 main-only files
changed since `a541e9f` remain equal to `a3f9fc1`. Rust formatting, JavaScript and
Python syntax, and diff whitespace checks passed. These are not build, runtime,
test-suite or rollout acceptance results.

## Local implementation overlay, 2026-11-18

The deployment/source-location sections below record the earlier prototype handoff.
That prototype was subsequently preserved on `coven-web-observer` in commit
`323dbc95ccb86f1c49b070f6735a40a3cdd679dd` and pushed. Its original Megatron
worktree was retained. The following new implementation is **local, uncommitted,
and not deployed**:

- `/home/jdawson/Projects/coven-web`: combined Coven implementation. The saved
  prototype has a pending, conflict-resolved **no-commit merge** of committed
  `main` at `a541e9f`, plus the new web and team-directory changes. Automatic
  merge results are staged; new implementation edits include unstaged and untracked
  files. Review both the index and working tree, including new files. No merge
  commit or additional push has been made.
- `/home/jdawson/Projects/coven-team-isolation`, branch `work/team-isolation`:
  retained isolation worker source. Its changes were copied into the combined
  worktree while preserving the web modules.
- `/home/jdawson/Projects/djinn-conversation-view`, branch
  `work/native-conversation-view`: the companion native reader and tests, based on
  committed Djinn `main` at `88e2042`.

The original Coven and Djinn main worktrees have ongoing unrelated, uncommitted
integration changes. They were not edited or copied into these worker worktrees.
**The combined checks cover the named committed baselines plus this implementation,
not those evolving main-worktree edits.** Reconcile those changes before preparing
an installed rollout; do not overwrite either main worktree.

### Delivered first slice

1. [Agent directory isolation](AGENT_DIRECTORY_ISOLATION.md): explicit full-roster
   mapping to distinct, existing Git working-tree roots for legacy team launches.
   Coordination state stays in the control directory. No automatic worktree
   creation/deletion, relocation of running panes, security sandbox or scheduler
   is claimed. Reviewed `runtime_launch` v1 explicitly refuses this new mapping.
2. [Native conversation viewing](COVEN_WEB.md): explicit local socket/runtime/session
   registrations with per-resource Serve-login authorization. A separate
   Conversations list opens bounded, inert user/assistant text. No guessed binding
   from process/pane cards, terminal-output fallback, prompt submission or decision
   response was added. Remote native conversation endpoints remain unsupported.
3. Djinn's opt-in full-TUI private endpoint advertises
   `djinn.conversation-read.v1`. It reads already-active native contexts, limits
   persisted-record acquisition before materialization, and returns at most 20
   messages / 8192 UTF-8 text bytes. Its digest identifies the returned projection,
   not a transactional whole-conversation revision.

### Validation and operational boundary

- Combined Coven: native-conversation units (5), observer units (44), HTTP gateway
  tests (6), isolation acceptance, selected legacy-lifecycle/runtime-launch/refresh
  parity workflows, native install (9) and native package (2) checks passed.
- The separately invoked, default-ignored real Djinn socket producer through the
  authenticated HTTP gateway passed. Its messages are synthetic; it is not live
  native DB/TUI, provider/model, physical browser or tailnet acceptance.
- Final Node suite: 39 passed, including authorization invalidation, metadata
  expiry, deferred responses and keyboard-focus preservation.
- Djinn: 103 focused tests and its package typecheck passed. Independent native,
  isolation and web reviews were performed; identified correctness issues were
  fixed with regression coverage.
- Combined all-target Cargo check, formatting and whitespace checks passed. The
  full regression gate and a fresh strict Clippy run were not performed.
- Owned test/build/fixture directories were cleaned after child shutdown; unrelated
  run directories and shared package caches were preserved.

No installed executable, provider configuration, live agent, systemd unit or
Tailscale Serve route was changed. Activation needs separately reviewed builds,
explicit native endpoint startup, authorized target registration, and a deliberate
gateway rollout. Existing sessions are not retrofitted or restarted automatically.

## Decision and immediate coordination boundary

Coven Web is **Coven's browser client**, not a separate product, repository or
agent-management authority. Coven owns agent identity, inventory, qualified
observations, attention and supported actions. Kitsune is becoming a generic
multiplexer; Djinn/provider integrations supply their qualified native facts.

Bumblebee is actively integrating Djinn, Kitsune and Coven. **Do not expand the
prototype's backend independently of that work.** Its legacy metadata and process
evidence adapters are transitional, not the final canonical agent inventory.
Read the active `AGENT_MANAGEMENT_MIGRATION.md`,
`KITSUNE_RETIREMENT_FEATURE_DECISIONS.md` and `NORMAL_AGENT_WORKFLOW.md` before
deciding what to incorporate. This document records progress; it does not assign
parallel tasks, merge code, or override the migration decisions.

## Source location: important

The implementation is on **Megatron**, not merged into Bumblebee's checkout:

```text
~/Projects/coven-web
branch: coven-web-observer
base:   ec2d227e308f4558bada1c7099d78ff693fb0cc9
```

This is an isolated Git worktree of Coven, not another repository. Changes are
**uncommitted and unpushed**, including untracked client, observer and test files.
`git diff` alone will not capture those new files. Preserve the worktree until a
reviewed transfer is verified; do not copy it over the active integration tree,
reset/clean that tree, or assume an available commit to cherry-pick.

Only this handoff document is being added to Bumblebee's source tree. Its existing
integration edits under `docs/` and `tools/normal-workflow-check/` are untouched.
The deployed executable is a separate immutable payload, not the active Coven
launcher or a source checkout.

## What exists and is running

- Mobile-first cards, host filter, observation ages, stale/unavailable/unsupported
  states, and qualified evidence labels. No analytics, offline cache or transcript
  persistence. Static ES modules; no frontend production server or build dependency.
- A read-only Coven HTTP facade, co-serving the client on loopback behind private
  Tailscale Serve HTTPS. No Vercel, public Funnel, router forwarding or new runner.
- Two explicitly selected existing Kitsune endpoints: Bumblebee's dev default and
  Megatron's stable default. Identical version labels do not make these interchangeable.
- No agent/runtime start, stop, attach, resize, handoff or provider upgrade was
  required. Public Coven launchers and installed agent binaries were not replaced.

Deployment on Bumblebee:

```text
unit:    coven-web.service                 (user service, active and enabled)
config:  ~/.config/coven-web/observer.json  (0600)
listen:  127.0.0.1:7341 only
payload: ~/.local/libexec/coven-web/920bef44922edaf6/coven
```

The same scoped executable is installed on Megatron for bounded SSH reads.
Full artifact SHA256:
`920bef44922edaf6618df449fecf815ae282bbdba8a615ad41a9901581ea3ddc`.
Its manifest records the source base and uncommitted-build status. Retired
prototype units, configuration, payloads and owned cache artifacts were removed
after rename acceptance; they are not rollback artifacts still available today.

Use `tailscale serve status` on Bumblebee for the private URL. The HTTPS listener
proxies port 443 to loopback 7341. Existing SSH permissions remain separate.
At this handoff check, the real HTTPS API returned both endpoints reachable and
one recognized entry on each. **Counts are dynamic, not acceptance constants.**

## Prototype contract and limitations

Contract: `coven.fleet-observer.v1`.

```text
GET /api/v1/fleet
GET /api/v1/agents/<opaque-reference>/output
GET /api/v1/health
```

Only fixed assets and these routes are served. The browser cannot select socket
paths, SSH commands, RPC methods or arbitrary terminal locators. Read transport
permits only `session.snapshot`, `pane.process_info` and eligible `pane.read`.
Helpers accept a closed bounded inventory/output request over stdin; they never
start missing runtimes. Helper and gateway schema versions must match.
Upstream read dispatch can still perform housekeeping; read-only here means no
deliberate control operations, not a guarantee of zero internal runtime effects.

Current inventory combines Kitsune agent metadata with Coven's existing fenced
process-discovery adapter. The empty Kitsune agent list is legitimate during
retirement, not proof of no running agents. A prior Bumblebee sample had six
panes and no agent metadata; process discovery recognized two Djinn-family
targets and left four terminals unclassified. The adapter now handles that case.

- Process evidence supplies family hints only: activity is **unknown**, output is
  unsupported, and it establishes no native identity, request or completion truth.
- Metadata-backed status is explicitly heuristic. Idle is not success, and blocked
  is not an authoritative approval request. No title-based classifier was added.
- Coverage is incomplete even after successful discovery. Unsupported capability
  differs from a failed refresh; failures retain stale observations, not fresh emptiness.
- References currently describe endpoint-scoped terminal observations. They are
  **not canonical Coven agent identities**. A pane should become an optional
  attachment when the client moves to the canonical agent-management contract.
- Stable references survive reorder, other terminal births/removals and layout
  changes; disappearance/reappearance, expiry, replacement and failures invalidate them.

Recent-output infrastructure exists, but deployed `allow_best_effort_output` is
**false**. Current upstream pane reads lack an atomic expected-terminal/incarnation
precondition and reliable output revision. Matching before/read/after snapshots
cannot eliminate ABA races or attest output freshness. Do not enable this merely
to complete a UI demo; obtain explicit acceptance or use a stronger supported
contract. There is no prompt composer, submission, approval, native notification
or generic keyboard-injection feature in this slice.

Authentication requires the exact configured Host/Origin, allowed Serve login and
a loopback peer. Serve strips/replaces network-supplied identity headers. This
explicitly trusts Bumblebee's local execution environment: a local malicious
process can forge proxy headers. The web facade is narrow, but its upstream
account still has privileged socket access. Do not call this multi-user isolation.
Output is not logged, prefetched, cached or rendered as HTML/terminal instructions.

## Changes to review by ownership

| Area in Megatron's worktree | Responsibility / coordination |
| --- | --- |
| `clients/web/` | HTML/CSS/JS, Node tests, mobile presentation; can proceed against fixtures. |
| `src/observer/` | Prototype configuration, cache, adapters, HTTP/auth and bounded transport; Coven integration owner should review/adapt. |
| `src/agent_discovery.rs`, `src/agent_discovery/schema.rs` | Shared fenced acquisition extraction and explicit caller pane limits; shared-core hotspot. |
| `src/observation_socket.rs` and tests | Closed `PaneRead` addition; not arbitrary RPC. |
| `src/observation_wire.rs` | Separately bounded terminal-text decoding; ordinary wire limits unchanged. |
| `src/main.rs`, `src/lib.rs` | `coven observer check/serve/probe` registration; not the final management API design. |
| Cargo files, `justfile`, package/install tests | HTTP dependencies, embedded assets, `test-web`, packaging coverage. |
| `docs/COVEN_WEB.md` | Detailed prototype schema, trust boundaries, limits and operations. |

The HTTP facade uses Hyper/Tokio with connection/header deadlines, no request-body
aggregation, bounded connection/output concurrency and cancellation-safe output
permits. Inventory has priority over repeated output. Existing discovery keeps
its 64-pane CLI limit; the observer uses an explicit 1024-pane ceiling under one
five-second / 4 MiB acquisition budget. Do not silently widen these boundaries.

## Verification: what passed and what did not

After the inventory correction: 42 observer Rust tests, 20 discovery workflow and
33 discovery presentation tests, four real HTTP/CLI tests, nine native-install and
two packaging tests passed. After the naming/path migration: 42 observer tests,
four HTTP/CLI, nine install, two package and **26 Node tests** passed, as did
all-target Clippy, formatting and whitespace checks. Protected discovery behavior
was not changed by the naming migration.

Real HTTPS checks exercised page, assets, API and Serve identity forwarding.
An isolated browser at 390px/320px verified current branding, inventory parity,
modal behavior, output remaining unavailable and no browser persistence. Synthetic
browser checks separately covered hostile output as inert text. The user has
confirmed page visibility; this is not complete actual-Pixel/background acceptance.

**The full regression gate is incomplete.** An earlier broad Rust run passed
1,348 library tests (seven ignored), 100 CLI tests and multiple integration
binaries, then hit a ten-minute command timeout during an existing execution-
inspection parity test. No completed test reported failure, but do not describe
this as a passing full gate. No commits, pushes, PRs or release occurred.

## Suggested next decisions, not work already assigned

1. Integration owner chooses the canonical Coven inventory/observation/attention
   interfaces and qualification scope; do not freeze this prototype as that contract.
2. Agree versioned fixtures for identity, optional hosting attachments, evidence,
   leases, capability refusal, incomplete coverage and access restrictions.
3. Separate client-only changes from shared Coven/backend changes before transfer.
   Use a dedicated worktree based on the active integration branch, not an overwrite.
4. One owner handles the API/schema/adapters and cross-stack acceptance. UI work
   can parallelize against fixtures without editing discovery/provider internals.
5. Verify actual phone/cellular, hidden/resume, stale/offline recovery, saved policy
   tests, expiry/revocation and useful card targeting before calling v1 accepted.
6. Design message/approval controls only through qualified Coven authority. Never
   translate a generic browser approval into blind Enter/key input.

## Read-only operator checks and recovery boundary

```sh
systemctl --user status coven-web.service
journalctl --user -u coven-web.service
tailscale serve status
```

Do not reset Serve configuration, restart agent runtimes or enable output as part
of documentation/UI work. Stop/disable only `coven-web.service` if rollback is
authorized; remove only its own HTTPS listener if exposure is being withdrawn.
Lingering remains off: operation after logout or before user login following
encrypted boot is not guaranteed. The browser is unavailable if Bumblebee is off.
The current model is one private gateway, not high availability.

This handoff contains no private keys, passwords, auth links or backend credentials.
The operator configuration remains private on its host. Recheck live state and
the active integration diff before starting new work.
