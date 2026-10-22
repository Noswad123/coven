"""All runtime calls are fake; no native multiplexer or provider is invoked."""

import copy
import json
import subprocess
import unittest
from types import SimpleNamespace
from unittest import mock

from runner import actions, launch


class FakeRunner:
    def __init__(self, backend="tmux", workspaces=None, overrides=None):
        self.backend = backend
        self.workspaces = [] if workspaces is None else workspaces
        self.overrides = overrides or {}
        self.calls = []
        self.tabs = 0

    def __call__(self, argv, **kwargs):
        index = len(self.calls)
        self.calls.append((list(argv), kwargs))
        if index in self.overrides:
            value = self.overrides[index]
            if isinstance(value, Exception):
                raise value
            return value
        code, payload = 0, {}
        if argv[:2] == ["tmux", "has-session"]:
            return response(1, "", f"can't find session: {argv[-1].removeprefix('=')}")
        if argv[:3] == ["herdr", "workspace", "list"]:
            payload = {"result": {"workspaces": self.workspaces}}
        if argv[:3] == ["herdr", "workspace", "create"]:
            payload = {"result": {"workspace": {"workspace_id": "ws_1"}, "root_pane": {"pane_id": "root-1"}}}
        if argv[:3] == ["herdr", "tab", "create"]:
            self.tabs += 1
            payload = {"result": {"root_pane": {"pane_id": f"pane_{self.tabs}"}}}
        return response(code, json.dumps(payload))


def response(code=0, stdout="{}", stderr=""):
    return SimpleNamespace(returncode=code, stdout=stdout, stderr=stderr)


class LaunchTests(unittest.TestCase):
    def plan(self, backend="tmux", **kwargs):
        inputs = dict(root="/not/read/workspace", namespace="coven-a1-b2", multiplexer=backend,
                      agents=[{"id": "alice", "command": "exec worker --prompt /mutable/prompt.md",
                               "starter": "- literal 'quotes' \"double\" $HOME $(touch nope)\n\u96ea"}],
                      environment={"PATH": "/usr/bin:/bin", "HOME": "/frozen", "LANG": "C.UTF-8"})
        inputs.update(kwargs)
        return launch.build_plan(**inputs)

    def run_plan(self, backend="tmux", fake=None):
        plan = self.plan(backend)
        fake = fake or FakeRunner(backend)
        sleeper = mock.Mock()
        report = launch.execute(plan, runner=fake, sleeper=sleeper)
        return plan, fake, sleeper, report

    def test_deterministic_and_independent(self):
        for backend in ("tmux", "herdr"):
            plan = self.plan(backend)
            self.assertEqual(plan, self.plan(backend))
            frozen = launch.validate_plan(plan)
            self.assertEqual(plan, frozen)
            self.assertIsNot(plan["agents"], frozen["agents"])
            self.assertEqual(launch.validate_plan(json.loads(json.dumps(plan))), plan)

    def test_strict_recomputed_schema(self):
        mutations = [lambda p: p.update(version=True), lambda p: p.update(version=2),
                     lambda p: p.update(operation="task_dispatch"), lambda p: p.update(cwd="/elsewhere"),
                     lambda p: p.update(hidden_program="evil"), lambda p: p["steps"].reverse(),
                     lambda p: p["steps"][1]["argv"].append("evil"),
                     lambda p: p["steps"][3].update(seconds=5.0),
                     lambda p: p["steps"][1].update(outputs={"evil": ["command"]})]
        for mutation in mutations:
            plan = self.plan()
            mutation(plan)
            with self.subTest(plan=plan), self.assertRaises(ValueError):
                launch.validate_plan(plan)
            fake = FakeRunner()
            self.assertEqual(launch.execute(plan, fake)["status"], "transport_failed_or_partial")
            self.assertEqual(fake.calls, [])

    def test_invalid_inputs(self):
        for kwargs in ({"root": "relative"}, {"root": "/bad\0path"}, {"namespace": "coven-human"},
                       {"namespace": "coven-\uff11-ab"}, {"multiplexer": "screen"}, {"agents": []},
                       {"environment": {"TOKEN": "secret"}}, {"environment": {"PATH": "a\0b"}},
                       {"environment": {"PATH": "x" * 32769}}, {"environment": {"LANG": "\ud800"}}):
            with self.subTest(kwargs=kwargs), self.assertRaises(ValueError):
                self.plan(**kwargs)
        agent = self.plan()["agents"][0]
        for agents in ([agent] * 2, [agent] * 33,
                       [{**agent, "id": "alice:other"}], [{**agent, "command": " "}],
                       [{**agent, "starter": "\udfff"}], [{**agent, "starter": "a\0b"}],
                       [{**agent, "extra": "hidden"}]):
            with self.subTest(agents=agents), self.assertRaises(ValueError):
                self.plan(agents=agents)
        with self.assertRaises(ValueError):
            self.plan(agents=[{**agent, "starter": "x" * launch.MAX_PLAN_BYTES}])

    def test_happy_transport_literal_payload_and_exact_call_contract(self):
        for backend in ("tmux", "herdr"):
            plan, fake, sleeper, report = self.run_plan(backend)
            self.assertEqual(report["status"], "transport_completed")
            self.assertEqual(report["completed_steps"], len(plan["steps"]))
            self.assertEqual(report["diagnostics"], [])
            sleeper.assert_called_once_with(5)
            calls = [call[0] for call in fake.calls]
            self.assertEqual(calls[-2][-1], plan["agents"][0]["starter"])
            self.assertEqual(calls[-1][-1].lower(), "enter")
            for argv, kwargs in fake.calls:
                self.assertEqual(kwargs, dict(env=plan["env"], cwd=plan["cwd"],
                                             capture_output=True, text=True, check=False, timeout=60))
                self.assertIsNot(kwargs["env"], plan["env"])
                self.assertNotIn("shell", kwargs)
                self.assertNotIn("attach", argv)
                self.assertNotIn("focus", argv)
            if backend == "tmux":
                self.assertEqual(calls[0], ["tmux", "has-session", "-t", "=coven-a1-b2"])
                self.assertEqual(calls[1], ["tmux", "new-session", "-d", "-s", "coven-a1-b2",
                                            "-n", "orchestrator", "-c", plan["cwd"], "/bin/zsh"])
                self.assertIn("=coven-a1-b2:=agent-alice", calls[-2])
                self.assertEqual(calls[-2][-3:-1], ["-l", "--"])
                self.assertEqual(report["handles"], {})
            else:
                self.assertEqual(report["handles"], {"workspace": "ws_1", "orchestrator": "root-1", "agent_0": "pane_1"})
                self.assertEqual(calls[2], ["herdr", "pane", "run", "root-1", "/bin/zsh"])
                self.assertEqual(calls[3][3:5], ["--workspace", "ws_1"])
                self.assertEqual(calls[4][-1], plan["agents"][0]["command"])

    def test_no_files_or_ambient_environment_even_with_defaults(self):
        for backend in ("tmux", "herdr"):
            fake = FakeRunner(backend)
            with mock.patch("builtins.open", side_effect=AssertionError("file read")), \
                    mock.patch("io.open", side_effect=AssertionError("file read")), \
                    mock.patch("os.environ", new=mock.Mock(side_effect=AssertionError("ambient env"))), \
                    mock.patch.object(launch.subprocess, "run", new=fake), \
                    mock.patch.object(launch.time, "sleep") as sleeper:
                plan = self.plan(backend, environment={})
                report = launch.execute(plan)
                self.assertEqual(report["status"], "transport_completed")
                sleeper.assert_called_once_with(5)
                for _, kwargs in fake.calls:
                    self.assertEqual(kwargs["env"], {})

    def test_collisions_and_query_errors_have_no_startup_effects(self):
        for backend, fake in (("tmux", FakeRunner(overrides={0: response(0)})),
                              ("tmux", FakeRunner(overrides={0: response(2)})),
                              ("herdr", FakeRunner(workspaces=[{"label": "coven-a1-b2"}])),
                              ("herdr", FakeRunner(overrides={0: response(1)}))):
            _, fake, sleeper, report = self.run_plan(backend, fake)
            self.assertEqual(report["status"], "transport_failed_or_partial")
            self.assertEqual(len(fake.calls), 1)
            self.assertEqual(report["completed_steps"], 0)
            sleeper.assert_not_called()

    def test_every_required_command_failure_stops_without_cleanup(self):
        for backend in ("tmux", "herdr"):
            plan = self.plan(backend)
            commands = [s for s in plan["steps"] if s["operation"] != "sleep"]
            for index in range(1, len(commands)):
                fake = FakeRunner(backend, overrides={index: response(9)})
                report = launch.execute(plan, fake, mock.Mock())
                with self.subTest(backend=backend, index=index):
                    self.assertEqual(report["status"], "transport_failed_or_partial")
                    self.assertEqual(len(fake.calls), index + 1)
                    self.assertEqual(report["completed_steps"], index + (index >= len(commands) - 2))
                    self.assertTrue(report["diagnostics"])

    def test_untrusted_json_and_handles_fail_cleanly(self):
        invalid_lists = ["{", '{"result":{"workspaces":null}}', '{"result":{"workspaces":[1]}}',
                         '{"result":{"workspaces":[],"workspaces":[]}}',
                         '{"result":{"workspaces":[]},"extra":NaN}',
                         '{"result":{"workspaces":[]},"extra":Infinity}',
                         '{"result":{"workspaces":[]},"extra":1e999}']
        for output in invalid_lists:
            fake = FakeRunner("herdr", overrides={0: response(stdout=output)})
            report = launch.execute(self.plan("herdr"), fake, mock.Mock())
            self.assertEqual(report["status"], "transport_failed_or_partial")
            self.assertEqual(len(fake.calls), 1)
        for value in (None, "", "$(evil)", "a/b", "x" * 129, "\u96ea", 123):
            payload = {"result": {"workspace": {"workspace_id": value}, "root_pane": {"pane_id": "root"}}}
            fake = FakeRunner("herdr", overrides={1: response(stdout=json.dumps(payload))})
            report = launch.execute(self.plan("herdr"), fake, mock.Mock())
            self.assertEqual(report["status"], "transport_failed_or_partial")
            self.assertEqual(len(fake.calls), 2)
        for index, payload in ((1, {}), (1, {"result": {"workspace": {"workspace_id": "observed"}}}),
                               (3, {"result": {"root_pane": {"pane_id": ";evil"}}})):
            fake = FakeRunner("herdr", overrides={index: response(stdout=json.dumps(payload))})
            report = launch.execute(self.plan("herdr"), fake, mock.Mock())
            self.assertEqual(report["status"], "transport_failed_or_partial")
            self.assertEqual(len(fake.calls), index + 1)
            if index == 1 and payload:
                self.assertEqual(report["handles"], {"workspace": "observed"})
        for output in ('{"result":{},"result":{}}', '{"extra":NaN}', '{"extra":1e999}', '{'):
            for index in (1, 3):
                fake = FakeRunner("herdr", overrides={index: response(stdout=output)})
                report = launch.execute(self.plan("herdr"), fake, mock.Mock())
                self.assertEqual(report["status"], "transport_failed_or_partial")
                self.assertEqual(len(fake.calls), index + 1)

    def test_execution_uses_snapshot_even_if_caller_mutates_plan(self):
        plan = self.plan("herdr")
        fake = FakeRunner("herdr")
        original = copy.deepcopy(plan)

        def runner(argv, **kwargs):
            plan["agents"][0]["starter"] = "changed"
            plan["env"]["HOME"] = "/changed"
            kwargs_env = dict(kwargs["env"])
            result = fake(argv, **kwargs)
            kwargs["env"].clear()
            self.assertEqual(kwargs_env, original["env"])
            return result

        report = launch.execute(plan, runner, mock.Mock())
        self.assertEqual(report["status"], "transport_completed")
        self.assertEqual(fake.calls[-2][0][-1], original["agents"][0]["starter"])

    def test_sleep_failure_does_not_send_starter_or_clean_up(self):
        for backend in ("tmux", "herdr"):
            fake = FakeRunner(backend)
            report = launch.execute(self.plan(backend), fake, mock.Mock(side_effect=RuntimeError("delay failed")))
            self.assertEqual(report["status"], "transport_failed_or_partial")
            self.assertEqual(len(fake.calls), 3 if backend == "tmux" else 5)
            self.assertEqual(report["completed_steps"], len(fake.calls))

    def test_timeouts_oserrors_and_ordinary_exceptions_preserve_partial_progress(self):
        for error in (subprocess.TimeoutExpired("fake", 60), OSError("unavailable"), RuntimeError("fake")):
            fake = FakeRunner("herdr", overrides={4: error})
            report = launch.execute(self.plan("herdr"), fake, mock.Mock())
            self.assertEqual(report["status"], "transport_failed_or_partial")
            self.assertEqual(report["completed_steps"], 4)
            self.assertEqual(report["handles"]["agent_0"], "pane_1")
            self.assertEqual(report["outcome_unknown"], not isinstance(error, RuntimeError))
            self.assertEqual(len(fake.calls), 5)

    def test_interrupts_propagate(self):
        for error in (KeyboardInterrupt, SystemExit):
            with self.assertRaises(error):
                launch.execute(self.plan(), mock.Mock(side_effect=error), mock.Mock())

    def test_ordered_roster_and_delay_for_each_agent(self):
        for backend in ("tmux", "herdr"):
            first = self.plan()["agents"][0]
            plan = self.plan(backend, agents=[first, {**first, "id": "bob", "starter": "second"}])
            fake, sleeper = FakeRunner(backend), mock.Mock()
            report = launch.execute(plan, fake, sleeper)
            self.assertEqual(report["status"], "transport_completed")
            self.assertEqual(sleeper.call_args_list, [mock.call(5), mock.call(5)])
            self.assertEqual(fake.calls[-2][0][-1], "second")

    def test_stop_exact_target_absence_and_no_legacy_adoption(self):
        for backend in ("tmux", "herdr"):
            fake = FakeRunner(backend, workspaces=[{"label": "human-name", "workspace_id": "legacy"}])
            report = launch.stop(self.plan(backend), fake)
            self.assertEqual(report["status"], "transport_completed")
            self.assertEqual(report["completed_steps"], 1)
            self.assertFalse(report["stop_command_attempted"])
            self.assertFalse(report["stop_command_succeeded"])
            self.assertEqual(len(fake.calls), 1)
            fake = (FakeRunner(overrides={0: response(0)}) if backend == "tmux" else
                    FakeRunner(backend, workspaces=[{"label": "coven-a1-b2", "workspace_id": "scoped-id"},
                                                    {"label": "human-name", "workspace_id": "legacy"}]))
            report = launch.stop(self.plan(backend), fake)
            self.assertEqual(report["status"], "transport_completed")
            self.assertTrue(report["stop_command_attempted"])
            self.assertTrue(report["stop_command_succeeded"])
            self.assertEqual(report["completed_steps"], 2)
            expected = (["tmux", "kill-session", "-t", "=coven-a1-b2"] if backend == "tmux" else
                        ["herdr", "workspace", "close", "scoped-id"])
            self.assertEqual(fake.calls[-1][0], expected)

    def test_stop_ambiguity_untrusted_identity_and_failure_never_retry(self):
        for workspaces in ([{"label": "coven-a1-b2", "workspace_id": "one"}] * 2,
                           [{"label": "coven-a1-b2", "workspace_id": "bad;id"}],
                           [{"label": "coven-a1-b2"}]):
            fake = FakeRunner("herdr", workspaces=workspaces)
            report = launch.stop(self.plan("herdr"), fake)
            self.assertEqual(report["status"], "transport_failed_or_partial")
            self.assertEqual(len(fake.calls), 1)
        for backend in ("tmux", "herdr"):
            overrides = {1: response(7)}
            if backend == "tmux":
                overrides[0] = response(0)
            fake = FakeRunner(backend, workspaces=[{"label": "coven-a1-b2", "workspace_id": "scoped"}],
                              overrides=overrides)
            report = launch.stop(self.plan(backend), fake)
            self.assertEqual(report["status"], "transport_failed_or_partial")
            self.assertEqual(report["completed_steps"], 1)
            self.assertEqual(len(fake.calls), 2)
            self.assertTrue(report["stop_command_attempted"])
            self.assertFalse(report["stop_command_succeeded"])
            self.assertTrue(report["outcome_unknown"])

    def test_tmux_query_failure_is_not_evidence_of_absence(self):
        for message in ("error connecting to tmux socket (Permission denied)", "", "unexpected socket failure"):
            for stopping in (False, True):
                with self.subTest(message=message, stopping=stopping):
                    fake = FakeRunner(overrides={0: response(1, "", message)})
                    result = launch.stop(self.plan(), fake) if stopping else launch.execute(self.plan(), fake, lambda _: None)
                    self.assertEqual(result["status"], "transport_failed_or_partial")
                    self.assertEqual(len(fake.calls), 1)
                    self.assertIn("presence query failed", result["diagnostics"][0]["message"])
        for message in ("no server running on /tmp/tmux-test", "error connecting to /tmp/tmux-test (No such file or directory)"):
            fake = FakeRunner(overrides={0: response(1, "", message)})
            self.assertEqual(launch.stop(self.plan(), fake)["status"], "transport_completed")

    def test_timed_out_stop_records_attempt_but_not_success(self):
        fake = FakeRunner(overrides={0: response(0), 1: subprocess.TimeoutExpired("tmux", 60)})
        report = launch.stop(self.plan(), fake)
        self.assertTrue(report["stop_command_attempted"])
        self.assertFalse(report["stop_command_succeeded"])
        self.assertTrue(report["outcome_unknown"])
        self.assertEqual(report["status"], "transport_failed_or_partial")

    def test_orchestrator_role_uses_a_distinct_agent_window(self):
        agent = {**self.plan()["agents"][0], "id": "orchestrator"}
        plan = self.plan(agents=[agent])
        fake = FakeRunner()
        self.assertEqual(launch.execute(plan, fake, lambda _: None)["status"], "transport_completed")
        self.assertTrue(any("agent-orchestrator" in argv for argv, _ in fake.calls))

    def test_invalid_plan_stop_still_has_complete_report_schema(self):
        plan = self.plan()
        plan["steps"] = []
        fake = FakeRunner()
        report = launch.stop(plan, fake)
        self.assertEqual(report["status"], "transport_failed_or_partial")
        self.assertFalse(report["stop_command_attempted"])
        self.assertFalse(report["stop_command_succeeded"])
        self.assertEqual(fake.calls, [])
        actions._report(report, "runtime_stop")
        plan = copy.deepcopy(self.plan())
        plan["steps"] = []
        fake = FakeRunner()
        self.assertEqual(launch.stop(plan, fake)["status"], "transport_failed_or_partial")
        self.assertEqual(fake.calls, [])


if __name__ == "__main__":
    unittest.main()
