"""Task ledger contracts, using only temporary local files and held locks."""

from copy import deepcopy
import json
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import patch

from runner import storage, task_ledger as ledger


class TaskLedgerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        (self.root / "state").mkdir()
        self.tasks_path = self.root / "state/tasks.json"
        self.task = {"id": "legacy_task", "title": "Legacy", "status": "pending", "owner": None,
                     "notes": "original", "created_at": "old", "updated_at": "old", "extension": {"x": [1]}}
        self.source = b"  " + storage.encode_json([self.task]) + b"\n"
        self.tasks_path.write_bytes(self.source)
        self.tasks_path.chmod(0o640)
        self.store = ledger.LedgerStore(self.root)
        self.lock_context = storage.workspace_lock(self.root, self.root / "state/.workspace.lock")
        self.lock = self.lock_context.__enter__()
        self.addCleanup(self.lock_context.__exit__, None, None, None)

    def activate(self):
        return self.store.import_tasks(storage.snapshot_revision(self.source), self.lock)

    def add(self, view, key="add-1"):
        binding = ledger.request_binding("add", {"title": "New Task", "owner": "Worker One", "notes": "note"}, None)
        task = {"id": "new-task", "title": "New Task", "owner": "worker-one", "status": "assigned",
                "notes": "note", "created_at": "new", "updated_at": "new"}
        return self.store.commit(view, key, binding, task, self.lock), binding

    def test_exact_import_and_mode_backup_without_projection_rewrite(self):
        inode = self.tasks_path.stat().st_ino
        view = self.activate()
        base = self.root / ledger.LEDGER / view.epoch_id
        backup = base / "baseline.tasks.bytes"
        self.assertEqual(backup.read_bytes(), self.source)
        self.assertEqual(stat.S_IMODE(backup.stat().st_mode), 0o640)
        self.assertEqual(self.tasks_path.stat().st_ino, inode)
        self.assertEqual(view.projection_bytes, self.source)
        self.assertEqual(view.tasks, [self.task])
        self.assertEqual(self.store.read(), view)
        self.assertEqual(self.store.projection_health(view)["status"], "current")
        self.assertFalse(self.store.has_pending_import())
        self.store.confirm_durability(view, self.lock)

    def test_absent_source_and_explicit_missing_recovery_backup(self):
        self.tasks_path.unlink()
        view = self.store.import_tasks("tasks-missing:v1", self.lock)
        base = self.root / ledger.LEDGER / view.epoch_id
        self.assertFalse((base / "baseline.tasks.bytes").exists())
        manifest = json.loads((base / "manifest.json").read_bytes())
        self.assertFalse(manifest["source_present"])
        self.assertIsNone(manifest["source_mode"])
        self.assertFalse(self.tasks_path.exists())
        self.assertEqual(view.projection_bytes, storage.encode_json([]))
        self.assertEqual(self.store.projection_health(view)["status"], "missing")
        result = self.store.repair_projection(self.lock)
        backup = Path(result["backup_path"])
        self.assertTrue(json.loads((backup / "metadata.json").read_bytes())["missing"])
        self.assertFalse((backup / "tasks.bytes").exists())
        self.assertEqual(self.tasks_path.read_bytes(), view.projection_bytes)

    def test_stale_import_and_invalid_snapshot_leave_source_unchanged(self):
        with self.assertRaises(storage.RevisionConflict):
            self.store.import_tasks("tasks-missing:v1", self.lock)
        self.assertFalse(self.store.has_pending_import())
        self.assertEqual(self.tasks_path.read_bytes(), self.source)
        for raw in (b'[{"id":"a","id":"b","title":"x","status":"pending"}]',
                    b'[{"id":"a","title":"x","status":"pending","x":NaN}]', b'{}'):
            self.tasks_path.write_bytes(raw)
            with self.assertRaises(ledger.LedgerError):
                self.store.import_tasks(storage.snapshot_revision(raw), self.lock)
            self.assertEqual(self.tasks_path.read_bytes(), raw)
            self.assertFalse(self.store.has_pending_import())

    def test_bindings_and_keys(self):
        view = self.activate()
        binding = ledger.request_binding("assign", {"task_id": "legacy_task", "agent": "Worker One"}, view.revision)
        self.assertEqual(binding["version"], 1)
        self.assertEqual(binding["args"]["agent"], "Worker One")
        for bad in ("", " ", "../bad", "a/b", "x" * 129, "\u00e9", 1):
            with self.subTest(key=bad), self.assertRaises(ledger.LedgerError):
                self.store.lookup(view, bad, binding)
        for args in ({"task_id": "legacy_task", "agent": 3}, {"task_id": "x", "agent": "a", "extra": "x"}):
            with self.assertRaises(ledger.LedgerError):
                ledger.request_binding("assign", args, None)
        for token in (True, "tasks-ledger:v2:x", "tasks-missing:v1\n"):
            with self.assertRaises(ledger.LedgerError):
                ledger.validate_revision(token)
        ledger.validate_revision(storage.snapshot_revision(self.source))
        ledger.validate_revision(view.revision)

    def test_add_assign_frozen_receipt_and_unknown_fields(self):
        initial = self.activate()
        added, binding = self.add(initial)
        frozen = self.store.lookup(added, "add-1", binding)
        frozen["task"]["notes"] = "caller mutation"
        self.assertEqual(self.store.lookup(added, "add-1", binding)["task"]["notes"], "note")
        self.store.publish_projection(added, self.lock)
        assign = ledger.request_binding("assign", {"task_id": "legacy_task", "agent": "Worker_One"}, added.revision)
        task = deepcopy(self.task)
        task.update(owner="worker_one", status="assigned", updated_at="later")
        final = self.store.commit(added, "assign-1", assign, task, self.lock)
        self.assertEqual(final.tasks[0]["extension"], {"x": [1]})
        final.tasks[0]["extension"]["x"].append(2)
        replay = self.store.read()
        self.assertEqual(replay.tasks[0]["extension"], {"x": [1]})
        receipt = self.store.lookup(replay, "add-1", binding)
        self.assertEqual(receipt["after_revision"], added.revision)
        self.assertEqual(receipt["sequence"], 1)
        changed = deepcopy(binding)
        changed["expected_revision"] = replay.revision
        with self.assertRaisesRegex(ledger.LedgerError, "changed binding"):
            self.store.lookup(replay, "add-1", changed)
        with self.assertRaises(ledger.LedgerError):
            self.store.commit(initial, "another", binding, receipt["task"], self.lock)

    def test_projection_is_not_authority_and_repair_preserves_invalid_bytes(self):
        view = self.activate()
        corrupt = b"not JSON\xff"
        self.tasks_path.write_bytes(corrupt)
        self.assertEqual(self.store.read(), view)
        self.assertEqual(self.store.projection_health(view)["status"], "diverged")
        with self.assertRaisesRegex(ledger.LedgerError, "projection"):
            self.add(view)
        result = self.store.repair_projection(self.lock)
        self.assertEqual((Path(result["backup_path"]) / "tasks.bytes").read_bytes(), corrupt)
        self.assertEqual(self.tasks_path.read_bytes(), self.source)
        self.add(view)

    def test_observed_uncertain_frame_can_be_confirmed_and_replayed(self):
        view = self.activate()
        with patch.object(storage, "sync_directory", side_effect=OSError("injected directory sync")):
            with self.assertRaises(ledger.PublicationUncertain) as caught:
                self.add(view)
        self.assertEqual(caught.exception.path.name, "00000000000000000001.frame")
        observed = self.store.read()
        self.assertEqual(observed.sequence, 1)
        self.assertIn("add-1", observed.receipts)
        with patch.object(storage.os, "fsync", side_effect=OSError("injected sync")):
            with self.assertRaises(ledger.PublicationUncertain):
                self.store.confirm_durability(observed, self.lock)
        self.store.confirm_durability(observed, self.lock)
        self.store.publish_projection(observed, self.lock)
        self.assertEqual(self.store.projection_health(observed)["status"], "current")

    def test_pending_resume_and_abort_preserve_artifacts(self):
        publish = self.store._publish

        def interrupt(relative, data, mode=0o600):
            if relative == ledger.MARKER:
                raise ledger.LedgerError("before activation")
            return publish(relative, data, mode)

        with patch.object(self.store, "_publish", side_effect=interrupt):
            with self.assertRaises(ledger.LedgerError):
                self.activate()
        self.assertIsNone(self.store.read())
        self.assertTrue(self.store.has_pending_import())
        self.assertEqual(self.tasks_path.read_bytes(), self.source)
        with self.assertRaises(ledger.LedgerError):
            self.activate()
        view = self.store.resume_import(storage.snapshot_revision(self.source), self.lock)
        self.assertEqual(self.store.read(), view)
        with self.assertRaises(ledger.LedgerError):
            self.store.abort_import(self.lock)
        (self.root / ledger.MARKER).unlink()
        aborted = Path(self.store.abort_import(self.lock))
        self.assertTrue((aborted / view.epoch_id / "baseline.tasks.bytes").exists())
        self.assertFalse(self.store.has_pending_import())
        self.assertIsNone(self.store.read())
        self.assertEqual(self.tasks_path.read_bytes(), self.source)

    def test_empty_pending_is_not_adopted_and_abort_is_nondestructive(self):
        (self.root / ledger.LEDGER).mkdir()
        self.assertTrue(self.store.has_pending_import())
        with self.assertRaises(ledger.LedgerError):
            self.store.resume_import(storage.snapshot_revision(self.source), self.lock)
        destination = Path(self.store.abort_import(self.lock))
        self.assertTrue(destination.is_dir())

    def test_bad_marker_never_falls_back(self):
        self.activate()
        marker = self.root / ledger.MARKER
        original = marker.read_bytes()
        for bad in (b"{}", b'{"version":1,"version":1}', b"null", b"invalid"):
            marker.write_bytes(bad)
            with self.assertRaises(ledger.LedgerError):
                self.store.read()
            self.assertEqual(self.tasks_path.read_bytes(), self.source)
        marker.write_bytes(original)

    def test_frame_integrity_identity_versions_gaps_and_duplicate_requests(self):
        view, _ = self.add(self.activate())
        frames = self.root / ledger.LEDGER / view.epoch_id / "frames"
        frame = frames / "00000000000000000001.frame"
        original = frame.read_bytes()
        payload = ledger._payload(original)
        variants = []
        for field, value in (("workspace_id", "0" * 32), ("epoch_id", "0" * 32),
                             ("version", True), ("version", 2), ("sequence", True),
                             ("previous_digest", "0" * 64)):
            altered = deepcopy(payload)
            altered[field] = value
            variants.append(ledger._envelope(altered))
        variants.extend((original.replace(b'"length":', b'"length":0,"length":', 1), original[:-4],
                         original.replace(b'"sha256":"', b'"sha256":"0', 1)))
        for raw in variants:
            frame.write_bytes(raw)
            with self.subTest(raw=raw[:80]), self.assertRaises(ledger.LedgerError):
                self.store.read()
        frame.write_bytes(original)
        gap = frames / "00000000000000000003.frame"
        gap.write_bytes(original)
        with self.assertRaises(ledger.LedgerError):
            self.store.read()
        gap.unlink()
        duplicate = deepcopy(payload)
        duplicate.update(sequence=2, previous_digest=view.head_digest)
        second = frames / "00000000000000000002.frame"
        second.write_bytes(ledger._envelope(duplicate))
        with self.assertRaisesRegex(ledger.LedgerError, "duplicate request"):
            self.store.read()
        second.unlink()
        unexpected = frames / "unrecognized"
        unexpected.mkdir()
        with self.assertRaises(ledger.LedgerError):
            self.store.read()

    def test_lock_validation_and_static_symlinks(self):
        with self.assertRaises(storage.StorageError):
            self.store.import_tasks(storage.snapshot_revision(self.source), None)
        for relative in ("../outside", "/outside"):
            with self.assertRaises(ledger.LedgerError):
                self.store.path(relative)
        target = self.root / "original"
        self.tasks_path.rename(target)
        self.tasks_path.symlink_to(target)
        with self.assertRaises(ledger.LedgerError):
            self.activate()
        self.tasks_path.unlink()
        target.rename(self.tasks_path)
        view = self.activate()
        base = self.root / ledger.LEDGER / view.epoch_id
        (base / "frames").rename(base / "real-frames")
        (base / "frames").symlink_to(base / "real-frames", target_is_directory=True)
        with self.assertRaises(ledger.LedgerError):
            self.store.read()


if __name__ == "__main__":
    unittest.main()
