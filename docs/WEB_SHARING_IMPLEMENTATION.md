# Normal-launch web sharing: implementation contract

This is the agreed implementation boundary for the next local end-to-end slice.
It extends, rather than silently enabling, the existing explicit native socket.
Permission/question answers, remote native adapters and terminal injection remain
outside this slice. Sharing is a user-owned opt-in, not implied by status reporting.

## Native enablement and ownership

- Read only the user configuration `${XDG_CONFIG_HOME:-$HOME/.config}/djinn/coven-web.json`.
  Missing configuration means off. Closed schema:
  `{"version":1,"mode":"off"|"read"|"read_write","registry_directory":"ABS"}`.
  Require a bounded regular same-user non-symlink configuration file, mode 0600,
  and trusted user-owned/non-writable canonical parent. A user-owned dotfile-managed
  parent symlink is supported: pin the selected alias and canonical target identity,
  open the canonical file without following a leaf symlink, and fence both before
  and after reading. Never replace the user's directory layout or weaken canonical
  target ownership/permission checks. Invalid configuration fails closed.
  Configuration is captured at worker startup. Editing/removing the file is not
  live revocation: shutdown revokes the endpoint; restart applies changed policy.
  Selected-session, lease and context-reload revocation remain live independently.
- Automatic sharing supports only the normal local full-TUI worker/private-RPC
  path. Explicit HTTP, attach, mini and headless paths do not publish grants.
  Existing `DJINN_PROMPT_SOCKET` behavior remains available separately; if supplied,
  use that legacy explicit endpoint and do not also auto-publish another endpoint.
- The configured registry is a private same-user non-symlink 0700 directory. A
  missing leaf can be exclusively created below a verified private parent; never
  repair/chmod/adopt an unsafe existing directory. Native startup creates its own
  short private subdirectory/socket, preserving both Unix socket name limits.
- `RUNTIME_UUID.json` in the registry is an atomically replaced owned 0600 file:
  `{"contract":"djinn.web-discovery.v1","version":1,"native_socket":"ABS","expected_runtime":"UUID"}`.
  No logins, control workspaces, titles, prompts or transcript text are published.
  Descriptor files are hints, not liveness or authority. Discovery must handshake
  with the pinned native runtime. Cleanup removes only identity-matching owned
  artifacts; stale-looking sockets/files are never blindly unlinked.

## Native selected-session authority

The existing hydrated selected-session callback supplies exact session ID,
directory and optional workspace placement through private worker RPC. A new
latest-only frontend observer sends increasing revisions and one-second
heartbeats with a frontend UUID. Native code independently qualifies the exact
root, implicit-local placement, fixed startup directory and already-active instance
context. It must not call InstanceStore.load to qualify or execute automatic sharing.

A grant UUID is stable only for one continuously qualified selection/context;
selection change, deselect, expiry, deletion, context reload, shutdown or endpoint
failure revokes it. Its monotonic lease is five seconds. Re-selecting the same SID
after revocation creates another UUID. New selection information revokes the old
grant before any asynchronous qualification. Heartbeats renew grants, not activity,
model readiness or task success. Concurrent/obsolete frontend revisions cannot
renew or recreate a newer grant.

Pin one frontend UUID for the worker's sharing lifetime. A delayed/foreign UUID
cannot take ownership. Unchanged-selection heartbeats renew the current grant;
they must not revoke it on every increasing revision. An obsolete qualification
completion cannot restore or renew a grant after a newer selection/revision.

The automatic endpoint refuses legacy SID-addressed conversation/submit methods;
its shared operations require a matching current grant. Native admission rechecks
grant, context, root/placement and persisted model/agent configuration immediately
before entering ordinary native prompt admission. Once admission may have begun,
revocation/HTTP abort is not cancellation and a lost receipt is uncertain.

## Shared native wire (additive to existing version-1 framing)

Envelope stays `{version:1,id:UUID,method,params}` and
`{version:1,id,runtime_id,result|error}`. Existing capabilities add:
`web_sharing:{contract:"djinn.web-sharing.v1",version:1}` on automatic endpoints.

- `share_status`, exact params `{expected_runtime_id}`. Result:
  `{type:"share_status",contract:"djinn.web-sharing.v1",grant:null|{grant_id,session_id,can_submit,lease_ms}}`.
  `lease_ms` is an integer in 1..5000. `can_submit` means sharing policy and current
  persisted configuration permit admission, not execution success or continuous readiness.
- `shared_conversation`, exact params `{expected_runtime_id,session_id,grant_id}`.
  Result `{type:"shared_conversation",contract:"djinn.web-sharing.v1",grant_id,conversation:CONVERSATION_V1}`.
  The nested conversation is the unchanged bounded `djinn.conversation-read.v1`
  projection and digest. Native rechecks grant/context before returning its content.
- `shared_submit`, exact params `{expected_runtime_id,session_id,grant_id,text}`.
  Accepted result `{type:"shared_prompt_accepted",contract:"djinn.web-sharing.v1",grant_id,session_id,message_id,prompt_sha256,prompt_bytes}`.
  Use the existing exact UTF-8 prompt contract/65536-byte limit and request UUID
  suppression. No retry. Shared errors use closed existing codes plus
  `grant_unavailable` and `sharing_read_only`; pre-admission delivery is `rejected`,
  possible admission errors are `uncertain`.

Capabilities and response/frame bounds remain finite. Shared conversation output
retains its existing 20-message/8192-text-byte limits. Status is metadata-only and
does not bootstrap instances, query conversations or enumerate sessions.

## Gateway discovery and authority

Add optional `conversation_sources` (default `[]`, maximum eight). Closed entries:
`{id,label,team,directory,allowed_logins,submit_logins,control_workspace}`.
`directory` is local-only for this slice. Reader ACL is a nonempty subset of global
logins; writer ACL defaults empty and is a subset of readers. Writes require an
existing absolute control workspace supplied by gateway configuration, never by a
native publication or browser. Static conversation registrations remain read-only.

Authenticate the browser and source ACL before inspecting discovery directories or
native endpoints. Bound directory scanning, descriptor bytes/count and total native
handshake time. Open only regular owned 0600 descriptor files in the private
directory, reject duplicates/unknown fields/symlinks/unsafe endpoints, and handshake
against the same endpoint/runtime. Never use mtime, PID or directory labels as proof
of a live grant. Missing/inactive sources are explicit unavailable/empty metadata,
not permission to follow arbitrary targets.

References bind gateway/configuration epoch, source, endpoint identity, runtime,
grant, SID and configured control workspace. Expire cached mappings within the
native lease and revalidate natively on every read/submit. Old references never
retarget. Publisher content cannot grant tailnet access or choose a control root.

Metadata list rows add `can_submit:bool,max_prompt_bytes:65536`; older rows without
these fields remain read-only in the client. List/discovery I/O runs in bounded
blocking-job capacity, not on the async event loop.

Effective `can_submit` is the intersection of native grant permission, the current
principal's membership in `submit_logins`, and a configured control workspace.
POST rechecks these conditions independently of the displayed flag.

## Browser follow-up submission

- `GET /api/v1/csrf` returns a no-store, principal/daemon-epoch-bound, short-lived
  unpredictable token. Verify with constant-time authenticated comparison.
- `POST /api/v1/conversations/<opaque>/prompts`, closed JSON body:
  `{contract:"coven.conversation-submit.v1",request_id:UUID,execute:true,text:STRING}`.
  Browser never supplies native/session/socket/workspace/model/command selectors.
- Require authenticated Host/login, exactly matching Origin, permitted Fetch
  Metadata, custom CSRF header, JSON Content-Type, one bounded Content-Length and
  no transfer/content encoding. Reject before polling a body unless route, ACL,
  token and framing are authorized. Body read deadline two seconds, encoded cap
  400 KiB, decoded text cap 65536 UTF-8 bytes; reject duplicate keys and extra fields.
- Refactor a gated in-memory prompt service, retaining control-workspace admission,
  owned shared transport gate and runtime-launch authority refusal. Never install
  another process-wide InterruptGuard or spool browser text to prompt files.
- Bounded global mutation jobs and one in-flight admission per target. Preserve
  permits/gate on the blocking job if its HTTP future disappears. Bounded attempt
  UUID suppression must never silently evict and authorize replay. No automatic
  retries, including after HTTP timeout or runtime/grant replacement.
  Single-flight identity is the pinned native runtime/session, not a browser
  reference, source ID or grant. Aliases/grant changes must not create another
  admission lane for the same native conversation. Attempt suppression is global
  to the gateway epoch and does not reset when a grant/reference changes.
- Sanitized receipts contain contract, opaque ID, request UUID, outcome/delivery,
  fixed diagnostic codes and optional text digest/byte count. Never expose private
  native IDs/paths/errors or echo prompt text. Accepted means persisted/scheduled,
  not completed. A lost HTTP receipt after sending is uncertain.
- Composer is explicitly enabled only for writable rows, with UTF-8 byte count,
  explicit send and one-flight status. Read refresh never resubmits. On uncertainty,
  require inspecting the conversation and a separate deliberate decision to send
  again; no retry button. Authorization/grant changes fence late callbacks and
  clear sensitive views without claiming that previously delivered data is recalled.

## Acceptance and activation

Tests must exercise normal callback -> grant/publication -> gateway discovery ->
authorized read/POST -> same native admission once. Include default-off/read-only,
other same-directory SID refusal, stale grant/selection/reload races, huge/unsafe
descriptors, revoked ACL, forged CSRF/duplicate headers, native lost receipts,
duplicate attempts and HTTP cancellation. Use synthetic native callbacks/fixtures,
not live user prompts or model work. Existing explicit CLI/socket behavior remains
covered. Default-disabled setup and installed selection are separate from activation.

Rollout enables a reviewed user-level sharing configuration and matching gateway
source once. An already-running compiled worker still needs one deliberate restart;
do not interrupt the current agent or claim live-chat acceptance from fixture tests.
Any worktrees created for implementation must be integrated/preserved and removed
when done, without deleting unrelated worktrees or main-checkout changes.
