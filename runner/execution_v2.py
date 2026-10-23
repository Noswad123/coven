"""Explicit, backed-up v1 upgrade and reviewed, preparation-only graph authority."""

from copy import deepcopy
from dataclasses import dataclass, field
from datetime import datetime, timezone
import hashlib
import os
import stat
import uuid

if __package__:
    from . import storage, task_attempts as v1, task_graphs
else:
    import storage
    import task_attempts as v1
    import task_graphs


GRAPH_REVIEW_RESERVE = 512 * 1024
CANCELLATION_RESERVE = v1.CANCELLATION_RESERVE
MAX_CONTROL = v1.MAX_CONTROL
EXEC_REV = rf"execution-control-v2:{v1.UUID}:[1-9][0-9]*:{v1.HEX}"
canonical = v1.canonical
_fail = v1._fail


@dataclass
class View(v1.View):
    schema_version: int = 2
    graph_id: str | None = None
    current_graph_revision: str | None = None
    graphs: dict = field(default_factory=dict)
    managed_tasks: set = field(default_factory=set)


def _revision(control):
    return (f"execution-control-v2:{control['execution_authority_id']}:{control['generation']}:"
            + hashlib.sha256(canonical(control)).hexdigest())


def request_binding(operation, parameters, expected_execution_revision, expected_tasks_revision):
    fields = {"upgrade": "", "enroll": "task_id rebind_source", "prepare": "task_id intent",
              "cancel": "attempt_id reason", "graph_propose": "document",
              "graph_decide": "graph_revision decision reason expected_graph_digest",
              "prepare_graph": "task_id graph_revision"}
    if not isinstance(operation, str) or operation not in fields:
        _fail("unsupported execution v2 operation")
    v1._fields(parameters, fields[operation])
    v1._match(expected_execution_revision, v1.EXEC_REV if operation == "upgrade" else EXEC_REV,
              "execution revision")
    needs_source = operation in ("enroll", "prepare", "graph_propose", "prepare_graph")
    if operation == "graph_decide":
        v1._match(parameters["graph_revision"], v1.UUID, "graph revision")
        if parameters["decision"] not in ("approve", "changes-requested"):
            _fail("invalid graph decision")
        v1._text(parameters["reason"], "review reason")
        v1._match(parameters["expected_graph_digest"], rf"task-graph-manifest-v1:{v1.HEX}", "graph digest")
        needs_source = parameters["decision"] == "approve"
    if needs_source:
        v1._match(expected_tasks_revision, v1.TASK_REV, "required tasks revision")
    elif expected_tasks_revision is not None:
        _fail("operation must not depend on task source")
    if operation in ("enroll", "prepare", "cancel"):
        # Reuse parameter validation, not the v1 request namespace.
        v1.request_binding(operation, parameters,
                           "execution-control-v1:" + "0" * 32 + ":1:" + "0" * 64,
                           expected_tasks_revision)
    if operation == "graph_propose":
        task_graphs.validate_document(parameters["document"])
    if operation == "prepare_graph":
        v1._id(parameters["task_id"])
        v1._match(parameters["graph_revision"], v1.UUID, "graph revision")
    return {"schema_version": 2, "operation": operation, "parameters": deepcopy(parameters),
            "expected_execution_revision": expected_execution_revision,
            "expected_tasks_revision": expected_tasks_revision}


def _intent_digest(record):
    return hashlib.sha256(b"coven-task-attempt-intent-v2\0" + canonical(
        v1._intent_payload(record) | {"graph_binding": record["graph_binding"]})).hexdigest()


def _head(view, revision):
    if revision != view.current_graph_revision or revision is None:
        _fail("graph revision must be the current head")
    return view.graphs[revision]


def _no_prepared(view, tasks):
    if any(a["state"] == "prepared" and a["prepared"]["task_id"] in tasks
           for a in view.attempts.values()):
        _fail("prepared attempt blocks graph proposal/approval; cancel first")


def _source(control, binding, effect):
    source = v1._source(effect["source"], control["snapshot_source_id"])
    if source["revision"] != binding["expected_tasks_revision"]:
        _fail("tasks revision conflict")
    return source


def _graph_current(graph, source, view):
    match = task_graphs.match_source(graph["manifest"], source, view.enrollments)
    if match["status"] != "current":
        _fail("graph source is not current: " + match["status"])
    return match


def _apply(control, view, binding, effect, generation):
    op, params = binding["operation"], binding["parameters"]
    result = {"operation": op, "generation": generation,
              "execution_authority_id": view.authority_id}
    if op == "upgrade":
        if generation != control["legacy_v1"]["generation"] + 1:
            _fail("upgrade must be first v2 entry")
        if canonical(effect) != canonical(control["upgrade"]):
            _fail("upgrade effect mismatch")
        return result | {k: effect[k] for k in ("upgrade_id", "backup_path", "source_revision")}
    if op == "graph_propose":
        v1._fields(effect, "source manifest")
        source = _source(control, binding, effect)
        manifest = task_graphs.validate_manifest(effect["manifest"])
        expected = task_graphs.freeze(params["document"], source, view.enrollments,
            authority_id=view.authority_id, snapshot_source_id=view.snapshot_source_id,
            graph_id=view.graph_id or manifest["graph_id"], revision_id=manifest["graph_revision"],
            predecessor=view.current_graph_revision, proposed_at=manifest["proposed_at"])
        if canonical(manifest) != canonical(expected):
            _fail("graph manifest/source mismatch")
        revision = manifest["graph_revision"]
        if revision in view.graphs:
            _fail("duplicate graph revision")
        selected = {n["task_id"] for n in manifest["nodes"]}
        _no_prepared(view, view.managed_tasks | selected)
        view.graph_id = manifest["graph_id"]
        view.current_graph_revision = revision
        view.managed_tasks.update(selected)
        digest = task_graphs.digest(manifest)
        view.graphs[revision] = {"manifest": deepcopy(manifest), "digest": digest,
                                 "decisions": [], "current_review": "proposed"}
        return result | {"graph_id": view.graph_id, "graph_revision": revision,
                         "graph_digest": digest, "manifest": deepcopy(manifest)}
    if op == "graph_decide":
        approve = params["decision"] == "approve"
        v1._fields(effect, "source decision" if approve else "decision")
        graph = _head(view, params["graph_revision"])
        if params["expected_graph_digest"] != graph["digest"]:
            _fail("graph digest conflict")
        if approve:
            source_match = _graph_current(graph, _source(control, binding, effect), view)
            _no_prepared(view, view.managed_tasks)
        decision = effect["decision"]
        v1._fields(decision, "decision_id graph_revision decision reason state decided_at")
        v1._match(decision["decision_id"], v1.UUID, "decision ID")
        v1._timestamp(decision["decided_at"])
        if any(d["decision_id"] == decision["decision_id"]
               for g in view.graphs.values() for d in g["decisions"]):
            _fail("duplicate decision ID")
        state = "approved_for_preparation" if approve else "changes_requested"
        expected = {"decision_id": decision["decision_id"], "graph_revision": params["graph_revision"],
                    "decision": params["decision"], "reason": params["reason"], "state": state,
                    "decided_at": decision["decided_at"]}
        if canonical(decision) != canonical(expected):
            _fail("graph decision mismatch")
        graph["decisions"].append(deepcopy(decision))
        graph["current_review"] = state
        return result | {"graph_id": view.graph_id, "graph_digest": graph["digest"],
                         "decision": deepcopy(decision)} | ({"source_match": source_match} if approve else {})
    # v1's reducer supplies the enrollment/cancellation and preparation guards.
    legacy_binding = deepcopy(binding)
    legacy_binding["schema_version"] = 1
    legacy_binding["expected_execution_revision"] = (
        f"execution-control-v1:{view.authority_id}:{generation - 1}:" + "0" * 64)
    if op in ("enroll", "cancel"):
        return v1._apply(control, view.enrollments, view.attempts, legacy_binding, effect, generation)
    v1._fields(effect, "source prepared")
    source = _source(control, binding, effect)
    graph_binding = None
    if op == "prepare_graph":
        graph = _head(view, params["graph_revision"])
        if graph["current_review"] != "approved_for_preparation":
            _fail("graph is not approved for preparation")
        _graph_current(graph, source, view)
        node = next((n for n in graph["manifest"]["nodes"] if n["task_id"] == params["task_id"]), None)
        if node is None:
            _fail("task is not in the current graph")
        intent = {"schema_version": 1, "kind": "task-intent",
                  **{k: deepcopy(node[k]) for k in ("task_id", "owner", "acceptance_criteria")}}
        graph_binding = {"graph_id": view.graph_id, "graph_revision": view.current_graph_revision,
                         "graph_digest": graph["digest"],
                         "decision_id": graph["decisions"][-1]["decision_id"], "node_digest": node["node_digest"]}
    elif op == "prepare":
        if params["task_id"] in view.managed_tasks:
            _fail("graph-managed task requires prepare_graph, not standalone intent")
        intent = params["intent"]
    else:
        _fail("unsupported execution v2 operation")
    record = effect["prepared"]
    v1._fields(record, "schema_version attempt_id task_id enrollment_id attempt_number previous_attempt_id execution_authority_id task_authority_identity task_source_revision complete_task_snapshot task_snapshot_digest intent intent_digest prepared_at graph_binding")
    if not isinstance(record, dict) or type(record.get("schema_version")) is not int or record["schema_version"] != 2:
        _fail("prepared record requires schema version 2")
    if canonical(record["graph_binding"]) != canonical(graph_binding) or record["intent_digest"] != _intent_digest(record):
        _fail("graph binding/intent digest mismatch")
    old_record = deepcopy(record)
    old_record.pop("graph_binding")
    old_record["schema_version"] = 1
    old_record["intent_digest"] = v1.intent_digest(v1._intent_payload(old_record))
    legacy_binding.update(operation="prepare", parameters={"task_id": params["task_id"], "intent": intent})
    old_result = v1._apply(control, view.enrollments, view.attempts, legacy_binding,
                           {"source": source, "prepared": old_record}, generation)
    view.attempts[record["attempt_id"]]["prepared"] = deepcopy(record)
    return old_result | {"operation": op, "attempt": deepcopy(record)}


def replay(control):
    v1._fields(control, "schema_version execution_authority_id snapshot_source_id generation legacy_v1 upgrade history extensions")
    if type(control["schema_version"]) is not int or control["schema_version"] != 2:
        _fail("unsupported execution schema version")
    legacy = v1._replay(control["legacy_v1"])
    if any(a["state"] == "prepared" for a in legacy.attempts.values()):
        _fail("upgrade requires no prepared attempts")
    if (control["execution_authority_id"] != legacy.authority_id
            or control["snapshot_source_id"] != legacy.snapshot_source_id
            or canonical(control["extensions"]) != canonical(legacy.extensions)):
        _fail("upgrade changed legacy identities/extensions")
    metadata = control["upgrade"]
    v1._fields(metadata, "upgrade_id source_revision source_sha256 source_mode backup_path")
    v1._match(metadata["upgrade_id"], v1.UUID, "upgrade ID")
    v1._match(metadata["source_sha256"], v1.HEX, "source bytes digest")
    if (metadata["source_revision"] != legacy.revision
            or type(metadata["source_mode"]) is not int or not 0 <= metadata["source_mode"] <= 0o7777
            or metadata["backup_path"] != f"state/execution/upgrades/{metadata['upgrade_id']}/control-v1.bytes"):
        _fail("invalid upgrade source/backup metadata")
    history = control["history"]
    if (not isinstance(history, list) or not history or type(control["generation"]) is not int
            or control["generation"] != legacy.generation + len(history)):
        _fail("invalid v2 history/generation")
    view = View(legacy.authority_id, legacy.snapshot_source_id, legacy.generation, legacy.revision,
                deepcopy(legacy.enrollments), deepcopy(legacy.attempts), deepcopy(legacy.receipts),
                deepcopy(legacy.extensions))
    for index, entry in enumerate(history):
        v1._fields(entry, "generation request_id binding effect result")
        generation = legacy.generation + index + 1
        if type(entry["generation"]) is not int or entry["generation"] != generation:
            _fail("invalid history generation")
        key = v1._match(entry["request_id"], r"[A-Za-z0-9][A-Za-z0-9._:-]{0,127}", "request key")
        if key in view.receipts:
            _fail("duplicate request key")
        binding = entry["binding"]
        v1._fields(binding, "schema_version operation parameters expected_execution_revision expected_tasks_revision")
        checked = request_binding(binding["operation"], binding["parameters"],
                                  binding["expected_execution_revision"], binding["expected_tasks_revision"])
        if canonical(binding) != canonical(checked):
            _fail("invalid v2 binding")
        if (binding["operation"] == "upgrade") != (index == 0):
            _fail("upgrade must be first and unique")
        if binding["expected_execution_revision"] != view.revision:
            _fail("history execution revision digest mismatch")
        result = _apply(control, view, checked, entry["effect"], generation)
        if canonical(result) != canonical(entry["result"]):
            _fail("receipt/effect mismatch")
        view.receipts[key] = deepcopy(entry)
        view.generation = generation
        prefix = {**control, "generation": generation, "history": history[:index + 1]}
        active = sum(a["state"] == "prepared" for a in view.attempts.values())
        approved = (view.current_graph_revision is not None and
                    view.graphs[view.current_graph_revision]["current_review"] == "approved_for_preparation")
        if len(canonical(prefix)) + 1 + active * CANCELLATION_RESERVE + approved * GRAPH_REVIEW_RESERVE > MAX_CONTROL:
            _fail("execution capacity must retain cancellation and graph review reserves")
        view.revision = _revision(prefix)
    return view


def _confirm(store, view, lock):
    store._lock(lock)
    current = replay(store._control())
    if current.revision != view.revision:
        _fail("execution authority changed before durability confirmation")
    fd = storage._open_regular(store.root / v1.CONTROL, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)
    for path in (store.root / v1.DIR, store.root / "state", store.root):
        storage.sync_directory(path)


def _known(store, view, result, replayed, lock):
    try:
        _confirm(store, view, lock)
    except (OSError, storage.StorageError) as exc:
        return store._verification_failed(view, result, replayed, exc)
    return store._envelope(view, result, replayed)


def _lookup(store, view, key, operation, parameters, execution, tasks, lock):
    receipt = view.receipts.get(key)
    if receipt is None:
        return None
    binder = v1.request_binding if receipt["binding"]["schema_version"] == 1 else request_binding
    try:
        binding = binder(operation, parameters, execution, tasks)
    except storage.StorageError as exc:
        raise v1.AttemptError("request key binding conflict") from exc
    if canonical(binding) != canonical(receipt["binding"]):
        _fail("request key binding conflict")
    return _known(store, view, receipt["result"], True, lock)


def _publish(store, control, view, binding, effect, key, lock):
    generation = view.generation + 1
    result = _apply(control, deepcopy(view), binding, effect, generation)
    control["generation"] = generation
    control["history"].append({"generation": generation, "request_id": key, "binding": binding,
                               "effect": effect, "result": result})
    proposed = replay(control)
    try:
        storage.atomic_replace_bytes(store.root / v1.CONTROL, canonical(control) + b"\n")
    except storage.CommitUncertain as exc:
        return store._verification_failed(proposed, result, False, exc)
    return _known(store, proposed, result, False, lock)


def upgrade(store, expected_execution_revision, request_id, lock):
    store._lock(lock)
    key = v1._match(request_id, r"[A-Za-z0-9][A-Za-z0-9._:-]{0,127}", "request key")
    control = store._control()
    if control is None:
        _fail("execution v1 must be enabled before upgrade")
    if control.get("schema_version") == 2:
        view = replay(control)
        known = _lookup(store, view, key, "upgrade", {}, expected_execution_revision, None, lock)
        if known is not None:
            return known
        _fail("execution is already upgraded")
    view = v1._replay(control)
    if key in view.receipts:
        _fail("request key binding conflict")
    binding = request_binding("upgrade", {}, expected_execution_revision, None)
    if view.revision != expected_execution_revision:
        _fail("execution revision conflict")
    if any(a["state"] == "prepared" for a in view.attempts.values()):
        _fail("upgrade requires no prepared attempts; cancel first")
    store._paths()
    path = store.root / v1.CONTROL
    storage._check_path(path)
    fd = storage._open_regular(path, os.O_RDONLY)
    with os.fdopen(fd, "rb") as stream:
        mode = stat.S_IMODE(os.fstat(stream.fileno()).st_mode)
        raw = stream.read(MAX_CONTROL + 1)
    if len(raw) > MAX_CONTROL or canonical(v1._json(raw)) != canonical(control):
        _fail("upgrade source changed or exceeds control limit")
    upgrade_id = uuid.uuid4().hex
    backup_path = f"state/execution/upgrades/{upgrade_id}/control-v1.bytes"
    metadata = {"upgrade_id": upgrade_id, "source_revision": view.revision,
                "source_sha256": hashlib.sha256(raw).hexdigest(), "source_mode": mode,
                "backup_path": backup_path}
    candidate = {"schema_version": 2, "execution_authority_id": view.authority_id,
                 "snapshot_source_id": view.snapshot_source_id, "generation": view.generation,
                 "legacy_v1": deepcopy(control), "upgrade": metadata, "history": [],
                 "extensions": deepcopy(view.extensions)}
    # Backups are evidence, never an alternate authority or recovery instruction.
    upgrades = store.root / v1.DIR / "upgrades"
    try:
        try:
            upgrades.mkdir()
        except FileExistsError:
            if not stat.S_ISDIR(upgrades.lstat().st_mode):
                _fail("unsafe upgrades directory")
        directory = upgrades / upgrade_id
        directory.mkdir()
        backup = store.root / backup_path
        fd = storage._open_regular(backup, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
        with os.fdopen(fd, "wb") as stream:
            stream.write(raw)
            stream.flush()
            os.fchmod(stream.fileno(), mode)
            os.fsync(stream.fileno())
        fd = storage._open_regular(backup, os.O_RDONLY)
        with os.fdopen(fd, "rb") as stream:
            if stream.read(MAX_CONTROL + 1) != raw or stat.S_IMODE(os.fstat(stream.fileno()).st_mode) != mode:
                _fail("upgrade backup verification failed")
        for parent in (directory, upgrades, store.root / v1.DIR, store.root / "state", store.root):
            storage.sync_directory(parent)
    except OSError as exc:
        raise v1.AttemptError(f"upgrade not published; inert backup artifacts retained: {exc}") from exc
    return _publish(store, candidate, view, binding, metadata, key, lock)


def mutate(store, operation, parameters, *, expected_execution_revision,
           expected_tasks_revision, request_id, source_loader, lock):
    store._lock(lock)
    key = v1._match(request_id, r"[A-Za-z0-9][A-Za-z0-9._:-]{0,127}", "request key")
    control = store._control()
    view = replay(control)
    known = _lookup(store, view, key, operation, parameters, expected_execution_revision,
                    expected_tasks_revision, lock)
    if known is not None:
        return known
    binding = request_binding(operation, parameters, expected_execution_revision, expected_tasks_revision)
    if operation == "upgrade":
        _fail("execution is already upgraded")
    if view.revision != expected_execution_revision:
        _fail("execution revision conflict")
    now = datetime.now(timezone.utc).isoformat()
    effect = {}
    if expected_tasks_revision is not None:
        if not callable(source_loader):
            _fail("source loader required")
        source = deepcopy(v1._source(source_loader(view.snapshot_source_id), view.snapshot_source_id))
        if source["revision"] != expected_tasks_revision:
            _fail("tasks revision conflict")
        effect["source"] = source
    if operation == "graph_propose":
        effect["manifest"] = task_graphs.freeze(parameters["document"], source, view.enrollments,
            authority_id=view.authority_id, snapshot_source_id=view.snapshot_source_id,
            graph_id=view.graph_id or uuid.uuid4().hex, revision_id=uuid.uuid4().hex,
            predecessor=view.current_graph_revision, proposed_at=now)
    elif operation == "graph_decide":
        effect["decision"] = {"decision_id": uuid.uuid4().hex,
            "graph_revision": parameters["graph_revision"], "decision": parameters["decision"],
            "reason": parameters["reason"], "decided_at": now,
            "state": "approved_for_preparation" if parameters["decision"] == "approve" else "changes_requested"}
    elif operation == "cancel":
        effect = {"cancel_id": uuid.uuid4().hex, "attempt_id": parameters["attempt_id"],
                  "reason": parameters["reason"], "cancelled_at": now}
    elif operation in ("enroll", "prepare", "prepare_graph"):
        task_id = parameters["task_id"]
        task = next((t for t in source["tasks"] if t["id"] == task_id), None)
        if task is None:
            _fail("task not found")
        previous = view.enrollments.get(task_id)
        common = {"task_id": task_id, "task_authority_identity": source["identity"],
                  "task_source_revision": source["revision"], "complete_task_snapshot": task,
                  "task_snapshot_digest": v1.task_snapshot_digest(task)}
        if operation == "enroll":
            effect["enrollment"] = common | {
                "enrollment_id": previous["enrollment_id"] if previous else uuid.uuid4().hex,
                "binding_number": len(previous["bindings"]) + 1 if previous else 1, "enrolled_at": now}
        else:
            graph_binding = None
            if operation == "prepare_graph":
                graph = _head(view, parameters["graph_revision"])
                if graph["current_review"] != "approved_for_preparation":
                    _fail("graph is not approved for preparation")
                node = next((n for n in graph["manifest"]["nodes"] if n["task_id"] == task_id), None)
                if node is None:
                    _fail("task is not in the current graph")
                intent = {"schema_version": 1, "kind": "task-intent",
                          **{k: deepcopy(node[k]) for k in ("task_id", "owner", "acceptance_criteria")}}
                graph_binding = {"graph_id": view.graph_id, "graph_revision": view.current_graph_revision,
                    "graph_digest": graph["digest"], "decision_id": graph["decisions"][-1]["decision_id"],
                    "node_digest": node["node_digest"]}
            else:
                intent = deepcopy(parameters["intent"])
            older = [a["prepared"] for a in view.attempts.values() if a["prepared"]["task_id"] == task_id]
            record = common | {"schema_version": 2, "attempt_id": uuid.uuid4().hex,
                "enrollment_id": previous["enrollment_id"] if previous else None,
                "attempt_number": len(older) + 1, "previous_attempt_id": older[-1]["attempt_id"] if older else None,
                "execution_authority_id": view.authority_id, "intent": intent, "graph_binding": graph_binding,
                "prepared_at": now}
            record["intent_digest"] = _intent_digest(record)
            effect["prepared"] = record
    return _publish(store, control, view, binding, effect, key, lock)
