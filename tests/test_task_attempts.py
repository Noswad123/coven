"""Offline preparation authority contracts, with local files and fake sources."""

from copy import deepcopy
import hashlib
import json
import os
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import Mock, patch

from runner import storage, task_attempts as attempts


class TaskAttemptTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        (self.root / "state").mkdir()
        self.store = attempts.Store(self.root)
        self.path = self.root / attempts.CONTROL
        self.context = storage.workspace_lock(self.root, self.root / "state/.workspace.lock")
        self.lock = self.context.__enter__()
        self.addCleanup(self.context.__exit__, None, None, None)
        self.task = {"id": "task-1", "title": "Task", "status": "custom-legacy-state", "owner": "Worker",
                     "unknown": {"unicode": "\u2003text\u00e9", "array": [None, True, 4]}}
        self.tasks = [self.task]
        self.roster = ["Worker", "other"]
        self.mode = "snapshot"
        self.calls = []
        self.intent = {"schema_version": 1, "kind": "task-intent", "task_id": "task-1", "owner": "Worker",
                       "acceptance_criteria": [{"id": "check-1", "text": "  $(do-not-run) \u2003\u00e9  "}]}

    def source(self, snapshot_id):
        self.calls.append(snapshot_id)
        if self.mode == "snapshot":
            identity = {"kind": "snapshot-v1", "source_id": snapshot_id, "path": "state/tasks.json"}
        else:
            identity = {"kind": "task-ledger-v1", "workspace_id": "a" * 32,
                        "epoch_id": "b" * 32, "path": "state/task-ledger"}
        return {"identity": identity, "revision": self.tasks_revision(),
                "tasks": deepcopy(self.tasks), "roster": list(self.roster)}

    def tasks_revision(self):
        if self.mode == "snapshot":
            return storage.snapshot_revision(attempts.canonical(self.tasks))
        return "tasks-ledger:v1:" + "a" * 32 + ":" + "b" * 32 + ":0:" + hashlib.sha256(attempts.canonical(self.tasks)).hexdigest()

    def mutate(self, operation, parameters, key=None, execution=None, tasks="auto", loader="auto"):
        view = self.store.read()
        return self.store.mutate(operation, parameters,
            expected_execution_revision=execution if execution is not None else view.revision if view else attempts.DISABLED,
            expected_tasks_revision=(self.tasks_revision() if operation in ("enroll", "prepare") else None) if tasks == "auto" else tasks,
            request_id=key or operation + ":" + str(view.generation if view else 0),
            source_loader=self.source if loader == "auto" else loader, lock=self.lock)

    def enable(self):
        return self.mutate("enable", {}, "enable")

    def enroll(self):
        return self.mutate("enroll", {"task_id": "task-1", "rebind_source": False})

    def prepare(self):
        return self.mutate("prepare", {"task_id": "task-1", "intent": self.intent})

    def ready(self):
        self.enable()
        self.enroll()
        return self.prepare()

    def cancel(self, attempt_id, **kwargs):
        return self.mutate("cancel", {"attempt_id": attempt_id, "reason": "  no submission \u2003 "}, **kwargs)

    def raw(self):
        return json.loads(self.path.read_bytes())

    def write(self, control):
        self.path.write_bytes(attempts.canonical(control))

    def assert_rejected(self, operation, parameters, **kwargs):
        before = self.path.read_bytes() if self.path.exists() else None
        with self.assertRaises(storage.StorageError):
            self.mutate(operation, parameters, **kwargs)
        self.assertEqual(self.path.read_bytes() if self.path.exists() else None, before)

    def test_enable_explicit_uuid_source_and_receipt(self):
        self.assertIsNone(self.store.read())
        self.assertEqual(self.store.inspect_control(), {"status": "disabled", "revision": attempts.DISABLED})
        response = self.enable()
        view = self.store.read()
        self.assertEqual(self.calls, [view.snapshot_source_id])
        self.assertRegex(view.authority_id, r"^[0-9a-f]{32}$")
        self.assertEqual(view.generation, 1)
        self.assertEqual(view.enrollments, {})
        self.assertEqual(view.attempts, {})
        self.assertEqual(response["result"]["snapshot_source_id"], view.snapshot_source_id)
        self.assertEqual(view.receipts["enable"]["result"], response["result"])
        self.assertFalse(response["dispatchable"])
        self.assertEqual(response["authorization"], "none")
        self.assertEqual(response["blockers"], attempts.BLOCKERS)
        self.assertNotIn("current_revision", response["result"])
        self.assertEqual(view.revision, "execution-control-v1:" + view.authority_id + ":1:" + hashlib.sha256(attempts.canonical(self.raw())).hexdigest())

    def test_enable_replay_and_already_enabled(self):
        original = self.enable()
        replay = self.mutate("enable", {}, "enable", execution=attempts.DISABLED, loader=Mock(side_effect=AssertionError))
        self.assertEqual(original["result"], replay["result"])
        self.assertTrue(replay["delivery"]["replayed"])
        self.assert_rejected("enable", {}, key="different-enable")
        self.assertEqual(len(self.calls), 1)

    def test_pending_fails_closed_and_archive_retains_entire_directory(self):
        directory = self.root / attempts.DIR
        directory.mkdir()
        (directory / "orphan").write_text("retain")
        with self.assertRaises(attempts.AttemptError):
            self.store.read()
        self.assertEqual(self.store.inspect_control()["status"], "pending")
        for guard in (lambda: attempts.guard_task_mutation(self.root, "x", self.lock),
                      lambda: attempts.guard_authority_change(self.root, self.lock)):
            with self.assertRaises(attempts.AttemptError):
                guard()
        archived = Path(self.store.archive_pending(self.lock))
        self.assertRegex(archived.name, r"^execution-pending-archive-[0-9a-f]{32}$")
        self.assertEqual((archived / "orphan").read_text(), "retain")
        self.assertIsNone(self.store.read())
        self.enable()

    def test_malformed_present_is_never_disabled_or_archivable(self):
        (self.root / attempts.DIR).mkdir()
        self.path.write_bytes(b"{not-json")
        self.assertEqual(self.store.inspect_control()["status"], "unavailable")
        with self.assertRaises(attempts.AttemptError):
            self.store.archive_pending(self.lock)
        self.assert_rejected("enable", {})

    def test_execution_and_task_cas_do_not_cache_rejection(self):
        enabled = self.enable()
        self.assert_rejected("enroll", {"task_id": "task-1", "rebind_source": False},
                             execution=attempts.DISABLED, key="retry")
        self.assert_rejected("enroll", {"task_id": "task-1", "rebind_source": False},
                             tasks="tasks-missing:v1", key="retry")
        self.assertNotIn("retry", self.store.read().receipts)
        self.assertEqual(self.enroll()["result"]["generation"], 2)
        self.assertNotEqual(enabled["delivery"]["current_revision"], self.store.read().revision)

    def test_key_namespace_and_changed_binding(self):
        self.enable()
        self.assert_rejected("enroll", {"task_id": "task-1", "rebind_source": False}, key="enable")
        for key in ("", "../x", "a/b", "\u00e9", "x" * 129, True):
            with self.subTest(key=key), self.assertRaises(attempts.AttemptError):
                self.store.mutate("enable", {}, expected_execution_revision=attempts.DISABLED,
                    expected_tasks_revision=None, request_id=key, source_loader=None, lock=self.lock)
        self.enroll()
        original_revision = self.store.read().revision
        prepared = self.prepare()
        self.assert_rejected("prepare", {"task_id": "task-1", "intent": self.intent},
                             key="prepare:2", execution=self.store.read().revision)
        replay = self.mutate("prepare", {"task_id": "task-1", "intent": self.intent},
                            key="prepare:2", execution=original_revision)
        self.assertEqual(replay["result"], prepared["result"])

    def test_replay_precedes_source_owner_roster_and_retains_original_result_after_cancel(self):
        self.enable()
        self.enroll()
        before = self.store.read().revision
        revision = self.tasks_revision()
        prepared = self.prepare()
        attempt_id = prepared["result"]["attempt_id"]
        self.task["owner"] = "nobody"
        self.roster = []
        poison = Mock(side_effect=AssertionError("must not load source"))
        replay = self.mutate("prepare", {"task_id": "task-1", "intent": self.intent},
                            key="prepare:2", execution=before, tasks=revision, loader=poison)
        self.assertEqual(replay["result"], prepared["result"])
        cancelled = self.cancel(attempt_id, loader=poison)
        replay = self.mutate("prepare", {"task_id": "task-1", "intent": self.intent},
                            key="prepare:2", execution=before, tasks=revision, loader=poison)
        self.assertEqual(replay["result"]["state"], "prepared")
        self.assertEqual(replay["delivery"]["attempt_state"], "cancelled_before_submission")
        cancel_binding = self.store.read().receipts["cancel:3"]["binding"]
        replay_cancel = self.cancel(attempt_id, key="cancel:3", loader=poison,
                                    execution=cancel_binding["expected_execution_revision"])
        self.assertEqual(replay_cancel["result"], cancelled["result"])
        self.assert_rejected("cancel", {"attempt_id": attempt_id, "reason": "again"}, loader=poison)
        poison.assert_not_called()

    def test_intent_strict_shapes_and_preserved_text(self):
        self.assertEqual(attempts.validate_intent(self.intent), self.intent)
        invalid = []
        for field in ("argv", "env", "native_id", "policy", "grant", "extensions"):
            invalid.append(self.intent | {field: []})
        invalid.extend([self.intent | {"schema_version": True}, self.intent | {"kind": "execute"},
                        self.intent | {"acceptance_criteria": []}, self.intent | {"owner": "bad/owner"}])
        for criterion in ({"id": "a", "text": "\u2003"}, {"id": "a", "text": "x" * 16385},
                          {"id": "a", "text": "\ud800"}, {"id": "../x", "text": "x"},
                          {"id": "a", "text": "x", "argv": []}, {"id": "x" * 129, "text": "x"}):
            invalid.append(self.intent | {"acceptance_criteria": [criterion]})
        invalid.append(self.intent | {"acceptance_criteria": [{"id": "a", "text": "x"}] * 2})
        invalid.append(self.intent | {"acceptance_criteria": [{"id": str(i), "text": "x"} for i in range(65)]})
        for value in invalid:
            with self.subTest(value=repr(value)[:100]), self.assertRaises(attempts.AttemptError):
                attempts.validate_intent(value)

    def test_full_immutable_snapshot_and_retry_chain(self):
        first = self.ready()["result"]["attempt"]
        self.assertEqual(first["complete_task_snapshot"], self.task)
        self.assertEqual(first["intent"], self.intent)
        self.assertEqual(first["task_snapshot_digest"], attempts.task_snapshot_digest(self.task))
        self.assertEqual(first["attempt_number"], 1)
        self.assertIsNone(first["previous_attempt_id"])
        self.assert_rejected("prepare", {"task_id": "task-1", "intent": self.intent})
        self.cancel(first["attempt_id"])
        self.task["unknown"]["array"].append("new")
        second = self.prepare()["result"]["attempt"]
        self.assertEqual(second["attempt_number"], 2)
        self.assertEqual(second["previous_attempt_id"], first["attempt_id"])
        self.assertNotEqual(second["intent_digest"], first["intent_digest"])
        self.assertEqual(self.store.read().attempts[first["attempt_id"]]["prepared"], first)
        self.assertEqual(self.store.read().enrollments["task-1"]["bindings"][0]["complete_task_snapshot"], first["complete_task_snapshot"])

    def test_source_change_requires_explicit_rebind_and_preserves_history(self):
        first = self.ready()["result"]["attempt"]
        self.mode = "ledger"
        self.assert_rejected("prepare", {"task_id": "task-1", "intent": self.intent})
        self.assert_rejected("enroll", {"task_id": "task-1", "rebind_source": True})
        self.cancel(first["attempt_id"])
        self.assert_rejected("enroll", {"task_id": "task-1", "rebind_source": False})
        rebound = self.mutate("enroll", {"task_id": "task-1", "rebind_source": True})
        self.assertEqual(rebound["result"]["enrollment_id"], first["enrollment_id"])
        self.assertEqual(rebound["result"]["enrollment"]["binding_number"], 2)
        self.assertEqual(len(self.store.read().enrollments["task-1"]["bindings"]), 2)
        second = self.prepare()["result"]["attempt"]
        self.assertEqual(second["previous_attempt_id"], first["attempt_id"])
        self.assertEqual(self.store.read().attempts[first["attempt_id"]]["prepared"], first)
        self.assert_rejected("enroll", {"task_id": "task-1", "rebind_source": True})

    def test_ordinary_edits_do_not_require_reenrollment(self):
        self.enable()
        self.enroll()
        self.task["title"] = "Edited"
        self.task["status"] = "not-an-interpreted-enum"
        prepared = self.prepare()["result"]["attempt"]
        self.assertEqual(prepared["complete_task_snapshot"], self.task)
        self.assert_rejected("enroll", {"task_id": "task-1", "rebind_source": False})

    def test_owner_is_exact_canonical_roster_id_only_for_prepared_task(self):
        self.tasks.append({"id": "unrelated", "title": "x", "status": "x", "owner": "outside-roster"})
        self.task["owner"] = "worker"
        self.enable()
        self.enroll()
        self.assert_rejected("prepare", {"task_id": "task-1", "intent": self.intent})
        self.intent["owner"] = "worker"
        self.assert_rejected("prepare", {"task_id": "task-1", "intent": self.intent})
        self.task["owner"] = None
        self.assert_rejected("prepare", {"task_id": "task-1", "intent": self.intent})
        self.task["owner"] = "Worker"
        self.intent["owner"] = "Worker"
        self.prepare()

    def test_guards_targeted_authority_and_durable_cancel_release(self):
        attempts.guard_authority_change(self.root, self.lock)
        attempts.guard_task_mutation(self.root, "task-1", self.lock)
        prepared = self.ready()
        attempts.guard_task_mutation(self.root, "unrelated", self.lock)
        for guard in (lambda: attempts.guard_authority_change(self.root, self.lock),
                      lambda: attempts.guard_task_mutation(self.root, "task-1", self.lock)):
            with self.assertRaises(attempts.AttemptError):
                guard()
        self.cancel(prepared["result"]["attempt_id"])
        with patch.object(storage, "sync_directory", side_effect=OSError("not durable")):
            with self.assertRaises(storage.CommitUncertain):
                attempts.guard_task_mutation(self.root, "task-1", self.lock)
        attempts.guard_authority_change(self.root, self.lock)
        attempts.guard_task_mutation(self.root, "task-1", self.lock)

    def test_corrupt_control_strict_json_and_schema(self):
        self.enable()
        original = self.path.read_bytes()
        for data in (b'{"schema_version":1,"schema_version":1}', b'{"x":NaN}', b'{"x":1e999}',
                     b'{"x":"\\ud800"}', b'\xff', b'[]', b'{"x":1} trailing'):
            self.path.write_bytes(data)
            with self.subTest(data=data), self.assertRaises(storage.StorageError):
                self.store.read()
            self.assertEqual(self.store.inspect_control()["status"], "unavailable")
        self.path.write_bytes(original)
        for field, value in (("schema_version", 2), ("schema_version", True), ("generation", 0),
                             ("generation", 2), ("extensions", []), ("snapshot_source_id", "bad")):
            control = json.loads(original)
            control[field] = value
            self.write(control)
            with self.subTest(field=field, value=value), self.assertRaises(storage.StorageError):
                self.store.read()

    def test_corrupt_effect_receipt_references_and_ambiguous_prepared(self):
        self.ready()
        baseline = self.raw()
        changes = [lambda c: c["history"][2]["result"].update(state="cancelled_before_submission"),
                   lambda c: c["history"][1]["binding"].update(expected_execution_revision=c["history"][1]["binding"]["expected_execution_revision"].rsplit(":", 1)[0] + ":" + "0" * 64),
                   lambda c: c["history"][2]["effect"]["prepared"].update(enrollment_id="c" * 32),
                   lambda c: c["history"][2]["effect"]["prepared"].update(previous_attempt_id="c" * 32),
                   lambda c: c["history"][2]["effect"]["prepared"].update(intent_digest="0" * 64),
                   lambda c: c["history"][1]["effect"]["enrollment"].update(binding_number=True),
                   lambda c: c["history"][1].update(request_id="enable"),
                   lambda c: c["history"][0]["effect"]["source"]["identity"].update(source_id="c" * 32)]
        for change in changes:
            control = deepcopy(baseline)
            change(control)
            self.write(control)
            with self.assertRaises(storage.StorageError):
                self.store.read()
        control = deepcopy(baseline)
        extra = deepcopy(control["history"][-1])
        extra.update(generation=4, request_id="ambiguous")
        extra["binding"]["expected_execution_revision"] = attempts._revision(baseline)
        extra["effect"]["prepared"]["attempt_id"] = "d" * 32
        extra["effect"]["prepared"]["attempt_number"] = 2
        extra["effect"]["prepared"]["previous_attempt_id"] = baseline["history"][-1]["result"]["attempt_id"]
        extra["result"].update(generation=4, attempt_id="d" * 32, attempt=extra["effect"]["prepared"])
        control["history"].append(extra)
        control["generation"] = 4
        self.write(control)
        with self.assertRaises(attempts.AttemptError):
            self.store.read()

    def test_extensions_retained(self):
        self.enable()
        control = self.raw()
        control["extensions"] = {"future": {"preserve": [1, "\u2003"]}}
        self.write(control)
        self.enroll()
        self.prepare()
        self.assertEqual(self.store.read().extensions, control["extensions"])
        self.assertEqual(self.raw()["extensions"], control["extensions"])

    def test_before_publication_failure_leaves_pending_enable(self):
        with patch.object(storage, "atomic_replace_bytes", side_effect=storage.StorageError("before replace")):
            with self.assertRaises(storage.StorageError):
                self.enable()
        self.assertFalse(self.path.exists())
        self.assertEqual(self.store.inspect_control()["status"], "pending")
        with self.assertRaises(attempts.AttemptError):
            self.enable()
        self.store.archive_pending(self.lock)
        self.enable()

    def test_prepublication_failure_active_authority_is_unchanged(self):
        self.enable()
        before = self.path.read_bytes()
        with patch.object(storage, "atomic_replace_bytes", side_effect=storage.StorageError("before replace")):
            with self.assertRaises(storage.StorageError):
                self.enroll()
        self.assertEqual(self.path.read_bytes(), before)
        self.enroll()

    def test_preparation_admission_reserves_cancellation_space(self):
        self.enable()
        self.enroll()
        baseline = self.path.read_bytes()
        self.prepare()
        prepared_size = len(self.path.read_bytes())
        self.path.write_bytes(baseline)  # Inactive fixture rollback, not a recovery API.
        with patch.object(attempts, "MAX_CONTROL", prepared_size + attempts.CANCELLATION_RESERVE - 1024):
            self.assert_rejected("prepare", {"task_id": "task-1", "intent": self.intent})
        self.assertEqual(self.path.read_bytes(), baseline)

    def test_other_history_cannot_spend_prepared_cancellation_capacity(self):
        prepared = self.ready()
        before = self.path.read_bytes()
        bound = len(before) + attempts.CANCELLATION_RESERVE + 2048
        self.tasks.append({**self.task, "id": "task-2", "unknown": "x" * 12000})
        with patch.object(attempts, "MAX_CONTROL", bound):
            self.assert_rejected("enroll", {"task_id": "task-2", "rebind_source": False})
            self.assertEqual(self.path.read_bytes(), before)
            view = self.store.read()
            result = self.store.mutate("cancel", {"attempt_id": prepared["result"]["attempt_id"], "reason": "\0" * 16384},
                expected_execution_revision=view.revision, expected_tasks_revision=None,
                request_id="c" * 128, source_loader=None, lock=self.lock)
            self.assertEqual(result["result"]["state"], "cancelled_before_submission")
            self.assertLessEqual(len(self.path.read_bytes()), bound)

    def test_multiple_preparations_each_retain_maximum_cancellation_capacity(self):
        self.tasks.append({**self.task, "id": "task-2"})
        self.enable()
        self.enroll()
        self.mutate("enroll", {"task_id": "task-2", "rebind_source": False}, key="enroll-second")
        first = self.prepare()
        second = self.mutate("prepare", {"task_id": "task-2", "intent": self.intent | {"task_id": "task-2"}}, key="prepare-second")
        bound = len(self.path.read_bytes()) + 2 * attempts.CANCELLATION_RESERVE + 2048
        with patch.object(attempts, "MAX_CONTROL", bound):
            for index, value in enumerate((first, second)):
                view = self.store.read()
                cancelled = self.store.mutate("cancel", {"attempt_id": value["result"]["attempt_id"], "reason": "\0" * 16384},
                    expected_execution_revision=view.revision, expected_tasks_revision=None,
                    request_id=str(index) + "c" * 127, source_loader=None, lock=self.lock)
                self.assertEqual(cancelled["result"]["state"], "cancelled_before_submission")
                self.assertLessEqual(len(self.path.read_bytes()), bound)

    def test_postpublication_read_failure_preserves_known_receipt(self):
        self.enable()
        before = self.store.read()
        real_publish = storage.atomic_replace_bytes
        published = False
        real_read = self.store.read
        def publish(path, data):
            nonlocal published
            real_publish(path, data)
            published = True
        def read():
            if published:
                raise attempts.AttemptError("injected execution reopen EIO")
            return real_read()
        with patch.object(storage, "atomic_replace_bytes", side_effect=publish), patch.object(self.store, "read", side_effect=read):
            value = self.store.mutate("enroll", {"task_id": "task-1", "rebind_source": False},
                expected_execution_revision=before.revision, expected_tasks_revision=self.tasks_revision(),
                request_id="post-publish", source_loader=self.source, lock=self.lock)
        self.assertEqual(value["delivery"]["outcome"], "durability_uncertain")
        self.assertIsNone(value["delivery"]["current_revision"])
        self.assertEqual(self.store.read().receipts["post-publish"]["result"], value["result"])

    def test_uncertain_publication_and_replay_do_not_require_confirmation_reread(self):
        self.enable()
        before = self.store.read()
        publish = storage.atomic_replace_bytes
        def uncertain(path, data):
            publish(path, data)
            raise storage.CommitUncertain(path)
        parameters = {"task_id": "task-1", "rebind_source": False}
        with patch.object(storage, "atomic_replace_bytes", side_effect=uncertain), \
                patch.object(self.store, "read", side_effect=attempts.AttemptError("injected reread EIO")) as read:
            value = self.store.mutate("enroll", parameters, expected_execution_revision=before.revision,
                expected_tasks_revision=self.tasks_revision(), request_id="uncertain", source_loader=self.source, lock=self.lock)
            read.assert_not_called()
        self.assertEqual(value["delivery"]["outcome"], "durability_uncertain")
        with patch.object(self.store, "read", side_effect=attempts.AttemptError("confirmation EIO")):
            replay = self.store.mutate("enroll", parameters, expected_execution_revision=before.revision,
                expected_tasks_revision=self.tasks_revision(), request_id="uncertain", source_loader=None, lock=self.lock)
        self.assertEqual(replay["result"], value["result"])
        self.assertEqual(replay["delivery"]["outcome"], "durability_uncertain")
        self.assertTrue(replay["delivery"]["replayed"])

    def test_replace_directory_fsync_uncertainty_and_retry_confirmation(self):
        real_sync = storage.sync_directory
        def sync(path):
            if Path(path) == self.root / attempts.DIR:
                raise OSError("directory sync failed")
            real_sync(path)
        with patch.object(storage, "sync_directory", side_effect=sync):
            response = self.enable()
            self.assertEqual(response["delivery"]["outcome"], "durability_uncertain")
            replay = self.mutate("enable", {}, "enable", execution=attempts.DISABLED, loader=None)
            self.assertEqual(replay["delivery"]["outcome"], "durability_uncertain")
        replay = self.mutate("enable", {}, "enable", execution=attempts.DISABLED, loader=None)
        self.assertEqual(replay["result"], response["result"])
        self.assertEqual(replay["delivery"]["outcome"], "committed")
        self.assertTrue(replay["delivery"]["replayed"])

    def test_confirmation_syncs_control_directory_state_root_and_rejects_stale_view(self):
        self.enable()
        view = self.store.read()
        with patch.object(storage, "sync_directory", wraps=storage.sync_directory) as sync:
            self.store.confirm_durability(view, self.lock)
        self.assertEqual([call.args[0] for call in sync.call_args_list],
                         [self.root / attempts.DIR, self.root / "state", self.root])
        self.enroll()
        with self.assertRaises(attempts.AttemptError):
            self.store.confirm_durability(view, self.lock)

    def test_lock_tokens_explicit_active_root_pid_thread_and_alias(self):
        for lock in (None, object(), storage.WorkspaceLock(self.root, self.lock.lock_path, self.lock.fd,
                    False, os.getpid(), threading.get_ident()), storage.WorkspaceLock(self.root,
                    self.lock.lock_path, self.lock.fd, True, os.getpid() + 1, threading.get_ident()),
                    storage.WorkspaceLock(self.root, self.lock.lock_path, self.lock.fd, True, os.getpid(), -1),
                     storage.WorkspaceLock(self.root.parent, self.lock.lock_path, self.lock.fd, True,
                                           os.getpid(), threading.get_ident()),
                     storage.WorkspaceLock(self.root, self.root / "state/other.lock", self.lock.fd, True,
                                           os.getpid(), threading.get_ident())):
            with self.assertRaises(attempts.AttemptError):
                self.store.mutate("enable", {}, expected_execution_revision=attempts.DISABLED,
                                  expected_tasks_revision=None, request_id="enable", source_loader=self.source, lock=lock)
        alias = self.root / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        alias_store = attempts.Store(alias)
        self.assertEqual(alias_store.root, self.root)
        alias_store._lock(self.lock)
        self.enable()
        self.assertEqual(alias_store.read(), self.store.read())

    def test_workspace_move_preserves_ids_and_revision(self):
        self.ready()
        before = self.store.read()
        moved = self.root / "moved"
        moved.mkdir()
        (self.root / "state").rename(moved / "state")
        new_store = attempts.Store(moved)
        self.assertEqual(new_store.read(), before)
        self.context.__exit__(None, None, None)
        with storage.workspace_lock(moved, moved / "state/.workspace.lock", timeout=0) as lock:
            new_store.confirm_durability(before, lock)

    def test_static_symlinks_types_and_traversal_do_not_redirect(self):
        directory = self.root / attempts.DIR
        directory.symlink_to(self.root / "state", target_is_directory=True)
        with self.assertRaises(attempts.AttemptError):
            self.store.read()
        directory.unlink()
        directory.mkdir()
        self.path.symlink_to(self.root / "missing")
        self.assertEqual(self.store.inspect_control()["status"], "unavailable")
        with self.assertRaises(attempts.AttemptError):
            self.store.archive_pending(self.lock)
        self.path.unlink()
        self.path.mkdir()
        with self.assertRaises(attempts.AttemptError):
            self.store.read()
        for bad in ("../task", "task/x", "\u00e9"):
            with self.assertRaises(attempts.AttemptError):
                attempts.validate_intent(self.intent | {"task_id": bad})

    def test_control_read_and_write_bounds_no_pruning(self):
        self.enable()
        before = self.path.read_bytes()
        with patch.object(attempts, "MAX_CONTROL", len(before)):
            self.assert_rejected("enroll", {"task_id": "task-1", "rebind_source": False})
        self.path.write_bytes(b" " * (attempts.MAX_CONTROL + 1))
        with self.assertRaises(attempts.AttemptError):
            self.store.read()

    def test_read_and_inspection_are_read_only(self):
        with patch.object(storage, "atomic_replace_bytes", side_effect=AssertionError), \
             patch.object(storage, "sync_directory", side_effect=AssertionError):
            self.assertIsNone(self.store.read())
            self.store.inspect_control()
        self.assertFalse((self.root / attempts.DIR).exists())
        self.ready()
        before = self.path.read_bytes()
        with patch.object(storage, "atomic_replace_bytes", side_effect=AssertionError), \
             patch.object(storage, "sync_directory", side_effect=AssertionError):
            self.store.read()
            self.store.inspect_control()
        self.assertEqual(self.path.read_bytes(), before)

    def test_source_validation_strict_identity_revision_records_and_json(self):
        good = self.source("c" * 32)
        bad_sources = [good | {"extra": 1}, good | {"roster": ["Worker", "worker"]},
                       good | {"revision": "tasks-missing:v1"},
                       good | {"tasks": [self.task, self.task]}, good | {"tasks": [self.task | {"x": float("nan")}]},
                       good | {"identity": good["identity"] | {"source_id": "d" * 32}},
                       good | {"identity": good["identity"] | {"path": "elsewhere"}},
                       good | {"revision": "tasks-ledger:v1:" + "a" * 32 + ":" + "b" * 32 + ":0:" + "0" * 64}]
        for source in bad_sources:
            with self.assertRaises(storage.StorageError):
                attempts._source(source, "c" * 32)
        self.mode = "ledger"
        good = self.source("c" * 32)
        attempts._source(good, "c" * 32)
        for source in (good | {"revision": "tasks-missing:v1"},
                       good | {"identity": good["identity"] | {"epoch_id": "d" * 32}},
                       good | {"tasks": []}):
            with self.assertRaises(storage.StorageError):
                attempts._source(source, "c" * 32)

    def test_cancel_rejects_tasks_revision_without_loading(self):
        prepared = self.ready()
        poison = Mock(side_effect=AssertionError)
        self.assert_rejected("cancel", {"attempt_id": prepared["result"]["attempt_id"], "reason": "stop"},
                             tasks=self.tasks_revision(), loader=poison)
        poison.assert_not_called()

    def test_pure_digest_domains_and_no_timestamp_in_intent_digest(self):
        record = self.ready()["result"]["attempt"]
        payload = attempts._intent_payload(record)
        self.assertEqual(record["intent_digest"], hashlib.sha256(
            b"coven-task-attempt-intent-v1\0" + attempts.canonical(payload)).hexdigest())
        changed = record | {"attempt_id": "c" * 32, "prepared_at": "changed"}
        self.assertEqual(attempts._intent_payload(changed), payload)
        for field in ("execution_authority_id", "enrollment_id"):
            changed = record | {field: "c" * 32}
            self.assertNotEqual(attempts.intent_digest(attempts._intent_payload(changed)), record["intent_digest"])
        with self.assertRaises(attempts.AttemptError):
            attempts.canonical({1: "not a JSON key"})

    def test_activation_parent_sync_failure_requires_explicit_archive(self):
        with patch.object(storage, "sync_directory", side_effect=OSError("state fsync")):
            with self.assertRaises(attempts.AttemptError):
                self.enable()
        self.assertEqual(self.store.inspect_control()["status"], "pending")
        self.assertFalse(self.path.exists())
        with patch.object(storage, "sync_directory", wraps=storage.sync_directory) as sync:
            archived = self.store.archive_pending(self.lock)
        self.assertTrue(Path(archived).is_dir())
        sync.assert_called_once_with(self.root / "state")

    def test_postpublication_parent_sync_uncertainty_has_exact_receipt(self):
        self.enable()
        real_sync = storage.sync_directory
        def sync(path):
            if Path(path) == self.root:
                raise OSError("root fsync")
            real_sync(path)
        with patch.object(storage, "sync_directory", side_effect=sync):
            response = self.enroll()
        self.assertEqual(response["delivery"]["outcome"], "durability_uncertain")
        view = self.store.read()
        entry = view.receipts["enroll:1"]
        self.assertEqual(entry["result"], response["result"])
        replay = self.mutate("enroll", {"task_id": "task-1", "rebind_source": False},
                            key="enroll:1", execution=entry["binding"]["expected_execution_revision"], loader=None)
        self.assertEqual(replay["result"], response["result"])
        self.assertEqual(replay["delivery"]["outcome"], "committed")

    def test_lock_cannot_be_used_from_another_thread(self):
        failures = []
        def run():
            try:
                attempts.guard_authority_change(self.root, self.lock)
            except attempts.AttemptError as exc:
                failures.append(exc)
        thread = threading.Thread(target=run)
        thread.start()
        thread.join()
        self.assertEqual(len(failures), 1)
        self.assertIsNone(self.store.read())

    def test_cancel_reference_and_transition_corruption(self):
        prepared = self.ready()["result"]["attempt_id"]
        self.cancel(prepared)
        baseline = self.raw()
        for field, value in (("attempt_id", "c" * 32), ("cancel_id", "bad"),
                             ("reason", "changed"), ("cancelled_at", "not a timestamp")):
            control = deepcopy(baseline)
            control["history"][-1]["effect"][field] = value
            self.write(control)
            with self.subTest(field=field), self.assertRaises(attempts.AttemptError):
                self.store.read()

    def test_unenrolled_prepare_rebind_and_missing_tasks_rejected(self):
        self.enable()
        self.assert_rejected("prepare", {"task_id": "task-1", "intent": self.intent})
        self.assert_rejected("enroll", {"task_id": "task-1", "rebind_source": True})
        self.assert_rejected("enroll", {"task_id": "missing", "rebind_source": False})
        self.assert_rejected("enroll", {"task_id": "task-1", "rebind_source": False}, tasks=None)


if __name__ == "__main__":
    unittest.main()
