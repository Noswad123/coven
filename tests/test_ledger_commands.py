"""Opt-in authority, recoverable imports, and cross-process committed receipts."""

import contextlib
import io
import json
from pathlib import Path
import select
import subprocess
import sys
from unittest import mock

from runner import coven, storage, task_ledger
from test_coven import CovenTestCase, ROOT, snapshot


class LedgerCommandTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace)

    def listing(self, workspace=None):
        return json.loads(self.invoke("task", "list", workspace or self.workspace, "--json"))

    def activate(self):
        before = self.listing()["revision"]
        result = json.loads(self.invoke("state", "import-tasks", self.workspace, "--expect-revision", before, "--json"))
        self.assertEqual(result["outcome"], "committed")
        return result

    def spawn(self, code, *args):
        process = subprocess.Popen([sys.executable, "-c", code, *map(str, args)], cwd=ROOT,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        def cleanup():
            if process.poll() is None:
                process.kill()
            process.communicate(timeout=10)
        self.addCleanup(cleanup)
        return process

    def wait(self, process, line):
        self.assertTrue(select.select([process.stdout], [], [], 15)[0], "child did not signal readiness")
        self.assertEqual(process.stdout.readline().strip(), line)

    def test_import_is_explicit_backed_up_and_read_only_inspection_has_no_effects(self):
        source = (self.workspace / "state/tasks.json").read_bytes()
        metadata = (self.workspace / "coven.json").read_bytes()
        before = snapshot(self.workspace)
        self.invoke("task", "add", self.workspace, "No fake dedup", "--request-id", "key", error="explicit state import-tasks")
        self.assertEqual(snapshot(self.workspace), before)
        imported = self.activate()
        self.assertEqual(imported["authority"], "task-ledger-v1")
        base = self.workspace / "state/task-ledger" / imported["epoch_id"]
        self.assertEqual((base / "baseline.tasks.bytes").read_bytes(), source)
        self.assertEqual((self.workspace / "coven.json").read_bytes(), metadata)
        observed = snapshot(self.workspace)
        inspection = json.loads(self.invoke("state", "inspect", self.workspace, "--json"))
        self.assertEqual(inspection["revision"], imported["revision"])
        self.assertEqual(snapshot(self.workspace), observed)
        self.invoke("task", "add", self.workspace, "Missing key", error="requires --request-id")
        self.assertEqual(snapshot(self.workspace), observed)

    def test_frozen_retry_survives_later_state_and_roster_changes(self):
        initial = self.activate()
        args = ("task", "add", self.workspace, "Receipt task", "--owner", "JAMAL", "--request-id", "request-1",
                "--expect-revision", initial["revision"], "--json")
        first = json.loads(self.invoke(*args))
        task_id = first["result"]["task"]["id"]
        self.invoke("task", "assign", self.workspace, task_id, "eric", "--request-id", "assign-2")
        team = self.read_json("coven.json")
        team["agents"] = ["eric"]  # The original owner selector no longer resolves.
        (self.workspace / "coven.json").write_text(json.dumps(team))
        before = snapshot(self.workspace)
        retry = json.loads(self.invoke(*args))
        self.assertTrue(retry["delivery"]["replayed"])
        self.assertEqual(retry["result"], first["result"])
        self.assertEqual(retry["result"]["task"]["owner"], "jamal")
        self.assertEqual(next(task for task in self.listing()["tasks"] if task["id"] == task_id)["owner"], "eric")
        self.assertEqual(snapshot(self.workspace), before)

    def test_changed_binding_or_revision_cannot_reuse_a_key(self):
        self.activate()
        self.invoke("task", "add", self.workspace, "Original", "--request-id", "key")
        before = snapshot(self.workspace)
        for extra in (("Changed",), ("Original", "--notes", "Changed"),
                      ("Original", "--expect-revision", self.listing()["revision"])):
            self.invoke("task", "add", self.workspace, *extra, "--request-id", "key", error="changed binding")
            self.assertEqual(snapshot(self.workspace), before)

    def test_diverged_projection_reads_authority_and_repair_backs_up_exact_edits(self):
        self.activate()
        args = ("task", "add", self.workspace, "Authoritative", "--request-id", "key", "--json")
        frozen = json.loads(self.invoke(*args))["result"]
        corrupt = b"Uncommitted user edit\xff"
        path = self.workspace / "state/tasks.json"
        path.write_bytes(corrupt)
        self.assertEqual(self.listing()["projection"]["status"], "diverged")
        self.assertIn("Authoritative", self.invoke("status", self.workspace))
        self.assertEqual(path.read_bytes(), corrupt)  # Rendering never repairs task projections.
        retry = json.loads(self.invoke(*args))
        self.assertEqual(retry["result"], frozen)
        self.assertTrue(retry["delivery"]["warnings"])
        self.invoke("task", "add", self.workspace, "Blocked", "--request-id", "new-key", error="projection")
        repaired = json.loads(self.invoke("state", "recover-tasks", self.workspace, "--repair-projection", "--json"))
        self.assertEqual((Path(repaired["recovery"]["backup_path"]) / "tasks.bytes").read_bytes(), corrupt)
        self.assertEqual(self.listing()["projection"]["status"], "current")
        self.invoke("task", "add", self.workspace, "After repair", "--request-id", "new-key")

    def test_unreadable_projection_diagnostics_do_not_suppress_receipts(self):
        self.activate()
        args = ("task", "add", self.workspace, "Task", "--request-id", "key", "--json")
        frozen = json.loads(self.invoke(*args))["result"]
        original = task_ledger.LedgerStore._read
        def unreadable(store, relative, *a, **kw):
            if relative == "state/tasks.json":
                raise task_ledger.LedgerError("injected EIO reading projection")
            return original(store, relative, *a, **kw)
        with mock.patch.object(task_ledger.LedgerStore, "_read", new=unreadable):
            retry = json.loads(self.invoke(*args))
            self.assertEqual(retry["result"], frozen)
            self.assertEqual(retry["delivery"]["projection"]["status"], "unreadable")
            inspection = json.loads(self.invoke("state", "inspect", self.workspace, "--json"))
            self.assertEqual(inspection["projection"]["status"], "unreadable")
            self.invoke("task", "add", self.workspace, "Blocked", "--request-id", "new-key", error="projection")

    def test_postcommit_projection_read_failure_is_a_delivery_warning(self):
        self.activate()
        original = task_ledger.LedgerStore._read
        def after_commit(store, relative, *a, **kw):
            if relative == "state/tasks.json" and list((store.workspace / "state/task-ledger").glob("*/frames/00000000000000000001.frame")):
                raise task_ledger.LedgerError("injected postcommit projection read failure")
            return original(store, relative, *a, **kw)
        with mock.patch.object(task_ledger.LedgerStore, "_read", new=after_commit):
            result = json.loads(self.invoke("task", "add", self.workspace, "Committed", "--request-id", "key", "--json"))
        self.assertEqual(result["result"]["outcome"], "committed")
        self.assertEqual(result["delivery"]["outcome"], "committed")
        self.assertEqual(result["delivery"]["projection"]["status"], "unreadable")
        self.assertEqual(self.listing()["sequence"], 1)

    def test_raw_projection_path_io_errors_are_diagnostics_not_lost_receipts(self):
        self.activate()
        args = ("task", "add", self.workspace, "Task", "--request-id", "key", "--json")
        frozen = json.loads(self.invoke(*args))["result"]
        original = task_ledger.LedgerStore.path
        def fail_path(store, relative, *a, **kw):
            if str(relative) == "state/tasks.json":
                raise OSError("injected raw projection metadata EIO")
            return original(store, relative, *a, **kw)
        with mock.patch.object(task_ledger.LedgerStore, "path", new=fail_path):
            retry = json.loads(self.invoke(*args))
            self.assertEqual(retry["result"], frozen)
            self.assertEqual(retry["delivery"]["projection"]["status"], "unreadable")
            self.assertEqual(json.loads(self.invoke("state", "inspect", self.workspace, "--json"))["projection"]["status"], "unreadable")
            self.invoke("task", "add", self.workspace, "Blocked", "--request-id", "new-key", error="projection")
        self.assertEqual(self.listing()["sequence"], 1)

    def test_postcommit_raw_projection_path_failure_still_delivers_commit(self):
        self.activate()
        original = task_ledger.LedgerStore.path
        def fail_after_frame(store, relative, *a, **kw):
            if str(relative) == "state/tasks.json" and list((store.workspace / "state/task-ledger").glob("*/frames/00000000000000000001.frame")):
                raise OSError("injected postcommit metadata EIO")
            return original(store, relative, *a, **kw)
        with mock.patch.object(task_ledger.LedgerStore, "path", new=fail_after_frame):
            result = json.loads(self.invoke("task", "add", self.workspace, "Committed", "--request-id", "key", "--json"))
        self.assertEqual(result["result"]["outcome"], "committed")
        self.assertEqual(result["delivery"]["projection"]["status"], "unreadable")

    def test_unsafe_projection_is_not_followed_or_allowed_to_block_receipt_reads(self):
        self.activate()
        args = ("task", "add", self.workspace, "Task", "--request-id", "key", "--json")
        frozen = json.loads(self.invoke(*args))["result"]
        target = self.root / "outside-task-file"
        target.write_text("Do not read or overwrite")
        path = self.workspace / "state/tasks.json"
        path.unlink()
        path.symlink_to(target)
        retry = json.loads(self.invoke(*args))
        self.assertEqual(retry["result"], frozen)
        self.assertEqual(retry["delivery"]["projection"]["status"], "unreadable")
        self.invoke("task", "add", self.workspace, "Blocked", "--request-id", "new-key", error="projection")
        self.invoke("state", "recover-tasks", self.workspace, "--repair-projection", error="unsafe path")
        self.assertEqual(target.read_text(), "Do not read or overwrite")
        self.assertTrue(path.is_symlink())

    def test_observational_history_corruption_does_not_gate_ledger_commit_or_retry(self):
        self.activate()
        for name in ("events", "messages"):
            (self.workspace / f"logs/{name}.jsonl").write_bytes(b"broken\n")
        args = ("task", "add", self.workspace, "Independent of history", "--request-id", "key", "--json")
        first = json.loads(self.invoke(*args))
        self.assertEqual(first["delivery"]["outcome"], "committed")
        self.assertTrue(any("history" in warning for warning in first["delivery"]["warnings"]))
        second = json.loads(self.invoke(*args))
        self.assertTrue(second["delivery"]["replayed"])
        self.assertEqual(second["result"], first["result"])

    def test_concurrent_identical_keys_commit_once_in_fresh_processes(self):
        initial = self.activate()
        code = """
import sys
from runner import coven
print('ready', flush=True)
raise SystemExit(coven.main(sys.argv[1:]))
"""
        args = ("task", "add", self.workspace, "One effect", "--request-id", "same-key",
                "--expect-revision", initial["revision"], "--json")
        with coven.workspace_mutation(self.workspace):
            children = [self.spawn(code, *args) for _ in range(2)]
            for child in children:
                self.wait(child, "ready")
        results = []
        for child in children:
            output, error = child.communicate(timeout=20)
            self.assertEqual(child.returncode, 0, error)
            results.append(json.loads(output))
        self.assertEqual(results[0]["result"], results[1]["result"])
        self.assertEqual(sorted(r["delivery"]["replayed"] for r in results), [False, True])
        self.assertEqual(self.listing()["sequence"], 1)

    def test_killed_after_frame_before_projection_retries_original_receipt(self):
        initial = self.activate()
        code = """
import sys
from runner import coven, task_ledger
def paused(*args):
    print('committed', flush=True)
    sys.stdin.readline()
task_ledger.LedgerStore.publish_projection = paused
raise SystemExit(coven.main(sys.argv[1:]))
"""
        args = ("task", "add", self.workspace, "Lost response", "--request-id", "lost-key",
                "--expect-revision", initial["revision"], "--json")
        process = self.spawn(code, *args)
        self.wait(process, "committed")
        process.kill()
        process.communicate(timeout=10)
        retry = json.loads(self.invoke(*args))
        self.assertTrue(retry["delivery"]["replayed"])
        self.assertEqual(retry["result"]["task"]["title"], "Lost response")
        self.assertEqual(retry["delivery"]["projection"]["status"], "diverged")
        self.assertEqual(self.listing()["sequence"], 1)
        self.invoke("state", "recover-tasks", self.workspace, "--repair-projection")

    def test_frame_publication_uncertainty_has_frozen_result_and_retry_confirms_it(self):
        self.activate()
        original = storage.sync_directory
        def fail_new_frame(path):
            if path.name == "frames" and (path / "00000000000000000001.frame").exists():
                raise OSError("injected frame directory sync failure")
            original(path)
        args = ["task", "add", str(self.workspace), "Uncertain frame", "--request-id", "key", "--json"]
        output = io.StringIO()
        with mock.patch.object(storage, "sync_directory", side_effect=fail_new_frame), contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(args), 2)
        uncertain = json.loads(output.getvalue())
        self.assertEqual(uncertain["delivery"]["outcome"], "durability_uncertain")
        retry = json.loads(self.invoke(*args))
        self.assertEqual(retry["result"], uncertain["result"])
        self.assertTrue(retry["delivery"]["replayed"])

    def test_interrupted_import_can_resume_without_adopting_old_events(self):
        revision = self.listing()["revision"]
        source = (self.workspace / "state/tasks.json").read_bytes()
        original = task_ledger.LedgerStore._publish
        def stop(store, relative, data, mode=0o600):
            if relative == task_ledger.MARKER:
                raise task_ledger.LedgerError("injected interruption before activation")
            return original(store, relative, data, mode)
        with mock.patch.object(task_ledger.LedgerStore, "_publish", new=stop):
            self.invoke("state", "import-tasks", self.workspace, "--expect-revision", revision, error="before activation")
        self.assertTrue(json.loads(self.invoke("state", "inspect", self.workspace, "--json"))["import_pending"])
        self.invoke("task", "add", self.workspace, "Blocked while pending", error="pending")
        result = json.loads(self.invoke("state", "recover-tasks", self.workspace, "--resume-import", "--expect-revision", revision, "--json"))
        self.assertEqual(result["authority"], "task-ledger-v1")
        self.assertEqual((self.workspace / "state/tasks.json").read_bytes(), source)

    def test_activation_directory_sync_uncertainty_requires_confirmation_not_reimport(self):
        revision = self.listing()["revision"]
        source = (self.workspace / "state/tasks.json").read_bytes()
        original = storage.sync_directory
        marker = (self.workspace / task_ledger.MARKER).resolve()
        def fail_after_activation(path):
            if path == marker.parent and marker.exists():
                raise OSError("injected activation directory sync failure")
            original(path)
        output = io.StringIO()
        with mock.patch.object(storage, "sync_directory", side_effect=fail_after_activation), contextlib.redirect_stdout(output):
            code = coven.main(["state", "import-tasks", str(self.workspace), "--expect-revision", revision, "--json"])
        self.assertEqual(code, 2)
        uncertain = json.loads(output.getvalue())
        self.assertEqual(uncertain["authority"], "task-ledger-v1")
        self.assertEqual(uncertain["outcome"], "durability_uncertain")
        self.invoke("state", "import-tasks", self.workspace, "--expect-revision", revision, error="already active")
        confirmed = json.loads(self.invoke("state", "recover-tasks", self.workspace, "--repair-projection", "--json"))
        self.assertEqual(confirmed["outcome"], "recovered")
        self.assertEqual(confirmed["epoch_id"], uncertain["epoch_id"])
        self.assertEqual((self.workspace / "state/tasks.json").read_bytes(), source)

    def test_pending_abort_and_inspection_do_not_require_valid_source(self):
        original = task_ledger.LedgerStore._publish
        def stop(store, relative, data, mode=0o600):
            if relative == task_ledger.MARKER:
                raise task_ledger.LedgerError("stopped")
            return original(store, relative, data, mode)
        for index, bad in enumerate((b"{truncated", b"\xff")):
            workspace = self.root / f"pending-{index}"
            self.invoke("init", workspace, "--blank")
            revision = self.listing(workspace)["revision"]
            with mock.patch.object(task_ledger.LedgerStore, "_publish", new=stop):
                self.invoke("state", "import-tasks", workspace, "--expect-revision", revision, error="stopped")
            (workspace / "state/tasks.json").write_bytes(bad)
            info = json.loads(self.invoke("state", "inspect", workspace, "--json"))
            self.assertTrue(info["import_pending"])
            self.assertIn("source_error", info)
            result = json.loads(self.invoke("state", "recover-tasks", workspace, "--abort-import", "--json"))
            archived = Path(result["preserved_artifacts"])
            self.assertEqual(len(list(archived.glob("*/baseline.tasks.bytes"))), 1)
            self.assertEqual((workspace / "state/tasks.json").read_bytes(), bad)
            self.assertFalse((workspace / "state/task-ledger").exists())

    def test_bad_marker_or_final_frame_never_falls_back_to_projection(self):
        imported = self.activate()
        marker = self.workspace / task_ledger.MARKER
        original = marker.read_bytes()
        marker.write_bytes(b"{}")
        self.invoke("task", "list", self.workspace, error="ledger fields")
        marker.write_bytes(original)
        self.invoke("task", "add", self.workspace, "Task", "--request-id", "key")
        frame = self.workspace / task_ledger.LEDGER / imported["epoch_id"] / "frames/00000000000000000001.frame"
        frame.write_bytes(b"broken")
        self.invoke("task", "list", self.workspace, error="strict JSON")
        self.invoke("state", "recover-tasks", self.workspace, "--repair-projection", error="strict JSON")

    def test_import_wins_race_without_allowing_a_queued_v1_snapshot_write(self):
        initial = self.listing()
        code = """
import contextlib, sys
from runner import coven, storage
original = storage.workspace_lock
@contextlib.contextmanager
def announced(*args, **kwargs):
    if kwargs.get('held') is None:
        print('waiting', flush=True)
    with original(*args, **kwargs) as held:
        yield held
storage.workspace_lock = announced
raise SystemExit(coven.main(sys.argv[1:]))
"""
        with coven.workspace_mutation(self.workspace) as held:
            child = self.spawn(code, "task", "add", self.workspace, "Old backend draft")
            self.wait(child, "waiting")
            task_ledger.LedgerStore(self.workspace).import_tasks(initial["revision"], held)
        _, error = child.communicate(timeout=20)
        self.assertNotEqual(child.returncode, 0)
        self.assertIn("requires --request-id", error)
        imported = self.listing()
        self.assertEqual(imported["sequence"], 0)
        self.assertEqual(imported["tasks"], initial["tasks"])

    def test_rejected_requests_are_not_cached_and_snapshot_tokens_are_not_ledger_tokens(self):
        old = self.listing()["revision"]
        current = self.activate()["revision"]
        self.invoke("task", "add", self.workspace, "New request", "--request-id", "key", "--expect-revision", old,
                    error="Stale tasks revision")
        self.assertEqual(self.listing()["sequence"], 0)
        result = json.loads(self.invoke("task", "add", self.workspace, "New request", "--request-id", "key",
                                       "--expect-revision", current, "--json"))
        self.assertEqual(result["result"]["sequence"], 1)
