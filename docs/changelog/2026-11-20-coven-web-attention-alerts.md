# Actionable attention and background alerts deployed, 2026-11-20

Historical record: “current” below means this change-set only. Payload selection
was superseded by [quiet UI](2026-11-20-coven-web-quiet-ui.md) later on 2026-11-20;
the later phone-subscription evidence is in [complete titles](2026-11-21-coven-web-complete-titles.md).
Source/stash retention requires the [recovery review](../plans/COVEN_WEB_RECOVERY.md),
not an assumption that the old uncommitted baseline still applies. [History index](index.md).

This is the current feature record. Existing verified BASIC cards now support
native question/permission attention inside the same dialog. Follow-up messages
remain separate from answering questions or releasing permission waits. Supported
remote actions are question answer/reject and permission allow-once/session-pending
rejection; **never always allow**. Native policy and exact occurrence CAS remain
authoritative; alert dismissal is not a response.

The contract is [ACTIONABLE_ATTENTION_CONTRACT.md](../architecture/ACTIONABLE_ATTENTION_CONTRACT.md).
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
