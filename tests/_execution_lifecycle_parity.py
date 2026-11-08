"""TEMPORARY Python oracle for the last six execution authoring CLI routes.

Retirement gate: convert these contracts to native fixtures before removing the
test-only oracle. Production children have no Python, Git or runtime on PATH.
Fresh workflows bootstrap through the selected CLI, not through Store/SDK setup.
Private owner faults and worst-case admission proofs are separate required gates,
never claimed as native coverage merely because Python characterizes them.
The fresh graphs have two nodes, not a maximum-size full-lifecycle proof. Each
shared-lock race runs once with an uncontrolled winner, not both lock orderings.
Fresh flows do not invoke recover; that route belongs to the maintenance corpus.
Passing this harness does not establish complete Python retirement.
"""
import argparse
from collections import Counter
from contextlib import ExitStack, chdir, redirect_stderr, redirect_stdout
from datetime import datetime
import fcntl
import io
import json
import os
from pathlib import Path
import select
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import traceback
import uuid
from unittest.mock import patch

sys.dont_write_bytecode = True
from _execution_inspection_parity import (
    ENV, ROOT, exact, intent, normalize_pair, presentation as read_presentation,
)
from _execution_maintenance_parity import presentation
from _execution_parity import resign, snapshot
from runner import coven, execution_v2 as v2, launch, storage, task_attempts as v1, workers

ROUTES = [
    "execution enable [workspace] --expect-execution-revision TOKEN --request-id KEY [--json]",
    "execution enroll [workspace] TASK [--rebind-source] --expect-execution-revision TOKEN --expect-tasks-revision TOKEN --request-id KEY [--json]",
    "execution upgrade [workspace] --expect-execution-revision TOKEN --request-id KEY [--json]",
    "task attempt prepare [workspace] TASK (--intent FILE | --graph-revision REV) --expect-execution-revision TOKEN --expect-tasks-revision TOKEN --request-id KEY [--json]",
    "task graph propose [workspace] --document FILE --expect-execution-revision TOKEN --expect-tasks-revision TOKEN --request-id KEY [--json]",
    "task graph decide [workspace] REV --decision {approve,changes-requested} --reason TEXT --expect-graph-digest DIGEST --expect-execution-revision TOKEN --request-id KEY [--expect-tasks-revision TOKEN] [--json] (tasks pin required for approve; forbidden for changes-requested)",
]
OWNER_GATES = [
    "External owner evidence, not executed by this harness: activation-boundary faults and pending-authority guards",
    "External owner evidence, not executed by this harness: upgrade backup/publication faults, process death and uncertainty receipts",
    "External owner evidence, not executed by this harness: postpublication reread/profile/sync failures and retained reservations/receipts",
    "Capacity owner checks are separate: admission against 8 MiB control, 1024 combined history entries, 256 MiB work and 32 MiB delivery limits; this harness exercises one preparation-headroom refusal",
    "Capacity examples use 3/6 tasks and reason-escaping bounds; pure graph-validator 64-node/256-edge limits are separate evidence, not maximum-size full-lifecycle coverage",
    "Real-coordinator revocation-headroom evidence belongs to the capacity owner; this harness does not certify that external gate",
]


def forbidden(*args, **kwargs):
    # Source admission catches Exception: runtime escape must not become a diagnostic.
    raise RuntimeEscape((args, kwargs))


class RuntimeEscape(BaseException):
    pass


def python_cli(args, cwd, entropy=None):
    out, err = io.StringIO(), io.StringIO()
    with ExitStack() as stack:
        stack.enter_context(chdir(cwd))
        stack.enter_context(redirect_stdout(out))
        stack.enter_context(redirect_stderr(err))
        stack.enter_context(patch.dict(os.environ, ENV, clear=True))
        for target, name in ((subprocess, "Popen"), (subprocess, "run"),
                             (socket, "socket"), (os, "system"),
                             (launch, "execute"), (launch, "stop"),
                             (workers, "inspect_readiness"), (coven, "send_to_agent_pane")):
            stack.enter_context(patch.object(target, name, forbidden))
        if entropy is not None:
            ids, stamp = entropy
            values = iter(ids)
            stack.enter_context(patch.object(v1.uuid, "uuid4", side_effect=lambda: uuid.UUID(hex=next(values))))
            if stamp is not None:
                class Clock(datetime):
                    @classmethod
                    def now(cls, tz=None):
                        return datetime.fromisoformat(stamp)
                for module in (v1, v2):
                    stack.enter_context(patch.object(module, "datetime", Clock))
        try:
            code = coven.main(list(map(str, args)))
        except SystemExit as exc:
            code = exc.code
        if entropy is not None:
            assert list(values) == [], "unused native operation UUID metadata"
    return code, out.getvalue().encode("ascii"), err.getvalue().encode("utf-8")


def operation_entropy(before, after):
    """Only explicit generated metadata, not UUID/time-looking operator strings."""
    if before is not None and before == after:
        return [], None
    ids, stamp = [], None
    if before is None:
        ids += [after["execution_authority_id"], after["snapshot_source_id"]]
    if after["schema_version"] == 2 and before["schema_version"] == 1:
        return [after["upgrade"]["upgrade_id"]], None
    entry = after["history"][-1]
    effect, op = entry["effect"], entry["binding"]["operation"]
    if op == "enroll":
        item = effect["enrollment"]
        if item["binding_number"] == 1:
            ids.append(item["enrollment_id"])
        stamp = item["enrolled_at"]
    elif op in ("prepare", "prepare_graph"):
        ids.append(effect["prepared"]["attempt_id"])
        stamp = effect["prepared"]["prepared_at"]
    elif op == "cancel":
        ids.append(effect["cancel_id"])
        stamp = effect["cancelled_at"]
    elif op == "graph_propose":
        item = effect["manifest"]
        if item["predecessor"] is None:
            ids.append(item["graph_id"])
        ids.append(item["graph_revision"])
        stamp = item["proposed_at"]
    elif op == "graph_decide":
        ids.append(effect["decision"]["decision_id"])
        stamp = effect["decision"]["decided_at"]
    return ids, stamp


def inventory(root):
    return {key: (value[0], *value[3:]) for key, value in snapshot(root).items()}


def outside_execution(tree):
    return {k: v for k, v in tree.items() if k not in (".", "state")
            and k != v1.DIR and not k.startswith(v1.DIR + "/")}


class Suite:
    def __init__(self, root, binary, oracle_only):
        self.root, self.binary, self.oracle_only = root, binary, oracle_only
        self.counts, self.failures = Counter(), []
        self.serial = 0

    def cli(self, args, cwd, code=0, category="workflow", mutation=False, diagnostic=None, profile=None):
        args = list(map(str, args))
        self.serial += 1
        before = snapshot(cwd)
        control = cwd / v1.CONTROL
        initial = v1._json(control.read_bytes()) if control.is_file() else None
        with tempfile.TemporaryDirectory(prefix="lifecycle-operation-", dir=self.root) as temp:
            saved = Path(temp) / "initial"
            if mutation and (cwd / v1.DIR).exists():
                shutil.copytree(cwd / v1.DIR, saved)
            if self.oracle_only:
                actual_code, out, err = python_cli(args, cwd)
            else:
                child = subprocess.run([self.binary, *args], cwd=cwd, env=ENV,
                                       capture_output=True, timeout=15)
                actual_code, out, err = child.returncode, child.stdout, child.stderr
            wanted = 1 if profile and not self.oracle_only else code
            assert actual_code == wanted, (args, actual_code, out[:500], err)
            if wanted:
                assert not out and err and len(err) <= 65536, (args, out, err)
                if profile and not self.oracle_only:
                    assert profile.encode() in err, err
                assert snapshot(cwd) == before, (args, "refusal changed bytes/mode/inode/namespace")
                if not self.oracle_only and not profile:
                    expected_code, expected_out, _ = python_cli(args, cwd)
                    assert expected_code == code and not expected_out
                    assert snapshot(cwd) == before
                self.counts["profile_refusal" if profile else category] += 1
                return None
            assert not err, (args, err)
            value = presentation(out, args) if out.startswith(b"{") else None
            after = snapshot(cwd)
            if mutation:
                # Apart from first persistent lock creation, no task, projection,
                # config, log, checkpoint, dashboard or operator-file write.
                a, b = outside_execution(before), outside_execution(after)
                if "state/.workspace.lock" not in before:
                    b.pop("state/.workspace.lock", None)
                    assert after["state/.workspace.lock"][3:] == ("file", b"")
                assert a == b, (args, "secondary write")
                final = v1._json(control.read_bytes())
                changed = initial is None or v1.canonical(initial) != v1.canonical(final)
                if changed:
                    assert control.read_bytes() == v1.canonical(final) + b"\n"
                    if initial is not None:
                        assert after[v1.CONTROL][0] == before[v1.CONTROL][0]
                        assert after[v1.CONTROL][1] != before[v1.CONTROL][1], "non-atomic control replace"
                    for key, item in before.items():
                        if key.startswith(v1.DIR + "/") and key != v1.CONTROL:
                            if item[3] == "directory":
                                assert after[key][:2] == item[:2], (args, "existing evidence directory replaced", key)
                            else:
                                assert after[key] == item, (args, "existing evidence rewritten", key)
                else:
                    assert after == before, "receipt replay rewrote authority"
                v1.Store(cwd).read()  # Replay the COMPLETE published authority.
                # Exercise entropy extraction/rollback in oracle-only mode too,
                # without claiming that this is native execution evidence.
                native = Path(temp) / "native"
                (cwd / v1.DIR).rename(native)
                if saved.exists():
                    shutil.copytree(saved, cwd / v1.DIR)
                try:
                    expected_code, expected_out, expected_err = python_cli(args, cwd, operation_entropy(initial, final))
                    assert expected_code == code and not expected_err, (args, expected_err)
                    exact(value, presentation(expected_out, args))
                    assert inventory(cwd / v1.DIR) == inventory(native), (args, "full control/backup bytes or modes differ")
                finally:
                    if (cwd / v1.DIR).exists():
                        shutil.rmtree(cwd / v1.DIR)
                    native.rename(cwd / v1.DIR)
            elif category == "read":
                assert after == before, "read mutated workspace"
                if not self.oracle_only:
                    expected_code, expected_out, expected_err = python_cli(args, cwd)
                    assert expected_code == 0 and not expected_err
                    expected = read_presentation(expected_out, "--json" in args)
                    exact(*normalize_pair(value, expected, diagnostic))
                    assert snapshot(cwd) == before
            if value and "authorization" in value:
                assert value["authorization"] == "none" and value["dispatchable"] is False
            self.counts[category] += 1
            return value

    def fresh(self, name, keys=("zeta",)):
        workspace = self.root / name
        workspace.mkdir()
        self.cli(["init", workspace, "--blank", "--agents", "worker,other", "--multiplexer", "tmux"], workspace)
        for key in keys:
            value = self.cli(["task", "add", workspace, key, "--owner", "worker", "--json"], workspace)
            assert value is not None
        values = v1._json((workspace / "state/tasks.json").read_bytes())
        assert [t["id"] for t in values] == list(keys), values
        return workspace

    def pin(self, workspace):
        view = v1.Store(workspace).read()
        return view.revision if view else v1.DISABLED

    def task_pin(self, workspace):
        return self.cli(["task", "list", workspace, "--json"], workspace, category="read")["revision"]

    def args(self, workspace, command, key, selectors=(), flags=(), tasks=False):
        return [*command.split(), workspace, *selectors, *flags,
                "--expect-execution-revision", self.pin(workspace), "--request-id", key,
                *(["--expect-tasks-revision", self.task_pin(workspace)] if tasks else [])]

    def write(self, workspace, command, key, selectors=(), flags=(), tasks=False, compact=False, code=0):
        args = self.args(workspace, command, key, selectors, flags, tasks) + (["--json"] if compact else [])
        return args, self.cli(args, workspace, code, category="authoring", mutation=True)

    def ready(self, workspace, keys):
        enabled, _ = self.write(workspace, "execution enable", "enable")
        for key in keys:
            args, enrolled = self.write(workspace, "execution enroll", "enroll-" + key, [key], tasks=True, compact=True)
            replay = self.cli(args, workspace, category="receipt", mutation=True)
            exact(replay["result"], enrolled["result"])
            assert replay["delivery"]["replayed"]
        return enabled

    def read(self, workspace, command, selectors=(), compact=False, diagnostic=None):
        return self.cli([*command.split(), workspace, *selectors, *(["--json"] if compact else [])],
                        workspace, category="read", diagnostic=diagnostic)

    def cancel(self, workspace, aid, key="cancel"):
        return self.write(workspace, "task attempt cancel", key, [aid], ["--reason", " exact operator cancellation "])[1]


def selected_file(s, workspace, args, flag, value):
    """Selected file must be read/validated even on a known-key retry."""
    original = Path(args[args.index(flag) + 1])
    another = s.root / (workspace.name + flag + "-equal.json")
    another.write_text(json.dumps(value, indent=4) + "\n", encoding="ascii")
    changed = list(args)
    changed[changed.index(flag) + 1] = str(another)
    replay = s.cli(changed, workspace, category="receipt", mutation=True)
    assert replay["delivery"]["replayed"] is True
    raw = original.read_bytes()
    original.unlink()
    try:
        s.cli(args, workspace, code=1, category="document-before-receipt", mutation=True)
        with patch.object(coven, "mutate_execution", forbidden):
            code, out, err = python_cli(args, workspace)
        assert code == 1 and not out and err
        s.counts["oracle_read_before_mutator"] += 1
    finally:
        original.write_bytes(raw)
    different = json.loads(raw)
    node = different if flag == "--intent" else different["nodes"][0]
    node["acceptance_criteria"][0]["text"] += " binding conflict 00000000000000000000000000000000"
    original.write_text(json.dumps(different), encoding="ascii")
    s.cli(args, workspace, code=1, category="binding-conflict", mutation=True)
    original.write_bytes(raw)


def legacy_flow(s):
    w = s.fresh("intent")
    # Harness-owned fixture fields include values Python == would conflate.
    tasks = v1._json((w / "state/tasks.json").read_bytes())
    tasks[0]["extensions"] = {"float": 1.0, "zero": -0.0, "boolean": True,
        "large": 2**90 + 1, "operator": "2026-01-01T00:00:00+00:00 00000000000000000000000000000000"}
    (w / "state/tasks.json").write_bytes(v1.canonical(tasks) + b"\n\n")
    enabled = s.ready(w, ["zeta"])
    control = w / v1.CONTROL
    payload = v1._json(control.read_bytes())
    payload["extensions"] = tasks[0]["extensions"]
    control.write_bytes(v1.canonical(resign(payload)))
    path = s.root / "intent.json"
    path.write_bytes(v1.canonical(intent("zeta")))
    args, first = s.write(w, "task attempt prepare", "intent-prepare", ["zeta"], ["--intent", path], tasks=True)
    aid = first["result"]["attempt_id"]
    exact(first["result"]["attempt"]["complete_task_snapshot"], tasks[0])
    selected_file(s, w, args, "--intent", intent("zeta"))
    conflict = list(args)
    conflict[conflict.index("--expect-execution-revision") + 1] = s.pin(w)
    s.cli(conflict, w, code=1, category="binding-conflict", mutation=True)
    assert s.read(w, "task attempt inspect", [aid], True)["attempt"]["state"] == "prepared"
    s.write(w, "execution upgrade", "blocked-upgrade", code=1)
    s.cli(["task", "assign", w, "zeta", "other", "--json"], w, code=1, category="reservation")
    s.cli(["state", "import-tasks", w, "--expect-revision", s.task_pin(w), "--json"], w, code=1, category="reservation")
    s.cancel(w, aid)
    s.cli(["task", "assign", w, "zeta", "worker", "--json"], w)
    # Exact noncanonical v1 raw bytes and permissions are upgrade evidence.
    payload = v1._json(control.read_bytes())
    control.write_bytes(json.dumps(payload, indent=3).encode("ascii") + b"\n\n")
    control.chmod(0o640)
    raw = control.read_bytes()
    upgrade_args, upgraded = s.write(w, "execution upgrade", "upgrade", compact=True)
    backup = w / upgraded["result"]["backup_path"]
    assert backup.read_bytes() == raw and backup.stat().st_mode & 0o777 == 0o640
    backup.unlink()  # Evidence loss after publication cannot break authority/retry.
    assert s.cli(upgrade_args, w, category="receipt", mutation=True)["delivery"]["replayed"]
    assert s.cli(enabled, w, category="legacy-receipt", mutation=True)["delivery"]["replayed"]
    replay = s.cli(args, w, category="legacy-receipt", mutation=True)
    exact(replay["result"], first["result"])
    assert replay["delivery"]["attempt_state"] == "cancelled_before_submission"
    # Fresh unmanaged v2 intent, then explicit authority import and rebind.
    _, unmanaged = s.write(w, "task attempt prepare", "unmanaged-v2", ["zeta"], ["--intent", path], tasks=True, compact=True)
    assert unmanaged["result"]["attempt"]["schema_version"] == 2
    assert unmanaged["result"]["attempt"]["graph_binding"] is None
    s.cancel(w, unmanaged["result"]["attempt_id"], "cancel-v2")
    s.cli(["state", "import-tasks", w, "--expect-revision", s.task_pin(w), "--json"], w)
    s.write(w, "task attempt prepare", "post-import", ["zeta"], ["--intent", path], tasks=True, code=1)
    s.write(w, "execution enroll", "explicit-rebind", ["zeta"], ["--rebind-source"], tasks=True)
    _, rebound = s.write(w, "task attempt prepare", "post-import", ["zeta"], ["--intent", path], tasks=True)
    assert rebound["result"]["attempt"]["task_authority_identity"]["kind"] == "task-ledger-v1"
    s.cancel(w, rebound["result"]["attempt_id"], "cancel-rebound")
    return w


def graph_report(value, manifest, digest, decisions, prepared, review, eligible, affected=()):
    """Manual COMPLETE typed graph report, not a subset/SDK projection oracle."""
    expected = {"graph_revision": manifest["graph_revision"], "graph_digest": digest,
        "current": True, "manifest": manifest, "planning_review": review,
        "decisions": decisions, "source_binding": {"status": "current", "whole_revision_changed": False, "reasons": []},
        "dependency_order": ["alpha", "zeta"], "preparation_eligible": eligible,
        "nodes": [{"task_id": n["task_id"], "depends_on": n["depends_on"],
            "preparation_eligible": eligible and n["task_id"] != prepared,
            "dependency_acceptance": "unestablished" if n["depends_on"] else "not_required",
            "authorization": "none", "dispatchable": False} for n in manifest["nodes"]],
        "affected_preparations": list(affected), "authorization": "none", "dispatchable": False}
    exact(value, expected)


def graph_flow(s):
    w = s.fresh("graph", ("zeta", "alpha"))
    enabled = s.ready(w, ["zeta", "alpha"])
    s.write(w, "execution upgrade", "upgrade")
    path = s.root / "graph.json"
    document = {"schema_version": 1, "kind": "task-graph", "nodes": [
        {**{k: intent(key)[k] for k in ("task_id", "owner", "acceptance_criteria")},
         "depends_on": ["alpha"] if key == "zeta" else []} for key in ("zeta", "alpha")]}
    path.write_bytes(v1.canonical(document))
    proposal_args, proposal = s.write(w, "task graph propose", "proposal", flags=["--document", path], tasks=True)
    selected_file(s, w, proposal_args, "--document", document)
    result = proposal["result"]
    rid, digest, manifest = result["graph_revision"], result["graph_digest"], result["manifest"]
    graph_report(s.read(w, "task graph inspect")["graph"], manifest, digest, [], None, "proposed", False)
    review_flags = ["--decision", "approve", "--reason", "  exact \\ \" \n \u03bb review  ", "--expect-graph-digest", digest]
    s.write(w, "task attempt prepare", "unreviewed", ["zeta"], ["--graph-revision", rid], tasks=True, code=1)
    review_args, review = s.write(w, "task graph decide", "approve", [rid], review_flags, tasks=True, compact=True)
    # Keep an approved, unreserved fixture for real mixed CLI races.
    race_template = s.root / "approved-races"
    shutil.copytree(w, race_template)
    prep_args, prepared = s.write(w, "task attempt prepare", "downstream", ["zeta"], ["--graph-revision", rid], tasks=True)
    aid = prepared["result"]["attempt_id"]
    graph = s.read(w, "task graph inspect", [rid], True)["graph"]
    assert graph["affected_preparations"] == [aid]
    graph_report(graph, manifest, digest, [review["result"]["decision"]], "zeta", "approved_for_preparation", True, [aid])
    assert graph["decisions"][-1]["decision_id"] == prepared["result"]["attempt"]["graph_binding"]["decision_id"]
    exported = s.read(w, "task graph export", [rid])
    exact(exported, {"manifest": manifest, "graph_digest": digest, "execution_revision": s.pin(w),
                     "authorization": "none", "dispatchable": False})
    s.write(w, "task graph propose", "reserved-successor", flags=["--document", path], tasks=True, code=1)
    originals = {p: (w / p).read_bytes() for p in ("coven.json", "state/tasks.json", "state/agents.json")}
    for relative in originals:
        (w / relative).write_bytes(b"poisoned source\xff")
    for args, frozen in ((proposal_args, proposal), (prep_args, prepared), (review_args, review), (enabled, None)):
        retry = s.cli(args, w, category="receipt-before-source-CAS-roster", mutation=True)
        assert retry["delivery"]["replayed"]
        if frozen:
            exact(retry["result"], frozen["result"])
    revoke_args, revoked = s.write(w, "task graph decide", "revoke", [rid],
                                 ["--decision", "changes-requested", "--reason", "poisoned source is allowed", "--expect-graph-digest", digest])
    exact(s.cli(revoke_args, w, category="receipt-before-source-CAS-roster", mutation=True)["result"], revoked["result"])
    inspected = s.read(w, "task attempt inspect", [aid], diagnostic="source")
    assert inspected["attempt"]["state"] == "prepared"
    assert inspected["attempt"]["graph_binding_status"] == "planning_review_changed"
    exact(inspected["attempt"]["prepared"], prepared["result"]["attempt"])
    s.read(w, "task graph export", [rid], True)
    # Prove revocation did not release the task guard, rather than mistaking a
    # poisoned-source preflight failure for reservation enforcement.
    for relative, raw in originals.items():
        (w / relative).write_bytes(raw)
    s.cli(["task", "assign", w, "zeta", "other", "--json"], w, code=1, category="revoked-reservation")
    for relative in originals:
        (w / relative).write_bytes(b"poisoned source\xff")
    s.cancel(w, aid)
    for relative, raw in originals.items():
        (w / relative).write_bytes(raw)
    # Import requires explicit rebind AND a successor with fresh review.
    s.cli(["state", "import-tasks", w, "--expect-revision", s.task_pin(w), "--json"], w)
    for key in ("zeta", "alpha"):
        s.write(w, "execution enroll", "rebind-" + key, [key], ["--rebind-source"], tasks=True)
    _, successor = s.write(w, "task graph propose", "successor", flags=["--document", path], tasks=True, compact=True)
    new = successor["result"]
    assert new["manifest"]["predecessor"] == rid
    s.write(w, "task attempt prepare", "successor-prepare", ["zeta"], ["--graph-revision", new["graph_revision"]], tasks=True, code=1)
    s.write(w, "task attempt prepare", "old-prepare", ["zeta"], ["--graph-revision", rid], tasks=True, code=1)
    s.write(w, "task graph decide", "successor-review", [new["graph_revision"]],
            ["--decision", "approve", "--reason", "fresh review", "--expect-graph-digest", new["graph_digest"]], tasks=True)
    _, fresh = s.write(w, "task attempt prepare", "successor-prepare", ["zeta"], ["--graph-revision", new["graph_revision"]], tasks=True)
    assert fresh["result"]["attempt"]["graph_binding"]["decision_id"] != review["result"]["decision"]["decision_id"]
    s.cancel(w, fresh["result"]["attempt_id"], "cancel-successor")
    return race_template, path, rid


def refusal_contracts(s):
    w = s.fresh("refusals")
    (w / "state/.workspace.lock").unlink(missing_ok=True)
    raw = (w / "state/tasks.json").read_bytes()
    (w / "state/tasks.json").write_bytes(b"{}")
    s.write(w, "execution enable", "bad-source", code=1)
    assert not (w / "state/.workspace.lock").exists()
    (w / "state/tasks.json").write_bytes(raw)
    s.ready(w, ["zeta"])
    path = s.root / "refusal-intent.json"
    path.write_bytes(v1.canonical(intent("zeta")))
    base = s.args(w, "task attempt prepare", "parser", ["zeta"], ["--intent", path], tasks=True)
    # Representative selection, required flags, schema and conditional pin rules.
    for args, code in ((base + ["--graph-revision", "f" * 32], 2),
                       ([x for x in base if x not in ("--intent", path)], 2),
                       (base[:base.index("--request-id")] + base[base.index("--request-id") + 2:], 2),
                       (base[:base.index("--expect-execution-revision")] + base[base.index("--expect-execution-revision") + 2:], 2),
                       (base[:base.index("--expect-tasks-revision")] + base[base.index("--expect-tasks-revision") + 2:], 2),
                       (base[:4] + ["--json"] + base[4:], 2),
                       (base + ["--invalid-lifecycle-flag"], 2)):
        s.cli(args, w, code, category="flags-selection", mutation=True)
    bad = intent("zeta") | {"argv": ["never", "execute"]}
    path.write_bytes(v1.canonical(bad))
    s.cli(base, w, 1, category="schema", mutation=True)
    path.write_bytes(v1.canonical(intent("zeta")))
    stale = list(base)
    stale[stale.index("--expect-tasks-revision") + 1] = "tasks-missing:v1"
    s.cli(stale, w, 1, category="source-CAS", mutation=True)
    s.write(w, "execution upgrade", "upgrade")
    for decision, tasks in (("approve", False), ("changes-requested", True)):
        s.write(w, "task graph decide", "conditional-pin-" + decision, ["f" * 32],
                ["--decision", decision, "--reason", "exact", "--expect-graph-digest", "task-graph-manifest-v1:" + "f" * 64], tasks=tasks, code=1)


def growth_headroom(s):
    """One Python-valid preparation refused by writer-only escape reservation.

    History/work/output maxima and worst-case escape bounds belong in owner
    units. Do not generate hundreds of CLI histories to approximate that proof.
    """
    w = s.fresh("growth")
    enabled = s.ready(w, ["zeta"])
    control = w / v1.CONTROL
    value = v1._json(control.read_bytes())
    value["extensions"]["padding"] = ["x" * 1024**2] * 7 + [""]
    overhead = len(v1.canonical(value)) + 1
    value["extensions"]["padding"][-1] = "x" * (8 * 1024**2 - 32768 - overhead)
    control.write_bytes(v1.canonical(resign(value)) + b"\n")
    original = control.read_bytes()
    assert len(original) < 8 * 1024**2
    path = s.root / "growth-intent.json"
    path.write_bytes(v1.canonical(intent("zeta")))
    args = s.args(w, "task attempt prepare", "growth-prepare", ["zeta"], ["--intent", path], tasks=True)
    # Characterize Python acceptance using its actual handler, then restore the
    # same initial native inode before checking the stricter explicit profile.
    with tempfile.TemporaryDirectory(dir=s.root) as temp:
        original_path = Path(temp) / "original-control"
        control.rename(original_path)
        control.write_bytes(original)
        control.chmod(original_path.stat().st_mode & 0o777)
        try:
            code, out, err = python_cli(args, w)
            assert code == 0 and not err
            accepted = presentation(out, args)
            assert accepted["result"]["state"] == "prepared"
            v1.Store(w).read()
            s.counts["oracle-growth-characterization"] += 1
        finally:
            control.unlink()
            original_path.rename(control)
    if not s.oracle_only:
        s.cli(args, w, mutation=True, profile="unsupported_execution_escape_capacity")
    assert control.read_bytes() == original
    s.read(w, "execution inspect", compact=True)
    replay = s.cli(enabled, w, category="old-receipt-after-growth-refusal", mutation=True)
    assert replay["delivery"]["replayed"]
    assert control.read_bytes() == original


ORACLE_CHILD = (
    "import sys; sys.dont_write_bytecode=True; "
    f"sys.path.insert(0, {str(ROOT / 'tests')!r}); "
    "from _execution_lifecycle_parity import python_cli; from pathlib import Path; "
    "code,out,err=python_cli(sys.argv[2:],Path(sys.argv[1])); "
    "sys.stdout.buffer.write(out); sys.stderr.buffer.write(err); raise SystemExit(code)"
)


def races(s, template, path, rid):
    """One run per scenario, shared flock barrier, uncontrolled winning order.

    No SDK-under-lock winner and no claim that both lock orderings were tested.
    """
    for scenario in ("CAS", "same-key", "key-conflict", "assignment", "import", "proposal"):
        w = s.root / ("race-" + scenario)
        shutil.copytree(template, w)
        prep = s.args(w, "task attempt prepare", "prepare", ["zeta"], ["--graph-revision", rid], tasks=True) + ["--json"]
        if scenario in ("CAS", "same-key", "key-conflict"):
            other = list(prep)
            if scenario == "CAS":
                other[other.index("--request-id") + 1] = "competitor"
            if scenario == "key-conflict":
                other[other.index("zeta")] = "alpha"
        elif scenario == "assignment":
            other = ["task", "assign", w, "zeta", "other", "--json"]
        elif scenario == "import":
            other = ["state", "import-tasks", w, "--expect-revision", s.task_pin(w), "--json"]
        else:
            other = s.args(w, "task graph propose", "successor", flags=["--document", path], tasks=True) + ["--json"]
        commands = [[sys.executable, "-B", "-c", ORACLE_CHILD, str(w), *map(str, prep)],
                    [sys.executable, "-B", "-c", ORACLE_CHILD, str(w), *map(str, other)] if s.oracle_only
                    else [s.binary, *map(str, other)]]
        if not s.oracle_only and scenario in ("assignment", "proposal"):
            commands = [[s.binary, *map(str, prep)],
                        [sys.executable, "-B", "-c", ORACLE_CHILD, str(w), *map(str, other)]]
        lock = w / "state/.workspace.lock"
        inode = lock.stat().st_ino
        tasks_raw = (w / "state/tasks.json").read_bytes()
        with ExitStack() as stack:
            held = stack.enter_context(lock.open("r+b"))
            fcntl.flock(held, fcntl.LOCK_EX)
            children = []
            for command in commands:
                child = subprocess.Popen(command, cwd=ROOT, env=ENV, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                children.append(child)
                stack.callback(lambda child=child: child.kill() if child.poll() is None else None)
            def opened(child):
                for fd in Path(f"/proc/{child.pid}/fd").iterdir():
                    try:
                        if fd.stat().st_ino == inode and fd.stat().st_dev == lock.stat().st_dev:
                            return True
                    except FileNotFoundError:
                        pass
                return False
            if Path("/proc/self/fd").is_dir():
                deadline = time.monotonic() + 4
                while not all(opened(child) for child in children):
                    assert all(child.poll() is None for child in children), scenario
                    assert time.monotonic() < deadline, (scenario, "shared lock not reached")
                    time.sleep(0.01)
            assert not select.select([c.stdout for c in children], [], [], 0.05)[0]
            assert all(child.poll() is None for child in children)
            fcntl.flock(held, fcntl.LOCK_UN)
            outputs = [child.communicate(timeout=15) for child in children]
        codes = [c.returncode for c in children]
        assert sorted(codes) == ([0, 0] if scenario == "same-key" else [0, 1]), (scenario, codes, outputs)
        for code, (out, err) in zip(codes, outputs):
            assert (not err and out) if code == 0 else (not out and err)
        view = v1.Store(w).read()
        expected_growth = int(codes[0] == 0 or scenario in ("CAS", "same-key", "key-conflict", "proposal"))
        assert view.generation == v1.Store(template).read().generation + expected_growth
        if scenario == "same-key":
            values = [v1._json(out) for out, err in outputs]
            exact(values[0]["result"], values[1]["result"])
            assert sorted(v["delivery"]["replayed"] for v in values) == [False, True]
        if codes[0] == 0:
            assert view.attempts[v1._json(outputs[0][0])["result"]["attempt_id"]]["state"] == "prepared"
            assert (w / "state/tasks.json").read_bytes() == tasks_raw
        elif scenario not in ("CAS", "key-conflict"):
            assert not view.attempts
        assert lock.stat().st_ino == inode
        s.counts["oracle-process-races" if s.oracle_only else "mixed-process-races"] += 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, help="candidate CLI, never a Python fallback")
    parser.add_argument("--oracle-only", action="store_true")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="coven-execution-lifecycle-") as temporary:
        suite = Suite(Path(temporary), str(Path(args.binary).resolve()), args.oracle_only)
        for name, operation in (("fresh-intent", lambda: legacy_flow(suite)),
                                ("fresh-graph", lambda: graph_flow(suite)),
                                ("refusals", lambda: refusal_contracts(suite)),
                                ("growth-headroom", lambda: growth_headroom(suite))):
            try:
                result = operation()
                suite.counts[name] += 1
                if name == "fresh-graph":
                    races(suite, *result)
            except Exception:
                suite.failures.append({"gate": name, "error": traceback.format_exc()[-6000:]})
        print(json.dumps({"cases": sum(suite.counts.values()), "cli_cases": suite.serial,
            "categories": dict(suite.counts), "failures": suite.failures,
            "oracle_only": args.oracle_only, "native_executed": not args.oracle_only,
            "required_main_routes": ROUTES, "owner_unit_gates": OWNER_GATES,
            "coverage_limits": ["Fresh graph flows use two nodes, not a 64-node/256-edge lifecycle",
                "Six shared-lock races run once each; winners are uncontrolled, not both orderings",
                "Fresh flows do not invoke execution recover; separate maintenance corpus covers it",
                "External owner fault/capacity results are not collected or certified by this harness"],
            "blockers": ["Complete Python retirement is outside this six-route gate; task list/action inspect without --json remain separate cutover inventory gaps"],
            "oracle_retirement": "TEMPORARY test-only: convert to native fixtures before Python retirement",
            "claims_process_stop_or_dispatch": False}, ensure_ascii=True))
        return bool(suite.failures)


if __name__ == "__main__":
    raise SystemExit(main())
