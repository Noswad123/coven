# Private hook-file backend contract

## Scope And Status

Linux and macOS are shared-behavior targets now, not a Linux implementation with
a late macOS port. The private file-mutation backend is source-complete for both:
macOS explicit `--apply` is source-enabled, conservatively restricted to eligible
local APFS objects, and **provisional, not natively qualified**. This is not a claim
that all agent management works on both operating systems.

`src/hook_files.rs` owns descriptor-relative storage. `src/hook_files/platform.rs`
selects native primitives and supplies one `supports_apply()` capability. The
Claude/Copilot single-file runner, Codex two-file runner and Pi/OpenCode shared
extension runner all consult it; five provider-specific platform switches are not
the contract. Provider policy remains in the provider modules.

| Capability | Linux | macOS | Other Unix | Windows |
| --- | --- | --- | --- | --- |
| Claude/Codex/Copilot hook-file and Pi/OpenCode extension-file preview | Implemented | Source-enabled; physical paths and APFS admission required | Preview only; not a replacement mutator | Unsupported |
| Explicit install/uninstall/rollback `--apply` | Implemented; existing admission unchanged | Source-enabled, provisional local APFS only; native qualification pending | Refused before mutation | Unsupported |
| Private agent receiver/store/socket authorization | Existing Unix implementation; separate validation | Untested separately; hook-file ACL checks do not protect this layer | Unix implementation is not qualification | Unsupported |
| Generic foreground-cohort discovery/watch | Implemented recognition, not full status | Explicitly rejected by current discovery contract | Unsupported by current discovery contract | Selected-process/legacy-agent-aware evidence rejected |
| Full status interpretation, provider start/resume and agent-aware restore transfer | Pending; explicit Djinn capabilities are not all-provider completion | Pending | Pending | Pending |
| Kitsune new-install/recommend/update/bulk retirement for these five providers | Existing retirement unchanged | **Not retired** | Not retired | Not retired |
| Native macOS SDK linking, APFS and installed-provider qualification | Not established by Linux tests | Pending on Apple Silicon and Intel | Not applicable | Not applicable |

Kitsune retains every old non-Linux installer, asset, result type, recommendation,
update/bulk path, status, uninstall and ingress surface. Backend existence alone
does not authorize retirement. See [migration ownership](AGENT_MANAGEMENT_MIGRATION.md).

## Shared Invariants

Roots are explicit existing absolute physical paths: component-by-component
`openat` with `O_NOFOLLOW` pins directories. Production does not canonicalize an
untrusted root. In particular, macOS `/tmp` and `/var` aliases must not be traversed;
select the inspected physical existing path instead. Test helpers canonicalize
only the known-good temporary **base before `tempdir_in`**; deliberate malicious
test aliases remain unresolved so rejection is actually exercised.

Exact legacy hook commands referring to a logical alias may not match the selected
physical root. Migration then safely refuses an ambiguous reference; production
does not automatically canonicalize legacy commands or user paths to make them
match. Test-base canonicalization does not widen migration admission.

Fixed `Spec` allowlists control leaves, permitted subdirectories and provider
recovery names. Receipts/manifests never supply filesystem authority. Opened roots
and allowed subdirectories require current effective-UID ownership and no
group/other write bits; leaves require owned single-link regular files, not
symlinks or hardlinks. Private leaves are exactly 0600. Stashes are exactly 0700;
new file modes are set and verified by descriptor. The native access hook checks
opened roots/subdirectories, leaves and recovery/staging descriptors, including
staging **before any secret write**, after writing/syncing and after reading.
This is not an ACL audit of every ancestor or protection against later external
changes. Production neither strips ACLs nor automatically canonicalizes paths.

Provider receipt versions, namespaces, error/status contracts, native-source pins
and generated template postimages remain unchanged: Claude/Copilot v2, Codex v1
and Pi/OpenCode v1. Pi's frozen `887451b` compatibility remains authoritative.
The platform extraction adds no automatic migration or new provider semantics.

`MoveKind::{Exchange, NoReplace}` expresses semantics, not shared numeric flags:

| Semantic move | Linux | macOS |
| --- | --- | --- |
| Retain displaced replacement | `renameat2`, `RENAME_EXCHANGE` = 2 | `renameatx_np`, `RENAME_SWAP` = 2 |
| Publish only into absence / retain deletion | `renameat2`, `RENAME_NOREPLACE` = 1 | `renameatx_np`, `RENAME_EXCL` = 4 |

There is **no ordinary overwrite-rename fallback**. Shared code requires distinct
source/destination directory `(device, inode)` identities before every move,
including aliases. This also prevents Darwin EXCL's same-directory/same-vnode
success case from weakening no-replace semantics.

Files and all affected directories must synchronize successfully using `fsync`.
Directory errors are not suppressed. This is fsync-level durability, **not
`F_FULLFSYNC` or a power-loss guarantee**. Locks coordinate Coven writers, not
noncooperating editors/providers or writes through old descriptors; fences and
retained displacement are not global CAS, a multi-file transaction or atomic native
activation. Stop providers, editors and watchers before apply.

Errors after mutation can leave changed live files and recovery evidence. Preserve
and inspect it; never blindly exchange back or retry over a newer edit. Cleanup
sync can fail after backups have already been removed, and final sync can fail
after the recovery directory has been removed (`*_durability_uncertain`). A
refusal/error is not proof of zero effects or guaranteed recoverable backups.
Single-file apply may also create its persistent lock before provider validation.
Provider pending-receipt recovery rules remain unchanged.

## Native Admission

Linux retains Root's existing ownership/mode admission. Its extra platform access
hook is a no-op: success **does not claim ACL absence** or add a Linux ACL policy.

macOS requires each checked descriptor to identify local APFS (`MNT_LOCAL`) with
ownership enabled: `MNT_IGNORE_OWNERSHIP`/`noowners` is rejected even with no ACL.
`fgetattrlist` must return the exact valid, supported extended-security volume
capabilities before an ACL query's `ENOENT` may mean absence. HFS, network and other
filesystems are unsupported. Malformed capabilities, unsupported operations and
query errors fail closed, rather than being interpreted as an empty ACL.

Every ACE is rejected, including allow, deny-only, self and inheritance-only
entries. The adapter accepts established absence or a valid zero-entry object;
it does not infer privacy by interpreting individual ACE permissions. It does not
restore full metadata (ACLs, xattrs or flags), validate ancestor ACL policy, or
guarantee SIP/TCC authorization. Conservative refusal is intentional.

Rust libc lacks the needed Darwin ACL API. `macos_acl.c` therefore uses native
Apple SDK ACL, mount and attribute types; Rust sees only the project-owned
`int coven_hook_acl_state(int fd)` ABI (0 absent, 1 any ACE, -1 error). Linux mock
types exercise decision logic, **not native Apple ABI correctness**.

`build.rs` selects `CARGO_CFG_TARGET_OS`, not host cfg/HOST, and compiles the native
static C library only for macOS, never defining `COVEN_HOOK_ACL_TEST`. The direct
build dependency uses already-locked `cc` 1.2.66; no product version/MSRV bump is
implied. The source-package boundary includes `build.rs` and platform C/H files,
including the unit-test mock fixture, with assertions in `tests/native_package.rs`.
The development target-check harness remains outside that package boundary.

`renameatx_np` was introduced in macOS 10.12. That is an API floor, **not the
product deployment floor**: the eventual minimum must satisfy the maximum of
Rust, dependencies, packaging and SDK/deployment requirements.

## Validation Gates

The completed parent validation below is recorded without rerunning tests/builds
in this documentation update. Review is ready with no blockers **within the
provisional source scope**, not native macOS certification or full-suite success.

| Focused Linux Rust Scope | Passed Distinct Checks |
| --- | --- |
| Root/backend | 14, including ONE C-mock test with 33 internal scenarios |
| Codex units | 7 |
| Copilot units | 6 |
| Shared extension-engine units | 20 |
| Provider integrations | 64 across all five providers' workflows, including 2 frozen-Pi checks |
| `native_package` | 2 |
| CLI | 91 |
| **Total** | **204 distinct focused Linux Rust checks** |

The 33 C scenarios are not additional Rust tests. Historical discovery 201,
OpenCode 236 and 1,318 Kit seam checks are separate, non-additive evidence.

| Evidence | Status And Limit |
| --- | --- |
| Focused Linux Rust validation | All 204 distinct checks above passed; not a full-suite pass or native macOS evidence. |
| Strict all-target lint | Passed on Linux. |
| MSRV | Rust 1.85 all-target checking passed. |
| Optimized build | Linux optimized build passed. |
| Source package | Locked package verification and packaged test-target compilation passed. |
| C mock noowners review | The 33 scenarios include the source-backed P1 fix: `MNT_IGNORE_OWNERSHIP` is rejected early with zero capability/ACL queries. Not a native reproduction or two-UID test. |
| Apple Rust target availability | Both `aarch64-apple-darwin` and `x86_64-apple-darwin` standard libraries installed for Rust 1.96.1 in the reported development environment. |
| Full Coven Apple-target build attempts | Both failed: default cc rejects Apple architecture/minimum-version flags. An ARM retry using the correct Clang target confirmed the missing Apple SDK (`TargetConditionals.h` in ring). **Neither an application macOS build pass nor a native backend C compilation pass.** |
| Development Rust target-check harness | `--tests` passed warning-free for both `aarch64-apple-darwin` and `x86_64-apple-darwin`. Imports actual Root/platform Rust, types and shared tests with locked direct libc 0.2.186 and real target cfg. Initial ambiguous child-module path failure was resolved with explicit paths. No synthetic OS cfg or copied implementation. |
| What that harness does not validate | Does not compile Apple SDK C, link/run the application, exercise APFS, certify Cocoa/provider behavior or establish native qualification. |
| Historical discovery/state-seam checks | Separate evidence only; 1,318 Kit seam checks and earlier Coven/provider totals are not new hook-backend results and are not additive here. |
| Native qualification | Pending both architectures, native SDK linking and product deployment-floor validation. |

An incidental broader root agent full-library run **did not pass**:

| Full-Library Evidence | Result |
| --- | --- |
| Aggregate | 1,144 passed, 2 failed, 5 ignored |
| `agent_management::tests::copilot_capture_strips_native_fields_before_rpc_store_and_stdout` | Failed at `src/agent_management/tests.rs:484`, unwrapping `agent_delivery_uncertain`; isolated exact rerun passed. |
| `agent_management::tests::extension_capture_projects_only_reference_fields_and_keeps_store_v1` | Failed at `src/agent_management/tests.rs:318`, unwrapping `agent_delivery_uncertain`; isolated exact rerun passed. |
| Interpretation | Root cause not established. Isolated passes do not make the full-library run green or prove the failures unrelated to changes. No production timeout relaxation or resends were added. |

Native acceptance must exercise ordinary and case-sensitive APFS; absent/empty,
nonempty and inherited ACLs; noowners and two distinct UIDs; unsupported filesystems;
actual rename flags and same-vnode/alias behavior; mandatory file/directory sync
errors; locking, exact modes, crash recovery and physical-path alias rejection.
Linux C mocks and Rust target typechecks cannot discharge these gates.

The separate `agent_management::{store,unix}` code still uses its own Unix
ownership/mode checks for store, lease and socket authorization; it does not call
the hook-file Darwin ACL admission adapter. Its macOS authorization/privacy and
runtime behavior remain separately unqualified and are the next macOS gate. These
hook-file checks do not protect workspace state. Qualifying this configuration backend
would not qualify private receiver privacy, process discovery, full status,
start/resume/restore or installed native delivery.
