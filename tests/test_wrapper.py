"""Offline installation-layout and public callback identity regressions."""

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
        self.wrapper = self.home / "bin/coven"
        shutil.copy2(ROOT / "bin/coven", self.wrapper)
        native = Path(os.environ.get("COVEN_NATIVE_BIN", ROOT / "target/debug/coven"))
        if not native.is_absolute() or not native.is_file() or not os.access(native, os.X_OK):
            self.fail("Prerequisite: build the native Rust binary first or set absolute COVEN_NATIVE_BIN")
        self.native = self.home / "target/release/coven"
        self.native.parent.mkdir(parents=True)
        shutil.copy2(native, self.native)
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(("COVEN_", "ARCANA_PUBLIC_"))}

    def run_wrapper(self, path, *args, **env):
        return subprocess.run([str(path), *args], env=self.env | env,
                              text=True, capture_output=True, timeout=10)

    def assert_callbacks(self, path, overrides, expected):
        workspace = self.root / "workspace"
        for args in (["init", str(workspace), "--blank"], ["refresh", str(workspace)]):
            result = self.run_wrapper(path, *args, **overrides)
            self.assertEqual(result.returncode, 0, result.stderr)
            manifest = tomllib.loads((workspace / "tmux/coven.toml").read_text())
            panes = manifest["sessions"][0]["windows"][0]["panes"]
            for title in ("events", "messages"):
                command = next(pane["command"] for pane in panes if pane["title"] == title)
                self.assertIn(shlex.quote(expected) + " " + title + ";", command)
        shutil.rmtree(workspace)

    def test_relative_and_absolute_symlink_chain_finds_full_tree(self):
        relative = self.root / "relative"
        relative.symlink_to("isolated tree/bin/coven")
        absolute = self.root / "absolute"
        absolute.symlink_to(relative)
        for path in (self.wrapper, relative, absolute):
            with self.subTest(path=path):
                result = self.run_wrapper(path, "--help", PATH="")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("coven", result.stdout)

    def test_explicit_home_bypasses_discovery(self):
        elsewhere = self.root / "elsewhere"
        elsewhere.mkdir()
        wrapper = elsewhere / "coven"
        shutil.copy2(self.wrapper, wrapper)
        assets = self.home / "examples/custom"
        assets.mkdir(parents=True)
        (assets / "example.json").write_text('{"id":"custom"}')
        (assets / "goal.md").write_text("asset home goal\n")
        workspace = self.root / "workspace"
        result = self.run_wrapper(wrapper, "init", str(workspace), "--example", "custom",
                                  COVEN_HOME=str(self.home), COVEN_NATIVE_BIN=str(self.native))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((workspace / "goal.md").read_text(), "asset home goal\n")
        result = self.run_wrapper(wrapper, "--help", COVEN_HOME=str(self.home))
        self.assertNotEqual(result.returncode, 0)

    def test_public_identity_precedence_and_empty_policy(self):
        public = "/future/Arcana Public/../bin/co'ven"
        explicit = "/custom/../chosen coven"
        for overrides, expected in (
            ({"ARCANA_PUBLIC_COVEN": public}, public),
            ({"ARCANA_PUBLIC_COVEN": public, "COVEN_COMMAND": explicit}, explicit),
            ({"ARCANA_PUBLIC_COVEN": public, "COVEN_COMMAND": ""}, public),
            ({"ARCANA_PUBLIC_COVEN": "", "COVEN_COMMAND": ""}, "coven"),
            ({"COVEN_COMMAND": ""}, "coven"),
            ({"ARCANA_PUBLIC_COVEN": ""}, "coven"),
            ({}, "coven"),
            ({"ARCANA_PUBLIC_COMMAND": "/wrong/numen", "ARCANA_PUBLIC_NUMEN": "/wrong/numen"},
             "coven"),
        ):
            with self.subTest(overrides=overrides):
                for path in (self.wrapper, self.native):
                    self.assert_callbacks(path, overrides, expected)

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
                     'source "$entry"', str(entry), str(entry), target],
                    env=self.env, text=True, capture_output=True, timeout=10)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(message, result.stderr)

    def test_stable_callbacks_and_direct_runner_flat_root_fallback(self):
        public = "/future/Arcana Public/../bin/coven"
        self.assert_callbacks(self.native, {"ARCANA_PUBLIC_COVEN": public}, public)
        with mock.patch.dict(os.environ, self.env, clear=True):
            self.assertEqual(coven.cli_command(), shlex.quote(str(ROOT / "bin/coven")))
        with mock.patch.dict(os.environ, {"COVEN_COMMAND": public}):
            self.assertEqual(coven.cli_command(), shlex.quote(public))


if __name__ == "__main__":
    unittest.main()
