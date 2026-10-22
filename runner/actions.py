"""Opt-in, single-use runtime launch authority, with no runtime side effects.

Static path guards share storage's threat model, not a race-proof sandbox.
Approval binds reviewed bytes, not reviewer authentication or worker readiness.
"""

from copy import deepcopy
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import tempfile
import uuid

if __package__:
    from . import checkpoints, launch, storage
else:
    import checkpoints
    import launch
    import storage


CONTROL = "state/runtime-launch-control.json"
BUNDLES = "state/actions"
MAX_BYTES = 16 * 1024 * 1024
UUID = r"[0-9a-f]{32}"
HEX = r"[0-9a-f]{64}"
TERMINAL = ("transport_completed", "transport_failed_or_partial")


class ActionError(storage.StorageError):
    """Runtime launch authority cannot safely be used."""


def canonical(value) -> bytes:
    def check(item):
        if type(item) is dict:
            for key, child in item.items():
                if type(key) is not str:
                    raise ValueError("JSON object keys must be strings")
                check(child)
        elif type(item) is list:
            for child in item:
                check(child)
        elif item is not None and type(item) not in (str, int, float, bool):
            raise ValueError("unsupported JSON value")

    try:
        check(value)
        data = json.dumps(value, sort_keys=True, separators=(",", ":"),
                          ensure_ascii=False, allow_nan=False).encode("utf-8")
        if len(data) > MAX_BYTES:
            raise ValueError("authority JSON exceeds 16 MiB")
        return data
    except (TypeError, ValueError, UnicodeError, RecursionError) as exc:
        raise ActionError(f"invalid bounded strict JSON: {exc}") from exc


def digest(bundle) -> str:
    return "runtime-launch-manifest-v1:" + hashlib.sha256(canonical(bundle)).hexdigest()


def control_revision(control) -> str:
    return "runtime-launch-control-v1:" + hashlib.sha256(canonical(control)).hexdigest()


def _token(value, pattern, name):
    if not isinstance(value, str) or not re.fullmatch(pattern, value):
        raise ActionError(f"invalid {name}")
    return value


def validate_action_id(value):
    return _token(value, UUID, "action ID (expected lowercase UUID hex)")


def _fields(value, names):
    if not isinstance(value, dict) or set(value) != set(names.split()):
        raise ActionError("invalid authority fields")


def _version(value):
    if type(value) is not int or value != 1:
        raise ActionError("unsupported authority version")


def _location(value):
    _fields(value, "root device inode")
    root = value["root"]
    if (not isinstance(root, str) or not root.startswith("/") or root.startswith("//") or "\0" in root
            or str(Path(root)) != root or ".." in Path(root).parts):
        raise ActionError("invalid canonical workspace root")
    for name in ("device", "inode"):
        if type(value[name]) is not int or value[name] < 0:
            raise ActionError("invalid workspace location identity")


def _plan(plan, location, workspace_id, action_id):
    try:
        frozen = launch.validate_plan(plan)
    except (ValueError, TypeError, KeyError, RecursionError) as exc:
        raise ActionError(f"invalid frozen launch plan: {exc}") from exc
    if (frozen["root"] != location["root"]
            or frozen["namespace"] != f"coven-{workspace_id}-{action_id}"):
        raise ActionError("plan root or namespace differs from action identity")


def _bundle(bundle):
    canonical(bundle)
    _fields(bundle, "version action action_id workspace_id epoch_id location control_revision inputs plan")
    _version(bundle["version"])
    if bundle["action"] != "runtime_launch":
        raise ActionError("invalid action")
    for name in ("action_id", "workspace_id", "epoch_id"):
        validate_action_id(bundle[name])
    _location(bundle["location"])
    _token(bundle["control_revision"], rf"runtime-launch-control-v1:{HEX}", "control revision")
    if not isinstance(bundle["inputs"], dict):
        raise ActionError("inputs must be a frozen inventory object")
    _plan(bundle["plan"], bundle["location"], bundle["workspace_id"], bundle["action_id"])


def checkpoint_id(bundle) -> str:
    return "runtime-launch-" + validate_action_id(bundle["action_id"])


def checkpoint_bytes(bundle) -> bytes:
    _bundle(bundle)
    plan = bundle["plan"]
    # JSON quoting keeps arbitrary commands/paths from becoming Markdown machinery.
    summary = {"action": bundle["action"], "action_id": bundle["action_id"],
               "workspace_id": bundle["workspace_id"], "epoch_id": bundle["epoch_id"],
               "manifest_digest": digest(bundle), "location": bundle["location"],
               "runtime": plan["multiplexer"], "namespace": plan["namespace"],
               "roster": plan["agents"], "commands": plan["steps"]}
    encoded = json.dumps(summary, sort_keys=True, ensure_ascii=True, indent=2)
    encoded = encoded.replace("<", "\\u003c").replace(">", "\\u003e").replace("`", "\\u0060")
    return ("# Runtime Launch Review\nStatus: pending\n\n"
            "## Covered Action\n"
            "WARNING: This authorizes one unrestricted COLD runtime launch, not a read-only "
            "sandbox, task dispatch, native worker acceptance, or readiness guarantee. "
            "Other authorized runtimes may still be running.\n\n"
            "```json\n" + encoded + "\n```\n\n"
            f"Inspect the immutable manifest with `coven action inspect {bundle['action_id']}` "
            "before approving. Sources are provenance, not authentication.\n\n"
            "## User Decision\n- [ ] Approved\n- [ ] Rejected / needs changes\n").encode("utf-8")


def _report(report, operation="runtime_launch"):
    canonical(report)
    fields = "operation status completed_steps handles diagnostics outcome_unknown"
    if operation == "runtime_stop":
        fields += " stop_command_attempted stop_command_succeeded"
    _fields(report, fields)
    if report["operation"] != operation or report["status"] not in TERMINAL:
        raise ActionError("invalid transport report operation or status")
    if type(report["completed_steps"]) is not int or report["completed_steps"] < 0:
        raise ActionError("invalid completed step count")
    if type(report["outcome_unknown"]) is not bool:
        raise ActionError("invalid report uncertainty")
    if operation == "runtime_stop":
        if any(type(report[field]) is not bool for field in ("stop_command_attempted", "stop_command_succeeded")):
            raise ActionError("invalid stop report")
        if report["stop_command_succeeded"] and not report["stop_command_attempted"]:
            raise ActionError("stop success without an attempt")
    if not isinstance(report["handles"], dict) or not isinstance(report["diagnostics"], list):
        raise ActionError("invalid report handles or diagnostics")
    for key, value in report["handles"].items():
        _token(key, r"[A-Za-z0-9_-]{1,128}", "handle name")
        _token(value, r"[A-Za-z0-9_-]{1,128}", "native handle")
    for item in report["diagnostics"]:
        _fields(item, "step error message")
        if item["step"] is not None and (type(item["step"]) is not int or item["step"] < 0):
            raise ActionError("invalid diagnostic step")
        if not isinstance(item["error"], str) or not isinstance(item["message"], str):
            raise ActionError("invalid diagnostic text")


def _refs(manifest, checkpoint, decision):
    _token(manifest, rf"runtime-launch-manifest-v1:{HEX}", "manifest digest")
    _token(checkpoint, rf"checkpoint-snapshot-v1:{HEX}", "checkpoint revision")
    validate_action_id(decision)


class ActionStore:
    def __init__(self, workspace):
        try:
            self.workspace = Path(workspace).resolve(strict=True)
        except (OSError, ValueError, RuntimeError) as exc:
            raise ActionError(f"cannot resolve action workspace: {exc}") from exc
        if not self.workspace.is_dir():
            raise ActionError("workspace must be a directory")

    def path(self, relative, directory=False):
        try:
            relative = Path(relative)
        except (TypeError, ValueError) as exc:
            raise ActionError("unsafe action path") from exc
        if relative.is_absolute() or not relative.parts or ".." in relative.parts:
            raise ActionError("unsafe action path")
        path = self.workspace
        try:
            for index, part in enumerate(relative.parts):
                path = path / part
                try:
                    mode = path.lstat().st_mode
                except FileNotFoundError:
                    continue
                if stat.S_ISLNK(mode):
                    raise ActionError(f"{path}: unsafe path type or symlink; symlinks are not supported")
                want_dir = directory or index < len(relative.parts) - 1
                if not (stat.S_ISDIR(mode) if want_dir else stat.S_ISREG(mode)):
                    raise ActionError(f"{path}: unsafe path type or symlink")
        except (OSError, ValueError) as exc:
            raise ActionError(f"cannot inspect action path: {exc}") from exc
        return path

    def _lock(self, lock):
        if lock is None:
            raise ActionError("explicit active workspace lock required")
        return storage.workspace_lock(self.workspace, self.path("state/.workspace.lock"), held=lock)

    def location(self) -> dict:
        try:
            info = self.workspace.stat()
            return {"root": str(self.workspace), "device": info.st_dev, "inode": info.st_ino}
        except OSError as exc:
            raise ActionError(f"cannot inspect workspace identity: {exc}") from exc

    def _read(self, relative, missing=False):
        path = self.path(relative)
        try:
            with os.fdopen(storage._open_regular(path, os.O_RDONLY), "rb") as stream:
                data = stream.read(MAX_BYTES + 1)
            if len(data) > MAX_BYTES:
                raise ActionError("authority file exceeds 16 MiB")
            return data
        except FileNotFoundError as exc:
            if missing:
                return None
            raise ActionError(f"{path}: required authority file missing") from exc
        except OSError as exc:
            raise ActionError(f"{path}: cannot read authority: {exc}") from exc

    def _json(self, relative, missing=False):
        def pairs(items):
            result = {}
            for key, value in items:
                if key in result:
                    raise ValueError("duplicate JSON key")
                result[key] = value
            return result

        data = self._read(relative, missing)
        if data is None:
            return None
        try:
            return json.loads(data, object_pairs_hook=pairs, parse_constant=storage._reject_constant,
                              parse_float=storage._finite_float)
        except (ValueError, UnicodeError, RecursionError) as exc:
            raise ActionError(f"invalid strict authority JSON: {exc}") from exc

    def read_control(self) -> dict | None:
        control = self._json(CONTROL, missing=True)
        if control is None:
            # A JSON null is malformed authority, not absence.
            if self.path(CONTROL).exists():
                raise ActionError("invalid null control")
            if self.path(BUNDLES, directory=True).exists():
                raise ActionError("orphan actions directory without enabled control")
            return None
        canonical(control)
        _fields(control, "version action workspace_id epoch_id location head claims")
        _version(control["version"])
        if control["action"] != "runtime_launch":
            raise ActionError("invalid control action")
        validate_action_id(control["workspace_id"])
        validate_action_id(control["epoch_id"])
        _location(control["location"])
        if control["location"] != self.location():
            raise ActionError("workspace location changed; explicit future reactivation required")
        if not isinstance(control["claims"], dict):
            raise ActionError("invalid control claims")
        head = control["head"]
        if head is not None:
            validate_action_id(head)
            if head not in control["claims"]:
                raise ActionError("control head is missing its claim")
        elif control["claims"]:
            raise ActionError("nonempty claims require a head")
        claim_ids = set()
        for action_id, claim in control["claims"].items():
            validate_action_id(action_id)
            _fields(claim, "claim_id action_id manifest_digest checkpoint_revision decision_id scope_revision plan status report operator_acknowledged")
            validate_action_id(claim["claim_id"])
            if claim["claim_id"] in claim_ids or claim["action_id"] != action_id:
                raise ActionError("conflicting claim identity")
            claim_ids.add(claim["claim_id"])
            _refs(claim["manifest_digest"], claim["checkpoint_revision"], claim["decision_id"])
            _token(claim["scope_revision"], rf"checkpoint-scope-v1:{HEX}", "scope revision")
            _plan(claim["plan"], control["location"], control["workspace_id"], action_id)
            status, ack = claim["status"], claim["operator_acknowledged"]
            if type(ack) is not bool or status not in ("claimed", "outcome_unknown") + TERMINAL:
                raise ActionError("invalid claim status or acknowledgement")
            if status in TERMINAL:
                _report(claim["report"])
                if claim["report"]["status"] != status:
                    raise ActionError("claim outcome differs from report")
            elif claim["report"] is not None:
                raise ActionError("nonterminal claim cannot have a report")
            if status == "claimed" and (ack or head != action_id):
                raise ActionError("unresolved claim must be the unacknowledged head")
            if status == "outcome_unknown" and not ack:
                raise ActionError("unknown outcome requires explicit operator acknowledgement")
        return control

    def _control(self):
        control = self.read_control()
        if control is None:
            raise ActionError("runtime launch authority is not enabled")
        return control

    def _confirm(self):
        path = self.path(CONTROL)
        try:
            with os.fdopen(storage._open_regular(path, os.O_RDONLY), "rb") as stream:
                os.fsync(stream.fileno())
            storage.sync_directory(self.path("state", directory=True))
            storage.sync_directory(self.workspace)
        except OSError as exc:
            raise storage.CommitUncertain(path) from exc

    def _write(self, control):
        storage.atomic_replace_bytes(self.path(CONTROL), canonical(control))

    def enable(self, lock, *, gate=None):
        with self._lock(lock):
            control = self.read_control()
            if control is None:
                if gate is None:
                    raise ActionError("first activation requires an exclusive transport gate acquired before the workspace lock")
                with storage.transport_gate(self.workspace, self.path("state/.runtime-transport.lock"), exclusive=True, held=gate):
                    control = {"version": 1, "action": "runtime_launch", "workspace_id": uuid.uuid4().hex,
                               "epoch_id": uuid.uuid4().hex, "location": self.location(),
                               "head": None, "claims": {}}
                    self._write(control)
            self._confirm()
            return deepcopy(control)

    def _available(self, control):
        if control["head"] is not None and control["claims"][control["head"]]["status"] == "claimed":
            raise ActionError("unresolved claimed head; inspect and explicitly acknowledge unknown outcome")

    def _current(self, bundle, control):
        if any(bundle[name] != control[name] for name in ("workspace_id", "epoch_id", "location")):
            raise ActionError("bundle activation identity differs from control")
        if bundle["control_revision"] != control_revision(control):
            raise ActionError("bundle control revision is stale")

    def make_bundle(self, inputs, plan, action_id, lock):
        with self._lock(lock):
            control = self._control()
            self._available(control)
            validate_action_id(action_id)
            if action_id in control["claims"]:
                raise ActionError("action ID already consumed")
            bundle = {"version": 1, "action": "runtime_launch", "action_id": action_id,
                      "workspace_id": control["workspace_id"], "epoch_id": control["epoch_id"],
                      "location": deepcopy(control["location"]), "control_revision": control_revision(control),
                      "inputs": deepcopy(inputs), "plan": deepcopy(plan)}
            _bundle(bundle)
            return bundle

    def publish_bundle(self, bundle, lock):
        with self._lock(lock):
            _bundle(bundle)
            control = self._control()
            self._available(control)
            self._current(bundle, control)
            if bundle["action_id"] in control["claims"]:
                raise ActionError("action ID already consumed")
            directory = self.path(BUNDLES, directory=True)
            temp, published = None, False
            path = self.path(f"{BUNDLES}/{bundle['action_id']}.json")
            try:
                directory.mkdir(exist_ok=True)
                storage.sync_directory(directory.parent)
                fd, name = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".tmp", dir=directory)
                temp = Path(name)
                with os.fdopen(fd, "wb") as stream:
                    stream.write(canonical(bundle))
                    stream.flush()
                    os.fsync(stream.fileno())
                self.path(f"{BUNDLES}/{bundle['action_id']}.json")
                os.link(temp, path)
                published = True
                storage.sync_directory(directory)
            except OSError as exc:
                if published:
                    raise storage.CommitUncertain(path) from exc
                raise ActionError(f"immutable bundle publication failed: {exc}") from exc
            finally:
                if temp is not None:
                    try:
                        temp.unlink(missing_ok=True)
                    except OSError:
                        pass
            return digest(bundle)

    def read_bundle(self, action_id):
        validate_action_id(action_id)
        bundle = self._json(f"{BUNDLES}/{action_id}.json")
        _bundle(bundle)
        if bundle["action_id"] != action_id:
            raise ActionError("bundle filename and action ID differ")
        return bundle

    def claim(self, action_id, expected_manifest, expected_checkpoint, expected_decision,
              current_inputs, lock):
        validate_action_id(action_id)
        _refs(expected_manifest, expected_checkpoint, expected_decision)
        with self._lock(lock):
            control = self._control()
            old = control["claims"].get(action_id)
            if old is not None:
                if (old["manifest_digest"], old["checkpoint_revision"], old["decision_id"]) != (
                        expected_manifest, expected_checkpoint, expected_decision):
                    raise ActionError("consumed action references conflict")
                self._confirm()
                return deepcopy(old), True
            self._available(control)
            bundle = self.read_bundle(action_id)
            if digest(bundle) != expected_manifest:
                raise ActionError("manifest digest mismatch")
            self._current(bundle, control)
            if canonical(current_inputs) != canonical(bundle["inputs"]):
                raise ActionError("captured inputs changed")
            cp_id = checkpoint_id(bundle)
            view = checkpoints.inspect(cp_id, self._read(f"checkpoints/{cp_id}.md"))
            scope = checkpoints.inspect(cp_id, checkpoint_bytes(bundle)).scope_revision
            if (view.binding != "bound" or view.effective_status != "approved"
                    or view.revision != expected_checkpoint or view.decision is None
                    or view.decision["decision_id"] != expected_decision or view.scope_revision != scope):
                raise ActionError("checkpoint is not the exact bound approval for this manifest")
            claim = {"claim_id": uuid.uuid4().hex, "action_id": action_id,
                     "manifest_digest": expected_manifest, "checkpoint_revision": expected_checkpoint,
                     "decision_id": expected_decision, "scope_revision": scope,
                     "plan": deepcopy(bundle["plan"]), "status": "claimed", "report": None,
                     "operator_acknowledged": False}
            control["claims"][action_id] = claim
            control["head"] = action_id
            self._write(control)
            return deepcopy(claim), False

    def record_outcome(self, action_id, claim_id, report, lock):
        validate_action_id(action_id)
        validate_action_id(claim_id)
        _report(report)
        with self._lock(lock):
            control = self._control()
            claim = control["claims"].get(action_id)
            if claim is None or claim["claim_id"] != claim_id:
                raise ActionError("claim identity mismatch")
            if claim["status"] in TERMINAL:
                if canonical(claim["report"]) != canonical(report):
                    raise ActionError("terminal outcome cannot be changed")
                self._confirm()
            else:
                claim["report"] = deepcopy(report)
                claim["status"] = report["status"]
                self._write(control)
            return deepcopy(claim)

    def acknowledge_unknown(self, action_id, claim_id, lock):
        validate_action_id(action_id)
        validate_action_id(claim_id)
        with self._lock(lock):
            control = self._control()
            claim = control["claims"].get(action_id)
            if claim is None or claim["claim_id"] != claim_id or claim["status"] != "claimed":
                raise ActionError("only the identified unresolved claim can be acknowledged")
            claim.update(status="outcome_unknown", operator_acknowledged=True)
            self._write(control)
            return deepcopy(claim)

    def record_stop(self, action_id, report, lock):
        validate_action_id(action_id)
        _report(report, "runtime_stop")
        with self._lock(lock):
            claim = self._control()["claims"].get(action_id)
            if claim is None:
                raise ActionError("action has no consumed claim")
            return deepcopy(claim)

    def get_claim(self, action_id):
        validate_action_id(action_id)
        control = self.read_control()
        return None if control is None else deepcopy(control["claims"].get(action_id))

    def inspect(self, action_id):
        validate_action_id(action_id)
        control = self.read_control()
        result = {"control": deepcopy(control), "workspace_id": None, "epoch_id": None,
                  "location": self.location(), "control_revision": None, "bundle": None,
                  "bundle_error": None, "manifest_digest": None,
                  "checkpoint_id": "runtime-launch-" + action_id,
                  "claim": None if control is None else deepcopy(control["claims"].get(action_id))}
        if control is not None:
            result.update(workspace_id=control["workspace_id"], epoch_id=control["epoch_id"],
                          control_revision=control_revision(control))
        try:
            result["bundle"] = self.read_bundle(action_id)
            result["manifest_digest"] = digest(result["bundle"])
        except storage.StorageError as exc:
            result["bundle_error"] = str(exc)
        return result
