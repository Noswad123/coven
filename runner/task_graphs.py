"""Pure reviewed DAGs, not scheduling, acceptance, or execution authority.

An edge B.depends_on[A] intends A's accepted output before B executes, not
before B is prepared. No acceptance-result authority exists here; all nodes
remain non-dispatchable, including nodes with no dependencies.
"""

from copy import deepcopy
import hashlib
import heapq
import re

if __package__:
    from . import records, storage, task_attempts
else:
    import records
    import storage
    import task_attempts


class GraphError(storage.StorageError):
    """A reviewed graph or its source cannot safely be used."""


MAX_BYTES = 1024 * 1024
DOCUMENT_NODE = "task_id owner acceptance_criteria depends_on"
FROZEN_NODE = DOCUMENT_NODE + " enrollment_id binding_number complete_task_snapshot task_snapshot_digest node_digest"


def _attempt(helper, *args):
    try:
        return helper(*args)
    except task_attempts.AttemptError as exc:
        raise GraphError(str(exc)) from exc


def canonical(value):
    """Strict canonical JSON; sort object keys, never arrays or operator text."""
    return _attempt(task_attempts.canonical, value)


def digest(manifest):
    return "task-graph-manifest-v1:" + hashlib.sha256(
        b"coven-task-graph-manifest-v1\0" + canonical(manifest)).hexdigest()


def node_digest(node):
    if not isinstance(node, dict):
        raise GraphError("graph node must be an object")
    payload = {key: value for key, value in node.items() if key != "node_digest"}
    return "task-graph-node-v1:" + hashlib.sha256(
        b"coven-task-graph-node-v1\0" + canonical(payload)).hexdigest()


def _fields(value, names):
    if not isinstance(value, dict) or set(value) != set(names.split()):
        raise GraphError("invalid graph fields; expected only " + names)


def _match(value, pattern, name):
    if not isinstance(value, str) or not re.fullmatch(pattern, value):
        raise GraphError("invalid " + name)


def _bounded(value):
    if len(canonical(value)) > MAX_BYTES:
        raise GraphError("graph exceeds 1 MiB")


def _header(value, kind, fields):
    _bounded(value)
    _fields(value, fields)
    if type(value["schema_version"]) is not int or value["schema_version"] != 1:
        raise GraphError("unsupported graph schema version")
    if value["kind"] != kind:
        raise GraphError("invalid graph kind")


def _order(nodes):
    pending = {node["task_id"]: len(node["depends_on"]) for node in nodes}
    children = {key: [] for key in pending}
    for node in nodes:
        for dependency in node["depends_on"]:
            if dependency not in pending or dependency == node["task_id"]:
                raise GraphError("dangling or self dependency")
            children[dependency].append(node["task_id"])
    ready = [key for key, count in pending.items() if count == 0]
    heapq.heapify(ready)
    result = []
    while ready:
        key = heapq.heappop(ready)
        result.append(key)
        for child in children[key]:
            pending[child] -= 1
            if pending[child] == 0:
                heapq.heappush(ready, child)
    if len(result) != len(nodes):
        raise GraphError("graph contains a cycle")
    return result


def _nodes(nodes, frozen=False):
    if not isinstance(nodes, list) or not 1 <= len(nodes) <= 64:
        raise GraphError("graph requires 1..64 nodes")
    seen, edges = set(), 0
    for node in nodes:
        _fields(node, FROZEN_NODE if frozen else DOCUMENT_NODE)
        _attempt(task_attempts.validate_intent, {
            "schema_version": 1, "kind": "task-intent",
            **{key: node[key] for key in ("task_id", "owner", "acceptance_criteria")}})
        if node["task_id"] in seen:
            raise GraphError("duplicate task ID")
        seen.add(node["task_id"])
        dependencies = node["depends_on"]
        if not isinstance(dependencies, list):
            raise GraphError("depends_on must be a list")
        for dependency in dependencies:
            _attempt(task_attempts._id, dependency)
        if len(set(dependencies)) != len(dependencies):
            raise GraphError("duplicate dependency")
        edges += len(dependencies)
    if edges > 256:
        raise GraphError("graph exceeds 256 edges")
    _order(nodes)


def validate_document(value):
    _header(value, "task-graph", "schema_version kind nodes")
    _nodes(value["nodes"])
    return deepcopy(value)


def topology(document_or_manifest):
    if isinstance(document_or_manifest, dict) and document_or_manifest.get("kind") == "task-graph-manifest":
        value = validate_manifest(document_or_manifest)
    else:
        value = validate_document(document_or_manifest)
    return _order(value["nodes"])


def _identity(identity, revision):
    _match(revision, task_attempts.TASK_REV, "task source revision")
    if not isinstance(identity, dict):
        raise GraphError("invalid task authority identity")
    if identity.get("kind") == "snapshot-v1":
        _fields(identity, "kind source_id path")
        _match(identity["source_id"], task_attempts.UUID, "snapshot source ID")
        if identity["path"] != "state/tasks.json":
            raise GraphError("invalid snapshot logical path")
        try:
            storage.validate_revision(revision)
        except storage.StorageError as exc:
            raise GraphError("snapshot identity/revision mismatch") from exc
    elif identity.get("kind") == "task-ledger-v1":
        _fields(identity, "kind workspace_id epoch_id path")
        for key in ("workspace_id", "epoch_id"):
            _match(identity[key], task_attempts.UUID, key)
        prefix = f"tasks-ledger:v1:{identity['workspace_id']}:{identity['epoch_id']}:"
        if identity["path"] != "state/task-ledger" or not revision.startswith(prefix):
            raise GraphError("ledger identity/revision mismatch")
    else:
        raise GraphError("unsupported task authority identity")


def _task(task, task_id):
    try:
        records.tasks([task])
    except ValueError as exc:
        raise GraphError(str(exc)) from exc
    if task["id"] != task_id:
        raise GraphError("task snapshot ID mismatch")


def _positive(value):
    if type(value) is not int or value < 1:
        raise GraphError("binding number must be a positive integer")


def validate_manifest(manifest):
    _header(manifest, "task-graph-manifest", "schema_version kind execution_authority_id graph_id graph_revision predecessor task_authority_identity task_source_revision nodes proposed_at")
    for key in ("execution_authority_id", "graph_id", "graph_revision"):
        _match(manifest[key], task_attempts.UUID, key)
    if manifest["predecessor"] is not None:
        _match(manifest["predecessor"], task_attempts.UUID, "predecessor")
    _attempt(task_attempts._timestamp, manifest["proposed_at"])
    _identity(manifest["task_authority_identity"], manifest["task_source_revision"])
    if manifest["task_source_revision"] == "tasks-missing:v1":
        raise GraphError("missing source cannot contain graph tasks")
    _nodes(manifest["nodes"], frozen=True)
    enrollments = set()
    for node in manifest["nodes"]:
        _match(node["enrollment_id"], task_attempts.UUID, "enrollment ID")
        if node["enrollment_id"] in enrollments:
            raise GraphError("duplicate enrollment ID")
        enrollments.add(node["enrollment_id"])
        _positive(node["binding_number"])
        task = node["complete_task_snapshot"]
        _task(task, node["task_id"])
        if task.get("owner") != node["owner"]:
            raise GraphError("frozen owner must exactly match task assignment")
        if node["task_snapshot_digest"] != _attempt(task_attempts.task_snapshot_digest, task):
            raise GraphError("task snapshot digest mismatch")
        if node["node_digest"] != node_digest(node):
            raise GraphError("node digest mismatch")
    # Selected snapshots cannot reconstruct a whole-ledger revision hash.
    # The store must re-freeze against its retained full source during replay.
    return deepcopy(manifest)


def _enrollment(enrollments, task_id):
    if not isinstance(enrollments, dict):
        raise GraphError("enrollments must be a current v1 enrollment map")
    enrollment = enrollments.get(task_id)
    if enrollment is None:
        return None
    _fields(enrollment, "enrollment_id task_id bindings")
    _match(enrollment["enrollment_id"], task_attempts.UUID, "enrollment ID")
    if enrollment["task_id"] != task_id:
        raise GraphError("enrollment task ID mismatch")
    bindings = enrollment["bindings"]
    if not isinstance(bindings, list) or not bindings:
        raise GraphError("enrollment requires a current binding")
    binding = bindings[-1]
    _fields(binding, "enrollment_id task_id binding_number task_authority_identity task_source_revision complete_task_snapshot task_snapshot_digest enrolled_at")
    _positive(binding["binding_number"])
    if binding["task_id"] != task_id or binding["enrollment_id"] != enrollment["enrollment_id"]:
        raise GraphError("enrollment binding identity mismatch")
    _identity(binding["task_authority_identity"], binding["task_source_revision"])
    _attempt(task_attempts._timestamp, binding["enrolled_at"])
    _task(binding["complete_task_snapshot"], task_id)
    if binding["task_snapshot_digest"] != _attempt(task_attempts.task_snapshot_digest, binding["complete_task_snapshot"]):
        raise GraphError("enrollment snapshot digest mismatch")
    return binding


def freeze(document, source, enrollments, *, authority_id, snapshot_source_id,
           graph_id, revision_id, predecessor, proposed_at):
    document = validate_document(document)
    _match(snapshot_source_id, task_attempts.UUID, "snapshot source ID")
    source = _attempt(task_attempts._source, source, snapshot_source_id)
    canonical(enrollments)
    tasks = {task["id"]: task for task in source["tasks"]}
    nodes = []
    for node in document["nodes"]:
        task = tasks.get(node["task_id"])
        if task is None:
            raise GraphError("selected task not found")
        if task.get("owner") != node["owner"] or node["owner"] not in source["roster"]:
            raise GraphError("owner must exactly match assignment and current canonical roster ID")
        binding = _enrollment(enrollments, node["task_id"])
        if binding is None or binding["task_authority_identity"] != source["identity"]:
            raise GraphError("selected task must be enrolled against the current source identity")
        frozen = node | {"enrollment_id": binding["enrollment_id"],
                         "binding_number": binding["binding_number"],
                         "complete_task_snapshot": deepcopy(task),
                         "task_snapshot_digest": _attempt(task_attempts.task_snapshot_digest, task)}
        frozen["node_digest"] = node_digest(frozen)
        nodes.append(frozen)
    return validate_manifest({"schema_version": 1, "kind": "task-graph-manifest",
        "execution_authority_id": authority_id, "graph_id": graph_id,
        "graph_revision": revision_id, "predecessor": predecessor,
        "task_authority_identity": deepcopy(source["identity"]),
        "task_source_revision": source["revision"], "nodes": nodes, "proposed_at": proposed_at})


def match_source(manifest, source, enrollments):
    manifest = validate_manifest(manifest)
    canonical(source)
    _fields(source, "identity revision tasks roster")
    _identity(source["identity"], source["revision"])
    snapshot_id = source["identity"].get("source_id", "0" * 32)
    source = _attempt(task_attempts._source, source, snapshot_id)
    canonical(enrollments)
    tasks = {task["id"]: task for task in source["tasks"]}
    reasons = []
    changed_authority = source["identity"] != manifest["task_authority_identity"]
    for node in sorted(manifest["nodes"], key=lambda item: item["task_id"]):
        task_id = node["task_id"]
        task = tasks.get(task_id)
        binding = _enrollment(enrollments, task_id)
        changed = []
        if changed_authority:
            changed.append("source_authority_changed")
        if task is None or canonical(task) != canonical(node["complete_task_snapshot"]):
            changed.append("selected_inputs_changed")
        if (binding is None or binding["enrollment_id"] != node["enrollment_id"]
                or binding["binding_number"] != node["binding_number"]
                or binding["task_authority_identity"] != source["identity"]):
            changed.append("enrollment_changed")
        if node["owner"] not in source["roster"]:
            changed.append("owner_missing")
        reasons.extend({"task_id": task_id, "reason": reason} for reason in changed)
    priority = ("source_authority_changed", "selected_inputs_changed", "enrollment_changed", "owner_missing")
    statuses = {reason["reason"] for reason in reasons}
    return {"status": next((status for status in priority if status in statuses), "current"),
            "whole_revision_changed": source["revision"] != manifest["task_source_revision"],
            "reasons": reasons}
