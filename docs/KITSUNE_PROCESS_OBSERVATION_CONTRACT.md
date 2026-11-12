# Kitsune process observation: Djinn shadow slice

## Overview

Coven can interpret one selected pane's **generic process evidence** without
using Kitsune's agent classification. This is a bounded, opt-in shadow slice:
Kitsune's detection, lifecycle reporting, start/resume and existing snapshot adapter
remain unchanged. Default snapshot group bindings remain supported; a separate,
explicit [process group binding path](GROUPS_CONTRACT.md) can now consume fresh
evidence. The observer alone still never changes membership or publishes labels.
No Kitsune or Djinn API changes are required.

This is **executable-family recognition**, not active-agent detection. A process
named `djinn` may be running help, configuration, session listing or maintenance.
No activity, native session, acceptance, progress or success is inferred.

## API / CLI

```python
from runner.kitsune_process_observation import inspect

inspect(socket_path, pane_id, timeout=3.0, max_response_bytes=1048576)
```

```sh
coven observe kitsune --socket /absolute/path/to/existing-api.sock \
  --process-pane w18:p85 --json
```

The pane must be an explicit exact current ID. There is no focused-pane fallback,
endpoint discovery, inherited socket selection, runtime invocation, autostart,
screen access, annotation, input, retry or state write. Invalid caller arguments
fail before connecting. Limits and private-socket checks are shared with the
[default observation adapter](KITSUNE_OBSERVATION_CONTRACT.md).

The read sequence is exactly:

```text
session.snapshot -> pane.process_info {pane_id} -> session.snapshot
```

All connections are pinned to the original resolved socket path/device/inode.
One overall deadline covers all reads and validation. One **aggregate** byte
budget covers all three response frames including LF; a separate allowance is
not granted per request. Responses require strict JSON, matching generated IDs,
the expected result type, LF followed by EOF, and no trailing data. Snapshots
currently require protocol 18; this is a compatibility gate, not feature discovery.
Missing `foreground_processes` is an empty list, as Kitsune omits empty vectors.

### Generic pane snapshot projection

All process brackets use `observation._pane_snapshot`, separate from the unchanged
strict legacy `_snapshot`. Both share `_result`/`_decode`, response-envelope checks
and the protocol-18 gate. The generic contract requires only:

- A snapshot object with `version: string`, `protocol: 18`, and `panes`, a list of
  at most 1024 objects.
- All four locators on every pane: `pane_id`, `terminal_id`, `workspace_id`,
  `tab_id`. Each is a nonempty, printable, whitespace-free UTF-8 string of at most
  4096 characters. IDs are exact opaque values: no prefix, ASCII, UUID, or
  relationship between ID spellings is required or inferred.
- Unique pane IDs and unique terminal IDs across the entire panes list, not just
  the selected pane. Workspace and tab IDs may repeat.

The projection retains only version, protocol, pane locators, and `annotations`
when present (including null). It ignores the semantics of `agents`, `workspaces`,
`tabs`, `layouts`, focus/revision metadata and pane agent/native fields, whether
absent or malformed against the legacy schema. **This is not lenient JSON:** the
entire response is decoded before projection. Unused JSON still counts against the
wire-byte budget, depth limit 32, 4096-entry object/array limits and shared string
limits (normally 4096 characters, selected path/value fields 8192). Invalid UTF-8,
surrogates, duplicate keys, nonfinite numbers and malformed JSON are rejected,
even in unused fields and even within size limits.

Standalone process observation and `add --process-evidence` do not interpret the
annotation descriptor or require annotation opt-in. Publication always validates
descriptor presence strictly: absent means unsupported; present null or malformed
is an error. Enabled-source and epoch checks are unchanged.

The selected pane must appear in both snapshots with identical pane, terminal,
workspace and tab IDs. The process response's pane ID must match. Only after the
final successful fence is an observation published. Failed final reads, missing
panes, mapping changes, endpoint replacement, malformed data and exhausted budgets
discard all positive evidence. No cached active result survives a failure.

Publication also checks the expected bound terminal in the first bracket snapshot,
before querying process info. All process brackets are generic, including those
in mixed groups. Only a pure-process group's initial cycle snapshot is generic;
mixed and legacy-only groups retain strict initial validation and snapshot-member
native-hint checks. The contract is selected from validated member provenance
before parsing, never by falling back after strict validation fails.

## Interpretation and output

The distinct local contract is `coven.kitsune.djinn-process.v1`, schema version 1.
The envelope retains endpoint provenance, local UTC receipt times, duration,
diagnostic codes and limitations. `authorization` is always `none`, `dispatchable`
is false, runtime freshness is unknown, and server snapshot time is null.

Successful reports contain one `observations` entry, **not an agents inventory**:

- `provenance: kitsune.pane.process_info` and Coven interpreter contract.
- Endpoint-scoped pane/terminal/workspace/tab locators.
- `sample_received_at`: local receipt/validation time, not server sampling time.
- `mapping_evidence: matching_bracketing_samples`.
- `recognition: djinn_executable_family` if any matches, otherwise `unknown`.
- `matches`: only reported PID, executable-family name and matching rule.
- Unknown activity/attention; unestablished native lifecycle/job completion.

Only `foreground_processes[*].argv[0]` is interpreted. The basename is the literal
substring after its last POSIX `/`; it must equal `djinn` or `djinn-ui`. No shell
parsing, trimming, case folding, filesystem resolution, command-name fallback,
`argv0` fallback, cmdline matching, prompt inspection or native-reference extraction
occurs. Arguments, executable paths, cwd, names and raw error text are not returned
or persisted. Duplicate PIDs and malformed consumed fields reject the response;
multiple matching processes produce one pane observation, not multiple agents.

CLI exits remain **0** observed, **1** unavailable, **2** incompatible. A valid
unknown observation exits 0: it proves neither presence nor absence of an agent.
Failures have an empty `observations` list and safe diagnostic codes.

## Limits and safety

- Structured argv is reported evidence, not authenticated executable identity.
  Arbitrary binaries can use these names. Native session identity remains unset.
- Bun/Node launchers, renamed binaries and missing argv are intentionally unknown.
  This slice does not parse invocation modes or session references from arguments.
- Only a foreground process group is sampled. Background/headless jobs are not
  inventoried, and process disappearance is not termination or job completion.
- Bracketing snapshots **are not atomic attribution**. They cannot exclude an
  intervening mapping change and reversal, process replacement/exit, or PID reuse.
  `pane.process_info` supplies no terminal identity/runtime generation. The output
  records matching mapping samples, not a verified continuous terminal binding.
- Socket checks are not peer authentication. Read requests can trigger existing
  Kitsune dispatch housekeeping, including default-workspace/shell creation.
  Read-only client behavior does not promise zero server-side effects.
- Group membership requires a separate explicit binding command. The opt-in
  `group add --process-evidence` reads fresh evidence itself; it cannot import this
  report. Observation grants no membership, participant identity, policy or
  execution authority.

## Validation and next boundary

`tests/test_kitsune_process_observation.py` uses only private fake endpoints and
pure fixtures. It covers exact matching, false-positive exclusions, privacy,
request sequencing, mapping/endpoint fences, failure handling, aggregate bytes,
deadlines, CLI/wrapper behavior, and no runtime/file invocation in the adapter.
Existing observation/CLI tests protect the unchanged default path. No installed
runtime compatibility or live lifecycle guarantee follows from these tests.

The initial shadow-observation slice recorded **565 passing tests**, including
18 shadow-observation tests:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -q
```

`git diff --check` passes. This initial offline validation queried no live endpoint.
Subsequent [live validation](KITSUNE_PROCESS_LIVE_VALIDATION.md) confirmed shell,
negative-argument, native Djinn, direct UI and post-exit behavior against installed
Linux binaries, without model requests. It did not require recognition changes.

Explicit opt-in group adoption and revalidation are now implemented under the
[group contract](GROUPS_CONTRACT.md). The full offline suite now passes
**617 tests**, protecting this observer's unchanged public contract and
socket-alias transport safety. The generic snapshot slice adds 18 test methods in
`tests/test_process_snapshot_contract.py`; focused and baseline strict suites also
pass. No stored schema, protocol,
public API output, CLI flag or native authorization changes in this slice.
[Live group validation](GROUPS_LIVE_VALIDATION.md) also confirms
label publication, exit/TTL behavior and explicit process rebind. Installed Linux
native/UI forms are sampled, not universally certified across packaging/platforms;
additional launcher forms, activity interpretation and native lifecycle remain
separate work. Do not disable Kitsune built-ins based on executable-family
recognition alone.

Those historical live runs used valid real-server snapshots; they do not cover
the artificial malformed-legacy payloads exercised by this slice's offline tests.

## Related files

- [runner/kitsune_observation.py](runner/kitsune_observation.py) — shared decoder and separate snapshot contracts.
- [runner/kitsune_process_observation.py](runner/kitsune_process_observation.py) — process brackets and recognition.
- [runner/group_publisher.py](runner/group_publisher.py), [GROUPS_CONTRACT.md](GROUPS_CONTRACT.md) — conditional initial snapshot validation and publication fences.
- [tests/test_process_snapshot_contract.py](tests/test_process_snapshot_contract.py) — generic projection and strict-path regressions.
