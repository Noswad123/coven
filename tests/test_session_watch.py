"""Deterministic foreground-watch tests: no server, native runtime or wall sleeps."""

from copy import deepcopy
import inspect
import json
import socket
import subprocess
import unittest
from unittest import mock

from runner import session_watch as watch


def report(assessment="observed", *, recognized=True):
    observed = assessment == "observed"
    return {
        "schema_version": 1, "contract": "coven.session-inspection.v1",
        "scope": "existing_panes_on_selected_kitsune_endpoint", "runtime": "djinn",
        "workspace": "/fixture/workspace",
        "request": {"mode": "list", "pane_id": None, "expected_terminal_id": None},
        "endpoint": {"selected": "/fixture/socket", "resolved": "/fixture/socket", "device": 1, "inode": 2},
        "started_at": "2026-10-26T00:00:00+00:00", "observed_at": "2026-10-26T00:00:00+00:00",
        "duration_ms": 0, "assessment": assessment,
        "diagnostics": [] if observed else ["unsupported_protocol" if assessment == "incompatible" else "timeout"],
        "targets": [{
            "pane_id": "w1:p1", "terminal_id": "terminal-1", "workspace_id": "w1", "tab_id": "t1",
            "provenance": "kitsune.pane.process_info", "mapping_evidence": "matching_inventory_brackets",
            "recognition": "djinn_executable_family" if recognized else "unknown",
            "matches": [{"pid": 10, "executable_family": "djinn", "rule": "argv0_basename"}] if recognized else [],
            "activity": "unknown", "attention": "unknown", "native_identity": "unestablished",
            "native_lifecycle": "unestablished", "job_completion": "unestablished",
            "associations": [{"group_key": "team", "member_key": "one", "role": "Reviewer",
                              "binding_check": "process_evidence_changed",
                              "declared_native_reference": {"kind": "id", "value": "historical"},
                              "native_identity": "unestablished", "live_attachment_declared": True}],
        }] if observed else [],
        "target_count": 1 if observed else None,
        "recognized_target_count": int(recognized) if observed else None,
        "declarations": {"assessment": "observed" if observed else "unavailable",
                         "revision": "groups-v1:fixture" if observed else None,
                         "authority_schema_version": 3 if observed else None, "unmatched": [],
                         "other_endpoint_count": 0 if observed else None,
                         "outside_inspection_count": 0 if observed else None},
        "authorization": "none", "dispatchable": False, "native_catalog_complete": False,
        "native_identity": "unestablished", "native_lifecycle": "unestablished",
        "native_worker_acceptance": "unestablished", "job_completion": "unestablished",
        "runtime_fact_freshness": "unknown", "server_snapshot_timestamp": None,
        "limitations": ["Declarations are historical, not verified native identity or permissions."],
    }


class FakeClock:
    def __init__(self, start=0):
        self.value = start
        self.sleeps = []

    def __call__(self):
        return self.value

    def sleep(self, seconds):
        self.sleeps.append(seconds)
        self.value += seconds


class SessionWatchTests(unittest.TestCase):
    def setUp(self):
        self.clock = FakeClock()
        self.events = []
        self.starts = []
        self.budgets = []
        # Catch accidental engine I/O even if a failure handler swallows it.
        for obj, name in ((subprocess, "run"), (subprocess, "Popen"), (socket, "socket")):
            patcher = mock.patch.object(obj, name, side_effect=AssertionError("engine I/O forbidden"))
            guard = patcher.start()
            self.addCleanup(patcher.stop)
            self.addCleanup(guard.assert_not_called)
        patcher = mock.patch("builtins.open", side_effect=AssertionError("engine filesystem I/O forbidden"))
        guard = patcher.start()
        self.addCleanup(patcher.stop)
        self.addCleanup(guard.assert_not_called)

    def collect(self, remaining):
        self.starts.append(self.clock.value)
        self.budgets.append(remaining)
        return report()

    def run_watch(self, collect=None, emit=None, **options):
        defaults = {"duration": 5, "interval": 2, "clock": self.clock, "sleeper": self.clock.sleep}
        defaults.update(options)
        return watch.watch(self.collect if collect is None else collect,
                           self.events.append if emit is None else emit, **defaults)

    def assert_summary(self, summary, outcome="completed", *, attempts=3, emitted=3, samples=(3, 0, 0)):
        self.assertEqual(summary["schema_version"], 1)
        self.assertEqual(summary["contract"], "coven.session-watch-summary.v1")
        self.assertEqual(summary["outcome"], outcome)
        self.assertEqual(summary["poll_attempts"], attempts)
        self.assertEqual(summary["emitted"], emitted)
        self.assertEqual(tuple(summary[key + "_samples"] for key in watch.ASSESSMENTS), samples)
        self.assertEqual(summary["diagnostics"], [] if outcome == "completed" else [
            "collection_failed" if outcome == "collector_failed" else outcome])
        self.assertEqual(summary["authorization"], "none")
        self.assertIs(summary["dispatchable"], False)
        self.assertIs(summary["native_catalog_complete"], False)
        for field in ("native_identity", "native_lifecycle", "native_worker_acceptance", "job_completion"):
            self.assertEqual(summary[field], "unestablished")
        json.dumps(summary, allow_nan=False)

    def test_defaults_are_sixty_seconds_and_two_seconds(self):
        parameters = inspect.signature(watch.watch).parameters
        self.assertEqual(parameters["duration"].default, 60)
        self.assertEqual(parameters["interval"].default, 2)

    def test_immediate_first_poll_sequence_budget_and_exact_envelopes(self):
        summary = self.run_watch()
        self.assert_summary(summary)
        self.assertEqual(self.starts, [0, 2, 4])
        self.assertEqual(self.budgets, [5, 3, 1])
        self.assertEqual(self.clock.sleeps, [2, 2, 1])
        for index, event in enumerate(self.events, 1):
            self.assertEqual(event, {"schema_version": 1, "contract": "coven.session-watch.v1",
                "sequence": index, "elapsed_ms": (index - 1) * 2000, "assessment": "observed",
                "diagnostics": [], "observation": report()})
            json.dumps(event, allow_nan=False)

    def test_validation_precedes_all_callbacks(self):
        for name, values in (("duration", [False, True, None, "1", 0, -1, .099, 3600.1,
                                          float("nan"), float("inf"), -float("inf"), 10 ** 1000]),
                             ("interval", [False, True, None, "1", 0, -1, .099, 60.1,
                                          float("nan"), float("inf"), -float("inf"), 10 ** 1000])):
            for value in values:
                with self.subTest(name=name, value=str(value)):
                    callbacks = [mock.Mock() for _ in range(4)]
                    with self.assertRaises(ValueError):
                        watch.watch(callbacks[0], callbacks[1], clock=callbacks[2], sleeper=callbacks[3],
                                    **{name: value})
                    for callback in callbacks:
                        callback.assert_not_called()

    def test_numeric_subclasses_are_not_literal_numbers(self):
        class Int(int):
            pass

        class Float(float):
            pass

        for value in (Int(1), Float(1)):
            with self.subTest(value=type(value)), self.assertRaises(ValueError):
                self.run_watch(duration=value)
        self.assertEqual(self.starts, [])

    def test_noncallables_fail_before_clock_or_collection(self):
        for name in ("collect", "emit", "clock", "sleeper"):
            callbacks = {key: mock.Mock() for key in ("collect", "emit", "clock", "sleeper")}
            callbacks[name] = None
            with self.subTest(name=name), self.assertRaises(ValueError):
                watch.watch(**callbacks)
            for callback in callbacks.values():
                if callback is not None:
                    callback.assert_not_called()

    def test_inclusive_numeric_limits_and_interval_larger_than_duration(self):
        for duration, interval in ((.1, .1), (.1, 60), (3600, 60), (3600.0, 60.0)):
            with self.subTest(duration=duration, interval=interval):
                clock = FakeClock()
                events = []
                summary = watch.watch(lambda remaining: report(), events.append, duration=duration,
                                      interval=interval, clock=clock, sleeper=clock.sleep)
                count = 1 if duration == .1 else 60
                self.assert_summary(summary, attempts=count, emitted=count, samples=(count, 0, 0))
                self.assertEqual(clock.value, duration)

    def test_no_collection_on_deadline_tick(self):
        summary = self.run_watch(duration=4)
        self.assert_summary(summary, attempts=2, emitted=2, samples=(2, 0, 0))
        self.assertEqual(self.starts, [0, 2])

    def test_collection_cost_skips_ticks_without_overlap_or_catchup(self):
        active = False

        def collect(remaining):
            nonlocal active
            self.assertFalse(active)
            active = True
            result = self.collect(remaining)
            self.clock.value += 5
            active = False
            return result

        summary = self.run_watch(collect=collect, duration=12)
        self.assert_summary(summary, attempts=2, emitted=2, samples=(2, 0, 0))
        self.assertEqual(self.starts, [0, 6])
        self.assertEqual(self.budgets, [12, 6])
        self.assertEqual(self.clock.sleeps, [1, 1])
        self.assertEqual([e["elapsed_ms"] for e in self.events], [5000, 11000])

    def test_emitter_time_also_skips_missed_ticks(self):
        def emit(event):
            self.events.append(event)
            self.clock.value += 3

        summary = self.run_watch(emit=emit, duration=9)
        self.assert_summary(summary)
        self.assertEqual(self.starts, [0, 4, 8])
        self.assertEqual(self.clock.value, 11)

    def test_callback_ending_exactly_on_tick_uses_next_future_tick(self):
        def collect(remaining):
            result = self.collect(remaining)
            self.clock.value += 2
            return result

        summary = self.run_watch(collect=collect, duration=5)
        self.assert_summary(summary, attempts=2, emitted=2, samples=(2, 0, 0))
        self.assertEqual(self.starts, [0, 4])

    def test_inflight_collection_can_emit_after_deadline_without_another_poll(self):
        def collect(remaining):
            result = self.collect(remaining)
            self.clock.value += 7
            return result

        summary = self.run_watch(collect=collect)
        self.assert_summary(summary, attempts=1, emitted=1, samples=(1, 0, 0))
        self.assertEqual(self.events[0]["elapsed_ms"], 7000)
        self.assertEqual(self.clock.sleeps, [])

    def test_oversleep_never_starts_a_late_collection(self):
        def oversleep(seconds):
            self.clock.value += seconds + 10

        summary = self.run_watch(sleeper=oversleep)
        self.assert_summary(summary, attempts=1, emitted=1, samples=(1, 0, 0))
        self.assertEqual(self.starts, [0])

    def test_early_wakes_wait_for_target_instead_of_polling_early(self):
        sleeps = []

        def early_sleep(seconds):
            sleeps.append(seconds)
            self.clock.value += min(.5, seconds)

        summary = self.run_watch(sleeper=early_sleep)
        self.assert_summary(summary)
        self.assertEqual(self.starts, [0, 2, 4])
        self.assertEqual(len(sleeps), 10)

    def test_ticks_are_anchored_to_nonzero_initial_clock(self):
        self.clock.value = 100
        summary = self.run_watch()
        self.assert_summary(summary)
        self.assertEqual(self.starts, [100, 102, 104])
        self.assertEqual([event["elapsed_ms"] for event in self.events], [0, 2000, 4000])

    def test_fractional_duration_never_starts_after_budget(self):
        summary = self.run_watch(duration=.55, interval=.2)
        self.assert_summary(summary)
        for actual, expected in zip(self.starts, (0, .2, .4)):
            self.assertAlmostEqual(actual, expected)
        for actual, expected in zip(self.budgets, (.55, .35, .15)):
            self.assertAlmostEqual(actual, expected)
        self.assertAlmostEqual(self.clock.value, .55)

    def test_fractional_tick_roundoff_does_not_falsely_fail_a_valid_clock(self):
        for start in (0, 100):
            with self.subTest(start=start):
                self.clock = FakeClock(start)
                self.starts = []
                self.events = []
                summary = self.run_watch(duration=10, interval=.1)
                self.assert_summary(summary, attempts=100, emitted=100, samples=(100, 0, 0))
                for tick, actual in enumerate(self.starts):
                    self.assertAlmostEqual(actual, start + tick * .1)
                self.assertAlmostEqual(self.clock.value, start + 10)

    def test_unavailable_incompatible_then_new_observed_sample_no_fallback(self):
        reports = [report("observed"), report("unavailable"), report("incompatible"), report("observed", recognized=False)]
        reports[-1]["targets"][0]["terminal_id"] = "replacement-terminal"
        source = iter(reports)
        summary = self.run_watch(collect=lambda remaining: next(source), duration=7)
        self.assert_summary(summary, attempts=4, emitted=4, samples=(2, 1, 1))
        self.assertEqual([event["observation"] for event in self.events], reports)
        for event in self.events[1:3]:
            self.assertEqual(event["observation"]["targets"], [])
            self.assertIsNone(event["observation"]["target_count"])
            self.assertEqual(event["diagnostics"], event["observation"]["diagnostics"])
            self.assertIsNot(event["diagnostics"], event["observation"]["diagnostics"])
        self.assertEqual(self.events[-1]["observation"]["targets"][0]["terminal_id"], "replacement-terminal")

    def test_all_normal_failures_complete_timer_without_native_success(self):
        summary = self.run_watch(collect=lambda remaining: report("unavailable"))
        self.assert_summary(summary, samples=(0, 3, 0))
        self.assertEqual(watch.exit_code(summary), 0)

    def test_observed_empty_and_unknown_do_not_become_native_absence_or_activity(self):
        empty = report()
        empty.update(targets=[], target_count=0, recognized_target_count=0)
        empty["declarations"]["assessment"] = "uninitialized"
        for value in (empty, report(recognized=False)):
            with self.subTest(count=value["target_count"]):
                events = []
                summary = self.run_watch(collect=lambda remaining: value, emit=events.append, duration=.1)
                self.assert_summary(summary, attempts=1, emitted=1, samples=(1, 0, 0))
                self.assertEqual(events[0]["observation"], value)

    def test_view_scope_stale_declarations_and_all_qualifiers_are_preserved(self):
        value = report()
        value["view"] = {"group_key": "team", "member_key": None, "recognized_only": True,
                         "observed_target_count": 5, "observed_recognized_target_count": 3,
                         "declaration_count_scope": "full_observation"}
        value["declarations"]["other_endpoint_count"] = 12
        before = deepcopy(value)
        self.run_watch(collect=lambda remaining: value, duration=.1)
        self.assertEqual(self.events[0]["observation"], before)
        self.assertEqual(value, before)
        self.assertIsNot(self.events[0]["observation"], value)

    def test_independent_nested_copies_even_when_collector_reuses_same_report(self):
        value = report()
        before = deepcopy(value)
        summary = self.run_watch(collect=lambda remaining: value)
        self.assert_summary(summary)
        self.events[0]["observation"]["targets"][0]["associations"][0]["role"] = "changed"
        self.events[0]["observation"]["limitations"].append("changed")
        self.events[0]["diagnostics"].append("changed")
        self.assertEqual(value, before)
        self.assertEqual(self.events[1]["observation"], before)
        value["targets"].clear()
        self.assertEqual(self.events[2]["observation"], before)

    def test_emitter_mutation_does_not_change_counts_or_next_event(self):
        value = report()
        before = deepcopy(value)

        def emit(event):
            self.events.append(deepcopy(event))
            event.update(sequence=99, assessment="unavailable", observation=None)
            event["diagnostics"].append("mutated")

        summary = self.run_watch(collect=lambda remaining: value, emit=emit)
        self.assert_summary(summary)
        self.assertEqual(value, before)
        self.assertEqual([event["sequence"] for event in self.events], [1, 2, 3])
        self.assertTrue(all(event["diagnostics"] == [] for event in self.events))

    def test_collector_exception_emits_one_clearing_event_then_stops_privately(self):
        source = mock.Mock(side_effect=[report(), RuntimeError("SECRET native argv /private")])
        summary = self.run_watch(collect=source)
        self.assert_summary(summary, "collector_failed", attempts=2, emitted=2, samples=(1, 1, 0))
        self.assertEqual(source.call_count, 2)
        self.assertEqual(self.events[-1], {"schema_version": 1, "contract": "coven.session-watch.v1",
            "sequence": 2, "elapsed_ms": 2000, "assessment": "unavailable",
            "diagnostics": ["collection_failed"], "observation": None})
        self.assertNotIn("SECRET", json.dumps([summary, self.events]))

    def test_invalid_report_shapes_and_invariants_clear_and_stop(self):
        invalid = [None, [], {}, "SECRET raw data"]
        mutations = [
            {"schema_version": True}, {"schema_version": 2}, {"contract": "other.v1"},
            {"assessment": "ready"}, {"assessment": []}, {"diagnostics": "SECRET"},
            {"diagnostics": [RuntimeError("SECRET")]}, {"targets": None}, {"targets": [None]},
            {"target_count": True}, {"target_count": 2}, {"recognized_target_count": -1},
            {"recognized_target_count": 0}, {"declarations": None},
            {"authorization": "launch"}, {"dispatchable": True}, {"native_catalog_complete": True},
            {"native_lifecycle": "running"}, {"native_identity": "verified"},
            {"native_worker_acceptance": "accepted"}, {"job_completion": "completed"},
            {"runtime_fact_freshness": "fresh"}, {"duration_ms": float("nan")},
            {"extra": object()}, {"extra": (1, 2)}, {"extra": {1: "not a JSON key"}},
        ]
        for mutation in mutations:
            value = report()
            value.update(mutation)
            invalid.append(value)
        missing = report()
        del missing["target_count"]
        invalid.append(missing)
        for field, bad in (("activity", "idle"), ("attention", "ready"), ("recognition", "native"),
                           ("native_identity", "verified"), ("native_lifecycle", "running"),
                           ("job_completion", "done"), ("associations", None)):
            value = report()
            value["targets"][0][field] = bad
            invalid.append(value)
        cyclic = report()
        cyclic["cycle"] = cyclic
        invalid.append(cyclic)
        for index, value in enumerate(invalid):
            with self.subTest(index=index):
                events = []
                collect = mock.Mock(return_value=value)
                summary = self.run_watch(collect=collect, emit=events.append)
                self.assert_summary(summary, "collector_failed", attempts=1, emitted=1, samples=(0, 1, 0))
                collect.assert_called_once()
                self.assertIsNone(events[0]["observation"])
                self.assertEqual(events[0]["diagnostics"], ["collection_failed"])
                self.assertNotIn("SECRET", json.dumps([summary, events]))

    def test_failure_reports_cannot_retain_positive_rows_or_declarations(self):
        mutations = [
            lambda value: value.update(target_count=0),
            lambda value: value.update(recognized_target_count=0),
            lambda value: value.update(targets=report()["targets"]),
            lambda value: value["declarations"].update(assessment="observed"),
            lambda value: value["declarations"].update(unmatched=[{"role": "Reviewer"}]),
            lambda value: value["declarations"].update(revision="old-revision"),
            lambda value: value["declarations"].update(authority_schema_version=3),
            lambda value: value["declarations"].update(other_endpoint_count=0),
            lambda value: value["declarations"].update(outside_inspection_count=0),
        ]
        for assessment in ("unavailable", "incompatible"):
            for index, mutate in enumerate(mutations):
                with self.subTest(assessment=assessment, index=index):
                    value = report(assessment)
                    mutate(value)
                    events = []
                    summary = self.run_watch(collect=lambda remaining: value, emit=events.append)
                    self.assert_summary(summary, "collector_failed", attempts=1, emitted=1, samples=(0, 1, 0))
                    self.assertIsNone(events[0]["observation"])

    def test_output_failure_stops_without_retry_or_exception_text(self):
        emit = mock.Mock(side_effect=BrokenPipeError("SECRET peer output"))
        summary = self.run_watch(emit=emit)
        self.assert_summary(summary, "output_failed", attempts=1, emitted=0, samples=(0, 0, 0))
        emit.assert_called_once()
        self.assertEqual(self.clock.sleeps, [])
        self.assertNotIn("SECRET", json.dumps(summary))

    def test_clearing_output_failure_takes_precedence_without_retry(self):
        collect = mock.Mock(side_effect=RuntimeError("SECRET collection"))
        emit = mock.Mock(side_effect=OSError("SECRET output"))
        summary = self.run_watch(collect=collect, emit=emit)
        self.assert_summary(summary, "output_failed", attempts=1, emitted=0, samples=(0, 0, 0))
        self.assertIsNone(emit.call_args.args[0]["observation"])
        collect.assert_called_once()
        emit.assert_called_once()

    def test_keyboard_interrupt_from_every_callback_is_interrupted(self):
        for name in ("collect", "emit", "clock", "sleeper"):
            with self.subTest(name=name):
                self.clock = FakeClock()
                self.events = []
                options = {name: mock.Mock(side_effect=KeyboardInterrupt)}
                summary = self.run_watch(**options)
                emitted = 1 if name == "sleeper" else 0
                attempts = 0 if name == "clock" else 1
                self.assert_summary(summary, "interrupted", attempts=attempts, emitted=emitted,
                                    samples=(emitted, 0, 0))
                self.assertEqual(watch.exit_code(summary), 130)

    def test_initial_invalid_or_unrepresentable_clock_fails_before_collection(self):
        for value in (float("nan"), float("inf"), -float("inf"), True, "0", None, 10 ** 1000, 1e308):
            with self.subTest(value=str(value)):
                collect, emit = mock.Mock(), mock.Mock()
                summary = self.run_watch(collect=collect, emit=emit, clock=lambda: value)
                self.assert_summary(summary, "clock_failed", attempts=0, emitted=0, samples=(0, 0, 0))
                collect.assert_not_called()
                emit.assert_not_called()

    def test_clock_exception_text_is_private(self):
        summary = self.run_watch(clock=mock.Mock(side_effect=OSError("SECRET clock")))
        self.assert_summary(summary, "clock_failed", attempts=0, emitted=0, samples=(0, 0, 0))
        self.assertNotIn("SECRET", json.dumps(summary))

    def test_invalid_or_regressing_clock_after_collect_never_emits_report(self):
        for value in (-1, float("nan"), float("inf"), True, 1e308):
            with self.subTest(value=value):
                events = []
                summary = self.run_watch(clock=mock.Mock(side_effect=[0, value]), emit=events.append)
                self.assert_summary(summary, "clock_failed", attempts=1, emitted=0, samples=(0, 0, 0))
                self.assertEqual(events, [])

    def test_clock_regression_after_emit_stops_further_collection(self):
        summary = self.run_watch(clock=mock.Mock(side_effect=[0, 0, -1]))
        self.assert_summary(summary, "clock_failed", attempts=1, emitted=1, samples=(1, 0, 0))

    def test_stalled_clock_after_sleep_fails_closed_instead_of_spinning(self):
        sleeper = mock.Mock()
        summary = self.run_watch(sleeper=sleeper)
        self.assert_summary(summary, "clock_failed", attempts=1, emitted=1, samples=(1, 0, 0))
        sleeper.assert_called_once_with(2)

    def test_regressing_or_nonfinite_clock_after_sleep_fails_closed(self):
        for value in (-1, float("nan"), float("inf")):
            with self.subTest(value=value):
                sleeper = mock.Mock()
                events = []
                summary = self.run_watch(clock=mock.Mock(side_effect=[0, 0, 0, value]),
                                         sleeper=sleeper, emit=events.append)
                self.assert_summary(summary, "clock_failed", attempts=1, emitted=1, samples=(1, 0, 0))
                sleeper.assert_called_once()

    def test_sleeper_failure_is_private_clock_failure(self):
        summary = self.run_watch(sleeper=mock.Mock(side_effect=OSError("SECRET sleeper")))
        self.assert_summary(summary, "clock_failed", attempts=1, emitted=1, samples=(1, 0, 0))
        self.assertNotIn("SECRET", json.dumps(summary))

    def test_exit_codes(self):
        for outcome, expected in (("completed", 0), ("interrupted", 130), ("collector_failed", 1),
                                  ("output_failed", 1), ("clock_failed", 1), ("unknown", 1)):
            with self.subTest(outcome=outcome):
                self.assertEqual(watch.exit_code({"outcome": outcome}), expected)


if __name__ == "__main__":
    unittest.main()
