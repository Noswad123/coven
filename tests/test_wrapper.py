"""Offline installation-layout and public callback identity regressions."""

import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import tomllib
import unittest
from unittest import mock

from runner import coven


ROOT = Path(__file__).resolve().parents[1]


class WrapperTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="coven-wrapper-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.home = self.root / "isolated tree"
        (self.home / "bin").mkdir(parents=True)
        (self.home / "runner").mkdir()
        self.wrapper = self.home / "bin/coven"
        shutil.copy2(ROOT / "bin/coven", self.wrapper)
        runner = self.home / "runner/coven.py"
        runner.write_text(
            "#!/usr/bin/env python3\nimport json, os\n"
            "print(json.dumps([os.environ['COVEN_HOME'], os.environ['COVEN_COMMAND']]))\n"
        )
        runner.chmod(0o755)
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(("COVEN_", "ARCANA_PUBLIC_"))}

    def run_wrapper(self, path, **env):
        # Exercise Apple's Bash 3.2 too, not just a newer PATH-selected Bash.
        return subprocess.run(["/bin/bash", str(path)], env=self.env | env,
                              text=True, capture_output=True, timeout=10)

    def test_relative_and_absolute_symlink_chain_finds_full_tree(self):
        relative = self.root / "relative"
        relative.symlink_to("isolated tree/bin/coven")
        absolute = self.root / "absolute"
        absolute.symlink_to(relative)
        for path in (self.wrapper, relative, absolute):
            with self.subTest(path=path):
                result = self.run_wrapper(path)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout), [str(self.home), str(self.wrapper)])

    def test_explicit_home_bypasses_discovery(self):
        elsewhere = self.root / "elsewhere"
        elsewhere.mkdir()
        wrapper = elsewhere / "coven"
        shutil.copy2(self.wrapper, wrapper)
        result = self.run_wrapper(wrapper, COVEN_HOME=str(self.home))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), [str(self.home), str(self.wrapper)])

    def test_public_identity_precedence_and_empty_policy(self):
        public = "/future/Arcana Public/coven"
        explicit = "/custom/coven"
        for overrides, expected in (
            ({"ARCANA_PUBLIC_COVEN": public}, public),
            ({"ARCANA_PUBLIC_COVEN": public, "COVEN_COMMAND": explicit}, explicit),
            ({"ARCANA_PUBLIC_COVEN": public, "COVEN_COMMAND": ""}, public),
            ({"ARCANA_PUBLIC_COVEN": "", "COVEN_COMMAND": ""}, str(self.wrapper)),
            ({"ARCANA_PUBLIC_COMMAND": "/wrong/numen", "ARCANA_PUBLIC_NUMEN": "/wrong/numen"},
             str(self.wrapper)),
        ):
            with self.subTest(overrides=overrides):
                result = self.run_wrapper(self.wrapper, **overrides)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout)[1], expected)

    def test_cycles_and_excessive_chains_fail_boundedly(self):
        entry = self.root / "entry"
        cycle = self.root / "cycle"
        cycle.symlink_to("entry")
        for index in range(41):
            (self.root / f"link-{index}").symlink_to(
                f"link-{index + 1}" if index < 40 else self.wrapper)
        for target, message in (("cycle", "symlink cycle"), ("entry", "symlink cycle"),
                                ("link-0", "exceeds 40 links")):
            with self.subTest(target=target):
                entry.unlink(missing_ok=True)
                entry.symlink_to(self.wrapper)
                # A preexisting executable cycle cannot be loaded by the OS.
                # Introduce the real cycle/long chain after Bash has loaded it.
                result = subprocess.run(
                    ["/bin/bash", "-c", 'entry="$1"; target="$2"; '
                     'readlink() { if [[ "$1" == "$entry" ]]; then '
                     'command ln -sf "$target" "$entry"; fi; command readlink "$1"; }; '
                     'source "$entry"', "test", str(entry), target],
                    env=self.env, text=True, capture_output=True, timeout=10)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(message, result.stderr)

    def test_stable_callbacks_and_direct_runner_flat_root_fallback(self):
        public = "/future/Arcana Public/../bin/coven"
        workspace = self.root / "workspace"
        env = self.env | {"ARCANA_PUBLIC_COVEN": public}
        for args in (["init", str(workspace), "--blank"], ["refresh", str(workspace)]):
            result = subprocess.run([str(ROOT / "bin/coven"), *args], env=env,
                                    text=True, capture_output=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            manifest = tomllib.loads((workspace / "tmux/coven.toml").read_text())
            panes = manifest["sessions"][0]["windows"][0]["panes"]
            events = next(pane["command"] for pane in panes if pane["title"] == "events")
            self.assertIn(shlex.quote(public) + " events", events)
            self.assertNotIn(str(ROOT / "bin/coven"), events)
        with mock.patch.dict(os.environ, self.env, clear=True):
            self.assertEqual(coven.cli_command(), shlex.quote(str(ROOT / "bin/coven")))
        with mock.patch.dict(os.environ, {"COVEN_COMMAND": public}):
            self.assertEqual(coven.cli_command(), shlex.quote(public))


if __name__ == "__main__":
    unittest.main()
