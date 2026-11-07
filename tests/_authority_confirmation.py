"""Real local Python authority histories and confirmation oracles, never a CLI.

Actions are explicit fixture writes performed outside native assertion windows.
Fault injection and exact native fsync ordering belong to core unit traits, not
to this real-filesystem SDK corpus.
"""
import argparse
from collections import Counter
from copy import deepcopy
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from runner import execution_v2, storage, task_attempts as execution, task_ledger as ledger


def copytree(source, target):
    def copy(path, destination):
        if stat.S_ISFIFO(os.lstat(path).st_mode):
            os.mkfifo(destination)
            return destination
        return shutil.copy2(path, destination)
    shutil.copytree(source, target, symlinks=True, copy_function=copy)


def lock(root):
    return storage.workspace_lock(root, root / "state/.workspace.lock")


def workspace(root):
    root.mkdir()
    (root / "state").mkdir()
    # The inherited native WorkspaceGuard requires a FILE, not valid config.
    (root / "coven.json").write_text("deliberately not configuration\n")
    with lock(root):
        pass
    return root


def append(root, operation="add", held=None):
    store = ledger.LedgerStore(root)

    def commit(token):
        view = store.read()
        if operation == "add":
            title = f"Task {view.sequence}"
            task = {"id": f"task-{view.sequence}", "title": title, "owner": None,
                    "status": "pending", "notes": "", "created_at": "then", "updated_at": "then"}
            args = {"title": title, "owner": None, "notes": ""}
        else:
            task = {**view.tasks[0], "owner": "worker", "status": "assigned", "updated_at": "later"}
            args = {"task_id": task["id"], "agent": "worker"}
        binding = ledger.request_binding(operation, args, view.revision)
        view = store.commit(view, f"request-{view.sequence + 1}", binding, task, token)
        store.publish_projection(view, token)
    if held is not None:
        commit(held)
    else:
        with lock(root) as token:
            commit(token)


def execution_call(root, operation, parameters, held=None):
    store = execution.Store(root)
    view = store.read()
    tasks = [{"id": key, "title": key, "owner": "worker", "status": "assigned",
              "extension": [2**90, -0.0, 1.0]} for key in ("a", "b", "unrelated")]

    def source(snapshot_id):
        return {"identity": {"kind": "snapshot-v1", "source_id": snapshot_id,
                             "path": "state/tasks.json"},
                "revision": storage.snapshot_revision(execution.canonical(tasks)),
                "tasks": deepcopy(tasks), "roster": ["worker"]}

    def mutate(token):
        key = f"exec-{view.generation + 1 if view else 1}"
        if operation == "upgrade":
            result = execution_v2.upgrade(store, view.revision, key, token)
        else:
            needs = operation in ("enroll", "prepare", "graph_propose", "prepare_graph") or (
                operation == "graph_decide" and parameters["decision"] == "approve")
            result = store.mutate(operation, parameters,
                expected_execution_revision=view.revision if view else execution.DISABLED,
                expected_tasks_revision=source(view.snapshot_source_id)["revision"] if needs else None,
                request_id=key, source_loader=source, lock=token)
        assert result["delivery"]["outcome"] == "committed", result
        return result["result"]
    if held is not None:
        return mutate(held)
    with lock(root) as token:
        return mutate(token)


def intent():
    return {"schema_version": 1, "kind": "task-intent", "task_id": "a", "owner": "worker",
            "acceptance_criteria": [{"id": "check", "text": "local acceptance"}]}


def action(root, name, held=None):
    if name == "append":
        append(root, held=held)
    elif name == "prepare":
        execution_call(root, "prepare", {"task_id": "a", "intent": intent()}, held)
    elif name == "cancel":
        view = execution.Store(root).read()
        attempt = next(k for k, v in view.attempts.items() if v["state"] == "prepared")
        execution_call(root, "cancel", {"attempt_id": attempt, "reason": "local cancellation"}, held)
    elif name in ("head-only", "corrupt-tail"):
        view = ledger.LedgerStore(root).read()
        frames = root / ledger.LEDGER / view.epoch_id / "frames"
        if name == "corrupt-tail":
            (frames / f"{view.sequence + 1:020d}.frame").write_bytes(b"corrupt tail\n")
        else:
            path = frames / f"{view.sequence:020d}.frame"
            payload = ledger._payload(path.read_bytes())
            old = payload["result"]["mutation_id"]
            payload["result"]["mutation_id"] = "0" * 32 if old != "0" * 32 else "1" * 32
            path.write_bytes(ledger._envelope(payload))
            current = ledger.LedgerStore(root).read()
            assert current.revision == view.revision and current.head_digest != view.head_digest
    else:
        raise AssertionError(name)


def oracle(root, api, task="a", change=None):
    try:
        with lock(root) as token:
            if api == "ledger":
                store = ledger.LedgerStore(root)
                view = store.read()
                if view is None:
                    return "absent"
                if change:
                    action(root, change, token)
                store.confirm_durability(view, token)
                return "Confirmed"
            view = execution.Store(root).read()
            if api == "task":
                execution.guard_task_mutation(root, task, token)
            else:
                execution.guard_authority_change(root, token)
            return "DisabledObserved" if view is None else "ActiveConfirmedClear"
    except (storage.StorageError, OSError, ValueError) as error:
        if "prepared attempt blocks" in str(error):
            return "prepared_attempt_blocks_mutation"
        return "refusal"


def main(root):
    counts = Counter()
    index = 0

    def emit(source, label, api, task="a", change=None, profile=False):
        nonlocal index
        index += 1
        target = root / f"case-{index:03d}"
        copytree(source, target)
        # Oracle actions cannot alter the native fixture before its View capture.
        oracle_root = root / "oracle"
        copytree(target, oracle_root)
        try:
            expected = oracle(oracle_root, api, task, change)
        finally:
            shutil.rmtree(oracle_root)
        if profile:
            assert expected == "ActiveConfirmedClear", (label, expected)
            expected = "profile_refusal"
        category = "native_profile" if profile else "invalid" if expected == "refusal" else expected
        counts[category] += 1
        print(json.dumps(dict(kind="case", path=str(target), label=label, api=api,
                              task=task, action=change, expected=expected, category=category)))

    def clone(source, name):
        target = root / name
        copytree(source, target)
        return target

    disabled = workspace(root / "disabled")
    for api in ("task", "authority", "ledger"):
        emit(disabled, "absent authority", api)
    pending = clone(disabled, "pending")
    (pending / execution.DIR).mkdir()
    for api in ("task", "authority"):
        emit(pending, "pending execution is not disabled", api)

    for present in (False, True):
        base = workspace(root / f"ledger-{present}")
        if present:
            (base / "state/tasks.json").write_bytes(b' [ ]\r\n')
            (base / "state/tasks.json").chmod(0o640)
        with lock(base) as token:
            store = ledger.LedgerStore(base)
            view = store.import_tasks(storage.snapshot_revision(
                (base / "state/tasks.json").read_bytes() if present else None), token)
            store.publish_projection(view, token)
        emit(base, f"baseline source present={present}", "ledger")
        append(base)
        emit(base, "single frame", "ledger")
        append(base, "assign")
        append(base)
        emit(base, "multiple add/assign frames", "ledger")
        for change in ("append", "head-only", "corrupt-tail"):
            emit(base, change, "ledger", change=change)
        for relative in ("state/tasks.json", "coven.json", "state/team.json", "state/control.json", "logs"):
            for fifo in (False, True):
                if relative == "coven.json" and fifo:
                    continue  # inherited WorkspaceGuard regular-marker restriction
                bad = clone(base, f"ledger-ignore-{present}-{relative.replace('/', '-')}-{fifo}")
                path = bad / relative
                path.unlink(missing_ok=True)
                if fifo:
                    os.mkfifo(path)
                else:
                    path.write_bytes(b"invalid non-authority\n")
                emit(bad, f"ledger ignores {relative} fifo={fifo}", "ledger")
        for relative in (ledger.MARKER, "manifest.json", "frame"):
            bad = clone(base, f"ledger-corrupt-{present}-{relative.replace('/', '-')}")
            view = ledger.LedgerStore(bad).read()
            epoch = bad / ledger.LEDGER / view.epoch_id
            path = bad / relative if relative == ledger.MARKER else epoch / (
                "manifest.json" if relative == "manifest.json" else "frames/00000000000000000003.frame")
            path.write_bytes(b"invalid authority\n")
            emit(bad, f"corrupt selected {relative}", "ledger")
        bad = clone(base, f"ledger-fifo-{present}")
        (bad / ledger.MARKER).unlink()
        os.mkfifo(bad / ledger.MARKER)
        emit(bad, "unsafe marker FIFO", "ledger")
        for value in (0, 1, "yes"):
            bad = clone(base, f"ledger-source-present-{present}-{value}")
            marker_path = bad / ledger.MARKER
            marker = json.loads(marker_path.read_bytes())
            manifest_path = bad / ledger.LEDGER / marker["epoch_id"] / "manifest.json"
            manifest = json.loads(manifest_path.read_bytes())
            manifest["source_present"] = value
            raw = ledger._canonical(manifest) + b"\n"
            manifest_path.write_bytes(raw)
            marker["manifest_digest"] = hashlib.sha256(raw).hexdigest()
            marker_path.write_bytes(ledger._canonical(marker) + b"\n")
            assert oracle(bad, "ledger") == "refusal"
            emit(bad, "source_present must be a strict bool", "ledger")
        for relative in (ledger.MARKER, ledger.LEDGER):
            bad = clone(base, f"ledger-link-{present}-{relative.replace('/', '-')}")
            path = bad / relative
            destination = bad / ("preserved-" + path.name)
            path.rename(destination)
            path.symlink_to(destination, target_is_directory=destination.is_dir())
            assert oracle(bad, "ledger") == "refusal"
            emit(bad, "selected authority symlink", "ledger")

    active = clone(disabled, "enabled")
    execution_call(active, "enable", {})
    enrolled = clone(active, "enrolled")
    execution_call(enrolled, "enroll", {"task_id": "a", "rebind_source": False})
    prepared = clone(enrolled, "prepared")
    execution_call(prepared, "prepare", {"task_id": "a", "intent": intent()})
    cancelled = clone(prepared, "cancelled")
    action(cancelled, "cancel")
    upgraded = clone(cancelled, "upgraded")
    execution_call(upgraded, "upgrade", {})
    proposed = clone(upgraded, "proposed")
    document = {"schema_version": 1, "kind": "task-graph", "nodes": [
        {**{k: v for k, v in intent().items() if k in ("task_id", "owner", "acceptance_criteria")},
         "depends_on": []}]}
    execution_call(proposed, "graph_propose", {"document": document})

    def decide(path, decision):
        view = execution.Store(path).read()
        execution_call(path, "graph_decide", {"graph_revision": view.current_graph_revision,
            "decision": decision, "reason": "local review",
            "expected_graph_digest": view.graphs[view.current_graph_revision]["digest"]})
    approved = clone(proposed, "approved")
    decide(approved, "approve")
    graph_prepared = clone(approved, "graph-prepared")
    execution_call(graph_prepared, "prepare_graph", {"task_id": "a",
        "graph_revision": execution.Store(graph_prepared).read().current_graph_revision})
    graph_cancelled = clone(graph_prepared, "graph-cancelled")
    action(graph_cancelled, "cancel")
    revoked = clone(graph_cancelled, "revoked")
    decide(revoked, "changes-requested")
    for source in (active, enrolled, prepared, cancelled, upgraded, proposed, approved,
                   graph_prepared, graph_cancelled, revoked):
        for api, task in (("task", "a"), ("task", "unrelated"), ("authority", "a")):
            emit(source, source.name + ":" + task, api, task)
    for source in (prepared, graph_prepared, cancelled, upgraded):
        bad = clone(source, source.name + "-stale-source")
        for relative in ("state/tasks.json", "state/team.json", "state/control.json", "logs"):
            os.mkfifo(bad / relative)
        # A corrupt current task marker/source must not be read for reservations.
        (bad / ledger.MARKER).write_bytes(b"bad current task authority")
        emit(bad, "current source/projections/config not dependencies", "task")
    for fifo in (False, True):
        bad = clone(upgraded, f"upgrade-backup-{fifo}")
        control = json.loads((bad / execution.CONTROL).read_bytes())
        backup = bad / control["upgrade"]["backup_path"]
        backup.unlink()
        if fifo:
            os.mkfifo(backup)
        emit(bad, "upgrade backup is not authority", "authority")
    for shape in ("bad-json", "null", "fifo", "symlink", "directory", "profile"):
        bad = clone(active, "control-" + shape)
        path = bad / execution.CONTROL
        raw = path.read_bytes()
        path.unlink()
        if shape == "fifo":
            os.mkfifo(path)
        elif shape == "symlink":
            path.symlink_to(bad / "coven.json")
        elif shape == "directory":
            path.mkdir()
        elif shape == "profile":
            path.write_bytes(raw + b" " * (8 * 1024 * 1024))
        else:
            path.write_bytes(b"{" if shape == "bad-json" else b"null")
        emit(bad, "selected control " + shape, "authority", profile=shape == "profile")
    print(json.dumps(dict(kind="summary", counts=dict(counts), total=index,
                         locks={"append": str(root / "ledger-True"),
                                "prepare": str(enrolled), "cancel": str(prepared)})))


def try_lock(root):
    fd = os.open(root / "state/.workspace.lock", os.O_RDWR)
    try:
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            print("busy")
        else:
            print("clear")
    finally:
        os.close(fd)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--action")
    parser.add_argument("--try-lock", action="store_true")
    parser.add_argument("--hold", type=Path)
    args = parser.parse_args()
    if args.try_lock:
        try_lock(args.root)
    elif args.hold:
        with lock(args.root) as token:
            (args.hold / "ready").touch()
            deadline = time.monotonic() + 10
            while not (args.hold / "release").exists():
                assert time.monotonic() < deadline, "fixture lock release timed out"
                time.sleep(0.01)
            action(args.root, args.action, token)
    elif args.action:
        action(args.root, args.action)
    else:
        main(args.root)
