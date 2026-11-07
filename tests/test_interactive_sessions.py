"""Private sockets and nonexecuted executable fixtures; no model/runtime calls."""

import contextlib
from copy import deepcopy
import io
import json
import os
import socket
import subprocess
from types import SimpleNamespace
from unittest import mock
import unittest

from runner import actions, coven, interactive_sessions as sessions, launch, storage, workers
from runner import kitsune_observation as wire
from test_coven import CovenTestCase, snapshot
from test_group_publisher import FakeServer, inventory


def capable():
    value = inventory()
    value["command_submission"] = {"version": 1}
    # Legacy agent interpretation must not decide whether a generic command is usable.
    value["agents"] = "not consumed"
    value["panes"][0]["agent_status"] = {"not": "consumed"}
    return value


def ack(server, stream, request):
    if request["method"] != "pane.submit_command":
        return False
    params = request["params"]
    result = {"type": "pane_command_submitted", "pane_id": params["pane_id"],
              "terminal_id": params["expected_terminal_id"], "argv": params["argv"]}
    stream.sendall(json.dumps({"id": request["id"], "result": result}).encode()+b"\n")
    return True


class InteractiveSessionTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace, "--blank", "--agents", "reviewer")
        self.executable = self.root / "trusted djinn"
        self.executable.write_text("#!/bin/sh\nexit 99\n")
        self.executable.chmod(0o700)
        for obj, name in ((subprocess, "run"), (subprocess, "Popen"), (launch, "execute"),
                          (workers, "inspect_readiness"), (coven, "send_to_agent_pane")):
            patcher = mock.patch.object(obj, name, side_effect=AssertionError("native execution forbidden"))
            guard = patcher.start()
            self.addCleanup(patcher.stop)
            self.addCleanup(guard.assert_not_called)

    def args(self, socket_path, *, operation="start", execute=True, selector="ses_example"):
        result = ["session", operation, str(self.workspace), "--runtime", "djinn",
                  "--executable", str(self.executable), "--socket", socket_path,
                  "--pane", "w1:p1", "--expected-terminal", "terminal-1", "--json"]
        if operation == "resume":
            result.extend(["--session", selector])
        if execute:
            result.append("--execute")
        return result

    def call(self, server, *, expected=0, **options):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(self.args(server.path, **options)), expected)
        return json.loads(output.getvalue())

    def writes(self, server):
        return [r for r in server.requests if r["method"] == "pane.submit_command"]

    def test_preview_has_no_filesystem_network_or_native_effects(self):
        before = snapshot(self.root)
        # Parse outside the guard: argparse may read standard-library locale files.
        args = coven.parse_args(self.args("/missing/socket", execute=False))
        output = io.StringIO()
        with mock.patch.object(sessions.socket, "socket", side_effect=AssertionError("socket during preview")), \
             mock.patch.object(sessions.os, "stat", side_effect=AssertionError("stat during preview")):
            with contextlib.redirect_stdout(output):
                self.assertEqual(coven.command_session(args), 0)
        report = json.loads(output.getvalue())
        self.assertEqual(report["outcome"], "preview")
        self.assertEqual(report["delivery"], "not_attempted")
        self.assertEqual(report["authorization"], "none")
        self.assertEqual(report["argv"], [str(self.executable), "--ui"])
        self.assertEqual(snapshot(self.root), before)

    def test_preview_does_not_require_existing_executable_or_workspace(self):
        self.executable.unlink()
        args = self.args("/missing/socket", execute=False)
        args[2] = str(self.root / "not initialized")
        report = json.loads(self.invoke(*args))
        self.assertEqual(report["outcome"], "preview")
        self.assertFalse((self.root / "not initialized").exists())

    def test_preview_resume_keeps_literal_native_selector(self):
        selector = "folder with spaces/'$(not a command); λ"
        report = json.loads(self.invoke(*self.args("/missing/socket", execute=False,
                                                 operation="resume", selector=selector)))
        self.assertEqual(report["argv"], [str(self.executable), "--session", selector])
        self.assertEqual(report["session_selector"], selector)
        self.assertEqual(report["native_lifecycle"], "unestablished")

    def test_invalid_arguments_and_timeout_fail_without_io(self):
        cases = [("--executable", "relative"), ("--pane", "w1:p1\nexit"),
                 ("--expected-terminal", " "), ("--socket", "/x/../y"),
                 ("--timeout", "nan"), ("--timeout", "0"), ("--timeout", "31")]
        for flag, value in cases:
            with self.subTest(flag=flag, value=value):
                args = self.args("/missing/socket", execute=False)
                if flag in args:
                    args[args.index(flag)+1] = value
                else:
                    args.extend([flag, value])
                with mock.patch.object(sessions.socket, "socket", side_effect=AssertionError("socket")):
                    with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as raised:
                        coven.main(args)
                    self.assertNotEqual(raised.exception.code, 0)
        for selector in ("", " ", "bad\nselector", "--help", "x"*4097):
            with self.subTest(selector=selector), self.assertRaises(ValueError):
                sessions.plan(str(self.workspace), "resume", "djinn", str(self.executable),
                              "/socket", "w1:p1", "terminal-1", selector)

    def test_canonical_plan_cannot_acquire_arbitrary_extra_native_arguments(self):
        plan = sessions.plan(str(self.workspace), "start", "djinn", str(self.executable),
                             "/socket", "w1:p1", "terminal-1")
        for mode in ("argv", "field", "version"):
            tampered = deepcopy(plan)
            if mode == "argv":
                tampered["argv"].append("--auto-approve")
            elif mode == "field":
                tampered["prompt"] = "not admitted"
            else:
                tampered["schema_version"] = True
            with self.subTest(mode=mode), self.assertRaises(ValueError):
                sessions.preview(tampered)

    def test_start_uses_only_generic_snapshots_and_one_pinned_submission(self):
        before = {path: (self.workspace/path).read_bytes() if (self.workspace/path).exists() else None
                  for path in ("state/agents.json", "state/tasks.json", "state/lead.json")}
        with FakeServer(capable(), ack) as server:
            report = self.call(server)
            self.assertEqual([r["method"] for r in server.requests],
                             ["session.snapshot", "session.snapshot", "pane.submit_command"])
            params = self.writes(server)[0]["params"]
            self.assertEqual(params, {"pane_id": "w1:p1", "expected_terminal_id": "terminal-1",
                                      "argv": [str(self.executable), "--ui"]})
        self.assertEqual(report["outcome"], "input_queued")
        self.assertEqual(report["delivery"], "queued")
        for field in ("native_lifecycle", "native_worker_acceptance", "job_completion"):
            self.assertEqual(report[field], "unestablished")
        self.assertNotIn("SECRET", json.dumps(report))
        for path, value in before.items():
            current = self.workspace/path
            self.assertEqual(current.read_bytes() if current.exists() else None, value)

    def test_resume_sends_exact_selector_without_native_probes_or_prompt(self):
        selector = "literal ' folder; $(never executed)"
        with FakeServer(capable(), ack) as server:
            report = self.call(server, operation="resume", selector=selector)
            self.assertEqual(report["outcome"], "input_queued")
            self.assertEqual(self.writes(server)[0]["params"]["argv"],
                             [str(self.executable), "--session", selector])

    def test_missing_false_malformed_or_future_capability_never_writes(self):
        for descriptor in (None, False, {}, {"version": True}, {"version": "1"},
                           {"version": 0}, {"version": 2}, {"version": 1.0}):
            value = capable()
            value["command_submission"] = descriptor
            with self.subTest(descriptor=descriptor), FakeServer(value, ack) as server:
                report = self.call(server, expected=1)
                self.assertEqual(report["diagnostics"], ["command_submission_unsupported"])
                self.assertEqual(self.writes(server), [])

    def test_capability_loss_on_second_snapshot_fails_before_submission(self):
        count = []
        def changed(server, stream, request):
            if request["method"] == "session.snapshot":
                count.append(True)
                if len(count) == 2:
                    server.value.pop("command_submission")
            return ack(server, stream, request)
        with FakeServer(capable(), changed) as server:
            report = self.call(server, expected=1)
            self.assertEqual(report["delivery"], "unsent")
            self.assertEqual(report["diagnostics"], ["command_submission_unsupported"])
            self.assertFalse(self.writes(server))

    def test_target_replacement_disappearance_and_tab_drift_never_write(self):
        for mode in ("terminal", "missing", "tab"):
            count = []
            def changed(server, stream, request):
                if request["method"] == "session.snapshot":
                    count.append(True)
                    if len(count) == 2:
                        if mode == "terminal":
                            server.value["panes"][0]["terminal_id"] = "replacement"
                        elif mode == "missing":
                            server.value["panes"] = []
                        else:
                            server.value["panes"][0]["tab_id"] = "new-tab"
                return ack(server, stream, request)
            with self.subTest(mode=mode), FakeServer(capable(), changed) as server:
                report = self.call(server, expected=1)
                self.assertEqual(report["delivery"], "unsent")
                self.assertEqual(self.writes(server), [])

    def test_missing_nonexecutable_and_changed_executable_never_write(self):
        for mode in ("missing", "permission", "changed"):
            self.executable.write_text("fixture")
            self.executable.chmod(0o700)
            if mode == "missing":
                self.executable.unlink()
            elif mode == "permission":
                self.executable.chmod(0o600)
            count = []
            def changed(server, stream, request):
                if request["method"] == "session.snapshot":
                    count.append(True)
                    if mode == "changed" and len(count) == 2:
                        self.executable.write_text("changed fixture")
                return ack(server, stream, request)
            with self.subTest(mode=mode), FakeServer(capable(), changed) as server:
                report = self.call(server, expected=1)
                self.assertEqual(report["delivery"], "unsent")
                self.assertFalse(self.writes(server))

    def test_unsafe_endpoint_never_receives_a_request(self):
        with FakeServer(capable(), ack) as server:
            os.chmod(server.path, 0o644)
            report = self.call(server, expected=1)
            self.assertEqual(report["diagnostics"], ["unsafe_endpoint"])
            self.assertFalse(server.requests)

    def test_server_rejection_is_not_retried(self):
        def reject(server, stream, request):
            if request["method"] != "pane.submit_command":
                return False
            stream.sendall(json.dumps({"id": request["id"], "error": {
                "code": "pane_command_busy", "message": "PRIVATE peer message"}}).encode()+b"\n")
            return True
        with FakeServer(capable(), reject) as server:
            report = self.call(server, expected=1)
            self.assertEqual(report["outcome"], "rejected")
            self.assertEqual(report["diagnostics"], ["server_error:pane_command_busy"])
            self.assertEqual(len(self.writes(server)), 1)
            self.assertNotIn("PRIVATE", json.dumps(report))

    def test_lost_malformed_and_wrong_ack_are_uncertain_without_retry(self):
        for mode in ("lost", "id", "terminal", "argv", "type", "both", "trailing"):
            def malformed(server, stream, request):
                if request["method"] != "pane.submit_command":
                    return False
                if mode == "lost":
                    return True
                params = request["params"]
                value = {"id": request["id"], "result": {"type": "pane_command_submitted",
                         "pane_id": params["pane_id"], "terminal_id": params["expected_terminal_id"],
                         "argv": params["argv"]}}
                if mode == "id":
                    value["id"] = "wrong"
                elif mode in ("terminal", "argv", "type"):
                    field = "terminal_id" if mode == "terminal" else mode
                    value["result"][field] = "wrong"
                elif mode == "both":
                    value["error"] = {"code": "busy", "message": "not a unique outcome"}
                data = json.dumps(value).encode()+b"\n"
                stream.sendall(data + (b"extra\n" if mode == "trailing" else b""))
                return True
            with self.subTest(mode=mode), FakeServer(capable(), malformed) as server:
                report = self.call(server, expected=2)
                self.assertEqual(report["outcome"], "uncertain")
                self.assertEqual(report["delivery"], "uncertain")
                self.assertEqual(len(self.writes(server)), 1)

    def test_direct_execution_requires_valid_workspace_gate(self):
        value = sessions.plan(str(self.workspace), "start", "djinn", str(self.executable),
                              "/socket", "w1:p1", "terminal-1")
        with self.assertRaises(storage.StorageError):
            sessions.execute(value, gate=None)
        other = self.root / "other"
        other.mkdir()
        with storage.transport_gate(other, other/"state/.runtime-transport.lock") as gate:
            with self.assertRaises(storage.StorageError):
                sessions.execute(value, gate=gate)

    def test_approved_launch_mode_blocks_cli_and_direct_helper_before_io(self):
        self.invoke("action", "enable-runtime-launch", self.workspace, "--json")
        before = snapshot(self.workspace)
        with mock.patch.object(sessions.socket, "socket", side_effect=AssertionError("socket")):
            self.invoke(*self.args("/socket"), error="Approved runtime-launch mode")
            value = sessions.plan(str(self.workspace), "start", "djinn", str(self.executable),
                                  "/socket", "w1:p1", "terminal-1")
            with storage.transport_gate(self.workspace, self.workspace/"state/.runtime-transport.lock") as gate:
                with self.assertRaises(actions.ActionError):
                    sessions.execute(value, gate=gate)
        self.assertEqual(snapshot(self.workspace), before)

    def test_submission_holds_activation_gate_but_not_workspace_lock_during_io(self):
        checked = []
        def inspect_locks(server, stream, request):
            if request["method"] == "pane.submit_command":
                with self.assertRaises(storage.LockBusy):
                    with storage.transport_gate(self.workspace, self.workspace/"state/.runtime-transport.lock",
                                                exclusive=True, timeout=0):
                        pass
                with storage.workspace_lock(self.workspace, self.workspace/"state/.workspace.lock", timeout=0):
                    checked.append(True)
            return ack(server, stream, request)
        with FakeServer(capable(), inspect_locks) as server:
            self.call(server)
        self.assertEqual(checked, [True])

    def test_interrupt_classification_before_and_during_send(self):
        for mode, expected, delivery in (("before", 130, "unsent"), ("inflight", 130, "uncertain")):
            with self.subTest(mode=mode), FakeServer(capable(), ack) as server:
                failure = sessions.SubmissionError("interrupted", delivery, interrupted=True)
                with mock.patch.object(sessions, "_submit", side_effect=failure):
                    report = self.call(server, expected=expected)
                self.assertEqual(report["delivery"], delivery)
                self.assertTrue(report["interrupted"])
                self.assertFalse(self.writes(server))

    def test_interruption_after_valid_ack_before_accounting_stays_uncertain(self):
        original = sessions._submit
        def interrupted(*args, **kwargs):
            original(*args, **kwargs)
            raise KeyboardInterrupt
        with FakeServer(capable(), ack) as server, mock.patch.object(sessions, "_submit", side_effect=interrupted):
            report = self.call(server, expected=130)
            self.assertEqual(report["delivery"], "uncertain")
            self.assertEqual(len(self.writes(server)), 1)

    def test_interruption_after_recorded_ack_preserves_queued_fact(self):
        original_preview = sessions.preview
        class InterruptAfterAck(dict):
            fired = False
            def update(self, *args, **kwargs):
                super().update(*args, **kwargs)
                if kwargs.get("outcome") == "input_queued" and not self.fired:
                    self.fired = True
                    raise KeyboardInterrupt
        def interrupted_preview(plan):
            return InterruptAfterAck(original_preview(plan))
        with FakeServer(capable(), ack) as server:
            with mock.patch.object(sessions, "preview", side_effect=interrupted_preview):
                report = self.call(server, expected=130)
            self.assertEqual(report["outcome"], "input_queued")
            self.assertEqual(report["delivery"], "queued")
            self.assertEqual(len(self.writes(server)), 1)

    def test_orphaned_action_artifacts_block_execute_without_network(self):
        directory = self.workspace / "state/actions"
        directory.mkdir(exist_ok=True)
        (directory / "orphan.json").write_text("{}\n")
        before = snapshot(self.workspace)
        with mock.patch.object(sessions.socket, "socket", side_effect=AssertionError("socket")):
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as raised:
                coven.main(self.args("/socket"))
            self.assertNotEqual(raised.exception.code, 0)
        self.assertEqual(snapshot(self.workspace), before)

    def test_connect_failure_is_unsent_but_send_failure_is_uncertain(self):
        real_socket, original = socket.socket, sessions._submit
        for mode, code, delivery in (("connect", 1, "unsent"), ("send", 2, "uncertain")):
            class FailingSocket:
                def __init__(self, *args, **kwargs):
                    self.stream = real_socket(*args, **kwargs)
                def __enter__(self):
                    return self
                def __exit__(self, *_args):
                    self.stream.close()
                def settimeout(self, timeout):
                    self.stream.settimeout(timeout)
                def connect(self, address):
                    if mode == "connect":
                        raise OSError("private connection error")
                    self.stream.connect(address)
                def sendall(self, _payload):
                    # A real sendall failure cannot establish how many bytes got through.
                    raise OSError("private send error")
            def failed(*args, **kwargs):
                replacement = SimpleNamespace(socket=FailingSocket, AF_UNIX=socket.AF_UNIX,
                                              SOCK_STREAM=socket.SOCK_STREAM)
                with mock.patch.object(sessions, "socket", replacement):
                    return original(*args, **kwargs)
            with self.subTest(mode=mode), FakeServer(capable(), ack) as server:
                with mock.patch.object(sessions, "_submit", side_effect=failed):
                    report = self.call(server, expected=code)
                self.assertEqual(report["delivery"], delivery)
                self.assertNotIn("private", json.dumps(report))
                self.assertEqual(len(server.requests), 2)


if __name__ == "__main__":
    unittest.main()
