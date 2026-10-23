"""Observation CLI is independent of workspace authority and executable runtimes."""

import contextlib
import io
import json
import os
import subprocess
from unittest import mock

from runner import coven, kitsune_observation, launch, storage, task_attempts, workers
from test_coven import CovenTestCase, ROOT, snapshot as workspace_snapshot
from test_kitsune_observation import FakeSocket, reply, sender, snapshot


class ObserveCommandTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace)
        for target, name in ((launch, "execute"), (workers, "inspect_readiness"), (coven, "send_to_agent_pane")):
            guard = mock.patch.object(target, name, side_effect=AssertionError("Observation never executes/probes native workers or sends prompts"))
            observed = guard.start()
            self.addCleanup(guard.stop)
            self.addCleanup(observed.assert_not_called)

    def report(self, *options, expected=0, compact=True):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(["observe", "kitsune", *map(str, options), *(["--json"] if compact else [])]), expected)
        return json.loads(output.getvalue()), output.getvalue()

    def test_json_and_human_output_escape_untrusted_labels(self):
        value = snapshot(True)
        value["agents"][0]["name"] = "reviewer\x1b[31m🦊"
        for compact in (True, False):
            with FakeSocket(sender(lambda r: reply(r, value))) as server:
                report, output = self.report("--socket", server.path, compact=compact)
            self.assertEqual(report["assessment"], "observed")
            self.assertEqual(report["agents"][0]["activity"], "idle")
            self.assertEqual(report["agents"][0]["attention"], "unacknowledged")
            self.assertNotIn("\x1b", output)
            self.assertTrue(output.isascii())
            self.assertEqual(report["authorization"], "none")
            self.assertFalse(report["dispatchable"])

    def test_missing_socket_and_incompatible_protocol_exit_codes(self):
        missing, _ = self.report("--socket", self.root / "no-such-socket", expected=1)
        self.assertEqual(missing["assessment"], "unavailable")
        self.assertEqual(missing["agents"], [])
        with FakeSocket(sender(lambda r: reply(r, snapshot() | {"protocol": 19}))) as server:
            incompatible, _ = self.report("--socket", server.path, expected=2)
        self.assertEqual(incompatible["assessment"], "incompatible")

    def test_explicit_socket_required_even_with_inherited_runtime_environment(self):
        before = workspace_snapshot(self.workspace)
        with mock.patch.dict(os.environ, {"KITSUNE_SOCKET_PATH": "/never-use", "KITSUNE_CLIENT_SOCKET_PATH": "/never-use-either"}), \
                mock.patch.object(kitsune_observation, "inspect", side_effect=AssertionError("No implicit endpoint")):
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as raised:
                coven.main(["observe", "kitsune", "--json"])
            self.assertEqual(raised.exception.code, 2)
        self.assertEqual(workspace_snapshot(self.workspace), before)

    def test_bad_arguments_fail_before_any_connection_or_workspace_effect(self):
        before = workspace_snapshot(self.workspace)
        for options in (("--socket", "relative"), ("--socket", "/s", "--timeout", "nan"),
                        ("--socket", "/s", "--timeout", "0"), ("--socket", "/s", "--timeout", "31"),
                        ("--socket", "/s", "--max-response-bytes", "1"),
                        ("--socket", "/s", "--max-response-bytes", "8388609")):
            with self.subTest(options=options), mock.patch.object(kitsune_observation.socket, "socket", side_effect=AssertionError("Connection")):
                self.invoke("observe", "kitsune", *options, error="Invalid Kitsune observation request")
                self.assertEqual(workspace_snapshot(self.workspace), before)

    def test_corrupt_workspace_cannot_block_or_be_repaired_by_observation(self):
        for relative in ("coven.json", "state/tasks.json", "state/agents.json", "state/runtime-launch-control.json", "state/repository.json"):
            (self.workspace / relative).write_bytes(b"corrupt\xff")
        before = workspace_snapshot(self.workspace)
        with FakeSocket(sender(reply)) as server, contextlib.chdir(self.workspace), \
                mock.patch.object(coven, "load_team", side_effect=AssertionError("Workspace load")), \
                mock.patch.object(storage, "workspace_lock", side_effect=AssertionError("Workspace lock")), \
                mock.patch.object(storage, "transport_gate", side_effect=AssertionError("Transport gate")), \
                mock.patch.object(coven.subprocess, "run", side_effect=AssertionError("CLI invocation")):
            report, _ = self.report("--socket", server.path)
        self.assertEqual(report["assessment"], "observed")
        self.assertEqual(workspace_snapshot(self.workspace), before)

    def test_existing_authorities_remain_unchanged(self):
        for stage in ("snapshot", "ledger", "runtime", "execution-v1", "execution-v2"):
            with self.subTest(stage=stage):
                if stage == "ledger":
                    revision = json.loads(self.invoke("task", "list", self.workspace, "--json"))["revision"]
                    self.invoke("state", "import-tasks", self.workspace, "--expect-revision", revision)
                if stage == "runtime":
                    self.invoke("action", "enable-runtime-launch", self.workspace)
                    self.invoke("action", "prepare-runtime-launch", self.workspace)
                if stage == "execution-v1":
                    self.invoke("execution", "enable", self.workspace, "--expect-execution-revision", task_attempts.DISABLED, "--request-id", "enable")
                if stage == "execution-v2":
                    self.invoke("execution", "upgrade", self.workspace, "--expect-execution-revision",
                        task_attempts.Store(self.workspace).read().revision, "--request-id", "upgrade")
                before = workspace_snapshot(self.workspace)
                with FakeSocket(sender(reply)) as server, \
                        mock.patch.object(storage, "workspace_lock", side_effect=AssertionError("Workspace lock")), \
                        mock.patch.object(storage, "transport_gate", side_effect=AssertionError("Transport gate")):
                    self.report("--socket", server.path)
                self.assertEqual(workspace_snapshot(self.workspace), before)

    def test_one_shot_does_not_cache_previous_active_observation(self):
        value = snapshot(True)
        for field in ("panes", "agents"):
            value[field][0]["agent_status"] = "working"
        with FakeSocket(sender(lambda r: reply(r, value))) as server:
            active, _ = self.report("--socket", server.path)
            endpoint = server.path
        unavailable, _ = self.report("--socket", endpoint, expected=1)
        self.assertEqual(active["agents"][0]["activity"], "working")
        self.assertEqual(unavailable["agents"], [])
        self.assertEqual(unavailable["runtime_fact_freshness"], "unknown")

    def test_no_annotation_or_arbitrary_api_control_command_is_exposed(self):
        before = workspace_snapshot(self.workspace)
        for options in (("observe", "kitsune", "--socket", "/s", "--method", "pane.send_keys"),
                        ("observe", "kitsune", "--socket", "/s", "--annotate", "working"),
                        ("observe", "kitsune", str(self.workspace), "--socket", "/s")):
            with self.subTest(options=options), contextlib.redirect_stderr(io.StringIO()), \
                    mock.patch.object(kitsune_observation, "inspect", side_effect=AssertionError("Unexpected dispatch")), \
                    self.assertRaises(SystemExit) as raised:
                coven.main(list(options))
            self.assertEqual(raised.exception.code, 2)
        self.assertEqual(workspace_snapshot(self.workspace), before)

    def test_real_wrapper_uses_only_fake_selected_socket(self):
        before = workspace_snapshot(self.workspace)
        env = {key: value for key, value in os.environ.items() if key not in {"COVEN_HOME", "COVEN_COMMAND"}}
        env.update(KITSUNE_SOCKET_PATH="/not-selected", KITSUNE_CLIENT_SOCKET_PATH="/not-selected")
        with FakeSocket(sender(lambda r: reply(r, snapshot(True)))) as server:
            response = subprocess.run([str(ROOT / "bin/coven"), "observe", "kitsune", "--socket", server.path, "--json"],
                cwd=self.workspace, env=env, text=True, capture_output=True, timeout=15)
        self.assertEqual(response.returncode, 0, response.stderr)
        report = json.loads(response.stdout)
        self.assertEqual(report["endpoint"]["selected"], server.path)
        self.assertEqual(report["agents"][0]["job_completion"], "unestablished")
        self.assertEqual(len(server.requests), 1)
        self.assertEqual(json.loads(server.requests[0])["method"], "session.snapshot")
        self.assertEqual(workspace_snapshot(self.workspace), before)
