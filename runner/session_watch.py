"""Serial, bounded foreground observations; no native control or output formatting.

The collector owns bounded inspection and the emitter owns delivery. Neither a
watch timer nor a successful sample establishes native readiness or completion.
"""

from copy import deepcopy
import math
import time


CONTRACT = "coven.session-watch.v1"
SUMMARY_CONTRACT = "coven.session-watch-summary.v1"
INSPECTION_CONTRACT = "coven.session-inspection.v1"
ASSESSMENTS = ("observed", "unavailable", "incompatible")


class _ClockFailure(Exception):
    pass


def _require(condition):
    if not condition:
        raise ValueError("invalid inspection report")


def _json_data(value, active=None):
    """Reject non-JSON objects/cycles before deepcopy can invoke object hooks."""
    if value is None or type(value) in (str, bool, int):
        return
    if type(value) is float:
        _require(math.isfinite(value))
        return
    _require(type(value) in (dict, list))
    active = set() if active is None else active
    _require(id(value) not in active)
    active.add(id(value))
    if type(value) is dict:
        _require(all(type(key) is str for key in value))
        children = value.values()
    else:
        children = value
    for child in children:
        _json_data(child, active)
    active.remove(id(value))


def _observation(report):
    """Check the callback boundary, not replace the bounded inspection decoder.

    The collector must supply an already validated inspection-v1 report. Preserve
    its scope, declarations, diagnostics, limitations and optional view verbatim.
    Check envelope/count/failure and no-authority invariants before forwarding it.
    """
    _require(type(report) is dict)
    _json_data(report)
    _require(type(report.get("schema_version")) is int and report["schema_version"] == 1)
    _require(report.get("contract") == INSPECTION_CONTRACT)
    _require(report.get("assessment") in ASSESSMENTS)
    _require(type(report.get("diagnostics")) is list
             and all(type(code) is str for code in report["diagnostics"]))
    _require(report.get("authorization") == "none" and report.get("dispatchable") is False
             and report.get("native_catalog_complete") is False)
    for field in ("native_identity", "native_lifecycle", "native_worker_acceptance", "job_completion"):
        _require(report.get(field) == "unestablished")
    _require(report.get("runtime_fact_freshness") == "unknown")
    _require(type(report.get("targets")) is list)
    declarations = report.get("declarations")
    _require(type(declarations) is dict and type(declarations.get("unmatched")) is list)
    _require("target_count" in report and "recognized_target_count" in report)
    if report["assessment"] == "observed":
        for field in ("target_count", "recognized_target_count"):
            _require(type(report[field]) is int and report[field] >= 0)
        _require(report["target_count"] == len(report["targets"]))
        for target in report["targets"]:
            _require(type(target) is dict
                     and target.get("recognition") in ("unknown", "djinn_executable_family")
                     and type(target.get("associations")) is list)
            _require(target.get("activity") == "unknown" and target.get("attention") == "unknown")
            for field in ("native_identity", "native_lifecycle", "job_completion"):
                _require(target.get(field) == "unestablished")
        _require(report["recognized_target_count"] == sum(
            target["recognition"] == "djinn_executable_family" for target in report["targets"]))
        _require(declarations.get("assessment") in ("observed", "uninitialized"))
    else:
        _require(report["targets"] == [] and report["target_count"] is None
                 and report["recognized_target_count"] is None)
        _require(declarations.get("assessment") == "unavailable"
                 and declarations["unmatched"] == []
                 and declarations.get("revision") is None
                 and declarations.get("authority_schema_version") is None
                 and declarations.get("other_endpoint_count") is None
                 and declarations.get("outside_inspection_count") is None)
    return deepcopy(report)


class _Clock:
    def __init__(self, read, sleeper):
        self.read = read
        self.sleeper = sleeper
        self.previous = None

    def now(self):
        try:
            value = self.read()
            if (type(value) not in (int, float) or not math.isfinite(value)
                    or (self.previous is not None and value < self.previous)):
                raise _ClockFailure()
        except Exception:
            raise _ClockFailure() from None
        self.previous = value
        return value

    def wait(self, target, now):
        while now < target:
            try:
                self.sleeper(target - now)
            except Exception:
                raise _ClockFailure() from None
            after = self.now()
            # A sleeper that returns without clock progress must not busy-spin.
            if after <= now:
                raise _ClockFailure()
            now = after
        return now


def watch(collect, emit, *, duration=60, interval=2, clock=time.monotonic, sleeper=time.sleep):
    """Collect immediately, then on future origin-anchored ticks, without overlap.

    Validate options before any callbacks. collect(remaining_seconds) must cap its
    inspection timeout to min(API budget, remaining_seconds). Blocking callbacks
    cannot be interrupted: an in-flight sample may be delivered after the deadline,
    but no new collection starts at/after it. elapsed_ms is local envelope creation
    time, not native freshness. Counters count only emit calls that return normally.
    """
    for name, value, upper in (("duration", duration, 3600), ("interval", interval, 60)):
        if type(value) not in (int, float) or not 0.1 <= value <= upper:
            raise ValueError(f"{name} must be a finite int/float in [0.1,{upper}]")
    if not all(callable(callback) for callback in (collect, emit, clock, sleeper)):
        raise ValueError("collect, emit, clock and sleeper must be callable")

    summary = {
        "schema_version": 1, "contract": SUMMARY_CONTRACT, "outcome": "completed",
        "poll_attempts": 0, "emitted": 0, "observed_samples": 0,
        "unavailable_samples": 0, "incompatible_samples": 0, "diagnostics": [],
        "authorization": "none", "dispatchable": False, "native_catalog_complete": False,
        "native_identity": "unestablished", "native_lifecycle": "unestablished",
        "native_worker_acceptance": "unestablished", "job_completion": "unestablished",
    }
    timer = _Clock(clock, sleeper)
    try:
        started = now = timer.now()
        deadline = started + duration
        if not math.isfinite(deadline) or deadline <= started:
            raise _ClockFailure()
        while now < deadline:
            summary["poll_attempts"] += 1
            failed = False
            try:
                observation = _observation(collect(deadline - now))
            except Exception:
                observation = None
                failed = True
            now = timer.now()
            assessment = "unavailable" if failed else observation["assessment"]
            elapsed_ms = (now - started) * 1000
            if not math.isfinite(elapsed_ms):
                raise _ClockFailure()
            event = {
                "schema_version": 1, "contract": CONTRACT, "sequence": summary["emitted"] + 1,
                "elapsed_ms": round(elapsed_ms, 3), "assessment": assessment,
                "diagnostics": ["collection_failed"] if failed else list(observation["diagnostics"]),
                "observation": observation,
            }
            try:
                emit(event)
            except Exception:
                summary.update(outcome="output_failed", diagnostics=["output_failed"])
                break
            summary["emitted"] += 1
            summary[assessment + "_samples"] += 1
            if failed:
                summary.update(outcome="collector_failed", diagnostics=["collection_failed"])
                break
            now = timer.now()
            if now >= deadline:
                break
            # Strictly future tick: a callback ending exactly on a tick skips it.
            tick = math.floor((now - started) / interval) + 1
            next_start = started + tick * interval
            # Floating division/subtraction can round the current tick down (for
            # example 4.3 / 0.1). Advance once before treating it as clock failure.
            if next_start <= now:
                next_start = started + (tick + 1) * interval
            if not math.isfinite(next_start) or next_start <= now:
                raise _ClockFailure()
            now = timer.wait(min(next_start, deadline), now)
    except KeyboardInterrupt:
        summary.update(outcome="interrupted", diagnostics=["interrupted"])
    except _ClockFailure:
        summary.update(outcome="clock_failed", diagnostics=["clock_failed"])
    return summary


def exit_code(summary):
    """Only the watch timer completes; no native job-success interpretation."""
    return 0 if summary["outcome"] == "completed" else 130 if summary["outcome"] == "interrupted" else 1
