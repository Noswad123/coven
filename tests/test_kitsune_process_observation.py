"""Shadow recognition uses only private fake endpoints and sanitized process data."""

import contextlib
import io
import json
import socket
import subprocess
import time
import unittest
from unittest import mock

from runner import coven, kitsune_observation as observation
from runner import kitsune_process_observation as process_observation
from test_kitsune_observation import FakeSocket, ROOT, reply, sender, snapshot


def pane_snapshot():
    value = snapshot(True)
    value["agents"] = []
    # Deliberately no Kitsune agent recognition, status or native identity evidence.
    value["panes"][0].update(agent=None, agent_status="unknown", agent_session=None)
    return value


def process(pid=10, argv=None, **extra):
    return {"pid": pid, "name": "untrusted-name", "argv": argv, **extra}


def process_reply(request, processes=None, **extra):
    return json.dumps({"id": request["id"], "result": {
        "type": "pane_process_info", "process_info": {
            "pane_id": "pane:1", "foreground_processes": processes or [], **extra,
        }}}).encode() + b"\n"


class SequenceSocket(FakeSocket):
    """One connection per request, EOF framing, accepting a bounded read sequence."""

    def __init__(self, handlers):
        super().__init__(None)
        self.handlers = handlers

    def serve(self):
        try:
            for handler in self.handlers:
                while not self.stop.is_set():
                    try:
                        stream, _ = self.listener.accept()
                        break
                    except socket.timeout:
                        continue
                else:
                    return
                with stream:
                    stream.settimeout(1)
                    data = bytearray()
                    while b"\n" not in data and len(data) < 8192:
                        part = stream.recv(8192)
                        if not part:
                            return
                        data.extend(part)
                    self.requests.append(json.loads(data))
                    handler(self, stream, self.requests[-1])
        except (BrokenPipeError, ConnectionResetError):
            pass
        except Exception as exc:
            if not self.stop.is_set():
                self.errors.append(exc)


def sequence(processes=None, before=None, after=None):
    return [sender(lambda r: reply(r, pane_snapshot() if before is None else before)),
            sender(lambda r: process_reply(r, processes)),
            sender(lambda r: reply(r, pane_snapshot() if after is None else after))]


class RecognitionTests(unittest.TestCase):
    def recognize(self, processes):
        payload = process_reply({"id": "r"}, processes)
        return process_observation._processes(payload[:-1], "r", "pane:1")

    def test_exact_native_and_ui_basename_without_mode_inference(self):
        for executable in ("djinn", "/usr/local/bin/djinn", "djinn-ui", "/tmp/djinn-ui"):
            for args in ([], ["--help"], ["session", "list"], ["-s", "SECRET SESSION"],
                         ["ask", "SECRET PROMPT", "--api-key", "SECRET KEY"]):
                with self.subTest(executable=executable, args=args):
                    match, = self.recognize([process(argv=[executable, *args])])
                    self.assertEqual(match, {"pid": 10, "executable_family": executable.rsplit("/", 1)[-1],
                                            "rule": "exact_posix_argv0_basename"})

    def test_no_substrings_wrappers_or_name_fallback(self):
        for argv in (None, [], [""], ["DJINN"], ["djinn.exe"], ["not-djinn"],
                     ["/djinn/"], ["djinn "], ["/path/djinn --help"],
                     ["sh", "-c", "djinn"], ["env", "djinn"], ["cargo", "run", "djinn"],
                     ["rg", "djinn"], ["node", "/repo/bin/djinn-ui"],
                     ["bun", "run", "./src/index.ts"], ["opencode"]):
            with self.subTest(argv=argv):
                self.assertEqual(self.recognize([process(argv=argv, name="djinn", argv0="djinn",
                    cmdline="djinn", cwd="/repo/djinn")]), [])

    def test_missing_optional_process_inventory_and_argv(self):
        root = json.loads(process_reply({"id": "r"}))
        del root["result"]["process_info"]["foreground_processes"]
        self.assertEqual(process_observation._processes(json.dumps(root).encode(), "r", "pane:1"), [])
        self.assertEqual(self.recognize([{"pid": 1, "name": "djinn"}]), [])

    def test_malformed_processes_are_not_partial_positive_matches(self):
        for invalid in (None, {}, True, process(pid=True), process(pid=-1), process(pid=2**32),
                        process(argv="djinn"), process(argv=[1]), process(argv=["djinn\0"]),
                        process(argv=["djinn"], name=None), process(argv=["djinn"] * 1025)):
            with self.subTest(invalid=invalid), self.assertRaises(observation._Failure):
                self.recognize([process(argv=["djinn"]), invalid])
        with self.assertRaises(observation._Failure):
            self.recognize([process(argv=["djinn"]), process(argv=["djinn-ui"])])


class ProcessObservationTests(unittest.TestCase):
    def observe(self, handlers=None, **kwargs):
        with SequenceSocket(sequence() if handlers is None else handlers) as server:
            report = process_observation.inspect(server.path, "pane:1", **kwargs)
            requests = list(server.requests)
        return report, requests

    def test_independent_recognition_and_exact_read_only_sequence(self):
        report, requests = self.observe(sequence([
            process(argv=["/private/djinn", "ask", "SECRET PROMPT"]),
            process(pid=11, argv=["/private/djinn-ui", "--api-key", "SECRET KEY"])]))
        self.assertEqual(report["assessment"], "observed")
        self.assertEqual([(r["method"], r["params"]) for r in requests], [
            ("session.snapshot", {}), ("pane.process_info", {"pane_id": "pane:1"}),
            ("session.snapshot", {})])
        self.assertEqual(len({r["id"] for r in requests}), 3)
        item, = report["observations"]
        self.assertEqual(item["recognition"], "djinn_executable_family")
        self.assertEqual(len(item["matches"]), 2)  # One pane observation, not two agents.
        self.assertEqual(item["locators"]["terminal_id"], "term:1")
        self.assertEqual(item["activity"], "unknown")
        self.assertEqual(item["attention"], "unknown")
        self.assertEqual(item["native_lifecycle"], "unestablished")
        self.assertEqual(item["job_completion"], "unestablished")
        self.assertEqual(item["mapping_evidence"], "matching_bracketing_samples")
        self.assertEqual(report["authorization"], "none")
        self.assertFalse(report["dispatchable"])
        self.assertNotIn("agents", report)
        self.assertNotIn("native_session", item)
        self.assertNotIn("SECRET", json.dumps(report))
        self.assertNotIn("/private/", json.dumps(report))

    def test_valid_unknown_does_not_mean_agent_absent(self):
        report, _ = self.observe()
        self.assertEqual(report["assessment"], "observed")
        self.assertEqual(report["observations"][0]["recognition"], "unknown")

    def test_mapping_change_or_disappearance_discards_positive(self):
        for key in ("terminal_id", "workspace_id", "tab_id", "pane_id"):
            after = pane_snapshot()
            after["panes"][0][key] = "changed"
            report, _ = self.observe(sequence([process(argv=["djinn"])], after=after))
            self.assertEqual(report["assessment"], "unavailable")
            self.assertEqual(report["observations"], [])
            self.assertEqual(report["diagnostics"], ["pane_not_found" if key == "pane_id" else "pane_mapping_changed"])
        report, requests = self.observe(sequence(before=snapshot()))
        self.assertEqual(report["diagnostics"], ["pane_not_found"])
        self.assertEqual(len(requests), 1)

    def test_snapshot_agent_status_does_not_drive_recognition(self):
        before, after = snapshot(True), snapshot(True)
        after["panes"][0]["revision"] = after["agents"][0]["revision"] = 100
        report, _ = self.observe(sequence(before=before, after=after))
        self.assertEqual(report["assessment"], "observed")
        item, = report["observations"]
        self.assertEqual(item["recognition"], "unknown")
        self.assertNotIn("native_session", item)
        self.assertEqual(item["activity"], "unknown")

    def test_wrong_pane_and_malformed_process_response(self):
        cases = [lambda r: process_reply(r, pane_id="other"),
                 lambda r: process_reply(r, foreground_processes=None),
                 lambda r: process_reply(r, foreground_process_group_id=True),
                 lambda r: process_reply(r, shell_pid=-1),
                 lambda r: json.dumps({"id": r["id"], "result": {"type": "pong"}}).encode() + b"\n",
                 lambda r: process_reply({"id": "wrong"}),
                 lambda r: b'{"x":1,"x":2}\n']
        for case in cases:
            handlers = sequence()
            handlers[1] = sender(case)
            report, requests = self.observe(handlers)
            self.assertEqual(report["assessment"], "unavailable")
            self.assertEqual(report["observations"], [])
            self.assertEqual(len(requests), 2)

    def test_final_fence_failure_and_unsupported_protocol(self):
        for index in (0, 2):
            handlers = sequence([process(argv=["djinn"])])
            handlers[index] = sender(lambda r: reply(r, pane_snapshot() | {"protocol": 19}))
            report, requests = self.observe(handlers)
            self.assertEqual(report["assessment"], "incompatible")
            self.assertEqual(report["observations"], [])
            self.assertEqual(len(requests), index + 1)

    def test_server_errors_not_echoed_and_no_fallback_or_retry(self):
        for index in (0, 1, 2):
            handlers = sequence([process(argv=["djinn"])])
            handlers[index] = sender(lambda r: json.dumps({"id": r["id"], "error": {
                "code": "SECRET", "message": "SECRET"}}).encode() + b"\n")
            report, requests = self.observe(handlers)
            self.assertEqual(report["diagnostics"], ["server_error"])
            self.assertEqual(report["observations"], [])
            self.assertNotIn("SECRET", json.dumps(report))
            self.assertEqual(len(requests), index + 1)

    def test_aggregate_byte_budget_includes_all_frames(self):
        handlers = sequence()
        r = {"id": "x" * 32}
        total = len(reply(r, pane_snapshot())) * 2 + len(process_reply(r))
        self.assertGreaterEqual(total, 1024)
        report, _ = self.observe(handlers, max_response_bytes=total)
        self.assertEqual(report["assessment"], "observed")
        report, _ = self.observe(handlers, max_response_bytes=total - 1)
        self.assertEqual(report["diagnostics"], ["response_limit"])
        self.assertEqual(report["observations"], [])

    def test_one_deadline_and_final_frame_requires_eof(self):
        def delay(server, stream, request):
            if not server.stop.wait(0.045):
                sender(lambda r: process_reply(r, [process(argv=["djinn"])]))(server, stream, request)

        def open_frame(server, stream, request):
            stream.sendall(reply(request, pane_snapshot()))
            server.stop.wait(1)

        handlers = sequence()
        handlers[1], handlers[2] = delay, open_frame
        start = time.monotonic()
        report, _ = self.observe(handlers, timeout=0.08)
        self.assertLess(time.monotonic() - start, 0.5)
        self.assertEqual(report["diagnostics"], ["timeout"])
        self.assertEqual(report["observations"], [])

    def test_endpoint_frozen_between_reads(self):
        original = observation._endpoint_stat
        for replacement_call, request_count in ((4, 1), (6, 2), (7, 3)):
            calls = 0

            def changed(path):
                nonlocal calls
                calls += 1
                info = original(path)
                return mock.Mock(st_dev=info.st_dev, st_ino=info.st_ino + 1) if calls == replacement_call else info

            with mock.patch.object(observation, "_endpoint_stat", side_effect=changed):
                report, requests = self.observe(sequence([process(argv=["djinn"])]))
            self.assertEqual(report["diagnostics"], ["endpoint_replaced"])
            self.assertEqual(report["observations"], [])
            self.assertEqual(len(requests), request_count)

    def test_final_validation_exhausts_deadline(self):
        original = observation._snapshot
        clock, validations = [100.0], []

        def validate(*args):
            value = original(*args)
            validations.append(True)
            if len(validations) == 2:
                clock[0] += 1.0
            return value

        with mock.patch.object(observation.time, "monotonic", side_effect=lambda: clock[0]), \
                mock.patch.object(observation, "_snapshot", side_effect=validate):
            report, requests = self.observe(sequence([process(argv=["djinn"])]), timeout=0.5)
        self.assertEqual(len(requests), 3)
        self.assertEqual(report["diagnostics"], ["timeout"])
        self.assertEqual(report["observations"], [])

    def test_arguments_rejected_before_io(self):
        for pane_id in (None, "", "with space", "\x1b", "\ud800", "x" * 4097):
            with self.subTest(pane_id=pane_id), mock.patch.object(observation.socket, "socket", side_effect=AssertionError("socket")), \
                    self.assertRaises(ValueError):
                process_observation.inspect("/s", pane_id)
        with mock.patch.object(observation.socket, "socket", side_effect=AssertionError("socket")):
            with self.assertRaises(ValueError):
                process_observation.inspect("relative", "pane:1")
            with self.assertRaises(ValueError):
                process_observation.inspect("/s", "pane:1", timeout=float("nan"))

    def test_cli_and_wrapper_integration_without_workspace_or_runtime(self):
        for wrapper in (False, True):
            with SequenceSocket(sequence([process(argv=["djinn"])])) as server:
                args = ["observe", "kitsune", "--socket", server.path, "--process-pane", "pane:1", "--json"]
                if wrapper:
                    result = subprocess.run([str(ROOT / "bin/coven"), *args], capture_output=True,
                                            text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    output = result.stdout
                else:
                    stream = io.StringIO()
                    with contextlib.redirect_stdout(stream), \
                            mock.patch("subprocess.Popen", side_effect=AssertionError("process")), \
                            mock.patch("builtins.open", side_effect=AssertionError("file")), \
                            mock.patch("os.open", side_effect=AssertionError("file")):
                        self.assertEqual(coven.main(args), 0)
                    output = stream.getvalue()
            self.assertEqual(json.loads(output)["adapter_contract"], process_observation.ADAPTER_CONTRACT)
            self.assertTrue(output.isascii())

    def test_cli_error_and_incompatible_exit_codes(self):
        for handlers, code in ((sequence(before=snapshot()), 1),
                               (sequence(before=pane_snapshot() | {"protocol": 19}), 2)):
            with SequenceSocket(handlers) as server, contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(coven.main(["observe", "kitsune", "--socket", server.path,
                                             "--process-pane", "pane:1"]), code)


if __name__ == "__main__":
    unittest.main()
