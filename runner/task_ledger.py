"""Opt-in task authority. Immutable frames are authority; tasks.json is a projection.

Static path checks are deliberately the same threat model as storage.py, not a
race-proof sandbox. All publication and recovery operations require its lock.
"""

from copy import deepcopy
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import tempfile
import uuid

if __package__:
    from . import records, storage, task_attempts
else:
    import records
    import storage
    import task_attempts


class LedgerError(storage.StorageError):
    """Ledger authority cannot safely be used."""


class PublicationUncertain(LedgerError):
    def __init__(self, path):
        self.path = Path(path)
        super().__init__(f"{path}: final name published; durability uncertain; inspect/recover before retry")


UUID = r"[0-9a-f]{32}"
HEX = r"[0-9a-f]{64}"
MAX_FRAME = 8 * 1024 * 1024
MARKER = "state/task-authority.json"
LEDGER = "state/task-ledger"


def validate_revision(token):
    if token is None:
        return
    if not isinstance(token, str) or not re.fullmatch(
            rf"tasks-missing:v1|tasks-sha256:{HEX}|tasks-ledger:v1:{UUID}:{UUID}:(0|[1-9][0-9]*):{HEX}", token):
        raise LedgerError("invalid tasks revision")


def validate_key(key):
    if not isinstance(key, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:-]{0,127}", key):
        raise LedgerError("invalid request key")


def request_binding(operation, args, expected_revision):
    validate_revision(expected_revision)
    fields = {"add": {"title", "owner", "notes"}, "assign": {"task_id", "agent"}}
    if not isinstance(operation, str) or operation not in fields or not isinstance(args, dict) or set(args) != fields[operation]:
        raise LedgerError("invalid request operation or argument fields")
    try:
        for name, value in args.items():
            if operation == "add" and name == "owner" and value is None:
                continue
            records.text(value, name, nonblank=name not in {"notes"})
    except ValueError as exc:
        raise LedgerError(str(exc)) from exc
    return {"version": 1, "operation": operation, "args": deepcopy(args), "expected_revision": expected_revision}


def _canonical(value):
    try:
        return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode("utf-8")
    except (ValueError, TypeError, UnicodeError, RecursionError) as exc:
        raise LedgerError(f"invalid JSON: {exc}") from exc


def _digest(data):
    return hashlib.sha256(data).hexdigest()


def _json(data):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError(f"duplicate JSON key {key}")
            result[key] = value
        return result
    try:
        return json.loads(data, object_pairs_hook=pairs, parse_constant=storage._reject_constant,
                          parse_float=storage._finite_float)
    except (ValueError, UnicodeError, RecursionError) as exc:
        raise LedgerError(f"invalid strict JSON: {exc}") from exc


def _fields(value, names):
    if not isinstance(value, dict) or set(value) != set(names.split()):
        raise LedgerError("invalid ledger fields")


def _version(value):
    if type(value) is not int or value != 1:
        raise LedgerError("unsupported ledger version")


def _tasks(value):
    try:
        return records.tasks(value)
    except ValueError as exc:
        raise LedgerError(str(exc)) from exc


def _revision(workspace, epoch, sequence, tasks):
    return f"tasks-ledger:v1:{workspace}:{epoch}:{sequence}:{_digest(_canonical(tasks))}"


def _envelope(payload):
    raw = _canonical(payload)
    data = _canonical({"version": 1, "length": len(raw), "sha256": _digest(raw), "payload": payload}) + b"\n"
    if len(data) > MAX_FRAME:
        raise LedgerError("frame exceeds 8 MiB")
    return data


def _payload(data):
    if len(data) > MAX_FRAME:
        raise LedgerError("frame exceeds 8 MiB")
    envelope = _json(data)
    _fields(envelope, "version length sha256 payload")
    _version(envelope["version"])
    raw = _canonical(envelope["payload"])
    if type(envelope["length"]) is not int or envelope["length"] != len(raw) or envelope["sha256"] != _digest(raw):
        raise LedgerError("frame integrity mismatch")
    if data != _envelope(envelope["payload"]):
        raise LedgerError("frame encoding is not canonical")
    return envelope["payload"]


@dataclass
class LedgerView:
    tasks: list
    revision: str
    workspace_id: str
    epoch_id: str
    sequence: int
    head_digest: str
    projection_bytes: bytes
    receipts: dict


def _alias(value):
    return re.sub(r"[^a-zA-Z0-9_-]+", "-", value.strip().lower()).strip("-")


def _apply(view, payload, digest):
    _fields(payload, "version workspace_id epoch_id sequence previous_digest request_id binding result")
    _version(payload["version"])
    seq = view.sequence + 1
    if (type(payload["sequence"]) is not int or payload["sequence"] != seq
            or payload["workspace_id"] != view.workspace_id or payload["epoch_id"] != view.epoch_id
            or payload["previous_digest"] != view.head_digest):
        raise LedgerError("frame identity, sequence, or previous digest mismatch")
    key, binding, result = payload["request_id"], payload["binding"], payload["result"]
    validate_key(key)
    if key in view.receipts:
        raise LedgerError("duplicate request ID")
    _fields(binding, "version operation args expected_revision")
    _version(binding["version"])
    if binding != request_binding(binding["operation"], binding["args"], binding["expected_revision"]):
        raise LedgerError("invalid request binding")
    if binding["expected_revision"] not in (None, view.revision):
        raise LedgerError("expected revision mismatch")
    _fields(result, "outcome mutation_id task before_revision after_revision sequence")
    if not isinstance(result["mutation_id"], str) or not re.fullmatch(UUID, result["mutation_id"]):
        raise LedgerError("invalid mutation ID")
    task = deepcopy(result["task"])
    _tasks([task])
    tasks, args = deepcopy(view.tasks), binding["args"]
    if binding["operation"] == "add":
        base = _alias(args["title"])
        if not re.search(r"[a-z0-9]", base):
            raise LedgerError("invalid task title slug")
        ids = {item["id"] for item in tasks}
        candidate, index = base, 2
        while candidate in ids:
            candidate, index = f"{base}-{index}", index + 1
        owner = task.get("owner")
        if (task["id"] != candidate or task["title"] != args["title"] or task.get("notes") != args["notes"]
                or (owner is None) != (args["owner"] is None)
                or (owner is not None and _alias(owner) != _alias(args["owner"]))
                or task["status"] != ("assigned" if owner is not None else "pending")
                or not isinstance(task.get("created_at"), str) or not isinstance(task.get("updated_at"), str)):
            raise LedgerError("add effect does not match request")
        tasks.append(task)
    else:
        matches = [i for i, item in enumerate(tasks) if item["id"] == args["task_id"]]
        if len(matches) != 1:
            raise LedgerError("assigned task not found")
        index = matches[0]
        expected = deepcopy(tasks[index])
        expected.update(owner=task.get("owner"), status="assigned", updated_at=task.get("updated_at"))
        if (_canonical(task) != _canonical(expected) or not isinstance(task.get("owner"), str)
                or _alias(task["owner"]) != _alias(args["agent"])
                or not isinstance(task.get("updated_at"), str)):
            raise LedgerError("assign effect does not match request")
        tasks[index] = task
    revision = _revision(view.workspace_id, view.epoch_id, seq, tasks)
    if (result["outcome"] != "committed" or result["before_revision"] != view.revision
            or result["after_revision"] != revision or type(result["sequence"]) is not int or result["sequence"] != seq):
        raise LedgerError("invalid frozen result or revision")
    receipts = deepcopy(view.receipts)
    receipts[key] = {"binding": deepcopy(binding), "result": deepcopy(result)}
    return LedgerView(tasks, revision, view.workspace_id, view.epoch_id, seq, digest,
                      storage.encode_json(tasks), receipts)


class LedgerStore:
    def __init__(self, workspace):
        try:
            self.workspace = Path(workspace).resolve(strict=True)
        except OSError as exc:
            raise LedgerError(f"cannot resolve ledger workspace: {exc}") from exc
        if not self.workspace.is_dir():
            raise LedgerError("workspace must be a directory")

    def path(self, relative, directory=False):
        relative = Path(relative)
        if relative.is_absolute() or not relative.parts or ".." in relative.parts:
            raise LedgerError("unsafe ledger path")
        path = self.workspace
        for index, part in enumerate(relative.parts):
            path = path / part
            try:
                mode = path.lstat().st_mode
            except FileNotFoundError:
                continue
            except OSError as exc:
                raise LedgerError(f"{path}: cannot inspect ledger path: {exc}") from exc
            want_dir = directory or index < len(relative.parts) - 1
            if not (stat.S_ISDIR(mode) if want_dir else stat.S_ISREG(mode)):
                raise LedgerError(f"{path}: unsafe path type or symlink")
        return path

    def _lock(self, lock):
        if lock is None:
            raise LedgerError("explicit active workspace lock required")
        return storage.workspace_lock(self.workspace, self.path("state/.workspace.lock"), held=lock)

    def _read(self, relative, missing=False):
        path = self.workspace / relative
        try:
            path = self.path(relative)
            with os.fdopen(storage._open_regular(path, os.O_RDONLY), "rb") as stream:
                if path.suffix == ".frame":
                    data = stream.read(MAX_FRAME + 1)
                    if len(data) > MAX_FRAME:
                        raise LedgerError("frame exceeds 8 MiB")
                    return data
                return stream.read()
        except FileNotFoundError as exc:
            if missing:
                return None
            raise LedgerError(f"{path}: required authority file missing") from exc
        except OSError as exc:
            raise LedgerError(f"{path}: {exc}") from exc

    def _mkdir(self, relative):
        path = self.path(relative, directory=True)
        try:
            path.mkdir()
            storage.sync_directory(path.parent)
        except OSError as exc:
            raise LedgerError(f"{path}: cannot create durable directory: {exc}") from exc

    def _publish(self, relative, data, mode=0o600):
        path, temp = self.path(relative), None
        published = False
        try:
            fd, name = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".tmp", dir=path.parent)
            temp = Path(name)
            with os.fdopen(fd, "wb") as stream:
                stream.write(data)
                stream.flush()
                os.fchmod(stream.fileno(), mode)
                os.fsync(stream.fileno())
            self.path(relative)
            os.link(temp, path)
            published = True
            storage.sync_directory(path.parent)
        except OSError as exc:
            if published:
                raise PublicationUncertain(path) from exc
            raise LedgerError(f"{path}: immutable publication failed: {exc}") from exc
        finally:
            if temp is not None:
                try:
                    temp.unlink(missing_ok=True)
                except OSError:
                    pass

    def _entries(self, relative, allowed):
        path = self.path(relative, directory=True)
        try:
            names = []
            for entry in path.iterdir():
                # Only our exact temp naming scheme is inert, and never a directory/symlink.
                temp = re.fullmatch(r"\.(?:manifest\.json|baseline\.tasks\.bytes|[0-9]{20}\.frame)\.[A-Za-z0-9_-]+\.tmp", entry.name)
                self.path(entry.relative_to(self.workspace), directory=entry.name in allowed and allowed[entry.name])
                if temp:
                    continue
                names.append(entry.name)
            return sorted(names)
        except OSError as exc:
            raise LedgerError(f"{path}: cannot enumerate ledger: {exc}") from exc

    def has_pending_import(self):
        return self._read(MARKER, missing=True) is None and self.path(LEDGER, directory=True).exists()

    def _epoch(self, marker):
        _fields(marker, "version workspace_id epoch_id manifest_digest baseline_digest")
        _version(marker["version"])
        for field in ("workspace_id", "epoch_id"):
            if not isinstance(marker[field], str) or not re.fullmatch(UUID, marker[field]):
                raise LedgerError("invalid ledger identity")
        base = f"{LEDGER}/{marker['epoch_id']}"
        raw = self._read(f"{base}/manifest.json")
        if _digest(raw) != marker["manifest_digest"]:
            raise LedgerError("manifest digest mismatch")
        manifest = _json(raw)
        _fields(manifest, "version workspace_id epoch_id source_revision source_present source_mode source_length source_digest baseline_digest")
        _version(manifest["version"])
        if any(manifest[name] != marker[name] for name in ("workspace_id", "epoch_id", "baseline_digest")):
            raise LedgerError("manifest identity mismatch")
        if type(manifest["source_present"]) is not bool or type(manifest["source_length"]) is not int or manifest["source_length"] < 0:
            raise LedgerError("invalid source metadata")
        source = self._read(f"{base}/baseline.tasks.bytes") if manifest["source_present"] else None
        mode = manifest["source_mode"]
        if source is not None:
            if type(mode) is not int or not 0 <= mode <= 0o7777:
                raise LedgerError("invalid source mode")
        elif mode is not None:
            raise LedgerError("missing source has a mode")
        if (manifest["source_revision"] != storage.snapshot_revision(source)
                or manifest["source_length"] != len(source or b"") or manifest["source_digest"] != _digest(source or b"")):
            raise LedgerError("source backup mismatch")
        tasks = _tasks(_json(source) if source is not None else [])
        allowed = {"manifest.json": False, "frames": True}
        if source is not None:
            allowed["baseline.tasks.bytes"] = False
        if self._entries(base, allowed) != sorted(allowed):
            raise LedgerError("unexpected epoch entries")
        names = self._entries(f"{base}/frames", {})
        if not names or names != [f"{i:020d}.frame" for i in range(len(names))]:
            raise LedgerError("unexpected frame filename or sequence gap")
        first = self._read(f"{base}/frames/{names[0]}")
        digest = _digest(first)
        if digest != marker["baseline_digest"]:
            raise LedgerError("baseline digest mismatch")
        payload = _payload(first)
        _fields(payload, "version workspace_id epoch_id sequence previous_digest source_revision tasks revision")
        _version(payload["version"])
        rev = _revision(marker["workspace_id"], marker["epoch_id"], 0, tasks)
        if (type(payload["sequence"]) is not int or payload["sequence"] != 0 or payload["previous_digest"] is not None
                or _canonical(payload["tasks"]) != _canonical(tasks) or payload["revision"] != rev
                or payload["source_revision"] != manifest["source_revision"]
                or any(payload[k] != marker[k] for k in ("workspace_id", "epoch_id"))):
            raise LedgerError("invalid baseline")
        view = LedgerView(deepcopy(tasks), rev, marker["workspace_id"], marker["epoch_id"], 0, digest,
                          source if source is not None else storage.encode_json([]), {})
        for name in names[1:]:
            raw = self._read(f"{base}/frames/{name}")
            view = _apply(view, _payload(raw), _digest(raw))
        return view, manifest

    def read(self):
        raw = self._read(MARKER, missing=True)
        return None if raw is None else self._epoch(_json(raw))[0]

    def projection_health(self, view):
        try:
            observed = self._read("state/tasks.json", missing=True)
        except LedgerError as exc:
            return {"status": "unreadable", "error": str(exc),
                    "expected_revision": storage.snapshot_revision(view.projection_bytes), "observed_revision": None}
        return {"status": "current" if observed == view.projection_bytes else "missing" if observed is None else "diverged",
                "expected_revision": storage.snapshot_revision(view.projection_bytes),
                "observed_revision": storage.snapshot_revision(observed)}

    def lookup(self, view, key, binding):
        validate_key(key)
        _fields(binding, "version operation args expected_revision")
        _version(binding["version"])
        request_binding(binding["operation"], binding["args"], binding["expected_revision"])
        receipt = view.receipts.get(key)
        if receipt is None:
            return None
        if receipt["binding"] != binding:
            raise LedgerError("request key reused with changed binding")
        return deepcopy(receipt["result"])

    def import_tasks(self, expected_revision, lock):
        with self._lock(lock):
            task_attempts.guard_authority_change(self.workspace, lock)
            if self._read(MARKER, missing=True) is not None or self.has_pending_import():
                raise LedgerError("active or pending ledger already exists")
            validate_revision(expected_revision)
            source = self._read("state/tasks.json", missing=True)
            tasks = _tasks(_json(source) if source is not None else [])
            if expected_revision != storage.snapshot_revision(source):
                raise storage.RevisionConflict("snapshot revision mismatch")
            mode = stat.S_IMODE(self.path("state/tasks.json").stat().st_mode) if source is not None else None
            wid, eid = uuid.uuid4().hex, uuid.uuid4().hex
            base = f"{LEDGER}/{eid}"
            self._mkdir(LEDGER)
            self._mkdir(base)
            self._mkdir(f"{base}/frames")
            if source is not None:
                self._publish(f"{base}/baseline.tasks.bytes", source, mode)
            baseline = _envelope({"version": 1, "workspace_id": wid, "epoch_id": eid, "sequence": 0,
                                  "previous_digest": None, "source_revision": expected_revision, "tasks": tasks,
                                  "revision": _revision(wid, eid, 0, tasks)})
            self._publish(f"{base}/frames/{0:020d}.frame", baseline)
            manifest = {"version": 1, "workspace_id": wid, "epoch_id": eid, "source_revision": expected_revision,
                        "source_present": source is not None, "source_mode": mode, "source_length": len(source or b""),
                        "source_digest": _digest(source or b""), "baseline_digest": _digest(baseline)}
            raw = _canonical(manifest) + b"\n"
            self._publish(f"{base}/manifest.json", raw)
            marker = {k: manifest[k] for k in ("version", "workspace_id", "epoch_id", "baseline_digest")}
            marker["manifest_digest"] = _digest(raw)
            view, _ = self._epoch(marker)
            try:
                storage.sync_directory(self.workspace)
            except OSError as exc:
                raise LedgerError(f"cannot confirm import parent durability: {exc}") from exc
            self._publish(MARKER, _canonical(marker) + b"\n")
            return view

    def _current(self, view):
        actual = self.read()
        if actual is None or (actual.revision, actual.head_digest) != (view.revision, view.head_digest):
            raise LedgerError("ledger head changed")
        return actual

    def commit(self, view, key, binding, task, lock):
        with self._lock(lock):
            actual = self._current(view)
            if self.lookup(actual, key, binding) is not None:
                raise LedgerError("request key already committed")
            if binding["expected_revision"] not in (None, actual.revision):
                raise storage.RevisionConflict("ledger revision mismatch")
            if self.projection_health(actual)["status"] != "current":
                raise LedgerError("projection is not current; repair explicitly")
            task_attempts.guard_task_mutation(self.workspace, task["id"], lock)
            tasks = deepcopy(actual.tasks)
            if binding["operation"] == "add":
                tasks.append(deepcopy(task))
            else:
                tasks = [deepcopy(task) if item["id"] == binding["args"]["task_id"] else item for item in tasks]
            seq = actual.sequence + 1
            result = {"outcome": "committed", "mutation_id": uuid.uuid4().hex, "task": deepcopy(task),
                      "before_revision": actual.revision, "after_revision": _revision(actual.workspace_id, actual.epoch_id, seq, tasks),
                      "sequence": seq}
            payload = {"version": 1, "workspace_id": actual.workspace_id, "epoch_id": actual.epoch_id, "sequence": seq,
                       "previous_digest": actual.head_digest, "request_id": key, "binding": deepcopy(binding), "result": result}
            raw = _envelope(payload)
            candidate = _apply(actual, _payload(raw), _digest(raw))
            self._publish(f"{LEDGER}/{actual.epoch_id}/frames/{seq:020d}.frame", raw)
            return candidate

    def confirm_durability(self, view, lock):
        with self._lock(lock):
            actual = self._current(view)
            base = f"{LEDGER}/{actual.epoch_id}"
            paths = [MARKER, f"{base}/manifest.json"]
            manifest = _json(self._read(f"{base}/manifest.json"))
            if manifest["source_present"]:
                paths.append(f"{base}/baseline.tasks.bytes")
            paths.extend(f"{base}/frames/{i:020d}.frame" for i in range(actual.sequence + 1))
            try:
                for relative in paths:
                    fd = storage._open_regular(self.path(relative), os.O_RDONLY)
                    try:
                        os.fsync(fd)
                    finally:
                        os.close(fd)
                for relative in (f"{base}/frames", base, LEDGER, "state"):
                    storage.sync_directory(self.path(relative, directory=True))
                storage.sync_directory(self.workspace)
            except OSError as exc:
                raise PublicationUncertain(self.path(MARKER)) from exc

    def publish_projection(self, view, lock):
        with self._lock(lock):
            actual = self._current(view)
            storage.atomic_replace_bytes(self.path("state/tasks.json"), actual.projection_bytes)

    def repair_projection(self, lock):
        with self._lock(lock):
            view = self.read()
            if view is None:
                raise LedgerError("no active ledger")
            health = self.projection_health(view)
            if health["status"] == "current":
                return {"status": "current", "backup_path": None}
            observed = self._read("state/tasks.json", missing=True)
            recovery = "state/task-recovery"
            if not self.path(recovery, directory=True).exists():
                self._mkdir(recovery)
            base = f"{recovery}/{uuid.uuid4().hex}"
            self._mkdir(base)
            if observed is not None:
                self._publish(f"{base}/tasks.bytes", observed)
            self._publish(f"{base}/metadata.json", storage.encode_json({"version": 1, "missing": observed is None,
                                                                       "revision": storage.snapshot_revision(observed)}))
            storage.atomic_replace_bytes(self.path("state/tasks.json"), view.projection_bytes)
            return {"status": "repaired", "backup_path": str(self.path(base, directory=True))}

    def resume_import(self, expected_revision, lock):
        with self._lock(lock):
            task_attempts.guard_authority_change(self.workspace, lock)
            validate_revision(expected_revision)
            if not self.has_pending_import():
                raise LedgerError("no resumable pending import")
            root = self.path(LEDGER, directory=True)
            entries = list(root.iterdir())
            if len(entries) != 1 or not re.fullmatch(UUID, entries[0].name):
                raise LedgerError("pending import must have one complete epoch")
            base = f"{LEDGER}/{entries[0].name}"
            raw = self._read(f"{base}/manifest.json")
            manifest = _json(raw)
            try:
                marker = {k: manifest[k] for k in ("version", "workspace_id", "epoch_id", "baseline_digest")}
            except (KeyError, TypeError) as exc:
                raise LedgerError("incomplete manifest") from exc
            if marker["epoch_id"] != entries[0].name:
                raise LedgerError("pending epoch identity mismatch")
            marker["manifest_digest"] = _digest(raw)
            view, manifest = self._epoch(marker)
            source = self._read("state/tasks.json", missing=True)
            if view.sequence != 0 or expected_revision != manifest["source_revision"] or storage.snapshot_revision(source) != expected_revision:
                raise LedgerError("pending import source changed or contains mutations")
            # Sync all pending artifacts before publishing the sole authority marker.
            paths = [f"{base}/manifest.json", f"{base}/frames/{0:020d}.frame"]
            if manifest["source_present"]:
                paths.append(f"{base}/baseline.tasks.bytes")
                if stat.S_IMODE(self.path(paths[-1]).stat().st_mode) != manifest["source_mode"]:
                    raise LedgerError("pending source backup mode mismatch")
            try:
                for relative in paths:
                    fd = storage._open_regular(self.path(relative), os.O_RDONLY)
                    try:
                        os.fsync(fd)
                    finally:
                        os.close(fd)
                for relative in (f"{base}/frames", base, LEDGER, "state"):
                    storage.sync_directory(self.path(relative, directory=True))
                storage.sync_directory(self.workspace)
            except OSError as exc:
                raise LedgerError(f"cannot confirm pending import durability: {exc}") from exc
            self._publish(MARKER, _canonical(marker) + b"\n")
            return view

    def abort_import(self, lock):
        with self._lock(lock):
            if self._read(MARKER, missing=True) is not None:
                raise LedgerError("cannot abort active ledger")
            if not self.has_pending_import():
                raise LedgerError("no pending import")
            destination = self.path(f"state/task-ledger-aborted-{uuid.uuid4().hex}", directory=True)
            try:
                os.rename(self.path(LEDGER, directory=True), destination)
                storage.sync_directory(self.path("state", directory=True))
            except OSError as exc:
                raise LedgerError(f"abort import uncertain; inspect preserved artifacts: {exc}") from exc
            return str(destination)
