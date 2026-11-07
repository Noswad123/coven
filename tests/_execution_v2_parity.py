"""Differential, read-only v2 replay corpus, generated from the locked Python API.

No CLI mutation, provider, runtime, current configuration or backup is authority.
Graph primitive size boundaries belong to the focused graph unit suite; this
corpus exercises retained full sources, cross-version replay and prefix guards.
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
from unittest.mock import patch

sys.set_int_max_str_digits(4300)
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from runner import execution_v2 as v2, storage, task_attempts as v1, task_graphs as graphs


def snapshot(root):
    result = {}

    def visit(path):
        metadata = path.lstat()
        identity = (metadata.st_mode, metadata.st_ino, metadata.st_mtime_ns)
        key = str(path.relative_to(root))
        if stat.S_ISLNK(metadata.st_mode):
            result[key] = (*identity, "link", os.readlink(path))
        elif stat.S_ISDIR(metadata.st_mode):
            result[key] = (*identity, "directory")
            for child in sorted(path.iterdir()):
                visit(child)
        elif stat.S_ISREG(metadata.st_mode):
            result[key] = (*identity, "file", path.read_bytes())
        else:
            result[key] = (*identity, "special", metadata.st_rdev)
    visit(root)
    return result


def report(view):
    return {"status": "active", "schema_version": 2,
            "execution_authority_id": view.authority_id,
            "snapshot_source_id": view.snapshot_source_id,
            "generation": view.generation, "revision": view.revision,
            "enrollments": view.enrollments, "attempts": view.attempts,
            "receipts": view.receipts, "extensions": view.extensions,
            "reserved_task_ids": sorted({a["prepared"]["task_id"]
                for a in view.attempts.values() if a["state"] == "prepared"}),
            "graph_id": view.graph_id, "current_graph_revision": view.current_graph_revision,
            "graphs": view.graphs, "graph_managed_tasks": sorted(view.managed_tasks),
            "authorization": "none", "dispatchable": False,
            "current_source": "not_assessed", "durability": "not_assessed",
            "assessment": "historical_authority_only"}


def resign_v1(control):
    for index in range(1, len(control["history"])):
        prefix = {**control, "generation": index, "history": control["history"][:index]}
        control["history"][index]["binding"]["expected_execution_revision"] = v1._revision(prefix)


def resign(control, legacy_changed=False):
    """Re-sign every prefix, including the v1/v2 bridge if legacy was changed."""
    if legacy_changed:
        resign_v1(control["legacy_v1"])
        revision = v1._revision(control["legacy_v1"])
        control["upgrade"]["source_revision"] = revision
        control["history"][0]["effect"]["source_revision"] = revision
        control["history"][0]["result"]["source_revision"] = revision
    previous = v1._revision(control["legacy_v1"])
    for index, entry in enumerate(control["history"]):
        entry["binding"]["expected_execution_revision"] = previous
        prefix = {**control, "generation": entry["generation"],
                  "history": control["history"][:index + 1]}
        previous = v2._revision(prefix)
    return control


def main(binary, oracle_only=False):
    counts = {"parity": 0, "corruption_refusals": 0, "native_profile_refusals": 0}
    names = set()
    # Representative semantic fixtures must fail at the intended guard, not a
    # later receipt mismatch or an unrelated stale binding.
    refusals = {
        "duplicate-decision": ("duplicate decision ID", "duplicate_execution_graph_decision"),
        "decision-id-across-graphs": ("duplicate decision ID", "duplicate_execution_graph_decision"),
        "global-cancel-id": ("duplicate cancellation identity", "duplicate_execution_cancellation_id"),
        "request-global": ("duplicate request key", "duplicate_execution_request_id"),
        "v2-duplicate-request": ("duplicate request key", "duplicate_execution_request_id"),
        "duplicate-graph-revision": ("duplicate graph revision", "duplicate_execution_graph_revision"),
        "sticky-standalone-ban": ("graph-managed task requires prepare_graph, not standalone intent",
                                  "execution_graph_managed_task"),
        "old-head-decision": ("graph revision must be the current head", "execution_graph_not_head"),
        "old-head-prepare": ("graph revision must be the current head", "execution_graph_not_head"),
        "early-capacity-then-revoke": (
            "execution capacity must retain cancellation and graph review reserves",
            "execution_cancellation_capacity_exceeded"),
        "early-capacity-then-cancel": (
            "execution capacity must retain cancellation and graph review reserves",
            "execution_cancellation_capacity_exceeded"),
    }
    with tempfile.TemporaryDirectory(prefix="native-execution-v2-") as temporary:
        root = Path(temporary)
        binary = str(Path(binary).resolve())

        def check(workspace, accepted=True, profile=False, no_python=False,
                  diagnostic=None, native_error=None):
            if workspace.name in refusals:
                diagnostic, native_error = refusals[workspace.name]
            before = snapshot(root)
            try:
                view = v1.Store(workspace).read()
            except (storage.StorageError, OSError, ValueError) as error:
                assert not accepted and not profile, (workspace.name, error)
                if diagnostic:
                    assert diagnostic in str(error), (workspace.name, error)
            else:
                assert accepted or profile, (workspace.name, "Python accepted corruption")
                assert view.schema_version == 2
            if not oracle_only:
                env = {**os.environ, "COVEN_MULTIPLEXER": ""}
                if no_python:
                    env.update(PATH=str(root / "no-programs"), PYTHONPATH=str(root / "no-python"))
                result = subprocess.run([binary, "execution", "history", str(workspace), "--json"],
                    cwd=root, env=env, capture_output=True, timeout=15)
                if accepted and not profile:
                    assert result.returncode == 0, (workspace.name, result.stdout, result.stderr)
                    actual = v1._json(result.stdout)
                    assert v1.canonical(actual) == v1.canonical(report(view)), (
                        workspace.name, report(view), actual)
                else:
                    assert result.returncode != 0, (workspace.name, result.stdout, result.stderr)
                    assert not result.stdout.strip(), (workspace.name, "success JSON on refusal")
                    if native_error:
                        assert native_error.encode() in result.stderr, (workspace.name, result.stderr)
            assert snapshot(root) == before, (workspace.name, "history changed filesystem")
            counts["native_profile_refusals" if profile else
                   "parity" if accepted else "corruption_refusals"] += 1

        def load(workspace):
            return v1._json((workspace / v1.CONTROL).read_bytes())

        def save(workspace, control):
            (workspace / v1.CONTROL).write_bytes(v1.canonical(control) + b"\n")

        def clone(workspace, name):
            assert name not in names, name
            names.add(name)
            target = root / name
            shutil.copytree(workspace, target, symlinks=True)
            v1.Store(target).read()
            return target

        def corrupt(name, workspace, change, legacy=False):
            target = clone(workspace, name)
            control = load(target)
            change(control)
            save(target, resign(control, legacy))
            check(target, accepted=False)

        tasks = [{"id": key, "title": key, "status": "operator-state", "owner": "Worker",
                  "extension": {"unicode": "\u03bb\U0001f98a", "number": 1.0, "zero": -0.0,
                                "big": 2**90 + 1}} for key in ("a", "b", "c", "unrelated")]
        ledger = False

        def source(snapshot_id):
            identity = {"kind": "snapshot-v1", "source_id": snapshot_id, "path": "state/tasks.json"}
            revision = storage.snapshot_revision(v1.canonical(tasks))
            if ledger:
                identity = {"kind": "task-ledger-v1", "workspace_id": "1" * 32,
                            "epoch_id": "2" * 32, "path": "state/task-ledger"}
                revision = ("tasks-ledger:v1:" + "1" * 32 + ":" + "2" * 32 + ":7:"
                            + hashlib.sha256(v1.canonical(tasks)).hexdigest())
            return {"identity": identity, "revision": revision,
                    "tasks": deepcopy(tasks), "roster": ["Worker", "other"]}

        def intent(key):
            return {"schema_version": 1, "kind": "task-intent", "task_id": key, "owner": "Worker",
                    "acceptance_criteria": [{"id": "check", "text": "  exact \u03bb criterion  "}]}

        def document(keys=("b", "a")):
            return {"schema_version": 1, "kind": "task-graph", "nodes": [
                {**{k: intent(key)[k] for k in ("task_id", "owner", "acceptance_criteria")},
                 "depends_on": ["a"] if key == "b" and "a" in keys else []} for key in keys]}

        counter = 0

        def call(workspace, op, params):
            nonlocal counter
            counter += 1
            store = v1.Store(workspace)
            view = store.read()
            needs = op in ("enroll", "prepare", "prepare_graph", "graph_propose") or (
                op == "graph_decide" and params["decision"] == "approve")
            with storage.workspace_lock(workspace, workspace / "state/.workspace.lock") as lock:
                result = store.mutate(op, params,
                    expected_execution_revision=view.revision if view else v1.DISABLED,
                    expected_tasks_revision=source(view.snapshot_source_id)["revision"] if needs else None,
                    request_id=f"request-{counter}", source_loader=source, lock=lock)
            assert result["authorization"] == "none" and result["dispatchable"] is False
            assert result["delivery"]["outcome"] == "committed"
            store.read()
            return result["result"]

        def propose(workspace, keys=("b", "a")):
            return call(workspace, "graph_propose", {"document": document(keys)})

        def decide(workspace, decision="approve"):
            view = v1.Store(workspace).read()
            return call(workspace, "graph_decide", {"graph_revision": view.current_graph_revision,
                "decision": decision, "reason": "reviewed locally",
                "expected_graph_digest": view.graphs[view.current_graph_revision]["digest"]})

        def prepare(workspace, key):
            return call(workspace, "prepare_graph", {"task_id": key,
                "graph_revision": v1.Store(workspace).read().current_graph_revision})

        def cancel(workspace, attempt):
            return call(workspace, "cancel", {"attempt_id": attempt["attempt_id"], "reason": "cancel locally"})

        legacy = root / "legacy"
        legacy.mkdir()
        call(legacy, "enable", {})
        control = load(legacy)
        control["extensions"] = {"opaque": [True, None, 1.0, -0.0, 2**100, "\u03bb"]}
        save(legacy, control)
        for key in ("a", "b", "c"):
            call(legacy, "enroll", {"task_id": key, "rebind_source": False})
        old_attempt = call(legacy, "prepare", {"task_id": "a", "intent": intent("a")})
        active_legacy = clone(legacy, "active-legacy")
        cancel(legacy, old_attempt)
        old_view = v1.Store(legacy).read()
        path = legacy / v1.CONTROL
        path.write_bytes(json.dumps(load(legacy), indent=3).encode() + b"\n\n")
        path.chmod(0o640)
        raw = path.read_bytes()
        with storage.workspace_lock(legacy, legacy / "state/.workspace.lock") as lock:
            v2.upgrade(v1.Store(legacy), old_view.revision, "upgrade", lock)
        upgraded = clone(legacy, "upgraded")
        new_view = v1.Store(upgraded).read()
        assert v1.canonical(new_view.attempts) == v1.canonical(old_view.attempts)
        assert all(v1.canonical(new_view.receipts[k]) == v1.canonical(value)
                   for k, value in old_view.receipts.items())
        assert load(upgraded)["upgrade"]["source_sha256"] == hashlib.sha256(raw).hexdigest()
        assert new_view.attempts[old_attempt["attempt_id"]]["prepared"]["schema_version"] == 1
        check(upgraded)
        check(upgraded, no_python=True)

        standalone = clone(upgraded, "standalone")
        standalone_attempt = call(standalone, "prepare", {"task_id": "a", "intent": intent("a")})
        record = standalone_attempt["attempt"]
        assert record["graph_binding"] is None and record["schema_version"] == 2
        assert record["previous_attempt_id"] == old_attempt["attempt_id"] and record["attempt_number"] == 2
        check(standalone, no_python=True)

        history = clone(upgraded, "graph-history")
        tasks[0]["title"] = "current task, not enrollment snapshot"
        proposal = propose(history)
        assert proposal["manifest"]["nodes"][1]["complete_task_snapshot"]["title"] == tasks[0]["title"]
        proposed = clone(history, "proposed")
        check(proposed)
        decide(history)
        decide(history, "changes-requested")
        decide(history, "changes-requested")
        decide(history)
        decide(history)
        approved = clone(history, "approved")
        check(approved)
        downstream = prepare(history, "b")
        bound = clone(history, "bound")
        check(bound, no_python=True)
        decide(history, "changes-requested")
        revoked = clone(history, "revoked-reserved")
        assert report(v1.Store(revoked).read())["reserved_task_ids"] == ["b"]
        check(revoked)
        cancel(history, downstream)
        propose(history, ("c",))
        removed = clone(history, "removed")
        assert v1.Store(removed).read().managed_tasks == {"a", "b", "c"}
        check(removed)
        decide(history)
        propose(history)
        reintroduced = clone(history, "reintroduced-fresh-review")
        assert v1.Store(reintroduced).read().graphs[proposal["graph_revision"]]["manifest"] == proposal["manifest"]
        check(reintroduced)
        decide(history)
        retry = prepare(history, "a")
        assert retry["attempt"]["previous_attempt_id"] == old_attempt["attempt_id"]
        cancel(history, retry)
        assert v1.Store(history).read().graphs[v1.Store(history).read().current_graph_revision]["current_review"] == "approved_for_preparation"
        check(history)

        unrelated = clone(upgraded, "unmanaged-reservation")
        call(unrelated, "prepare", {"task_id": "c", "intent": intent("c")})
        propose(unrelated)
        decide(unrelated)
        check(unrelated)
        drift = clone(proposed, "unrelated-source-drift")
        tasks[-1]["extension"]["number"] = 2
        approval = decide(drift)
        assert approval["source_match"] == {"status": "current", "whole_revision_changed": True, "reasons": []}
        prepare(drift, "b")
        check(drift)

        # Every mutation has valid downstream hashes; effect receipts are also
        # repaired where needed to reach the intended semantic guard.
        for field, value in (("schema_version", True), ("generation", 1.0),
                             ("extra", 1), ("extensions", []),
                             ("execution_authority_id", "f" * 32), ("snapshot_source_id", "f" * 32)):
            corrupt("control-" + field, bound, lambda c, k=field, v=value: c.update({k: v}))
        corrupt("extensions-mismatch", bound, lambda c: c["extensions"].update(extra=1))
        for field, value in (("upgrade_id", "bad"), ("source_sha256", "G" * 64),
                             ("source_mode", True), ("source_mode", -1), ("source_mode", 4096),
                             ("backup_path", "../backup"), ("source_revision", old_view.revision + "x"),
                             ("extra", None)):
            corrupt("metadata-" + field + "-" + str(value), upgraded,
                    lambda c, k=field, v=value: c["upgrade"].update({k: v}))
        corrupt("active-legacy-upgrade", upgraded,
                lambda c: c.update(legacy_v1=load(active_legacy), generation=load(active_legacy)["generation"] + 1,
                    history=[{**c["history"][0], "generation": load(active_legacy)["generation"] + 1}]), legacy=True)
        corrupt("legacy-receipt", bound,
                lambda c: c["legacy_v1"]["history"][1]["result"].update(generation=999), legacy=True)

        def entry_cases(workspace, index, cases):
            for name, change in cases:
                corrupt(name, workspace, lambda c, fn=change: fn(c["history"][index]))

        entry_cases(bound, 1, [
            ("request-global", lambda e: e.update(request_id=next(iter(old_view.receipts)))),
            ("binding-version-bool", lambda e: e["binding"].update(schema_version=True)),
            ("binding-extra", lambda e: e["binding"].update(extra=None)),
            ("parameters-extra", lambda e: e["binding"]["parameters"].update(extra=1)),
            ("manifest-extra", lambda e: e["effect"]["manifest"].update(extra=1)),
            ("manifest-bool", lambda e: e["effect"]["manifest"].update(schema_version=True)),
            ("wrong-family", lambda e: e["effect"]["manifest"].update(graph_id="f" * 32)),
            ("wrong-predecessor", lambda e: e["effect"]["manifest"].update(predecessor="f" * 32)),
            ("wrong-authority", lambda e: e["effect"]["manifest"].update(execution_authority_id="f" * 32)),
            ("source-path", lambda e: e["effect"]["source"]["identity"].update(path="../tasks.json")),
            ("source-id", lambda e: e["effect"]["source"]["identity"].update(source_id="f" * 32)),
            ("source-revision", lambda e: e["effect"]["source"].update(revision="tasks-missing:v1")),
            ("document-empty", lambda e: e["binding"]["parameters"]["document"].update(nodes=[])),
            ("document-cycle", lambda e: e["binding"]["parameters"]["document"]["nodes"][1].update(depends_on=["b"])),
        ])
        # The first graph chooses its own family ID, so changing it consistently
        # is valid; the above one-sided change must still fail receipt equality.
        def refreeze_change(e, field, value):
            node = e["effect"]["manifest"]["nodes"][0]
            if field == "complete_task_snapshot":
                node[field]["extension"]["number"] = value
                node["task_snapshot_digest"] = v1.task_snapshot_digest(node[field])
            else:
                node[field] = value
                if field in ("owner", "task_id"):
                    node["complete_task_snapshot"]["owner" if field == "owner" else "id"] = value
                    node["task_snapshot_digest"] = v1.task_snapshot_digest(node["complete_task_snapshot"])
            node["node_digest"] = graphs.node_digest(node)
            e["result"]["manifest"] = deepcopy(e["effect"]["manifest"])
            e["result"]["graph_digest"] = graphs.digest(e["effect"]["manifest"])

        for field, value in (("complete_task_snapshot", 1), ("owner", "other"), ("task_id", "c"),
                             ("acceptance_criteria", [{"id": "check", "text": "tampered"}]),
                             ("depends_on", []), ("enrollment_id", "f" * 32), ("binding_number", 2)):
            entry_cases(proposed, 1, [("refreeze-" + field,
                lambda e, k=field, v=value: refreeze_change(e, k, v))])

        decision_index = len(load(approved)["history"]) - 1
        prepare_index = len(load(bound)["history"]) - 1
        duplicate_decision_id = load(approved)["history"][2]["effect"]["decision"]["decision_id"]

        def duplicate_decision(e):
            e["effect"]["decision"]["decision_id"] = duplicate_decision_id
            e["result"]["decision"] = deepcopy(e["effect"]["decision"])

        entry_cases(approved, decision_index, [
            ("duplicate-decision", duplicate_decision),
            ("decision-reason", lambda e: e["effect"]["decision"].update(reason="tampered")),
            ("decision-state", lambda e: e["effect"]["decision"].update(state="accepted")),
            ("decision-extra", lambda e: e["effect"]["decision"].update(extra=1)),
            ("digest-conflict", lambda e: e["binding"]["parameters"].update(expected_graph_digest="task-graph-manifest-v1:" + "0" * 64)),
        ])
        for field in ("graph_id", "graph_revision", "decision_id", "graph_digest", "node_digest"):
            def wrong_binding(e, k=field):
                record = e["effect"]["prepared"]
                record["graph_binding"][k] = "f" * 32 if k.endswith("id") or k == "graph_revision" else "wrong-domain:" + "0" * 64
                record["intent_digest"] = v2._intent_digest(record)
                e["result"]["attempt"] = deepcopy(record)
            entry_cases(bound, prepare_index, [("graph-binding-" + field, wrong_binding)])
        entry_cases(bound, prepare_index, [
            ("prepared-version-bool", lambda e: e["effect"]["prepared"].update(schema_version=True)),
            ("prepared-predecessor", lambda e: e["effect"]["prepared"].update(previous_attempt_id="f" * 32)),
            ("prepared-extra", lambda e: e["effect"]["prepared"].update(extra=1)),
            ("prepared-null-binding", lambda e: e["effect"]["prepared"].update(graph_binding=None)),
            ("receipt-state", lambda e: e["result"].update(state="accepted")),
        ])
        def wrong_domain(e):
            record = e["effect"]["prepared"]
            record["intent_digest"] = v1.intent_digest(v1._intent_payload(record))
            e["result"]["attempt"] = deepcopy(record)
        entry_cases(standalone, -1, [("standalone-null-intent-domain", wrong_domain)])
        entry_cases(bound, prepare_index, [("graph-intent-domain", wrong_domain)])

        def append_case(name, workspace, template, change=lambda e: None):
            target = clone(workspace, name)
            control = load(target)
            entry = deepcopy(template)
            generation = control["generation"] + 1
            entry.update(generation=generation, request_id="synthetic-" + name)
            entry["result"]["generation"] = generation
            change(entry)
            control["generation"] = generation
            control["history"].append(entry)
            save(target, resign(control))
            check(target, accepted=False)

        append_case("sticky-standalone-ban", removed, load(standalone)["history"][-1])
        append_case("unique-upgrade", upgraded, load(upgraded)["history"][0])
        append_case("old-head-decision", reintroduced, load(approved)["history"][2])
        append_case("old-head-prepare", removed, load(bound)["history"][-1])
        append_case("unreviewed-successor", reintroduced, load(bound)["history"][-1],
                    lambda e: e["binding"]["parameters"].update(
                        graph_revision=v1.Store(reintroduced).read().current_graph_revision))
        def duplicate_cancellation(e):
            legacy_cancel = next(r for r in old_view.receipts.values()
                                 if r["binding"]["operation"] == "cancel")
            e["effect"]["cancel_id"] = legacy_cancel["effect"]["cancel_id"]
            e["result"]["transition"] = deepcopy(e["effect"])
        entry_cases(history, -1, [("global-cancel-id", duplicate_cancellation)])
        def old_decision_binding(e):
            record = e["effect"]["prepared"]
            record["graph_binding"]["decision_id"] = load(approved)["history"][2]["effect"]["decision"]["decision_id"]
            record["intent_digest"] = v2._intent_digest(record)
            e["result"]["attempt"] = deepcopy(record)
        entry_cases(bound, prepare_index, [("latest-decision-binding", old_decision_binding)])
        entry_cases(approved, decision_index, [("v2-duplicate-request", lambda e: e.update(request_id="upgrade"))])

        # A successor must retain the graph family, not just a well-formed UUID.
        successor_index = next(i for i, e in enumerate(load(removed)["history"])
                               if e["binding"]["operation"] == "graph_propose" and i > 1)
        def manifest_header(e, field, value):
            e["effect"]["manifest"][field] = value
            e["result"]["manifest"] = deepcopy(e["effect"]["manifest"])
            e["result"]["graph_digest"] = graphs.digest(e["effect"]["manifest"])
            for key in ("graph_id", "graph_revision"):
                e["result"][key] = e["effect"]["manifest"][key]
        entry_cases(removed, successor_index, [
            ("successor-family", lambda e: manifest_header(e, "graph_id", "f" * 32)),
            ("successor-predecessor", lambda e: manifest_header(e, "predecessor", None)),
            ("duplicate-graph-revision", lambda e: manifest_header(e, "graph_revision", proposal["graph_revision"])),
        ])
        def duplicate_decision_across_graphs(control):
            duplicate_decision(control["history"][-3])
            # Keep the later preparation valid if uniqueness enforcement is
            # removed, so its latest-decision check cannot mask the regression.
            entry = control["history"][-2]
            record = entry["effect"]["prepared"]
            record["graph_binding"]["decision_id"] = duplicate_decision_id
            record["intent_digest"] = v2._intent_digest(record)
            entry["result"]["attempt"] = deepcopy(record)
        corrupt("decision-id-across-graphs", history, duplicate_decision_across_graphs)

        # Changing a different selected node must block downstream preparation.
        def selected_drift(e):
            e["effect"]["source"]["tasks"][0]["extension"]["number"] = 1
        entry_cases(bound, prepare_index, [("other-selected-extension-drift", selected_drift)])
        entry_cases(approved, decision_index, [("approval-selected-extension-drift", selected_drift)])

        # These synthetic operations use the real producer with only the replay
        # guard disabled, then normal Store.read must reject the retained proof.
        def forbidden(name, workspace, operation):
            target = clone(workspace, name)
            with patch.object(v2, "_no_prepared", return_value=None):
                operation(target)
            check(target, accepted=False)
        forbidden("approve-while-prepared", bound, decide)
        forbidden("propose-while-prepared", bound, lambda w: propose(w, ("c",)))

        # Identity change is permitted only through a real enrollment rebind.
        ledger = True
        ledger_history = clone(upgraded, "ledger-full-source")
        for key in ("a", "b"):
            call(ledger_history, "enroll", {"task_id": key, "rebind_source": True})
        propose(ledger_history)
        decide(ledger_history)
        prepare(ledger_history, "b")
        check(ledger_history)
        ledger_proposal = next(i for i, e in enumerate(load(ledger_history)["history"])
                               if e["binding"]["operation"] == "graph_propose")
        entry_cases(ledger_history, ledger_proposal, [
            ("ledger-unselected-digest", lambda e: e["effect"]["source"]["tasks"][-1].update(title="tampered")),
            ("ledger-epoch", lambda e: e["effect"]["source"]["identity"].update(epoch_id="f" * 32)),
        ])
        # Match must validate the full source even for an unselected ledger task.
        entry_cases(ledger_history, -1, [("ledger-prepare-unselected-digest",
            lambda e: e["effect"]["source"]["tasks"][-1].update(title="tampered"))])

        # Historical reads must ignore every current or evidence-only path,
        # even if opening it would block or following it would escape the root.
        backup = load(upgraded)["upgrade"]["backup_path"]
        for name in ("coven.json", "state/tasks.json", "state/agents.json", "state/task-ledger",
                     "state/task-authority.json", "checkpoints", "state/runtime-launch-control.json",
                     "state/.runtime-transport.lock", "state/.workspace.lock", backup):
            for kind in ("missing", "corrupt", "fifo", "symlink"):
                target = clone(bound, "ignored-" + name.replace("/", "-") + "-" + kind)
                path = target / name
                if path.exists():
                    if path.is_dir():
                        shutil.rmtree(path)
                    else:
                        path.unlink()
                path.parent.mkdir(parents=True, exist_ok=True)
                if kind == "corrupt":
                    path.write_bytes(b"not JSON\xff")
                elif kind == "fifo":
                    os.mkfifo(path)
                elif kind == "symlink":
                    path.symlink_to(root / "missing-evidence")
                check(target, no_python=True)
                control = load(target)
                control["history"][-1]["result"]["state"] = "accepted"
                save(target, control)
                check(target, accepted=False)
        held = clone(bound, "held-lock")
        with storage.workspace_lock(held, held / "state/.workspace.lock"):
            check(held, no_python=True)
            control = load(held)
            control["history"][-1]["result"]["state"] = "accepted"
            save(held, control)
            check(held, accepted=False)

        uncorrelated = clone(upgraded, "uncorrelated-raw-hash")
        control = load(uncorrelated)
        control["upgrade"]["source_sha256"] = "0" * 64
        control["history"][0]["effect"]["source_sha256"] = "0" * 64
        save(uncorrelated, resign(control))
        check(uncorrelated)

        # Python accepts controls over the native raw limit; do not mislabel
        # these as semantic corruption or let a future implementation accept it.
        large = clone(upgraded, "native-raw-profile")
        control = load(large)
        padding = ["x" * (1024 * 1024)] * 4
        control["extensions"]["padding"] = padding
        control["legacy_v1"]["extensions"]["padding"] = padding
        save(large, resign(control, legacy_changed=True))
        check(large, accepted=False, profile=True)

        # The combined preflight rejects this raw <8 MiB control. Each
        # version's prefix sum fits separately, but their total exceeds the
        # budget. This does NOT reach or prove the shared replay work meter;
        # its exact boundary is covered by the core's focused private unit test.
        shared_work = clone(history, "combined-cross-version-preflight-profile")
        while len(load(shared_work)["history"]) < 40:
            decide(shared_work, "changes-requested")
        control = load(shared_work)
        padding = ["w" * (1024 * 1024)] * 3
        control["extensions"]["padding"] = padding
        control["legacy_v1"]["extensions"]["padding"] = padding
        resign(control, legacy_changed=True)
        legacy_control = control["legacy_v1"]
        legacy_work = sum(len(v1.canonical({**legacy_control, "generation": i,
            "history": legacy_control["history"][:i]}))
            for i in range(1, len(legacy_control["history"]) + 1))
        v2_work = sum(len(v1.canonical({**control, "generation": e["generation"],
            "history": control["history"][:i + 1]})) for i, e in enumerate(control["history"]))
        assert max(legacy_work, v2_work) < 256 * 1024 * 1024 < legacy_work + v2_work
        assert len(v1.canonical(control)) < 8 * 1024 * 1024
        save(shared_work, control)
        check(shared_work, accepted=False, profile=True)

        # Generate an authority with an invalid EARLY approved+prepared prefix,
        # then release each kind of reserve. The final state alone fits, but
        # replay must reject both histories without ignoring the earlier breach.
        ledger = False
        capacity_tasks = [{"id": f"capacity-{i}", "title": "Capacity", "status": "pending", "owner": "Worker"}
                          for i in range(31)]
        capacity = clone(upgraded, "capacity-base")
        for task in capacity_tasks:
            tasks[:] = [task]
            call(capacity, "enroll", {"task_id": task["id"], "rebind_source": False})
        tasks[:] = capacity_tasks[:1]
        propose(capacity, (capacity_tasks[0]["id"],))
        decide(capacity)
        for task in capacity_tasks[1:]:
            tasks[:] = [task]
            call(capacity, "prepare", {"task_id": task["id"], "intent": intent(task["id"])})
        check(capacity)
        tasks[:] = capacity_tasks[:1]
        with patch.object(v2, "MAX_CONTROL", 32 * 1024 * 1024):
            overflow = prepare(capacity, tasks[0]["id"])
            # Copy while the relaxed producer budget is in scope; after this
            # point all checks run with Python's actual 16 MiB reserve budget.
            revoked_capacity = clone(capacity, "early-capacity-then-revoke")
            decide(revoked_capacity, "changes-requested")
            cancelled_capacity = clone(capacity, "early-capacity-then-cancel")
            cancel(cancelled_capacity, overflow)
        for target in (revoked_capacity, cancelled_capacity):
            control = load(target)
            assert len(v1.canonical(control)) < 8 * 1024 * 1024
            final_active = 31 if target == revoked_capacity else 30
            final_approved = target == cancelled_capacity
            assert len(v1.canonical(control)) + 1 + final_active * v2.CANCELLATION_RESERVE + final_approved * v2.GRAPH_REVIEW_RESERVE < v2.MAX_CONTROL
            check(target, accepted=False)

    print(json.dumps({**counts, "total": sum(counts.values()), "oracle_only": oracle_only,
        "native_writes_enabled": False, "schema_versions": [1, 2]}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--oracle-only", action="store_true")
    args = parser.parse_args()
    main(args.binary, args.oracle_only)
