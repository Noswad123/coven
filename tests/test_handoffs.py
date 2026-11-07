"""Offline claim validation/capture; native effects and check execution forbidden."""

import contextlib
from copy import deepcopy
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import unittest
from unittest import mock

from runner import coven, handoffs, storage, task_attempts
from test_coven import CovenTestCase, snapshot

ROOT = Path(__file__).resolve().parents[1]


def claim(task_id="task", revision="tasks-sha256:"+"0"*64):
    return {"schema_version": 1, "contract": handoffs.HANDOFF,
        "task": {"id": task_id, "owner": "operator", "revision": revision},
        "claimed_status": "completed", "summary": "Worker reports a result; not verified. λ",
        "confidence": "medium", "findings": [{"priority": "high", "claim": "An observed behavior",
                                               "evidence": ["djinn://declared/session#message"]}],
        "files_inspected": ["src/example.rs"], "files_changed": [],
        "checks": [{"name": "unit", "claimed_outcome": "passed", "details": "just test (worker report only)", "evidence": []}],
        "blockers": [], "risks": ["No independent checks"], "follow_ups": [],
        "references": [{"kind": "artifact", "uri": "file:///missing/never-followed"}], "source_claim": None}


class SchemaTests(unittest.TestCase):
    def test_valid_claim_is_independent_and_does_not_claim_verification(self):
        value = claim()
        result = handoffs.validate(value)
        result["findings"][0]["claim"] = "changed"
        self.assertNotEqual(result, value)

    def test_schema_rejects_grants_status_authority_and_unknown_fields(self):
        for key, value in (("authorization", "merge"), ("verified", True), ("status", "done"),
                           ("attempt_id", "prepared-attempt"), ("native_accepted", True)):
            with self.subTest(key=key), self.assertRaises(handoffs.HandoffError):
                handoffs.validate({**claim(), key: value})

    def test_invalid_types_limits_and_paths(self):
        changes = [("schema_version", True), ("confidence", []), ("claimed_status", "verified"),
                   ("summary", ""), ("summary", "\x1b[31m"), ("summary", "x"*16385),
                   ("summary", "\ud800"), ("findings", [None]), ("checks", [None]),
                   ("references", [None]), ("files_changed", ["src/file"]),
                   ("files_inspected", ["../escape"]), ("files_inspected", ["/absolute"]),
                   ("files_inspected", ["a//b"]), ("files_inspected", ["a/./b"]),
                   ("files_inspected", ["a\\b"]), ("files_inspected", ["."]),
                   ("files_inspected", ["same", "same"]), ("risks", ["x"]*65)]
        for key, value in changes:
            with self.subTest(key=key, value=repr(value)[:50]), self.assertRaises(handoffs.HandoffError):
                handoffs.validate({**claim(), key: value})

    def test_source_claim_honestly_supports_uncommitted_workers(self):
        value = claim()
        value["files_changed"] = ["src/file"]
        value["source_claim"] = {"state": "uncommitted", "repository_id": None,
                                 "base_sha": "a"*40, "source_sha": None, "tree_sha": None}
        self.assertEqual(handoffs.validate(value), value)
        value["source_claim"].update(state="committed", source_sha="b"*40, tree_sha="c"*40)
        self.assertEqual(handoffs.validate(value), value)
        value["source_claim"]["tree_sha"] = "d"*64
        with self.assertRaises(handoffs.HandoffError):
            handoffs.validate(value)

    def test_strict_json_rejects_duplicate_keys_nonfinite_unicode_and_depth(self):
        for raw in (b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":1e999}', b'\xff', b'{',
                    b'['*17+b'0'+b']'*17, b'{"x":"\\ud800"}'):
            with self.subTest(raw=raw), self.assertRaises(handoffs.HandoffError):
                handoffs._json(raw)


class HandoffFixture(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace, "--blank", "--agents", "operator")
        self.invoke("task", "add", self.workspace, "Review worker result", "--owner", "operator")
        revision, tasks = coven.read_task_snapshot(self.workspace)
        self.value = claim(tasks[0]["id"], revision)
        self.file = self.root / "handoff.json"
        self.write()

    def write(self):
        self.file.write_text(json.dumps(self.value, ensure_ascii=False)+"\n", encoding="utf-8")
        self.digest = hashlib.sha256(self.file.read_bytes()).hexdigest()

    def collect(self):
        return handoffs.collect(self.workspace, self.file, expected_sha256=self.digest,
                                expected_tasks_revision=self.value["task"]["revision"])


class CollectionTests(HandoffFixture):
    def setUp(self):
        super().setUp()
        for name in ("run", "Popen"):
            patcher = mock.patch.object(subprocess, name, side_effect=AssertionError("No native/Git/check execution"))
            guard = patcher.start()
            self.addCleanup(patcher.stop)
            self.addCleanup(guard.assert_not_called)

    def call(self, *args, expected=0):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(list(map(str, args))), expected)
        return json.loads(output.getvalue())

    def test_collect_is_local_evidence_only_and_preserves_all_prior_workspace_bytes(self):
        before = snapshot(self.workspace)
        result = self.collect()
        after = snapshot(self.workspace)
        self.assertEqual({k: after[k] for k in before}, before)
        self.assertTrue(all(k.startswith("output/handoffs") for k in set(after)-set(before)))
        self.assertEqual(result["assessment"], "collected_unverified")
        self.assertEqual(result["effect"], "local_artifact_capture_only")
        self.assertEqual(result["authorization"], "none")
        self.assertFalse(result["dispatchable"])
        self.assertEqual(result["verification"]["task_completion"], "unestablished")
        self.assertEqual(coven.read_task_snapshot(self.workspace)[1][0]["status"], "assigned")
        self.assertEqual(Path(result["artifact_path"]).stat().st_mode & 0o777, 0o600)

    def test_same_input_collection_is_idempotent_without_overwriting(self):
        first = self.collect()
        before = snapshot(self.workspace)
        second = self.collect()
        self.assertEqual(first["collection_id"], second["collection_id"])
        self.assertEqual(second["publication"], "existing")
        self.assertEqual(snapshot(self.workspace), before)

    def test_archive_retains_exact_input_after_worker_rewrites_it(self):
        original = self.file.read_bytes()
        result = self.collect()
        self.file.write_text("changed")
        report = handoffs.inspect(self.workspace, result["collection_id"])
        self.assertEqual(report["handoff"], self.value)
        self.assertEqual(report["handoff_sha256"], hashlib.sha256(original).hexdigest())
        archive = json.loads(Path(result["artifact_path"]).read_text())
        self.assertEqual(archive["payload"]["handoff_json"].encode("utf-8"), original)

    def test_raw_input_hash_pin_rejects_file_drift_before_effects(self):
        before = snapshot(self.workspace)
        self.file.write_text(self.file.read_text()+" ")
        with self.assertRaisesRegex(handoffs.HandoffError, "input_changed"):
            self.collect()
        self.assertEqual(snapshot(self.workspace), before)

    def test_stale_task_and_wrong_owner_fail_before_archive_creation(self):
        before = snapshot(self.workspace)
        self.value["task"]["owner"] = "other"
        self.write()
        with self.assertRaisesRegex(handoffs.HandoffError, "owner_mismatch"):
            self.collect()
        self.value["task"]["owner"] = "operator"
        self.write()
        self.invoke("task", "assign", self.workspace, self.value["task"]["id"], "operator")
        with self.assertRaisesRegex(handoffs.HandoffError, "stale_handoff"):
            self.collect()
        self.assertFalse((self.workspace / "output/handoffs").exists())
        self.assertEqual(before["state/agents.json"], snapshot(self.workspace)["state/agents.json"])

    def test_current_roster_is_not_inferred_from_worker_claim(self):
        config = self.workspace / "coven.json"
        value = json.loads(config.read_text())
        value["agents"] = ["different"]
        config.write_text(json.dumps(value))
        with self.assertRaisesRegex(handoffs.HandoffError, "owner_mismatch"):
            self.collect()

    def test_imported_task_authority_not_broken_projection_is_used(self):
        revision = coven.read_task_snapshot(self.workspace)[0]
        self.invoke("state", "import-tasks", self.workspace, "--expect-revision", revision, "--json")
        self.value["task"]["revision"] = coven.read_task_snapshot(self.workspace)[0]
        self.write()
        (self.workspace / "state/tasks.json").write_text("broken projection")
        result = self.collect()
        self.assertEqual(result["task_binding"]["revision"], self.value["task"]["revision"])
        self.assertEqual((self.workspace / "state/tasks.json").read_text(), "broken projection")

    def test_corrupt_ledger_and_pending_import_never_fall_back(self):
        (self.workspace / "state/task-ledger").mkdir()
        with self.assertRaisesRegex(handoffs.HandoffError, "pending_task_authority"):
            self.collect()
        (self.workspace / "state/task-authority.json").write_text("{}")
        with self.assertRaises(storage.StorageError):
            self.collect()
        self.assertFalse((self.workspace / "output/handoffs").exists())

    def test_capture_does_not_interpret_unrelated_corrupt_control_as_permission(self):
        (self.workspace / "state/execution").mkdir()
        (self.workspace / "state/execution/control.json").write_text("unavailable execution authority")
        (self.workspace / "state/actions").mkdir()
        (self.workspace / "state/actions/control.json").write_text("unavailable launch authority")
        before = snapshot(self.workspace)
        self.collect()
        after = snapshot(self.workspace)
        self.assertEqual({k: after[k] for k in before}, before)

    def test_collection_cannot_release_or_complete_a_real_prepared_attempt(self):
        task = self.value["task"]
        self.invoke("execution", "enable", self.workspace, "--expect-execution-revision", task_attempts.DISABLED,
                    "--request-id", "enable", "--json")
        self.invoke("execution", "enroll", self.workspace, task["id"], "--expect-execution-revision",
                    task_attempts.Store(self.workspace).read().revision, "--expect-tasks-revision", task["revision"],
                    "--request-id", "enroll", "--json")
        intent = self.root / "intent.json"
        intent.write_text(json.dumps({"schema_version": 1, "kind": "task-intent", "task_id": task["id"],
            "owner": task["owner"], "acceptance_criteria": [{"id": "review", "text": "Review independently"}]}))
        self.invoke("task", "attempt", "prepare", self.workspace, task["id"], "--intent", intent,
                    "--expect-execution-revision", task_attempts.Store(self.workspace).read().revision,
                    "--expect-tasks-revision", task["revision"], "--request-id", "prepare", "--json")
        before = snapshot(self.workspace)
        self.collect()
        after = snapshot(self.workspace)
        self.assertEqual({k: after[k] for k in before}, before)
        self.invoke("task", "assign", self.workspace, task["id"], task["owner"], error="prepared attempt blocks")

    def test_task_revision_is_rechecked_inside_the_shared_lock(self):
        original = handoffs._context
        first = []
        def race(*args):
            result = original(*args)
            if not first:
                first.append(True)
                self.invoke("task", "assign", self.workspace, self.value["task"]["id"], "operator")
            return result
        with mock.patch.object(handoffs, "_context", side_effect=race), self.assertRaisesRegex(handoffs.HandoffError, "stale_handoff"):
            self.collect()
        self.assertFalse((self.workspace / "output/handoffs").exists())

    def test_moving_workspace_preserves_historical_capture_without_identity_adoption(self):
        old_root = str(self.workspace.resolve())
        result = self.collect()
        moved = self.root / "moved"
        self.workspace.rename(moved)
        inspected = handoffs.inspect(moved, result["collection_id"])
        self.assertEqual(inspected["workspace_at_collection"], old_root)
        self.assertEqual(inspected["current_task_binding"], "not_checked")

    def test_list_is_a_summary_not_automatic_result_or_transcript_import(self):
        self.collect()
        result = handoffs.list_collections(self.workspace)
        self.assertEqual(result["selected_collection_count"], 1)
        row, = result["collections"]
        self.assertNotIn("references", row)
        self.assertNotIn("checks", row)
        self.assertNotIn("handoff", row)

    def test_historical_inspect_and_list_do_not_read_current_task_state_or_create_locks(self):
        result = self.collect()
        (self.workspace / "state/tasks.json").write_text("unreadable current source")
        (self.workspace / "state/.workspace.lock").unlink()
        before = snapshot(self.workspace)
        inspected = handoffs.inspect(self.workspace, result["collection_id"])
        listed = handoffs.list_collections(self.workspace, task_id=self.value["task"]["id"])
        self.assertEqual(inspected["current_task_binding"], "not_checked")
        self.assertEqual(len(listed["collections"]), 1)
        self.assertEqual(snapshot(self.workspace), before)

    def test_symlink_fifo_and_oversized_input_never_collect(self):
        original = self.file.read_bytes()
        self.file.unlink()
        self.file.symlink_to(self.workspace / "goal.md")
        with self.assertRaises(storage.StorageError):
            self.collect()
        self.file.unlink()
        os.mkfifo(self.file)
        with self.assertRaises(storage.StorageError):
            self.collect()
        self.file.unlink()
        self.file.write_bytes(b"x"*(handoffs.MAX_INPUT+1))
        with self.assertRaises(storage.StorageError):
            self.collect()
        self.assertFalse((self.workspace / "output/handoffs").exists())
        self.file.write_bytes(original)

    def test_output_symlink_cannot_redirect_capture(self):
        outside = self.root / "outside"
        outside.mkdir()
        (self.workspace / "output/handoffs").symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(handoffs.HandoffError, "unsafe_archive_path"):
            self.collect()
        self.assertEqual(list(outside.iterdir()), [])

    def test_tampered_archive_is_rejected_and_never_overwritten(self):
        result = self.collect()
        path = Path(result["artifact_path"])
        archive = json.loads(path.read_text())
        archive["payload"]["handoff_json"] += " "
        path.write_text(json.dumps(archive))
        before = path.read_bytes()
        with self.assertRaises(storage.StorageError):
            handoffs.inspect(self.workspace, result["collection_id"])
        with self.assertRaises(storage.StorageError):
            self.collect()
        self.assertEqual(path.read_bytes(), before)

    def test_list_filter_does_not_hide_an_unrelated_bad_archive(self):
        self.collect()
        (self.workspace / "output/handoffs" / ("f"*64+".json")).write_text("bad")
        with self.assertRaises(storage.StorageError):
            handoffs.list_collections(self.workspace, task_id="unmatched")

    def test_capacity_refuses_new_capture_but_allows_existing(self):
        with mock.patch.object(handoffs, "MAX_ENTRIES", 2):
            first = self.collect()
            self.assertEqual(self.collect()["publication"], "existing")
            self.value["summary"] = "Different claim"
            self.write()
            with self.assertRaisesRegex(handoffs.HandoffError, "archive_capacity"):
                self.collect()
        self.assertEqual(len(handoffs.list_collections(self.workspace)["collections"]), 1)

    def test_known_orphan_temp_is_inert_and_never_promoted_or_removed(self):
        directory = self.workspace / "output/handoffs"
        directory.mkdir()
        orphan = directory / ".handoff-orphan.tmp"
        orphan.write_text("incomplete")
        self.collect()
        self.assertEqual(orphan.read_text(), "incomplete")
        self.assertEqual(len(handoffs.list_collections(self.workspace)["collections"]), 1)

    def test_unlinked_owned_temp_during_scan_is_not_a_failed_collection(self):
        directory = self.workspace / "output/handoffs"
        directory.mkdir()
        temporary = directory / ".handoff-racing.tmp"
        temporary.write_text("inert")
        original = Path.stat
        removed = []
        def disappear(path, *args, **kwargs):
            if path == temporary and kwargs.get("follow_symlinks", True) and not removed:
                removed.append(True)
                temporary.unlink()
            return original(path, *args, **kwargs)
        with mock.patch.object(Path, "stat", new=disappear):
            result = self.collect()
        self.assertEqual(result["delivery"], "committed")
        self.assertEqual(removed, [True])

    def test_existing_file_sync_failure_is_uncertain_not_a_new_capture(self):
        first = self.collect()
        with mock.patch.object(handoffs.os, "fsync", side_effect=OSError("fixture sync")):
            retry = self.collect()
        self.assertEqual(retry["delivery"], "durability_uncertain")
        self.assertEqual(retry["publication"], "existing")
        self.assertEqual(retry["collection_id"], first["collection_id"])

    def test_cli_has_exit_two_after_known_publication_with_uncertain_durability(self):
        original = storage.sync_directory
        calls = []
        def fail_final(path):
            if Path(path) == self.workspace / "output/handoffs":
                calls.append(True)
                if len(calls) == 2:
                    raise OSError("fixture sync")
            return original(path)
        with mock.patch.object(storage, "sync_directory", side_effect=fail_final):
            result = self.call("handoff", "collect", self.workspace, "--file", self.file,
                "--expect-sha256", self.digest, "--expect-tasks-revision", self.value["task"]["revision"], "--json", expected=2)
        self.assertEqual(result["delivery"], "durability_uncertain")

    def test_prepublication_file_sync_failure_leaves_no_final_archive(self):
        original = os.fsync
        def fail_file(fd):
            if stat.S_ISREG(os.fstat(fd).st_mode):
                raise OSError("fixture file sync failure")
            return original(fd)
        with mock.patch.object(handoffs.os, "fsync", side_effect=fail_file), self.assertRaises(storage.StorageError):
            self.collect()
        self.assertEqual(list((self.workspace / "output/handoffs").glob("*.json")), [])

    def test_postpublication_directory_sync_is_uncertain_and_retry_confirms(self):
        original = storage.sync_directory
        calls = []
        def fail_final(path):
            if Path(path) == self.workspace / "output/handoffs":
                calls.append(True)
                if len(calls) == 2:
                    raise OSError("fixture final sync failure")
            return original(path)
        with mock.patch.object(storage, "sync_directory", side_effect=fail_final):
            result = self.collect()
        self.assertEqual(result["delivery"], "durability_uncertain")
        self.assertTrue(Path(result["artifact_path"]).is_file())
        retry = self.collect()
        self.assertEqual(retry["delivery"], "committed")
        self.assertEqual(retry["collection_id"], result["collection_id"])

    def test_cli_validate_collect_inspect_and_list_surfaces_are_explicit(self):
        validated = self.call("handoff", "validate", self.file, "--json")
        collected = self.call("handoff", "collect", self.workspace, "--file", self.file,
            "--expect-sha256", validated["handoff_sha256"], "--expect-tasks-revision", self.value["task"]["revision"], "--json")
        inspected = self.call("handoff", "inspect", self.workspace, collected["collection_id"], "--json")
        listed = self.call("handoff", "list", self.workspace, "--task", self.value["task"]["id"], "--json")
        self.assertEqual(inspected["assessment"], "historical_unverified")
        self.assertEqual(listed["observed_collection_count"], 1)


class ProcessTests(HandoffFixture):
    def test_competing_python_collectors_capture_only_one_artifact(self):
        code = "from runner.handoffs import collect; import json,sys; print(json.dumps(collect(sys.argv[1],sys.argv[2],expected_sha256=sys.argv[3],expected_tasks_revision=sys.argv[4])))"
        env = {**os.environ, "PYTHONDONTWRITEBYTECODE": "1"}
        args = [sys.executable, "-c", code, str(self.workspace), str(self.file), self.digest, self.value["task"]["revision"]]
        processes = [subprocess.Popen(args, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) for _ in range(2)]
        try:
            results = []
            for process in processes:
                output, error = process.communicate(timeout=15)
                self.assertEqual(process.returncode, 0, error)
                results.append(json.loads(output))
            self.assertEqual(results[0]["collection_id"], results[1]["collection_id"])
            self.assertEqual({item["publication"] for item in results}, {"created", "existing"})
        finally:
            for process in processes:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=5)
                for stream in (process.stdout, process.stderr):
                    stream.close()

    def test_real_wrapper_validates_without_runtime_dependency(self):
        env = {**os.environ, "COVEN_HOME": str(ROOT), "PYTHONDONTWRITEBYTECODE": "1"}
        result = subprocess.run([str(ROOT / "bin/coven"), "handoff", "validate", str(self.file), "--json"],
                                cwd=self.root, env=env, capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["assessment"], "valid_unverified")
