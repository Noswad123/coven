"""Five real CLI read routes, using disposable Python-produced authority only.

The oracle runs coven.main (parser AND handlers), never a handwritten report.
Native reads run as an external tool with an empty runtime PATH. Race seams are
oracle characterization, not a substitute for the inspection owner's native
fault/Work/output-admission unit tests. No runtime mutation handler is invoked.
"""
import argparse
import builtins
from collections import Counter
from contextlib import ExitStack, chdir, contextmanager, redirect_stderr, redirect_stdout
from copy import deepcopy
import fcntl
import io
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.set_int_max_str_digits(4300)
from runner import coven, execution_v2 as v2, storage, task_attempts as v1, task_ledger
from _execution_parity import snapshot

ENV = {key: value for key, value in os.environ.items() if not key.startswith("COVEN_")}
ENV.update(PATH="", PYTHONPATH="", COVEN_MULTIPLEXER="", PYTHONDONTWRITEBYTECODE="1")
MISSING_LOCK = "Coherent source assessment requires the existing workspace lock; none was created"
LIMITS = {"execution_bytes": 8 * 1024**2, "combined_history_entries": 1024,
          "replay_work_bytes": 256 * 1024**2, "assessment_work_bytes": 256 * 1024**2,
          "ascii_output_bytes": 32 * 1024**2}
DIAGNOSTICS = {
    "authority": {"pending_execution_authority", "invalid_execution_fields"},
    "source": {"invalid_compat_json", "invalid_workspace_toml", "unsupported_multiplexer",
               "invalid_workspace_record_list", "invalid_lead_record", "invalid_utf8",
                "invalid_execution_source_revision", "unsupported_nonfinite_json",
                "unsupported_or_invalid_json_scalar", "pending_task_import", "invalid_action_fields"},
    "coordination": {"unsafe_archive_path", "workspace_lock_busy"},
    "marker": {"invalid_handoff_workspace"},
}


def exact(actual, expected):
    # Unlike Python equality, preserve bool/int, float/int, signed zero and arrays.
    assert v1.canonical(actual) == v1.canonical(expected), (actual, expected)


def forbidden(*args, **kwargs):
    # BaseException is deliberate: source admission catches Exception/SystemExit.
    raise ReadEffect((args, kwargs))


class ReadEffect(BaseException):
    pass


@contextmanager
def read_only():
    """Permit only existing lock O_RDWR/flock; legacy O_CREAT never reaches OS."""
    real_open, real_io, real_builtin = os.open, io.open, builtins.open

    def os_open(path, flags, *args, **kwargs):
        if flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC | os.O_APPEND):
            selected = Path(path)
            if not (selected.name == ".workspace.lock" and selected.is_file() and not selected.is_symlink()
                    and flags & os.O_RDWR and not flags & (os.O_WRONLY | os.O_TRUNC | os.O_APPEND | os.O_EXCL)):
                forbidden(path, flags)
            flags &= ~os.O_CREAT
        return real_open(path, flags, *args, **kwargs)

    def opened(original, path, mode="r", *args, **kwargs):
        if isinstance(mode, str) and any(flag in mode for flag in "wax+"):
            forbidden(path, mode)
        return original(path, mode, *args, **kwargs)

    def mkdir(path, *args, **kwargs):
        # Python's existing-lock helper calls mkdir(exist_ok=True). No syscall
        # is needed for this already-existing directory, and no creation allowed.
        if Path(path).is_dir():
            raise FileExistsError(str(path))
        forbidden(path)

    with ExitStack() as stack:
        stack.enter_context(patch.object(os, "open", os_open))
        stack.enter_context(patch.object(io, "open", lambda *a, **k: opened(real_io, *a, **k)))
        stack.enter_context(patch.object(builtins, "open", lambda *a, **k: opened(real_builtin, *a, **k)))
        stack.enter_context(patch.object(os, "mkdir", mkdir))
        for name in ("fsync", "fdatasync", "rename", "replace", "unlink", "remove", "rmdir", "chmod", "truncate", "write",
                     "system", "fork", "execve", "posix_spawn"):
            stack.enter_context(patch.object(os, name, forbidden))
        for target, name in ((subprocess, "Popen"), (socket, "socket"),
                             (v1.Store, "mutate"), (v1.Store, "confirm_durability"),
                             (task_ledger.LedgerStore, "projection_health"),
                             (coven, "mutate_execution"), (coven, "write_json"),
                             (coven, "append_event")):
            stack.enter_context(patch.object(target, name, forbidden))
        yield


def python_cli(args, cwd):
    out, err = io.StringIO(), io.StringIO()
    with chdir(cwd), redirect_stdout(out), redirect_stderr(err), read_only(), patch.dict(os.environ, ENV, clear=True):
        try:
            code = coven.main(list(map(str, args)))
        except SystemExit as exc:
            code = exc.code
    return code, out.getvalue().encode("ascii"), err.getvalue().encode("utf-8")


def diagnostic_paths(value):
    """Only exception prose, never static binding reasons or operator strings."""
    paths = []
    for key in ("execution", "source_assessment"):
        text = value.get(key, {}).get("error")
        if text is not None and text != MISSING_LOCK:
            paths.append((key, "error"))
    records = [(key, value[key]) for key in ("attempt", "graph") if isinstance(value.get(key), dict)]
    records += [((key, index), item) for key in ("enrollments", "attempts", "graphs")
                for index, item in enumerate(value.get(key, []))]
    for key, item in records:
        binding = item.get("source_binding", {})
        if binding.get("status") == "source_unavailable" and binding.get("reason") not in (None, MISSING_LOCK):
            paths.append((*((key,) if isinstance(key, str) else key), "source_binding", "reason"))
    return paths


def at(value, path):
    for key in path:
        value = value[key]
    return value


def diagnostics_consistent(value):
    assessment = value.get("source_assessment")
    if assessment is not None:
        assert assessment["coordinated"] is (assessment["error"] is None)
        if assessment["error"] is not None:
            assert assessment["revision"] is None
            for path in diagnostic_paths(value):
                if path[-1] == "reason":
                    assert at(value, path) == assessment["error"], (path, value)
            for group in ("enrollments", "attempts", "graphs"):
                for item in value.get(group, []):
                    binding = item["source_binding"]
                    assert binding == {"status": "source_unavailable", "reason": assessment["error"]}
            if "attempt" in value:
                assert value["attempt"]["source_binding"] == {"status": "source_unavailable", "reason": assessment["error"]}


def normalize_pair(actual, expected, diagnostic):
    a, b = deepcopy(actual), deepcopy(expected)
    if diagnostic is None:
        return a, b
    assert diagnostic in DIAGNOSTICS
    paths = diagnostic_paths(b)
    assert paths and set(diagnostic_paths(a)) == set(paths), (a, b)
    for path in paths:
        text = at(a, path)
        assert isinstance(text, str) and 0 < len(text.encode("utf-8")) <= 4096, (path, text)
        prefix = {"authority": "", "source": "Current source could not be assessed: ",
                  "coordination": "Coordinated source assessment unavailable: ",
                  "marker": "Coordinated source assessment unavailable: "}[diagnostic]
        assert text.startswith(prefix) and text[len(prefix):] in DIAGNOSTICS[diagnostic], (path, text)
        if diagnostic in {"source", "marker"}:
            assert at(b, path).startswith("Current source could not be assessed:"), (path, b)
        elif diagnostic == "coordination":
            assert at(b, path).startswith("Coordinated source assessment unavailable:"), (path, b)
        parent_a, parent_b = at(a, path[:-1]), at(b, path[:-1])
        parent_a[path[-1]] = parent_b[path[-1]] = "<bounded-exception-diagnostic>"
    return a, b


def presentation(raw, compact):
    assert raw.endswith(b"\n") and not raw.endswith(b"\n\n"), raw[:300]
    raw.decode("ascii")
    value = v1._json(raw)
    if compact:
        assert b"\n" not in raw[:-1]
    else:
        assert raw == (json.dumps(value, ensure_ascii=True, indent=2) + "\n").encode("ascii")
    diagnostics_consistent(value)
    return value


class Suite:
    def __init__(self, root, binary, oracle_only):
        self.root, self.binary, self.oracle_only = root, binary, oracle_only
        self.counts, self.failures, self.names = Counter(), [], set()
        self.serial = 0

    def fixture(self, name, original=None):
        path = self.root / name
        if original:
            shutil.copytree(original, path, symlinks=True)
        else:
            path.mkdir()
            (path / "state").mkdir()
            (path / "coven.json").write_bytes(v1.canonical({"name": "Inspection", "agents": ["worker", "other"], "multiplexer": "tmux"}))
            self.tasks(path, tasks())
        return path

    def tasks(self, workspace, values):
        # Noncanonical raw bytes are authority too, not canonical task digests.
        (workspace / "state/tasks.json").write_text(json.dumps(values, ensure_ascii=True, indent=3) + "\n\n", encoding="ascii")

    def source(self, workspace, source_id):
        ledger = task_ledger.LedgerStore(workspace).read()
        if ledger is not None:
            identity = {"kind": "task-ledger-v1", "workspace_id": ledger.workspace_id,
                        "epoch_id": ledger.epoch_id, "path": "state/task-ledger"}
            revision, current = ledger.revision, ledger.tasks
        else:
            identity = {"kind": "snapshot-v1", "source_id": source_id, "path": "state/tasks.json"}
            raw = (workspace / "state/tasks.json").read_bytes()
            revision, current = storage.snapshot_revision(raw), v1._json(raw)
        return {"identity": identity, "revision": revision, "tasks": current, "roster": ["worker", "other"]}

    def mutate(self, workspace, operation, parameters, source_loader=None):
        self.serial += 1
        store = v1.Store(workspace)
        view = store.read()
        loader = source_loader or (lambda sid: self.source(workspace, sid))
        needs = operation in ("enroll", "prepare", "graph_propose", "prepare_graph") or (operation == "graph_decide" and parameters["decision"] == "approve")
        with storage.workspace_lock(workspace, workspace / "state/.workspace.lock") as lock:
            value = store.mutate(operation, parameters, expected_execution_revision=view.revision if view else v1.DISABLED,
                expected_tasks_revision=loader(view.snapshot_source_id)["revision"] if needs else None,
                request_id=f"setup-{self.serial}", source_loader=loader, lock=lock)
        assert value["delivery"]["outcome"] == "committed"
        return value["result"]

    def upgrade(self, workspace):
        store = v1.Store(workspace)
        with storage.workspace_lock(workspace, workspace / "state/.workspace.lock") as lock:
            v2.upgrade(store, store.read().revision, "setup-upgrade", lock)

    def decide(self, workspace, decision="approve", reason="operator Current source could not be assessed: exact"):
        view = v1.Store(workspace).read()
        return self.mutate(workspace, "graph_decide", {"graph_revision": view.current_graph_revision,
            "decision": decision, "reason": reason, "expected_graph_digest": view.graphs[view.current_graph_revision]["digest"]})

    def check(self, name, args, cwd, category="routes", code=0, diagnostic=None,
              verify=None, profile=None, contains=None):
        assert name not in self.names, name
        self.names.add(name)
        before = snapshot(self.root)
        try:
            expected_code, expected_out, expected_err = python_cli(args, cwd)
            assert expected_code == code, (name, args, expected_code, expected_out[:300], expected_err)
            expected = None
            if code == 0 and expected_out.startswith(b"{"):
                expected = presentation(expected_out, "--json" in args or "--j" in args or "--js" in args)
                if diagnostic:
                    assert diagnostic_paths(expected), (name, "fixture did not reach diagnostic", expected)
                if verify:
                    verify(expected)
            elif code != 0:
                assert expected_out == b"", (name, expected_out[:300])
                if contains:
                    assert contains.encode() in expected_err, (name, expected_err)
            if not self.oracle_only:
                started = time.monotonic()
                result = subprocess.run([self.binary, *map(str, args)], cwd=cwd, env=ENV, capture_output=True, timeout=12)
                assert time.monotonic() - started < (7 if category == "busy_lock" else 5), name
                wanted = 1 if profile else code
                assert result.returncode == wanted, (name, result.returncode, result.stdout[:300], result.stderr)
                if profile or code != 0:
                    assert result.stdout == b"", (name, result.stdout[:300])
                    if profile:
                        assert result.stderr == f"[ERROR] {profile}\n".encode("ascii"), (name, result.stderr)
                elif expected is not None:
                    assert result.stderr == b"", (name, result.stderr)
                    actual = presentation(result.stdout, "--json" in args or "--j" in args or "--js" in args)
                    exact(*normalize_pair(actual, expected, diagnostic))
                else:
                    assert result.stdout and not result.stderr, name
            self.counts["native_profile_refusals" if profile else category] += 1
        except Exception as exc:
            self.failures.append({"case": name, "category": category, "error": str(exc)[:4000]})
        finally:
            assert snapshot(self.root) == before, (name, "read changed namespace/bytes/mode/inode/mtime")

    def routes(self, name, workspace, attempt, revision=None, diagnostic=None, verify=None, legacy=False):
        routes = [("execution", ["execution", "inspect", workspace]),
                  ("list", ["task", "attempt", "list", workspace]),
                  ("attempt", ["task", "attempt", "inspect", workspace, attempt]),
                  ("graph", ["task", "graph", "inspect", workspace])]
        if revision:
            routes.append(("export", ["task", "graph", "export", workspace, revision]))
        for route, args in routes:
            for compact in (False, True):
                self.check(f"{name}/{route}/{compact}", args + (["--json"] if compact else []), workspace,
                           diagnostic=None if route == "export" else diagnostic,
                           code=1 if legacy and route == "graph" else 0,
                           verify=verify if route == "execution" else None)


def tasks():
    return [{"id": key, "title": "Exact | `operator` " + key, "status": "operator-state", "owner": "worker",
             "extension": {"text": "\u03bb\U0001f98a", "big": 2**90 + 1, "float": 1.0, "zero": -0.0,
                           "reason": "Current source could not be assessed: operator-owned"}}
            for key in ("zeta", "alpha", "unrelated")]


def intent(key):
    return {"schema_version": 1, "kind": "task-intent", "task_id": key, "owner": "worker",
            "acceptance_criteria": [{"id": "check", "text": "  exact \u03bb criterion  "}]}


def document(keys=("zeta", "alpha")):
    return {"schema_version": 1, "kind": "task-graph", "nodes": [
        {**{field: intent(key)[field] for field in ("task_id", "owner", "acceptance_criteria")},
         "depends_on": ["alpha"] if key == "zeta" and "alpha" in keys else []} for key in keys]}


def binding(status):
    def verify(value):
        assert value["source_assessment"]["coordinated"]
        assert {item["source_binding"]["status"] for item in value["attempts"]} == {status}, value
    return verify


def run(s):
    disabled = s.fixture("disabled")
    pending = s.fixture("pending", disabled)
    (pending / v1.DIR).mkdir()
    unavailable = s.fixture("unavailable", pending)
    (unavailable / v1.CONTROL).write_bytes(b"null")
    for name, workspace, diagnostic in (("disabled", disabled, None), ("pending", pending, "authority"),
                                         ("unavailable", unavailable, "authority")):
        # Broken current source and an unsafe lock must not be inspected at all.
        (workspace / "coven.json").write_bytes(b"not JSON")
        (workspace / "state/.workspace.lock").symlink_to(s.root / "absent")
        def minimal(value, status=name):
            assert set(value) == {"execution", "authorization", "dispatchable", "blockers"}
            assert value["execution"]["status"] == status
        s.routes(name, workspace, "f" * 32, diagnostic=diagnostic, verify=minimal)
        s.check(name + "/bad-revision-inactive", ["task", "graph", "inspect", workspace, "bad"], workspace, diagnostic=diagnostic)
        s.check(name + "/invalid-filter", ["task", "attempt", "list", workspace, "--task", "bad/id"], workspace, code=1, category="precedence", contains="Invalid task ID")

    legacy = s.fixture("legacy")
    s.mutate(legacy, "enable", {})
    for key in ("zeta", "alpha", "unrelated"):
        s.mutate(legacy, "enroll", {"task_id": key, "rebind_source": False})
    old = s.mutate(legacy, "prepare", {"task_id": "zeta", "intent": intent("zeta")})
    s.routes("v1-prepared", legacy, old["attempt_id"], legacy=True)
    # The graph route requires upgrade, unlike the other three inspection routes.
    s.check("v1-upgrade-before-uuid", ["task", "graph", "inspect", legacy, "bad"], legacy, code=1, category="precedence", contains="explicit execution upgrade")
    s.mutate(legacy, "cancel", {"attempt_id": old["attempt_id"], "reason": "operator cancellation exact"})
    active = s.fixture("active", legacy)
    s.upgrade(active)
    s.routes("v2-no-head", active, old["attempt_id"])
    standalone = s.mutate(active, "prepare", {"task_id": "unrelated", "intent": intent("unrelated")})
    s.mutate(active, "cancel", {"attempt_id": standalone["attempt_id"], "reason": "standalone cancelled"})
    # Freeze reverse-lexical revision IDs to distinguish proposal order from a
    # BTree presentation. Only setup entropy is fixed; reports remain untouched.
    for rid in ("e" * 32, "1" * 32):
        with patch.object(v2.uuid, "uuid4", return_value=type("UUID", (), {"hex": rid})()):
            proposal = s.mutate(active, "graph_propose", {"document": document()})
        if rid.startswith("e"):
            first_revision = proposal["graph_revision"]
            s.decide(active)
            prepared = s.mutate(active, "prepare_graph", {"task_id": "zeta", "graph_revision": first_revision})
            bound = s.fixture("bound", active)
            def bound_contract(value):
                binding("current")(value)
                assert value["attempts"][-1]["graph_binding_status"] == "current"
                graph = value["graphs"][0]
                assert graph["preparation_eligible"] is True
                assert [(n["task_id"], n["preparation_eligible"]) for n in graph["nodes"]] == [("zeta", False), ("alpha", True)]
            s.routes("bound", bound, prepared["attempt_id"], first_revision, verify=bound_contract)
            s.decide(active, "changes-requested")
            revoked = s.fixture("revoked", active)
            def review_changed(value):
                assert value["attempts"][-1]["graph_binding_status"] == "planning_review_changed"
                assert prepared["attempt_id"] in value["graphs"][0]["affected_preparations"]
            s.routes("revoked", revoked, prepared["attempt_id"], first_revision, verify=review_changed)
            s.mutate(active, "cancel", {"attempt_id": prepared["attempt_id"], "reason": "cancel independent of review"})
            s.decide(active)
            rereview = s.fixture("rereview", active)
            s.routes("rereview", rereview, prepared["attempt_id"], first_revision, verify=review_changed)
    revision = v1.Store(active).read().current_graph_revision
    s.decide(active)

    def graph_order(value):
        assert [g["graph_revision"] for g in value["graphs"]] == [first_revision, revision]
        assert prepared["attempt_id"] in value["graphs"][0]["affected_preparations"]
        assert value["graphs"][0]["current"] is False
        assert {a["graph_binding_status"] for a in value["attempts"]} == {"not_graph_bound", "superseded_revision"}
        head = value["graphs"][-1]
        assert head["preparation_eligible"] is True
        assert head["dependency_order"] == ["alpha", "zeta"]
        assert [n["dependency_acceptance"] for n in head["nodes"]] == ["unestablished", "not_required"]
    s.routes("proposal-order", active, prepared["attempt_id"], revision, verify=graph_order)
    s.check("historical-graph", ["task", "graph", "inspect", active, first_revision, "--json"], active)

    for name, change, status in (
        ("revision-only", lambda t: t[-1].update(title="unrelated drift"), "source_revision_changed"),
        ("task-changed", lambda t: t[0].update(title="selected changed"), "task_changed"),
        ("task-missing", lambda t: t.pop(0), "task_missing")):
        workspace = s.fixture(name, bound)
        current = tasks()
        change(current)
        s.tasks(workspace, current)
        def selected(value, wanted=status):
            assert value["attempts"][-1]["source_binding"]["status"] == wanted
            graph = value["graphs"][0]
            assert graph["source_binding"]["status"] == ("current" if wanted == "source_revision_changed" else "selected_inputs_changed")
            assert graph["source_binding"]["whole_revision_changed"] is True
        s.routes(name, workspace, prepared["attempt_id"], first_revision, verify=selected)

    owner = s.fixture("owner-missing", bound)
    (owner / "coven.json").write_bytes(v1.canonical({"name": "Inspection", "agents": ["other"], "multiplexer": "tmux"}))
    def owner_missing(value):
        assert value["attempts"][-1]["source_binding"]["status"] == "owner_missing"
        assert value["graphs"][0]["source_binding"]["status"] == "owner_missing"
    s.routes("owner-missing", owner, prepared["attempt_id"], first_revision, verify=owner_missing)
    priority = s.fixture("selected-before-owner", owner)
    current = tasks()
    current[0]["title"] = "changed selected input with owner also missing"
    s.tasks(priority, current)
    def selected_before_owner(value):
        assert value["attempts"][-1]["source_binding"]["status"] == "task_changed"
        match = value["graphs"][0]["source_binding"]
        assert match["status"] == "selected_inputs_changed"
        assert match["reasons"] == [{"task_id": "alpha", "reason": "owner_missing"},
                                    {"task_id": "zeta", "reason": "selected_inputs_changed"},
                                    {"task_id": "zeta", "reason": "owner_missing"}]
    s.routes("selected-before-owner", priority, prepared["attempt_id"], first_revision, verify=selected_before_owner)

    for kind in ("missing", "empty", "raw-whitespace"):
        workspace = s.fixture("raw-" + kind, bound)
        path = workspace / "state/tasks.json"
        if kind == "missing":
            path.unlink()
        elif kind == "empty":
            path.write_bytes(b" [] \r\n")
        else:
            path.write_bytes(v1.canonical(tasks()))
        def raw_revision(value, kind=kind, path=path):
            raw = None if kind == "missing" else path.read_bytes()
            assert value["source_assessment"]["revision"] == storage.snapshot_revision(raw)
            assert value["attempts"][-1]["source_binding"]["status"] == ("source_revision_changed" if kind == "raw-whitespace" else "task_missing")
        s.routes("raw-" + kind, workspace, prepared["attempt_id"], first_revision, verify=raw_revision)

    missing = s.fixture("missing-lock", bound)
    (missing / "state/.workspace.lock").unlink()
    def missing_lock(value):
        assert value["source_assessment"] == {"coordinated": False, "error": MISSING_LOCK, "revision": None}
    s.routes("missing-lock", missing, prepared["attempt_id"], first_revision, verify=missing_lock)

    # Full workspace admission precedes raw capture, including unrelated controls.
    bad_sources = [("config", "coven.json", b"{"), ("toml", "coven.toml", b"[runtime"),
                   ("multiplexer", "coven.toml", b'[runtime]\nmultiplexer="not-supported"\n'),
                   ("tasks", "state/tasks.json", b"{}"), ("agents", "state/agents.json", b"{}"),
                   ("lead", "state/lead.json", b"[]"), ("checkpoint", "checkpoints/plan-approval.md", b"\xff"),
                   ("runtime", "state/runtime-launch-control.json", b"{"),
                   ("runtime-null", "state/runtime-launch-control.json", b"null"),
                   ("runtime-invalid-control", "state/runtime-launch-control.json", b'{"version":1}'),
                   ("nonfinite-extension", "state/tasks.json", b'[{"id":"zeta","title":"t","status":"s","extension":NaN}]'),
                   ("surrogate-extension", "state/tasks.json", b'[{"id":"zeta","title":"t","status":"s","extension":"\\ud800"}]')]
    for name, relative, raw in bad_sources:
        workspace = s.fixture("bad-" + name, bound)
        path = workspace / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)
        s.routes("bad-" + name, workspace, prepared["attempt_id"], first_revision, diagnostic="source")
        # Export must replay all authority, not only the chosen proposal prefix.
        (workspace / v1.CONTROL).write_bytes(b"null")
        s.check("bad-" + name + "/corrupt-export", ["task", "graph", "export", workspace, first_revision], workspace, code=1, category="export_independence")

    pending_import = s.fixture("pending-import", bound)
    (pending_import / "state/task-ledger").mkdir()
    s.routes("pending-import", pending_import, prepared["attempt_id"], first_revision, diagnostic="source")

    # A real import changes identity; a later explicit rebind changes only latest
    # enrollment observation, never frozen preparations or graph enrollment IDs.
    imported = s.fixture("imported", active)
    with storage.workspace_lock(imported, imported / "state/.workspace.lock") as lock:
        task_ledger.LedgerStore(imported).import_tasks(storage.snapshot_revision((imported / "state/tasks.json").read_bytes()), lock)
    def changed_authority(value):
        assert {a["source_binding"]["status"] for a in value["attempts"]} == {"source_authority_changed"}
        assert value["graphs"][-1]["source_binding"]["status"] == "source_authority_changed"
    s.routes("imported", imported, prepared["attempt_id"], revision, verify=changed_authority)
    for key in ("zeta", "alpha", "unrelated"):
        s.mutate(imported, "enroll", {"task_id": key, "rebind_source": True})
    def rebound(value):
        assert {e["source_binding"]["status"] for e in value["enrollments"]} == {"current"}
        assert {a["source_binding"]["status"] for a in value["attempts"]} == {"source_authority_changed"}
    s.routes("rebound", imported, prepared["attempt_id"], revision, verify=rebound)
    enrollment_drift = s.fixture("enrollment-drift", imported)
    ledger_proposal = s.mutate(enrollment_drift, "graph_propose", {"document": document()})
    s.decide(enrollment_drift)
    def historical_snapshot(sid):
        source = s.source(enrollment_drift, sid)
        source["identity"] = {"kind": "snapshot-v1", "source_id": sid, "path": "state/tasks.json"}
        source["revision"] = storage.snapshot_revision(v1.canonical(source["tasks"]))
        return source
    s.mutate(enrollment_drift, "enroll", {"task_id": "zeta", "rebind_source": True}, source_loader=historical_snapshot)
    s.mutate(enrollment_drift, "enroll", {"task_id": "zeta", "rebind_source": True})
    def enrollment_changed(value):
        graph = value["graphs"][-1]
        assert graph["source_binding"]["status"] == "enrollment_changed"
        assert graph["source_binding"]["reasons"] == [{"task_id": "zeta", "reason": "enrollment_changed"}]
        assert graph["preparation_eligible"] is False
    s.routes("enrollment-drift", enrollment_drift, prepared["attempt_id"], ledger_proposal["graph_revision"], verify=enrollment_changed)
    for kind in ("missing", "corrupt", "symlink", "fifo"):
        workspace = s.fixture("projection-" + kind, imported)
        path = workspace / "state/tasks.json"
        path.unlink()
        if kind == "corrupt":
            path.write_bytes(b"not JSON")
        elif kind == "symlink":
            path.symlink_to(s.root / "absent")
        elif kind == "fifo":
            os.mkfifo(path)
        s.routes("projection-" + kind, workspace, prepared["attempt_id"], revision, verify=rebound)

    for kind in ("directory", "symlink", "fifo"):
        workspace = s.fixture("unsafe-lock-" + kind, bound)
        path = workspace / "state/.workspace.lock"
        path.unlink()
        if kind == "directory":
            path.mkdir()
        elif kind == "symlink":
            path.symlink_to(bound / "state/.workspace.lock")
        else:
            os.mkfifo(path)
        s.routes("unsafe-lock-" + kind, workspace, prepared["attempt_id"], first_revision, diagnostic="coordination")

    for kind in ("missing", "corrupt", "symlink", "fifo"):
        workspace = s.fixture("export-backup-" + kind, bound)
        control = v1._json((workspace / v1.CONTROL).read_bytes())
        path = workspace / control["upgrade"]["backup_path"]
        path.unlink()
        if kind == "corrupt":
            path.write_bytes(b"not historical authority")
        elif kind == "symlink":
            path.symlink_to(s.root / "absent")
        elif kind == "fifo":
            os.mkfifo(path)
        s.check("export-backup/" + kind, ["task", "graph", "export", workspace, first_revision, "--json"], workspace,
                category="export_independence")

    marker_free = s.fixture("marker-free", bound)
    (marker_free / "coven.json").unlink()
    # Native existing WorkspaceGuard intentionally requires a regular marker.
    # Python can lock but fails source admission. Compare frozen facts with an
    # explicit coordination/source diagnostic exception, not a widened guard.
    s.check("marker-free-export", ["task", "graph", "export", marker_free, first_revision], marker_free, category="export_independence")
    s.check("marker-free-inspect", ["execution", "inspect", marker_free, "--json"], marker_free, diagnostic="marker")
    s.check("graph-unmarked-path-is-selector", ["task", "graph", "inspect", marker_free], bound,
            category="precedence", code=1, contains="Invalid graph revision")
    busy = s.fixture("busy", bound)
    with (busy / "state/.workspace.lock").open("r+b") as held:
        fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
        s.check("busy-inspect", ["execution", "inspect", busy, "--json"], busy, diagnostic="coordination", category="busy_lock")
        s.check("busy-export", ["task", "graph", "export", busy, first_revision], busy, category="export_independence")

    parser_cases(s, active, prepared["attempt_id"], revision)
    oracle_seams(s, bound)
    action_profile_cases(s, bound, prepared["attempt_id"], first_revision)
    resource_cases(s, legacy)


def parser_cases(s, active, attempt, revision):
    # Required selectors use arity, even when their sole argument is marked.
    for command, selector in ((["task", "attempt", "inspect"], attempt), (["task", "graph", "export"], revision)):
        stem = "/".join(command)
        s.check(stem + "/one-selector", command + [selector], active, category="parser")
        s.check(stem + "/marked-is-selector", command + [active], active, category="parser", code=1, contains="Invalid")
        s.check(stem + "/invalid-before-root", command + [s.root / "absent", "bad"], active, category="precedence", code=1, contains="Invalid")
        s.check(stem + "/empty-selector", command + [""], active, category="parser", code=1, contains="Invalid")
        s.check(stem + "/between-fixed", command + [active, "--json", selector], active, category="parser", code=2)
        s.check(stem + "/double-dash", command + ["--", active, selector], active, category="parser")
        s.check(stem + "/help-no-selector", command + ["--help"], active, category="parser")
        s.check(stem + "/prefix", command + ["--js", active, selector], active, category="parser")
        s.check(stem + "/repeat", command + [active, selector, "--json", "--json"], active, category="parser")
        s.check(stem + "/uppercase-selector", command + ["A" * 32], active, category="parser", code=1, contains="Invalid")
        s.check(stem + "/option-before-command", command[:-1] + ["--json", command[-1], selector], active, category="parser", code=2)
    for command in (["execution", "inspect"], ["task", "attempt", "list"], ["task", "graph", "inspect"]):
        stem = "/".join(command)
        for suffix, args, code in (("default", [], 0), ("prefix", ["--j"], 0),
                ("repeat", ["--json", "--json"], 0), ("equals-bool", ["--json=true"], 2),
                ("unknown", ["--unknown"], 2), ("double-dash", ["--", ""], 0),
                ("empty", [""], 0), ("help", ["--help"], 0),
                ("ascii-negative", ["-12"], 1), ("unicode-negative", ["-\u0661\u0662"], 1),
                ("nondecimal-negative", ["-\u00b2"], 2)):
            s.check(stem + "/" + suffix, command + args, active, category="parser", code=code)
    for suffix, args, code in (("before-variadic", ["--json", active, revision], 0),
            ("between-variadic", [active, "--json", revision], 2),
            ("after-variadic", [active, revision, "--json"], 0),
            ("empty-head", [active, ""], 0), ("excess", [active, "bad", "extra"], 1)):
        s.check("graph-arity/" + suffix, ["task", "graph", "inspect", *args], active, category="parser", code=code,
                contains="accepts one optional graph revision" if suffix == "excess" else None)
    for task in ("zeta", "absent", "", "bad/id"):
        s.check("filter/" + repr(task), ["task", "attempt", "list", active, "--task", task, "--json"], active,
                category="filter", code=1 if task in ("", "bad/id") else 0)
    s.check("filter/repeat-last-wins", ["task", "attempt", "list", active, "--task", "bad/id", "--ta", "zeta", "--json"], active, category="filter")
    s.check("filter/inspection-before-filter", ["task", "attempt", "list", active, "--task", "bad/id"], active, category="precedence", code=1, contains="Invalid task ID")
    s.check("attempt/unknown", ["task", "attempt", "inspect", active, "f" * 32], active, category="selectors", code=1, contains="Unknown attempt ID")
    s.check("graph/unknown", ["task", "graph", "inspect", active, "f" * 32], active, category="selectors", code=1, contains="Unknown graph revision")
    s.check("graph/invalid", ["task", "graph", "inspect", active, "bad"], active, category="selectors", code=1, contains="Invalid graph revision")
    # Never invoke Python runtime writers. Keep authoring no-effects guards even
    # as routes are ported: invalid flags must reject before reading or writing.
    if not s.oracle_only:
        view = v1.Store(active).read()
        flags = ["--expect-execution-revision", view.revision, "--request-id", "forbidden-native-write", "--json"]
        task_flags = ["--expect-tasks-revision", s.source(active, view.snapshot_source_id)["revision"]]
        for args in (["execution", "enable", active, *flags],
                     ["execution", "enroll", active, "zeta", *flags, *task_flags],
                     ["execution", "upgrade", active, *flags],
                     ["execution", "recover", active, "--confirm-authority", "--expect-execution-revision", view.revision, "--invalid-maintenance-flag", "--json"],
                     ["task", "attempt", "prepare", active, "zeta", "--graph-revision", revision, *flags, *task_flags],
                     ["task", "attempt", "cancel", active, attempt, "--reason", "never dispatch", *flags, "--invalid-maintenance-flag"],
                     ["task", "graph", "propose", active, "--document", "never-open.json", *flags, *task_flags],
                     ["task", "graph", "decide", active, revision, "--decision", "changes-requested", "--reason", "never write",
                      "--expect-graph-digest", view.graphs[revision]["digest"], *flags]):
            before = snapshot(s.root)
            result = subprocess.run([s.binary, *args, "--invalid-authoring-guard"], cwd=active, env=ENV, capture_output=True, timeout=5)
            assert result.returncode != 0 and result.stdout == b"", (args, result)
            assert snapshot(s.root) == before
            s.counts["authoring_no_effects_guards"] += 1


def oracle_seams(s, workspace):
    """Characterize read/lock transitions without racy authority file writes."""
    view = v1.Store(workspace).read()
    before = snapshot(s.root)
    def check(name, callback):
        try:
            with read_only():
                callback()
            s.counts["oracle_state_machine"] += 1
        except Exception as exc:
            s.failures.append({"case": name, "category": "oracle_state_machine", "error": str(exc)[:4000]})
        assert snapshot(s.root) == before

    def second_hard():
        with patch.object(v1.Store, "read", side_effect=[view, v1.AttemptError("second-read-hard")]) as reads:
            try:
                coven.execution_inspection(workspace)
            except v1.AttemptError as error:
                assert str(error) == "second-read-hard"
            else:
                raise AssertionError("second read was downgraded")
            assert reads.call_count == 2
    check("second-read-outside-catch", second_hard)

    def disabled_under_lock():
        with patch.object(v1.Store, "read", side_effect=[view, view, None]) as reads, patch.object(coven, "execution_source", forbidden):
            value = coven.execution_inspection(workspace)
            assert value["execution"] == {"status": "disabled", "revision": v1.DISABLED}
            assert "attempts" not in value and reads.call_count == 3
    check("disabled-under-lock", disabled_under_lock)

    def coordination_reclassify():
        with patch.object(v1.Store, "read", side_effect=[view, view, v1.AttemptError("locked-read"), None]) as reads:
            value = coven.execution_inspection(workspace)
            assert value["execution"]["status"] == "disabled" and reads.call_count == 4
    check("coordination-reclassify", coordination_reclassify)

    def fresh_raw():
        old_load = coven.load_team
        def load(root):
            team = old_load(root)
            return team
        # Supply stale first-load task data without changing raw bytes. The
        # independent raw capture must still report the byte-based revision.
        original = coven.load_workspace_state
        def stale(root, *args, **kwargs):
            state = original(root, *args, **kwargs)
            return coven.WorkspaceState(state.team, [], state.agents, state.lead)
        with patch.object(coven, "load_workspace_state", stale), patch.object(coven, "load_team", load):
            value = coven.execution_inspection(workspace)
            assert value["source_assessment"]["revision"] == storage.snapshot_revision((workspace / "state/tasks.json").read_bytes())
            assert value["attempts"][-1]["source_binding"]["status"] == "current"
    check("fresh-raw-not-loaded-projection", fresh_raw)

    def filter_after_capture():
        with patch.object(coven, "execution_source", wraps=coven.execution_source) as captured:
            code, out, err = python_cli(["task", "attempt", "list", workspace, "--task", "bad/id"], workspace)
            assert code == 1 and out == b"" and b"Invalid task ID" in err
            assert captured.call_count == 1
    check("invalid-filter-still-captures-source", filter_after_capture)


def action_profile_cases(s, baseline, attempt, revision):
    # Runtime-control input reaches execution through full source preflight.
    # Profile failures are fatal native refusals, never normalized diagnostics.
    routes = [("execution", ["execution", "inspect"], []),
              ("list", ["task", "attempt", "list"], []),
              ("attempt", ["task", "attempt", "inspect"], [attempt]),
              ("graph", ["task", "graph", "inspect"], [revision]),
              ("export", ["task", "graph", "export"], [revision])]
    before = snapshot(s.root)
    references = {}
    for route, command, selectors in routes:
        code, out, err = python_cli([*command, baseline, *selectors, "--json"], baseline)
        assert code == 0 and err == b"", (route, code, err)
        references[route] = presentation(out, True)
    assert snapshot(s.root) == before

    for kind, error in (("input", "unsupported_action_input_profile"),
                        ("utf8-bom", "unsupported_action_encoding_profile"),
                        ("utf16", "unsupported_action_encoding_profile")):
        workspace = s.fixture("profile-runtime-" + kind, baseline)
        info = workspace.stat()
        control = {"version": 1, "action": "runtime_launch", "workspace_id": "a" * 32,
                   "epoch_id": "b" * 32,
                   "location": {"root": str(workspace), "device": info.st_dev, "inode": info.st_ino},
                   "head": None, "claims": {}}
        text = json.dumps(control, ensure_ascii=True)
        if kind == "input":
            raw = text.encode("ascii") + b" " * (8 * 1024**2)
            assert 8 * 1024**2 < len(raw) < 16 * 1024**2
        elif kind == "utf8-bom":
            raw = text.encode("utf-8-sig")
        else:
            raw = text.encode("utf-16")
        (workspace / "state/runtime-launch-control.json").write_bytes(raw)
        for route, command, selectors in routes:
            for compact in (False, True):
                s.check(f"profile/runtime-{kind}/{route}/{compact}",
                        [*command, workspace, *selectors, *(["--json"] if compact else [])], workspace,
                        category="export_independence" if route == "export" else "profile",
                        profile=None if route == "export" else error,
                        verify=lambda value, expected=references[route]: exact(value, expected))
    # unsupported_action_work_profile uses the service owner's private fault
    # seams/classifier unit gate, not a production environment injection hook.


def resource_cases(s, baseline):
    # Distinguish native resource refusal from ordinary source-unavailable JSON.
    # Python deliberately has a broader control/value profile than native.
    for name, value, error in (("raw-8MiB", ["x" * 1024**2] * 8, "handoff_size_limit"),
            ("value-depth", nest(70), "unsupported_json_depth_profile"),
            ("value-string", "x" * (1024**2 + 1), "unsupported_json_string_profile")):
        workspace = s.fixture("profile-" + name, baseline)
        control = v1._json((workspace / v1.CONTROL).read_bytes())
        control["extensions"]["profile"] = value
        from _execution_parity import resign
        (workspace / v1.CONTROL).write_bytes(v1.canonical(resign(control)))
        s.check("profile/" + name, ["execution", "inspect", workspace, "--json"], workspace,
                category="profile", profile=error)

    source_profile = s.fixture("profile-current-source", baseline)
    current = tasks()
    current[-1]["extension"]["padding"] = "s" * (1024**2 + 1)
    s.tasks(source_profile, current)
    s.check("profile/source-fatal-not-unavailable", ["execution", "inspect", source_profile, "--json"], source_profile,
            category="profile", profile="unsupported_json_string_profile")

    # Use the existing pure reducer, not CLI mutation handlers or patched replay
    # guards. Both oversized controls remain valid Python authority histories.
    control = v1._json((baseline / v1.CONTROL).read_bytes())
    view = v1.Store(baseline).read()
    enrollments, attempts = deepcopy(view.enrollments), deepcopy(view.attempts)
    prepare = next(e for e in control["history"] if e["binding"]["operation"] == "prepare")
    cancel = next(e for e in control["history"] if e["binding"]["operation"] == "cancel")

    def extend(length):
        while len(control["history"]) < length:
            previous = v1._revision(control)
            generation = len(control["history"]) + 1
            active = next((a for a in attempts.values() if a["state"] == "prepared"), None)
            entry = deepcopy(cancel if active else prepare)
            if active:
                aid = active["prepared"]["attempt_id"]
                entry["binding"]["parameters"]["attempt_id"] = aid
                entry["effect"].update(attempt_id=aid, cancel_id=uuid.uuid4().hex)
            else:
                older = [a["prepared"] for a in attempts.values() if a["prepared"]["task_id"] == "zeta"]
                entry["effect"]["prepared"].update(attempt_id=uuid.uuid4().hex,
                    attempt_number=len(older) + 1, previous_attempt_id=older[-1]["attempt_id"])
            entry.update(generation=generation, request_id=f"profile-history-{generation}")
            entry["binding"]["expected_execution_revision"] = previous
            entry["result"] = v1._apply(control, enrollments, attempts, entry["binding"], entry["effect"], generation)
            control["generation"] = generation
            control["history"].append(entry)

    extend(40)
    work_control = deepcopy(control)
    work_control["extensions"]["padding"] = ["w" * 1024**2] * 7
    from _execution_parity import resign
    resign(work_control)
    assert len(v1.canonical(work_control)) < LIMITS["execution_bytes"]
    assert sum(len(v1.canonical({**work_control, "generation": i, "history": work_control["history"][:i]}))
               for i in range(1, 41)) > LIMITS["replay_work_bytes"]
    workspace = s.fixture("profile-replay-work", baseline)
    (workspace / v1.CONTROL).write_bytes(v1.canonical(work_control))
    s.check("profile/replay-256MiB", ["execution", "inspect", workspace, "--json"], workspace,
            category="profile", profile="unsupported_execution_work_profile")

    extend(1025)
    assert len(v1.canonical(control)) < LIMITS["execution_bytes"]
    workspace = s.fixture("profile-history", baseline)
    (workspace / v1.CONTROL).write_bytes(v1.canonical(control))
    s.check("profile/history-1025", ["execution", "inspect", workspace, "--json"], workspace,
            category="profile", profile="unsupported_execution_history_profile")


def nest(depth):
    value = None
    for _ in range(depth):
        value = [value]
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--oracle-only", action="store_true")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="coven-execution-inspection-") as temporary:
        suite = Suite(Path(temporary), str(Path(args.binary).resolve()), args.oracle_only)
        run(suite)
        print(json.dumps({"categories": dict(suite.counts), "cli_cases": len(suite.names),
            "cases": len(suite.names) + suite.counts["oracle_state_machine"],
            "failures": suite.failures, "oracle_only": args.oracle_only,
            "native_executed": not args.oracle_only, "limits": LIMITS,
            "owner_unit_gates": ["execution::inspection::tests::second_read_is_hard_and_precedes_lock",
                "execution::inspection::tests::all_known_native_profiles_are_classified_explicitly_not_by_unsupported_prefix",
                "execution::inspection::projection::tests::aggregate_work_exact_boundary_is_fatal_including_graph_matching",
                "execution::inspection::tests::pretty_compact_ascii_and_final_admission_share_work"],
            "native_writes_enabled": False}, ensure_ascii=True))
        return bool(suite.failures)


if __name__ == "__main__":
    raise SystemExit(main())
