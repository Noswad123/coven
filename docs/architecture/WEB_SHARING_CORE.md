# Shared Conversation Core

`WEB_SHARING_IMPLEMENTATION.md` in the parent checkout is the authoritative
cross-worker specification. This note describes the core integration surface.

## HTTP Integration

`observer::conversations::Conversations` retains its constructor and existing
`list(login) -> Value` and `read(login, id) -> Result<Value>` methods. It adds:

- `submission_allowed(login, id) -> bool`: cache-only authorization, no filesystem
  or native transport access. Static registrations always return false.
- `submit(login, id, request_id, text) -> Result<Value>`: synchronous mutation
  service with independent argument, ACL, expiry, authority and native grant
  checks. Run the whole call on an owned bounded blocking mutation job.

The HTTP API should delegate to these methods. Listing performs bounded local
discovery and metadata-only native calls; it also belongs on a bounded blocking
job. Cancellation of an HTTP future must not stop its already-owned blocking job.
Core permits, runtime/session single-flight identity and the non-Send shared
transport guard remain owned until that synchronous job finishes.

List rows contain `can_submit` and `max_prompt_bytes:65536`. Optional `sources`
metadata describes only ACL-authorized sources and contains no private paths.
The source writer ACL defaults empty; omitted sources preserve static behavior.
Missing registries are permitted and yield empty metadata.

The combined authorized list is capped at 64 rows, with static registrations
first. `omitted_conversations` counts authorized, unexpired discovered rows that
do not fit; those hidden handles cannot authorize reads or submissions. Limits
are principal-specific, not based on another principal's registrations.

Refresh retains unexpired cached handles while scanning and replaces a source's
mapping atomically on completion. Busy lists serve the same live cached rows.
Expiry still applies during refresh; failures/revocation clear the source, and
an obsolete refresh generation cannot restore a newer mapping.

## Submission Semantics

The return value contains only the browser submit contract, opaque reference,
request UUID, outcome/delivery, closed diagnostics, prompt digest/UTF-8 byte count,
and native acceptance status. An error return is possible only before a mutation
could have been attempted. After possible native submission, transport, schema,
runtime, endpoint and receipt failures become an `uncertain` report, never an
error implying the prompt was unsent. Accepted means native persistence/scheduling,
not model success.

Attempt suppression is bounded to 4096 UUIDs per observer epoch, with no eviction
or automatic retry. Single-flight identity is `(native runtime, session)`, not
source, grant or browser reference; up to eight core mutation lanes can be owned.
The native wire shares one four-second deadline and one 64 KiB response budget
across capabilities, grant status and the single possible submit. Admission uses
the existing transport-gate wait, which can take up to five seconds, but does not
grant a fresh wire deadline after that wait. No process signal guard is installed
by the shared service, and browser text is never spooled or logged.

## Synthetic Validation

`src/observer/sharing_tests.rs` contains owned private-registry/socket fixtures.
Run focused observer conversation tests, session prompt units and
`session_prompts_workflow` offline/locked with disk-backed `TMPDIR`, isolated
`CARGO_TARGET_DIR`, one build job and exact-run cleanup. These fixtures establish
gateway protocol behavior, not normal-launch native activation or live model work.
