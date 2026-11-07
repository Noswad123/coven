"""Python SDK fixtures/oracles for native baseline activation, never CLI execution.

All mutations here are explicit fixture windows, outside Rust's inventories.
Native preplanning is deliberately stronger than Python's: profile failures can
leave Python pending artifacts but must leave the native namespace unchanged.
"""
import argparse
from collections import Counter
import json
import os
from pathlib import Path
import re
import shutil
import stat
import sys
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from runner import storage, task_ledger as ledger, task_attempts as execution
from _authority_confirmation import (
    action, append, copytree, execution_call, intent, lock, try_lock, workspace,
)


def source(root):
    path = root / "state/tasks.json"
    return path.read_bytes() if path.exists() else None


def token(root):
    return storage.snapshot_revision(source(root))


def activate(root, pending=False):
    with lock(root) as held:
        ledger.LedgerStore(root).import_tasks(token(root), held)
    if pending:
        (root / ledger.MARKER).unlink()


def epoch(root):
    return next((root / ledger.LEDGER).iterdir())


def oracle(root, operation, expected):
    try:
        with lock(root) as held:
            store = ledger.LedgerStore(root)
            if operation == "abort":
                store.abort_import(held)
            else:
                getattr(store, operation + "_import" if operation == "resume" else "import_tasks")(
                    expected, held)
        return "ok"
    except (storage.StorageError, OSError, ValueError) as error:
        if "prepared attempt blocks" in str(error):
            return "prepared"
        return "refusal"


def verify(root, canonical=False):
    """Inspect actual random identities and compare the complete Python codec."""
    store = ledger.LedgerStore(root)
    view = store.read()
    assert view is not None
    marker_raw = (root / ledger.MARKER).read_bytes()
    marker = ledger._json(marker_raw)
    base = root / ledger.LEDGER / marker["epoch_id"]
    manifest_raw = (base / "manifest.json").read_bytes()
    manifest = ledger._json(manifest_raw)
    frame = (base / "frames/00000000000000000000.frame").read_bytes()
    payload = ledger._payload(frame)
    for key in ("workspace_id", "epoch_id"):
        identity = marker[key]
        assert re.fullmatch(ledger.UUID, identity)
        assert uuid.UUID(hex=identity).version == 4
        assert uuid.UUID(hex=identity).variant == uuid.RFC_4122
        assert manifest[key] == payload[key] == identity
    assert marker_raw == ledger._canonical(marker) + b"\n"
    if canonical:
        assert manifest_raw == ledger._canonical(manifest) + b"\n"
    assert marker["manifest_digest"] == ledger._digest(manifest_raw)
    assert marker["baseline_digest"] == manifest["baseline_digest"] == ledger._digest(frame)
    assert frame == ledger._envelope(payload)
    raw = source(root)
    assert manifest["source_revision"] == storage.snapshot_revision(raw)
    assert manifest["source_length"] == len(raw or b"")
    assert manifest["source_digest"] == ledger._digest(raw or b"")
    assert manifest["source_present"] is (raw is not None)
    assert payload["revision"] == ledger._revision(marker["workspace_id"], marker["epoch_id"], 0,
                                                   ledger._json(raw) if raw is not None else [])
    assert view.revision == payload["revision"] and view.sequence == 0
    if raw is not None:
        backup = base / "baseline.tasks.bytes"
        assert backup.read_bytes() == raw
        assert stat.S_IMODE(backup.stat().st_mode) == manifest["source_mode"]
    else:
        assert not (base / "baseline.tasks.bytes").exists()
        assert not (root / "state/tasks.json").exists()
    print(view.revision)


def main(root):
    counts = Counter()
    index = 0

    def clone(original, name):
        destination = root / name
        copytree(original, destination)
        return destination

    def emit(original, label, operation="import", expected_token=None, profile=False,
             roundtrip=False, wrapped=False):
        nonlocal index
        index += 1
        destination = clone(original, f"case-{index:03d}")
        expected_token = token(destination) if expected_token is None else expected_token
        other = clone(destination, "oracle")
        try:
            expected = oracle(other, operation, expected_token)
        finally:
            shutil.rmtree(other)
        if profile:
            # Python has no matching bounded value profile; do not pretend its
            # failure timing or error code is the native oracle.
            expected = "profile"
        category = f"{operation}:{expected}"
        counts[category] += 1
        print(json.dumps(dict(kind="case", path=str(destination), label=label,
                              operation=operation, expected=expected, token=expected_token,
                              category=category, roundtrip=roundtrip, wrapped=wrapped)), flush=True)

    blank = workspace(root / "blank")
    # Every unrelated namespace is inventoried, not just the projection.
    for relative in ("logs/events.jsonl", "state/events.jsonl", "state/dashboard.json",
                     "state/current.json", "state/team.json", "state/control.json",
                     "actions/sentinel", "dashboard.md"):
        path = blank / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"sentinel: not authority\n")
    raws = [None, b" [] \r\n", b'[ {"status":"pending", "title":"raw", "id":"a"} ]\n',
            '[{"id":"a","title":"雪 🦊 é","status":"assigned","owner":"worker"}]'.encode(),
            b'[{"id":"a","title":"large","status":"pending","extension":1267650600228229401496703205376}]',
            b'[{"id":"a","title":"zeros","status":"pending","extension":[-0.0,0.0,1.0,1e-07]}]',
            b'[{"id":"a","title":"extension","status":"pending","extension":{"nested":[null,true,{"x":2}]}}]',
            b'[{"id":"a","title":"a","status":"pending"},{"id":"b","title":"b","status":"done"}]']
    valid = []
    for number, raw in enumerate(raws):
        fixture = clone(blank, f"source-{number}")
        if raw is not None:
            path = fixture / "state/tasks.json"
            path.write_bytes(raw)
            path.chmod(0o640)
        valid.append(fixture)
        emit(fixture, f"native import source {number}", roundtrip=True)
    emit(valid[2], "import stale original source token", expected_token="tasks-sha256:" + "0" * 64)
    emit(valid[2], "import malformed source token", expected_token="not-a-revision")
    emit(blank, "resume absent pending directory", "resume")
    emit(blank, "abort absent pending directory", "abort")
    for number, raw in enumerate((b"\xff", b"null", b"{}", b"[1]", b'[{"id":"a","id":"b","title":"x","status":"pending"}]',
                                b'[{"id":"a","title":"x","status":"pending","x":NaN}]',
                                b'[{"id":"a","title":"x","status":"pending","x":"\\ud800"}]', b"[")):
        fixture = clone(blank, f"invalid-source-{number}")
        (fixture / "state/tasks.json").write_bytes(raw)
        emit(fixture, f"strict invalid source {number}")
    # Valid Python sources outside native explicit profiles, including values
    # which fit in isolation but not after baseline/envelope wrapping.
    profile_values = [
        [{"id": "a", "title": "x", "status": "pending", "x": "x" * (1048576 + 1)}],
        [{"id": "a", "title": "x", "status": "pending", "x": [[0] * 65532] * 4}],
    ]
    nested = 0
    for _ in range(62):
        nested = [nested]
    profile_values.append([{"id": "a", "title": "x", "status": "pending", "x": nested}])
    frame_limit = [{"id": "a", "title": "x", "status": "pending", "x": ["x" * 1048576] * 8}]
    excess = len(ledger._canonical(frame_limit)) - (ledger.MAX_FRAME - 1)
    frame_limit[0]["x"][-1] = frame_limit[0]["x"][-1][:-excess]
    assert len(ledger._canonical(frame_limit)) == ledger.MAX_FRAME - 1
    profile_values.append(frame_limit)
    for number, value in enumerate(profile_values):
        fixture = clone(blank, f"profile-{number}")
        raw = ledger._canonical(value)
        assert len(raw) < ledger.MAX_FRAME
        (fixture / "state/tasks.json").write_bytes(raw)
        emit(fixture, f"native bounded/wrapped profile {number}", profile=True, wrapped=number > 0)

    pending_bases = []
    for present in (False, True):
        pending = clone(valid[2 if present else 0], f"pending-{present}")
        activate(pending, pending=True)
        pending_bases.append(pending)
        emit(pending, f"import refuses existing pending directory {present}")
        variants = ["complete", "raw-manifest", "inert-epoch", "inert-frame", "inert-both",
                    "root-inert", "root-unknown", "multiple", "empty", "incomplete",
                    "missing-manifest", "missing-frame", "unknown-epoch", "unknown-frame",
                    "temp-directory", "temp-symlink", "temp-fifo", "bad-manifest", "bad-frame",
                    "changed-source", "wrong-token", "new-source-token", "mutation"]
        if present:
            variants += ["source-mode", "backup-mode", "missing-backup"]
        for variant in variants:
            fixture = clone(pending, f"pending-{present}-{variant}")
            base = epoch(fixture)
            frames = base / "frames"
            expected_token = token(fixture)
            if variant == "raw-manifest":
                path = base / "manifest.json"
                path.write_text(json.dumps(ledger._json(path.read_bytes()), indent=3) + " \n")
            elif variant in ("inert-epoch", "inert-both"):
                (base / ".manifest.json.owned_123.tmp").write_bytes(b"unpublished garbage")
            if variant in ("inert-frame", "inert-both"):
                (frames / ".00000000000000000000.frame.owned-123.tmp").write_bytes(b"garbage")
            elif variant in ("root-inert", "root-unknown"):
                (fixture / ledger.LEDGER / (".manifest.json.owned.tmp" if variant == "root-inert" else "README")).write_bytes(b"inert is not allowed at root")
            elif variant == "multiple":
                copytree(base, fixture / ledger.LEDGER / uuid.uuid4().hex)
            elif variant == "empty":
                shutil.rmtree(base)
            elif variant == "incomplete":
                shutil.rmtree(base / "frames")
            elif variant in ("missing-manifest", "missing-frame", "missing-backup"):
                (base / {"missing-manifest": "manifest.json", "missing-frame": "frames/00000000000000000000.frame",
                         "missing-backup": "baseline.tasks.bytes"}[variant]).unlink()
                # An orphan temp is never promoted to regenerate the final.
                (base / ".manifest.json.orphan.tmp").write_bytes(b"not a final")
            elif variant in ("unknown-epoch", "unknown-frame"):
                ((base if variant == "unknown-epoch" else frames) / "unknown.tmp").write_bytes(b"unknown")
            elif variant.startswith("temp-"):
                path = base / ".manifest.json.unsafe.tmp"
                if variant == "temp-directory":
                    path.mkdir()
                elif variant == "temp-symlink":
                    path.symlink_to(fixture / "coven.json")
                else:
                    os.mkfifo(path)
            elif variant in ("bad-manifest", "bad-frame"):
                (base / ("manifest.json" if variant == "bad-manifest" else "frames/00000000000000000000.frame")).write_bytes(b"null\n")
            elif variant in ("changed-source", "new-source-token"):
                (fixture / "state/tasks.json").write_bytes(b"[ ]\n")
                if variant == "new-source-token":
                    expected_token = token(fixture)
            elif variant == "wrong-token":
                expected_token = "tasks-sha256:" + "0" * 64
            elif variant == "source-mode":
                (fixture / "state/tasks.json").chmod(0o600)
            elif variant == "backup-mode":
                (base / "baseline.tasks.bytes").chmod(0o600)
            elif variant == "mutation":
                # Generate real, valid mutation frames via Python, then remove
                # only the marker. Resume must not activate a nonbaseline head.
                original_source = source(fixture)
                with lock(fixture) as held:
                    ledger.LedgerStore(fixture).resume_import(expected_token, held)
                    store = ledger.LedgerStore(fixture)
                    store.publish_projection(store.read(), held)
                append(fixture)
                (fixture / ledger.MARKER).unlink()
                if original_source is None:
                    (fixture / "state/tasks.json").unlink()
                else:
                    (fixture / "state/tasks.json").write_bytes(original_source)
                assert token(fixture) == expected_token
            emit(fixture, f"pending {present} {variant}", "resume", expected_token)
        for variant in ("complete", "empty", "incomplete", "multiple", "corrupt"):
            fixture = clone(pending, f"abort-{present}-{variant}")
            base = epoch(fixture)
            if variant == "empty":
                shutil.rmtree(base)
            elif variant == "incomplete":
                shutil.rmtree(base / "frames")
                (base / "manifest.json").unlink()
            elif variant == "multiple":
                copytree(base, fixture / ledger.LEDGER / uuid.uuid4().hex)
            elif variant == "corrupt":
                (base / "manifest.json").write_bytes(b"corrupt")
                (fixture / "state/tasks.json").write_bytes(b"bad source")
                (fixture / execution.DIR).mkdir()
                (fixture / execution.CONTROL).write_bytes(b"null\n")
                os.mkfifo(base / "unreadable-fifo")
                (base / "dangling-link").symlink_to("does-not-exist")
            emit(fixture, f"abort {present} {variant}", "abort")

    enabled = clone(valid[3], "enabled")
    execution_call(enabled, "enable", {})
    enrolled = clone(enabled, "enrolled")
    execution_call(enrolled, "enroll", {"task_id": "a", "rebind_source": False})
    prepared = clone(enrolled, "prepared")
    action(prepared, "prepare")
    cancelled = clone(prepared, "cancelled")
    action(cancelled, "cancel")
    upgraded = clone(cancelled, "upgraded")
    execution_call(upgraded, "upgrade", {})
    proposed = clone(upgraded, "proposed")
    document = {"schema_version": 1, "kind": "task-graph", "nodes": [
        {**{k: v for k, v in intent().items() if k in ("task_id", "owner", "acceptance_criteria")}, "depends_on": []}]}
    execution_call(proposed, "graph_propose", {"document": document})

    def decide(path, decision):
        view = execution.Store(path).read()
        execution_call(path, "graph_decide", {"graph_revision": view.current_graph_revision,
            "decision": decision, "reason": "fixture review",
            "expected_graph_digest": view.graphs[view.current_graph_revision]["digest"]})

    approved = clone(proposed, "approved")
    decide(approved, "approve")
    graph_prepared = clone(approved, "graph-prepared")
    execution_call(graph_prepared, "prepare_graph", {"task_id": "a",
        "graph_revision": execution.Store(graph_prepared).read().current_graph_revision})
    revoked = clone(graph_prepared, "revoked-prepared")
    decide(revoked, "changes-requested")
    stale = clone(prepared, "stale-prepared")
    (stale / "state/tasks.json").write_bytes(b"[]\n")
    for authority in (enabled, enrolled, prepared, cancelled, upgraded, proposed, approved,
                      graph_prepared, revoked, stale):
        emit(authority, authority.name)
        # Construct pending authority before copying execution, so preparation
        # itself cannot prevent fixture construction.
        fixture = clone(pending_bases[1], "authority-pending-" + authority.name)
        for path in (authority / "state").iterdir():
            if path.name.startswith("execution"):
                if path.is_dir():
                    copytree(path, fixture / "state" / path.name)
                else:
                    shutil.copy2(path, fixture / "state" / path.name)
        emit(fixture, authority.name + " pending", "resume")
        emit(fixture, authority.name + " abort bypasses execution", "abort")
    for shape in ("active", "null", "malformed", "dangling", "directory", "fifo"):
        fixture = clone(pending_bases[1], "marker-" + shape)
        marker = fixture / ledger.MARKER
        if shape == "active":
            with lock(fixture) as held:
                ledger.LedgerStore(fixture).resume_import(token(fixture), held)
        elif shape == "dangling":
            marker.symlink_to(fixture / "missing")
        elif shape == "directory":
            marker.mkdir()
        elif shape == "fifo":
            os.mkfifo(marker)
        else:
            marker.write_bytes(b"null" if shape == "null" else b"{")
        emit(fixture, "abort present marker " + shape, "abort")
        emit(fixture, "import present marker " + shape)
        emit(fixture, "resume present marker " + shape, "resume")
    locks = {}
    for name, original in (("clear", enrolled), ("prepared", prepared), ("resume", enrolled)):
        fixture = clone(original, "lock-" + name)
        if name == "resume":
            # Pending baseline plus an already enabled, unprepared authority.
            other = clone(valid[3], "lock-pending-builder")
            activate(other, pending=True)
            copytree(other / ledger.LEDGER, fixture / ledger.LEDGER)
        locks[name] = str(fixture)
    print(json.dumps(dict(kind="summary", total=index, counts=dict(counts), locks=locks)), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--canonical", action="store_true")
    parser.add_argument("--followup", action="store_true")
    parser.add_argument("--python-resume", action="store_true")
    parser.add_argument("--try-lock", action="store_true")
    parser.add_argument("--hold", type=Path)
    parser.add_argument("--action", choices=("prepare", "cancel", "none", "prepare-current"))
    args = parser.parse_args()
    if args.try_lock:
        try_lock(args.root)
    elif args.hold:
        (args.hold / "waiting-python").touch()
        with lock(args.root) as held:
            (args.hold / "ready").touch()
            deadline = time.monotonic() + 15
            while not (args.hold / "release").exists():
                assert time.monotonic() < deadline, "fixture release timeout"
                time.sleep(0.01)
            if args.action == "prepare-current":
                store = execution.Store(args.root)
                view = store.read()
                tasks = ledger.LedgerStore(args.root).read()
                def load(_snapshot_id):
                    return {"identity": {"kind": "task-ledger-v1", "workspace_id": tasks.workspace_id,
                                         "epoch_id": tasks.epoch_id, "path": ledger.LEDGER},
                            "revision": tasks.revision, "tasks": tasks.tasks, "roster": ["worker"]}
                try:
                    store.mutate("prepare", {"task_id": "a", "intent": intent()},
                        expected_execution_revision=view.revision, expected_tasks_revision=tasks.revision,
                        request_id="current-source-prepare", source_loader=load, lock=held)
                except execution.AttemptError as error:
                    assert "current source identity" in str(error), str(error)
                    (args.hold / "stale-binding").touch()
                else:
                    raise AssertionError("SDK activation automatically rebound the enrollment")
            elif args.action != "none":
                action(args.root, args.action, held)
            (args.hold / "mutated").touch()
    elif args.python_resume:
        (args.root / ledger.MARKER).unlink()
        with lock(args.root) as held:
            view = ledger.LedgerStore(args.root).resume_import(token(args.root), held)
        assert ledger.LedgerStore(args.root).read().revision == view.revision
        print(view.revision)
    elif args.followup:
        with lock(args.root) as held:
            store = ledger.LedgerStore(args.root)
            store.publish_projection(store.read(), held)
        append(args.root)
        append(args.root, "assign")
        print(ledger.LedgerStore(args.root).read().revision)
    elif args.verify:
        verify(args.root, args.canonical)
    else:
        main(args.root)
