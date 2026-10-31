"""Python execution authority oracle; native history is never admission.

Native profile: UTF-8 strict JSON, <=8 MiB control, <=1024 entries, <=256 MiB
of canonical prefix work, bounded JSON values, and timestamps of the form
YYYY-MM-DD[T or space]HH:MM:SS[.1..6 digits](Z|+/-HH:MM). Python's 16 MiB
controls and additional datetime.fromisoformat spellings remain valid Python,
but are explicit native refusals, never corrupt-Python fixtures.
"""
import argparse
from copy import deepcopy
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import uuid

sys.set_int_max_str_digits(4300)
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from runner import execution_v2, storage, task_attempts as execution


FIXED = {"authorization": "none", "dispatchable": False,
         "current_source": "not_assessed", "durability": "not_assessed",
         "assessment": "historical_authority_only"}


def snapshot(root):
    """Do not follow symlinks or open special files, including FIFOs."""
    result = {}

    def visit(path):
        metadata = path.lstat()
        mode = metadata.st_mode
        key = str(path.relative_to(root))
        identity = (mode, metadata.st_ino, metadata.st_mtime_ns)
        if stat.S_ISLNK(mode):
            result[key] = (*identity, "link", os.readlink(path))
        elif stat.S_ISDIR(mode):
            result[key] = (*identity, "directory")
            for child in sorted(path.iterdir()):
                visit(child)
        elif stat.S_ISREG(mode):
            result[key] = (*identity, "file", path.read_bytes())
        else:
            result[key] = (*identity, "special", metadata.st_rdev)
    visit(root)
    return result


def report(view):
    if view is None:
        return {"status": "disabled", "revision": execution.DISABLED, **FIXED}
    schema = getattr(view, "schema_version", 1)
    graph = ({"graph_id": view.graph_id,
              "current_graph_revision": view.current_graph_revision,
              "graphs": view.graphs,
              "graph_managed_tasks": sorted(view.managed_tasks)} if schema == 2 else {})
    return {"status": "active", "schema_version": schema,
            "execution_authority_id": view.authority_id,
            "snapshot_source_id": view.snapshot_source_id,
            "generation": view.generation, "revision": view.revision,
            "enrollments": view.enrollments, "attempts": view.attempts,
            "receipts": view.receipts, "extensions": view.extensions,
            "reserved_task_ids": sorted({a["prepared"]["task_id"]
                for a in view.attempts.values() if a["state"] == "prepared"}), **graph, **FIXED}


def resign(control, start=1):
    """Rehash ALL downstream expected prefixes without repairing semantic effects."""
    for index in range(start, len(control["history"])):
        previous = {**control, "generation": index, "history": control["history"][:index]}
        control["history"][index]["binding"]["expected_execution_revision"] = execution._revision(previous)
    return control


def main(binary, oracle_only=False):
    counts = {"parity": 0, "corruption_refusals": 0, "native_profile_refusals": 0}
    with tempfile.TemporaryDirectory(prefix="native-execution-") as temporary:
        root = Path(temporary)
        binary = str(Path(binary).resolve())

        def check(workspace, accepted=True, unsupported=False, no_python=False, native_error=None):
            before = snapshot(workspace)
            try:
                view = execution.Store(workspace).read()
            except (storage.StorageError, OSError, ValueError) as error:
                assert not accepted and not unsupported, (workspace.name, error)
                view = None
            else:
                assert accepted or unsupported, (workspace.name, "Python accepted corrupt control")
            if oracle_only:
                counts["native_profile_refusals" if unsupported else
                       "parity" if accepted else "corruption_refusals"] += 1
                assert snapshot(workspace) == before, workspace.name
                return
            env = {**os.environ, "COVEN_MULTIPLEXER": ""}
            if no_python:
                env["PATH"] = str(root / "no-programs")
            result = subprocess.run([binary, "execution", "history", str(workspace), "--json"],
                cwd=root, env=env, capture_output=True, timeout=60)
            if accepted and not unsupported:
                assert result.returncode == 0, (workspace.name, result.stdout, result.stderr)
                actual = execution._json(result.stdout)
                expected = report(view)
                # Python equality alone erases signed zero, bool/int and float/int
                # distinctions. Canonical bytes also freeze every nested receipt.
                assert execution.canonical(actual) == execution.canonical(expected), (
                    workspace.name, expected, actual)
                counts["parity"] += 1
            else:
                assert result.returncode != 0, (workspace.name, result.stdout, result.stderr)
                assert not result.stdout.strip(), (workspace.name, "success JSON on refusal", result.stdout)
                if native_error:
                    assert native_error.encode() in result.stderr, (workspace.name, result.stderr)
                counts["native_profile_refusals" if unsupported else "corruption_refusals"] += 1
            assert snapshot(workspace) == before, (workspace.name, "read changed namespace/bytes/mode/inode/mtime")

        def clone(workspace, name):
            target = root / name
            shutil.copytree(workspace, target, symlinks=True)
            # Every derived fixture starts from a validated real Python authority.
            execution.Store(target).read()
            return target

        def save(workspace, control):
            (workspace / execution.CONTROL).write_bytes(execution.canonical(control) + b"\n")

        def load(workspace):
            return execution._json((workspace / execution.CONTROL).read_bytes())

        tasks = [{"id": key, "title": "Task " + key, "status": "pending", "owner": "operator"}
                 for key in ("zeta", "alpha")]
        tasks[0]["extension"] = {"text": "\u03bb\U0001f98a", "big": 2**80 + 1,
                                  "zero": -0.0, "tie": 2.9802322387695312e-08}
        source = {"revision": storage.snapshot_revision(execution.canonical(tasks)),
                  "tasks": tasks, "roster": ["operator", "other"]}

        def loader(snapshot_id):
            return {"identity": {"kind": "snapshot-v1", "source_id": snapshot_id,
                                 "path": "state/tasks.json"}, **deepcopy(source)}

        def mutate(workspace, operation, parameters, request):
            store = execution.Store(workspace)
            view = store.read()
            with storage.workspace_lock(workspace, workspace / "state/.workspace.lock") as lock:
                result = store.mutate(operation, parameters,
                    expected_execution_revision=view.revision if view else execution.DISABLED,
                    expected_tasks_revision=None if operation == "cancel" else source["revision"],
                    request_id=request, source_loader=loader, lock=lock)
            assert result["authorization"] == "none" and result["dispatchable"] is False
            assert result["delivery"]["outcome"] == "committed"
            execution.Store(workspace).read()
            return result["result"]

        def intent(task="zeta"):
            return {"schema_version": 1, "kind": "task-intent", "task_id": task,
                    "owner": "operator", "acceptance_criteria": [{"id": "verify", "text": "Explicit check"}]}

        disabled = root / "disabled"
        disabled.mkdir()
        check(disabled)
        check(disabled, no_python=True)
        pending = clone(disabled, "pending")
        (pending / execution.DIR).mkdir(parents=True)
        check(pending, accepted=False)
        baseline = root / "enabled"
        baseline.mkdir()
        mutate(baseline, "enable", {}, "enable")
        check(baseline)
        history = clone(baseline, "history")
        mutate(history, "enroll", {"task_id": "zeta", "rebind_source": False}, "enroll-zeta")
        check(history)
        first = mutate(history, "prepare", {"task_id": "zeta", "intent": intent()}, "prepare-first")
        prepared = clone(history, "prepared")
        check(prepared)
        mutate(history, "cancel", {"attempt_id": first["attempt_id"], "reason": "Before submission"}, "cancel-first")
        check(history)
        cancelled = execution.Store(history).read()
        assert cancelled.attempts[first["attempt_id"]]["state"] == "cancelled_before_submission"
        assert cancelled.receipts["prepare-first"]["result"] == first
        assert first["state"] == "prepared" and report(cancelled)["reserved_task_ids"] == []
        cancelled_fixture = clone(history, "cancelled")
        second = mutate(history, "prepare", {"task_id": "zeta", "intent": intent()}, "prepare-second")
        assert second["attempt"]["previous_attempt_id"] == first["attempt_id"]
        mutate(history, "enroll", {"task_id": "alpha", "rebind_source": False}, "enroll-alpha")
        mutate(history, "prepare", {"task_id": "alpha", "intent": intent("alpha")}, "prepare-alpha")
        check(history)
        assert report(execution.Store(history).read())["reserved_task_ids"] == ["alpha", "zeta"]
        check(history, no_python=True)
        current = clone(history, "changed-valid-current-files")
        (current / "coven.json").write_text(json.dumps({"name": "Changed", "agents": ["replacement"]}))
        (current / "state/tasks.json").write_text(json.dumps([
            {"id": "unrelated", "title": "Changed", "status": "done", "owner": "replacement"}]))
        (current / "state/agents.json").write_text("[]")
        check(current)

        # Current files, config, and roster are not historical authority inputs.
        for name in ("coven.json", "state/tasks.json", "state/agents.json", "state/task-ledger"):
            for kind in ("corrupt", "missing", "symlink", "fifo"):
                target = clone(history, "current-" + name.replace("/", "-") + "-" + kind)
                path = target / name
                if kind == "corrupt":
                    path.write_bytes(b"not current JSON\xff")
                elif kind == "symlink":
                    path.symlink_to(root / "nonexistent")
                elif kind == "fifo":
                    os.mkfifo(path)
                check(target)
        moved = clone(history, "before-move")
        moved.rename(root / "moved workspace")
        check(root / "moved workspace")

        # Real source rebind from snapshot to a ledger identity, digest included.
        rebound = clone(cancelled_fixture, "rebound")
        workspace_id, epoch_id = uuid.uuid4().hex, uuid.uuid4().hex
        ledger_identity = {"kind": "task-ledger-v1", "workspace_id": workspace_id,
                           "epoch_id": epoch_id, "path": "state/task-ledger"}
        source["revision"] = (f"tasks-ledger:v1:{workspace_id}:{epoch_id}:3:"
                              + hashlib.sha256(execution.canonical(tasks)).hexdigest())
        snapshot_loader = loader

        def loader(snapshot_id):
            return {"identity": deepcopy(ledger_identity), **deepcopy(source)}
        mutate(rebound, "enroll", {"task_id": "zeta", "rebind_source": True}, "rebind")
        mutate(rebound, "prepare", {"task_id": "zeta", "intent": intent()}, "prepare-ledger")
        check(rebound)
        source["revision"] = storage.snapshot_revision(execution.canonical(tasks))
        loader = snapshot_loader

        # Extensions participate in every expected prefix, including Unicode,
        # arbitrary integers, signed zero and halfway/tied float formatting.
        extended = clone(history, "extensions")
        control = load(extended)
        control["extensions"] = {"\u03bb": {"text": ["e\u0301", "\u00e9", "\U0001f98a"],
            "big": 2**100 + 1, "zero": -0.0, "tie": 2.9802322387695312e-08,
            "one": 1.0, "small": 1e-7, "large": 1e20}}
        save(extended, resign(control))
        check(extended)

        def corrupt(name, workspace, index, change, diagnostic=None):
            target = clone(workspace, name)
            control = load(target)
            change(control["history"][index])
            save(target, resign(control, index + 1))
            if diagnostic:
                try:
                    execution.Store(target).read()
                except execution.AttemptError as error:
                    assert diagnostic in str(error), (name, str(error))
                else:
                    raise AssertionError((name, "oracle accepted semantic corruption"))
            check(target, accepted=False)

        # All mutations below preserve valid downstream hashes. Where possible
        # pin the oracle diagnostic to the intended semantic layer as well.
        cases = [
            ("prefix-digest", 1, lambda e: e["binding"].update(expected_execution_revision="execution-control-v1:" + load(history)["execution_authority_id"] + ":1:" + "0"*64), "revision digest mismatch"),
            ("duplicate-request", 2, lambda e: e.update(request_id="enable"), "duplicate request"),
            ("request-bool-version", 1, lambda e: e["binding"].update(schema_version=True), None),
            ("request-extra", 1, lambda e: e["binding"]["parameters"].update(extra=1), None),
            ("request-rebind-literal", 1, lambda e: e["binding"]["parameters"].update(rebind_source=0), None),
            ("request-id", 1, lambda e: e.update(request_id="bad key"), None),
            ("task-id", 1, lambda e: e["binding"]["parameters"].update(task_id="../bad"), None),
            ("source-uuid", 1, lambda e: e["effect"]["source"]["identity"].update(source_id="0"*32), "snapshot identity mismatch"),
            ("source-path", 1, lambda e: e["effect"]["source"]["identity"].update(path="../tasks.json"), None),
            ("source-revision", 1, lambda e: e["effect"]["source"].update(revision="tasks-missing:v1"), None),
            ("source-roster", 2, lambda e: e["effect"]["source"].update(roster=["other"]), "prepared owner"),
            ("source-roster-alias", 1, lambda e: e["effect"]["source"].update(roster=["operator", "OPERATOR"]), None),
            ("enrollment-snapshot", 1, lambda e: e["effect"]["enrollment"]["complete_task_snapshot"].update(title="changed"), "enrollment effect mismatch"),
            ("enrollment-digest", 1, lambda e: e["effect"]["enrollment"].update(task_snapshot_digest="0"*64), "enrollment effect mismatch"),
            ("enrollment-number", 1, lambda e: e["effect"]["enrollment"].update(binding_number=2), "enrollment effect mismatch"),
            ("attempt-id", 2, lambda e: e["effect"]["prepared"].update(attempt_id="not-uuid"), None),
            ("attempt-number", 2, lambda e: e["effect"]["prepared"].update(attempt_number=True), "prepared effect mismatch"),
            ("attempt-predecessor", 4, lambda e: e["effect"]["prepared"].update(previous_attempt_id=None), "prepared effect mismatch"),
            ("attempt-digest", 2, lambda e: e["effect"]["prepared"].update(intent_digest="0"*64), "prepared effect mismatch"),
            ("attempt-fields", 2, lambda e: e["effect"]["prepared"].update(state="prepared"), None),
            ("receipt-state", 2, lambda e: e["result"].update(state="cancelled_before_submission"), "receipt/effect mismatch"),
            ("receipt-task", 2, lambda e: e["result"]["attempt"]["complete_task_snapshot"].update(owner="other"), "receipt/effect mismatch"),
            ("cancel-id", 3, lambda e: e["effect"].update(cancel_id="wrong"), None),
            ("cancel-reason", 3, lambda e: e["effect"].update(reason="tampered"), "cancel effect mismatch"),
            ("cancel-attempt", 3, lambda e: e["effect"].update(attempt_id="0"*32), "cancel effect mismatch"),
            ("cancel-receipt", 3, lambda e: e["result"]["transition"].update(reason="tampered"), "receipt/effect mismatch"),
            ("intent-fields", 2, lambda e: e["binding"]["parameters"]["intent"].update(extra=1), None),
            ("intent-version", 2, lambda e: e["binding"]["parameters"]["intent"].update(schema_version=1.0), None),
            ("intent-owner", 2, lambda e: e["binding"]["parameters"]["intent"].update(owner="other"), "prepared owner"),
            ("intent-task", 2, lambda e: e["binding"]["parameters"]["intent"].update(task_id="alpha"), None),
            ("criteria-empty", 2, lambda e: e["binding"]["parameters"]["intent"].update(acceptance_criteria=[]), None),
            ("criteria-duplicate", 2, lambda e: e["binding"]["parameters"]["intent"]["acceptance_criteria"].append({"id": "verify", "text": "Other"}), None),
            ("criteria-id", 2, lambda e: e["binding"]["parameters"]["intent"]["acceptance_criteria"][0].update(id="bad.id"), None),
            ("criteria-blank", 2, lambda e: e["binding"]["parameters"]["intent"]["acceptance_criteria"][0].update(text=" "), None),
        ]
        for name, index, change, diagnostic in cases:
            corrupt(name, history, index, change, diagnostic)
        corrupt("duplicate-attempt-id", history, 4,
                lambda e: e["effect"]["prepared"].update(attempt_id=first["attempt_id"]),
                "duplicate attempt identity")
        corrupt("wrong-enrollment-id", history, 2,
                lambda e: e["effect"]["prepared"].update(enrollment_id="0"*32),
                "prepared effect mismatch")
        two_cancels = clone(history, "two-valid-cancellations")
        mutate(two_cancels, "cancel", {"attempt_id": second["attempt_id"], "reason": "Cancel again"}, "cancel-second")
        check(two_cancels)
        corrupt("duplicate-cancel-id", two_cancels, 7,
                lambda e: e["effect"].update(cancel_id=load(history)["history"][3]["effect"]["cancel_id"]),
                "duplicate cancellation identity")
        corrupt("ledger-task-digest", rebound, 4,
                lambda e: e["effect"]["source"]["tasks"][0].update(title="changed"), "ledger tasks digest mismatch")
        corrupt("ledger-uuid", rebound, 4,
                lambda e: e["effect"]["source"]["identity"].update(epoch_id="0"*32), "ledger identity/revision mismatch")
        corrupt("rebind-number", rebound, 4,
                lambda e: e["effect"]["enrollment"].update(binding_number=1), "enrollment effect mismatch")

        for name, change in (
            ("schema-bool", lambda c: c.update(schema_version=True)),
            ("generation-float", lambda c: c.update(generation=float(c["generation"]))),
            ("extra-control", lambda c: c.update(extra=1)),
            ("extensions-array", lambda c: c.update(extensions=[])),
            ("authority-id", lambda c: c.update(execution_authority_id="../bad"))):
            target = clone(history, name)
            control = load(target)
            change(control)
            save(target, control)
            check(target, accepted=False)

        for name, raw in (("null", b"null"), ("invalid-json", b"{broken"),
                          ("invalid-utf8", b"\xff"), ("array", b"[]"),
                          ("nonfinite", b'{"extensions":NaN}')):
            target = clone(history, name)
            (target / execution.CONTROL).write_bytes(raw)
            check(target, accepted=False)
        duplicate = clone(history, "duplicate-keys")
        path = duplicate / execution.CONTROL
        path.write_bytes(path.read_bytes().rstrip()[:-1] + b',"schema_version":1}')
        check(duplicate, accepted=False)
        for location in ("state", execution.DIR, execution.CONTROL):
            for kind in ("symlink", "fifo", "wrong-type"):
                target = clone(history, location.replace("/", "-") + "-" + kind)
                path = target / location
                if path.is_dir():
                    shutil.rmtree(path)
                else:
                    path.unlink()
                if kind == "symlink":
                    path.symlink_to(root / "nonexistent")
                elif kind == "fifo":
                    os.mkfifo(path)
                elif location == execution.CONTROL:
                    path.mkdir()
                else:
                    path.write_bytes(b"not a directory")
                check(target, accepted=False)
        # Reject live symlinks too, not only dangling links. The external
        # authority target is included in the unchanged-byte/metadata check.
        for location in ("state", execution.DIR, execution.CONTROL):
            target = clone(history, "live-link-" + location.replace("/", "-"))
            path = target / location
            evidence = root / ("evidence-" + location.replace("/", "-"))
            path.rename(evidence)
            path.symlink_to(evidence)
            before = snapshot(evidence) if evidence.is_dir() else evidence.read_bytes()
            check(target, accepted=False)
            assert (snapshot(evidence) if evidence.is_dir() else evidence.read_bytes()) == before

        # Real Python v2 upgrade plus a live reservation retains all v1 state
        # and adds the exact graph projection, even when no graph exists yet.
        upgraded = clone(cancelled_fixture, "valid-v2")
        store = execution.Store(upgraded)
        with storage.workspace_lock(upgraded, upgraded / "state/.workspace.lock") as lock:
            execution_v2.upgrade(store, store.read().revision, "upgrade", lock)
        mutate(upgraded, "prepare", {"task_id": "zeta", "intent": intent()}, "prepare-v2")
        assert any(a["state"] == "prepared" for a in store.read().attempts.values())
        check(upgraded)

        # Supported timestamp variants must also update the immutable result.
        for index, timestamp in enumerate(("2026-01-02T03:04:05Z", "2026-01-02 03:04:05+02:30",
                "2026-01-02T03:04:05.1-04:00", "2026-01-02T03:04:05.123456+00:00",
                "20260102T030405+00:00", "2026-01-02X03:04:05+00:00",
                "2026-01-02T03:04:05.1234567+00:00", "2026-01-02T03:04:05+00:00:30")):
            target = clone(history, "timestamp-" + str(index))
            control = load(target)
            entry = control["history"][1]
            entry["effect"]["enrollment"]["enrolled_at"] = timestamp
            entry["result"]["enrollment"]["enrolled_at"] = timestamp
            save(target, resign(control, 2))
            check(target, accepted=index < 4, unsupported=index >= 4)
        for timestamp in ("2026-01-02T03:04:05", "2026-02-30T03:04:05Z", "", True):
            corrupt("bad-time-" + str(timestamp).replace(":", "-"), history, 1,
                    lambda e, t=timestamp: e["effect"]["enrollment"].update(enrolled_at=t))

        # Large controls are generated temporarily, and remain real Python
        # authorities with sufficient cancellation reserve.
        large = clone(baseline, "python-valid-over-8MiB")
        control = load(large)
        control["extensions"]["padding"] = ["x" * (1024 * 1024)] * 8
        save(large, control)
        check(large, accepted=False, unsupported=True)
        # 31 reservations fit Python's 16 MiB reserve budget; the 32nd does
        # not. The control itself stays far below native's 8 MiB input limit,
        # so this refusal cannot be accidentally covered by the byte limit.
        capacity = clone(baseline, "cancellation-capacity")
        original_source = deepcopy(source)
        for number in range(32):
            task_id = f"capacity-{number}"
            source["tasks"] = [{"id": task_id, "title": "Capacity", "status": "pending", "owner": "operator"}]
            source["revision"] = storage.snapshot_revision(execution.canonical(source["tasks"]))
            mutate(capacity, "enroll", {"task_id": task_id, "rebind_source": False}, f"capacity-enroll-{number}")
            if number < 31:
                last = mutate(capacity, "prepare", {"task_id": task_id, "intent": intent(task_id)}, f"capacity-prepare-{number}")
        check(capacity)
        control = load(capacity)
        view = execution.Store(capacity).read()
        binding = execution.request_binding("prepare", {"task_id": task_id, "intent": intent(task_id)},
                                            view.revision, source["revision"])
        record = deepcopy(last["attempt"])
        task = source["tasks"][0]
        record.update(attempt_id=uuid.uuid4().hex, task_id=task_id,
                      enrollment_id=view.enrollments[task_id]["enrollment_id"],
                      complete_task_snapshot=task, task_source_revision=source["revision"],
                      task_snapshot_digest=execution.task_snapshot_digest(task), intent=intent(task_id))
        record["intent_digest"] = execution.intent_digest(execution._intent_payload(record))
        effect = {"source": loader(view.snapshot_source_id), "prepared": record}
        generation = control["generation"] + 1
        result = execution._apply(control, deepcopy(view.enrollments), deepcopy(view.attempts),
                                  binding, effect, generation)
        control["history"].append({"generation": generation, "request_id": "capacity-overflow",
                                   "binding": binding, "effect": effect, "result": result})
        control["generation"] = generation
        save(capacity, control)
        assert (capacity / execution.CONTROL).stat().st_size < 8 * 1024 * 1024
        try:
            execution.Store(capacity).read()
        except execution.AttemptError as error:
            assert "cancellation reserve" in str(error), str(error)
        else:
            raise AssertionError("Python accepted insufficient cancellation reserve")
        check(capacity, accepted=False)
        source.clear()
        source.update(original_source)
        huge = clone(baseline, "over-python-limit")
        with (huge / execution.CONTROL).open("wb") as stream:
            stream.truncate(execution.MAX_CONTROL + 1)
        check(huge, accepted=False)

        # Isolate the bounded-value profile from the overall 8 MiB limit.
        nested = None
        for _ in range(70):
            nested = [nested]
        for name, value in (("depth", nested), ("members", {str(i): None for i in range(16385)}),
                ("items", [None] * 65537), ("string", "x" * (1024*1024 + 1)),
                ("nodes", [[None] * 60000 for _ in range(5)])):
            target = clone(baseline, "bounded-value-" + name)
            control = load(target)
            control["extensions"]["value"] = value
            save(target, control)
            assert (target / execution.CONTROL).stat().st_size < 8 * 1024 * 1024
            check(target, accepted=False, unsupported=True)
        numeric = clone(baseline, "bounded-numeric-token")
        control = load(numeric)
        control["extensions"]["numeric"] = 1.0
        save(numeric, control)
        path = numeric / execution.CONTROL
        path.write_bytes(path.read_bytes().replace(b'"numeric":1.0', b'"numeric":1.' + b'0' * 8200))
        check(numeric, accepted=False, unsupported=True)
        boundary = clone(baseline, "supported-value-boundaries")
        control = load(boundary)
        control["extensions"] = {"string": "x" * (1024*1024), "integer": 10**4299,
                                  "float": 5e-324, "negative": -1.7976931348623157e308}
        save(boundary, control)
        check(boundary)

        # Build long controls using the Python reducer's sole permissible result,
        # then have Store.read validate them before expecting native refusal.
        def long_control(length):
            control = load(cancelled_fixture)
            view = execution.Store(cancelled_fixture).read()
            enrollments, attempts = deepcopy(view.enrollments), deepcopy(view.attempts)
            while len(control["history"]) < length:
                generation = len(control["history"]) + 1
                previous = execution._revision(control)
                if generation % 2:
                    template = deepcopy(load(prepared)["history"][2])
                    record = template["effect"]["prepared"]
                    older = [a["prepared"] for a in attempts.values()]
                    record.update(attempt_id=uuid.uuid4().hex, attempt_number=len(older)+1,
                                  previous_attempt_id=older[-1]["attempt_id"])
                else:
                    template = deepcopy(load(cancelled_fixture)["history"][3])
                    attempt_id = next(k for k, a in attempts.items() if a["state"] == "prepared")
                    template["binding"]["parameters"]["attempt_id"] = attempt_id
                    template["effect"].update(attempt_id=attempt_id, cancel_id=uuid.uuid4().hex)
                template.update(generation=generation, request_id=f"long-{generation}")
                template["binding"]["expected_execution_revision"] = previous
                template["result"] = execution._apply(control, enrollments, attempts,
                    template["binding"], template["effect"], generation)
                control["generation"] = generation
                control["history"].append(template)
            return control

        bounded = clone(cancelled_fixture, "over-history-limit")
        control = long_control(1025)
        save(bounded, control)
        check(bounded, accepted=False, unsupported=True)
        work = clone(cancelled_fixture, "over-prefix-work-limit")
        control = long_control(40)
        control["extensions"]["padding"] = ["x" * (1024 * 1024)] * 7
        save(work, resign(control))
        assert len(execution.canonical(control)) < 8 * 1024 * 1024
        assert sum(len(execution.canonical({**control, "generation": i,
            "history": control["history"][:i]})) for i in range(1, 40)) > 256 * 1024 * 1024
        check(work, accepted=False, unsupported=True)

        # Literal DEL occupies one canonical byte but six presentation bytes.
        # Retained receipts plus derived enrollment/attempt records expand a
        # supported <8 MiB control past the final ASCII-output budget. Refuse
        # before stdout rather than emitting a partial success-shaped report.
        expanded = root / "over-ascii-output-limit"
        expanded.mkdir()
        source["tasks"] = [{"id": "zeta", "title": "Expansion", "status": "assigned",
                            "owner": "operator", "extension": "\x7f" * 600000}]
        source["revision"] = storage.snapshot_revision(execution.canonical(source["tasks"]))
        mutate(expanded, "enable", {}, "output-enable")
        mutate(expanded, "enroll", {"task_id": "zeta", "rebind_source": False}, "output-enroll")
        mutate(expanded, "prepare", {"task_id": "zeta", "intent": intent()}, "output-prepare")
        assert (expanded / execution.CONTROL).stat().st_size < 8 * 1024 * 1024
        expected = report(execution.Store(expanded).read())
        assert len(execution.canonical(expected)) < 32 * 1024 * 1024
        assert len(json.dumps(expected, ensure_ascii=True).encode()) > 32 * 1024 * 1024
        check(expanded, accepted=False, unsupported=True, native_error="unsupported_json_output_profile")

    print(json.dumps({**counts, "total": sum(counts.values()), "oracle_only": oracle_only,
        "native_writes_enabled": False,
        "native_profile": {"control_bytes": 8*1024*1024, "history_entries": 1024,
            "canonical_prefix_bytes": 256*1024*1024, "schema_versions": [1, 2],
            "timestamps": "YYYY-MM-DD[T or space]HH:MM:SS[.1..6](Z|+/-HH:MM)"}}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--oracle-only", action="store_true",
                        help="validate all Python fixture controls without invoking native")
    args = parser.parse_args()
    main(args.binary, args.oracle_only)
