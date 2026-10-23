"""Reviewed graph authoring/preparation, explicit upgrades, and process races."""

import contextlib
import io
import json
from pathlib import Path
import select
import subprocess
import sys
from unittest import mock

from runner import coven, execution_v2, launch, storage, task_attempts, task_ledger, workers
from test_coven import CovenTestCase, ROOT, snapshot


WORKER = """
import contextlib,sys
from runner import coven,storage
coven.send_to_agent_pane=lambda *args:False
original=storage.workspace_lock
@contextlib.contextmanager
def announced(*args,**kwargs):
    if kwargs.get('held') is None: print('ready',flush=True)
    with original(*args,**kwargs) as held: yield held
storage.workspace_lock=announced
raise SystemExit(coven.main(sys.argv[1:]))
"""


class GraphCommandTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace)
        for target, name in ((coven.subprocess, "run"), (launch, "execute"), (workers, "inspect_readiness")):
            guard = mock.patch.object(target, name, side_effect=AssertionError("Graph planning never starts/probes native execution"))
            observed = guard.start()
            self.addCleanup(guard.stop)
            self.addCleanup(observed.assert_not_called)
        sender = mock.patch.object(coven, "send_to_agent_pane", return_value=False)
        sender.start()
        self.addCleanup(sender.stop)
        self.invoke("execution", "enable", self.workspace, "--expect-execution-revision", task_attempts.DISABLED, "--request-id", "enable", "--json")
        for task in ("core-chatbot", "qa-verification"):
            self.invoke("execution", "enroll", self.workspace, task, "--expect-execution-revision", self.revision(),
                        "--expect-tasks-revision", self.task_revision(), "--request-id", "enroll-" + task)
        self.document = {"schema_version": 1, "kind": "task-graph", "nodes": [
            {"task_id": "core-chatbot", "owner": "jamal", "acceptance_criteria": [{"id": "evidence", "text": "Reviewed API evidence"}], "depends_on": []},
            {"task_id": "qa-verification", "owner": "eric", "acceptance_criteria": [{"id": "reconcile", "text": "Reconcile findings"}], "depends_on": ["core-chatbot"]}]}
        self.graph_file = self.root / "operator-graph.json"
        self.graph_file.write_text(json.dumps(self.document))
        self.intent_file = self.root / "operator-intent.json"
        self.intent_file.write_text(json.dumps({"schema_version": 1, "kind": "task-intent",
            **{key: self.document["nodes"][0][key] for key in ("task_id", "owner", "acceptance_criteria")}}))

    def revision(self):
        return task_attempts.Store(self.workspace).read().revision

    def task_revision(self):
        return json.loads(self.invoke("task", "list", self.workspace, "--json"))["revision"]

    def upgrade_args(self, key="upgrade"):
        return ["execution", "upgrade", str(self.workspace), "--expect-execution-revision", self.revision(), "--request-id", key, "--json"]

    def upgrade(self):
        return json.loads(self.invoke(*self.upgrade_args()))

    def propose_args(self, key="propose"):
        return ["task", "graph", "propose", str(self.workspace), "--document", str(self.graph_file),
                "--expect-execution-revision", self.revision(), "--expect-tasks-revision", self.task_revision(), "--request-id", key, "--json"]

    def propose(self, key="propose"):
        return json.loads(self.invoke(*self.propose_args(key)))

    def decide_args(self, graph, decision="approve", key="review"):
        result = graph["result"]
        args = ["task", "graph", "decide", str(self.workspace), result["graph_revision"], "--decision", decision,
                "--reason", "Reviewed local plan", "--expect-graph-digest", result["graph_digest"],
                "--expect-execution-revision", self.revision(), "--request-id", key, "--json"]
        if decision == "approve":
            args += ["--expect-tasks-revision", self.task_revision()]
        return args

    def prepare_args(self, graph, task="core-chatbot", key="prepare"):
        return ["task", "attempt", "prepare", str(self.workspace), task, "--graph-revision", graph["result"]["graph_revision"],
                "--expect-execution-revision", self.revision(), "--expect-tasks-revision", self.task_revision(), "--request-id", key, "--json"]

    def cancel(self, prepared, key="cancel"):
        return json.loads(self.invoke("task", "attempt", "cancel", self.workspace, prepared["result"]["attempt_id"],
            "--reason", "Revise planning", "--expect-execution-revision", self.revision(), "--request-id", key, "--json"))

    def inspect_graph(self, revision=None):
        return json.loads(self.invoke("task", "graph", "inspect", self.workspace, *([revision] if revision else []), "--json"))

    def spawn(self, code, *args):
        child = subprocess.Popen([sys.executable, "-c", code, *map(str, args)], cwd=ROOT,
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        def cleanup():
            if child.poll() is None:
                child.kill()
            child.communicate(timeout=10)
        self.addCleanup(cleanup)
        return child

    def wait(self, child, expected="ready"):
        self.assertTrue(select.select([child.stdout], [], [], 15)[0])
        line = child.stdout.readline().strip()
        if line != expected:
            self.fail(f"expected {expected}, got {line}: {child.communicate(timeout=10)}")

    def test_no_implicit_upgrade_and_exact_backup_preserves_legacy_receipts(self):
        original = (self.workspace / task_attempts.CONTROL).read_bytes()
        self.invoke(*self.propose_args(), error="explicit execution upgrade")
        self.assertEqual((self.workspace / task_attempts.CONTROL).read_bytes(), original)
        upgrade_args = self.upgrade_args()
        result = json.loads(self.invoke(*upgrade_args))
        self.assertEqual((self.workspace / result["result"]["backup_path"]).read_bytes(), original)
        view = task_attempts.Store(self.workspace).read()
        self.assertEqual(view.schema_version, 2)
        raw = task_attempts._json((self.workspace / task_attempts.CONTROL).read_bytes())
        legacy = raw["legacy_v1"]
        for entry in legacy["history"]:
            binding = entry["binding"]
            replay = coven.mutate_execution(self.workspace, binding["operation"], binding["parameters"],
                expected_execution_revision=binding["expected_execution_revision"], expected_tasks_revision=binding["expected_tasks_revision"],
                request_id=entry["request_id"])
            self.assertEqual(replay["result"], entry["result"])
        self.assertEqual(json.loads(self.invoke(*upgrade_args))["result"], result["result"])

    def test_reviewed_graph_is_one_criteria_source_and_downstream_is_not_dispatchable(self):
        self.upgrade()
        graph = self.propose()
        self.invoke("approve", self.workspace, "plan-approval")  # A local checkpoint cannot review this graph.
        self.invoke(*self.prepare_args(graph), error="not approved")
        self.invoke(*self.decide_args(graph))
        first = json.loads(self.invoke(*self.prepare_args(graph)))
        second = json.loads(self.invoke(*self.prepare_args(graph, "qa-verification", "downstream")))
        self.assertFalse(second["dispatchable"])
        self.assertEqual(second["authorization"], "none")
        self.assertEqual(first["result"]["attempt"]["schema_version"], 2)
        self.assertEqual(first["result"]["attempt"]["intent"]["acceptance_criteria"], self.document["nodes"][0]["acceptance_criteria"])
        self.assertEqual(second["result"]["attempt"]["graph_binding"]["graph_revision"], graph["result"]["graph_revision"])
        current = self.inspect_graph()["graph"]
        self.assertEqual(current["dependency_order"], ["core-chatbot", "qa-verification"])
        self.assertEqual(current["nodes"][1]["dependency_acceptance"], "unestablished")
        self.assertFalse(current["nodes"][1]["dispatchable"])
        self.invoke("task", "attempt", "prepare", self.workspace, "core-chatbot", "--intent", self.intent_file,
                    "--expect-execution-revision", self.revision(), "--expect-tasks-revision", self.task_revision(),
                    "--request-id", "no-criteria-bypass", error="graph-managed")

    def test_revocation_and_successor_never_reuse_old_review_or_rewrite_attempts(self):
        self.upgrade()
        graph = self.propose()
        self.invoke(*self.decide_args(graph))
        attempt = json.loads(self.invoke(*self.prepare_args(graph)))
        self.invoke(*self.propose_args("blocked-successor"), error="prepared attempt blocks")
        self.invoke(*self.decide_args(graph, "changes-requested", "revoke"))
        self.assertEqual(task_attempts.Store(self.workspace).read().attempts[attempt["result"]["attempt_id"]]["prepared"], attempt["result"]["attempt"])
        self.invoke(*self.prepare_args(graph, "qa-verification", "blocked-downstream"), error="not approved")
        self.cancel(attempt)
        self.document["nodes"][0]["acceptance_criteria"][0]["text"] = "Revised evidence criteria"
        self.graph_file.write_text(json.dumps(self.document))
        successor = self.propose("successor")
        self.assertEqual(successor["result"]["manifest"]["predecessor"], graph["result"]["graph_revision"])
        self.invoke(*self.prepare_args(successor, key="unreviewed-successor"), error="not approved")
        self.invoke(*self.prepare_args(graph, key="old-revision"), error="current head")
        self.invoke(*self.decide_args(successor, key="successor-review"))
        revised = json.loads(self.invoke(*self.prepare_args(successor, key="successor-prepare")))
        self.assertNotEqual(revised["result"]["attempt"]["intent_digest"], attempt["result"]["attempt"]["intent_digest"])

    def test_removing_a_node_does_not_restore_standalone_intent_bypass(self):
        self.upgrade()
        first = self.propose()
        self.invoke(*self.decide_args(first))
        self.document["nodes"] = self.document["nodes"][:1]
        self.graph_file.write_text(json.dumps(self.document))
        successor = self.propose("reduced-plan")
        self.invoke(*self.decide_args(successor, key="reduced-review"))
        intent = {"schema_version": 1, "kind": "task-intent", "task_id": "qa-verification", "owner": "eric",
                  "acceptance_criteria": [{"id": "anything", "text": "Not in the current plan"}]}
        self.intent_file.write_text(json.dumps(intent))
        self.invoke("task", "attempt", "prepare", self.workspace, "qa-verification", "--intent", self.intent_file,
                    "--expect-execution-revision", self.revision(), "--expect-tasks-revision", self.task_revision(), "--request-id", "removed", error="graph-managed")
        self.invoke(*self.prepare_args(successor, "qa-verification", "excluded"), error="not in the current graph")

    def test_unrelated_task_changes_differ_from_selected_source_drift(self):
        self.upgrade()
        graph = self.propose()
        self.invoke("task", "add", self.workspace, "Unrelated work")
        self.assertTrue(self.inspect_graph()["graph"]["source_binding"]["whole_revision_changed"])
        self.assertEqual(self.inspect_graph()["graph"]["source_binding"]["status"], "current")
        self.invoke(*self.decide_args(graph))
        self.invoke("task", "assign", self.workspace, "core-chatbot", "eric")
        self.assertFalse(self.inspect_graph()["graph"]["preparation_eligible"])
        self.invoke(*self.prepare_args(graph), error="graph source is not current")
        self.invoke(*self.decide_args(graph, key="cannot-refresh-old-plan"), error="graph source is not current")

    def test_source_corruption_preserves_export_receipts_revocation_and_cancellation(self):
        self.upgrade()
        proposal_args = self.propose_args()
        graph = json.loads(self.invoke(*proposal_args))
        self.invoke(*self.decide_args(graph))
        prepare_args = self.prepare_args(graph)
        prepared = json.loads(self.invoke(*prepare_args))
        revoke_args = self.decide_args(graph, "changes-requested", "source-free-revoke")
        for relative in ("coven.json", "state/tasks.json", "state/agents.json"):
            (self.workspace / relative).write_bytes(b"broken\xff")
        exported = json.loads(self.invoke("task", "graph", "export", self.workspace, graph["result"]["graph_revision"], "--json"))
        self.assertEqual(exported["manifest"], graph["result"]["manifest"])
        self.assertEqual(json.loads(self.invoke(*proposal_args))["result"], graph["result"])
        self.invoke(*revoke_args)
        self.cancel(prepared)
        replay = json.loads(self.invoke(*prepare_args))
        self.assertEqual(replay["result"], prepared["result"])
        self.assertEqual(replay["delivery"]["attempt_state"], "cancelled_before_submission")

    def test_import_requires_rebind_and_successor_not_implicit_graph_retargeting(self):
        self.upgrade()
        graph = self.propose()
        self.invoke(*self.decide_args(graph))
        self.invoke("state", "import-tasks", self.workspace, "--expect-revision", self.task_revision())
        self.assertEqual(self.inspect_graph()["graph"]["source_binding"]["status"], "source_authority_changed")
        self.invoke(*self.prepare_args(graph), error="graph source is not current")
        for task in ("core-chatbot", "qa-verification"):
            self.invoke("execution", "enroll", self.workspace, task, "--rebind-source", "--expect-execution-revision", self.revision(),
                        "--expect-tasks-revision", self.task_revision(), "--request-id", "rebind-" + task)
        successor = self.propose("new-authority-plan")
        self.invoke(*self.decide_args(successor, key="new-authority-review"))
        self.invoke(*self.prepare_args(successor, key="new-authority-prepare"))
        self.assertEqual(json.loads(self.invoke("task", "list", self.workspace, "--json"))["sequence"], 0)

    def test_same_request_document_values_replay_but_changed_criteria_conflict(self):
        self.upgrade()
        args = self.propose_args()
        first = json.loads(self.invoke(*args))
        self.graph_file.write_text(json.dumps(self.document, indent=4))
        self.assertEqual(json.loads(self.invoke(*args))["result"], first["result"])
        self.document["nodes"][0]["acceptance_criteria"][0]["text"] += " Changed"
        self.graph_file.write_text(json.dumps(self.document))
        self.invoke(*args, error="binding conflict")

    def test_unmanaged_v2_intent_preparation_keeps_supported_local_workflow(self):
        self.upgrade()
        graph = self.propose()
        self.intent_file.write_text(json.dumps({"schema_version": 1, "kind": "task-intent", "task_id": "architecture", "owner": "srikanth",
                                              "acceptance_criteria": [{"id": "design", "text": "Independent task"}]}))
        self.invoke("execution", "enroll", self.workspace, "architecture", "--expect-execution-revision", self.revision(),
                    "--expect-tasks-revision", self.task_revision(), "--request-id", "enroll-unmanaged")
        value = json.loads(self.invoke("task", "attempt", "prepare", self.workspace, "architecture", "--intent", self.intent_file,
            "--expect-execution-revision", self.revision(), "--expect-tasks-revision", self.task_revision(), "--request-id", "unmanaged", "--json"))
        self.assertIsNone(value["result"]["attempt"]["graph_binding"])
        self.assertEqual(self.inspect_graph()["graph"]["planning_review"], "proposed")
        self.assertFalse(value["dispatchable"])

    def test_concurrent_graph_proposals_have_one_effect_per_key_and_cas(self):
        self.upgrade()
        for same_key in (False, True):
            with self.subTest(same_key=same_key):
                first_args = self.propose_args("same" if same_key else "first")
                second_args = list(first_args)
                if not same_key:
                    second_args[second_args.index("--request-id") + 1] = "second"
                with coven.workspace_mutation(self.workspace):
                    children = [self.spawn(WORKER, *args) for args in (first_args, second_args)]
                    for child in children:
                        self.wait(child)
                results = []
                for child in children:
                    out, error = child.communicate(timeout=20)
                    results.append((child.returncode, json.loads(out) if child.returncode == 0 else error))
                self.assertEqual(sorted(code for code, _ in results), [0, 0] if same_key else [0, 1])
                if same_key:
                    self.assertEqual(results[0][1]["result"], results[1][1]["result"])

    def test_preparation_and_successor_proposal_races_never_change_plan_under_active_attempt(self):
        self.upgrade()
        graph = self.propose()
        self.invoke(*self.decide_args(graph))
        queued = self.propose_args("queued-successor")
        with coven.workspace_mutation(self.workspace) as held:
            child = self.spawn(WORKER, *queued)
            self.wait(child)
            prepared = task_attempts.Store(self.workspace).mutate("prepare_graph", {"task_id": "core-chatbot", "graph_revision": graph["result"]["graph_revision"]},
                expected_execution_revision=self.revision(), expected_tasks_revision=self.task_revision(), request_id="winning-prepare",
                source_loader=lambda source: coven.execution_source(self.workspace, source), lock=held)
        _, error = child.communicate(timeout=20)
        self.assertNotEqual(child.returncode, 0)
        self.assertIn("execution revision conflict", error)
        self.invoke(*self.propose_args("fresh-but-reserved"), error="prepared attempt blocks")
        self.assertEqual(task_attempts.Store(self.workspace).read().attempts[prepared["result"]["attempt_id"]]["state"], "prepared")

    def test_source_change_before_queued_review_cannot_approve_unseen_inputs(self):
        self.upgrade()
        graph = self.propose()
        args = self.decide_args(graph)
        with coven.workspace_mutation(self.workspace):
            child = self.spawn(WORKER, *args)
            self.wait(child)
            tasks = self.read_json("state/tasks.json")
            next(item for item in tasks if item["id"] == "core-chatbot")["owner"] = "eric"
            storage.atomic_replace_bytes(coven.managed_path(self.workspace, "state/tasks.json"), storage.encode_json(tasks))
        _, error = child.communicate(timeout=20)
        self.assertNotEqual(child.returncode, 0)
        self.assertIn("tasks revision conflict", error)
        self.assertEqual(self.inspect_graph()["graph"]["planning_review"], "proposed")

    def test_upgrade_process_death_before_or_after_publication_is_recoverable(self):
        for phase in ("backup", "before", "after"):
            with self.subTest(phase=phase):
                args = self.upgrade_args()
                source = (self.workspace / task_attempts.CONTROL).read_bytes()
                code = """
import os,sys
from pathlib import Path
from runner import coven,execution_v2,storage
if sys.argv[1]=='backup':
    original=storage.os.fsync
    def paused(fd):
        backups=Path(sys.argv[4]).glob('state/execution/upgrades/*/control-v1.bytes')
        if any(item.stat().st_ino==os.fstat(fd).st_ino for item in backups):
            print('paused',flush=True);sys.stdin.readline()
        return original(fd)
    storage.os.fsync=paused
elif sys.argv[1]=='before':
    original=storage.atomic_replace_bytes
    def paused(*args):
        print('paused',flush=True);sys.stdin.readline();return original(*args)
    storage.atomic_replace_bytes=paused
else:
    original=execution_v2._confirm
    def paused(*args):
        print('paused',flush=True);sys.stdin.readline();return original(*args)
    execution_v2._confirm=paused
raise SystemExit(coven.main(sys.argv[2:]))
"""
                child = self.spawn(code, phase, *args)
                self.wait(child, "paused")
                child.kill()
                child.communicate(timeout=10)
                if phase != "after":
                    self.assertEqual((self.workspace / task_attempts.CONTROL).read_bytes(), source)
                    self.assertTrue(list((self.workspace / "state/execution/upgrades").glob("*/control-v1.bytes")))
                else:
                    self.assertEqual(task_attempts.Store(self.workspace).read().schema_version, 2)
                replay = json.loads(self.invoke(*args))
                self.assertEqual(replay["delivery"]["replayed"], phase == "after")
                # The second phase starts with another independently valid v1
                # fixture, never a production downgrade/reset operation.
                if phase != "after":
                    raw = task_attempts._json((self.workspace / task_attempts.CONTROL).read_bytes())["legacy_v1"]
                    (self.workspace / task_attempts.CONTROL).write_bytes(task_attempts.canonical(raw) + b"\n")

    def test_upgrade_uncertainty_returns_known_receipt_without_touching_tasks(self):
        args = self.upgrade_args()
        tasks = (self.workspace / "state/tasks.json").read_bytes()
        real = storage.atomic_replace_bytes
        def uncertain(path, data):
            real(path, data)
            raise storage.CommitUncertain(path)
        output = io.StringIO()
        with mock.patch.object(storage, "atomic_replace_bytes", side_effect=uncertain), contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(args), 2)
        frozen = json.loads(output.getvalue())["result"]
        self.assertEqual(json.loads(self.invoke(*args))["result"], frozen)
        self.assertEqual((self.workspace / "state/tasks.json").read_bytes(), tasks)

    def test_present_null_execution_control_is_not_disabled_or_a_guard_bypass(self):
        args = ["task", "attempt", "prepare", str(self.workspace), "core-chatbot", "--intent", str(self.intent_file),
                "--expect-execution-revision", self.revision(), "--expect-tasks-revision", self.task_revision(), "--request-id", "v1-prepare"]
        self.invoke(*args)
        (self.workspace / task_attempts.CONTROL).write_text("null")
        self.assertEqual(json.loads(self.invoke("execution", "inspect", self.workspace, "--json"))["execution"]["status"], "unavailable")
        tasks = (self.workspace / "state/tasks.json").read_bytes()
        self.invoke("task", "assign", self.workspace, "core-chatbot", "eric", error="present execution control")
        self.assertEqual((self.workspace / "state/tasks.json").read_bytes(), tasks)
