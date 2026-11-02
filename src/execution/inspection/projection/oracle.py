"""Test-only frozen projection vectors from the unchanged Python oracle.

Run from the repository root with an output path to regenerate vectors.json.
"""
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[4]))
from runner import coven, execution_v2, storage, task_attempts


def report(view, source, error):
    result = {
        "execution": {"status": "active", "revision": view.revision,
                      "execution_authority_id": view.authority_id,
                      "generation": view.generation, "schema_version": getattr(view, "schema_version", 1)},
        "authorization": "none", "dispatchable": False,
        "blockers": list(task_attempts.BLOCKERS),
        "source_assessment": {"coordinated": source is not None, "error": error,
                              "revision": source["revision"] if source else None},
        "enrollments": [{**record, "source_binding": coven.execution_binding_status(
            record["bindings"][-1], source, error)} for record in view.enrollments.values()],
        "attempts": [{**record, "source_binding": coven.execution_binding_status(
            record["prepared"], source, error)} for record in view.attempts.values()],
    }
    if getattr(view, "schema_version", 1) == 2:
        result.update(graphs=[coven.graph_inspection(view, rev, source, error) for rev in view.graphs],
                      current_graph_revision=view.current_graph_revision,
                      graph_managed_tasks=sorted(view.managed_tasks))
        for attempt in result["attempts"]:
            binding = attempt["prepared"].get("graph_binding")
            if binding is None:
                status = "not_graph_bound"
            elif binding["graph_revision"] != view.current_graph_revision:
                status = "superseded_revision"
            else:
                graph = view.graphs[binding["graph_revision"]]
                current = next(g for g in result["graphs"] if g["current"])
                status = ("planning_review_changed" if graph["current_review"] != "approved_for_preparation"
                          or graph["decisions"][-1]["decision_id"] != binding["decision_id"] else
                          "current" if current["source_binding"]["status"] == "current" else
                          "source_unavailable_or_stale")
            attempt["graph_binding_status"] = status
    return result


def generate():
    vectors = []
    counter = 0
    uuid_counter = 0
    ledger = False

    def uuid():
        nonlocal uuid_counter
        uuid_counter += 1
        # Every later proposal sorts before the earlier one.
        return SimpleNamespace(hex=f"{2**128 - uuid_counter:032x}")

    tasks = [{"id": key, "title": key, "status": "open", "owner": "worker",
              "extension": {"nested": [1.0, -0.0, 2**100 + 1, "\u03bb"]}}
             for key in ("z", "b", "a", "unrelated")]

    def source(snapshot_id):
        result = {"identity": {"kind": "snapshot-v1", "source_id": snapshot_id,
                             "path": "state/tasks.json"},
                "revision": storage.snapshot_revision(task_attempts.canonical(tasks)),
                "tasks": deepcopy(tasks), "roster": ["worker"]}
        if ledger:
            result["identity"] = {"kind": "task-ledger-v1", "workspace_id": "1" * 32,
                                  "epoch_id": "2" * 32, "path": "state/task-ledger"}
            result["revision"] = ("tasks-ledger:v1:" + "1" * 32 + ":" + "2" * 32 + ":1:"
                                  + hashlib.sha256(task_attempts.canonical(tasks)).hexdigest())
        return result

    with tempfile.TemporaryDirectory() as temporary, patch.object(task_attempts.uuid, "uuid4", uuid):
        root = Path(temporary)
        store = task_attempts.Store(root)

        def call(op, params):
            nonlocal counter
            counter += 1
            view = store.read()
            needs = op in ("enroll", "prepare", "prepare_graph", "graph_propose") or (
                op == "graph_decide" and params["decision"] == "approve")
            with storage.workspace_lock(root, root / "state/.workspace.lock") as held:
                result = store.mutate(op, params,
                    expected_execution_revision=view.revision if view else task_attempts.DISABLED,
                    expected_tasks_revision=source(view.snapshot_source_id)["revision"] if needs else None,
                    request_id=f"request-{counter}", source_loader=source, lock=held)
            return result["result"]

        def capture(name, matrix=False):
            view = store.read()
            current = source(view.snapshot_source_id)
            sources = [("current", current, None), ("unavailable", None, "not coordinated"),
                       ("unassessed", None, None)]
            if matrix:
                for kind in ("revision", "owner", "changed", "missing", "authority"):
                    changed = deepcopy(current)
                    changed["revision"] = "tasks-sha256:" + "0" * 64
                    if kind in ("owner", "changed", "missing", "authority"):
                        changed["roster"] = ["other"]
                    if kind in ("changed", "missing", "authority"):
                        changed["tasks"][0]["title"] = "changed"
                    if kind in ("missing", "authority"):
                        changed["tasks"] = []
                    if kind == "authority":
                        changed["identity"]["source_id"] = "0" * 32
                    sources.append((kind, changed, None))
                unrelated = deepcopy(current)
                unrelated["tasks"][-1]["title"] = "drift"
                unrelated["revision"] = storage.snapshot_revision(task_attempts.canonical(unrelated["tasks"]))
                sources.append(("unrelated", unrelated, None))
            vectors.append({"name": name,
                            "control": task_attempts._json((root / task_attempts.CONTROL).read_bytes()),
                            "cases": [{"name": label, "source": value, "error": error,
                                       "expected": report(view, value, error)}
                                      for label, value, error in sources]})

        call("enable", {})
        for key in ("z", "b", "a"):
            call("enroll", {"task_id": key, "rebind_source": False})
        explicit = {"schema_version": 1, "kind": "task-intent", "task_id": "z", "owner": "worker",
                    "acceptance_criteria": [{"id": "proof", "text": "data, not a command"}]}
        first = call("prepare", {"task_id": "z", "intent": explicit})
        capture("v1", True)
        call("cancel", {"attempt_id": first["attempt_id"], "reason": "cancel"})
        with storage.workspace_lock(root, root / "state/.workspace.lock") as held:
            execution_v2.upgrade(store, store.read().revision, "upgrade", held)
        capture("no-graphs")
        document = {"schema_version": 1, "kind": "task-graph", "nodes": [
            {"task_id": key, "owner": "worker", "acceptance_criteria": explicit["acceptance_criteria"],
             "depends_on": ["b", "a"] if key == "z" else []} for key in ("z", "b", "a")]}

        def decide(decision):
            view = store.read()
            return call("graph_decide", {"graph_revision": view.current_graph_revision,
                "decision": decision, "reason": "review",
                "expected_graph_digest": view.graphs[view.current_graph_revision]["digest"]})

        call("graph_propose", {"document": document})
        capture("proposed")
        decide("approve")
        bound = call("prepare_graph", {"task_id": "z", "graph_revision": store.read().current_graph_revision})
        capture("bound", True)
        decide("changes-requested")
        capture("revoked")
        call("cancel", {"attempt_id": bound["attempt_id"], "reason": "cancel"})
        decide("approve")
        capture("decision-changed")
        call("graph_propose", {"document": document})
        decide("approve")
        capture("superseded", True)
        ledger = True
        call("enroll", {"task_id": "z", "rebind_source": True})
        capture("latest-enrollment-binding")
    return vectors


if __name__ == "__main__":
    Path(sys.argv[1]).write_text(json.dumps(generate(), ensure_ascii=True, separators=(",", ":")) + "\n")
