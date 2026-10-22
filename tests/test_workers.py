"""Help-only tests: no installed Djinn, provider, config or session calls."""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

from runner import workers


def help_text(route=(), commands=(), flags=("--help",), prose=""):
    lines = [prose, "Usage: djinn " + " ".join((*route, "[OPTIONS]"))]
    if commands:
        lines.extend(["", "Commands:", *(f"  {name}  Description" for name in commands)])
    lines.extend(["", "Options:", *(f"  {flag}  Description" for flag in flags), ""])
    return "\n".join(lines).encode()


class FakeProbe:
    def __init__(self, overrides=None):
        self.calls = []
        self.overrides = overrides or {}

    def __call__(self, **kwargs):
        self.calls.append(kwargs)
        arguments = kwargs["arguments"]
        if arguments in self.overrides:
            value = self.overrides[arguments]
            if callable(value):
                return value(kwargs)
            return value
        route = arguments[:-1]
        commands = ("session", "ask", "agents") if not route else (
            ("run", "status", "watch") if route == ("session",) else ())
        return workers.ProbeResult("ok", 0, help_text(route, commands), b"", 1.0)


def result(data, status="ok", code=0, stderr=b""):
    return workers.ProbeResult(status, code, data, stderr, 1.0)


class ReadinessTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.workspace = Path(self.temp.name).resolve()
        self.executable = self.workspace / "dummy"
        self.executable.write_bytes(b"#!/bin/sh\nexit 99\n")
        self.executable.chmod(0o700)

    def inspect(self, fake=None, **kwargs):
        return workers.inspect_readiness(
            self.workspace, executable=str(self.executable),
            probe_runner=fake or FakeProbe(), **kwargs)

    def test_complete_surface_is_unknown_and_json_safe(self):
        fake = FakeProbe()
        report = self.inspect(fake)
        self.assertEqual(report, json.loads(json.dumps(report, allow_nan=False)))
        self.assertEqual(report["schema_version"], 1)
        self.assertEqual(report["adapter_contract"], "djinn-help-v1")
        self.assertEqual(report["assessment"], "unknown")
        self.assertEqual(workers.report_exit_code(report), 2)
        self.assertEqual(report["authorization"], "none")
        self.assertIsNone(report["native_version"])
        self.assertEqual(report["context"], {
            "workspace": str(self.workspace), "configuration_inspected": False,
            "policy_inspected": False, "sessions_inspected": False})
        for fact in report["capabilities"].values():
            self.assertEqual(fact, {"state": "unknown", "evidence_source": "unverified"})
        codes = {b["code"] for b in report["blockers"]}
        self.assertTrue({"worker_contract_unverified", "native_identity_unverified",
                         "reconcile_unverified", "inspection_purity_unverified",
                         "cancellation_unverified", "readonly_policy_unverified",
                         "resource_scope_unverified"} <= codes)
        self.assertEqual(len(fake.calls), 7)
        for call, provenance in zip(fake.calls, report["probes"]):
            self.assertEqual(call["executable"], self.executable.resolve())
            self.assertIn(call["arguments"], workers.HELP_PROBE_ALLOWLIST)
            self.assertEqual(call["arguments"][-1], "--help")
            self.assertEqual(call["cwd"], self.workspace)
            self.assertLessEqual(set(call["env"]), workers.ENV_KEYS)
            self.assertEqual(provenance["argv"], [str(self.executable), *call["arguments"]])
            self.assertIn("observed_at", provenance)
            self.assertEqual(len(provenance["output_sha256"]), 64)
        self.assertEqual(report["executable"]["identity"]["sha256"],
                         hashlib.sha256(self.executable.read_bytes()).hexdigest())
        self.assertEqual(report["executable"]["identity"], report["executable"]["identity_after"])

    def test_symlink_selected_but_resolved_path_invoked(self):
        alias = self.workspace / "alias"
        alias.symlink_to(self.executable)
        fake = FakeProbe()
        report = workers.inspect_readiness(self.workspace, executable=str(alias), probe_runner=fake)
        self.assertEqual(report["executable"]["selected_path"], str(alias))
        self.assertEqual(report["executable"]["resolved_path"], str(self.executable))
        self.assertTrue(all(call["executable"] == self.executable for call in fake.calls))

    def test_executable_change_before_next_probe_stops_execution(self):
        def change(_):
            self.executable.write_bytes(b"changed executable")
            return result(help_text(commands=("session", "ask", "agents")))
        fake = FakeProbe({("--help",): change})
        report = self.inspect(fake)
        self.assertEqual(len(fake.calls), 1)
        self.assertEqual(report["assessment"], "unknown")
        self.assertIn("executable_changed", [b["code"] for b in report["blockers"]])

    def test_hash_after_probe_detects_same_metadata_rewrite(self):
        info = self.executable.stat()
        def change(_):
            original = self.executable.read_bytes()
            self.executable.write_bytes(b"X" * len(original))
            os.utime(self.executable, ns=(info.st_atime_ns, info.st_mtime_ns))
            return result(help_text(commands=()))
        report = self.inspect(FakeProbe({("--help",): change}))
        self.assertEqual(report["assessment"], "unknown")
        self.assertIn("executable_changed", [b["code"] for b in report["blockers"]])

    def test_alias_retargeted_during_last_probe_is_unknown(self):
        alias = self.workspace / "alias"
        alias.symlink_to(self.executable)
        replacement = self.workspace / "replacement"
        replacement.write_bytes(self.executable.read_bytes())
        replacement.chmod(0o700)

        def retarget(_):
            alias.unlink()
            alias.symlink_to(replacement)
            return result(help_text())

        fake = FakeProbe({("--help",): retarget})
        report = workers.inspect_readiness(self.workspace, executable=str(alias), probe_runner=fake)
        self.assertEqual(report["assessment"], "unknown")
        self.assertEqual(len(fake.calls), 1)
        self.assertIn("executable_changed", [b["code"] for b in report["blockers"]])

    def test_oversized_executable_not_hashed_or_probed(self):
        with self.executable.open("r+b") as stream:
            stream.truncate(workers.MAX_EXECUTABLE_BYTES + 1)
        fake = FakeProbe()
        report = self.inspect(fake)
        self.assertEqual(report["assessment"], "unknown")
        self.assertEqual(fake.calls, [])
        self.assertIsNone(report["executable"]["identity"])

    def test_missing_commands_blocked_but_probe_failures_unknown(self):
        for overrides in (
            {("--help",): result(help_text(commands=("ask",)))},
            {("session", "--help"): result(help_text(("session",), ("status",)))},
        ):
            fake = FakeProbe(overrides)
            report = self.inspect(fake)
            self.assertEqual(report["assessment"], "blocked")
            self.assertEqual(workers.report_exit_code(report), 1)
            self.assertIn("unsupported_advertised_session_route",
                          [b["code"] for b in report["blockers"]])
            self.assertNotIn(("session", "run", "--help"), [c["arguments"] for c in fake.calls])
        for status in ("nonzero", "timeout", "output_limit", "spawn_error"):
            fake = FakeProbe({("--help",): result(b"secret TOKEN=private", status, 1)})
            report = self.inspect(fake)
            self.assertEqual(report["assessment"], "unknown")
            self.assertEqual(len(fake.calls), 1)
            self.assertIn("probe_failure", [b["code"] for b in report["blockers"]])
            self.assertNotIn("TOKEN=private", json.dumps(report))

    def test_missing_executable_and_nonexecutables_never_probe(self):
        fake = FakeProbe()
        with mock.patch.object(workers.shutil, "which", return_value=None) as which:
            report = workers.inspect_readiness(self.workspace, probe_runner=fake)
        which.assert_called_once_with("djinn")
        self.assertIsNone(report["executable"]["selected_path"])
        self.assertIn("executable_missing", [b["code"] for b in report["blockers"]])
        for path in (self.workspace / "missing", self.workspace):
            report = workers.inspect_readiness(self.workspace, executable=str(path), probe_runner=fake)
            self.assertEqual(report["assessment"], "unknown")
        self.executable.chmod(0o600)
        self.assertEqual(self.inspect(fake)["probes"], [])
        self.assertEqual(fake.calls, [])

    def test_roles_flags_and_versions_do_not_attest_semantics(self):
        root = help_text(commands=("session", "ask", "agents"), flags=("--help", "--version"),
                         prose="Research-readonly role enforces read-only policy. --capabilities JSON")
        fake = FakeProbe({("--help",): result(root), ("--version",): result(b"djinn 9.8.7\n"),
                          ("session", "run", "--help"): result(help_text(
                              ("session", "run"), flags=("--help", "--background", "--readonly")))})
        report = self.inspect(fake)
        self.assertEqual(report["native_version"], "djinn 9.8.7")
        self.assertEqual(report["assessment"], "unknown")
        self.assertFalse(report["cli_surfaces"]["version"]["attestation"])
        self.assertTrue(all(c["state"] == "unknown" for c in report["capabilities"].values()))
        self.assertEqual(fake.calls[-1]["arguments"], ("--version",))
        self.assertNotIn("--capabilities", report["cli_surfaces"]["root"]["flags"])

    def test_version_only_when_options_section_advertises_it(self):
        fake = FakeProbe({("--help",): result(help_text(commands=("session",),
                          prose="Try --version. Commands: ask agents"))})
        report = self.inspect(fake)
        self.assertIsNone(report["native_version"])
        self.assertNotIn(("--version",), [c["arguments"] for c in fake.calls])
        self.assertNotIn(("ask", "--help"), [c["arguments"] for c in fake.calls])
        for text in (b"package version unknown\n", b"{\"version\":\"1.2.3\"}\n", b"\xff"):
            fake = FakeProbe({("--help",): result(help_text(flags=("--help", "--version"))),
                              ("--version",): result(text)})
            report = self.inspect(fake)
            self.assertEqual(report["assessment"], "unknown")
            self.assertIsNone(report["native_version"])

    def test_wrapped_option_prose_does_not_advertise_version(self):
        data = help_text(commands=("session",)).replace(
            b"  --help  Description", b"  -h, --help  Description\n          --version is prose")
        fake = FakeProbe({("--help",): result(data)})
        report = self.inspect(fake)
        self.assertEqual(report["cli_surfaces"]["root"]["flags"], ["--help"])
        self.assertNotIn(("--version",), [c["arguments"] for c in fake.calls])
        # Clap aligns long-only options with the long spelling of short options.
        fake = FakeProbe({("--help",): result(data.replace(
            b"          --version is prose", b"      --version  Print version")),
            ("--version",): result(b"djinn 1.2.3\n")})
        self.assertEqual(self.inspect(fake)["native_version"], "djinn 1.2.3")

    def test_malformed_nonutf8_json_truncated_help_is_unknown(self):
        for text in (b"session run --help\n", b'{"Commands": ["session"]}\n',
                     b"\xff", help_text()[:-1], b"Usage: djinn [OPTIONS]\n",
                     b"Usage: djinn [OPTIONS]\nCommands:\nsession\n",
                     b"Usage: djinn [OPTIONS]\nCommands:\n",
                     b"Usage: djinn [OPTIONS]\nOptions:\n",
                     b"Usage: djinn [OPTIONS]\nOptions:\n  --help  Help\x00\n"):
            for arguments in (("--help",), ("session", "--help")):
                fake = FakeProbe({arguments: result(text)})
                report = self.inspect(fake)
                self.assertEqual(report["assessment"], "unknown")
                self.assertIn("probe_parse_error", [b["code"] for b in report["blockers"]])
                self.assertTrue(all(c["arguments"] in workers.HELP_PROBE_ALLOWLIST for c in fake.calls))

    def test_ansi_help_and_bounded_sections(self):
        text = help_text(commands=("session",), prose="Examples:\n  ask  not a command")
        text = text.replace(b"Usage:", b"\x1b[1mUsage:\x1b[0m")
        text = text.replace(b"Commands:", b"\x1b[32mCommands:\x1b[0m")
        fake = FakeProbe({("--help",): result(text)})
        report = self.inspect(fake)
        self.assertEqual(report["cli_surfaces"]["root"]["commands"], ["session"])
        self.assertEqual(report["cli_surfaces"]["root"]["help_sha256"], hashlib.sha256(text).hexdigest())
        self.assertNotIn(("ask", "--help"), [c["arguments"] for c in fake.calls])

    def test_invalid_arguments_raise_before_any_probe(self):
        for kwargs in ({"timeout": 0}, {"timeout": 31}, {"timeout": float("nan")},
                       {"timeout": float("inf")}, {"timeout": True}, {"timeout": "3"},
                       {"timeout": 10 ** 1000},
                       {"max_output_bytes": 1023}, {"max_output_bytes": 1048577},
                       {"max_output_bytes": 1024.0}, {"max_output_bytes": True},
                       {"adapter": "other"}, {"profile": "read-only"}):
            fake = FakeProbe()
            with self.subTest(kwargs=kwargs), self.assertRaises(ValueError):
                self.inspect(fake, **kwargs)
            self.assertEqual(fake.calls, [])
        for workspace in (str(self.workspace), self.workspace / "missing", self.executable):
            with self.assertRaises(ValueError):
                workers.inspect_readiness(workspace, probe_runner=FakeProbe())

    def test_safe_environment_no_values_in_report(self):
        fake = FakeProbe()
        with mock.patch.dict(os.environ, {"PATH": "private-path", "HOME": "private-home",
                                          "PROVIDER_TOKEN": "private-token"}, clear=True):
            report = self.inspect(fake)
        self.assertEqual(fake.calls[0]["env"], {"PATH": "private-path", "HOME": "private-home"})
        self.assertNotIn("private-", json.dumps(report))

    def test_fake_oversize_and_exception_are_fail_closed(self):
        for override in (result(b"x" * 1025), lambda _: (_ for _ in ()).throw(OSError("secret"))):
            report = self.inspect(FakeProbe({("--help",): override}), max_output_bytes=1024)
            self.assertEqual(report["assessment"], "unknown")
            self.assertLessEqual(report["probes"][0]["output_size"], 1024)
            self.assertNotIn("secret", json.dumps(report))

    def test_reserved_exit_codes(self):
        for assessment, code in (("compatible", 0), ("blocked", 1), ("unknown", 2), ("bad", 2)):
            self.assertEqual(workers.report_exit_code({"assessment": assessment}), code)


class BoundedRunnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()

    def script(self, body):
        path = self.root / "fake-help"
        path.write_text(f"#!{sys.executable}\nimport os, sys, time\n"
                        "assert sys.argv[1:] == ['--help']\n" + body)
        path.chmod(0o700)
        return path

    def run_probe(self, path, **kwargs):
        return workers.run_help_probe(path, ("--help",), cwd=self.root,
                                      env={}, timeout=kwargs.get("timeout", 0.5),
                                      max_output_bytes=kwargs.get("limit", 1024))

    def test_success_nonzero_nonutf8_and_spawn_error(self):
        path = self.script("os.write(1, b'help\\n'); os.write(2, b'warning\\n')\n")
        probe = self.run_probe(path)
        self.assertEqual((probe.status, probe.returncode, probe.stdout, probe.stderr),
                         ("ok", 0, b"help\n", b"warning\n"))
        path = self.script("os.write(1, b'bad'); sys.exit(4)\n")
        self.assertEqual(self.run_probe(path).status, "nonzero")
        path = self.script("os.write(1, b'\\xff')\n")
        report = workers.inspect_readiness(self.root, executable=str(path), timeout=0.5)
        self.assertEqual(report["assessment"], "unknown")
        self.assertEqual(report["probes"][0]["parse_status"], "error")
        self.assertEqual(self.run_probe(self.root / "missing").status, "spawn_error")

    def test_exact_limit_and_combined_output_limit(self):
        path = self.script("os.write(1, b'x' * 512); os.write(2, b'y' * 512)\n")
        probe = self.run_probe(path)
        self.assertEqual(probe.status, "ok")
        self.assertEqual(len(probe.stdout) + len(probe.stderr), 1024)
        for body in ("os.write(1, b'x' * 1000000)\n",
                     "os.write(2, b'x' * 1000000)\n",
                     "os.write(1, b'x' * 600); os.write(2, b'y' * 600)\n"):
            probe = self.run_probe(self.script(body))
            self.assertEqual(probe.status, "output_limit")
            self.assertEqual(len(probe.stdout) + len(probe.stderr), 1024)

    def test_timeout_even_with_closed_pipes(self):
        probe = self.run_probe(self.script("os.close(1); os.close(2); time.sleep(10)\n"), timeout=0.1)
        self.assertEqual(probe.status, "timeout")
        self.assertLess(probe.duration_ms, 2000)
        self.assertIsNone(probe.returncode)  # Native exit was not acknowledged.

    def test_process_safety_options_and_stdin(self):
        path = self.script("assert sys.stdin.buffer.read() == b''\n"
                           "assert 'PROVIDER_TOKEN' not in os.environ\n"
                           "os.write(1, b'ok')\n")
        real_popen = workers.subprocess.Popen
        with mock.patch.object(workers.subprocess, "Popen", wraps=real_popen) as popen:
            probe = self.run_probe(path)
        self.assertEqual(probe.status, "ok")
        args, kwargs = popen.call_args
        self.assertEqual(args[0][:4], [sys.executable, "-I", "-S", "-c"])
        self.assertEqual(args[0][-2:], [str(path), "--help"])
        self.assertEqual(len(kwargs["pass_fds"]), 2)
        self.assertEqual(kwargs["stdin"], workers.subprocess.DEVNULL)
        self.assertTrue(kwargs["close_fds"])
        self.assertTrue(kwargs["start_new_session"])
        self.assertNotIn("shell", kwargs)
        self.assertEqual(kwargs["env"], {})
        self.assertEqual(kwargs["cwd"], self.root)

    def test_descendant_inherited_pipes_killed_without_waiting_for_eof(self):
        marker = self.root / "should-not-exist"
        body = ("pid = os.fork()\nif pid == 0:\n"
                f"    time.sleep(0.7)\n    open({str(marker)!r}, 'w').close()\n    os._exit(0)\n"
                "os.write(1, b'leader done'); os._exit(0)\n")
        started = time.monotonic()
        probe = self.run_probe(self.script(body), timeout=0.15)
        self.assertEqual(probe.status, "timeout")
        self.assertLess(time.monotonic() - started, 1.5)
        time.sleep(0.8)
        self.assertFalse(marker.exists())

    def test_output_limit_kills_descendants(self):
        marker = self.root / "should-not-exist"
        body = ("pid = os.fork()\nif pid == 0:\n"
                f"    time.sleep(0.7)\n    open({str(marker)!r}, 'w').close()\n    os._exit(0)\n"
                "os.write(1, b'x' * 1000000); time.sleep(10)\n")
        probe = self.run_probe(self.script(body))
        self.assertEqual(probe.status, "output_limit")
        self.assertIsNone(probe.returncode)
        time.sleep(0.8)
        self.assertFalse(marker.exists())

    def test_success_and_nonzero_leaders_cannot_leave_closed_pipe_descendants(self):
        for code in (0, 4):
            with self.subTest(code=code):
                marker = self.root / f"surviving-child-{code}"
                body = ("pid = os.fork()\nif pid == 0:\n"
                        "    os.close(1); os.close(2)\n"
                        f"    time.sleep(0.7)\n    open({str(marker)!r}, 'w').close()\n    os._exit(0)\n"
                        f"os.write(1, {help_text()!r}); os._exit({code})\n")
                probe = self.run_probe(self.script(body), timeout=0.5)
                self.assertEqual(probe.status, "ok" if code == 0 else "nonzero")
                self.assertEqual(probe.returncode, code)
                time.sleep(0.8)
                self.assertFalse(marker.exists())

    def test_cleanup_error_is_a_failed_probe_not_success(self):
        path = self.script("os.write(1, b'help\\n')\n")
        with mock.patch.object(workers.os, "killpg", side_effect=PermissionError("cannot confirm cleanup")):
            self.assertEqual(self.run_probe(path).status, "spawn_error")

    def test_supervisor_isolated_from_workspace_import_shadowing(self):
        for name in ("subprocess.py", "json.py"):
            (self.root / name).write_text("raise RuntimeError('workspace module must not load in supervisor')\n")
        path = self.script("os.write(1, b'help\\n')\n")
        self.assertEqual(self.run_probe(path).status, "ok")

    def test_external_child_reaper_context_is_not_supported(self):
        with mock.patch.object(workers.signal, "getsignal", return_value=workers.signal.SIG_IGN), \
                mock.patch.object(workers.subprocess, "Popen") as popen:
            probe = self.run_probe(self.root / "not-run")
        self.assertEqual(probe.status, "spawn_error")
        popen.assert_not_called()

    def test_caller_termination_cleans_active_and_completed_native_groups(self):
        root = Path(__file__).resolve().parents[1]
        for termination in ("terminate", "kill"):
            for completed in (False, True):
                with self.subTest(termination=termination, completed=completed):
                    marker = self.root / f"late-{termination}-{completed}"
                    ready = self.root / f"ready-{termination}-{completed}"
                    body = ("pid = os.fork()\nif pid == 0:\n"
                            "    os.close(1); os.close(2)\n"
                            f"    time.sleep(0.8)\n    open({str(marker)!r}, 'w').close()\n    os._exit(0)\n")
                    if completed:
                        body += "os.write(1, b'help\\n'); os._exit(0)\n"
                    else:
                        body += f"open({str(ready)!r}, 'w').close()\nos.close(1); os.close(2); time.sleep(5)\n"
                    path = self.script(body)
                    code = """
import sys,time
from pathlib import Path
from runner import workers
ready=Path(sys.argv[2])
if sys.argv[3]=='completed':
    original=workers.os.killpg
    def paused(*args):
        ready.touch()
        time.sleep(5)
        return original(*args)
    workers.os.killpg=paused
workers.run_help_probe(Path(sys.argv[1]),('--help',),timeout=0.5,max_output_bytes=1024,cwd=ready.parent,env={})
"""
                    caller = subprocess.Popen([sys.executable, "-c", code, str(path), str(ready),
                                               "completed" if completed else "active"], cwd=root)
                    try:
                        deadline = time.monotonic() + 3
                        while not ready.exists() and caller.poll() is None and time.monotonic() < deadline:
                            time.sleep(0.01)
                        self.assertTrue(ready.exists())
                        getattr(caller, termination)()
                        caller.wait(timeout=2)
                        time.sleep(0.9)
                        self.assertFalse(marker.exists())
                    finally:
                        if caller.poll() is None:
                            caller.kill()
                        caller.wait(timeout=2)

    def test_supervisor_watchdog_bounds_a_stalled_parent_cleanup(self):
        root = Path(__file__).resolve().parents[1]
        marker, ready = self.root / "watchdog-late", self.root / "watchdog-ready"
        path = self.script("pid = os.fork()\nif pid == 0:\n"
                           "    os.close(1); os.close(2)\n"
                           f"    time.sleep(1.8)\n    open({str(marker)!r}, 'w').close()\n    os._exit(0)\n"
                           "os.write(1, b'help\\n'); os._exit(0)\n")
        code = """
import sys,time
from pathlib import Path
from runner import workers
original=workers.os.killpg
def stalled(*args):
    Path(sys.argv[2]).touch()
    time.sleep(5)
    return original(*args)
workers.os.killpg=stalled
workers.run_help_probe(Path(sys.argv[1]),('--help',),timeout=0.2,max_output_bytes=1024,cwd=Path(sys.argv[2]).parent,env={})
"""
        caller = subprocess.Popen([sys.executable, "-c", code, str(path), str(ready)], cwd=root)
        try:
            deadline = time.monotonic() + 3
            while not ready.exists() and caller.poll() is None and time.monotonic() < deadline:
                time.sleep(0.01)
            self.assertTrue(ready.exists())
            time.sleep(2)
            self.assertFalse(marker.exists())
        finally:
            if caller.poll() is None:
                caller.kill()
            caller.wait(timeout=2)

    def test_allowlist_validated_before_spawn(self):
        with mock.patch.object(workers.subprocess, "Popen") as popen:
            for arguments in (("session", "status"), ("session", "watch"), ("session", "ls"),
                              ("session", "run"), ("config", "--help"), ("--capabilities",),
                              ("doctor", "ui"), ["--help"], (["--help"],)):
                with self.subTest(arguments=arguments), self.assertRaises(ValueError):
                    workers.run_help_probe(self.root / "fake", arguments, timeout=1,
                                           max_output_bytes=1024, cwd=self.root, env={})
            popen.assert_not_called()

    def test_real_fake_help_round_trip_never_other_arguments(self):
        # This script accepts root help only; an unadvertised route would fail.
        path = self.script(f"os.write(1, {help_text()!r})\n")
        report = workers.inspect_readiness(self.root, executable=str(path), timeout=0.5)
        self.assertEqual(report["assessment"], "blocked")
        self.assertEqual(len(report["probes"]), 1)
        self.assertEqual(report["probes"][0]["status"], "ok")


if __name__ == "__main__":
    unittest.main()
