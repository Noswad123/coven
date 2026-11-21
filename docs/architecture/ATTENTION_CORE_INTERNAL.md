# Attention Core Integration

Implementation follows `ACTIONABLE_ATTENTION_CONTRACT.md`. No HTTP, frontend,
push, Cargo dependency, or native producer changes are part of this worktree.

## Public Observer Methods

```rust
attention(&self, login: &str) -> serde_json::Value
decision_details(&self, login: &str, id: &str) -> crate::Result<serde_json::Value>
decision_response_allowed(&self, login: &str, id: &str) -> bool
respond_decision(&self, login: &str, id: &str, attempt: &str,
    action: &serde_json::Value) -> crate::Result<serde_json::Value>
```

All except `decision_response_allowed` may block. HTTP must retain its blocking-job
permit until the worker finishes, even when the HTTP request is cancelled. Push
may collect `attention` through a weak Observer callback. Collection refreshes
discovery and native grant status; it never bootstraps native or renews a lease.
The allowed check is exclusively cached metadata/ACL checks, including the current
fleet cache identity and card binding. It performs no filesystem or socket I/O.

`respond_decision` accepts only an opaque decision ID and the closed action value;
HTTP independently validates its outer contract/attempt/execute envelope. Native
selectors, tool linkage, occurrence and meaning digest never leave private core
references. Complete details are read only on explicit inspection or submission,
verified against the canonical tuple digest, and never cached. Metadata refresh and
card-binding verification never read conversation transcripts.

## Authority And State

- Source `decision_logins` defaults to empty, must be a subset of source readers,
  and requires the existing control workspace. It is independent of submit ACL
  and model-dependent `grant.can_submit`.
- Native capability must match the exact versioned contract and actions. Native
  `can_respond` remains authoritative for decision policy/read-write mode.
- Only verified automatic native/root-card associations produce public items.
  Static/manual chat registrations remain readable but decision-unsupported.
- Alert IDs hash length-delimited principal/source/runtime/session/kind/occurrence.
  Decision IDs additionally bind configuration/daemon epoch, pinned endpoint,
  grant, native request and meaning hash, conversation and exact card binding.
- At most 64 principal entries, each with at most 32 combined metadata references.
  Principal refresh generations fence late writers without clearing valid handles
  before I/O. No state lock is held during native/filesystem work.
- Discovery keeps its original public schema. Private coverage bookkeeping reports
  skipped unreachable/unsupported descriptor targets as unknown attention rather
  than a confirmed zero. Unknown native omissions stay null.
- ACL-scoped failed discovery coverage also blocks card uniqueness and immediately
  invalidates cached bindings. An unreachable retained descriptor is a potential
  competitor, never evidence that a remaining card claim is unique.
- Candidates and coverage commit atomically under the conversation-state mutex.
  Each source authority revision changes on candidate/reference or coverage changes,
  not unchanged lease refreshes. ACL-scoped card proofs include these revisions and
  sorted current conversation IDs. Expiration invalidates a proof rather than
  removing a competitor and creating a winner. A later candidate-set ABA cannot
  revive an old proof. List rows and their proof are captured in one cache snapshot.
- Enrichment checks the captured proof before commit; every stored link carries it.
  Cache-only bound/response checks compare against current discovery proof even
  before a concurrent discovery refresh runs its card enrichment. No I/O under locks.
- Final attention publication revalidates assembled items and cached references as
  well as native streams. Invalid card bindings remove items without removing valid
  cardless native streams. Known initial reference/ACL/permission failures after
  valid response-body decoding return explicit unavailable/unsent reports.
- Attention adds at most 64 streams of at most 32 alert IDs, using the same native
  summary queries. Stream ID hashes principal/source/runtime/SID with the existing
  SHA256 helper: UTF-8 parts prefixed by their byte lengths as big-endian u64.
  All observed occurrences are included, even without actionable cards/details.
  Complete means native complete with omitted_count exactly zero and a still-current
  authorized target. Missing/partial streams cannot retire push ledger entries.
- Explicit decision workflows have a 256 KiB aggregate receive budget and a shared
  deadline, while each frame remains capped at 64 KiB. Default sharing stays at
  64 KiB aggregate. Permission metadata is validated directly in the lossless value
  representation, including finite JS numbers beyond Rust integer ranges; its
  original canonical string is hashed without conversion or reformatting.
- Follow-ups and decisions share the same non-evicting 4096-attempt set and at most
  8 runtime/session flights. Aliases do not create new execution lanes. Attempts
  remain consumed after refusal; there is no automatic retry.
- Response admission owns the existing non-Send shared transport guard on one
  blocking thread through source/card validation and native CAS. Runtime-launch
  authority is refused before a native connection, and checked again with cached
  decision authority immediately before mutating send. No signal handler or spool.
- Known workspace/gate admission refusals return sanitized unavailable/unsent
  decision receipts, not errors that the HTTP layer might classify as uncertain.
- A validated native applied receipt means settled native wait, not tool success.
  Errors after a possible mutating send, including lost/malformed/mismatched
  receipts, return sanitized uncertain delivery rather than an unsent error.

## Validation

Focused unit/workflow coverage includes strict tuple/hash/DTO/action validation,
ACL before socket access, unsupported/read-only/details-unavailable states,
unknown versus zero coverage, card/grant/meaning/occurrence fences, source CAS
rejection, lost/corrupt receipts, runtime-authority constructor poisoning,
cross-method UUID suppression, runtime/session flights, concurrent refresh fences,
combined metadata bounds, and existing card/follow-up/prompt regressions.

The synthetic native peer performs conditional compare/remove for the wire tests.
Actual native service settlement/CAS and real-producer Bun integration remain the
native worker/parent integration gate, not something proven by these Rust fixtures.
No production service, model, or live chat is contacted by these tests.
