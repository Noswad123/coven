"""Preparation-only attempts, durable receipts, and same-lock task guards."""

import contextlib
import io
import json
from pathlib import Path
import select
import subprocess
import sys
from unittest import mock

from runner import coven, storage, task_attempts, task_ledger, workers, launch
from test_coven import CovenTestCase, ROOT, snapshot


WORKER = """
import contextlib,sys
from runner import coven,storage
original=storage.workspace_lock
@contextlib.contextmanager
def announced(*args,**kwargs):
    if kwargs.get('held') is None: print('ready',flush=True)
    with original(*args,**kwargs) as held: yield held
storage.workspace_lock=announced
raise SystemExit(coven.main(sys.argv[1:]))
"""


class AttemptCommandTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace)
        self.intent_path = self.root / "operator-intent.json"
        self.write_intent()
        for target, name in ((coven.subprocess, "run"), (workers, "inspect_readiness"), (launch, "execute"), (launch, "stop")):
            guard = mock.patch.object(target, name, side_effect=AssertionError("Preparation must never perform native/launch/readiness operations"))
            guard.start()
            self.addCleanup(guard.stop)

    def write_intent(self, owner="jamal", task="core-chatbot"):
        self.intent_path.write_text(json.dumps({"schema_version": 1, "kind": "task-intent", "task_id": task,
            "owner": owner, "acceptance_criteria": [{"id": "evidence", "text": "Describe evidence; $(do-not-execute) is data."}]}))

    def tasks_revision(self):
        return json.loads(self.invoke("task", "list", self.workspace, "--json"))["revision"]

    def revision(self):
        view = task_attempts.Store(self.workspace).read()
        return task_attempts.DISABLED if view is None else view.revision

    def enable(self):
        return json.loads(self.invoke("execution", "enable", self.workspace, "--expect-execution-revision", task_attempts.DISABLED,
                                      "--request-id", "enable-1", "--json"))

    def enroll(self, task="core-chatbot", key="enroll-1", rebind=False):
        return json.loads(self.invoke("execution", "enroll", self.workspace, task, "--expect-execution-revision", self.revision(),
            "--expect-tasks-revision", self.tasks_revision(), "--request-id", key, *( ["--rebind-source"] if rebind else []), "--json"))

    def prepare_args(self, key="prepare-1"):
        return ["task", "attempt", "prepare", str(self.workspace), "core-chatbot", "--intent", str(self.intent_path),
                "--expect-execution-revision", self.revision(), "--expect-tasks-revision", self.tasks_revision(), "--request-id", key, "--json"]

    def prepare(self, key="prepare-1"):
        return json.loads(self.invoke(*self.prepare_args(key)))

    def ready(self):
        self.enable()
        self.enroll()
        return self.prepare()

    def cancel_args(self, attempt_id, key="cancel-1"):
        return ["task", "attempt", "cancel", str(self.workspace), attempt_id, "--reason", "Revise assignment before submission",
                "--expect-execution-revision", self.revision(), "--request-id", key, "--json"]

    def inspect(self):
        return json.loads(self.invoke("execution", "inspect", self.workspace, "--json"))

    def spawn(self, code, *args):
        child = subprocess.Popen([sys.executable, "-c", code, *map(str, args)], cwd=ROOT,
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        def cleanup():
            if child.poll() is None:
                child.kill()
            child.communicate(timeout=10)
        self.addCleanup(cleanup)
        return child

    def wait_ready(self, child, text="ready"):
        self.assertTrue(select.select([child.stdout], [], [], 15)[0], "child did not reach barrier")
        value = child.stdout.readline().strip()
        if value != text:
            self.fail(f"Expected {text!r}, got {value!r}: {child.communicate(timeout=10)}")

    def test_explicit_enable_enroll_prepare_changes_no_task_or_ledger_authority(self):
        before = snapshot(self.workspace)
        self.assertEqual(self.inspect()["execution"]["status"], "disabled")
        self.assertEqual(snapshot(self.workspace), before)
        source = (self.workspace / "state/tasks.json").read_bytes()
        logs = {name: (self.workspace / f"logs/{name}.jsonl").read_bytes() for name in ("events", "messages")}
        enabled = self.enable()
        self.assertEqual(enabled["authorization"], "none")
        self.assertEqual(self.inspect()["enrollments"], [])
        self.enroll()
        prepared = self.prepare()
        self.assertFalse(prepared["dispatchable"])
        self.assertEqual(prepared["result"]["attempt"]["complete_task_snapshot"]["owner"], "jamal")
        self.assertEqual(prepared["result"]["attempt"]["intent"]["acceptance_criteria"][0]["id"], "evidence")
        self.assertEqual((self.workspace / "state/tasks.json").read_bytes(), source)
        for name, data in logs.items():
            self.assertEqual((self.workspace / f"logs/{name}.jsonl").read_bytes(), data)
        inspection = self.inspect()
        self.assertEqual(inspection["attempts"][0]["source_binding"]["status"], "current")
        self.assertEqual(inspection["attempts"][0]["state"], "prepared")

    def test_cancel_reassign_reprepare_retains_old_attempt_and_original_receipt(self):
        self.enable()
        self.enroll()
        args = self.prepare_args()
        first = json.loads(self.invoke(*args))
        attempt_id = first["result"]["attempt_id"]
        before = snapshot(self.workspace)
        self.invoke("task", "assign", self.workspace, "core-chatbot", "jamal", error="prepared attempt blocks")
        self.assertEqual(snapshot(self.workspace), before)
        self.invoke(*self.cancel_args(attempt_id))
        self.invoke("task", "assign", self.workspace, "core-chatbot", "eric")
        self.write_intent(owner="eric")
        second = self.prepare("prepare-2")
        self.assertNotEqual(second["result"]["attempt_id"], attempt_id)
        self.assertEqual(second["result"]["attempt"]["previous_attempt_id"], attempt_id)
        self.assertEqual(second["result"]["attempt"]["attempt_number"], 2)
        self.write_intent()
        replay = json.loads(self.invoke(*args))
        self.assertEqual(replay["result"], first["result"])
        self.assertEqual(replay["result"]["state"], "prepared")
        self.assertEqual(replay["delivery"]["attempt_state"], "cancelled_before_submission")

    def test_source_corruption_does_not_hide_frozen_attempt_receipt_or_cancel(self):
        self.enable()
        self.enroll()
        args = self.prepare_args()
        prepared = json.loads(self.invoke(*args))
        attempt_id = prepared["result"]["attempt_id"]
        cancel = self.cancel_args(attempt_id)
        for relative in ("coven.json", "state/tasks.json", "state/agents.json", "state/runtime-launch-control.json"):
            (self.workspace / relative).write_bytes(b"broken\xff")
        before = snapshot(self.workspace)
        inspection = self.inspect()
        self.assertEqual(inspection["attempts"][0]["prepared"], prepared["result"]["attempt"])
        self.assertEqual(inspection["attempts"][0]["source_binding"]["status"], "source_unavailable")
        self.assertEqual(snapshot(self.workspace), before)
        self.assertEqual(json.loads(self.invoke(*args))["result"], prepared["result"])
        cancelled = json.loads(self.invoke(*cancel))
        self.assertEqual(cancelled["result"]["state"], "cancelled_before_submission")
        self.assertEqual(json.loads(self.invoke(*cancel))["result"], cancelled["result"])

    def test_nonfinite_or_surrogate_source_extensions_are_diagnostics_not_lost_snapshots(self):
        prepared = self.ready()
        tasks = self.read_json("state/tasks.json")
        for bad in (float("nan"), chr(0xD800)):
            with self.subTest(bad=repr(bad)):
                tasks[0]["unknown-broken-source"] = bad  # A different task, not the prepared one.
                (self.workspace / "state/tasks.json").write_text(json.dumps(tasks))
                before = snapshot(self.workspace)
                result = json.loads(self.invoke("task", "attempt", "inspect", self.workspace, prepared["result"]["attempt_id"], "--json"))
                self.assertEqual(result["attempt"]["prepared"], prepared["result"]["attempt"])
                self.assertEqual(result["attempt"]["source_binding"]["status"], "source_unavailable")
                self.assertEqual(snapshot(self.workspace), before)

    def test_changed_task_or_whole_source_revision_stales_but_never_releases_reservation(self):
        self.ready()
        self.invoke("task", "add", self.workspace, "Unrelated task")
        self.assertEqual(self.inspect()["attempts"][0]["source_binding"]["status"], "source_revision_changed")
        self.invoke("task", "assign", self.workspace, "core-chatbot", "eric", error="prepared attempt blocks")
        tasks = self.read_json("state/tasks.json")
        task = next(item for item in tasks if item["id"] == "core-chatbot")
        task["notes"] += " Manually edited source"
        (self.workspace / "state/tasks.json").write_text(json.dumps(tasks))
        self.assertEqual(self.inspect()["attempts"][0]["source_binding"]["status"], "task_changed")
        self.invoke("task", "assign", self.workspace, "core-chatbot", "eric", error="prepared attempt blocks")

    def test_task_import_requires_cancel_then_explicit_source_rebind(self):
        prepared = self.ready()
        before = snapshot(self.workspace)
        token = self.tasks_revision()
        self.invoke("state", "import-tasks", self.workspace, "--expect-revision", token, error="prepared attempt blocks")
        self.assertEqual(snapshot(self.workspace), before)
        self.invoke(*self.cancel_args(prepared["result"]["attempt_id"]))
        self.invoke("state", "import-tasks", self.workspace, "--expect-revision", token)
        self.assertEqual(self.inspect()["attempts"][0]["source_binding"]["status"], "source_authority_changed")
        self.invoke(*self.prepare_args("prepare-new-source"), error="current source identity")
        old_id = task_attempts.Store(self.workspace).read().enrollments["core-chatbot"]["enrollment_id"]
        self.enroll(key="explicit-rebind", rebind=True)
        second = self.prepare("prepare-new-source")
        self.assertEqual(second["result"]["attempt"]["enrollment_id"], old_id)
        self.assertEqual(second["result"]["attempt"]["attempt_number"], 2)
        self.assertEqual(json.loads(self.invoke("task", "list", self.workspace, "--json"))["sequence"], 0)

    def test_old_task_ledger_receipt_replays_before_execution_guard_even_if_control_bad(self):
        token = self.tasks_revision()
        self.invoke("state", "import-tasks", self.workspace, "--expect-revision", token)
        old_args = ("task", "assign", self.workspace, "core-chatbot", "eric", "--request-id", "assignment-before-prepare", "--json")
        old = json.loads(self.invoke(*old_args))
        self.write_intent(owner="eric")
        self.ready()
        before = self.tasks_revision()
        self.invoke("task", "assign", self.workspace, "core-chatbot", "jamal", "--request-id", "fresh-assignment", error="prepared attempt blocks")
        self.assertEqual(self.tasks_revision(), before)
        self.assertEqual(json.loads(self.invoke(*old_args))["result"], old["result"])
        (self.workspace / task_attempts.CONTROL).write_bytes(b"corrupt execution control")
        self.assertEqual(json.loads(self.invoke(*old_args))["result"], old["result"])
        self.invoke("task", "assign", self.workspace, "core-chatbot", "jamal", "--request-id", "fresh-assignment", error="control JSON")
        self.assertEqual(self.inspect()["execution"]["status"], "unavailable")

    def test_intent_content_not_path_is_bound_and_unknown_executable_fields_reject(self):
        self.enable()
        self.enroll()
        args = self.prepare_args()
        first = json.loads(self.invoke(*args))
        another = self.root / "same-document-different-path.json"
        another.write_text(json.dumps(json.loads(self.intent_path.read_text()), indent=4))
        same = [str(another) if value == str(self.intent_path) else value for value in args]
        self.assertEqual(json.loads(self.invoke(*same))["result"], first["result"])
        doc = json.loads(self.intent_path.read_text())
        doc["acceptance_criteria"][0]["text"] = "Different bound criteria"
        self.intent_path.write_text(json.dumps(doc))
        self.invoke(*args, error="binding conflict")
        before = snapshot(self.workspace)
        doc["argv"] = ["touch", str(self.root / "must-not-run")]
        self.intent_path.write_text(json.dumps(doc))
        self.invoke(*args, error="task-intent fields")
        self.assertEqual(snapshot(self.workspace), before)
        self.assertFalse((self.root / "must-not-run").exists())

    def test_inspection_never_creates_an_absent_lock_and_prepared_unknown_fields_remain(self):
        tasks = self.read_json("state/tasks.json")
        selected = next(item for item in tasks if item["id"] == "core-chatbot")
        selected.update(status="custom-legacy-status", custom={"nested": [1, {"value": "\u2003"}]})
        (self.workspace / "state/tasks.json").write_text(json.dumps(tasks))
        prepared = self.ready()
        self.assertEqual(prepared["result"]["attempt"]["complete_task_snapshot"], selected)
        (self.workspace / "state/.workspace.lock").unlink()  # Inactive disposable fixture only.
        before = snapshot(self.workspace)
        inspection = self.inspect()
        self.assertEqual(inspection["attempts"][0]["source_binding"]["status"], "source_unavailable")
        self.assertEqual(snapshot(self.workspace), before)

    def test_pending_enable_and_present_bad_control_fail_closed_without_reset(self):
        with mock.patch.object(storage, "atomic_replace_bytes", side_effect=storage.StorageError("injected before control replace")):
            self.invoke("execution", "enable", self.workspace, "--expect-execution-revision", task_attempts.DISABLED,
                        "--request-id", "enable", error="before control replace")
        self.assertEqual(self.inspect()["execution"]["status"], "pending")
        self.invoke("task", "assign", self.workspace, "core-chatbot", "eric", error="pending execution authority")
        orphan = self.workspace / task_attempts.DIR / "orphan.tmp"
        orphan.write_text("Preserve incomplete activation evidence")
        result = json.loads(self.invoke("execution", "recover", self.workspace, "--archive-pending", "--json"))
        self.assertEqual((Path(result["archive_path"]) / "orphan.tmp").read_text(), "Preserve incomplete activation evidence")
        self.enable()
        (self.workspace / task_attempts.CONTROL).write_text("{}")
        before = snapshot(self.workspace)
        self.invoke("execution", "recover", self.workspace, "--archive-pending", error="present control")
        self.invoke("task", "assign", self.workspace, "core-chatbot", "eric", error="authority fields")
        self.assertEqual(snapshot(self.workspace), before)

    def test_uncertain_prepare_and_cancel_never_release_guard_without_confirmation(self):
        self.enable()
        self.enroll()
        args = self.prepare_args()
        sync = storage.sync_directory
        def fail_execution_sync(path):
            if Path(path).name == "execution":
                raise OSError("execution directory fsync failed")
            sync(path)
        output = io.StringIO()
        with mock.patch.object(storage, "sync_directory", side_effect=fail_execution_sync), contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(args), 2)
        uncertain = json.loads(output.getvalue())
        retry = json.loads(self.invoke(*args))
        self.assertEqual(retry["result"], uncertain["result"])
        self.assertEqual(retry["delivery"]["outcome"], "committed")
        cancel = self.cancel_args(retry["result"]["attempt_id"])
        with mock.patch.object(storage, "sync_directory", side_effect=fail_execution_sync), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(coven.main(cancel), 2)
            before = (self.workspace / "state/tasks.json").read_bytes()
            self.invoke("task", "assign", self.workspace, "core-chatbot", "eric", error="durability uncertain")
            self.assertEqual((self.workspace / "state/tasks.json").read_bytes(), before)
        self.invoke("task", "assign", self.workspace, "core-chatbot", "eric")

    def test_concurrent_preparations_have_one_winner_or_one_idempotent_effect(self):
        self.enable()
        self.enroll()
        for identical in (False, True):
            with self.subTest(identical=identical):
                args = self.prepare_args("same-key" if identical else "first-key")
                second = list(args)
                if not identical:
                    second[second.index("--request-id") + 1] = "second-key"
                with coven.workspace_mutation(self.workspace):
                    children = [self.spawn(WORKER, *command) for command in (args, second)]
                    for child in children:
                        self.wait_ready(child)
                values = []
                for child in children:
                    out, error = child.communicate(timeout=20)
                    values.append((child.returncode, json.loads(out) if child.returncode == 0 else error))
                self.assertEqual(sorted(code for code, _ in values), [0, 0] if identical else [0, 1])
                prepared = next(value for code, value in values if code == 0)
                if identical:
                    self.assertEqual(values[0][1]["result"], values[1][1]["result"])
                self.invoke(*self.cancel_args(prepared["result"]["attempt_id"], key=f"cancel-{identical}"))

    def test_queued_assignment_cannot_overwrite_a_preparation_that_wins_the_lock(self):
        self.enable()
        self.enroll()
        before_tasks = (self.workspace / "state/tasks.json").read_bytes()
        with coven.workspace_mutation(self.workspace) as held:
            child = self.spawn(WORKER, "task", "assign", self.workspace, "core-chatbot", "eric")
            self.wait_ready(child)
            task_attempts.Store(self.workspace).mutate("prepare", {"task_id": "core-chatbot", "intent": json.loads(self.intent_path.read_text())},
                expected_execution_revision=self.revision(), expected_tasks_revision=self.tasks_revision(), request_id="prepare-under-lock",
                source_loader=lambda value: coven.execution_source(self.workspace, value), lock=held)
        _, error = child.communicate(timeout=20)
        self.assertNotEqual(child.returncode, 0)
        self.assertIn("prepared attempt blocks", error)
        self.assertEqual((self.workspace / "state/tasks.json").read_bytes(), before_tasks)

    def test_source_change_winning_before_queued_preparation_rejects_stale_input(self):
        self.enable()
        self.enroll()
        args = self.prepare_args()
        with coven.workspace_mutation(self.workspace):
            child = self.spawn(WORKER, *args)
            self.wait_ready(child)
            tasks = self.read_json("state/tasks.json")
            next(item for item in tasks if item["id"] == "core-chatbot")["owner"] = "eric"
            storage.atomic_replace_bytes(coven.managed_path(self.workspace, "state/tasks.json"), storage.encode_json(tasks))
        _, error = child.communicate(timeout=20)
        self.assertNotEqual(child.returncode, 0)
        self.assertIn("tasks revision conflict", error)
        self.assertEqual(task_attempts.Store(self.workspace).read().attempts, {})

    def test_low_level_ledger_import_resume_and_commit_respect_prepared_guards(self):
        prepared = self.ready()
        store = task_ledger.LedgerStore(self.workspace)
        with coven.workspace_mutation(self.workspace) as held:
            with self.assertRaises(task_attempts.AttemptError):
                store.import_tasks(self.tasks_revision(), held)
            (self.workspace / "state/task-ledger").mkdir()
            with self.assertRaisesRegex(task_attempts.AttemptError, "prepared attempt blocks"):
                store.resume_import(self.tasks_revision(), held)
            (self.workspace / "state/task-ledger").rmdir()
        self.invoke(*self.cancel_args(prepared["result"]["attempt_id"]))
        self.invoke("state", "import-tasks", self.workspace, "--expect-revision", self.tasks_revision())
        self.enroll(key="rebind-low-level", rebind=True)
        self.prepare("prepared-ledger")
        with coven.workspace_mutation(self.workspace) as held:
            view = store.read()
            task = next(item for item in view.tasks if item["id"] == "core-chatbot").copy()
            task.update(owner="eric", status="assigned", updated_at="later")
            binding = task_ledger.request_binding("assign", {"task_id": "core-chatbot", "agent": "eric"}, None)
            with self.assertRaisesRegex(task_attempts.AttemptError, "prepared attempt blocks"):
                store.commit(view, "direct-store-assignment", binding, task, held)

    def test_process_death_before_or_after_control_replacement_retries_same_request(self):
        self.enable()
        self.enroll()
        for phase in ("before", "after"):
            with self.subTest(phase=phase):
                args = self.prepare_args(f"crash-{phase}")
                code = """
import sys
from runner import coven,storage,task_attempts
phase=sys.argv[1]
if phase=='before':
    original=storage.os.replace
    def paused(*args):
        print('paused',flush=True);sys.stdin.readline();return original(*args)
    storage.os.replace=paused
else:
    original=task_attempts.Store.confirm_durability
    def paused(self,view,lock):
        print('paused',flush=True);sys.stdin.readline();return original(self,view,lock)
    task_attempts.Store.confirm_durability=paused
raise SystemExit(coven.main(sys.argv[2:]))
"""
                child = self.spawn(code, phase, *args)
                self.wait_ready(child, "paused")
                child.kill()
                child.communicate(timeout=10)
                observed = task_attempts.Store(self.workspace).read()
                expected = observed.receipts.get(f"crash-{phase}")
                result = json.loads(self.invoke(*args))
                if phase == "before":
                    self.assertIsNone(expected)
                    self.assertFalse(result["delivery"]["replayed"])
                else:
                    self.assertEqual(result["result"], expected["result"])
                    self.assertTrue(result["delivery"]["replayed"])
                self.invoke(*self.cancel_args(result["result"]["attempt_id"], key=f"cancel-crash-{phase}"))

    def test_ledger_assignment_and_preparation_races_obey_both_lock_orders(self):
        for prepare_first in (True, False):
            with self.subTest(prepare_first=prepare_first):
                self.workspace = self.root / f"ledger-race-{prepare_first}"
                self.invoke("init", self.workspace)
                self.invoke("state", "import-tasks", self.workspace, "--expect-revision", self.tasks_revision())
                self.enable()
                self.enroll()
                prepare = self.prepare_args("queued-prepare")
                ledger = task_ledger.LedgerStore(self.workspace)
                with coven.workspace_mutation(self.workspace) as held:
                    if prepare_first:
                        child = self.spawn(WORKER, "task", "assign", self.workspace, "core-chatbot", "eric", "--request-id", "queued-assignment")
                    else:
                        child = self.spawn(WORKER, *prepare)
                    self.wait_ready(child)
                    if prepare_first:
                        task_attempts.Store(self.workspace).mutate("prepare", {"task_id": "core-chatbot", "intent": json.loads(self.intent_path.read_text())},
                            expected_execution_revision=self.revision(), expected_tasks_revision=self.tasks_revision(), request_id="winning-prepare",
                            source_loader=lambda value: coven.execution_source(self.workspace, value), lock=held)
                    else:
                        view = ledger.read()
                        task = next(item for item in view.tasks if item["id"] == "core-chatbot").copy()
                        task.update(owner="eric", status="assigned", updated_at="later")
                        binding = task_ledger.request_binding("assign", {"task_id": "core-chatbot", "agent": "eric"}, None)
                        committed = ledger.commit(view, "winning-assignment", binding, task, held)
                        ledger.publish_projection(committed, held)
                _, error = child.communicate(timeout=20)
                self.assertNotEqual(child.returncode, 0)
                self.assertIn("prepared attempt blocks" if prepare_first else "tasks revision conflict", error)
                self.assertEqual(ledger.read().sequence, 0 if prepare_first else 1)
                self.assertEqual(len(task_attempts.Store(self.workspace).read().attempts), 1 if prepare_first else 0)

    def test_import_and_preparation_races_do_not_silently_retarget_source(self):
        for prepare_first in (True, False):
            with self.subTest(prepare_first=prepare_first):
                self.workspace = self.root / f"import-race-{prepare_first}"
                self.invoke("init", self.workspace)
                self.enable()
                self.enroll()
                source_revision = self.tasks_revision()
                prepare = self.prepare_args("queued-prepare")
                with coven.workspace_mutation(self.workspace) as held:
                    if prepare_first:
                        child = self.spawn(WORKER, "state", "import-tasks", self.workspace, "--expect-revision", source_revision)
                    else:
                        child = self.spawn(WORKER, *prepare)
                    self.wait_ready(child)
                    if prepare_first:
                        task_attempts.Store(self.workspace).mutate("prepare", {"task_id": "core-chatbot", "intent": json.loads(self.intent_path.read_text())},
                            expected_execution_revision=self.revision(), expected_tasks_revision=source_revision, request_id="winning-prepare",
                            source_loader=lambda value: coven.execution_source(self.workspace, value), lock=held)
                    else:
                        task_ledger.LedgerStore(self.workspace).import_tasks(source_revision, held)
                _, error = child.communicate(timeout=20)
                self.assertNotEqual(child.returncode, 0)
                self.assertIn("prepared attempt blocks" if prepare_first else "tasks revision conflict", error)
                self.assertEqual(len(task_attempts.Store(self.workspace).read().attempts), 1 if prepare_first else 0)
                if prepare_first:
                    self.assertFalse((self.workspace / "state/task-ledger").exists())
                else:
                    self.assertEqual(self.inspect()["enrollments"][0]["source_binding"]["status"], "source_authority_changed")
