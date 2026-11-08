# Live Domain Integration

Only these new module trees are implemented. Parent `lib.rs`, `main.rs`, common
platform code and all pre-existing uncommitted work are intentionally untouched.
Observer/status reports and `accepted` use `serde_json::Value`. Public attach and
detach return `crate::values::Value` directly from the groups mutation, with **no
postcommit conversion**. Valid historical endpoint device/inode integers may
exceed u64; returning a known mutation result must never narrow them or fail in a
late report conversion.

```rust
pub fn attach(workspace: &Path, request: AttachRequest<'_>)
    -> crate::Result<crate::values::Value>;
pub fn detach(workspace: &Path, group: &str, member: &str, expected: Option<&str>)
    -> crate::Result<crate::values::Value>;
pub fn status(workspace: &Path, group: &str, member: Option<&str>, timeout: f64)
    -> crate::Result<serde_json::Value>;
```

## Parent Wiring

Expose `djinn_live_observation` and `group_live` from the library. Make the existing
shared `observation_time`, `observation_wire` and incoming `groups` modules available
at crate root. No alternate decoder, clock, storage authority or Python fallback is
introduced by these production modules.

Expose the Unix implementation at `crate::platform::live_artifact`, compile-gated by
the parent platform module. Its exact crate-visible interface is:

```rust
fn open(path: &str) -> crate::Result<Artifact>;
impl Artifact {
    fn bytes(&self) -> crate::Result<&[u8]>;
    fn resolved(&self) -> &str;
    fn device(&self) -> Option<u64>;
    fn inode(&self) -> Option<u64>;
    fn recheck(&self) -> crate::Result<()>;
}
```

`open` fails when the parent cannot be checked/resolved. After a checked resolution,
it preserves partial checked metadata even on artifact failure; `bytes` and
`recheck` both return that safe failure rather than exposing failed contents. The
observer must honor both results. `recheck` is required after decoding and checks
FD/path stamps and selected-parent alias, identity, ownership and exact privacy.
The descriptor lives until the artifact is dropped.

Unsupported-platform stubs belong in parent integration: provide the same types
and signatures, with `open` returning `Error("unsupported_platform")`. No platform
implementation or conditional OS behavior is added to either core workflow.

## Shared Dependencies

- `observation_wire::decode(&[u8]) -> Result<serde_json::Value>`: whole-payload strict UTF-8, duplicates, nonfinite values, depth 32 and bounded containers/strings, before projection.
- `observation_wire::to_json(&values::Value) -> Result<serde_json::Value>`: exact bounded report conversion only. Before invoking it, the workflow projects group `{group_id, key, name}` and members `{participant_id, key, role, live_attachment}` in native values, omitting unused bindings and endpoint statistics. Preflight projects only participant/attachment fields; detach reads the participant string directly. Authority schema version is converted alone, never via whole-state conversion. Missing schema-1/2 attachments project as null. The group reader still validates the whole unchanged authority without new endpoint integer limits.
- `observation_time::{Clock, SystemClock}`: `monotonic() -> Instant`, `wall_ms() -> f64`, `timestamp() -> Result<String>`.
- `groups::read_groups(&Path) -> Result<View>`.
- `View::revision() -> &str`, `state() -> Option<&values::Value>`, `group(&str) -> Result<&values::Value>`, `member(&str, &str) -> Result<&values::Value>`.
- `groups::selector(&str) -> Result<()>` and `validate_attachment(&values::Value) -> Result<()>`, crate-visible.
- `groups::attach_live(&Path, AttachMember<'_>) -> Result<values::Value>`, consuming the request by value with borrowed fields `group`, `member`, `attachment`, `expected_revision`, `expected_participant_id` and boolean `reattach`.
- `groups::detach_live(&Path, group, member, expected_revision, participant) -> Result<values::Value>`.

The mutation owner must reload under its lock and enforce both captured revision
and participant fences, preserve identical attachment timestamps, and treat
publication counters as outside the content token. The workflow always supplies
both fences, including without a caller-supplied revision. No source I/O occurs
under the mutation lock or anywhere in detach.

`group_live::accepted(&report, &attachment)` is crate-visible for publication. A
status row can be checked by adding the fixed `adapter`, `adapter_contract`,
`schema_version` and attachment-derived `selection` fields, as the publisher does.
The function accepts only a usable selected envelope, including observed-unknown,
and returns exactly the ten selected source fields. It neither checks a new clock
nor renews any source lifetime; publication must keep reducing its own budget.

## CLI Mapping

- Observer caller errors are `Err` before any clock or artifact I/O. Operational failures are safe reports. Map `observed` to 0, `unavailable` to 1 and `incompatible` to 2. Observed `unknown` is success, never idle.
- Attach/detach return raw native mutation reports, preserving arbitrary-precision integers in the returned group/member bindings. The CLI must use native value serialization, not `observation_wire::to_json` or another serde roundtrip. Map uncertain durability to 2 using `delivery`, otherwise committed/unchanged to 0; ordinary `Err` is 1.
- Status maps top-level `observed` to 0 even when individual rows fail. Top-level `unavailable` maps to 1 with no participants. Local argument/authority errors are `Err`.
- JSON formatting/ASCII escaping and final CLI newline remain parent responsibilities.

## Focused Verification

```sh
env RUSTC="$HOME/.rustup/toolchains/1.96.1-x86_64-unknown-linux-gnu/bin/rustc" \
  rustup run 1.96.1 cargo test --offline \
  --manifest-path tools/harnesses/group_live/Cargo.toml \
  --target-dir target/live-focused --lib live --no-fail-fast
```

The isolated build stages a source mirror under its Cargo `OUT_DIR`, injects only
test wiring, omits unrelated groups/bootstrap suites, and runs the real artifact,
observer, workflow and groups/storage domain code. The small test-only storage
platform supplies real filesystem/advisory-lock operations; production uses the
existing parent platform. This is not a full binary or cross-platform acceptance
claim. Oracle subprocesses use Python `-B` only, on disposable fixtures; no real
Djinn, provider, user source, or workspace is changed. No full suite was run.
Regression fixtures include 100-digit device/inode integers accepted by the real
groups reader/store: all/member-filtered status succeeds, raw attach/detach results
retain exact integer digits, and detach succeeds with deleted or unsafe sources.
The harness does not substitute a compatible mutation return type or narrow those
statistics. Closed live-source oversized integers, including unknown fields, still
fail as schema faults rather than being ignored or reported as filesystem errors.
