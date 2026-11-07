# Process-bound groups: installed Linux validation

## Result

On **2026-10-24 UTC**, an isolated live run passed process-based group binding,
visible annotation publication, exit-triggered publication stop, timer-driven TTL
expiry, replacement-process rejection, explicit rebind, and resumed publication.
No model prompt was entered or model request submitted.

The first attempt exposed a real Unix-socket transport bug. After a narrow fix and
offline regressions, the second attempt passed against the same installed builds.
This is not complete agent-management extraction or native lifecycle validation.

## Versions and isolation

- Coven checkout `567853cfe30b73a31f6645b688927f4d51c0751e`, plus the local transport
  fix below; exercised the actual `bin/coven` CLI, not a replacement API.
- Installed Kitsune **0.7.5**, installed Djinn UI **0.0.0-local** on Linux x86-64.
  Binary hashes matched the earlier
  [process-observation validation](KITSUNE_PROCESS_LIVE_VALIDATION.md). No newly
  built Kitsune/Djinn source compatibility is claimed.
- Separate disposable named sessions, new Coven workspaces, and temporary Kitsune
  configuration. Only the test session enabled annotation source `coven-live`.
  Two inner panes supplied existing borders; no user configuration was edited.
- Test shells used `/bin/sh`; nested operation was enabled only in the temporary
  config, and native restore/resume was disabled there. The user's agent-detection
  override directory was absent.
- Direct `djinn-ui --pure` launches used a separate HOME/XDG tree, explicit
  environment allowlist, empty auth content, project-config/plugin isolation,
  disabled model fetching/autoupdate/autocompact, and deny-all tool permissions.
  The UI stayed at its idle home screen; no provider/tool work was requested.

## Confirmed transport failure and fix

The caller-selected API pathname was **78 encoded bytes**, but resolving its
parent symlinks into the dotfiles checkout expanded it to **108 bytes**. Linux
pathname Unix sockets allow at most 107 pathname bytes plus the terminating NUL.
Kitsune's CLI could connect through the short selected path; Coven's transport
attempted the expanded address and returned `io_error` before membership mutation.

Both Coven read and publication transports now choose the shorter filesystem-byte
encoding of the selected and resolved pathname **before one connection attempt**.
There is no retry, discovery or fallback after an attempted connection. Canonical
path/device/inode remain the endpoint identity and persisted binding. Observation
fences also re-resolve the selected parent alias, rejecting retargeting before
accepting the original ownership, mode and inode checks. Publication retains its
existing equivalent alias/identity fences and delivery-uncertainty classification.

Ten offline regressions failed before this fix and pass after it. They cover both
path-length directions, actual long canonical paths, canonical identity retention,
observation and publication, alias retargeting, permissions/leaf symlinks, framing,
deadlines and method allowlists. This does not eliminate pathname races or add peer
authentication; both paths being too long remains unavailable.

## Live sequence and observations

Successful session: `repro-coven-group-uyhgel5t`. Target pane/terminal IDs were read
from the runtime: `w1:p2`, `term_65e8ed97db76d3`. The short/expanded socket lengths
remained 78/108 bytes, so the passing run exercised the fix rather than avoiding it.

| Step | Observed result |
| --- | --- |
| Explicit `group add --process-evidence` | Schema 2 committed; process provenance retained; native reference null/unestablished. |
| Foreground publication | One complete cycle, one acknowledged write; `Live · Probe` visibly rendered beside the manual `probe` title. |
| Exit test UI with `ctrl+c` | Publisher stopped with `process_not_recognized`; no job success inference or continued refresh from legacy metadata. |
| New publication at the shell | Rejected before reservation/write; group authority bytes and counters unchanged. |
| Quiet-shell expiry | Label disappeared about **6.021 seconds after first observation**; underlying shell buffer identical before/after. This is a local interval, not the server acceptance timestamp. |
| Restart UI in the same terminal | PID changed `3352994` -> `3354148`; publication rejected with `rebind_required` before reservation/write. |
| Repeat add without `--rebind` | Rejected; authority bytes unchanged. |
| Explicit process-evidence rebind | Participant UUID preserved; new process evidence accepted. |
| Publish after rebind | One complete cycle and acknowledged write; label visible again, then expired while UI remained idle. |

All publisher reports retained annotation-only authorization, no native execution
authorization, and `dispatchable: false`. Kitsune's process classification was not
disabled; empty-inventory independence remains covered offline, not claimed as
the observed live configuration.

## Evidence and cleanup

- Initial failure artifacts: `/var/tmp/coven-group-live-SRXPqJVg/`.
- Passing artifacts: `/var/tmp/coven-group-live-UYHgeL5t/`, including CLI results,
  event timestamps, `summary.json`, isolated workspace, and rendered text captures
  `accepted-label.txt`, `expired-after-exit.txt`, `rebound-label.txt`, and
  `expired-while-ui-idle.txt`.
- The driver is `validate.py` in the first artifact directory. Its second run used
  explicit `COVEN_LIVE_*` inputs copied from runtime responses; each directory
  retains its temporary config and isolated launcher.

Both named sessions were stopped/deleted. The passing run verified the created
outer pane's terminal identity before closing it; the first outer pane was already
absent at cleanup and no other pane was closed in its place. Both captured passing
probe PIDs were absent afterward. Session listing contained only the still-running
default session. No existing user group/application was controlled and the main
server was not stopped/restarted. Temporary evidence and isolated caches remain
under `/var/tmp`; they are not repository dependencies.

## Validation and limits

The full offline suite passes **599 tests**, including the 10 new transport
regressions. Diff whitespace checks pass; static code review found no blocking
issues. No protocol, storage schema or execution-authority change was needed for
the transport fix.

This is one installed Linux direct-UI workflow. It does not certify other
platforms/packaging, multi-server orchestration, paid/native execution, native
session identity, lifecycle correctness, cancellation or policy enforcement.
PID reuse, sampled-mapping races, post-check process changes, and TTL-bounded stale
labels remain the existing [group-contract](GROUPS_CONTRACT.md) limitations.
