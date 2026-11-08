"""Test-only deterministic reviewed controls; all escape successors are native."""
import itertools
import json
from datetime import datetime
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[4]))
from runner import execution_v2 as v2, storage, task_attempts as v1


def fixture(count):
    ids = itertools.count(1)
    now = datetime.fromisoformat("2026-11-02T01:02:03.123456+00:00")
    with tempfile.TemporaryDirectory() as directory, \
         patch.object(v1.uuid, "uuid4", side_effect=lambda: SimpleNamespace(hex=f"{next(ids):032x}")), \
         patch.object(v1, "datetime") as clock, patch.object(v2, "datetime") as clock2:
        clock.now.return_value = clock2.now.return_value = now
        root = Path(directory)
        store = v1.Store(root)
        tasks = [{"id": f"task-{n}", "title": "T", "status": "open", "owner": "worker"}
                 for n in range(count)]

        def source(snapshot):
            return {"identity": {"kind": "snapshot-v1", "source_id": snapshot,
                                 "path": "state/tasks.json"},
                    "revision": "tasks-sha256:" + "0" * 64,
                    "tasks": tasks, "roster": ["worker"]}

        with storage.workspace_lock(root, root / "state/.workspace.lock") as held:
            def call(op, params, key):
                view = store.read()
                needs = op in ("enroll", "prepare_graph", "graph_propose") or (
                    op == "graph_decide" and params["decision"] == "approve")
                return store.mutate(op, params,
                    expected_execution_revision=view.revision if view else v1.DISABLED,
                    expected_tasks_revision=source("")["revision"] if needs else None,
                    request_id=key, source_loader=source, lock=held)

            call("enable", {}, "enable")
            for task in tasks:
                call("enroll", {"task_id": task["id"], "rebind_source": False}, "enroll-" + task["id"])
            v2.upgrade(store, store.read().revision, "upgrade", held)
            document = {"schema_version": 1, "kind": "task-graph", "nodes": [
                {"task_id": task["id"], "owner": "worker", "depends_on": [],
                 "acceptance_criteria": [{"id": "check", "text": "evidence"}]} for task in tasks]}
            call("graph_propose", {"document": document}, "propose")
            view = store.read()
            revision = view.current_graph_revision
            call("graph_decide", {"graph_revision": revision, "decision": "approve",
                "reason": "reviewed", "expected_graph_digest": view.graphs[revision]["digest"]}, "approve")
            for task in tasks:
                call("prepare_graph", {"task_id": task["id"], "graph_revision": revision}, "prepare-" + task["id"])
            return json.loads((root / v1.CONTROL).read_bytes())


def near_node_limit():
    sys.path.insert(0, str(Path(__file__).resolve().parents[4] / "tests"))
    from _execution_v2_parity import resign

    def nodes(value):
        if isinstance(value, dict):
            return 1 + sum(nodes(child) for child in value.values())
        if isinstance(value, list):
            return 1 + sum(nodes(child) for child in value)
        return 1

    control = fixture(3)
    # Extensions are duplicated in v2 and retained v1. Keep each individual
    # array within MAX_ITEMS, and leave room for two real decision entries but
    # much less than the three outstanding cancellation-node reserves.
    extension = {"padding": [[], [], []]}
    control["extensions"] = control["legacy_v1"]["extensions"] = extension
    target = 262144 - 96
    remaining = (target - nodes(control)) // 2
    for index in range(3):
        count = min(remaining, 65536)
        extension["padding"][index] = [None] * count
        remaining -= count
    assert remaining == 0
    resign(control, legacy_changed=True)
    assert target - 1 <= nodes(control) <= target
    return control


if "--near-nodes" in sys.argv:
    print(json.dumps(near_node_limit(), ensure_ascii=True))
else:
    print(json.dumps([fixture(3), fixture(6)], ensure_ascii=True))
