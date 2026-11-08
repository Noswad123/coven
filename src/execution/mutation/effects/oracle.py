"""Test-only effect capture from the unchanged production Python writers/reducers."""
from copy import deepcopy
from datetime import datetime
import hashlib
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
from unittest.mock import patch

from runner import execution_v2, storage, task_attempts as v1

NOW = "2026-11-02T01:02:03.123456+00:00"
INTENT = {"schema_version": 1, "kind": "task-intent", "task_id": "task",
          "owner": "worker", "acceptance_criteria": [{"id": "a", "text": " exact evidence \n"}]}


def generate(schema, case):
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        store = v1.Store(root)
        current = {"identity": None, "revision": "tasks-sha256:" + "0" * 64,
                   "tasks": [{"id": "task", "title": "T", "status": "open", "owner": "worker",
                              "unknown": {"big": 2**100, "zero": -0.0, "values": [True, None, 1e-7]}}],
                   "roster": ["worker"]}

        def source(snapshot):
            value = deepcopy(current)
            if value["identity"] is None:
                value["identity"] = {"kind": "snapshot-v1", "source_id": snapshot, "path": "state/tasks.json"}
            return value

        def call(operation, parameters, key):
            view = store.read()
            with storage.workspace_lock(root, root / "state/.workspace.lock") as held:
                return store.mutate(operation, parameters,
                    expected_execution_revision=view.revision if view else v1.DISABLED,
                    expected_tasks_revision=current["revision"] if operation != "cancel" else None,
                    request_id=key, source_loader=source, lock=held)

        if case != "enable":
            call("enable", {}, "enable")
            if case not in ("enroll", "missing_enrollment"):
                call("enroll", {"task_id": "task", "rebind_source": False}, "enroll")
            if case == "chain":
                first = call("prepare", {"task_id": "task", "intent": INTENT}, "prepare")
                call("cancel", {"attempt_id": first["result"]["attempt_id"], "reason": "withdraw"}, "cancel")
            if schema == 2:
                with storage.workspace_lock(root, root / "state/.workspace.lock") as held:
                    execution_v2.upgrade(store, store.read().revision, "upgrade", held)

        if case in ("edited", "reenroll"):
            current["tasks"][0]["title"] = "Current edited task, not historical enrollment"
            current["revision"] = "tasks-sha256:" + "1" * 64
        if case in ("rebind", "rebind_denied", "source_changed"):
            current["identity"] = {"kind": "task-ledger-v1", "workspace_id": "b" * 32,
                                   "epoch_id": "c" * 32, "path": "state/task-ledger"}
            current["revision"] = "tasks-ledger:v1:" + "b" * 32 + ":" + "c" * 32 + ":0:" + hashlib.sha256(v1.canonical(current["tasks"])).hexdigest()
        if case == "missing_task":
            current["tasks"] = []
        if case == "owner":
            current["tasks"][0]["owner"] = "different"
        if case == "roster":
            current["roster"] = ["different"]
        operation = ("enable" if case == "enable" else "enroll" if case in
                     ("enroll", "reenroll", "rebind", "rebind_denied") else "prepare")
        parameters = ({} if operation == "enable" else
                      {"task_id": "task", "rebind_source": case == "rebind"} if operation == "enroll" else
                      {"task_id": "task", "intent": deepcopy(INTENT)})
        before = store._control()
        view = store.read()
        expected = view.revision if view else v1.DISABLED
        binding = (execution_v2.request_binding if schema == 2 else v1.request_binding)(
            operation, parameters, expected, current["revision"])
        captured = []
        module = execution_v2 if schema == 2 else v1
        original = module._apply

        def apply(*args):
            captured.append(deepcopy(args[-2]))
            return original(*args)

        accepted = False
        result = None
        with patch.object(v1.uuid, "uuid4", return_value=SimpleNamespace(hex="d" * 32)), \
             patch.object(v1, "datetime") as clock, \
             patch.object(execution_v2, "datetime") as clock2, \
             patch.object(module, "_apply", side_effect=apply):
            clock.now.return_value = datetime.fromisoformat(NOW)
            clock2.now.return_value = datetime.fromisoformat(NOW)
            try:
                result = call(operation, parameters, "candidate")["result"]
                accepted = True
            except v1.AttemptError:
                pass
        if before is None:
            before = store._control()
            before["generation"] = 0
            before["history"] = []
        observed = source(before["snapshot_source_id"])
        return {"schema": schema, "case": case, "control": before, "source": observed,
                "binding": binding, "effect": captured[-1] if captured else None,
                "accepted": accepted, "result": result}


cases = [generate(1, "enable")]
for schema in (1, 2):
    cases.extend(generate(schema, case) for case in (
        "enroll", "reenroll", "rebind", "rebind_denied", "edited", "missing_enrollment",
        "missing_task", "owner", "roster", "source_changed", "chain"))
print(json.dumps(cases, ensure_ascii=True))
