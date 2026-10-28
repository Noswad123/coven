"""Coordinator-owned composed CLI tests; only private fake endpoints/clocks."""

import contextlib
import io
import json
import subprocess
from unittest import mock

from runner import coven, groups, session_inspection
from test_coven import CovenTestCase, snapshot
from test_group_publisher import FakeServer
from test_process_groups import process_handler
from test_session_inspection import generic


class Clock:
    def __init__(self):
        self.now = 0.0

    def monotonic(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds


class SessionToolCommandTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace, "--blank", "--agents", "operator")
        for name in ("run", "Popen"):
            patcher = mock.patch.object(subprocess, name, side_effect=AssertionError("native process"))
            guard = patcher.start()
            self.addCleanup(patcher.stop)
            self.addCleanup(guard.assert_not_called)

    def args(self, server, *, operation="list", extra=()):
        return ["session", operation, str(self.workspace), "--socket", server.path,
                "--json", *extra]

    def call(self, args, expected=0):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(list(map(str, args))), expected)
        return [json.loads(line) for line in output.getvalue().splitlines()]

    @contextlib.contextmanager
    def fast_watch(self):
        module = coven.session_watch_module()
        original = module.watch
        clock = Clock()
        def watch(collect, emit, **options):
            return original(collect, emit, clock=clock.monotonic, sleeper=clock.sleep, **options)
        with mock.patch.object(module, "watch", side_effect=watch):
            yield

    def bind(self, server):
        from runner import group_publisher
        groups.create_group(self.workspace, "team", "Review team")
        binding = group_publisher.binding_from_process(server.path, "w1:p1",
                                                       deadline=group_publisher.time.monotonic()+3)
        groups.add_member(self.workspace, "team", "reviewer", "Reviewer", binding)
        server.requests.clear()

    def test_list_filters_group_member_and_keeps_observed_counts(self):
        with FakeServer(generic(2), process_handler()) as server:
            self.bind(server)
            report, = self.call(self.args(server, extra=["--group", "team", "--member", "reviewer"]))
            self.assertEqual(report["target_count"], 1)
            self.assertEqual(report["view"]["observed_target_count"], 2)
            self.assertEqual(report["targets"][0]["pane_id"], "w1:p1")
            self.assertEqual(report["targets"][0]["associations"][0]["member_key"], "reviewer")
            self.assertEqual([r["method"] for r in server.requests],
                             ["session.snapshot", "pane.process_info", "pane.process_info", "session.snapshot"])
            self.assertEqual(report["authorization"], "none")

    def test_invalid_selection_fails_before_observation(self):
        with FakeServer(generic(), process_handler()) as server:
            for flags in (["--member", "reviewer"], ["--group", "BAD KEY"]):
                with self.subTest(flags=flags), contextlib.redirect_stderr(io.StringIO()), \
                        self.assertRaises(SystemExit), mock.patch.object(session_inspection, "inspect",
                        side_effect=AssertionError("observation before validation")):
                    coven.main(self.args(server, extra=flags))
            self.assertEqual(server.requests, [])

    def test_inspect_filters_do_not_override_the_expected_terminal_pin(self):
        with FakeServer(generic(), process_handler()) as server:
            self.bind(server)
            report, = self.call(self.args(server, operation="inspect", extra=["--pane", "w1:p1",
                "--expected-terminal", "wrong", "--group", "team"]), expected=1)
            self.assertEqual(report["assessment"], "unavailable")
            self.assertEqual(report["diagnostics"], ["terminal_mismatch"])
            self.assertEqual(report["targets"], [])
            self.assertIsNone(report["target_count"])

    def test_watch_emits_jsonl_then_scoped_summary_without_writes(self):
        with FakeServer(generic(), process_handler()) as server, self.fast_watch():
            before = snapshot(self.workspace)
            lines = self.call(self.args(server, operation="watch", extra=["--duration", "0.3", "--interval", "0.1"]))
            self.assertGreaterEqual(len(lines), 2)
            events, summary = lines[:-1], lines[-1]
            self.assertEqual(summary["contract"], "coven.session-watch-summary.v1")
            self.assertEqual(summary["outcome"], "completed")
            self.assertEqual(summary["emitted"], len(events))
            self.assertTrue(all(e["contract"] == "coven.session-watch.v1" for e in events))
            self.assertTrue(all(e["observation"]["authorization"] == "none" for e in events))
            self.assertTrue(all(e["observation"]["targets"][0]["activity"] == "unknown" for e in events))
            self.assertTrue(all(r["method"] in {"session.snapshot", "pane.process_info"} for r in server.requests))
            self.assertEqual(snapshot(self.workspace), before)

    def test_watch_and_filters_compose_after_each_new_observation(self):
        with FakeServer(generic(2), process_handler()) as server:
            self.bind(server)
            with self.fast_watch():
                lines = self.call(self.args(server, operation="watch", extra=["--duration", "0.2",
                    "--interval", "0.1", "--group", "team", "--recognized-only"]))
            for event in lines[:-1]:
                observed = event["observation"]
                self.assertEqual(observed["target_count"], 1)
                self.assertEqual(observed["view"]["observed_target_count"], 2)
                self.assertFalse(observed["native_catalog_complete"])

    def test_watch_caps_reader_budget_to_remaining_overall_duration(self):
        with FakeServer(generic(), process_handler()) as server:
            report = session_inspection.inspect(self.workspace, server.path)
            with self.fast_watch(), mock.patch.object(session_inspection, "inspect", return_value=report) as read:
                self.call(self.args(server, operation="watch", extra=["--duration", "0.2", "--interval", "0.1"]))
            self.assertGreaterEqual(read.call_count, 1)
            self.assertTrue(all(0 < call.kwargs["timeout"] <= 0.2 for call in read.call_args_list))

    def test_watch_unavailable_sample_never_reuses_previous_targets(self):
        with FakeServer(generic(), process_handler()) as server:
            success = session_inspection.inspect(self.workspace, server.path)
            failed = session_inspection.inspect(self.workspace, server.path,
                pane_id="w1:p1", expected_terminal_id="wrong")
            count = []
            def observations(*_args, **_kwargs):
                count.append(True)
                return success if len(count) == 1 else failed
            with self.fast_watch(), mock.patch.object(session_inspection, "inspect", side_effect=observations):
                lines = self.call(self.args(server, operation="watch", extra=["--duration", "0.3", "--interval", "0.1"]))
            self.assertGreaterEqual(len(lines), 3)
            self.assertTrue(lines[0]["observation"]["targets"])
            for event in lines[1:-1]:
                self.assertEqual(event["assessment"], "unavailable")
                self.assertEqual(event["observation"]["targets"], [])
                self.assertIsNone(event["observation"]["target_count"])
                self.assertEqual(event["observation"]["declarations"]["assessment"], "unavailable")
                self.assertIsNone(event["observation"]["declarations"]["revision"])

    def test_filter_does_not_bypass_full_observation_pane_limit(self):
        with FakeServer(generic(session_inspection.MAX_PANES + 1), process_handler()) as server:
            report, = self.call(self.args(server, extra=["--group", "team"]), expected=1)
            self.assertEqual(report["assessment"], "unavailable")
            self.assertEqual(report["diagnostics"], ["pane_limit"])
            self.assertIsNone(report["target_count"])
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"])

    def test_watch_terminal_pin_failure_remains_unavailable_after_selection(self):
        with FakeServer(generic(), process_handler()) as server, self.fast_watch():
            lines = self.call(self.args(server, operation="watch", extra=["--duration", "0.2",
                "--interval", "0.1", "--pane", "w1:p1", "--expected-terminal", "wrong",
                "--group", "team"]))
            self.assertEqual(lines[-1]["observed_samples"], 0)
            self.assertGreaterEqual(lines[-1]["unavailable_samples"], 1)
            for event in lines[:-1]:
                self.assertEqual(event["diagnostics"], ["terminal_mismatch"])
                self.assertEqual(event["observation"]["targets"], [])
                self.assertIsNone(event["observation"]["view"]["observed_target_count"])

    def test_watch_broken_output_is_not_retried_for_summary(self):
        with FakeServer(generic(), process_handler()) as server:
            report = session_inspection.inspect(self.workspace, server.path)
            with self.fast_watch(), mock.patch.object(session_inspection, "inspect", return_value=report) as read, \
                 mock.patch("builtins.print", side_effect=BrokenPipeError) as output:
                result = coven.main(self.args(server, operation="watch", extra=["--duration", "0.2"]))
            self.assertEqual(result, 1)
            read.assert_called_once()
            output.assert_called_once()

    def test_watch_collector_exception_clears_and_stops_without_private_error_text(self):
        with FakeServer(generic(), process_handler()) as server, self.fast_watch(), \
             mock.patch.object(session_inspection, "inspect", side_effect=RuntimeError("PRIVATE error")):
            lines = self.call(self.args(server, operation="watch", extra=["--duration", "0.2"]), expected=1)
        self.assertIsNone(lines[0]["observation"])
        self.assertEqual(lines[0]["assessment"], "unavailable")
        self.assertEqual(lines[-1]["outcome"], "collector_failed")
        self.assertNotIn("PRIVATE", json.dumps(lines))

    def test_invalid_watch_bounds_and_terminal_without_pane_fail_before_collection(self):
        with FakeServer(generic(), process_handler()) as server:
            for flags in (["--interval", "0"], ["--duration", "nan"], ["--expected-terminal", "terminal-1"]):
                with self.subTest(flags=flags), contextlib.redirect_stderr(io.StringIO()), \
                     self.assertRaises(SystemExit), mock.patch.object(session_inspection, "inspect",
                     side_effect=AssertionError("collection before validation")):
                    coven.main(self.args(server, operation="watch", extra=flags))
            self.assertEqual(server.requests, [])
