"""Focused storage contracts; no agent runtimes or external services."""

import hashlib
from dataclasses import replace
import json
import os
from pathlib import Path
import select
import stat
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from runner import storage


class StorageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.path = self.root / "tasks.json"
        self.lock = self.root / "locks" / "workspace.lock"

    def test_exact_revisions_and_validation(self):
        self.assertEqual(storage.snapshot_revision(None), "tasks-missing:v1")
        for data in (b"", b"[]", b"[]\n", b"[ ]\n", b"\xff"):
            token = storage.snapshot_revision(data)
            self.assertEqual(token, "tasks-sha256:" + hashlib.sha256(data).hexdigest())
            storage.validate_revision(token)
        storage.validate_revision(None)
        storage.validate_revision("tasks-missing:v1")
        for invalid in ("", 1, False, "tasks-missing:v2", "1", "tasks-sha256:" + "A" * 64,
                        "tasks-sha256:" + "a" * 63, "tasks-missing:v1\n"):
            with self.subTest(token=invalid), self.assertRaises(storage.StorageError):
                storage.validate_revision(invalid)

    def test_transport_gate_shared_admission_and_exclusive_activation(self):
        gate_path = self.root / ".runtime-transport.lock"
        with storage.transport_gate(self.root, gate_path) as shared:
            with storage.transport_gate(self.root, gate_path, held=shared) as nested:
                self.assertIs(nested, shared)
            with self.assertRaises(storage.StorageError), storage.transport_gate(self.root, gate_path, held=shared, exclusive=True):
                pass
            with self.assertRaises(storage.LockBusy), storage.transport_gate(self.root, gate_path, exclusive=True, timeout=0):
                pass
            with storage.workspace_lock(self.root, self.lock):
                pass  # The transport gate is independent from workspace state.
        with storage.transport_gate(self.root, gate_path, exclusive=True) as exclusive:
            self.assertTrue(exclusive.exclusive)
            self.assertFalse(os.get_inheritable(exclusive.fd))
            with self.assertRaises(storage.LockBusy), storage.transport_gate(self.root, gate_path, timeout=0):
                pass
        with self.assertRaises(storage.StorageError), storage.transport_gate(self.root, gate_path, held=exclusive):
            pass
        self.assertTrue(gate_path.exists())

    def test_transport_gate_nesting_exceptions_aliases_and_invalid_owners(self):
        path = self.root / ".runtime-transport.lock"
        alias = self.root / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        with storage.transport_gate(self.root, path) as outer:
            with self.assertRaisesRegex(ValueError, "nested failure"):
                with storage.transport_gate(alias, alias / path.name, held=outer):
                    raise ValueError("nested failure")
            self.assertTrue(outer.active)
            with self.assertRaises(storage.LockBusy), storage.transport_gate(self.root, path, exclusive=True, timeout=0):
                pass
            for token in (replace(outer, active=False), replace(outer, owner_pid=-1),
                          replace(outer, owner_thread_id=-1), replace(outer, path=self.root / "other")):
                with self.assertRaises(storage.StorageError), storage.transport_gate(self.root, path, held=token):
                    pass
            errors = []
            def wrong_thread():
                try:
                    with storage.transport_gate(self.root, path, held=outer):
                        pass
                except storage.StorageError:
                    errors.append(True)
            thread = threading.Thread(target=wrong_thread)
            thread.start()
            thread.join(timeout=5)
            self.assertEqual(errors, [True])
        with storage.transport_gate(self.root, path, exclusive=True):
            pass

    def test_json_encoding(self):
        self.assertEqual(storage.encode_json({"a": [1]}), b'{\n  "a": [\n    1\n  ]\n}\n')
        self.assertEqual(json.loads(storage.encode_json({"text": "\u2603"})), {"text": "\u2603"})
        for value in (float("nan"), float("inf"), {"bad": object()}, "\ud800"):
            with self.subTest(value=repr(value)), self.assertRaises(storage.StorageError):
                storage.encode_json(value)

    def test_missing_reads_do_not_create_anything(self):
        path = self.root / "absent" / "events.jsonl"
        self.assertEqual(storage.read_jsonl(path), [])
        self.assertEqual(list(self.root.iterdir()), [])

    def test_only_lock_layer_creates_parents(self):
        path = self.root / "absent" / "file"
        for operation in (lambda: storage.atomic_replace_bytes(path, b"{}\n"),
                          lambda: storage.append_jsonl(path, {})):
            with self.assertRaises(storage.StorageError):
                operation()
            self.assertFalse(path.parent.exists())
        with storage.workspace_lock(self.root, path):
            self.assertTrue(path.is_file())

    def test_jsonl_valid_legacy_and_append_without_rewrite(self):
        prefix = b' \n\t\r\n\r {"old": 1}\r\n\n'
        self.path.write_bytes(prefix)
        inode = self.path.stat().st_ino
        with storage.workspace_lock(self.root, self.lock):
            storage.append_jsonl(self.path, {"new": "\u2603"})
        self.assertEqual(storage.read_jsonl(self.path), [{"old": 1}, {"new": "\u2603"}])
        self.assertTrue(self.path.read_bytes().startswith(prefix))
        self.assertEqual(self.path.stat().st_ino, inode)

    def test_jsonl_rejects_corruption_with_line_and_repair_diagnostic(self):
        for bad in (b'{bad}\n', b'[]\n', b'null\n', b'{"x":NaN}\n',
                    b'{"x":Infinity}\n', b'{"x":-Infinity}\n', b'{"x":1e999}\n',
                    b'{"x":"\xff"}\n', b'{"valid":true}', b'  ', b'{"x":1}\r'):
            data = b'{"first":1}\n' + bad
            with self.subTest(data=data):
                self.path.write_bytes(data)
                with self.assertRaises(storage.StorageError) as caught:
                    storage.read_jsonl(self.path)
                self.assertIn(str(self.path), str(caught.exception))
                self.assertIn("line 2", str(caught.exception))
                self.assertIn("inspect/repair", str(caught.exception))
                with self.assertRaises(storage.StorageError):
                    storage.append_jsonl(self.path, {"next": 1})
                self.assertEqual(self.path.read_bytes(), data)
        self.path.write_bytes(b'{}\nBROKEN\n{}\n')
        with self.assertRaisesRegex(storage.StorageError, "line 2"):
            storage.read_jsonl(self.path)

    def test_append_serializes_before_open_and_reports_sync_failure(self):
        for value in ({"bad": float("nan")}, {"bad": object()}, {"bad": "\ud800"}, []):
            with self.subTest(value=repr(value)), self.assertRaises(storage.StorageError):
                storage.append_jsonl(self.path, value)
            self.assertFalse(self.path.exists())
        with patch.object(storage.os, "fsync", side_effect=OSError("sync failed")):
            with self.assertRaisesRegex(storage.StorageError, "append may be partial.*inspect/repair"):
                storage.append_jsonl(self.path, {"new": 1})
        self.assertEqual(storage.read_jsonl(self.path), [{"new": 1}])

    def test_atomic_replace_new_and_preserves_existing_mode(self):
        storage.atomic_replace_bytes(self.path, storage.encode_json({"new": 1}))
        self.assertEqual(stat.S_IMODE(self.path.stat().st_mode), 0o600)
        self.path.chmod(0o640)
        storage.atomic_replace_bytes(self.path, storage.encode_json({"next": 2}))
        self.assertEqual(stat.S_IMODE(self.path.stat().st_mode), 0o640)
        self.assertEqual(json.loads(self.path.read_bytes()), {"next": 2})
        self.assertEqual(list(self.root.iterdir()), [self.path])

    def test_atomic_failures_before_replace_leave_complete_old_file_and_clean_temp(self):
        old = storage.encode_json({"old": list(range(100))})
        new = storage.encode_json({"new": list(range(100))})
        self.path.write_bytes(old)
        for operation in ("fsync", "replace", "fchmod"):
            with self.subTest(operation=operation):
                with patch.object(storage.os, operation, side_effect=OSError("injected")):
                    with self.assertRaises(storage.StorageError) as caught:
                        storage.atomic_replace_bytes(self.path, new)
                self.assertNotIsInstance(caught.exception, storage.CommitUncertain)
                self.assertEqual(self.path.read_bytes(), old)
                self.assertEqual(list(self.root.iterdir()), [self.path])

    def test_atomic_directory_sync_failure_is_uncertain_after_replace(self):
        self.path.write_bytes(b'{"old":true}\n')
        new = storage.encode_json({"new": True})
        with patch.object(storage, "sync_directory", side_effect=OSError("directory sync")) as sync:
            with self.assertRaises(storage.CommitUncertain) as caught:
                storage.atomic_replace_bytes(self.path, new)
        sync.assert_called_once_with(self.root)
        self.assertEqual(caught.exception.path, self.path)
        self.assertIn("snapshot replaced", str(caught.exception))
        self.assertIn("durability uncertain", str(caught.exception))
        self.assertIn("inspect before retry", str(caught.exception))
        self.assertEqual(self.path.read_bytes(), new)
        self.assertEqual(list(self.root.iterdir()), [self.path])

    def test_failed_temp_cleanup_does_not_mask_original_precommit_error(self):
        old = b'{"old":true}\n'
        self.path.write_bytes(old)
        with patch.object(storage.os, "replace", side_effect=OSError("original replace failure")), \
                patch.object(Path, "unlink", side_effect=OSError("cleanup failed")):
            with self.assertRaisesRegex(storage.StorageError, "original replace failure"):
                storage.atomic_replace_bytes(self.path, b'{"new":true}\n')
        self.assertEqual(self.path.read_bytes(), old)
        self.assertTrue(list(self.root.glob(".tasks.json.*.tmp")))

    def test_replacement_is_whole_and_does_not_promote_orphans(self):
        old, new = b'{"old":true}\n', b'{"new":true}\n'
        orphan = self.root / ".tasks.json.orphan.tmp"
        orphan.write_bytes(b"incomplete")
        self.path.write_bytes(old)
        replace = os.replace

        def inspect_replace(source, target):
            self.assertEqual(self.path.read_bytes(), old)
            self.assertEqual(Path(source).parent, self.path.parent)
            self.assertEqual(Path(source).read_bytes(), new)
            replace(source, target)
            self.assertEqual(self.path.read_bytes(), new)

        with patch.object(storage.os, "replace", side_effect=inspect_replace):
            storage.atomic_replace_bytes(self.path, new)
        self.assertEqual(orphan.read_bytes(), b"incomplete")

    def test_static_symlink_and_nonregular_leaf_rejection(self):
        target = self.root / "real"
        target.write_bytes(b'{}\n')
        for kind in ("symlink", "directory", "fifo"):
            with self.subTest(kind=kind):
                if kind == "symlink":
                    self.path.symlink_to(target)
                elif kind == "directory":
                    self.path.mkdir()
                else:
                    os.mkfifo(self.path)
                try:
                    for operation in (lambda: storage.atomic_replace_bytes(self.path, b"{}\n"),
                                      lambda: storage.append_jsonl(self.path, {}),
                                      lambda: storage.read_jsonl(self.path)):
                        with self.assertRaises(storage.StorageError):
                            operation()
                    with self.assertRaises(storage.StorageError):
                        with storage.workspace_lock(self.root, self.path):
                            self.fail("unsafe lock accepted")
                finally:
                    if kind == "directory":
                        self.path.rmdir()
                    else:
                        self.path.unlink()
        self.assertEqual(target.read_bytes(), b'{}\n')

    def test_parent_symlinks_and_outside_lock_rejected(self):
        real = self.root / "real"
        real.mkdir()
        alias = self.root / "alias"
        alias.symlink_to(real, target_is_directory=True)
        path = alias / "file"
        for operation in (lambda: storage.atomic_replace_bytes(path, b"{}\n"),
                          lambda: storage.append_jsonl(path, {}),
                          lambda: storage.read_jsonl(path)):
            with self.assertRaises(storage.StorageError):
                operation()
        for path in (alias / "nested" / "lock", self.root.parent / "outside.lock",
                     self.root / ".." / "outside.lock", self.root):
            with self.subTest(path=path), self.assertRaises(storage.StorageError):
                with storage.workspace_lock(self.root, path):
                    self.fail("unsafe lock accepted")
        self.assertEqual(list(real.iterdir()), [])

    def test_lock_persists_no_truncation_noninheritable_and_exception_release(self):
        with storage.workspace_lock(self.root, self.lock) as held:
            self.assertTrue(held.active)
            self.assertEqual(held.root, self.root)
            self.assertFalse(os.get_inheritable(held.fd))
            self.assertEqual(stat.S_IMODE(os.fstat(held.fd).st_mode), 0o600)
            os.write(held.fd, b"persistent diagnostic")
            inode = os.fstat(held.fd).st_ino
        self.assertFalse(held.active)
        with self.assertRaisesRegex(RuntimeError, "body failed"):
            with storage.workspace_lock(self.root, self.lock):
                raise RuntimeError("body failed")
        with storage.workspace_lock(self.root, self.lock):
            self.assertEqual(self.lock.stat().st_ino, inode)
            self.assertEqual(self.lock.read_bytes(), b"persistent diagnostic")

    def test_nested_held_alias_and_expired_root_path_owner_validation(self):
        alias = self.root / "root-alias"
        alias.symlink_to(self.root, target_is_directory=True)
        with storage.workspace_lock(self.root, self.lock) as held:
            with patch.object(storage.fcntl, "flock", side_effect=AssertionError("reacquired")):
                with storage.workspace_lock(alias, alias / "locks" / "workspace.lock", held=held) as nested:
                    self.assertIs(nested, held)
            self.assertTrue(held.active)
            body_error = OSError("nested body error")
            with self.assertRaises(OSError) as caught:
                with storage.workspace_lock(self.root, self.lock, held=held):
                    raise body_error
            self.assertIs(caught.exception, body_error)
            self.assertTrue(held.active)
            for root, path in ((self.root, self.root / "other.lock"),
                               (self.root / "other", self.root / "other" / "lock")):
                with self.assertRaises(storage.StorageError):
                    with storage.workspace_lock(root, path, held=held):
                        self.fail("invalid held token accepted")
            errors = []

            def other_thread():
                try:
                    with storage.workspace_lock(self.root, self.lock, held=held):
                        pass
                except storage.StorageError as exc:
                    errors.append(exc)

            thread = threading.Thread(target=other_thread)
            thread.start()
            thread.join(timeout=5)
            self.assertFalse(thread.is_alive())
            self.assertEqual(len(errors), 1)
            with patch.object(storage.os, "getpid", return_value=held.owner_pid + 1):
                with self.assertRaises(storage.StorageError):
                    with storage.workspace_lock(self.root, self.lock, held=held):
                        self.fail("wrong pid accepted")
        with self.assertRaises(storage.StorageError):
            with storage.workspace_lock(self.root, self.lock, held=held):
                self.fail("expired token accepted")

    def test_lock_timeout_validation_and_explicit_nested_token_requirement(self):
        for timeout in (-1, float("nan"), float("inf")):
            with self.subTest(timeout=timeout), self.assertRaises(storage.StorageError):
                with storage.workspace_lock(self.root, self.lock, timeout=timeout):
                    self.fail("invalid timeout accepted")
        self.assertFalse(self.lock.exists())
        with storage.workspace_lock(self.root, self.lock):
            with self.assertRaises(storage.LockBusy):
                with storage.workspace_lock(self.root, self.lock, timeout=0):
                    self.fail("lock was silently reacquired")

    def test_independent_process_contention_and_death_release(self):
        script = """
import sys, time
from pathlib import Path
from runner.storage import workspace_lock
with workspace_lock(Path(sys.argv[1]), Path(sys.argv[2])):
    print('locked', flush=True)
    time.sleep(60)
"""
        process = subprocess.Popen([sys.executable, "-c", script, str(self.root), str(self.lock)],
                                   cwd=Path(__file__).resolve().parents[1],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            # Bound the handshake even if the child never takes the lock.
            ready, _, _ = select.select([process.stdout], [], [], 5)
            self.assertTrue(ready, "child did not acquire lock in time")
            self.assertEqual(process.stdout.readline().strip(), "locked")
            with self.assertRaisesRegex(storage.LockBusy, "workspace lock busy"):
                with storage.workspace_lock(self.root, self.lock, timeout=0.1):
                    self.fail("contended lock acquired")
            inode = self.lock.stat().st_ino
            process.kill()
            process.wait(timeout=5)
            with storage.workspace_lock(self.root, self.lock, timeout=1):
                self.assertEqual(self.lock.stat().st_ino, inode)
        finally:
            if process.poll() is None:
                process.kill()
            process.communicate(timeout=5)


if __name__ == "__main__":
    unittest.main()
