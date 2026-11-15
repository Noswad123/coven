"""Fast checkout installer tests; no Cargo compilation or installed selection writes."""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("install_dev", ROOT / "scripts/install_dev.py")
dev = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(dev)


class DevInstallTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="coven-dev-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.checkout = self.root / "checkout space"
        (self.checkout / "examples/custom").mkdir(parents=True)
        (self.checkout / "examples/custom/goal.md").write_text("first assets")
        self.parent = self.root / "install space"
        self.parent.mkdir()
        self.prefix = self.parent / "coven"
        self.binary = self.root / "cargo target space/release/coven"
        self.binary.parent.mkdir(parents=True)
        shutil.copyfile(shutil.which("true"), self.binary)
        self.binary.chmod(0o755)
        self.patch_root = patch.object(dev, "ROOT", self.checkout)
        self.patch_root.start()
        self.addCleanup(self.patch_root.stop)
        self.tools = self.root / "tools"
        self.tools.mkdir()
        cargo = self.tools / "cargo"
        cargo.write_text(f"#!{sys.executable}\n" + '''import json, os, pathlib, subprocess, sys, time
pathlib.Path(os.environ['RECORD']).write_text(json.dumps({
    'pid': os.getpid(), 'args': sys.argv[1:], 'cwd': os.getcwd(), 'target': os.environ.get('CARGO_TARGET_DIR'),
    'auto': os.environ.get('RUSTUP_AUTO_INSTALL')}))
if os.environ.get('DESCENDANT'):
    code = "import os, pathlib, signal, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); pathlib.Path(os.environ['DESCENDANT']).write_text(str(os.getpid())); time.sleep(60)"
    subprocess.Popen([sys.executable, '-c', code], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    while not pathlib.Path(os.environ['DESCENDANT']).exists(): time.sleep(0.01)
if os.environ.get('BLOCK'):
    pathlib.Path(os.environ['BLOCK']).write_text(str(os.getpid()))
    while True: time.sleep(1)
print(json.dumps({'reason': 'compiler-message', 'message': {'rendered': 'rendered diagnostic\\n'}}))
print(json.dumps({'reason': 'compiler-artifact', 'target': {'name': 'other', 'kind': ['bin']}, 'executable': '/stale/other'}))
if not os.environ.get('NO_ARTIFACT'):
    print(json.dumps({'reason': 'compiler-artifact', 'target': {'name': 'coven', 'kind': ['bin']}, 'executable': os.environ['ARTIFACT']}))
sys.exit(int(os.environ.get('FAIL', '0')))
''')
        cargo.chmod(0o755)
        self.env = os.environ.copy()
        for key in ("INSTALL_PREFIX", "HOME", "BLOCK", "FAIL", "NO_ARTIFACT", "DESCENDANT"):
            self.env.pop(key, None)
        self.env.update(PATH=str(self.tools) + os.pathsep + self.env["PATH"],
                        HOME=str(self.root / "home"), INSTALL_PREFIX=str(self.prefix),
                        ARTIFACT=str(self.binary), RECORD=str(self.root / "record"),
                        CARGO_TARGET_DIR="custom target with spaces", PYTHONDONTWRITEBYTECODE="1")

    def invoke(self, **extra):
        env = dict(self.env, **extra)
        # ROOT is the actual checkout for subprocess builds, not the fixture checkout.
        return subprocess.run([sys.executable, str(ROOT / "scripts/install_dev.py")],
                              env=env, cwd=self.root, capture_output=True, text=True)

    def snapshot(self, root):
        return {str(p.relative_to(root)): (p.read_bytes(), stat.S_IMODE(p.stat().st_mode))
                for p in root.rglob("*") if p.is_file()}

    def assert_clean_parent(self):
        self.assertEqual([p for p in self.parent.iterdir()
                          if p.name != ".coven.install.lock"], [])

    def test_build_actual_checkout_locked_release_reported_path_and_diagnostics(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        record = json.loads((self.root / "record").read_text())
        self.assertEqual(record["cwd"], str(ROOT))
        self.assertEqual(record["args"], ["build", "--locked", "--release", "--bin", "coven",
                                         "--message-format=json-render-diagnostics"])
        self.assertEqual(record["target"], "custom target with spaces")
        self.assertEqual(record["auto"], "0")
        self.assertIn("rendered diagnostic", result.stderr)
        self.assertEqual((self.prefix / "bin/coven").read_bytes(), self.binary.read_bytes())
        self.assertIn(str(self.prefix.resolve()), result.stdout)

    def test_relative_reported_artifact(self):
        relative = os.path.relpath(self.binary, ROOT)
        self.assertEqual(self.invoke(ARTIFACT=relative).returncode, 0)

    def test_default_prefix(self):
        env = self.env.copy()
        env.pop("INSTALL_PREFIX")
        # Keep default creation under an existing trusted HOME.
        Path(env["HOME"]).mkdir()
        with patch.dict(os.environ, env, clear=True):
            prefix = dev.install_prefix()
        self.assertEqual(prefix, Path(env["HOME"]) / ".local/libexec/arcana-dev/coven")
        result = subprocess.run([sys.executable, str(ROOT / "scripts/install_dev.py")],
                                env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(prefix.is_symlink())

    def test_repeat_keeps_canonical_binary_assets_and_unrelated_files(self):
        unrelated = self.parent / "unrelated"
        unrelated.write_text("keep")
        dev.install(self.prefix, self.binary, None)
        old = self.prefix.resolve()
        before = self.snapshot(old)
        (self.checkout / "examples/custom/goal.md").write_text("second assets")
        dev.install(self.prefix, self.binary, dev.selection(self.prefix))
        self.assertNotEqual(old, self.prefix.resolve())
        self.assertEqual(self.snapshot(old), before)
        self.assertEqual((old / "share/coven/examples/custom/goal.md").read_text(), "first assets")
        self.assertEqual((self.prefix / "share/coven/examples/custom/goal.md").read_text(), "second assets")
        self.assertEqual(unrelated.read_text(), "keep")
        self.assertEqual({p.name for p in self.prefix.iterdir()}, {"bin", "share"})
        self.assertEqual(stat.S_IMODE((self.prefix / "bin/coven").stat().st_mode), 0o755)

    def test_legacy_non_sibling_native_link(self):
        legacy = self.root / "legacy-native"
        (legacy / "bin").mkdir(parents=True)
        shutil.copy2(self.binary, legacy / "bin/coven")
        (legacy / "share/coven/examples").mkdir(parents=True)
        (legacy / "share/coven/examples/old").write_text("unchanged")
        self.prefix.symlink_to("../legacy-native")
        before = self.snapshot(legacy)
        dev.install(self.prefix, self.binary, dev.selection(self.prefix))
        self.assertEqual(self.snapshot(legacy), before)
        self.assertNotEqual(self.prefix.resolve(), legacy)

    def test_refuse_existing_real_directory(self):
        self.prefix.mkdir()
        (self.prefix / "keep").write_text("keep")
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("manual handoff", result.stderr)
        self.assertEqual((self.prefix / "keep").read_text(), "keep")
        self.assertFalse((self.root / "record").exists())

    def test_refuse_broken_unrelated_and_wrapper_links(self):
        for kind in ("broken", "no-assets", "wrapper"):
            with self.subTest(kind=kind):
                target = self.root / kind
                if kind != "broken":
                    (target / "bin").mkdir(parents=True)
                    shutil.copy2(self.binary, target / "bin/coven")
                if kind == "wrapper":
                    (target / "share/coven/examples").mkdir(parents=True)
                    (target / "bin/coven").write_text("#!/bin/sh\nexit 0\n")
                self.prefix.symlink_to(target)
                with self.assertRaisesRegex(RuntimeError, "INSTALL_PREFIX"):
                    dev.selection(self.prefix)
                self.assertEqual(os.readlink(self.prefix), str(target))
                self.prefix.unlink()

    def test_invalid_prefixes_and_missing_home(self):
        for value in ("", "relative", str(self.parent / "../oops"), "/"):
            with self.subTest(value=value), patch.dict(os.environ, {"INSTALL_PREFIX": value}):
                with self.assertRaisesRegex(RuntimeError, "absolute"):
                    dev.install_prefix()
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(RuntimeError, "HOME"):
                dev.install_prefix()

    def test_reject_unsafe_parent_before_writes(self):
        link = self.root / "parent-link"
        link.symlink_to(self.parent)
        for prefix in (link / "coven", self.parent / "missing/coven"):
            with patch.dict(os.environ, {"INSTALL_PREFIX": str(prefix)}):
                with self.assertRaises(RuntimeError):
                    dev.install_prefix()
        self.parent.chmod(0o777)
        with patch.dict(os.environ, {"INSTALL_PREFIX": str(self.prefix)}):
            with self.assertRaisesRegex(RuntimeError, "trusted"):
                dev.install_prefix()
        self.parent.chmod(0o755)
        self.assert_clean_parent()

    def test_build_failure_preserves_generation_and_exit_code(self):
        dev.install(self.prefix, self.binary, None)
        old = self.prefix.resolve()
        before = self.snapshot(old)
        result = self.invoke(FAIL="17")
        self.assertEqual(result.returncode, 17)
        self.assertIn("rendered diagnostic", result.stderr)
        self.assertEqual(self.prefix.resolve(), old)
        self.assertEqual(self.snapshot(old), before)

    def test_missing_artifact_never_uses_stale_binary(self):
        result = self.invoke(NO_ARTIFACT="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("without reporting", result.stderr)
        self.assert_clean_parent()

    def test_stage_and_publication_failures_preserve_old(self):
        dev.install(self.prefix, self.binary, None)
        old = self.prefix.resolve()
        before = self.snapshot(old)
        entries = set(self.parent.iterdir())
        for operation in ("copyfile", "copytree", "replace"):
            with self.subTest(operation=operation):
                module = dev.os if operation == "replace" else dev.shutil
                with patch.object(module, operation, side_effect=OSError("injected failure")):
                    with self.assertRaises(OSError):
                        dev.install(self.prefix, self.binary, dev.selection(self.prefix))
                self.assertEqual(self.prefix.resolve(), old)
                self.assertEqual(self.snapshot(old), before)
                self.assertEqual(set(self.parent.iterdir()), entries)

    def test_no_artifact_or_partial_payload_on_non_native_build_output(self):
        self.binary.write_text("#!/bin/sh\nexit 0\n")
        with self.assertRaisesRegex(RuntimeError, "native executable"):
            dev.install(self.prefix, self.binary, None)
        self.assert_clean_parent()

    def test_concurrent_selection_change_refused(self):
        dev.install(self.prefix, self.binary, None)
        with self.assertRaisesRegex(RuntimeError, "changed concurrently"):
            dev.install(self.prefix, self.binary, None)
        self.assertEqual(len(list(self.parent.iterdir())), 3)

    def test_unmanaged_entry_after_revalidation_is_not_clobbered(self):
        marker = self.root / "publication-ready"
        code = f'''import importlib.util, pathlib, sys
s = importlib.util.spec_from_file_location('dev', {str(ROOT / 'scripts/install_dev.py')!r})
d = importlib.util.module_from_spec(s); s.loader.exec_module(d)
symlink = d.os.symlink
def barrier(*args, **kwargs):
    pathlib.Path({str(marker)!r}).touch()
    assert sys.stdin.readline() == 'publish\\n'
    return symlink(*args, **kwargs)
d.os.symlink = barrier
sys.exit(d.main())
'''
        process = subprocess.Popen([sys.executable, "-c", code], env=self.env,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True)
        try:
            self.wait_marker(process, marker)
            self.prefix.write_bytes(b"unmanaged concurrent data")
            _, stderr = process.communicate("publish\n", timeout=10)
            self.assertEqual(process.returncode, 1, stderr)
            self.assertFalse(self.prefix.is_symlink())
            self.assertEqual(self.prefix.read_bytes(), b"unmanaged concurrent data")
            self.assertEqual({p.name for p in self.parent.iterdir()},
                             {"coven", ".coven.install.lock"})
        finally:
            if process.poll() is None:
                process.kill()
            process.communicate()

    def test_cooperating_publishers_serialize_and_reject_stale_snapshot(self):
        ready = self.root / "a-ready"
        waiting = self.root / "b-waiting"
        validated = self.root / "b-validated"
        code = f'''import importlib.util, os, pathlib, sys
s = importlib.util.spec_from_file_location('dev', {str(ROOT / 'scripts/install_dev.py')!r})
d = importlib.util.module_from_spec(s); s.loader.exec_module(d)
if os.environ['ROLE'] == 'a':
    symlink = d.os.symlink
    def barrier(*args, **kwargs):
        pathlib.Path({str(ready)!r}).touch()
        assert sys.stdin.readline() == 'publish\\n'
        return symlink(*args, **kwargs)
    d.os.symlink = barrier
else:
    flock = d.fcntl.flock
    def lock(*args):
        pathlib.Path({str(waiting)!r}).touch()
        result = flock(*args)
        pathlib.Path({str(validated)!r}).touch()
        return result
    d.fcntl.flock = lock
sys.exit(d.main())
'''
        processes = []
        try:
            for role in ("a", "b"):
                process = subprocess.Popen([sys.executable, "-c", code],
                                           env=dict(self.env, ROLE=role), stdin=subprocess.PIPE,
                                           stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                processes.append(process)
                self.wait_marker(process, ready if role == "a" else waiting)
            self.assertFalse(validated.exists(), "B acquired the lock while A was publishing")
            _, stderr = processes[0].communicate("publish\n", timeout=10)
            self.assertEqual(processes[0].returncode, 0, stderr)
            selected = self.prefix.resolve()
            before = self.snapshot(selected)
            _, stderr = processes[1].communicate(timeout=10)
            self.assertEqual(processes[1].returncode, 1, stderr)
            self.assertIn("changed concurrently", stderr)
            self.assertTrue(validated.exists())
            self.assertEqual(self.prefix.resolve(), selected)
            self.assertEqual(self.snapshot(selected), before)
            self.assertEqual({p.name for p in self.parent.iterdir()},
                             {"coven", ".coven.install.lock", selected.name})
        finally:
            for process in processes:
                if process.poll() is None:
                    process.kill()
                process.communicate()

    def test_unsafe_lock_entries_are_refused_without_adoption(self):
        lock = self.parent / ".coven.install.lock"
        target = self.root / "lock-target"
        target.write_bytes(b"keep lock target")
        for kind in ("symlink", "fifo", "writable", "hardlink"):
            with self.subTest(kind=kind):
                if kind == "symlink":
                    lock.symlink_to(target)
                elif kind == "fifo":
                    os.mkfifo(lock)
                elif kind == "hardlink":
                    os.link(target, lock)
                else:
                    lock.write_bytes(b"unsafe")
                    lock.chmod(0o666)
                with self.assertRaises((OSError, RuntimeError)):
                    dev.install(self.prefix, self.binary, None)
                self.assertEqual(target.read_bytes(), b"keep lock target")
                self.assertFalse(self.prefix.exists())
                self.assertEqual(list(self.parent.iterdir()), [lock])
                lock.unlink()

    def wait_marker(self, process, marker):
        deadline = time.monotonic() + 10
        while not marker.exists() and process.poll() is None and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertTrue(marker.exists(), "child did not reach blocking step")

    def test_signals_stop_and_reap_blocked_cargo(self):
        for sig in (signal.SIGINT, signal.SIGTERM):
            with self.subTest(sig=sig):
                marker = self.root / f"blocked-{sig}"
                process = subprocess.Popen([sys.executable, str(ROOT / "scripts/install_dev.py")],
                                           env=dict(self.env, BLOCK=str(marker)),
                                           stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                try:
                    self.wait_marker(process, marker)
                    pid = int(marker.read_text())
                    process.send_signal(sig)
                    process.communicate(timeout=10)
                    self.assertEqual(process.returncode, 128 + sig)
                    with self.assertRaises(ProcessLookupError):
                        os.kill(pid, 0)
                    self.assert_clean_parent()
                finally:
                    if process.poll() is None:
                        process.kill()
                    process.communicate()

    @unittest.skipUnless(sys.platform == "linux", "uses Linux subreaper to reap killed grandchildren")
    def test_first_signal_during_normal_child_cleanup_kills_descendant_and_reaps_child(self):
        descendant = self.root / "descendant"
        code = f'''import ctypes, importlib.util, json, os, pathlib, signal, sys, time
# Adopt the fixture grandchild so the test can reap it instead of leaving a zombie.
assert ctypes.CDLL(None, use_errno=True).prctl(36, 1, 0, 0, 0) == 0
s = importlib.util.spec_from_file_location('dev', {str(ROOT / 'scripts/install_dev.py')!r})
d = importlib.util.module_from_spec(s); s.loader.exec_module(d)
killpg = d.os.killpg
sent = False
def first_signal(pid, sig):
    global sent
    if not sent:
        sent = True
        assert sig == signal.SIGTERM
        os.kill(os.getpid(), signal.SIGTERM)
    killpg(pid, sig)
d.os.killpg = first_signal
try:
    try:
        d.main()
    except SystemExit as error:
        assert error.code == 143, error.code
    else:
        raise AssertionError('pending SIGTERM was not delivered')
    leader = json.loads(pathlib.Path(os.environ['RECORD']).read_text())['pid']
    try:
        os.waitpid(leader, os.WNOHANG)
    except ChildProcessError:
        pass
    else:
        raise AssertionError('owned Cargo child was not reaped')
    pid = int(pathlib.Path(os.environ['DESCENDANT']).read_text())
    deadline = time.monotonic() + 5
    while True:
        reaped, status = os.waitpid(pid, os.WNOHANG)
        if reaped:
            assert os.WIFSIGNALED(status) and os.WTERMSIG(status) == signal.SIGKILL, status
            break
        assert time.monotonic() < deadline, 'descendant survived cleanup'
        time.sleep(0.01)
finally:
    if pathlib.Path(os.environ['DESCENDANT']).exists():
        pid = int(pathlib.Path(os.environ['DESCENDANT']).read_text())
        try:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        except ProcessLookupError:
            pass
'''
        result = subprocess.run([sys.executable, "-c", code],
                                env=dict(self.env, DESCENDANT=str(descendant)),
                                capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_clean_parent()

    def test_signal_during_copy_cleans_unpublished_payload(self):
        marker = self.root / "copy-blocked"
        code = f'''import importlib.util, pathlib, sys, time
s = importlib.util.spec_from_file_location('dev', {str(ROOT / 'scripts/install_dev.py')!r})
d = importlib.util.module_from_spec(s); s.loader.exec_module(d)
def blocked(*args, **kwargs):
    pathlib.Path({str(marker)!r}).touch()
    while True: time.sleep(1)
d.shutil.copytree = blocked
sys.exit(d.main())
'''
        process = subprocess.Popen([sys.executable, "-c", code], env=self.env,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            self.wait_marker(process, marker)
            process.terminate()
            process.communicate(timeout=10)
            self.assertEqual(process.returncode, 143)
            self.assert_clean_parent()
        finally:
            if process.poll() is None:
                process.kill()
            process.communicate()

    def test_signal_after_atomic_replace_retains_published_payload(self):
        dev.install(self.prefix, self.binary, None)
        # Deliver in the rename/flag window: blocked signals must wait for ownership transfer.
        code = f'''import importlib.util, os, signal, sys
s = importlib.util.spec_from_file_location('dev', {str(ROOT / 'scripts/install_dev.py')!r})
d = importlib.util.module_from_spec(s); s.loader.exec_module(d)
replace = os.replace
def publish(*args):
    replace(*args)
    os.kill(os.getpid(), signal.SIGTERM)
d.os.replace = publish
sys.exit(d.main())
'''
        result = subprocess.run([sys.executable, "-c", code], env=self.env, capture_output=True)
        self.assertEqual(result.returncode, 143, result.stderr)
        self.assertTrue((self.prefix / "bin/coven").is_file())
        self.assertTrue((self.prefix / "share/coven/examples").is_dir())
        self.assertFalse(any(p.name.endswith(".select") for p in self.parent.iterdir()))
        with dev.publication_lock(self.prefix):
            pass  # Signal-driven unwinding released the retained lock inode.


if __name__ == "__main__":
    unittest.main()
