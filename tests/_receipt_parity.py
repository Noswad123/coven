"""Build real Python histories; emit lookup/read oracles, never invoke native code."""

import argparse
import contextlib
from copy import deepcopy
import io
import json
import os
from pathlib import Path
import shutil
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from runner import coven, task_ledger as ledger


def main(root):
    counts = {}

    def invoke(*args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            status = coven.main(list(map(str, args)))
        assert status == 0, (args, status, out.getvalue(), err.getvalue())

    def clone(source, name):
        target = root / name
        shutil.copytree(source, target, symlinks=True)
        return target

    def frames(path):
        marker = json.loads((path / ledger.MARKER).read_bytes())
        return sorted((path / ledger.LEDGER / marker["epoch_id"] / "frames").glob("*.frame"))

    def emit(path, key, binding, status="result", label="", fence=False):
        store = ledger.LedgerStore(path)
        expected = None
        try:
            view = store.read()
        except ledger.LedgerError:
            assert status == "read_error", label
        else:
            assert status != "read_error", label
            if view is None:
                assert status == "no_authority", label
            else:
                assert status != "no_authority", label
                try:
                    expected = store.lookup(view, key, binding)
                except ledger.LedgerError as error:
                    assert status in {"invalid_key", "invalid_binding", "conflict"}, (label, error)
                    if status == "conflict":
                        assert "changed binding" in str(error), (label, error)
                    elif status == "invalid_key":
                        try:
                            ledger.validate_key(key)
                        except ledger.LedgerError:
                            pass
                        else:
                            raise AssertionError(label)
                    else:
                        # Validate independently, before any receipt-map miss.
                        ledger.validate_key(key)
                        try:
                            ledger._fields(binding, "version operation args expected_revision")
                            ledger._version(binding["version"])
                            ledger.request_binding(binding["operation"], binding["args"], binding["expected_revision"])
                        except ledger.LedgerError:
                            pass
                        else:
                            raise AssertionError(label)
                else:
                    assert status == "result", label
                    if expected is not None:
                        # Python's public lookup must return an independent copy.
                        expected["task"]["caller_only"] = True
                        expected = store.lookup(view, key, binding)
                        assert "caller_only" not in expected["task"]
        counts[status] = counts.get(status, 0) + 1
        print(json.dumps({"kind": "case", "path": str(path), "key": key,
                          "binding": binding, "status": status, "label": label,
                          "expected": expected,
                          "canonical": ledger._canonical(expected).decode(), "fence": fence},
                         ensure_ascii=False, allow_nan=False))

    history = root / "history"
    invoke("init", history, "--blank", "--agents", "operator,other")
    source = history / "state/tasks.json"
    # Raw identity/formatting differs from canonical storage. Extension values
    # survive assign receipts, including integers beyond serde_json's range.
    source.write_bytes((json.dumps([{
        "id": "Seed_RAW", "title": "Original \u03bb", "status": "legacy custom",
        "notes": "original", "extension": {"large": 2**100 + 1, "negative": -(2**100),
        "signed_zero": -0.0, "float": 1.0, "tiny": 1e-7,
        "nested": [None, True, {"z": "\u00e9", "a": "e\u0301"}]},
    }], indent=3, ensure_ascii=False) + "\r\n").encode())
    source.chmod(0o640)
    invoke("state", "import-tasks", history, "--expect-revision", coven.read_task_snapshot(history)[0], "--json")
    baseline = clone(history, "baseline")
    requests = []

    def mutate(key, operation, args, expected):
        binding = ledger.request_binding(operation, args, expected)
        command = ["task", operation, history]
        if operation == "add":
            command += [args["title"], "--notes", args["notes"]]
            if args["owner"] is not None:
                command += ["--owner", args["owner"]]
        else:
            command += [args["task_id"], args["agent"]]
        command += ["--request-id", key, "--json"]
        if expected is not None:
            command += ["--expect-revision", expected]
        invoke(*command)
        requests.append((key, binding))

    mutate("Add:ONE_1", "add", {"title": "New Task", "owner": "OPERATOR", "notes": "opaque \u03bb"},
           ledger.LedgerStore(history).read().revision)
    mutate("assign.seed", "assign", {"task_id": "Seed_RAW", "agent": "OPERATOR"},
           ledger.LedgerStore(history).read().revision)
    mutate("assign.new", "assign", {"task_id": "new-task", "agent": "OTHER"}, None)
    mutate("assign.seed.again", "assign", {"task_id": "Seed_RAW", "agent": "OTHER"},
           ledger.LedgerStore(history).read().revision)
    mutate("add-unowned", "add", {"title": "New Task", "owner": None, "notes": ""}, None)
    mutate("assign.unowned", "assign", {"task_id": "new-task-2", "agent": "operator"}, None)
    final = ledger.LedgerStore(history).read()
    for key, binding in requests:
        emit(history, key, binding, label="all original bindings after later changes")
        reordered = dict(reversed(list(binding.items())))
        reordered["args"] = dict(reversed(list(binding["args"].items())))
        emit(history, key, reordered, label="object order is not identity")
    assert final.tasks[0]["owner"] == "other"
    assert ledger.LedgerStore(history).lookup(final, *requests[1])["task"]["owner"] == "operator"
    assert ledger.LedgerStore(history).lookup(final, *requests[0])["task"]["owner"] == "operator"

    add_key, add_binding = requests[0]
    assign_key, assign_binding = requests[1]
    for field, value in (("title", "New task"), ("title", " New Task "),
                         ("owner", "operator"), ("owner", " OPERATOR "),
                         ("owner", None), ("notes", "changed")):
        changed = deepcopy(add_binding)
        changed["args"][field] = value
        emit(history, add_key, changed, "conflict", f"changed add {field}: {value!r}")
    for field, value in (("agent", "operator"), ("agent", " OPERATOR "),
                         ("task_id", "seed_raw"), ("task_id", " Seed_RAW ")):
        changed = deepcopy(assign_binding)
        changed["args"][field] = value
        emit(history, assign_key, changed, "conflict", f"raw selector {field}")
    emit(history, add_key, assign_binding, "conflict", "operation changed")
    for key, binding in (requests[0], requests[2]):
        for revision in (None, final.revision, "tasks-sha256:" + "0" * 64):
            if revision == binding["expected_revision"]:
                continue
            changed = deepcopy(binding)
            changed["expected_revision"] = revision
            emit(history, key, changed, "conflict", "expected revision identity, not current CAS")
    for key in ("missing", "add:ONE_1", "x" * 128):
        emit(history, key, add_binding, label="valid absent key")
    emit(history, "missing", ledger.request_binding("add", {
        "title": "!!!", "owner": "unknown current agent", "notes": " \t"}, "tasks-missing:v1"),
         label="miss validates schema, not slug or current roster/CAS")
    emit(history, "missing", ledger.request_binding("assign", {
        "task_id": "unknown current task", "agent": "unknown current agent"}, None),
         label="miss does not resolve current task or agent")
    emit(baseline, add_key, add_binding, label="valid view with no receipts")

    for key in ("", " ", "../bad", "a/b", "_start", "a\n", "\u00e9", "x" * 129):
        emit(history, key, add_binding, "invalid_key", "invalid key")
    invalid = [None, [], {}, {**add_binding, "extra": True}]
    for version in (True, False, 1.0, 2, "1", None):
        invalid.append({**add_binding, "version": version})
    invalid += [{k: v for k, v in add_binding.items() if k != field} for field in add_binding]
    for operation in ("delete", True, None):
        invalid.append({**add_binding, "operation": operation})
    for revision in (True, 1.0, "", "tasks-missing:v1\n", "tasks-ledger:v2:x",
                     "tasks-ledger:v1:" + "0" * 32 + ":" + "1" * 32 + ":01:" + "0" * 64):
        invalid.append({**add_binding, "expected_revision": revision})
    for field, value in (("title", " \t"), ("owner", " "), ("notes", None), ("title", True)):
        changed = deepcopy(add_binding)
        changed["args"][field] = value
        invalid.append(changed)
    for field in ("title", "owner"):
        changed = deepcopy(add_binding)
        changed["args"][field] = "\u0085\u00a0"
        invalid.append(changed)
    for binding in (add_binding, assign_binding):
        for field in binding["args"]:
            changed = deepcopy(binding)
            del changed["args"][field]
            invalid.append(changed)
    invalid += [{**add_binding, "args": {**add_binding["args"], "extra": "x"}},
                {**add_binding, "args": []},
                {**assign_binding, "args": {"task_id": " ", "agent": "operator"}},
                {**assign_binding, "args": {"task_id": "Seed_RAW", "agent": "\t"}}]
    for binding in invalid:
        emit(history, "missing", binding, "invalid_binding", "invalid schema before map miss")

    absent = root / "absent"
    absent.mkdir()
    emit(absent, add_key, add_binding, "no_authority", "absent authority")
    pending = clone(baseline, "pending")
    (pending / ledger.MARKER).unlink()
    emit(pending, add_key, add_binding, "no_authority", "complete pending epoch is not authority")

    for kind in ("missing", "bad", "fifo", "symlink", "directory"):
        target = clone(history, "projection-" + kind)
        projection = target / "state/tasks.json"
        projection.unlink()
        if kind == "bad":
            projection.write_bytes(b"not JSON\xff")
        elif kind == "fifo":
            os.mkfifo(projection)
        elif kind == "symlink":
            projection.symlink_to("missing-target")
        elif kind == "directory":
            projection.mkdir()
        emit(target, add_key, add_binding, label="projection ignored: " + kind)
    poisoned = clone(history, "poisoned-current-state")
    (poisoned / "state/execution").mkdir()
    for relative in ("coven.json", "coven.toml", "state/agents.json", "state/execution/control.json",
                     "state/runtime-launch-control.json", "state/.workspace.lock",
                     "logs/events.jsonl", "dashboard.md"):
        path = poisoned / relative
        if path.is_file():
            path.unlink()
        if not path.exists():
            os.mkfifo(path)
    emit(poisoned, assign_key, assign_binding, label="no roster/control/execution/event/dashboard preflight")

    for kind in ("truncated", "hash", "effect", "duplicate", "binding", "result"):
        target = clone(history, "tail-" + kind)
        last = frames(target)[-1]
        raw = last.read_bytes()
        payload = ledger._payload(raw)
        if kind == "truncated":
            last.write_bytes(raw[:-5])
        elif kind == "hash":
            envelope = json.loads(raw)
            envelope["sha256"] = "0" * 64
            last.write_bytes(ledger._canonical(envelope) + b"\n")
        else:
            if kind == "effect":
                payload["result"]["task"]["title"] = "semantically wrong"
                tasks = deepcopy(final.tasks)
                tasks[-1] = payload["result"]["task"]
                payload["result"]["after_revision"] = ledger._revision(
                    payload["workspace_id"], payload["epoch_id"], payload["sequence"], tasks)
            elif kind == "duplicate":
                payload["request_id"] = add_key
            elif kind == "binding":
                payload["binding"]["expected_revision"] = "tasks-sha256:" + "0" * 64
            else:
                payload["result"]["sequence"] = True
            last.write_bytes(ledger._envelope(payload))
        emit(target, add_key, add_binding, "read_error", "full replay refuses early receipt: " + kind)
    fenced = clone(history, "captured-marker")
    emit(fenced, add_key, add_binding, label="historical lookup survives changed marker; fence fails", fence=True)
    print(json.dumps({"kind": "summary", "counts": counts, "total": sum(counts.values())}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    main(parser.parse_args().root.resolve(strict=True))
