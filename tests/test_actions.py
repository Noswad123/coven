"""Pure and temporary-file authority tests; never start a native runtime."""

from copy import deepcopy
from pathlib import Path
import shutil
import tempfile
import unittest
import uuid
from unittest import mock

from runner import actions, checkpoints, launch, storage


class ActionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "workspace"
        self.root.mkdir()
        self.root = self.root.resolve()
        (self.root / "state").mkdir()
        (self.root / "checkpoints").mkdir()
        self.store = actions.ActionStore(self.root)
        self.gate_context = storage.transport_gate(self.root, self.root / "state/.runtime-transport.lock", exclusive=True)
        self.gate = self.gate_context.__enter__()
        self.addCleanup(self.gate_context.__exit__, None, None, None)
        self.lock_context = storage.workspace_lock(self.root, self.root / "state/.workspace.lock")
        self.lock = self.lock_context.__enter__()
        self.addCleanup(self.lock_context.__exit__, None, None, None)
        self.inputs = {"files": {"coven.json": {"sha256": "frozen"}}, "inventory": ["coven.json"]}
        self.control = self.store.enable(self.lock, gate=self.gate)
        # Any accidental attempt to execute from the authority module fails the test.
        self.execute = mock.patch.object(launch, "execute", side_effect=AssertionError("native execution"))
        self.execute.start()
        self.addCleanup(self.execute.stop)

    def bundle(self, action_id=None):
        action_id = action_id or uuid.uuid4().hex
        plan = launch.build_plan(str(self.root.resolve()),
                                 f"coven-{self.control['workspace_id']}-{action_id}", "tmux",
                                 [{"id": "alice", "command": "exec worker", "starter": "Start\nnow"}],
                                 {"PATH": "/usr/bin:/bin"})
        return self.store.make_bundle(self.inputs, plan, action_id, self.lock)

    def approved(self, bundle=None):
        bundle = bundle or self.bundle()
        self.store.publish_bundle(bundle, self.lock)
        cp_id = actions.checkpoint_id(bundle)
        raw, view = checkpoints.prepare(cp_id, actions.checkpoint_bytes(bundle), "approved", "reviewed",
                                        decision_id=uuid.uuid4().hex, decided_at="now", source="cli")
        (self.root / "checkpoints" / f"{cp_id}.md").write_bytes(raw)
        return bundle, view

    def claim(self, bundle, view, inputs=None, **changes):
        args = dict(action_id=bundle["action_id"], expected_manifest=actions.digest(bundle),
                    expected_checkpoint=view.revision, expected_decision=view.decision["decision_id"],
                    current_inputs=self.inputs if inputs is None else inputs, lock=self.lock)
        args.update(changes)
        return self.store.claim(**args)

    def report(self, status="transport_completed"):
        return {"operation": "runtime_launch", "status": status, "completed_steps": 3,
                "handles": {}, "diagnostics": [], "outcome_unknown": False}

    def write_control(self, value):
        (self.root / actions.CONTROL).write_bytes(actions.canonical(value))

    def test_activation_idempotent_and_legacy_only_before_opt_in(self):
        self.assertFalse((self.root / actions.BUNDLES).exists())
        self.assertEqual(self.store.enable(self.lock), self.control)
        self.assertEqual(self.store.location(), self.control["location"])
        bundle, view = self.approved()
        claim, _ = self.claim(bundle, view)
        again = self.store.enable(self.lock)
        self.assertEqual(again["workspace_id"], self.control["workspace_id"])
        self.assertEqual(again["epoch_id"], self.control["epoch_id"])
        self.assertEqual(again["claims"][bundle["action_id"]], claim)
        empty = Path(self.temp.name) / "legacy"
        empty.mkdir()
        legacy = actions.ActionStore(empty)
        self.assertIsNone(legacy.read_control())
        self.assertIsNone(legacy.get_claim(uuid.uuid4().hex))
        (empty / "state").mkdir()
        (empty / actions.BUNDLES).mkdir()
        with self.assertRaisesRegex(actions.ActionError, "orphan"):
            legacy.read_control()
        with storage.workspace_lock(empty, empty / "state/.workspace.lock") as token:
            with self.assertRaises(actions.ActionError):
                legacy.enable(token)

    def test_canonical_strict_bounded_and_stable(self):
        self.assertEqual(actions.canonical({"z": 1, "a": [True, None]}), b'{"a":[true,null],"z":1}')
        self.assertEqual(actions.control_revision(self.control), actions.control_revision(deepcopy(self.control)))
        for value in ({1: "key"}, {"x": (1, 2)}, float("nan"), float("inf"), "\ud800", object()):
            with self.subTest(value=repr(value)), self.assertRaises(actions.ActionError):
                actions.canonical(value)
        for value in (None, True, "a", "A" * 32, "a" * 31, "../" + "a" * 32):
            with self.subTest(value=value), self.assertRaises(actions.ActionError):
                actions.validate_action_id(value)
        with mock.patch.object(actions, "MAX_BYTES", 20):
            with self.assertRaises(actions.ActionError):
                actions.canonical({"x": "a" * 30})
            with self.assertRaises(actions.ActionError):
                self.store.read_control()

    def test_immutable_publication_and_historical_reads(self):
        bundle, view = self.approved()
        self.assertEqual(bundle, self.store.read_bundle(bundle["action_id"]))
        self.assertEqual(self.control, self.store.read_control())
        with self.assertRaises(actions.ActionError):
            self.store.publish_bundle(bundle, self.lock)
        claim, replay = self.claim(bundle, view)
        self.assertFalse(replay)
        self.assertEqual(self.store.read_bundle(bundle["action_id"]), bundle)
        self.assertEqual(claim["plan"], bundle["plan"])
        claim["plan"]["agents"][0]["command"] = "mutated"
        self.assertEqual(self.store.get_claim(bundle["action_id"])["plan"], bundle["plan"])
        self.assertEqual(self.store.inspect(bundle["action_id"])["manifest_digest"], actions.digest(bundle))

    def test_exact_checkpoint_binding_and_no_future_proof_cycle(self):
        bundle, view = self.approved()
        raw = actions.checkpoint_bytes(bundle)
        generated = checkpoints.inspect(actions.checkpoint_id(bundle), raw)
        self.assertEqual(generated.scope_revision, view.scope_revision)
        self.assertNotIn("decision_id", bundle)
        for text in (actions.digest(bundle), bundle["workspace_id"], bundle["epoch_id"],
                     bundle["action_id"], str(self.root), bundle["plan"]["namespace"], "unrestricted"):
            self.assertIn(text.encode(), raw)
        claim, _ = self.claim(bundle, view)
        self.assertEqual(claim["scope_revision"], generated.scope_revision)
        self.assertEqual(claim["checkpoint_revision"], view.revision)
        self.assertEqual(claim["decision_id"], view.decision["decision_id"])

    def test_consumed_retry_precedes_changed_inputs_approval_and_bundle(self):
        bundle, view = self.approved()
        claim, _ = self.claim(bundle, view)
        cp = self.root / "checkpoints" / f"{actions.checkpoint_id(bundle)}.md"
        cp.write_text("Status: changes_requested\n")
        (self.root / actions.BUNDLES / f"{bundle['action_id']}.json").unlink()
        with mock.patch.object(storage, "atomic_replace_bytes") as write:
            repeated, replay = self.claim(bundle, view, inputs={"changed": True})
        self.assertTrue(replay)
        self.assertEqual(repeated, claim)
        write.assert_not_called()
        inspected = self.store.inspect(bundle["action_id"])
        self.assertEqual(inspected["claim"], claim)
        self.assertIsNone(inspected["bundle"])
        self.assertIsNotNone(inspected["bundle_error"])
        for changes in ({"expected_manifest": "runtime-launch-manifest-v1:" + "0" * 64},
                        {"expected_checkpoint": "checkpoint-snapshot-v1:" + "0" * 64},
                        {"expected_decision": uuid.uuid4().hex}):
            with self.subTest(changes=changes), self.assertRaises(actions.ActionError):
                self.claim(bundle, view, **changes)

    def test_input_and_reference_changes_refused_without_writes(self):
        bundle, view = self.approved()
        before = (self.root / actions.CONTROL).read_bytes()
        mutations = ({"current_inputs": {"changed": True}},
                     {"expected_manifest": "runtime-launch-manifest-v1:" + "0" * 64},
                     {"expected_checkpoint": "checkpoint-snapshot-v1:" + "0" * 64},
                     {"expected_decision": uuid.uuid4().hex}, {"expected_manifest": None},
                     {"expected_checkpoint": None}, {"expected_decision": []})
        for changes in mutations:
            with self.subTest(changes=changes), self.assertRaises(actions.ActionError):
                self.claim(bundle, view, **changes)
            self.assertEqual((self.root / actions.CONTROL).read_bytes(), before)
        self.assertIsNone(self.store.get_claim(bundle["action_id"]))

    def test_dedicated_scope_cannot_be_modified_then_reapproved(self):
        bundle, _ = self.approved()
        cp_id = actions.checkpoint_id(bundle)
        original = actions.checkpoint_bytes(bundle)
        cases = [original + b"\nAdditional covered instruction.\n",
                 original.replace(b"unrestricted", b"read-only"),
                 b"# Arbitrary\nStatus: pending\n" + actions.digest(bundle).encode() + b"\n"]
        for raw in cases:
            approved, view = checkpoints.prepare(cp_id, raw, "approved", "", decision_id=uuid.uuid4().hex,
                                                decided_at="now", source="review")
            (self.root / "checkpoints" / f"{cp_id}.md").write_bytes(approved)
            self.assertEqual(view.effective_status, "approved")
            with self.assertRaisesRegex(actions.ActionError, "exact bound"):
                self.claim(bundle, view)

    def test_unbound_stale_and_revoked_checkpoint_refused(self):
        bundle, view = self.approved()
        cp_id = actions.checkpoint_id(bundle)
        cp_path = self.root / "checkpoints" / f"{cp_id}.md"
        approved = cp_path.read_bytes()
        revoked, revoked_view = checkpoints.prepare(cp_id, approved, "changes_requested", "no",
                                                    decision_id=uuid.uuid4().hex, decided_at="now", source="cli")
        cases = [(approved + b"\nchanged\n", view), (revoked, revoked_view),
                 (actions.checkpoint_bytes(bundle).replace(b"Status: pending", b"Status: approved"), view)]
        for raw, expected in cases:
            cp_path.write_bytes(raw)
            with self.assertRaises(actions.ActionError):
                self.claim(bundle, expected)

    def test_single_active_head_and_control_revision_serializes_preparations(self):
        first, first_view = self.approved()
        second, second_view = self.approved()
        claim, _ = self.claim(first, first_view)
        with self.assertRaisesRegex(actions.ActionError, "unresolved"):
            self.claim(second, second_view)
        with self.assertRaises(actions.ActionError):
            self.bundle()
        self.store.record_outcome(first["action_id"], claim["claim_id"], self.report(), self.lock)
        with self.assertRaisesRegex(actions.ActionError, "stale"):
            self.claim(second, second_view)
        fresh, fresh_view = self.approved()
        self.claim(fresh, fresh_view)
        self.assertEqual(self.store.read_control()["head"], fresh["action_id"])

    def test_outcomes_persist_are_final_and_do_not_unconsume(self):
        bundle, view = self.approved()
        claim, _ = self.claim(bundle, view)
        report = self.report("transport_failed_or_partial")
        report.update(outcome_unknown=True, diagnostics=[{"step": 2, "error": "OSError", "message": "unknown"}])
        terminal = self.store.record_outcome(bundle["action_id"], claim["claim_id"], report, self.lock)
        self.assertEqual(terminal["report"], report)
        self.assertEqual(actions.ActionStore(self.root).get_claim(bundle["action_id"]), terminal)
        self.assertEqual(self.store.record_outcome(bundle["action_id"], claim["claim_id"], report, self.lock), terminal)
        with self.assertRaises(actions.ActionError):
            self.store.record_outcome(bundle["action_id"], claim["claim_id"], self.report(), self.lock)
        with self.assertRaises(actions.ActionError):
            self.store.acknowledge_unknown(bundle["action_id"], claim["claim_id"], self.lock)
        with self.assertRaises(actions.ActionError):
            self.bundle(bundle["action_id"])
        replay, was_replay = self.claim(bundle, view)
        self.assertTrue(was_replay)
        self.assertEqual(replay, terminal)

    def test_explicit_unknown_ack_allows_new_action_late_outcome_preserves_head(self):
        bundle, view = self.approved()
        claim, _ = self.claim(bundle, view)
        with self.assertRaises(actions.ActionError):
            self.store.acknowledge_unknown(bundle["action_id"], uuid.uuid4().hex, self.lock)
        ack = self.store.acknowledge_unknown(bundle["action_id"], claim["claim_id"], self.lock)
        self.assertEqual(ack["status"], "outcome_unknown")
        self.assertTrue(ack["operator_acknowledged"])
        self.assertTrue(self.claim(bundle, view)[1])
        fresh, fresh_view = self.approved()
        self.claim(fresh, fresh_view)
        result = self.store.record_outcome(bundle["action_id"], claim["claim_id"], self.report(), self.lock)
        self.assertTrue(result["operator_acknowledged"])
        self.assertEqual(self.store.read_control()["head"], fresh["action_id"])
        stop = {**self.report(), "operation": "runtime_stop", "stop_command_attempted": True, "stop_command_succeeded": True}
        before = (self.root / actions.CONTROL).read_bytes()
        self.assertEqual(self.store.record_stop(bundle["action_id"], stop, self.lock), result)
        self.assertEqual((self.root / actions.CONTROL).read_bytes(), before)

    def test_strict_control_and_claim_shapes_fail_closed(self):
        bundle, view = self.approved()
        self.claim(bundle, view)
        valid = self.store.read_control()
        path = self.root / actions.CONTROL
        for raw in (b'{"version":1,"version":1}', b'{"version":NaN}', b'{"version":1e999}', b'null', b'{', b'[]'):
            path.write_bytes(raw)
            with self.subTest(raw=raw), self.assertRaises(actions.ActionError):
                self.store.read_control()
        changes = [lambda c: c.update(version=True), lambda c: c.update(version=2),
                   lambda c: c.update(extra=1), lambda c: c.update(head=None),
                   lambda c: c["location"].update(inode=True),
                   lambda c: c["claims"][bundle["action_id"]].update(status="ready"),
                   lambda c: c["claims"][bundle["action_id"]].update(operator_acknowledged=True),
                   lambda c: c["claims"][bundle["action_id"]].update(report=self.report()),
                   lambda c: c["claims"][bundle["action_id"]].update(scope_revision="bad"),
                   lambda c: c["claims"][bundle["action_id"]]["plan"].update(cwd="/wrong")]
        for change in changes:
            value = deepcopy(valid)
            change(value)
            self.write_control(value)
            with self.subTest(change=change), self.assertRaises(actions.ActionError):
                self.store.inspect(bundle["action_id"])
        self.write_control(valid)

    def test_bundle_corruption_and_plan_identity_refused(self):
        bundle, view = self.approved()
        path = self.root / actions.BUNDLES / f"{bundle['action_id']}.json"
        mutations = [lambda b: b.update(version=2), lambda b: b.update(version=True),
                     lambda b: b.update(extra=1), lambda b: b.update(inputs=[]),
                     lambda b: b["plan"].update(namespace="coven-a-b"),
                     lambda b: b["plan"]["steps"].reverse(),
                     lambda b: b.update(workspace_id=uuid.uuid4().hex),
                     lambda b: b["inputs"].update(new="different")]
        for change in mutations:
            value = deepcopy(bundle)
            change(value)
            path.write_bytes(actions.canonical(value))
            with self.subTest(change=change), self.assertRaises(actions.ActionError):
                self.claim(bundle, view)
        for raw in (b'{"version":1,"version":1}', b'{"x":Infinity}', b'not JSON'):
            path.write_bytes(raw)
            with self.assertRaises(actions.ActionError):
                self.store.read_bundle(bundle["action_id"])
        self.assertIsNotNone(self.store.inspect(bundle["action_id"])["bundle_error"])

    def test_moved_or_copied_root_requires_explicit_future_reactivation(self):
        copied = Path(self.temp.name) / "copy"
        shutil.copytree(self.root, copied)
        for method in ("read_control", "enable"):
            with self.subTest(method=method), self.assertRaises(actions.ActionError):
                if method == "enable":
                    with storage.workspace_lock(copied, copied / "state/.workspace.lock") as token:
                        actions.ActionStore(copied).enable(token)
                else:
                    actions.ActionStore(copied).read_control()
        moved = Path(self.temp.name) / "moved"
        copied.rename(moved)
        with self.assertRaises(actions.ActionError):
            actions.ActionStore(moved).read_control()
        alias = Path(self.temp.name) / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        self.assertEqual(actions.ActionStore(alias).read_control(), self.control)

    def test_all_writes_require_the_exact_active_lock(self):
        bundle, view = self.approved()
        claim, _ = self.claim(bundle, view)
        for token in (None, object()):
            calls = [lambda: self.store.enable(token),
                     lambda: self.store.make_bundle(self.inputs, bundle["plan"], bundle["action_id"], token),
                     lambda: self.store.publish_bundle(bundle, token),
                     lambda: self.claim(bundle, view, lock=token),
                     lambda: self.store.record_outcome(bundle["action_id"], claim["claim_id"], self.report(), token),
                     lambda: self.store.acknowledge_unknown(bundle["action_id"], claim["claim_id"], token),
                     lambda: self.store.record_stop(bundle["action_id"], {**self.report(), "operation": "runtime_stop",
                                                                          "stop_command_attempted": True, "stop_command_succeeded": True}, token)]
            for call in calls:
                with self.assertRaises(storage.StorageError):
                    call()
        with storage.workspace_lock(self.root, self.root / "state/other.lock") as wrong:
            with self.assertRaises(storage.StorageError):
                self.store.enable(wrong)
        self.lock.active = False
        try:
            with self.assertRaises(storage.StorageError):
                self.store.enable(self.lock, gate=self.gate)
        finally:
            self.lock.active = True

    def test_static_path_guards(self):
        for relative in ("../escape", "/absolute", "state/../../escape", "."):
            with self.subTest(relative=relative), self.assertRaises(actions.ActionError):
                self.store.path(relative)
        (self.root / "state/link").symlink_to(self.root / "checkpoints", target_is_directory=True)
        for relative, directory in (("state/link", True), ("state/link/child", False), ("state", False),
                                    (actions.CONTROL, True)):
            with self.subTest(relative=relative), self.assertRaises(actions.ActionError):
                self.store.path(relative, directory=directory)
        bundle, view = self.approved()
        cp = self.root / "checkpoints" / f"{actions.checkpoint_id(bundle)}.md"
        target = self.root / "checkpoint-target"
        cp.rename(target)
        cp.symlink_to(target)
        with self.assertRaises(actions.ActionError):
            self.claim(bundle, view)
        control = self.root / actions.CONTROL
        control.rename(self.root / "control-target")
        control.symlink_to(self.root / "control-target")
        with self.assertRaises(actions.ActionError):
            self.store.read_control()

    def test_claim_directory_sync_uncertain_consumes_without_implicit_attempt(self):
        bundle, view = self.approved()
        with mock.patch.object(storage, "sync_directory", side_effect=OSError("sync failed")):
            with self.assertRaises(storage.CommitUncertain):
                self.claim(bundle, view)
        observed = self.store.get_claim(bundle["action_id"])
        self.assertEqual(observed["status"], "claimed")
        with mock.patch.object(storage, "atomic_replace_bytes") as write:
            claim, replayed = self.claim(bundle, view)
        self.assertTrue(replayed)
        self.assertEqual(claim, observed)
        write.assert_not_called()
        with mock.patch.object(storage, "sync_directory", side_effect=OSError("still uncertain")):
            with self.assertRaises(storage.CommitUncertain):
                self.claim(bundle, view)
            with self.assertRaises(storage.CommitUncertain):
                self.store.enable(self.lock)

    def test_enable_and_bundle_publication_uncertainty_are_not_implicit_authorization(self):
        bundle = self.bundle()
        real_sync = storage.sync_directory

        def fail_actions(path):
            if path == self.root / actions.BUNDLES:
                raise OSError("directory sync failed")
            real_sync(path)

        with mock.patch.object(storage, "sync_directory", side_effect=fail_actions):
            with self.assertRaises(storage.CommitUncertain):
                self.store.publish_bundle(bundle, self.lock)
        self.assertEqual(self.store.read_bundle(bundle["action_id"]), bundle)
        self.assertIsNone(self.store.get_claim(bundle["action_id"]))
        with self.assertRaises(actions.ActionError):
            self.store.publish_bundle(bundle, self.lock)

    def test_report_validation_and_claim_identity_before_mutation(self):
        bundle, view = self.approved()
        claim, _ = self.claim(bundle, view)
        malformed = [{}, {**self.report(), "status": "ready"}, {**self.report(), "operation": "runtime_stop"},
                     {**self.report(), "completed_steps": True}, {**self.report(), "outcome_unknown": 1},
                     {**self.report(), "handles": {"workspace": "unsafe handle"}},
                     {**self.report(), "diagnostics": [{"step": -1, "error": "X", "message": "x"}]}]
        before = (self.root / actions.CONTROL).read_bytes()
        for report in malformed:
            with self.subTest(report=report), self.assertRaises(actions.ActionError):
                self.store.record_outcome(bundle["action_id"], claim["claim_id"], report, self.lock)
        with self.assertRaises(actions.ActionError):
            self.store.record_outcome(bundle["action_id"], uuid.uuid4().hex, self.report(), self.lock)
        self.assertEqual((self.root / actions.CONTROL).read_bytes(), before)

    def test_initial_enable_uncertainty_retains_the_same_activation(self):
        control_path = self.root / actions.CONTROL
        control_path.unlink()
        with mock.patch.object(storage, "sync_directory", side_effect=OSError("sync failed")):
            with self.assertRaises(storage.CommitUncertain):
                self.store.enable(self.lock, gate=self.gate)
        observed = self.store.read_control()
        self.assertIsNotNone(observed)
        self.assertFalse((self.root / actions.BUNDLES).exists())
        self.assertEqual(self.store.enable(self.lock), observed)

    def test_bundle_activation_tamper_and_stale_publication(self):
        bundle = self.bundle()
        for field in ("epoch_id", "control_revision"):
            changed = deepcopy(bundle)
            changed[field] = (uuid.uuid4().hex if field == "epoch_id" else
                              "runtime-launch-control-v1:" + "0" * 64)
            with self.subTest(field=field), self.assertRaises(actions.ActionError):
                self.store.publish_bundle(changed, self.lock)
        wrong_root = deepcopy(bundle)
        wrong_root["location"]["root"] = "/different"
        with self.assertRaises(actions.ActionError):
            self.store.publish_bundle(wrong_root, self.lock)
        first, view = self.approved()
        claim, _ = self.claim(first, view)
        self.store.record_outcome(first["action_id"], claim["claim_id"], self.report(), self.lock)
        with self.assertRaisesRegex(actions.ActionError, "stale"):
            self.store.publish_bundle(bundle, self.lock)

    def test_bundle_path_symlinks_and_orphan_actions_symlink_fail_closed(self):
        bundle, view = self.approved()
        path = self.root / actions.BUNDLES / f"{bundle['action_id']}.json"
        target = self.root / "bundle-target"
        path.rename(target)
        path.symlink_to(target)
        with self.assertRaises(actions.ActionError):
            self.store.read_bundle(bundle["action_id"])
        with self.assertRaises(actions.ActionError):
            self.claim(bundle, view)
        path.unlink()
        (self.root / actions.BUNDLES).rmdir()
        (self.root / actions.BUNDLES).symlink_to(self.root / "checkpoints", target_is_directory=True)
        with self.assertRaises(actions.ActionError):
            self.store.publish_bundle(bundle, self.lock)
        (self.root / actions.CONTROL).unlink()
        with self.assertRaises(actions.ActionError):
            self.store.read_control()

    def test_read_only_inspection_does_not_write_or_acquire_locks(self):
        bundle, _ = self.approved()
        with mock.patch.object(storage, "workspace_lock", side_effect=AssertionError("lock acquisition")), \
                mock.patch.object(storage, "atomic_replace_bytes", side_effect=AssertionError("write")):
            self.assertEqual(self.store.read_bundle(bundle["action_id"]), bundle)
            self.assertIsNone(self.store.get_claim(bundle["action_id"]))
            self.assertEqual(self.store.inspect(bundle["action_id"])["bundle"], bundle)


if __name__ == "__main__":
    unittest.main()
