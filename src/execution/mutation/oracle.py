"""Test-only cancellation goldens from the unchanged Python store."""
import json
from datetime import datetime
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[3]))
from runner import execution_v2, storage, task_attempts


def generate(schema):
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        store = task_attempts.Store(root)

        def source(snapshot):
            return {"identity": {"kind": "snapshot-v1", "source_id": snapshot,
                                 "path": "state/tasks.json"},
                    "revision": "tasks-sha256:" + "0" * 64,
                    "tasks": [{"id": "task", "title": "T", "status": "open",
                               "owner": "worker", "extension": [1.0, -0.0, 2**100]}],
                    "roster": ["worker"]}

        def call(operation, parameters, key):
            view = store.read()
            expected = view.revision if view else task_attempts.DISABLED
            tasks = (source(view.snapshot_source_id)["revision"]
                     if operation in ("enroll", "prepare") else None)
            with storage.workspace_lock(root, root / "state/.workspace.lock") as held:
                return store.mutate(operation, parameters,
                    expected_execution_revision=expected, expected_tasks_revision=tasks,
                    request_id=key, source_loader=source, lock=held)

        call("enable", {}, "enable")
        call("enroll", {"task_id": "task", "rebind_source": False}, "enroll")
        if schema == 2:
            with storage.workspace_lock(root, root / "state/.workspace.lock") as held:
                execution_v2.upgrade(store, store.read().revision, "upgrade", held)
        result = call("prepare", {"task_id": "task", "intent": {
            "schema_version": 1, "kind": "task-intent", "task_id": "task", "owner": "worker",
            "acceptance_criteria": [{"id": "a", "text": "evidence"}]}}, "prepare")
        attempt = result["result"]["attempt_id"]
        control = json.loads((root / "state/execution/control.json").read_bytes())
        revision = store.read().revision
        now = datetime.fromisoformat("2026-11-02T01:02:03.123456+00:00")
        with patch.object(task_attempts.uuid, "uuid4", return_value=SimpleNamespace(hex="d" * 32)), \
             patch.object(task_attempts, "datetime") as clock, \
             patch.object(execution_v2, "datetime") as v2_clock:
            clock.now.return_value = now
            v2_clock.now.return_value = now
            result = call("cancel", {"attempt_id": attempt, "reason": "  withdraw \u03bb\n"}, "cancel")
        return {"control": control, "revision": revision, "attempt": attempt,
                "expected": result,
                "persisted": (root / "state/execution/control.json").read_text()}


print(json.dumps([generate(1), generate(2)], ensure_ascii=True))
