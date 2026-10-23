"""Only private fake endpoints: never invoke or discover a Kitsune runtime."""

import copy
import json
import os
from pathlib import Path
import socket
import tempfile
import threading
import time
import unittest
from unittest import mock

from runner import kitsune_observation as observation

ROOT = Path(__file__).resolve().parents[1]


def snapshot(populated=False):
    # schema/session.rs and schema/tests.rs session_snapshot_request_and_response_round_trip;
    # the old Rust test uses 16, but protocol/wire.rs currently specifies 18.
    result = dict(version="0.1.2", protocol=18, workspaces=[], tabs=[], panes=[], layouts=[], agents=[])
    if populated:
        native = dict(source="persisted", agent="djinn", kind="path", value="/old/session.json")
        pane = dict(pane_id="pane:1", terminal_id="term:1", workspace_id="ws:1",
                    tab_id="tab:1", focused=True, agent_status="done", revision=7,
                    agent="djinn", agent_session=native, title="SECRET TITLE",
                    cwd="/SECRET", tokens={"secret": "SECRET TOKEN"})
        result["panes"] = [pane]
        result["agents"] = [{**copy.deepcopy(pane), "name": "reviewer"}]
        result["workspaces"] = [dict(workspace_id="ws:1", number=1, label="private",
                                     focused=True, pane_count=1, tab_count=1,
                                     active_tab_id="tab:1", agent_status="done")]
        result["tabs"] = [dict(tab_id="tab:1", workspace_id="ws:1", number=1,
                               label="private", focused=True, pane_count=1, agent_status="done")]
        result["layouts"] = [dict(workspace_id="ws:1", tab_id="tab:1", zoomed=False,
                                  area=dict(x=0, y=0, width=80, height=24),
                                  focused_pane_id="pane:1", panes=[], splits=[])]
    return result


def reply(request, value=None):
    return json.dumps(dict(id=request["id"], result=dict(type="session_snapshot",
                       snapshot=snapshot() if value is None else value))).encode() + b"\n"


class FakeSocket:
    def __init__(self, handler):
        self.handler = handler
        self.stop = threading.Event()
        self.errors = []
        self.requests = []

    def __enter__(self):
        self.directory = tempfile.TemporaryDirectory(dir=ROOT, prefix=".coven-sock-")
        self.path = str(Path(self.directory.name) / "s")
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.listener.bind(self.path)
        os.chmod(self.path, 0o600)
        self.listener.listen(1)
        self.listener.settimeout(0.05)
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()
        return self

    def serve(self):
        try:
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
                while b"\n" not in data and len(data) < 4096:
                    part = stream.recv(4096)
                    if not part:
                        return
                    data.extend(part)
                self.requests.append(bytes(data))
                request = json.loads(data)
                self.handler(self, stream, request)
                # A one-request server must receive no additional commands.
                while not self.stop.is_set():
                    part = stream.recv(4096)
                    if not part:
                        break
                    self.requests.append(part)
        except (BrokenPipeError, ConnectionResetError):
            pass
        except Exception as exc:
            if not self.stop.is_set():
                self.errors.append(exc)

    def __exit__(self, *_args):
        self.stop.set()
        self.thread.join(1.5)
        self.listener.close()
        self.directory.cleanup()
        if self.thread.is_alive():
            raise AssertionError("fake socket thread leaked")
        if self.errors:
            raise AssertionError(self.errors)


def sender(payload):
    def send(_server, stream, request):
        stream.sendall(payload(request) if callable(payload) else payload)
        stream.shutdown(socket.SHUT_WR)
    return send


class ObservationTests(unittest.TestCase):
    def observe(self, payload, **kwargs):
        with FakeSocket(sender(payload)) as server:
            report = observation.inspect(server.path, **kwargs)
        return report

    def test_empty_snapshot_and_exact_single_request(self):
        with FakeSocket(sender(reply)) as server:
            report = observation.inspect(server.path)
        self.assertEqual(report["assessment"], "observed")
        self.assertEqual(report["agents"], [])
        self.assertEqual(report["pane_count"], 0)
        self.assertEqual(len(server.requests), 1)
        wire = server.requests[0]
        request = json.loads(wire)
        self.assertEqual(wire.count(b"\n"), 1)
        self.assertEqual(set(request), {"id", "method", "params"})
        self.assertIsInstance(request["id"], str)
        self.assertEqual(request["method"], "session.snapshot")
        self.assertEqual(request["params"], {})
        self.assertEqual(report["adapter_contract"], observation.ADAPTER_CONTRACT)
        self.assertEqual(report["authorization"], "none")
        self.assertFalse(report["dispatchable"])
        self.assertEqual(report["runtime_fact_freshness"], "unknown")
        self.assertIsNone(report["server_snapshot_timestamp"])
        self.assertIsInstance(report["endpoint"]["inode"], int)
        self.assertIn("+00:00", report["observed_at"])
        self.assertGreaterEqual(report["duration_ms"], 0)
        self.assertIn("potentially spawning", " ".join(report["limitations"]))

    def test_done_persisted_references_and_privacy(self):
        value = snapshot(True)
        report = self.observe(lambda r: reply(r, value))
        self.assertEqual(report["assessment"], "observed")
        agent, = report["agents"]
        self.assertEqual(agent["raw_status"], "done")
        self.assertEqual(agent["activity"], "idle")
        self.assertEqual(agent["attention"], "unacknowledged")
        self.assertEqual(agent["native_lifecycle"], "unestablished")
        self.assertEqual(agent["job_completion"], "unestablished")
        self.assertEqual(agent["provenance"], "kitsune.session.snapshot")
        self.assertEqual(agent["native_session"]["source"], "persisted")
        self.assertEqual(agent["native_session"]["value"], "/old/session.json")
        self.assertFalse(agent["launch_pending"])
        self.assertEqual(agent["state_change_seq"], 0)
        self.assertNotIn("SECRET", json.dumps(report))
        self.assertNotIn("title", agent)

    def test_only_agents_inventory_no_native_deduplication(self):
        value = snapshot(True)
        for collection in ("panes", "agents"):
            second = {**copy.deepcopy(value[collection][0]), "pane_id": "pane:2", "terminal_id": "term:2"}
            value[collection].append(second)
        report = self.observe(lambda r: reply(r, value))
        self.assertEqual(len(report["agents"]), 2)
        self.assertEqual(report["agents"][0]["native_session"], report["agents"][1]["native_session"])
        value["agents"] = []
        report = self.observe(lambda r: reply(r, value))
        self.assertEqual(report["agents"], [])
        self.assertEqual(report["pane_count"], 2)

    def test_status_normalization(self):
        for status in ("idle", "working", "blocked", "unknown"):
            with self.subTest(status=status):
                value = snapshot(True)
                for field in ("panes", "agents"):
                    value[field][0]["agent_status"] = status
                agent, = self.observe(lambda r: reply(r, value))["agents"]
                self.assertEqual(agent["activity"], status)
                self.assertEqual(agent["attention"], "needs_attention" if status == "blocked" else "unknown")

    def test_caller_errors_precede_any_socket_activity(self):
        invalid = [dict(socket_path=p) for p in (None, "", "relative", "/a/../b", "/a\0b", "/a/", "/" + "a" * 4096, "/\ud800")]
        invalid += [dict(socket_path="/s", timeout=t) for t in (0, -1, True, "3", float("nan"), float("inf"), 31, 10**1000)]
        invalid += [dict(socket_path="/s", max_response_bytes=b) for b in (True, 1, 1023, 8388609, 1024.0)]
        with mock.patch.object(observation.socket, "socket", side_effect=AssertionError("socket activity")), mock.patch.object(observation.os, "lstat", side_effect=AssertionError("stat activity")):
            for args in invalid:
                with self.subTest(args=args), self.assertRaises(ValueError):
                    observation.inspect(**args)

    def test_missing_regular_symlink_permissions_and_owner(self):
        with tempfile.TemporaryDirectory(dir=ROOT, prefix=".coven-sock-") as directory:
            path = str(Path(directory) / "s")
            self.assertEqual(observation.inspect(path)["diagnostics"], ["io_error"])
            Path(path).touch()
            self.assertEqual(observation.inspect(path)["diagnostics"], ["unsafe_endpoint"])
            os.unlink(path)
            os.symlink("missing", path)
            self.assertEqual(observation.inspect(path)["diagnostics"], ["unsafe_endpoint"])
        with FakeSocket(sender(reply)) as server:
            os.chmod(server.path, 0o660)
            self.assertEqual(observation.inspect(server.path)["diagnostics"], ["unsafe_endpoint"])
            os.chmod(server.path, 0o600)
            with mock.patch.object(observation.os, "getuid", return_value=os.getuid() + 1):
                self.assertEqual(observation.inspect(server.path)["diagnostics"], ["unsafe_endpoint"])

    def test_parent_alias_is_allowed_but_leaf_alias_is_not(self):
        with FakeSocket(sender(reply)) as server:
            alias = Path(server.directory.name) / "parent"
            alias.symlink_to(server.directory.name, target_is_directory=True)
            report = observation.inspect(str(alias / "s"))
            self.assertEqual(report["assessment"], "observed")
            self.assertEqual(report["endpoint"]["resolved"], os.path.realpath(server.path))

    def test_schema_corruption(self):
        changes = [("protocol", True), ("protocol", "18"), ("version", None),
                   ("panes", {}), ("agents", [None]), ("tabs", [False]),
                   ("workspaces", [{}]), ("layouts", [{}]), ("focused_pane_id", 2)]
        values = [{**snapshot(True), key: val} for key, val in changes]
        for field in ("pane_id", "terminal_id", "workspace_id", "tab_id", "focused", "agent_status", "revision"):
            value = snapshot(True)
            del value["agents"][0][field]
            values.append(value)
        for field, val in (("focused", 1), ("agent_status", []), ("revision", True),
                           ("revision", -1), ("revision", 2**64), ("state_change_seq", True),
                           ("launch_pending", "false"), ("interactive_ready", None),
                           ("screen_detection_skipped", 0), ("name", []),
                           ("agent_session", {"source": "x", "agent": "x", "kind": "url", "value": "x"})):
            value = snapshot(True)
            value["agents"][0][field] = val
            values.append(value)
        for field in ("panes", "agents"):
            value = snapshot(True)
            value[field] *= 2
            values.append(value)
        for field in ("terminal_id", "workspace_id", "agent_status", "revision", "agent"):
            value = snapshot(True)
            value["agents"][0][field] = "different" if field != "revision" else 9
            values.append(value)
        value = snapshot(True)
        value["agents"][0]["agent_session"]["value"] = "different"
        values.append(value)
        for value in values:
            with self.subTest(value=value):
                report = self.observe(lambda r: reply(r, value))
                self.assertEqual(report["assessment"], "unavailable")
                self.assertEqual(report["agents"], [])
                self.assertEqual(report["diagnostics"], ["schema_error"])

    def test_protocol_incompatible_not_stopped(self):
        for protocol in (16, 17, 19):
            report = self.observe(lambda r: reply(r, {**snapshot(), "protocol": protocol}))
            self.assertEqual(report["assessment"], "incompatible")
            self.assertEqual(report["diagnostics"], ["unsupported_protocol"])

    def test_root_errors_and_no_raw_error_echo(self):
        cases = [lambda r: dict(id="wrong", result={}),
                 lambda r: dict(id=r["id"], result=dict(type="pong")),
                 lambda r: dict(id=r["id"], result={}, error={}),
                 lambda r: dict(id=r["id"], error=dict(code="SECRET CODE", message="SECRET ERROR")),
                 lambda r: dict(id=r["id"], error=dict(code=1, message=[])),
                 lambda r: []]
        for case in cases:
            report = self.observe(lambda r: json.dumps(case(r)).encode() + b"\n")
            self.assertEqual(report["assessment"], "unavailable")
            self.assertNotIn("SECRET", json.dumps(report))

    def test_wire_bounds_and_malformed_json(self):
        cases = [(b"x" * 1025, "response_limit"), (b"{}", "incomplete_response"),
                 (b"\xff\n", "invalid_utf8"), (b"{}\n{}\n", "trailing_data"),
                 (b"{}\n ", "trailing_data"), (b"[" * 33 + b"]" * 33 + b"\n", "nesting_limit"),
                 (b'{"x":1,"x":2}\n', "duplicate_key"),
                 (b'{"x":NaN}\n', "nonfinite_json"), (b'{"x":Infinity}\n', "nonfinite_json"),
                 (b'{"x":1e999}\n', "schema_error"), (b"no json\n", "invalid_json")]
        for payload, code in cases:
            with self.subTest(code=code):
                self.assertEqual(self.observe(payload, max_response_bytes=1024)["diagnostics"], [code])
        # Exactly the byte budget, including LF, is accepted.
        def exact(r):
            data = reply(r)
            return data[:-1] + b" " * (1024 - len(data)) + b"\n"
        self.assertEqual(self.observe(exact, max_response_bytes=1024)["assessment"], "observed")

    def test_additive_fields_still_bounded_and_never_echoed(self):
        for extra in ("x" * 4097, [0] * 4097, {str(i): 0 for i in range(4097)}):
            value = {**snapshot(), "private_extension": extra}
            self.assertEqual(self.observe(lambda r: reply(r, value))["assessment"], "unavailable")
        value = {**snapshot(), "private_extension": {"safe": "SECRET"}}
        report = self.observe(lambda r: reply(r, value))
        self.assertEqual(report["assessment"], "observed")
        self.assertNotIn("SECRET", json.dumps(report))

    def test_timeout_and_slowdrip_use_one_deadline(self):
        def stall(server, _stream, _request):
            server.stop.wait(1)

        def drip(server, stream, _request):
            for _ in range(100):
                if server.stop.wait(0.015):
                    break
                stream.sendall(b" ")

        for handler in (stall, drip):
            with FakeSocket(handler) as server:
                start = time.monotonic()
                report = observation.inspect(server.path, timeout=0.08)
                self.assertLess(time.monotonic() - start, 0.5)
                self.assertEqual(report["diagnostics"], ["timeout"])

    def test_delayed_extra_frame_rejected(self):
        def extra(server, stream, request):
            stream.sendall(reply(request))
            server.stop.wait(0.02)
            stream.sendall(b"{}\n")
            stream.shutdown(socket.SHUT_WR)
        with FakeSocket(extra) as server:
            self.assertEqual(observation.inspect(server.path)["diagnostics"], ["trailing_data"])

    def test_endpoint_replacement_before_send_and_after_receipt(self):
        original = observation._endpoint_stat
        for replacement_call in (2, 3):
            calls = 0

            def changed(path):
                nonlocal calls
                calls += 1
                info = original(path)
                if calls == replacement_call:
                    return mock.Mock(st_dev=info.st_dev, st_ino=info.st_ino + 1)
                return info

            with FakeSocket(sender(reply)) as server, mock.patch.object(observation, "_endpoint_stat", side_effect=changed):
                report = observation.inspect(server.path)
                self.assertEqual(report["diagnostics"], ["endpoint_replaced"])
                if replacement_call == 2:
                    self.assertEqual(server.requests, [])

    def test_no_process_invocation_or_filesystem_state_writes(self):
        with FakeSocket(sender(reply)) as server:
            with mock.patch("subprocess.Popen", side_effect=AssertionError("process")), mock.patch("os.system", side_effect=AssertionError("process")), mock.patch("builtins.open", side_effect=AssertionError("file")), mock.patch("os.open", side_effect=AssertionError("file")), mock.patch("os.mkdir", side_effect=AssertionError("mkdir")), mock.patch("os.unlink", side_effect=AssertionError("unlink")):
                report = observation.inspect(server.path)
            self.assertEqual(report["assessment"], "observed")

    def test_actual_socket_replacement(self):
        def replace(server, stream, request):
            # Keep the original inode linked so it cannot be immediately reused.
            os.rename(server.path, server.path + "-old")
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as replacement:
                replacement.bind(server.path)
                os.chmod(server.path, 0o600)
                stream.sendall(reply(request))
                stream.shutdown(socket.SHUT_WR)
        with FakeSocket(replace) as server:
            report = observation.inspect(server.path)
            self.assertEqual(report["diagnostics"], ["endpoint_replaced"])

    def test_native_reference_and_string_limits(self):
        for key, val in (("source", None), ("agent", False), ("kind", []),
                         ("value", 1), ("value", "x" * 8193)):
            value = snapshot(True)
            value["agents"][0]["agent_session"][key] = val
            self.assertEqual(self.observe(lambda r: reply(r, value))["diagnostics"], ["schema_error"])
        value = snapshot(True)
        for field in ("panes", "agents"):
            value[field][0]["agent_session"]["value"] = "x" * 8192
        self.assertEqual(self.observe(lambda r: reply(r, value))["assessment"], "observed")
        for field in ("panes", "agents"):
            value[field][0]["agent_session"]["kind"] = "id"
        self.assertEqual(self.observe(lambda r: reply(r, value))["diagnostics"], ["schema_error"])
        value = snapshot(True)
        for field in ("panes", "agents"):
            del value[field][0]["agent_session"]
        agent, = self.observe(lambda r: reply(r, value))["agents"]
        self.assertNotIn("native_session", agent)
        value = {**snapshot(), "extra": "\ud800"}
        self.assertEqual(self.observe(lambda r: reply(r, value))["diagnostics"], ["schema_error"])

    def test_collection_caps_and_duplicate_terminal_ids(self):
        value = {**snapshot(), "agents": [{}] * 1025}
        self.assertEqual(self.observe(lambda r: reply(r, value))["diagnostics"], ["schema_error"])
        for field in ("panes", "agents"):
            value = snapshot(True)
            value[field].append({**value[field][0], "pane_id": "different"})
            self.assertEqual(self.observe(lambda r: reply(r, value))["diagnostics"], ["schema_error"])

    def test_fragmented_reply_native_id_and_reported_flags(self):
        value = snapshot(True)
        for field in ("panes", "agents"):
            value[field][0]["agent_session"]["kind"] = "id"
            value[field][0]["agent_session"]["value"] = "native-session-1"
        value["agents"][0].update(launch_pending=True, interactive_ready=True,
                                 screen_detection_skipped=True, state_change_seq=13)
        def fragmented(_server, stream, request):
            data = reply(request, value)
            for start in range(0, len(data), 37):
                stream.sendall(data[start:start + 37])
            stream.shutdown(socket.SHUT_WR)
        with FakeSocket(fragmented) as server:
            report = observation.inspect(server.path)
        self.assertEqual(report["assessment"], "observed")
        agent, = report["agents"]
        self.assertEqual(agent["native_session"]["kind"], "id")
        self.assertEqual(agent["state_change_seq"], 13)
        for flag in ("launch_pending", "interactive_ready", "screen_detection_skipped"):
            self.assertTrue(agent[flag])

    def test_complete_frame_without_eof_is_not_published(self):
        def keep_open(server, stream, request):
            stream.sendall(reply(request, snapshot(True)))
            server.stop.wait(1)
        with FakeSocket(keep_open) as server:
            report = observation.inspect(server.path, timeout=0.08)
        self.assertEqual(report["assessment"], "unavailable")
        self.assertEqual(report["diagnostics"], ["timeout"])
        self.assertEqual(report["agents"], [])
        self.assertIsNone(report["pane_count"])

    def test_workspace_and_tab_fields_are_characterized(self):
        fields = {"workspace_id": None, "number": True, "label": [], "focused": 1,
                  "pane_count": -1, "agent_status": "success"}
        for field, changes in (("workspaces", fields | {"tab_count": "1", "active_tab_id": False}),
                               ("tabs", fields | {"tab_id": 1})):
            for key, invalid in changes.items():
                with self.subTest(collection=field, key=key):
                    value = snapshot(True)
                    value[field][0][key] = invalid
                    report = self.observe(lambda r: reply(r, value))
                    self.assertEqual(report["diagnostics"], ["schema_error"])
                    self.assertEqual(report["agents"], [])


if __name__ == "__main__":
    unittest.main()
