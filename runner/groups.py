"""Explicit participant associations, not native identity or execution authority.

Bindings are caller-supplied observations, not authenticated facts. Conditional
retries require the current content token: there are no durable request receipts
or exactly-once promises. Reservation retries burn new numbers, even after a
crash; callers must not send when delivery is durability_uncertain.
"""

from copy import deepcopy
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import uuid

if __package__:
    from . import storage
else:
    import storage


UNINITIALIZED = "groups-uninitialized:v1"
MAX_RECORD_BYTES = 1024 * 1024
_U64 = 2**64 - 1
_DOMAIN = b"coven.groups.content:v1\0"


class GroupError(storage.StorageError):
    """Group authority or a requested association is invalid."""


def _require(condition, message="malformed group authority"):
    if not condition:
        raise GroupError(message)


def _fields(value, fields):
    _require(isinstance(value, dict) and set(value) == set(fields.split()))


def _string(value, limit=4096, *, nonempty=True):
    _require(isinstance(value, str))
    try:
        _require((bool(value) or not nonempty) and len(value.encode("utf-8")) <= limit
                 and "\0" not in value)
    except UnicodeError as exc:
        raise GroupError("invalid UTF-8 string") from exc


def _selector(value):
    _require(isinstance(value, str) and re.fullmatch(r"[a-z0-9][a-z0-9_-]{0,63}", value),
             "invalid human selector")


def _display(value):
    _string(value, 64)
    _require(value.isprintable() and bool(value.strip()), "invalid display name or role")


def _uint(value):
    _require(type(value) is int and 0 <= value <= _U64, "unsigned 64-bit integer required")


def _uuid(value):
    _string(value, 36)
    try:
        _require(str(uuid.UUID(value)) == value, "canonical UUID required")
    except ValueError as exc:
        raise GroupError("canonical UUID required") from exc


def _endpoint(value):
    _fields(value, "path device inode")
    path = value["path"]
    _string(path, 8192)
    _require(os.path.isabs(path) and os.path.normpath(path) == path
             and not path.startswith("//"), "endpoint path must be resolved and absolute")
    for key in ("device", "inode"):
        _require(type(value[key]) is int and value[key] >= 0, "invalid endpoint stat identity")


def _endpoint_key(endpoint):
    return endpoint["path"], endpoint["device"], endpoint["inode"]


def _binding(value):
    _fields(value, "presentation evidence")
    presentation, evidence = value["presentation"], value["evidence"]
    _fields(presentation, "adapter endpoint terminal_id pane_id_at_binding")
    _require(presentation["adapter"] == "kitsune")
    _endpoint(presentation["endpoint"])
    _string(presentation["terminal_id"])
    pane = presentation["pane_id_at_binding"]
    _string(pane)
    _require(pane.isprintable() and not any(c.isspace() for c in pane), "noncanonical pane ID")
    _require(isinstance(evidence, dict))
    process = evidence.get("provenance") == "kitsune.pane.process_info"
    _fields(evidence, "provenance observed_at agent_kind_hint native_reference native_binding"
            + (" process_evidence" if process else ""))
    _require(process or evidence["provenance"] == "kitsune.session.snapshot")
    if process:
        _require(evidence["agent_kind_hint"] == "djinn"
                 and evidence["native_reference"] is None
                 and evidence["native_binding"] == "unestablished")
        details = evidence["process_evidence"]
        _fields(details, "interpreter matches")
        _require(details["interpreter"] == "coven.kitsune.djinn-process.v1")
        matches = details["matches"]
        _require(isinstance(matches, list) and 1 <= len(matches) <= 1024)
        pids, order = set(), []
        for match in matches:
            _fields(match, "pid executable_family rule")
            pid, family, rule = match["pid"], match["executable_family"], match["rule"]
            _require(type(pid) is int and 1 <= pid <= 2**32 - 1 and pid not in pids,
                     "unique positive unsigned 32-bit PID required")
            _require(isinstance(family, str) and family in {"djinn", "djinn-ui"})
            _require(rule == "exact_posix_argv0_basename")
            pids.add(pid)
            order.append((pid, family, rule))
        _require(order == sorted(order), "canonical process match order required")
    stamp = evidence["observed_at"]
    _string(stamp, 64)
    try:
        observed = datetime.fromisoformat(stamp)
        _require("T" in stamp and observed.tzinfo is not None
                 and observed.utcoffset().total_seconds() == 0, "UTC ISO timestamp required")
    except ValueError as exc:
        raise GroupError("UTC ISO timestamp required") from exc
    if evidence["agent_kind_hint"] is not None:
        _string(evidence["agent_kind_hint"], nonempty=False)
    native = evidence["native_reference"]
    if native is not None:
        _fields(native, "source agent kind value")
        _require(isinstance(native["kind"], str) and native["kind"] in {"id", "path"})
        for key in ("source", "agent", "value"):
            _string(native[key], 8192 if key == "value" and native["kind"] == "path" else 4096,
                    nonempty=False)
    _require(evidence["native_binding"] == (
        "unestablished" if native is None else "observed_reference"))


def _stable(binding):
    result = deepcopy(binding)
    del result["evidence"]["observed_at"]
    del result["presentation"]["pane_id_at_binding"]
    return result


def _source(source):
    _require(isinstance(source, str) and re.fullmatch(r"[A-Za-z0-9_.-]{1,64}", source),
             "invalid publication source")


def _validate(state):
    _fields(state, "schema_version workspace_id content_revision groups publication_counters")
    _require(type(state["schema_version"]) is int and state["schema_version"] in (1, 2))
    _uuid(state["workspace_id"])
    _uint(state["content_revision"])
    groups, counters = state["groups"], state["publication_counters"]
    _require(isinstance(groups, list) and len(groups) <= 16)
    _require(isinstance(counters, list) and len(counters) <= 8)
    identities, selectors, associations = set(), set(), set()
    participants = 0
    for group in groups:
        _fields(group, "group_id key name members")
        _uuid(group["group_id"])
        _selector(group["key"])
        _display(group["name"])
        _require(group["group_id"] not in identities and group["key"] not in selectors,
                 "duplicate group identity or selector")
        identities.add(group["group_id"])
        selectors.add(group["key"])
        members = group["members"]
        _require(isinstance(members, list) and len(members) <= 16)
        keys = set()
        for member in members:
            _fields(member, "participant_id key role binding")
            _uuid(member["participant_id"])
            _selector(member["key"])
            _display(member["role"])
            _require(len((group["name"] + " \u00b7 " + member["role"]).encode("utf-8")) <= 256)
            _binding(member["binding"])
            _require(state["schema_version"] == 2 or member["binding"]["evidence"]["provenance"]
                     == "kitsune.session.snapshot", "process bindings require schema 2")
            presentation = member["binding"]["presentation"]
            association = (*_endpoint_key(presentation["endpoint"]), presentation["terminal_id"])
            _require(member["participant_id"] not in identities and member["key"] not in keys,
                     "duplicate participant identity or selector")
            _require(association not in associations, "duplicate endpoint/terminal association")
            identities.add(member["participant_id"])
            keys.add(member["key"])
            associations.add(association)
            participants += 1
    _require(participants <= 64, "active participant limit exceeded")
    indices = set()
    for counter in counters:
        _fields(counter, "endpoint source epoch watermarks")
        _endpoint(counter["endpoint"])
        _source(counter["source"])
        _string(counter["epoch"])
        index = (*_endpoint_key(counter["endpoint"]), counter["source"])
        _require(index not in indices, "duplicate publication counter")
        indices.add(index)
        watermarks = counter["watermarks"]
        _require(isinstance(watermarks, dict) and len(watermarks) <= 64)
        for terminal, number in watermarks.items():
            _string(terminal)
            _uint(number)
            _require(number > 0)
    return state


def _workspace(workspace):
    try:
        root = Path(workspace).resolve(strict=True)
        _require(root.is_dir(), "workspace must exist")
        storage._check_path(root / "coven.json")
        _require(stat.S_ISREG((root / "coven.json").lstat().st_mode), "regular coven.json required")
        _require(stat.S_ISDIR((root / "state").lstat().st_mode), "nonsymlink state directory required")
        storage._check_path(root / "state" / "groups.json")
        return root
    except (OSError, ValueError, RuntimeError) as exc:
        raise GroupError("invalid groups workspace structure") from exc


def _pairs(pairs):
    result = {}
    for key, value in pairs:
        _require(key not in result, "duplicate JSON key")
        result[key] = value
    return result


def _decode(data):
    text = data.decode("utf-8")
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
            _require(depth <= 16, "JSON nesting limit exceeded")
        elif char in "]}":
            depth -= 1
    return json.loads(text, object_pairs_hook=_pairs, parse_constant=storage._reject_constant,
                      parse_float=storage._finite_float)


def _read(root):
    path = root / "state" / "groups.json"
    try:
        storage._check_path(path)
        try:
            fd = storage._open_regular(path, os.O_RDONLY)
        except FileNotFoundError:
            return None, None
        with os.fdopen(fd, "rb") as stream:
            data = stream.read(MAX_RECORD_BYTES + 1)
        _require(len(data) <= MAX_RECORD_BYTES, "groups authority exceeds size limit")
        return _validate(_decode(data)), data
    except (OSError, ValueError, RecursionError, OverflowError) as exc:
        raise GroupError("cannot read groups authority") from exc


def _revision(state):
    if state is None:
        return UNINITIALIZED
    content = {key: state[key] for key in ("workspace_id", "content_revision", "groups")}
    data = json.dumps(content, sort_keys=True, ensure_ascii=False, allow_nan=False,
                      separators=(",", ":")).encode("utf-8")
    digest = hashlib.sha256(_DOMAIN + data).hexdigest()
    return f"groups-v1:{state['workspace_id']}:{state['content_revision']}:{digest}"


def read_groups(workspace):
    """Read without creating files, acquiring locks, or confirming durability."""
    state, _ = _read(_workspace(workspace))
    return {"initialized": state is not None, "revision": _revision(state), "state": state}


def _cas(state, expected):
    # Equality against a freshly validated token also rejects malformed tokens.
    _require(expected is None or (isinstance(expected, str) and expected == _revision(state)),
             "groups revision conflict")


def _finish(root, state, data, changed):
    path = root / "state" / "groups.json"
    if changed:
        _validate(state)
        data = storage.encode_json(state)
        _require(len(data) <= MAX_RECORD_BYTES, "groups authority exceeds size limit")
        try:
            storage.atomic_replace_bytes(path, data)
        except storage.CommitUncertain:
            return {"outcome": "durability_uncertain"}
    try:
        current, current_data = _read(root)
        _require(current == state and current_data == data, "groups changed after publication")
        storage._check_path(path)
        fd = storage._open_regular(path, os.O_RDONLY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
        storage.sync_directory(path.parent)
    except Exception:
        # The published result is known; verification failure is not rollback.
        return {"outcome": "durability_uncertain"}
    return {"outcome": "durably_committed"}


def _group(state, key):
    _require(state is not None, "groups are uninitialized; create a group explicitly")
    group = next((group for group in state["groups"] if group["key"] == key), None)
    _require(group is not None, "group does not exist")
    return group


def _mutate(workspace, key, operation, expected_revision, *, name=None, member_key=None,
            role=None, binding=None, rebind=False):
    _selector(key)
    root = _workspace(workspace)
    with storage.workspace_lock(root, root / "state" / ".workspace.lock"):
        _workspace(root)
        state, data = _read(root)
        _cas(state, expected_revision)
        changed, member = False, None
        if operation == "create":
            if state is None:
                state = {"schema_version": 1, "workspace_id": str(uuid.uuid4()),
                         "content_revision": 0, "groups": [], "publication_counters": []}
            group = next((g for g in state["groups"] if g["key"] == key), None)
            if group is None:
                group = {"group_id": str(uuid.uuid4()), "key": key, "name": name, "members": []}
                state["groups"].append(group)
                changed = True
            else:
                _require(group["name"] == name, "group name conflict; renaming is unsupported")
        else:
            group = _group(state, key)
            member = next((m for m in group["members"] if m["key"] == member_key), None)
            if operation == "remove":
                if member is not None:
                    group["members"].remove(member)
                    changed = True
            elif member is None:
                member = {"participant_id": str(uuid.uuid4()), "key": member_key,
                          "role": role, "binding": deepcopy(binding)}
                group["members"].append(member)
                changed = True
            else:
                same = _stable(member["binding"]) == _stable(binding)
                _require(same or rebind, "binding changed; explicit rebind required")
                if not same or member["role"] != role:
                    member.update(role=role, binding=deepcopy(binding))
                    changed = True
        if changed:
            if operation == "add" and binding["evidence"]["provenance"] == "kitsune.pane.process_info":
                state["schema_version"] = 2
            _require(state["content_revision"] < _U64, "content revision overflow")
            state["content_revision"] += 1
        delivery = _finish(root, state, data, changed)
        result = {"outcome": "committed" if changed else "unchanged", "revision": _revision(state),
                  "group": group, "delivery": delivery, "authorization": "none", "dispatchable": False}
        if operation in {"add", "remove"}:
            result["member"] = member
        return result


def create_group(workspace, key, name, expected_revision=None):
    _display(name)
    return _mutate(workspace, key, "create", expected_revision, name=name)


def add_member(workspace, group_key, member_key, role, binding, *, rebind=False, expected_revision=None):
    """Associate a caller-observed existing terminal; never probe or authorize it."""
    _selector(member_key)
    _display(role)
    _binding(binding)
    _require(type(rebind) is bool, "rebind must be boolean")
    return _mutate(workspace, group_key, "add", expected_revision, member_key=member_key,
                   role=role, binding=binding, rebind=rebind)


def remove_member(workspace, group_key, member_key, expected_revision=None):
    _selector(member_key)
    return _mutate(workspace, group_key, "remove", expected_revision, member_key=member_key)


def reserve_cycle(workspace, group_key, expected_revision, endpoint: dict, source: str,
                  epoch: str, terminal_ids: list[str], *, allow_new_epoch: bool):
    """Atomically burn a complete publication cycle before any caller transport I/O."""
    _selector(group_key)
    _endpoint(endpoint)
    _source(source)
    _string(epoch)
    _require(type(allow_new_epoch) is bool)
    _require(isinstance(expected_revision, str), "current content revision required")
    _require(isinstance(terminal_ids, list) and 1 <= len(terminal_ids) <= 16)
    for terminal in terminal_ids:
        _string(terminal)
    _require(len(set(terminal_ids)) == len(terminal_ids), "duplicate reservation terminal")
    root = _workspace(workspace)
    with storage.workspace_lock(root, root / "state" / ".workspace.lock"):
        _workspace(root)
        state, data = _read(root)
        _cas(state, expected_revision)
        group = _group(state, group_key)
        presentations = [m["binding"]["presentation"] for m in group["members"]]
        _require(all(p["endpoint"] == endpoint for p in presentations), "group endpoint mismatch")
        _require(set(terminal_ids) == {p["terminal_id"] for p in presentations},
                 "reservation requires the exact whole-group target set")
        counter = next((c for c in state["publication_counters"]
                        if c["endpoint"] == endpoint and c["source"] == source), None)
        if counter is None:
            _require(len(state["publication_counters"]) < 8, "publication counter limit exceeded")
            counter = {"endpoint": deepcopy(endpoint), "source": source, "epoch": epoch, "watermarks": {}}
            state["publication_counters"].append(counter)
        elif counter["epoch"] != epoch:
            _require(allow_new_epoch, "explicit new publisher epoch required")
            counter.update(epoch=epoch, watermarks={})
        watermarks = counter["watermarks"]
        _require(len(set(watermarks) | set(terminal_ids)) <= 64, "publication watermark limit exceeded")
        reservations = {}
        for terminal in terminal_ids:
            previous = watermarks.get(terminal, 0)
            _require(previous < _U64, "publication revision overflow")
            reservations[terminal] = previous + 1
        watermarks.update(reservations)
        delivery = _finish(root, state, data, True)
        return {"revision": _revision(state), "reservations": reservations, "delivery": delivery}
