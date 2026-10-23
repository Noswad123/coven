"""Explicit preparation-only authority; no task writes or execution admission.

History is retained and cross-validated, not an append-only tamperproof log.
Static path checks share storage's threat model, not a race-proof sandbox.
"""

from copy import deepcopy
from dataclasses import dataclass
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import threading
import uuid

if __package__:
    from . import records, storage
else:
    import records
    import storage


class AttemptError(storage.StorageError):
    """Execution preparation authority cannot safely be used."""


DIR = "state/execution"
CONTROL = DIR + "/control.json"
DISABLED = "execution-disabled:v1"
MAX_CONTROL = 16 * 1024 * 1024
# A valid cancellation carries a <=16 KiB reason in binding/effect/result.
# Even worst-case six-byte JSON escaping in all three copies, bounded keys,
# timestamps, IDs, and generation growth fit well below this per-attempt reserve.
CANCELLATION_RESERVE = 512 * 1024
UUID = r"[0-9a-f]{32}"
HEX = r"[0-9a-f]{64}"
EXEC_REV = rf"execution-control-v1:{UUID}:[1-9][0-9]*:{HEX}"
ALL_EXEC_REV = rf"{EXEC_REV}|execution-control-v2:{UUID}:[1-9][0-9]*:{HEX}"
TASK_REV = rf"tasks-missing:v1|tasks-sha256:{HEX}|tasks-ledger:v1:{UUID}:{UUID}:(0|[1-9][0-9]*):{HEX}"
BLOCKERS = ["PREPARATION_ONLY", "NATIVE_ADMISSION_NOT_IMPLEMENTED",
            "TASK_EXECUTION_AUTHORIZATION_NOT_IMPLEMENTED"]


def _fail(message):
    raise AttemptError(message)


def _fields(value, names):
    if not isinstance(value, dict) or set(value) != set(names.split()):
        _fail("invalid execution authority fields")


def _match(value, pattern, name):
    if not isinstance(value, str) or not re.fullmatch(pattern, value):
        _fail("invalid " + name)
    return value


def _version(value):
    if type(value) is not int or value != 1:
        _fail("unsupported execution schema version")


def canonical(value):
    """Canonical strict JSON bytes, without normalization of text."""
    def check(item):
        if isinstance(item, dict):
            if any(not isinstance(k, str) for k in item):
                _fail("JSON object keys must be strings")
            for key, child in item.items():
                key.encode("utf-8")
                check(child)
        elif isinstance(item, list):
            for child in item:
                check(child)
        elif item is not None and type(item) not in (str, int, float, bool):
            _fail("invalid JSON value")
    try:
        check(value)
        return json.dumps(value, sort_keys=True, separators=(",", ":"),
                          ensure_ascii=False, allow_nan=False).encode("utf-8")
    except (ValueError, TypeError, UnicodeError, RecursionError) as exc:
        raise AttemptError(f"invalid strict JSON: {exc}") from exc


def _json(data):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                _fail("duplicate JSON key")
            result[key] = value
        return result
    try:
        value = json.loads(data.decode("utf-8"), object_pairs_hook=pairs,
                           parse_constant=storage._reject_constant, parse_float=storage._finite_float)
        canonical(value)
        return value
    except (ValueError, UnicodeError, RecursionError) as exc:
        raise AttemptError(f"invalid control JSON: {exc}") from exc


def _text(value, name, limit=16384):
    try:
        records.text(value, name, nonblank=True)
        if len(value.encode("utf-8")) > limit:
            _fail(name + " exceeds size limit")
    except ValueError as exc:
        raise AttemptError(str(exc)) from exc
    return value


def _id(value):
    try:
        return records.identifier(value, "task/owner", "task/owner")
    except ValueError as exc:
        raise AttemptError(str(exc)) from exc


def validate_intent(value):
    try:
        _fields(value, "schema_version kind task_id owner acceptance_criteria")
    except AttemptError as exc:
        raise AttemptError("invalid task-intent fields; expected only schema_version, kind, task_id, owner, acceptance_criteria") from exc
    _version(value["schema_version"])
    if value["kind"] != "task-intent":
        _fail("invalid intent kind")
    _id(value["task_id"])
    _id(value["owner"])
    criteria = value["acceptance_criteria"]
    if not isinstance(criteria, list) or not 1 <= len(criteria) <= 64:
        _fail("explicit acceptance criteria required (1..64)")
    seen = set()
    for criterion in criteria:
        try:
            _fields(criterion, "id text")
        except AttemptError as exc:
            raise AttemptError("invalid acceptance criterion fields; expected only id and text") from exc
        key = _match(criterion["id"], r"[A-Za-z0-9][A-Za-z0-9_-]{0,127}", "criterion ID")
        if key in seen:
            _fail("duplicate criterion ID")
        seen.add(key)
        _text(criterion["text"], "criterion text")
    if len(canonical(value)) > 1024 * 1024:
        _fail("intent exceeds 1 MiB")
    return deepcopy(value)


def task_snapshot_digest(task):
    return hashlib.sha256(b"coven-task-snapshot-v1\0" + canonical(task)).hexdigest()


def intent_digest(value):
    return hashlib.sha256(b"coven-task-attempt-intent-v1\0" + canonical(value)).hexdigest()


def _intent_payload(record):
    return {key: record[key] for key in ("execution_authority_id", "enrollment_id",
            "task_authority_identity", "task_source_revision", "complete_task_snapshot")} | {
                "explicit_intent": record["intent"]}


def _source(value, snapshot_id):
    _fields(value, "identity revision tasks roster")
    if len(canonical(value)) > MAX_CONTROL:
        _fail("task source exceeds execution authority size limit")
    identity = value["identity"]
    if not isinstance(identity, dict):
        _fail("invalid task authority identity")
    rev = _match(value["revision"], TASK_REV, "tasks revision")
    if identity.get("kind") == "snapshot-v1":
        _fields(identity, "kind source_id path")
        if identity != {"kind": "snapshot-v1", "source_id": snapshot_id, "path": "state/tasks.json"}:
            _fail("snapshot identity mismatch")
        try:
            storage.validate_revision(rev)
        except storage.StorageError as exc:
            raise AttemptError("snapshot identity/revision mismatch") from exc
        if rev == "tasks-missing:v1" and value["tasks"] != []:
            _fail("missing task source cannot contain tasks")
    elif identity.get("kind") == "task-ledger-v1":
        _fields(identity, "kind workspace_id epoch_id path")
        for key in ("workspace_id", "epoch_id"):
            _match(identity[key], UUID, key)
        prefix = f"tasks-ledger:v1:{identity['workspace_id']}:{identity['epoch_id']}:"
        if identity["path"] != "state/task-ledger" or not rev.startswith(prefix):
            _fail("ledger identity/revision mismatch")
        if rev.rsplit(":", 1)[1] != hashlib.sha256(canonical(value["tasks"])).hexdigest():
            _fail("ledger tasks digest mismatch")
    else:
        _fail("unsupported task authority identity")
    try:
        records.tasks(value["tasks"])
        records.agent_ids(value["roster"])
    except ValueError as exc:
        raise AttemptError(str(exc)) from exc
    return value


def request_binding(operation, parameters, expected_execution_revision, expected_tasks_revision):
    fields = {"enable": "", "enroll": "task_id rebind_source", "prepare": "task_id intent",
              "cancel": "attempt_id reason"}
    if not isinstance(operation, str) or operation not in fields:
        _fail("unsupported execution operation")
    _fields(parameters, fields[operation])
    _match(expected_execution_revision, rf"{DISABLED}|{EXEC_REV}", "execution revision")
    if operation in ("enroll", "prepare"):
        _match(expected_tasks_revision, TASK_REV, "required tasks revision")
    elif operation == "cancel" and expected_tasks_revision is not None:
        _fail("cancel must not depend on task source")
    elif expected_tasks_revision is not None:
        _match(expected_tasks_revision, TASK_REV, "tasks revision")
    if operation in ("enroll", "prepare"):
        _id(parameters["task_id"])
    if operation == "enroll" and type(parameters["rebind_source"]) is not bool:
        _fail("rebind_source must be boolean")
    if operation == "prepare":
        validate_intent(parameters["intent"])
        if parameters["intent"]["task_id"] != parameters["task_id"]:
            _fail("intent task mismatch")
    if operation == "cancel":
        _match(parameters["attempt_id"], UUID, "attempt ID")
        _text(parameters["reason"], "cancellation reason")
    return {"schema_version": 1, "operation": operation, "parameters": deepcopy(parameters),
            "expected_execution_revision": expected_execution_revision,
            "expected_tasks_revision": expected_tasks_revision}


@dataclass
class View:
    authority_id: str
    snapshot_source_id: str
    generation: int
    revision: str
    enrollments: dict
    attempts: dict
    receipts: dict
    extensions: dict


def _revision(control):
    return (f"execution-control-v1:{control['execution_authority_id']}:{control['generation']}:"
            + hashlib.sha256(canonical(control)).hexdigest())


def _apply(control, enrollments, attempts, binding, effect, generation):
    """Validate one immutable effect and derive its sole permissible receipt."""
    op, params = binding["operation"], binding["parameters"]
    authority = control["execution_authority_id"]
    result = {"operation": op, "generation": generation, "execution_authority_id": authority}
    if op == "enable":
        if generation != 1 or binding["expected_execution_revision"] != DISABLED:
            _fail("enable must be the first request")
        _fields(effect, "source")
        source = _source(effect["source"], control["snapshot_source_id"])
        if binding["expected_tasks_revision"] not in (None, source["revision"]):
            _fail("tasks revision conflict")
        return result | {"snapshot_source_id": control["snapshot_source_id"], "source": deepcopy(source)}
    if generation == 1:
        _fail("authority must start with enable")
    expected = binding["expected_execution_revision"]
    if not expected.startswith(f"execution-control-v1:{authority}:{generation - 1}:"):
        _fail("history execution revision mismatch")
    if op == "cancel":
        _fields(effect, "cancel_id attempt_id reason cancelled_at")
        _match(effect["cancel_id"], UUID, "cancel ID")
        _timestamp(effect["cancelled_at"])
        if effect["attempt_id"] != params["attempt_id"] or effect["reason"] != params["reason"]:
            _fail("cancel effect mismatch")
        attempt = attempts.get(params["attempt_id"])
        if attempt is None or attempt["state"] != "prepared":
            _fail("attempt is not prepared")
        if any(t["cancel_id"] == effect["cancel_id"] for a in attempts.values() for t in a["transitions"]):
            _fail("duplicate cancellation identity")
        attempt["state"] = "cancelled_before_submission"
        attempt["transitions"].append(deepcopy(effect))
        return result | {"attempt_id": params["attempt_id"], "state": attempt["state"],
                         "transition": deepcopy(effect)}
    _fields(effect, "source enrollment" if op == "enroll" else "source prepared")
    source = _source(effect["source"], control["snapshot_source_id"])
    if source["revision"] != binding["expected_tasks_revision"]:
        _fail("tasks revision conflict")
    task_id = params["task_id"]
    task = next((t for t in source["tasks"] if t["id"] == task_id), None)
    if task is None:
        _fail("task not found")
    previous = enrollments.get(task_id)
    prepared = any(a["prepared"]["task_id"] == task_id and a["state"] == "prepared" for a in attempts.values())
    if op == "enroll":
        if previous:
            if not params["rebind_source"] or previous["bindings"][-1]["task_authority_identity"] == source["identity"] or prepared:
                _fail("rebind requires changed source identity and no prepared attempt")
        elif params["rebind_source"]:
            _fail("cannot rebind an unenrolled task")
        record = effect["enrollment"]
        _fields(record, "enrollment_id task_id binding_number task_authority_identity task_source_revision complete_task_snapshot task_snapshot_digest enrolled_at")
        _match(record["enrollment_id"], UUID, "enrollment ID")
        _timestamp(record["enrolled_at"])
        if previous:
            if record["enrollment_id"] != previous["enrollment_id"]:
                _fail("rebind changed enrollment identity")
        elif any(e["enrollment_id"] == record["enrollment_id"] for e in enrollments.values()):
            _fail("duplicate enrollment identity")
        expected_record = {"enrollment_id": record["enrollment_id"], "task_id": task_id,
                           "binding_number": len(previous["bindings"]) + 1 if previous else 1,
                           "task_authority_identity": source["identity"], "task_source_revision": source["revision"],
                           "complete_task_snapshot": task, "task_snapshot_digest": task_snapshot_digest(task),
                           "enrolled_at": record["enrolled_at"]}
        if canonical(record) != canonical(expected_record):
            _fail("enrollment effect mismatch")
        if previous is None:
            enrollments[task_id] = {"enrollment_id": record["enrollment_id"], "task_id": task_id, "bindings": []}
        enrollments[task_id]["bindings"].append(deepcopy(record))
        return result | {"task_id": task_id, "enrollment_id": record["enrollment_id"], "enrollment": deepcopy(record)}
    if previous is None or previous["bindings"][-1]["task_authority_identity"] != source["identity"]:
        _fail("task must be enrolled against the current source identity")
    intent = params["intent"]
    if task.get("owner") != intent["owner"] or intent["owner"] not in source["roster"]:
        _fail("prepared owner must exactly match assigned task owner and canonical roster ID")
    if prepared:
        _fail("task already has a prepared attempt")
    record = effect["prepared"]
    _fields(record, "schema_version attempt_id task_id enrollment_id attempt_number previous_attempt_id execution_authority_id task_authority_identity task_source_revision complete_task_snapshot task_snapshot_digest intent intent_digest prepared_at")
    _version(record["schema_version"])
    _match(record["attempt_id"], UUID, "attempt ID")
    _timestamp(record["prepared_at"])
    if record["attempt_id"] in attempts:
        _fail("duplicate attempt identity")
    older = [a["prepared"] for a in attempts.values() if a["prepared"]["task_id"] == task_id]
    expected_record = {"schema_version": 1, "attempt_id": record["attempt_id"], "task_id": task_id,
                       "enrollment_id": previous["enrollment_id"], "attempt_number": len(older) + 1,
                       "previous_attempt_id": older[-1]["attempt_id"] if older else None,
                       "execution_authority_id": authority, "task_authority_identity": source["identity"],
                       "task_source_revision": source["revision"], "complete_task_snapshot": task,
                       "task_snapshot_digest": task_snapshot_digest(task), "intent": intent,
                       "prepared_at": record["prepared_at"]}
    expected_record["intent_digest"] = intent_digest(_intent_payload(expected_record))
    if canonical(record) != canonical(expected_record):
        _fail("prepared effect mismatch")
    attempts[record["attempt_id"]] = {"prepared": deepcopy(record), "state": "prepared", "transitions": []}
    return result | {"attempt_id": record["attempt_id"], "attempt": deepcopy(record), "state": "prepared"}


def _timestamp(value):
    _text(value, "timestamp", 128)
    try:
        parsed = datetime.fromisoformat(value)
        if parsed.tzinfo is None:
            _fail("timestamp requires timezone")
    except ValueError as exc:
        raise AttemptError("invalid timestamp") from exc


def _replay(control):
    _fields(control, "schema_version execution_authority_id snapshot_source_id generation history extensions")
    _version(control["schema_version"])
    for key in ("execution_authority_id", "snapshot_source_id"):
        _match(control[key], UUID, key)
    if (type(control["generation"]) is not int or control["generation"] < 1
            or not isinstance(control["history"], list) or control["generation"] != len(control["history"])
            or not isinstance(control["extensions"], dict)):
        _fail("invalid execution history/generation/extensions")
    canonical(control)
    enrollments, attempts, receipts = {}, {}, {}
    for generation, entry in enumerate(control["history"], 1):
        _fields(entry, "generation request_id binding effect result")
        if type(entry["generation"]) is not int or entry["generation"] != generation:
            _fail("invalid history generation")
        key = _match(entry["request_id"], r"[A-Za-z0-9][A-Za-z0-9._:-]{0,127}", "request key")
        if key in receipts:
            _fail("duplicate request key")
        binding = entry["binding"]
        _fields(binding, "schema_version operation parameters expected_execution_revision expected_tasks_revision")
        _version(binding["schema_version"])
        checked = request_binding(binding["operation"], binding["parameters"],
                                   binding["expected_execution_revision"], binding["expected_tasks_revision"])
        if generation > 1:
            previous = {**control, "generation": generation - 1, "history": control["history"][:generation - 1]}
            if checked["expected_execution_revision"] != _revision(previous):
                _fail("history execution revision digest mismatch")
        result = _apply(control, enrollments, attempts, checked, entry["effect"], generation)
        if canonical(result) != canonical(entry["result"]):
            _fail("receipt/effect mismatch")
        receipts[key] = deepcopy(entry)
    active = sum(attempt["state"] == "prepared" for attempt in attempts.values())
    if len(canonical(control)) + 1 + active * CANCELLATION_RESERVE > MAX_CONTROL:
        _fail("execution capacity must retain cancellation reserve for every prepared attempt")
    return View(control["execution_authority_id"], control["snapshot_source_id"], control["generation"],
                _revision(control), enrollments, attempts, receipts, deepcopy(control["extensions"]))


class Store:
    def __init__(self, workspace):
        try:
            self.root = Path(workspace).resolve(strict=True)
            if not self.root.is_dir():
                _fail("execution workspace must be an existing directory")
        except (OSError, ValueError, RuntimeError) as exc:
            raise AttemptError(f"cannot resolve execution workspace: {exc}") from exc

    def _lock(self, lock):
        if (not isinstance(lock, storage.WorkspaceLock) or not lock.active
                or lock.root != self.root or lock.owner_pid != os.getpid()
                or lock.owner_thread_id != threading.get_ident()
                or lock.lock_path != self.root / "state/.workspace.lock"):
            _fail("active workspace lock with matching root/pid/thread required")
        try:
            os.fstat(lock.fd)
        except OSError as exc:
            raise AttemptError("workspace lock descriptor is closed") from exc

    def _paths(self):
        for path in (self.root, self.root / "state", self.root / DIR):
            try:
                if not stat.S_ISDIR(path.lstat().st_mode):
                    _fail(f"unsafe execution directory: {path}")
            except FileNotFoundError:
                pass
            except OSError as exc:
                raise AttemptError(f"cannot inspect execution directory {path}: {exc}") from exc
        # Unlike exists(), lstat also catches dangling symlinks.
        path = self.root / CONTROL
        try:
            if not stat.S_ISREG(path.lstat().st_mode):
                _fail("unsafe execution control type")
        except FileNotFoundError:
            pass
        except OSError as exc:
            raise AttemptError(f"cannot inspect execution control: {exc}") from exc

    def _control(self):
        self._paths()
        path = self.root / CONTROL
        try:
            fd = storage._open_regular(path, os.O_RDONLY)
        except FileNotFoundError:
            if (self.root / DIR).exists():
                _fail("pending execution authority (missing control); inspect preserved evidence and restore or archive explicitly")
            return None
        except OSError as exc:
            raise AttemptError(f"cannot read execution authority: {exc}") from exc
        try:
            with os.fdopen(fd, "rb") as stream:
                data = stream.read(MAX_CONTROL + 1)
        except OSError as exc:
            raise AttemptError(f"cannot read execution control: {exc}") from exc
        if len(data) > MAX_CONTROL:
            _fail("control exceeds 16 MiB")
        control = _json(data)
        if not isinstance(control, dict):
            _fail("present execution control must be an object, never disabled")
        return control

    def read(self):
        control = self._control()
        if control is not None and control.get("schema_version") == 2:
            if __package__:
                from . import execution_v2
            else:
                import execution_v2
            return execution_v2.replay(control)
        return None if control is None else _replay(control)

    def inspect_control(self):
        try:
            view = self.read()
            if view is None:
                return {"status": "disabled", "revision": DISABLED}
            return {"status": "active", "revision": view.revision,
                    "execution_authority_id": view.authority_id, "generation": view.generation,
                    "schema_version": getattr(view, "schema_version", 1)}
        except (storage.StorageError, OSError) as exc:
            try:
                self._paths()
                pending = (self.root / DIR).is_dir() and not (self.root / CONTROL).exists()
            except (storage.StorageError, OSError):
                pending = False
            return {"status": "pending" if pending else "unavailable", "error": str(exc)}

    def archive_pending(self, lock):
        self._lock(lock)
        self._paths()
        try:
            (self.root / CONTROL).lstat()
        except FileNotFoundError:
            pass
        else:
            _fail("cannot archive a present control, even if malformed")
        if not (self.root / DIR).is_dir():
            _fail("no pending execution directory")
        target = self.root / ("state/execution-pending-archive-" + uuid.uuid4().hex)
        try:
            os.rename(self.root / DIR, target)
            storage.sync_directory(self.root / "state")
        except OSError as exc:
            raise AttemptError(f"pending archive durability uncertain: {exc}") from exc
        return str(target)

    def confirm_durability(self, view, lock):
        self._lock(lock)
        current = self.read()
        if current is None or current.revision != view.revision or current.authority_id != view.authority_id:
            _fail("execution authority changed before durability confirmation")
        try:
            fd = storage._open_regular(self.root / CONTROL, os.O_RDONLY)
            try:
                os.fsync(fd)
            finally:
                os.close(fd)
            for path in (self.root / DIR, self.root / "state", self.root):
                storage.sync_directory(path)
        except OSError as exc:
            raise storage.CommitUncertain(self.root / CONTROL) from exc

    def _envelope(self, view, result, replayed, uncertain=False):
        attempt = view.attempts.get(result.get("attempt_id"))
        delivery = {"outcome": "durability_uncertain" if uncertain else "committed",
                    "replayed": replayed, "current_revision": view.revision, "warnings": []}
        if uncertain:
            delivery["warnings"].append("Control published; durability confirmation required")
        if attempt:
            delivery["attempt_state"] = attempt["state"]
        return {"result": deepcopy(result), "delivery": delivery, "authorization": "none",
                 "dispatchable": False, "blockers": list(BLOCKERS)}

    def _verification_failed(self, view, result, replayed, error):
        # The effect/receipt is already known from a published or previously
        # read valid authority. Failure to verify CURRENT authority cannot turn
        # that into an ordinary pre-publication rejection or erase its result.
        envelope = self._envelope(view, result, replayed, True)
        delivery = envelope["delivery"]
        delivery["known_control_revision"] = view.revision
        delivery["current_revision"] = None
        if "attempt_state" in delivery:
            delivery["attempt_state"] = None
        delivery["warnings"] = [f"Known execution receipt retained; current authority verification/durability unconfirmed: {error}"]
        return envelope

    def mutate(self, operation, parameters, *, expected_execution_revision,
               expected_tasks_revision, request_id, source_loader, lock):
        self._lock(lock)
        key = _match(request_id, r"[A-Za-z0-9][A-Za-z0-9._:-]{0,127}", "request key")
        control = self._control()
        if control is not None and control.get("schema_version") == 2:
            if __package__:
                from . import execution_v2
            else:
                import execution_v2
            return execution_v2.mutate(self, operation, parameters,
                expected_execution_revision=expected_execution_revision, expected_tasks_revision=expected_tasks_revision,
                request_id=request_id, source_loader=source_loader, lock=lock)
        binding = request_binding(operation, parameters, expected_execution_revision, expected_tasks_revision)
        view = None if control is None else _replay(control)
        if view and key in view.receipts:
            receipt = view.receipts[key]
            if canonical(receipt["binding"]) != canonical(binding):
                _fail("request key binding conflict")
            try:
                self.confirm_durability(view, lock)
                uncertain = False
            except (OSError, storage.StorageError) as exc:
                return self._verification_failed(view, receipt["result"], True, exc)
            return self._envelope(view, receipt["result"], True, uncertain)
        current = DISABLED if view is None else view.revision
        if current != expected_execution_revision:
            _fail("execution revision conflict")
        if (operation == "enable") != (view is None):
            _fail("execution already enabled" if view else "execution is disabled")
        if control is None:
            control = {"schema_version": 1, "execution_authority_id": uuid.uuid4().hex,
                       "snapshot_source_id": uuid.uuid4().hex, "generation": 0, "history": [], "extensions": {}}
        now = datetime.now(timezone.utc).isoformat()
        effect = {}
        if operation != "cancel":
            if not callable(source_loader):
                _fail("source loader required")
            source = deepcopy(_source(source_loader(control["snapshot_source_id"]), control["snapshot_source_id"]))
            if expected_tasks_revision is not None and source["revision"] != expected_tasks_revision:
                _fail("tasks revision conflict")
            effect["source"] = source
        if operation in ("enroll", "prepare"):
            task_id = parameters["task_id"]
            task = next((t for t in source["tasks"] if t["id"] == task_id), None)
            if task is None:
                _fail("task not found")
            previous = view.enrollments.get(task_id)
            common = {"task_id": task_id, "task_authority_identity": source["identity"],
                      "task_source_revision": source["revision"], "complete_task_snapshot": task,
                      "task_snapshot_digest": task_snapshot_digest(task)}
            if operation == "enroll":
                effect["enrollment"] = common | {
                    "enrollment_id": previous["enrollment_id"] if previous else uuid.uuid4().hex,
                    "binding_number": len(previous["bindings"]) + 1 if previous else 1, "enrolled_at": now}
            else:
                older = [a["prepared"] for a in view.attempts.values() if a["prepared"]["task_id"] == task_id]
                record = common | {"schema_version": 1, "attempt_id": uuid.uuid4().hex,
                    "enrollment_id": previous["enrollment_id"] if previous else None,
                    "attempt_number": len(older) + 1, "previous_attempt_id": older[-1]["attempt_id"] if older else None,
                    "execution_authority_id": control["execution_authority_id"], "intent": deepcopy(parameters["intent"]),
                    "prepared_at": now}
                record["intent_digest"] = intent_digest(_intent_payload(record))
                effect["prepared"] = record
        elif operation == "cancel":
            effect = {"cancel_id": uuid.uuid4().hex, "attempt_id": parameters["attempt_id"],
                      "reason": parameters["reason"], "cancelled_at": now}
        generation = control["generation"] + 1
        result = _apply(control, deepcopy(view.enrollments) if view else {},
                        deepcopy(view.attempts) if view else {}, binding, effect, generation)
        control["generation"] = generation
        control["history"].append({"generation": generation, "request_id": key, "binding": binding,
                                   "effect": effect, "result": result})
        proposed = _replay(control)
        data = canonical(control) + b"\n"
        if len(data) > MAX_CONTROL:
            _fail("control exceeds 16 MiB; history is never pruned")
        if view is None:
            self._paths()
            try:
                (self.root / "state").mkdir(exist_ok=True)
                (self.root / DIR).mkdir()
                storage.sync_directory(self.root / "state")
                storage.sync_directory(self.root)
            except OSError as exc:
                raise AttemptError(f"activation not published; inspect pending directory: {exc}") from exc
        try:
            storage.atomic_replace_bytes(self.root / CONTROL, data)
        except storage.CommitUncertain as exc:
            # storage guarantees replacement occurred. Do not depend on another
            # disk read to return the exact effect/receipt we just published.
            return self._verification_failed(proposed, result, False, exc)
        try:
            self.confirm_durability(proposed, lock)
        except (OSError, storage.StorageError) as exc:
            return self._verification_failed(proposed, result, False, exc)
        return self._envelope(proposed, result, False)


def guard_task_mutation(workspace, task_id, lock):
    _id(task_id)
    _guard(workspace, lock, task_id)


def guard_authority_change(workspace, lock):
    _guard(workspace, lock, None)


def _guard(workspace, lock, task_id):
    store = Store(workspace)
    store._lock(lock)
    view = store.read()
    if view is None:
        return
    store.confirm_durability(view, lock)
    if any(a["state"] == "prepared" and (task_id is None or a["prepared"]["task_id"] == task_id)
           for a in view.attempts.values()):
        _fail("prepared attempt blocks task mutation/authority change; cancel before submission first")
