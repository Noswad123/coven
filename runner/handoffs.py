"""Bounded worker claims and local evidence capture, never result acceptance.

No native/Git probes, checks, reference following or task transitions. Archives
are content-addressed evidence under a cooperating-process publication protocol,
not authenticated worker records or an alternate execution/task authority.
"""

from copy import deepcopy
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import tempfile
import uuid

if __package__:
    from . import records, storage, task_ledger
else:
    import records
    import storage
    import task_ledger

HANDOFF = "coven.worker-handoff.v1"
COLLECTION = "coven.handoff-collection.v1"
MAX_INPUT = 262144
MAX_ARCHIVE = 1048576
MAX_ENTRIES = 256
MAX_RETAINED = 32 * 1048576
_HEX = re.compile(r"[0-9a-f]{64}")
_DOMAIN = b"coven.handoff-collection.v1\0"
VERIFICATION = {"worker_identity": "unestablished", "native_binding": "unestablished",
                "source": "unverified", "checks": "worker_reported", "task_completion": "unestablished"}


class HandoffError(storage.StorageError):
    pass


def _require(condition, code):
    if not condition:
        raise HandoffError(code)


def _canonical(value):
    try:
        return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode("utf-8")
    except (TypeError, ValueError, UnicodeError, RecursionError):
        raise HandoffError("invalid_handoff_json_values") from None


def _json(raw):
    def pairs(items):
        value = {}
        for key, item in items:
            _require(key not in value, "duplicate_json_key")
            value[key] = item
        return value
    try:
        text = raw.decode("utf-8", errors="strict")
        depth, quoted, escaped = 0, False, False
        for char in text:
            if quoted:
                if escaped:
                    escaped = False
                elif char == "\\":
                    escaped = True
                elif char == '"':
                    quoted = False
            elif char == '"':
                quoted = True
            elif char in "[{":
                depth += 1
                _require(depth <= 16, "json_nesting_limit")
            elif char in "]}":
                depth -= 1
        value = json.loads(text, object_pairs_hook=pairs, parse_constant=storage._reject_constant,
                           parse_float=storage._finite_float)
        _canonical(value)
        return value
    except (ValueError, UnicodeError, RecursionError):
        raise HandoffError("invalid_handoff_json") from None


def _fields(value, names):
    _require(type(value) is dict and set(value) == set(names.split()), "invalid_handoff_fields")


def _text(value, limit=16384, *, blank=False, multiline=True):
    _require(type(value) is str, "invalid_handoff_text")
    try:
        _require(len(value.encode("utf-8")) <= limit and (blank or bool(value.strip()))
                 and not any((ord(c) < 32 and (not multiline or c not in "\n\r\t"))
                             or 127 <= ord(c) <= 159 for c in value), "invalid_handoff_text")
    except UnicodeError:
        raise HandoffError("invalid_handoff_text") from None


def _choice(value, choices):
    _require(type(value) is str and value in choices, "invalid_handoff_choice")


def _id(value):
    _require(type(value) is str and re.fullmatch(r"[A-Za-z0-9_-]{1,128}", value) is not None, "invalid_handoff_id")


def _revision(value):
    _require(type(value) is str, "required_tasks_revision")
    task_ledger.validate_revision(value)


def _strings(values, *, paths=False, limit=64):
    _require(type(values) is list and len(values) <= limit, "handoff_list_limit")
    for value in values:
        _text(value, 4096, multiline=not paths)
        if paths:
            path = PurePosixPath(value)
            _require(not path.is_absolute() and value == path.as_posix() and ".." not in path.parts
                     and value != "." and "\\" not in value, "invalid_claimed_path")
    _require(len(set(values)) == len(values), "duplicate_handoff_value")


def validate(value):
    """Pure schema validation. All returned contents are still worker claims."""
    _fields(value, "schema_version contract task claimed_status summary confidence findings files_inspected files_changed checks blockers risks follow_ups references source_claim")
    _require(type(value["schema_version"]) is int and value["schema_version"] == 1
             and value["contract"] == HANDOFF, "unsupported_handoff_contract")
    _fields(value["task"], "id owner revision")
    _id(value["task"]["id"])
    _id(value["task"]["owner"])
    _revision(value["task"]["revision"])
    _choice(value["claimed_status"], {"completed", "partial", "blocked", "failed"})
    _choice(value["confidence"], {"high", "medium", "low"})
    _text(value["summary"])
    _strings(value["files_inspected"], paths=True, limit=256)
    _strings(value["files_changed"], paths=True, limit=256)
    for field in ("blockers", "risks", "follow_ups"):
        _strings(value[field])
    for field in ("findings", "checks", "references"):
        _require(type(value[field]) is list and len(value[field]) <= 64, "handoff_list_limit")
    for finding in value["findings"]:
        _fields(finding, "priority claim evidence")
        _choice(finding["priority"], {"high", "medium", "low"})
        _text(finding["claim"], 8192)
        _strings(finding["evidence"], limit=16)
    for check in value["checks"]:
        _fields(check, "name claimed_outcome details evidence")
        _text(check["name"], 256, multiline=False)
        _choice(check["claimed_outcome"], {"passed", "failed", "not_run"})
        _text(check["details"], 8192, blank=True)
        _strings(check["evidence"], limit=16)
    for reference in value["references"]:
        _fields(reference, "kind uri")
        _choice(reference["kind"], {"transcript", "artifact", "check", "native_session", "repository"})
        _text(reference["uri"], 4096, multiline=False)
    source = value["source_claim"]
    if source is not None:
        _fields(source, "state repository_id base_sha source_sha tree_sha")
        _choice(source["state"], {"committed", "uncommitted", "unknown"})
        if source["repository_id"] is not None:
            try:
                _require(str(uuid.UUID(source["repository_id"])) == source["repository_id"], "invalid_repository_claim")
            except (ValueError, TypeError, AttributeError):
                raise HandoffError("invalid_repository_claim") from None
        for field in ("base_sha", "source_sha", "tree_sha"):
            _require(source[field] is None or (type(source[field]) is str and re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", source[field]) is not None),
                     "invalid_source_claim")
        hashes = [source[k] for k in ("base_sha", "source_sha", "tree_sha") if source[k] is not None]
        _require(len({len(item) for item in hashes}) <= 1, "mixed_object_formats")
        _require(len(hashes) == 3 if source["state"] == "committed"
                 else source["source_sha"] is None and source["tree_sha"] is None, "inconsistent_source_claim")
    _require(not value["files_changed"] or source is not None, "changed_files_require_source_claim")
    _require(len(_canonical(value)) <= MAX_INPUT, "handoff_size_limit")
    return deepcopy(value)


def _read(path, limit):
    try:
        with os.fdopen(storage._open_regular(Path(path), os.O_RDONLY), "rb") as source:
            data = source.read(limit+1)
        _require(len(data) <= limit, "handoff_size_limit")
        return data
    except (OSError, ValueError):
        raise HandoffError("handoff_file_unavailable") from None


def read_handoff(path):
    raw = _read(Path(path).absolute(), MAX_INPUT)
    return raw, validate(_json(raw))


def _report(assessment, value, **fields):
    return {"schema_version": 1, "contract": COLLECTION, "assessment": assessment,
            "authorization": "none", "dispatchable": False, "verification": dict(VERIFICATION),
            "handoff": deepcopy(value), **fields}


def validate_file(path):
    raw, value = read_handoff(path)
    return _report("valid_unverified", value, handoff_sha256=hashlib.sha256(raw).hexdigest(), handoff_bytes=len(raw))


def _path(root, relative, *, directory=False):
    current = root
    parts = Path(relative).parts
    _require(bool(parts) and not Path(relative).is_absolute() and ".." not in parts, "unsafe_archive_path")
    for index, part in enumerate(parts):
        current = current / part
        try:
            mode = current.lstat().st_mode
        except FileNotFoundError:
            continue
        _require(stat.S_ISDIR(mode) if directory or index < len(parts)-1 else stat.S_ISREG(mode), "unsafe_archive_path")
    return current


def _root(workspace):
    try:
        root = Path(workspace).resolve(strict=True)
        _require(root.is_dir(), "invalid_handoff_workspace")
        marker = _path(root, "coven.json")
        _require(marker.is_file(), "invalid_handoff_workspace")
        return root
    except (OSError, RuntimeError):
        raise HandoffError("invalid_handoff_workspace") from None


def _context(root, value, expected_revision):
    _path(root, "state", directory=True)
    try:
        team = records.team(_json(_read(_path(root, "coven.json"), 65536)))
    except ValueError:
        raise HandoffError("invalid_handoff_roster") from None
    store = task_ledger.LedgerStore(root)
    view = store.read()
    if view is None:
        _require(not store.has_pending_import(), "pending_task_authority")
        raw = _read(_path(root, "state/tasks.json"), 1048576)
        try:
            revision, tasks = storage.snapshot_revision(raw), records.tasks(_json(raw))
        except ValueError:
            raise HandoffError("invalid_handoff_task_source") from None
    else:
        revision, tasks = view.revision, view.tasks
    task_claim = value["task"]
    _require(revision == expected_revision == task_claim["revision"], "stale_handoff_task_revision")
    matches = [task for task in tasks if task["id"] == task_claim["id"]]
    _require(len(matches) == 1, "handoff_task_not_found")
    task = matches[0]
    _require(task.get("owner") == task_claim["owner"] and task_claim["owner"] in team["agents"], "handoff_owner_mismatch")
    return {"task_id": task["id"], "owner": task["owner"], "revision": revision,
            "task_sha256": hashlib.sha256(b"coven.handoff-task.v1\0"+_canonical(task)).hexdigest()}


def _scan(root):
    directory = _path(root, "output/handoffs", directory=True)
    if not directory.exists():
        return [], 0, 0
    entries, total, count = [], 0, 0
    for entry in directory.iterdir():
        count += 1
        _require(count <= MAX_ENTRIES, "handoff_archive_entry_limit")
        temporary = re.fullmatch(r"\.handoff-[A-Za-z0-9_-]+\.tmp", entry.name) is not None
        try:
            selected = _path(root, entry.relative_to(root))
            size = selected.stat().st_size
        except FileNotFoundError:
            # Read-only/preflight scans do not hold the writer lock. A publisher
            # may unlink its inert temp after iteration but before stat. Missing
            # final archives remain errors: cooperating writers never delete them.
            if temporary:
                continue
            raise
        _require(size <= MAX_ARCHIVE, "handoff_archive_size_limit")
        total += size
        _require(total <= MAX_RETAINED, "handoff_archive_capacity")
        if temporary:
            continue  # Inert interrupted publication; never promote or delete it.
        _require(re.fullmatch(r"[0-9a-f]{64}\.json", entry.name) is not None, "unexpected_handoff_archive_entry")
        entries.append(selected)
    return sorted(entries), total, count


def _envelope(raw, binding, root):
    _text(str(root), 8192, multiline=False)
    payload = {"handoff_json": raw.decode("utf-8"), "handoff_sha256": hashlib.sha256(raw).hexdigest(),
               "handoff_bytes": len(raw), "task_binding": binding, "workspace_at_collection": str(root)}
    identity = hashlib.sha256(_DOMAIN+_canonical(payload)).hexdigest()
    envelope = {"schema_version": 1, "contract": COLLECTION, "collection_id": identity, "payload": payload}
    encoded = _canonical(envelope)+b"\n"
    _require(len(encoded) <= MAX_ARCHIVE, "handoff_archive_size_limit")
    return identity, encoded


def _archive(root, path):
    raw = _read(path, MAX_ARCHIVE)
    archive = _json(raw)
    _fields(archive, "schema_version contract collection_id payload")
    _require(type(archive["schema_version"]) is int and archive["schema_version"] == 1
             and archive["contract"] == COLLECTION, "unsupported_collection_contract")
    payload = archive["payload"]
    _fields(payload, "handoff_json handoff_sha256 handoff_bytes task_binding workspace_at_collection")
    _require(type(payload["handoff_json"]) is str, "invalid_embedded_handoff")
    embedded = payload["handoff_json"].encode("utf-8")
    _require(len(embedded) <= MAX_INPUT and type(payload["handoff_bytes"]) is int
             and payload["handoff_bytes"] == len(embedded)
             and payload["handoff_sha256"] == hashlib.sha256(embedded).hexdigest(), "handoff_digest_mismatch")
    value = validate(_json(embedded))
    binding = payload["task_binding"]
    _fields(binding, "task_id owner revision task_sha256")
    _require(binding["task_id"] == value["task"]["id"] and binding["owner"] == value["task"]["owner"]
             and binding["revision"] == value["task"]["revision"] and type(binding["task_sha256"]) is str
             and _HEX.fullmatch(binding["task_sha256"]) is not None, "invalid_handoff_binding")
    _text(payload["workspace_at_collection"], 8192, multiline=False)
    _require(os.path.isabs(payload["workspace_at_collection"]), "invalid_historical_workspace")
    identity = hashlib.sha256(_DOMAIN+_canonical(payload)).hexdigest()
    _require(archive["collection_id"] == identity and path.name == identity+".json"
             and raw == _canonical(archive)+b"\n", "collection_integrity_mismatch")
    return _report("historical_unverified", value, collection_id=identity,
                   handoff_sha256=payload["handoff_sha256"], handoff_bytes=len(embedded),
                   task_binding=deepcopy(binding), workspace_at_collection=payload["workspace_at_collection"],
                   artifact_path=str(path), current_task_binding="not_checked")


def inspect(workspace, identity):
    _require(type(identity) is str and _HEX.fullmatch(identity) is not None, "invalid_collection_id")
    root = _root(workspace)
    return _archive(root, _path(root, f"output/handoffs/{identity}.json"))


def list_collections(workspace, *, task_id=None):
    if task_id is not None:
        _id(task_id)
    root = _root(workspace)
    entries, _, _ = _scan(root)
    reports = [_archive(root, path) for path in entries]
    selected = [item for item in reports if task_id is None or item["handoff"]["task"]["id"] == task_id]
    return {"schema_version": 1, "contract": COLLECTION, "assessment": "historical_unverified",
            "authorization": "none", "dispatchable": False, "verification": dict(VERIFICATION),
            "collections": [{"collection_id": item["collection_id"], "task": item["handoff"]["task"],
                "claimed_status": item["handoff"]["claimed_status"], "summary": item["handoff"]["summary"],
                "artifact_path": item["artifact_path"]} for item in selected],
            "selected_collection_count": len(selected),
            "observed_collection_count": len(reports), "current_task_binding": "not_checked"}


def _directories(root):
    for relative in ("output", "output/handoffs"):
        path = _path(root, relative, directory=True)
        if not path.exists():
            path.mkdir(mode=0o700)
        # Also sync existing parents on retry after interrupted directory creation.
        storage.sync_directory(path)
        storage.sync_directory(path.parent)


def _publish(root, path, encoded):
    temp = None
    published = False
    try:
        fd, name = tempfile.mkstemp(prefix=".handoff-", suffix=".tmp", dir=path.parent)
        temp = Path(name)
        with os.fdopen(fd, "wb") as stream:
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        _path(root, path.relative_to(root))
        os.link(temp, path)  # Exclusive publication; never replace captured bytes.
        published = True
        storage.sync_directory(path.parent)
        return "committed"
    except OSError as exc:
        if published:
            return "durability_uncertain"
        raise HandoffError("handoff_archive_not_published") from exc
    finally:
        if temp is not None:
            try:
                temp.unlink(missing_ok=True)
            except OSError:
                pass  # Inert orphan, never promoted implicitly.


def collect(workspace, path, *, expected_sha256, expected_tasks_revision):
    _require(type(expected_sha256) is str and _HEX.fullmatch(expected_sha256) is not None, "required_handoff_sha256")
    _revision(expected_tasks_revision)
    raw, value = read_handoff(path)
    _require(hashlib.sha256(raw).hexdigest() == expected_sha256, "handoff_input_changed")
    root = _root(workspace)
    _context(root, value, expected_tasks_revision)  # Deterministic errors before creating a lock/output.
    _scan(root)
    lock_path = _path(root, "state/.workspace.lock")
    with storage.workspace_lock(root, lock_path):
        binding = _context(root, value, expected_tasks_revision)
        identity, encoded = _envelope(raw, binding, root)
        target = _path(root, f"output/handoffs/{identity}.json")
        _, total, count = _scan(root)
        replayed = target.exists()
        if replayed:
            _require(_read(target, MAX_ARCHIVE) == encoded, "collection_integrity_mismatch")
        else:
            # Reserve both temp and final directory entries/size accounting, so
            # an interrupted hard-link publication remains inspectable/retryable.
            _require(count+2 <= MAX_ENTRIES and total+2*len(encoded) <= MAX_RETAINED, "handoff_archive_capacity")
        if replayed:
            try:
                _directories(root)
                with os.fdopen(storage._open_regular(target, os.O_RDONLY), "rb") as source:
                    os.fsync(source.fileno())
                storage.sync_directory(target.parent)
                delivery = "committed"
            except OSError:
                delivery = "durability_uncertain"
        else:
            _directories(root)
            delivery = _publish(root, target, encoded)
        return _report("collected_unverified", value, collection_id=identity,
                       handoff_sha256=expected_sha256, handoff_bytes=len(raw), task_binding=binding,
                       artifact_path=str(target), publication="existing" if replayed else "created",
                       delivery=delivery, effect="local_artifact_capture_only")
