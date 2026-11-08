"""Test-only graph writer/reducer vectors from the unchanged Python store."""
from copy import deepcopy
from datetime import datetime
import hashlib
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[4]))
from runner import execution_v2, storage, task_attempts


def generate():
    vectors = []
    counter = 0
    tasks = [{"id": key, "title": key, "status": "open", "owner": "worker",
              "extension": [1.0, -0.0, 2**100 + 1, "\u03bb", {"operator": "  exact\n"}]}
             for key in ("downstream", "upstream", "unrelated")]
    roster = ["worker"]
    ledger = False
    now = datetime.fromisoformat("2026-11-02T01:02:03.123456+00:00")

    def uuid():
        nonlocal counter
        counter += 1
        return SimpleNamespace(hex=f"{counter:032x}")

    def source(snapshot):
        identity = {"kind": "snapshot-v1", "source_id": snapshot, "path": "state/tasks.json"}
        revision = storage.snapshot_revision(task_attempts.canonical(tasks))
        if ledger:
            identity = {"kind": "task-ledger-v1", "workspace_id": "a" * 32,
                        "epoch_id": "b" * 32, "path": "state/task-ledger"}
            revision = ("tasks-ledger:v1:" + "a" * 32 + ":" + "b" * 32 + ":1:"
                        + hashlib.sha256(task_attempts.canonical(tasks)).hexdigest())
        return {"identity": identity, "revision": revision,
                "tasks": deepcopy(tasks), "roster": list(roster)}

    with tempfile.TemporaryDirectory() as directory, \
         patch.object(task_attempts.uuid, "uuid4", side_effect=uuid), \
         patch.object(task_attempts, "datetime") as clock, \
         patch.object(execution_v2, "datetime") as v2_clock:
        clock.now.return_value = now
        v2_clock.now.return_value = now
        root = Path(directory)
        store = task_attempts.Store(root)
        path = root / "state/execution/control.json"
        serial = 0

        def call(op, params, label=None):
            nonlocal serial
            serial += 1
            view = store.read()
            expected = view.revision if view else task_attempts.DISABLED
            needs = op in ("enroll", "prepare", "prepare_graph", "graph_propose") or (
                op == "graph_decide" and params["decision"] == "approve")
            current = source(view.snapshot_source_id) if view and needs else None
            revision = current["revision"] if current else None
            before = json.loads(path.read_bytes()) if label else None
            vector = {"label": label, "control": before, "operation": op,
                      "parameters": deepcopy(params), "source": current,
                      "expected_execution": expected, "expected_tasks": revision,
                      "key": f"request-{serial}", "counter": counter, "now": now.isoformat()}
            try:
                with storage.workspace_lock(root, root / "state/.workspace.lock") as held:
                    if op == "upgrade":
                        result = execution_v2.upgrade(store, expected, vector["key"], held)
                    else:
                        result = store.mutate(op, params, expected_execution_revision=expected,
                            expected_tasks_revision=revision, request_id=vector["key"],
                            source_loader=lambda sid: source(sid) if op == "enable" else deepcopy(current),
                            lock=held)
            except task_attempts.AttemptError:
                if not label:
                    raise
                vector["accepted"] = False
            else:
                vector["accepted"] = True
                if label:
                    after = json.loads(path.read_bytes())
                    vector.update(effect=after["history"][-1]["effect"],
                                  result=result["result"], after=after)
            if label:
                vectors.append(vector)
            return result if vector["accepted"] else None

        def propose(nodes, label):
            return call("graph_propose", {"document": {"schema_version": 1,
                "kind": "task-graph", "nodes": [{"task_id": key, "owner": "worker",
                    "acceptance_criteria": [{"id": "proof", "text": "  evidence \u03bb\n"}],
                    "depends_on": ["upstream"] if key == "downstream" else []}
                    for key in nodes]}}, label)

        def decide(decision, label):
            view = store.read()
            return call("graph_decide", {"graph_revision": view.current_graph_revision,
                "decision": decision, "reason": "  operator \u03bb\n",
                "expected_graph_digest": view.graphs[view.current_graph_revision]["digest"]}, label)

        def prepare(task, label, revision=None):
            return call("prepare_graph", {"task_id": task,
                "graph_revision": revision or store.read().current_graph_revision}, label)

        def standalone(task, label):
            return call("prepare", {"task_id": task, "intent": {"schema_version": 1,
                "kind": "task-intent", "task_id": task, "owner": "worker",
                "acceptance_criteria": [{"id": "proof", "text": "explicit standalone"}]}}, label)

        call("enable", {})
        for key in ("downstream", "upstream"):
            call("enroll", {"task_id": key, "rebind_source": False})
        call("upgrade", {})
        propose(["downstream", "upstream"], "fresh-two-node-downstream-first")
        first = store.read().current_graph_revision
        standalone("downstream", "proposed-managed-task-cannot-use-standalone")
        view = store.read()
        call("graph_decide", {"graph_revision": first, "decision": "approve",
            "reason": "digest guard", "expected_graph_digest": "task-graph-manifest-v1:" + "0" * 64},
            "wrong-digest-rejected")
        tasks[2]["title"] = "unrelated whole revision change"
        decide("approve", "approve-unrelated-revision-change")
        down = prepare("downstream", "downstream-does-not-dispatch-or-wait")["result"]["attempt_id"]
        up = prepare("upstream", "independent-upstream-reservation")["result"]["attempt_id"]
        decide("changes-requested", "source-free-revocation-preserves-reservations")
        prepare("downstream", "revoked-review-rejects-prepare")
        decide("approve", "prepared-blocks-reapproval")
        propose(["upstream"], "prepared-blocks-successor-even-removed-node")
        call("cancel", {"attempt_id": down, "reason": "cancel independently"})
        call("cancel", {"attempt_id": up, "reason": "cancel independently"})
        propose(["upstream"], "successor-preserves-family-and-head")
        standalone("downstream", "removed-node-sticky-managed-cannot-use-standalone")
        prepare("upstream", "old-head-rejected", first)
        call("graph_decide", {"graph_revision": first, "decision": "changes-requested",
            "reason": "old review", "expected_graph_digest": view.graphs[first]["digest"]},
            "source-free-review-still-requires-current-head")
        prepare("upstream", "successor-needs-fresh-review")
        decide("approve", "approve-successor")
        prepare("downstream", "removed-node-remains-unavailable")
        tasks[1]["extension"][0] = 1
        prepare("upstream", "selected-int-float-drift-rejected")
        tasks[1]["extension"][0] = 1.0
        roster.clear()
        prepare("upstream", "missing-owner-rejected")
        roster.append("worker")
        ledger = True
        prepare("upstream", "authority-change-rejected")
        call("enroll", {"task_id": "upstream", "rebind_source": True})
        prepare("upstream", "rebound-enrollment-invalidates-old-graph")
        propose(["upstream"], "successor-freezes-current-ledger-and-binding")
        decide("approve", "approve-rebound-successor")
        prepare("upstream", "graph-bound-attempt-chain-after-rebind")
        decide("changes-requested", "ledger-source-free-revocation")
    return vectors


print(json.dumps(generate(), ensure_ascii=True))
