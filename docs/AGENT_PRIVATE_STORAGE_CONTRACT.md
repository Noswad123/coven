# Agent private storage and endpoint contract

## Scope And Status

This contract covers the agent receipt receiver's private store, locks and Unix
socket, not generic workspace storage or hook-file transactions. Linux is
implemented; macOS is source-enabled, **provisional local APFS only, not natively
qualified**. Privacy admission supports Linux and macOS only. Other Unix receivers
refuse before binding; generic storage on those systems is unchanged. Windows is
unsupported. Receipt semantics remain in [agent management](AGENT_MANAGEMENT_CONTRACT.md).

`src/private_access.rs` owns shared OS descriptor and socket-entry admission.
Hook-file platform admission delegates to it, without making the receiver depend
on hook transactions. The single SDK C unit,
`src/hook_files/platform/macos_acl.c`, retains the existing FD guard and adds the
socket-entry guard; there are no duplicate native implementations.

Linux's additional FD access guard is a no-op: callers still enforce ownership,
type, link and mode policy. It does **not** establish ACL absence or add a Linux
ACL policy. macOS requires local APFS with ownership enforced (`noowners` refused),
valid supported extended-security capabilities, and established ACL absence or a
valid zero-entry ACL. **Every ACE refuses**, including deny-only, self and
inheritance-only entries. Capability/query errors fail closed. No ACL stripping,
ancestor-wide ACL audit, SIP/TCC guarantee or native producer authentication is added.

## Descriptor And Path Admission

The agent-only filesystem (`agent_management::files::Files`) holds root and state
directory descriptors, the ordinary `state/.workspace.lock`, and a separate
persistent `state/.agent-management.receiver.lock` lease. It cooperates on the same
workspace lock as generic writers, releasing it between reports. The private 0600
lease admits one receiver per workspace independently of its socket; it is never
truncated, replaced or unlinked on exit. Do not remove it to bypass a live receiver.

Directories must be owned by the effective UID and have no group/other write bits
(`mode & 0022 == 0`), not generally exact 0700. Existing root/state 0755 and
workspace-lock 0640 can be valid; they are not chmod-repaired. The ordinary lock
must be an owned single-link regular file without group/other write bits. Store
and lease must be owned single-link regular files of exact 0600. The socket parent
is stricter: **exact 0700**; the usable socket node is owned, single-link and
**exact 0600**.

Agent paths refuse physical symlink ancestors and `..`; this tightens agent
admission, not all workspace commands. Relative workspace paths are joined
lexically to physical `getcwd` (`std::env::current_dir`), not shell `PWD`.
Production does not canonicalize a user alias automatically. Select inspected
physical paths, including on macOS where `/tmp` and `/var` can be aliases.
Owned test fixtures canonicalize only the known-good `std::env::temp_dir()` base
before creating their directory, honoring `TMPDIR`, not hardcoding `/tmp`.

`Directory` fences check both held-FD metadata/privacy and the re-walked physical
name. Root/state/lock/lease and target/temp identities are rechecked at relevant
boundaries. System ancestors are pinned, not all privacy-admitted. These are
cooperating-local-filesystem checks, **not global CAS or protection against hostile
same-UID/root changes or writes through old descriptors**.

## Private Store Publication

1. Under the workspace guard, open `state/agent-management.json` relative to held
   state with no-follow admission. Read at most 256 KiB from that **same admitted
   regular 0600 FD**, admitting before bytes and rechecking metadata, identity,
   physical name and guard after reading. Do not validate one file then reopen
   another for content.
2. Create an exclusive empty 0600 temporary in held state. Admit its privacy,
   type, ownership, link count, empty length and identity **before secret writes**.
3. Write and sync the temporary, then repeat privacy/identity and
   name/parent/workspace-lock/receiver-lease/target fences before publication.
4. `renameat` relative to held state is the visibility boundary. This is ordinary
   single-file replacement, **not hook exchange/displacement journaling**. Only
   the following state-directory sync can turn this published result into
   `Delivery::DurabilityUncertain`; the store still accepts the receipt and updates
   memory. Never retry or roll back that publication automatically.

Owned-temp cleanup is best effort and never deletes a foreign inode. Failed
stat/unlink can leave **one private orphan per failed attempt**, potentially with
secret bytes, bounded to 256 KiB per attempt. Repeated failures can accumulate
orphans; refusal does not mean artifact-free execution. Inspect ownership and live
use before manual cleanup.

Generic replacement and storage policies are unchanged. Only the pure shared
`Delivery` type moved to `src/storage/delivery.rs`, reexported through the same
`storage::Delivery` path with unchanged serialization.

Store schema remains v1 with exactly five record fields: `provider`, `session_id`,
`last_event`, `timestamp_ms`, `server_epoch`. Five providers share an aggregate
128-record cap and 256 KiB bound. Provider + NUL + exact session-ID hashing,
redaction, event interpretation and restart unknown/null-age semantics are unchanged.

## Socket Admission And Lifetime

Bind is exclusive and never auto-unlinks a stale leaf. `UnixListener::bind` starts
listening at bind, **not after chmod**. The already-admitted exact-0700 parent
(macOS ownership enforced and no ACL entries) protects the pre-chmod interval even
with umask 000. RAII captures the owned node before chmod; chmod is descriptor-
relative and no-follow, followed by exact-mode and privacy fences.

**Linux receiver startup requires available `SYS_fchmodat2`**, introduced in
Linux 6.5 or supplied by a backport, and permission to use it under the sandbox or
seccomp policy. Missing or blocked syscall yields known
`agent_socket_chmod_failed`, attempts owned-node cleanup, and never reaches Store
private writes. There is **no pathname-following fallback**. This is a receiver
startup requirement, not a kernel minimum for every Coven command or hook-file
operation. macOS uses `fchmodat(..., AT_SYMLINK_NOFOLLOW)` in source; native
qualification remains pending.

On macOS a connected/listening socket FD is not the filesystem vnode, and
`O_EVTONLY` is not an ACL-admission substitute. The C socket-entry guard queries
the basename relative to the retained parent FD using `getattrlistat` with
`FSOPT_NOFOLLOW | FSOPT_REPORT_FULLSIZE`. Its fixed schema deliberately uses
neither `ATTR_CMN_RETURNED_ATTRS` nor `FSOPT_PACK_INVAL_ATTRS`: unsupported metadata
must fail, not look like absent ACLs. SDK field order is length, device, object
type, owner, access mask, relative extended-security reference, file ID, packed
at four-byte boundaries. Signed relative-offset bounds precede pointer creation;
ACL blob magic, count and exact length are checked. Before/after no-follow stat
checks fence device/inode, UID, socket type, exact mode and link count one, with
the parent on the same filesystem. Query errors never imply ACL absence.

Clients admit the endpoint **before connect and before first send**. Once sending
starts, privacy, identity, protocol or acknowledgement failure is delivery
uncertainty, never permission to resend. The shared one-second client deadline is
unchanged. Frozen v1 `agent serve` keeps its at-most-200 ms whole-connection budget
(also capped by serve duration). Normal `agent run` uses a 200 ms receive/admission
window and a separate fixed 200 ms reply-write window started after the handler
completes, before the existing privacy fence. It never acknowledges persisted
receipts ahead of actual commit handling. Native-event acceptance uses the same
reply phases but remains volatile, not durable storage. These windows cannot
interrupt native filesystem syscalls: slow storage can exceed the client deadline,
and a receipt may commit while its ACK is still lost. No automatic resend or
mutation retry is authorized.

Cleanup uses the held parent and only the still-owned inode in the same private
parent. Changed ACLs or namespace can prevent cleanup and leave an orphan/stale
socket. Inspect it; a later bind does not automatically delete it. SIGINT guarding
precedes bind and outlives cleanup; SIGTERM/SIGKILL cleanup is not promised.

No peer credentials or provider authentication were added. Outputs retain
`authorization: none`, `caller_supplied_unverified`, and native identity/completion
`unestablished`. Private local access does not turn receipts into native truth.

## Validation And Gates

Final reported parent evidence, not tests rerun by this documentation update:

No native applications or user configuration were changed, no new Kit retirement
was made, and no native credential/provider authentication was added.

| Focused Linux Rust Scope | Distinct Passed Checks |
| --- | --- |
| `agent_management::` units | 59, including Files 14, Unix 24, Store 1 and existing 20 |
| `hook_files` units | 15, including one C-mock test |
| Generic storage units | 28 |
| Receipt-presentation units | 5 |
| Codex / Copilot units | 7 / 6 |
| Shared extension-engine units | 20 |
| CLI | 91 |
| **Unit/bin subtotal** | **231** |
| Management workflow / presentation / private integrations | 6 / 3 / 3 |
| Codex capture / `native_package` integrations | 2 / 2 |
| Five generated end-to-end integrations | 5 |
| Five-provider configuration workflows | 57 |
| Frozen Pi compatibility integrations | 2 |
| **Integration subtotal** | **80** |
| **Total** | **311 distinct focused Linux Rust checks** |

Files/Unix/Store/existing units are subsets of 59, not additions. Child umask
libtest output and C-mock scenarios are not additional Rust checks. The C mock
contains 33 old FD, 46 socket and 31 parent scenarios (**110 scenarios**), not
native ABI evidence. Agent-reported ASan/UBSan C checks exercised source mocks on
Linux, not native macOS. Historical 204/201/236 and Kit counts are non-additive.

| Scope | Current Evidence And Limit |
| --- | --- |
| Focused Linux checks | All 311 distinct checks above passed in the final selected runs; not a full-suite-green claim. |
| Strict all-target lint / MSRV | Linux lint with warnings denied and Rust 1.85 all-target checking passed. |
| Optimized build / source package | Linux optimized build, locked package verification and packaged test-target compilation all passed. Package assertions include actual `src/private_access.rs`, `src/agent_management/files.rs` and `src/storage/delivery.rs`; generic storage privacy policy is not broadened. |
| Actual-source Rust target harness | `--tests` passed warning-free for `aarch64-apple-darwin` and `x86_64-apple-darwin`, importing actual Root, private access, agent Files, storage Delivery and tests; direct serde dependency and lock updated. |
| Harness limits | Does not import the full receiver/RPC module, compile Apple SDK C, link or run. Mocks test logic, not native ABI. |
| Full Apple application/native SDK build | Still blocked by the earlier missing Apple SDK (`TargetConditionals.h` in ring on the correctly targeted attempt); no new native full-build pass. |
| Ownership mutation/native ACL tests | UID-0 ownership-mutation branch not exercised; macOS ACL tests are source/typechecked, not natively run. |
| Final read-only review | Ready within provisional source scope, no remaining P0-P2 findings reported. Not native certification. |
| Fixture cleanup | Current teardown fixtures clean; production best-effort cleanup can still leave an owned secret-bearing temporary per failed attempt. |

The initial four-thread `storage::` run had **27 passed, 1 failed**:
`storage::bootstrap::tests::reservation_failure_matrix_preserves_exact_partials_without_adoption_or_lock_stealing`
failed at `src/storage/bootstrap/tests.rs:164`, asserting
`try_workspace_lock(probe)`. The serial rerun passed **all 28**. Root cause is
unestablished; the serial pass does not prove the failure unrelated, even though
generic storage logic changed only by pure Delivery-type extraction.

Earlier agent-reported serial workflow runs sporadically returned
`agent_delivery_uncertain`; the final selected serial runs passed. Root cause
remains unestablished; the normal-run reply-phase fix above does not prove the
cause of those historical failures. No production timeout relaxation or resend
was added in that historical validation cut.
Historical full-library failures remain failures in the
[migration validation record](AGENT_MANAGEMENT_MIGRATION.md#validation-record).

Separately reported controlled commit-delay traces demonstrated the normal-run
reply-phase fix: a 300 ms delay received an ACK, while a 1.1-second delay still
produced client delivery uncertainty. This is scoped timing evidence, not proof
of the root cause of either historical full-harness failure or a full-harness pass;
this documentation update did not rerun it.

Native qualification still requires actual SDK compile/link on Apple Silicon and
Intel; APFS sockets with absent/nonempty/inherited ACLs and capability errors;
noowners and two UIDs; default/permissive umask; before-connect/no-send refusal;
lock cooperation, cleanup/restart; file/directory sync faults, crash and partial
publication; and product platform-floor validation. Mock and target-typechecking
evidence cannot discharge these gates. macOS discovery is still explicitly
rejected, full status/execution/restore transfer remains pending, and Kit macOS
installers remain preserved, not retired.
