# Bounded foreground session watch engine

`runner/session_watch.py` exports:

```python
watch(collect, emit, *, duration=60, interval=2, clock=time.monotonic, sleeper=time.sleep)
exit_code(summary)
```

This is a serial observation engine, not a CLI, native watcher, daemon, cache or
control adapter. It performs no filesystem, network, subprocess or native I/O of
its own. The coordinator supplies `collect(remaining_seconds)` to make a **new**
bounded inspection-v1 observation with timeout `min(API budget, remaining_seconds)`.
Selection validation belongs before observation; optional view metadata is retained.

## Timing and failures

- Duration is a literal finite int/float (not bool), in `[0.1,3600]`; interval is
  similarly in `[0.1,60]`. Invalid options/noncallables raise `ValueError` before
  any callback, including the clock.
- The first collection starts immediately. Later starts use the next strictly
  future interval tick anchored to the initial monotonic clock reading. Time spent
  collecting **and emitting** skips missed ticks; there is no overlap or catch-up.
  A callback ending exactly on a tick skips that tick too.
- No collection starts at or beyond the deadline. Sleep is bounded to the next
  tick or deadline. Early wakes are rechecked; oversleep cannot start a late poll.
  A nonfinite/regressing/raising clock, a raising sleeper, an unrepresentable
  deadline/tick, or sleep returning with no clock progress fails `clock_failed`.
- Calls cannot be forcibly interrupted by the timer. An already-started collection
  can finish and emit after the deadline. There is **no hard wall-time bound or
  native freshness guarantee**. `KeyboardInterrupt` returns `interrupted` without
  stopping/cancelling any native session.
- Normal unavailable/incompatible samples are emitted as failures, not converted
  to empty observations. Later ordinary ticks may recover with a new observation.
  No earlier successful sample is retained or substituted.
- Collector exceptions or invalid reports emit one clearing event (`unavailable`,
  `observation: null`, `diagnostics: [collection_failed]`), then stop as
  `collector_failed`. Exception text is never copied. Emitter failure stops as
  `output_failed` without retry; it takes precedence if clearing-event delivery
  fails. Clock failure prevents further delivery/collection. Interrupts in any
  callback return `interrupted`.

## Callback data and summary

The collector must return an already validated, privacy-safe inspection-v1 report,
not raw wire/native data. The engine checks plain JSON-compatible data, the v1
envelope/assessment, native no-authority qualifiers, target/count coherence, and
failure clearing/declaration semantics. This is a callback-contract check, **not**
a substitute for inspection's bounded decoder, mapping/declaration fences or
privacy projection. It does not rescan, reinterpret declarations or upgrade facts.

Each successful emit receives a new independent envelope:

```json
{"schema_version":1,"contract":"coven.session-watch.v1","sequence":1,
 "elapsed_ms":0,"assessment":"observed","diagnostics":[],"observation":{}}
```

`observation` is a deep copy of that collection's report, including optional view,
scope, historical binding qualifiers, limitations, revisions and diagnostics.
Envelope diagnostics mirror the report using a separate list. `elapsed_ms` is
local time at envelope creation rounded to three decimal places, not a native
timestamp, receipt lease or attestation. Sequence starts at 1.

The returned `coven.session-watch-summary.v1` has `schema_version: 1`, `outcome`,
`poll_attempts`, `emitted`, `observed_samples`, `unavailable_samples`,
`incompatible_samples` and fixed-code `diagnostics`. Attempts count collection
entries (including failed/interrupted calls). Emitted/sample counters count only
emitter calls returning normally, including a delivered clearing event as an
unavailable sample. Failed/interrupted delivery might have partially written;
there is no retry or claim of transactional delivery. Emitter mutation cannot
alter counters, the collector's report or another event.

The summary explicitly retains authorization `none`, dispatchable/catalog-complete
`false`, native identity/lifecycle/worker acceptance and job completion
`unestablished`. `completed` describes **only the watch timer**, even if every
sample was unavailable. Exit codes are 0 completed, 130 interrupted, 1 all other
outcomes. Output formatting, CLI integration, final summary delivery and native
permissions remain coordinator responsibilities; no authorization follows.

## Focused offline validation

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -p test_session_watch.py -v
```

Tests use only fake collectors, emitters, clocks and sleepers; no server/native
application, model, provider or real-time waiting is required.
