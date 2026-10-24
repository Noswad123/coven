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
The shared snapshot validator still checks legacy agent inventory shape and
consistency. Recognition does not use those classifications, but accepting a
snapshot still depends on their current schema during this compatibility slice.
Missing `foreground_processes` is an empty list, as Kitsune omits empty vectors.

The selected pane must appear in both snapshots with identical pane, terminal,
workspace and tab IDs. The process response's pane ID must match. Only after the
final successful fence is an observation published. Failed final reads, missing
panes, mapping changes, endpoint replacement, malformed data and exhausted budgets
discard all positive evidence. No cached active result survives a failure.

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

Validation for this slice passes **565 tests**, including 18 new
shadow-observation tests:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -q
```

`git diff --check` passes. This initial offline validation queried no live endpoint.
Subsequent [live validation](KITSUNE_PROCESS_LIVE_VALIDATION.md) confirmed shell,
negative-argument, native Djinn, direct UI and post-exit behavior against installed
Linux binaries, without model requests. It did not require recognition changes.

Explicit opt-in group adoption and revalidation are now implemented under the
[group contract](GROUPS_CONTRACT.md). The full offline suite now passes
599 tests, protecting this observer's unchanged public contract and socket-alias
transport safety. [Live group validation](GROUPS_LIVE_VALIDATION.md) also confirms
label publication, exit/TTL behavior and explicit process rebind. Installed Linux
native/UI forms are sampled, not universally certified across packaging/platforms;
additional launcher forms, activity interpretation and native lifecycle remain
separate work. Do not disable Kitsune built-ins based on executable-family
recognition alone.
