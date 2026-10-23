"""Readiness is help-only observation, independent of workspace authorities."""

import contextlib
import io
import json
import os
import subprocess
import sys
from unittest import mock

from runner import coven, storage, workers
from test_coven import CovenTestCase, ROOT, snapshot
from test_workers import FakeProbe, help_text, result


class WorkerReadinessCommandTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace, "--blank", "--agents", "architect", "--multiplexer", "tmux")
        self.executable = self.root / "djinn-inspection-fixture"
        self.executable.write_text("#!/bin/sh\nexit 99\n")
        self.executable.chmod(0o700)

    def readiness(self, fake=None, expected=2, workspace=None, json_output=True, executable=None):
        args = ["worker", "readiness", str(workspace or self.workspace), "--executable", str(executable or self.executable)]
        if json_output:
            args.append("--json")
        output = io.StringIO()
        with mock.patch.object(workers, "run_help_probe", side_effect=fake or FakeProbe()), contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(args), expected)
        return json.loads(output.getvalue()) if json_output else output.getvalue()

    def test_all_workspace_authorities_are_byte_preserved_without_locks_or_native_effects(self):
        for authority in ("snapshot", "task-ledger", "approved-launch", "execution-preparation", "execution-graphs"):
            with self.subTest(authority=authority):
                if authority == "task-ledger":
                    token = json.loads(self.invoke("task", "list", self.workspace, "--json"))["revision"]
                    self.invoke("state", "import-tasks", self.workspace, "--expect-revision", token)
                if authority == "approved-launch":
                    self.invoke("action", "enable-runtime-launch", self.workspace)
                if authority == "execution-preparation":
                    self.invoke("execution", "enable", self.workspace, "--expect-execution-revision", "execution-disabled:v1",
                                "--request-id", "enable-readiness-fixture")
                if authority == "execution-graphs":
                    revision = json.loads(self.invoke("execution", "inspect", self.workspace, "--json"))["execution"]["revision"]
                    self.invoke("execution", "upgrade", self.workspace, "--expect-execution-revision", revision,
                                "--request-id", "upgrade-readiness-fixture")
                before = snapshot(self.workspace)
                fake = FakeProbe()
                with mock.patch.object(storage, "workspace_lock", side_effect=AssertionError("unexpected workspace lock")), \
                        mock.patch.object(storage, "transport_gate", side_effect=AssertionError("unexpected transport gate")), \
                        mock.patch.object(coven.subprocess, "Popen", side_effect=AssertionError("unexpected native launch")):
                    report = self.readiness(fake)
                self.assertEqual(snapshot(self.workspace), before)
                self.assertEqual(report["authorization"], "none")
                self.assertEqual(report["assessment"], "unknown")
                self.assertEqual(report["context"]["policy_inspected"], False)
                self.assertEqual(report["context"]["sessions_inspected"], False)
                self.assertEqual(len(fake.calls), 7)
                self.assertTrue(all(call["arguments"] in workers.HELP_PROBE_ALLOWLIST and call["arguments"][-1] == "--help" for call in fake.calls))

    def test_corrupt_workspace_records_do_not_prevent_independent_diagnostics_or_get_repaired(self):
        for relative in ("coven.json", "state/tasks.json", "state/lead.json", "state/runtime-launch-control.json"):
            path = self.workspace / relative
            path.write_bytes(b"corrupt record\xff")
        before = snapshot(self.workspace)
        report = self.readiness()
        self.assertEqual(report["assessment"], "unknown")
        self.assertEqual(snapshot(self.workspace), before)

    def test_readiness_works_outside_a_generated_workspace_without_creating_one(self):
        directory = self.root / "ordinary project"
        directory.mkdir()
        before = snapshot(directory)
        report = self.readiness(workspace=directory)
        self.assertEqual(report["context"]["workspace"], str(directory.resolve()))
        self.assertEqual(snapshot(directory), before)
        self.assertFalse((directory / "coven.json").exists())

    def test_cli_exit_codes_distinguish_missing_route_and_unknown_probe(self):
        blocked = self.readiness(FakeProbe({("--help",): result(help_text(commands=("ask",)))}), expected=1)
        self.assertEqual(blocked["assessment"], "blocked")
        self.assertIn("unsupported_advertised_session_route", {item["code"] for item in blocked["blockers"]})
        missing = self.readiness(executable=self.root / "not-installed")
        self.assertEqual(missing["assessment"], "unknown")
        self.assertEqual(missing["probes"], [])
        self.assertIn("executable_missing", {item["code"] for item in missing["blockers"]})

    def test_human_output_does_not_claim_execution_or_policy_support(self):
        output = self.readiness(json_output=False)
        self.assertIn("UNKNOWN", output)
        self.assertIn("Authorization: none", output)
        self.assertIn("No worker created", output)
        self.assertIn("readonly_policy_unverified", output)
        self.assertIn("Native version: not observed", output)

    def test_bad_limits_fail_before_probes_or_any_workspace_change(self):
        before = snapshot(self.workspace)
        for options in (("--timeout", "nan"), ("--timeout", "0"), ("--timeout", "31"),
                        ("--max-output-bytes", "1"), ("--max-output-bytes", "1048577")):
            with self.subTest(options=options), mock.patch.object(workers, "run_help_probe") as probe:
                self.invoke("worker", "readiness", self.workspace, *options, error="Invalid worker readiness request")
                probe.assert_not_called()
                self.assertEqual(snapshot(self.workspace), before)

    def test_no_worker_submission_command_exists_as_a_help_probe_fallback(self):
        errors = io.StringIO()
        before = snapshot(self.workspace)
        with mock.patch.object(workers, "run_help_probe") as probe, contextlib.redirect_stderr(errors), self.assertRaises(SystemExit):
            coven.main(["worker", "run", str(self.workspace)])
        self.assertIn("invalid choice", errors.getvalue())
        probe.assert_not_called()
        self.assertEqual(snapshot(self.workspace), before)

    def test_real_wrapper_with_help_only_fixture_uses_no_session_or_config_route(self):
        self.executable.write_text(f"#!{sys.executable}\n" + r'''
import sys
args = tuple(sys.argv[1:])
allowed = {('--help',),('session','--help'),('session','run','--help'),('session','status','--help'),
           ('session','watch','--help'),('ask','--help'),('agents','--help')}
if args not in allowed: raise SystemExit(97)
route = args[:-1]
commands = ('session','ask','agents') if not route else (('run','status','watch') if route == ('session',) else ())
lines = ['Usage: djinn ' + ' '.join((*route,'[OPTIONS]'))]
if commands: lines += ['', 'Commands:', *('  '+item+'  Description' for item in commands)]
lines += ['', 'Options:', '  --help  Print help', '']
print('\n'.join(lines))
''')
        env = {key: value for key, value in os.environ.items() if key not in {"COVEN_HOME", "COVEN_COMMAND"}}
        before = snapshot(self.workspace)
        response = subprocess.run([str(ROOT / "bin/coven"), "worker", "readiness", str(self.workspace),
                                   "--executable", str(self.executable), "--json"], env=env, cwd=ROOT,
                                  text=True, capture_output=True, timeout=30)
        self.assertEqual(response.returncode, 2, response.stderr)
        report = json.loads(response.stdout)
        self.assertEqual(len(report["probes"]), 7)
        self.assertEqual(report["assessment"], "unknown")
        self.assertEqual(snapshot(self.workspace), before)
