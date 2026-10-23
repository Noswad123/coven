"""Offline upgrade, reviewed graph preparation, replay, and durability contracts."""

from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

from runner import execution_v2 as v2, storage, task_attempts as v1, task_graphs


class ExecutionV2Tests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        (self.root / "state").mkdir()
        self.store = v1.Store(self.root)
        self.path = self.root / v1.CONTROL
        context = storage.workspace_lock(self.root, self.root / "state/.workspace.lock")
        self.lock = context.__enter__()
        self.addCleanup(context.__exit__, None, None, None)
        self.tasks = [{"id": key, "title": key, "status": "arbitrary", "owner": "Worker",
                       "unknown": {"preserved": [None, True, "text"]}} for key in ("a", "b", "c")]
        self.poison = Mock(side_effect=AssertionError("source must not be called"))
        self.counter = 0
        self.old("enable", {}, "enable")
        enabled = self.raw()
        enabled["extensions"] = {"opaque": ["retained", {"future": True}]}
        self.path.write_bytes(v1.canonical(enabled))
        for key in ("a", "b", "c"):
            self.old("enroll", {"task_id": key, "rebind_source": False}, "enroll-" + key)
        prepared = self.old("prepare", {"task_id": "a", "intent": self.intent("a")}, "old-prepare")
        self.old("cancel", {"attempt_id": prepared["result"]["attempt_id"], "reason": "old cancellation"}, "old-cancel")
        self.legacy = self.raw()
        self.path.write_bytes(json.dumps(self.legacy, indent=3).encode() + b"\n\n")
        self.path.chmod(0o640)
        self.original = self.path.read_bytes()
        self.old_view = v1._replay(self.legacy)

    def raw(self):
        return v1._json(self.path.read_bytes())

    def view(self):
        return v2.replay(self.raw())

    def revision(self):
        return storage.snapshot_revision(v1.canonical(self.tasks))

    def source(self, snapshot_id):
        return {"identity": {"kind": "snapshot-v1", "source_id": snapshot_id, "path": "state/tasks.json"},
                "revision": self.revision(), "tasks": deepcopy(self.tasks), "roster": ["Worker"]}

    def intent(self, key):
        return {"schema_version": 1, "kind": "task-intent", "task_id": key, "owner": "Worker",
                "acceptance_criteria": [{"id": "check", "text": "  exact reviewed criterion  "}]}

    def old(self, op, params, key):
        view = self.store.read()
        return self.store.mutate(op, params, expected_execution_revision=view.revision if view else v1.DISABLED,
            expected_tasks_revision=self.revision() if op in ("enroll", "prepare") else None,
            request_id=key, source_loader=self.source, lock=self.lock)

    def upgrade(self):
        return v2.upgrade(self.store, self.old_view.revision, "upgrade", self.lock)

    def call(self, op, params, **kwargs):
        self.counter += 1
        needs = op in ("enroll", "prepare", "prepare_graph", "graph_propose") or (
            op == "graph_decide" and params["decision"] == "approve")
        args = {"expected_execution_revision": self.view().revision,
                "expected_tasks_revision": self.revision() if needs else None,
                "request_id": "new-" + str(self.counter),
                "source_loader": self.source if needs else self.poison, "lock": self.lock}
        args.update(kwargs)
        return v2.mutate(self.store, op, params, **args)

    def document(self, keys=("a", "b")):
        return {"schema_version": 1, "kind": "task-graph", "nodes": [
            {**{k: self.intent(key)[k] for k in ("task_id", "owner", "acceptance_criteria")},
             "depends_on": ["a"] if key == "b" and "a" in keys else []} for key in keys]}

    def propose(self, keys=("a", "b")):
        return self.call("graph_propose", {"document": self.document(keys)})

    def decide(self, decision="approve", **kwargs):
        view = self.view()
        return self.call("graph_decide", {"graph_revision": view.current_graph_revision,
            "decision": decision, "reason": "reviewed locally", "expected_graph_digest":
            view.graphs[view.current_graph_revision]["digest"]}, **kwargs)

    def prepare(self, key="a"):
        return self.call("prepare_graph", {"task_id": key, "graph_revision": self.view().current_graph_revision})

    def cancel(self, attempt):
        return self.call("cancel", {"attempt_id": attempt["result"]["attempt_id"], "reason": "cancel locally"})

    def reject(self, fn):
        before = self.path.read_bytes()
        with self.assertRaises(storage.StorageError):
            fn()
        self.assertEqual(before, self.path.read_bytes())

    def test_upgrade_exact_backup_prefix_and_legacy_receipts(self):
        envelope = self.upgrade()
        control, view = self.raw(), self.view()
        self.assertEqual(control["legacy_v1"], self.legacy)
        self.assertEqual(view.authority_id, self.old_view.authority_id)
        self.assertEqual(view.snapshot_source_id, self.old_view.snapshot_source_id)
        self.assertEqual(view.attempts, self.old_view.attempts)
        self.assertEqual(view.extensions, self.old_view.extensions)
        backup = self.root / envelope["result"]["backup_path"]
        self.assertEqual(backup.read_bytes(), self.original)
        self.assertEqual(backup.stat().st_mode & 0o7777, 0o640)
        self.assertEqual(control["upgrade"]["source_sha256"], hashlib.sha256(self.original).hexdigest())
        self.assertEqual(view.generation, self.old_view.generation + 1)
        self.assertIsInstance(view, v1.View)
        self.assertEqual(view.schema_version, 2)
        for key, receipt in self.old_view.receipts.items():
            binding = receipt["binding"]
            response = self.call(binding["operation"], binding["parameters"], request_id=key,
                expected_execution_revision=binding["expected_execution_revision"],
                expected_tasks_revision=binding["expected_tasks_revision"], source_loader=self.poison)
            self.assertEqual(response["result"], receipt["result"])
            self.assertTrue(response["delivery"]["replayed"])
        self.poison.assert_not_called()
        backup.unlink()
        self.assertEqual(self.view().revision, view.revision)
        self.assertEqual(self.upgrade()["result"], envelope["result"])

    def test_no_implicit_upgrade_and_prepared_blocks_upgrade(self):
        self.assertEqual(self.store.read().revision, self.old_view.revision)
        self.assertEqual(self.path.read_bytes(), self.original)
        self.old("prepare", {"task_id": "a", "intent": self.intent("a")}, "still-v1")
        revision = self.store.read().revision
        self.reject(lambda: v2.upgrade(self.store, revision, "upgrade", self.lock))
        self.assertFalse((self.root / v1.DIR / "upgrades").exists())

    def test_new_namespace_operations_and_conflicting_legacy_keys(self):
        self.upgrade()
        self.reject(lambda: self.call("enable", {}))
        self.reject(lambda: self.call("prepare", {"task_id": "c", "intent": self.intent("c")},
                                      expected_execution_revision=self.old_view.revision))
        self.reject(lambda: self.call("prepare", {"task_id": "c", "intent": self.intent("c")}, request_id="enable"))
        with self.assertRaises(storage.StorageError):
            v1._replay(self.raw())
        record = self.call("prepare", {"task_id": "c", "intent": self.intent("c")})["result"]["attempt"]
        self.assertEqual(record["schema_version"], 2)
        self.assertIsNone(record["graph_binding"])

    def test_graph_review_derived_intent_and_downstream_preparation(self):
        self.upgrade()
        proposed = self.propose()["result"]
        self.assertEqual(task_graphs.topology(proposed["manifest"]), ["a", "b"])
        self.reject(lambda: self.prepare())
        self.reject(lambda: self.call("prepare", {"task_id": "a", "intent": self.intent("a")}))
        decision = self.decide()["result"]["decision"]
        downstream = self.prepare("b")
        record = downstream["result"]["attempt"]
        self.assertEqual(record["intent"], self.intent("b"))
        self.assertEqual(record["graph_binding"]["decision_id"], decision["decision_id"])
        self.assertEqual(record["graph_binding"]["graph_digest"], proposed["graph_digest"])
        self.assertEqual(record["graph_binding"]["node_digest"], proposed["manifest"]["nodes"][1]["node_digest"])
        self.assertEqual(record["intent_digest"], v2._intent_digest(record))
        self.assertEqual(downstream["authorization"], "none")
        self.assertFalse(downstream["dispatchable"])
        self.assertEqual(downstream["blockers"], v1.BLOCKERS)
        self.prepare("a")

    def test_unrelated_revision_allowed_selected_source_changes_rejected(self):
        self.upgrade()
        self.propose()
        self.tasks[2]["title"] = "unrelated edit"
        approved = self.decide()
        self.assertTrue(approved["result"]["source_match"]["whole_revision_changed"])
        self.tasks[0]["unknown"]["preserved"].append("changed")
        self.reject(lambda: self.prepare("b"))
        self.reject(lambda: self.decide())
        self.decide("changes-requested", source_loader=self.poison)
        self.poison.assert_not_called()

    def test_revocation_keeps_prepared_and_successor_requires_fresh_review(self):
        self.upgrade()
        self.propose()
        self.decide()
        first = self.prepare()
        old_revision = self.view().current_graph_revision
        old_id = self.view().graph_id
        self.reject(lambda: self.decide())
        self.reject(lambda: self.propose(("c",)))
        self.tasks[0]["owner"] = "broken-owner"
        self.decide("changes-requested", source_loader=self.poison)
        self.assertEqual(self.view().attempts[first["result"]["attempt_id"]]["state"], "prepared")
        self.reject(lambda: self.prepare("b"))
        self.cancel(first)
        self.propose(("c",))
        view = self.view()
        self.assertEqual(view.graph_id, old_id)
        self.assertEqual(view.graphs[view.current_graph_revision]["manifest"]["predecessor"], old_revision)
        self.assertEqual(view.managed_tasks, {"a", "b", "c"})
        self.reject(lambda: self.call("prepare", {"task_id": "b", "intent": self.intent("b")}))
        self.reject(lambda: self.call("prepare_graph", {"task_id": "b", "graph_revision": old_revision}))
        self.reject(lambda: self.prepare("c"))
        self.decide()
        self.prepare("c")
        self.poison.assert_not_called()

    def test_unmanaged_prepared_survives_graph_proposal(self):
        self.upgrade()
        self.call("prepare", {"task_id": "c", "intent": self.intent("c")})
        self.propose()
        self.decide()
        self.reject(lambda: self.propose(("c",)))

    def test_graph_request_replay_before_cas_and_source(self):
        self.upgrade()
        proposal = self.propose()
        receipt = list(self.view().receipts.values())[-1]
        self.decide()
        self.tasks.clear()
        binding = receipt["binding"]
        replay = self.call("graph_propose", binding["parameters"], request_id=receipt["request_id"],
            expected_execution_revision=binding["expected_execution_revision"],
            expected_tasks_revision=binding["expected_tasks_revision"], source_loader=self.poison)
        self.assertEqual(proposal["result"], replay["result"])
        self.assertTrue(replay["delivery"]["replayed"])
        self.poison.assert_not_called()

    def test_upgrade_prepublication_failure_leaves_v1_authority(self):
        with patch.object(storage, "atomic_replace_bytes", side_effect=storage.StorageError("before replace")):
            self.reject(self.upgrade)
        self.assertEqual(self.store.read().revision, self.old_view.revision)
        backups = list((self.root / v1.DIR / "upgrades").glob("*/control-v1.bytes"))
        self.assertEqual(len(backups), 1)
        self.assertEqual(backups[0].read_bytes(), self.original)
        self.upgrade()
        self.assertEqual(len(list((self.root / v1.DIR / "upgrades").iterdir())), 2)

    def test_upgrade_backup_sync_failure_leaves_v1_authority(self):
        with patch.object(storage, "sync_directory", side_effect=OSError("backup fsync")):
            self.reject(self.upgrade)
        self.assertEqual(self.store.read().revision, self.old_view.revision)

    def test_upgrade_uncertain_publication_and_durable_retry(self):
        replace = storage.atomic_replace_bytes
        def uncertain(path, data):
            replace(path, data)
            raise storage.CommitUncertain(path)
        with patch.object(storage, "atomic_replace_bytes", side_effect=uncertain):
            response = self.upgrade()
        self.assertEqual(response["delivery"]["outcome"], "durability_uncertain")
        self.assertIsNone(response["delivery"]["current_revision"])
        retry = self.upgrade()
        self.assertEqual(retry["result"], response["result"])
        self.assertEqual(retry["delivery"]["outcome"], "committed")
        self.assertTrue(retry["delivery"]["replayed"])

    def test_postpublication_and_replay_verification_failure_keep_receipt(self):
        with patch.object(v2, "_confirm", side_effect=OSError("cannot read current")):
            response = self.upgrade()
        with patch.object(v2, "_confirm", side_effect=storage.StorageError("cannot fsync")):
            retry = self.upgrade()
        self.assertEqual(retry["result"], response["result"])
        self.assertIsNone(retry["delivery"]["current_revision"])
        self.assertIn("known_control_revision", retry["delivery"])

    def test_reserves_reject_growth_but_allow_revocation_and_cancellation(self):
        self.upgrade()
        self.propose()
        self.decide()
        first = self.prepare()
        # Place the current authority just inside the budget, including both reserves.
        limit = len(v1.canonical(self.raw())) + 1 + v2.CANCELLATION_RESERVE + v2.GRAPH_REVIEW_RESERVE + 32
        with patch.object(v2, "MAX_CONTROL", limit):
            self.reject(lambda: self.prepare("b"))
            self.reject(lambda: self.decide())
            self.decide("changes-requested")
            self.cancel(first)
        self.assertEqual(self.view().attempts[first["result"]["attempt_id"]]["state"], "cancelled_before_submission")

    def test_maximum_escaped_review_and_cancel_reason_fit_reserves(self):
        self.upgrade()
        self.propose()
        self.decide()
        first = self.prepare()
        limit = len(v1.canonical(self.raw())) + 1 + v2.CANCELLATION_RESERVE + v2.GRAPH_REVIEW_RESERVE
        with patch.object(v2, "MAX_CONTROL", limit):
            view = self.view()
            self.call("graph_decide", {"graph_revision": view.current_graph_revision,
                "decision": "changes-requested", "reason": "\x01" * 16384,
                "expected_graph_digest": view.graphs[view.current_graph_revision]["digest"]})
            self.call("cancel", {"attempt_id": first["result"]["attempt_id"], "reason": "\x01" * 16384})

    def test_corrupted_history_and_strict_fields_fail_closed(self):
        self.upgrade()
        self.propose()
        self.decide()
        self.prepare()
        original = self.raw()
        changes = [
            lambda c: c.update(extra=True),
            lambda c: c.update(schema_version=True),
            lambda c: c.update(execution_authority_id="f" * 32),
            lambda c: c["extensions"].update(extra=1),
            lambda c: c["legacy_v1"]["history"][0]["result"].update(generation=2),
            lambda c: c["upgrade"].update(source_revision=self.view().revision),
            lambda c: c["history"][1]["binding"].update(expected_execution_revision=self.old_view.revision),
            lambda c: c["history"][1]["effect"]["manifest"]["nodes"][0].update(node_digest="bad"),
            lambda c: c["history"][1]["effect"]["manifest"]["nodes"][0]["acceptance_criteria"][0].update(text="tampered"),
            lambda c: c["history"][2]["effect"]["decision"].update(reason="tampered"),
            lambda c: c["history"][-1]["effect"]["prepared"].update(graph_binding=None),
            lambda c: c["history"][-1]["result"].update(state="accepted"),
            lambda c: c["history"][-1].update(request_id="enable"),
        ]
        for change in changes:
            control = deepcopy(original)
            change(control)
            with self.subTest(change=change), self.assertRaises(storage.StorageError):
                v2.replay(control)
        for raw in (b'{"schema_version":2,"schema_version":2}', b'{"x":NaN}', b'{"x":1e999}'):
            with self.assertRaises(storage.StorageError):
                v1._json(raw)

    def test_invalid_dag_and_source_cas_rejected_without_receipt(self):
        self.upgrade()
        document = self.document()
        document["nodes"][0]["depends_on"] = ["b"]
        self.reject(lambda: self.call("graph_propose", {"document": document}))
        self.reject(lambda: self.call("graph_propose", {"document": self.document()},
                                      expected_tasks_revision="tasks-missing:v1"))
        self.assertEqual(len(self.view().graphs), 0)

    def test_lock_and_backup_path_safety(self):
        with self.assertRaises(storage.StorageError):
            v2.upgrade(self.store, self.old_view.revision, "upgrade", None)
        (self.root / v1.DIR / "upgrades").symlink_to(self.root / "state", target_is_directory=True)
        self.reject(self.upgrade)

    def test_package_and_direct_import(self):
        result = subprocess.run([sys.executable, "-c", "import execution_v2; assert execution_v2.View.schema_version == 2"],
            cwd=Path(v2.__file__).parent, capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_retry_chain_preserves_legacy_and_graph_records(self):
        self.upgrade()
        self.propose()
        self.decide()
        first = self.prepare()
        record = first["result"]["attempt"]
        self.assertEqual(record["attempt_number"], 2)
        self.assertEqual(record["previous_attempt_id"], self.old_view.receipts["old-prepare"]["result"]["attempt_id"])
        self.cancel(first)
        second = self.prepare()["result"]["attempt"]
        self.assertEqual(second["previous_attempt_id"], record["attempt_id"])
        self.assertEqual(second["attempt_number"], 3)
        self.assertEqual(self.view().attempts[record["attempt_id"]]["prepared"], record)
        self.assertEqual(self.raw()["legacy_v1"], self.legacy)

    def test_historical_decision_replay_but_no_fresh_historical_decision(self):
        self.upgrade()
        self.propose()
        approved = self.decide()
        receipt = list(self.view().receipts.values())[-1]
        self.propose(("c",))
        binding = receipt["binding"]
        self.reject(lambda: self.call("graph_decide", binding["parameters"]))
        response = self.call("graph_decide", binding["parameters"], request_id=receipt["request_id"],
            expected_execution_revision=binding["expected_execution_revision"],
            expected_tasks_revision=binding["expected_tasks_revision"], source_loader=self.poison)
        self.assertEqual(response["result"], approved["result"])
        self.poison.assert_not_called()

    def test_rebind_identity_and_fresh_graph_review(self):
        self.upgrade()
        self.propose()
        self.decide()
        original_loader = self.source
        def changed(snapshot_id):
            source = original_loader(snapshot_id)
            source["identity"]["source_id"] = snapshot_id
            source["identity"] = {"kind": "task-ledger-v1", "workspace_id": "a" * 32,
                                  "epoch_id": "b" * 32, "path": "state/task-ledger"}
            source["revision"] = "tasks-ledger:v1:" + "a" * 32 + ":" + "b" * 32 + ":0:" + hashlib.sha256(v1.canonical(self.tasks)).hexdigest()
            return source
        self.source = changed
        revision = changed(self.view().snapshot_source_id)["revision"]
        self.reject(lambda: self.call("prepare_graph", {"task_id": "a", "graph_revision": self.view().current_graph_revision},
            expected_tasks_revision=revision))
        for key in ("a", "b"):
            before = self.view().enrollments[key]["enrollment_id"]
            result = self.call("enroll", {"task_id": key, "rebind_source": True}, expected_tasks_revision=revision)
            self.assertEqual(result["result"]["enrollment_id"], before)
            self.assertEqual(result["result"]["enrollment"]["binding_number"], 2)
        self.call("graph_propose", {"document": self.document()}, expected_tasks_revision=revision)
        self.reject(lambda: self.prepare())
        self.decide(expected_tasks_revision=revision)
        self.call("prepare_graph", {"task_id": "b", "graph_revision": self.view().current_graph_revision},
                  expected_tasks_revision=revision)

    def test_actual_postpublication_read_failure_retains_known_receipt(self):
        read = self.store._control
        calls = 0
        def fail_after_initial_read():
            nonlocal calls
            calls += 1
            if calls > 1:
                raise v1.AttemptError("post-publication read failure")
            return read()
        with patch.object(self.store, "_control", side_effect=fail_after_initial_read):
            response = self.upgrade()
        self.assertEqual(response["delivery"]["outcome"], "durability_uncertain")
        self.assertIsNone(response["delivery"]["current_revision"])
        self.assertEqual(self.upgrade()["result"], response["result"])


if __name__ == "__main__":
    unittest.main()
