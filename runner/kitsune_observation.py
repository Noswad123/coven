"""Bounded observation of one caller-selected, existing Kitsune API socket."""

import datetime
import json
import math
import os
import socket
import stat
import time
import uuid

ADAPTER_CONTRACT = "coven.kitsune.observation.v1"
LIMITATIONS = (
    "Observation only: no execution authorization or UI return channel. "
    "Kitsune's request pipeline performs metadata expiry, event drainage and title "
    "synchronization, and may ensure a default workspace (potentially spawning a "
    "shell) in an empty existing session. Zero server-side effects are not promised.",
    "Socket ownership, permissions and connection are not peer authentication. "
    "Parent path aliases are resolved but parent directories are trusted; stat "
    "checks cannot eliminate path replacement races. Device/inode identify the "
    "observed endpoint, not a globally stable server identity.",
    "Snapshot receipt is not runtime freshness attestation. Native references may "
    "be persisted; pane presence, done status and empty inventories do not establish "
    "native lifecycle, runtime shutdown, job completion or success.",
)
_LOCATORS = ("pane_id", "terminal_id", "workspace_id", "tab_id")
_STATUSES = {"idle", "working", "blocked", "done", "unknown"}


class _Failure(Exception):
    def __init__(self, code, assessment="unavailable"):
        self.code = code
        self.assessment = assessment


def _require(condition):
    if not condition:
        raise _Failure("schema_error")


def _string(value, limit=4096):
    _require(isinstance(value, str) and len(value) <= limit)
    _require(not any(0xD800 <= ord(c) <= 0xDFFF for c in value))


def _uint(value):
    _require(type(value) is int and 0 <= value <= 2**64 - 1)


def _pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise _Failure("duplicate_key")
        result[key] = value
    return result


def _constant(_value):
    raise _Failure("nonfinite_json")


def _decode(payload):
    """Pre-limit nesting before invoking the standard JSON decoder."""
    try:
        text = payload.decode("utf-8", errors="strict")
    except UnicodeError:
        raise _Failure("invalid_utf8") from None
    depth, quoted, escaped = 0, False, False
    for c in text:
        if quoted:
            if escaped:
                escaped = False
            elif c == "\\":
                escaped = True
            elif c == '"':
                quoted = False
        elif c == '"':
            quoted = True
        elif c in "[{":
            depth += 1
            if depth > 32:
                raise _Failure("nesting_limit")
        elif c in "]}":
            depth -= 1
    try:
        value = json.loads(text, object_pairs_hook=_pairs, parse_constant=_constant)
    except (ValueError, RecursionError):
        raise _Failure("invalid_json") from None
    pending = [(value, None)]
    while pending:
        item, key = pending.pop()
        if isinstance(item, dict):
            _require(len(item) <= 4096)
            for k, v in item.items():
                _string(k)
                pending.append((v, k))
        elif isinstance(item, list):
            _require(len(item) <= 4096)
            pending.extend((v, key) for v in item)
        elif isinstance(item, str):
            _string(item, 8192 if key in {"value", "cwd", "foreground_cwd", "path"}
                    or (isinstance(key, str) and key.endswith("_path")) else 4096)
        elif isinstance(item, float):
            _require(math.isfinite(item))
    return value


def _inventory(snapshot):
    _require(isinstance(snapshot, dict))
    _string(snapshot.get("version"))
    protocol = snapshot.get("protocol")
    _require(type(protocol) is int and 0 <= protocol <= 2**32 - 1)
    if protocol != 18:
        raise _Failure("unsupported_protocol", "incompatible")
    for field in ("focused_workspace_id", "focused_tab_id", "focused_pane_id"):
        if snapshot.get(field) is not None:
            _string(snapshot[field])
    for field in ("workspaces", "tabs", "panes", "layouts", "agents"):
        items = snapshot.get(field)
        _require(isinstance(items, list) and len(items) <= 1024)
        _require(all(isinstance(item, dict) for item in items))
    # Unused inventories are shape-checked, not interpreted as lifecycle evidence.
    for field, ids in (("workspaces", ("workspace_id",)),
                       ("tabs", ("workspace_id", "tab_id")),
                       ("layouts", ("workspace_id", "tab_id", "focused_pane_id"))):
        for item in snapshot[field]:
            for key in ids:
                _string(item.get(key))
            _require(type(item.get("focused" if field != "layouts" else "zoomed")) is bool)
            if field == "layouts":
                _require(isinstance(item.get("area"), dict))
                for key in ("panes", "splits"):
                    _require(isinstance(item.get(key), list))
                    _require(all(isinstance(v, dict) for v in item[key]))
            else:
                for key in ("number", "pane_count"):
                    _uint(item.get(key))
                _string(item.get("label"))
                _require(item.get("agent_status") in _STATUSES
                         if isinstance(item.get("agent_status"), str) else False)
                if field == "workspaces":
                    _uint(item.get("tab_count"))
                    _string(item.get("active_tab_id"))
    collections = []
    for field in ("panes", "agents"):
        panes, terminals = {}, set()
        for item in snapshot[field]:
            for key in _LOCATORS:
                _string(item.get(key))
            _require(type(item.get("focused")) is bool)
            status = item.get("agent_status")
            _require(isinstance(status, str) and status in _STATUSES)
            _uint(item.get("revision"))
            for key in ("agent", "name"):
                if item.get(key) is not None:
                    _string(item[key])
            native = item.get("agent_session")
            if native is not None:
                _require(isinstance(native, dict))
                for key in ("source", "agent", "kind", "value"):
                    _string(native.get(key), 8192 if key == "value" and native.get("kind") == "path" else 4096)
                _require(native["kind"] in {"id", "path"})
            if field == "agents":
                for key in ("launch_pending", "interactive_ready", "screen_detection_skipped"):
                    _require(type(item.get(key, False)) is bool)
                _uint(item.get("state_change_seq", 0))
            _require(item["pane_id"] not in panes and item["terminal_id"] not in terminals)
            panes[item["pane_id"]] = item
            terminals.add(item["terminal_id"])
        collections.append(panes)
    for agent in collections[1].values():
        pane = collections[0].get(agent["pane_id"])
        _require(pane is not None)
        for key in (*_LOCATORS, "focused", "agent_status", "revision", "agent"):
            _require(pane.get(key) == agent.get(key))
        # Compare only the current reference fields, allowing additive fields.
        for key in ("source", "agent", "kind", "value"):
            _require((pane.get("agent_session") or {}).get(key)
                     == (agent.get("agent_session") or {}).get(key))
    return snapshot


def _result(payload, request_id, result_type):
    root = _decode(payload)
    _require(isinstance(root, dict) and root.get("id") == request_id)
    _require(("result" in root) != ("error" in root))
    if "error" in root:
        error = root["error"]
        _require(isinstance(error, dict))
        _string(error.get("code"))
        _string(error.get("message"))
        raise _Failure("server_error")
    result = root["result"]
    _require(isinstance(result, dict) and result.get("type") == result_type)
    return result


def _snapshot(payload, request_id):
    return _inventory(_result(payload, request_id, "session_snapshot").get("snapshot"))


def _endpoint_stat(path):
    info = os.lstat(path)
    if not stat.S_ISSOCK(info.st_mode):
        raise _Failure("unsafe_endpoint")
    if info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) & 0o077:
        raise _Failure("unsafe_endpoint")
    return info


def _arguments(socket_path, timeout, max_response_bytes):
    if (not isinstance(socket_path, str) or not socket_path.startswith("/")
            or "\0" in socket_path or ".." in socket_path.split("/")
            or socket_path.endswith("/")):
        raise ValueError("an explicit absolute socket path without '..' is required")
    try:
        path_bytes = os.fsencode(socket_path)
    except UnicodeError:
        raise ValueError("invalid socket path") from None
    if len(path_bytes) > 4096 or any(0xD800 <= ord(c) <= 0xDFFF for c in socket_path):
        raise ValueError("socket path is too long or invalid")
    if (type(timeout) not in (int, float) or not 0 < timeout <= 30
            or not math.isfinite(timeout)):
        raise ValueError("timeout must be finite and in (0, 30]")
    if type(max_response_bytes) is not int or not 1024 <= max_response_bytes <= 8 * 1024 * 1024:
        raise ValueError("response budget must be between 1024 and 8388608")


def _remaining(deadline):
    left = deadline - time.monotonic()
    if left <= 0:
        raise _Failure("timeout")
    return left


def _read_request(endpoint, method, params, *, deadline, max_response_bytes):
    """Internal read-only transport, fenced to an already selected endpoint.

    Budget includes LF; EOF is required, not just a complete JSON frame. Callers
    performing multiple reads must subtract returned wire bytes from their budget.
    """
    if method not in {"session.snapshot", "pane.process_info"}:
        raise ValueError("unsupported observation method")
    if max_response_bytes <= 0:
        raise _Failure("response_limit")

    def fence():
        info = _endpoint_stat(endpoint["resolved"])
        if (info.st_dev, info.st_ino) != (endpoint["device"], endpoint["inode"]):
            raise _Failure("endpoint_replaced")

    request_id = uuid.uuid4().hex
    request = json.dumps({"id": request_id, "method": method, "params": params}).encode() + b"\n"
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
        stream.settimeout(_remaining(deadline))
        stream.connect(endpoint["resolved"])
        fence()
        stream.settimeout(_remaining(deadline))
        stream.sendall(request)
        data = bytearray()
        while True:
            stream.settimeout(_remaining(deadline))
            chunk = stream.recv(min(65536, max_response_bytes - len(data) + 1))
            if not chunk:
                break
            data.extend(chunk)
            if len(data) > max_response_bytes:
                raise _Failure("response_limit")
            newline = data.find(b"\n")
            if newline >= 0 and newline != len(data) - 1:
                raise _Failure("trailing_data")
        if not data.endswith(b"\n"):
            raise _Failure("incomplete_response")
        fence()
    return bytes(data[:-1]), request_id, len(data)


def inspect(socket_path: str, *, timeout: float = 3.0,
            max_response_bytes: int = 1048576) -> dict:
    """Observe once. Caller errors raise ValueError; runtime failures are envelopes."""
    _arguments(socket_path, timeout, max_response_bytes)
    started = time.monotonic()
    deadline = started + timeout
    report = {
        "schema_version": 1, "adapter": "kitsune", "adapter_contract": ADAPTER_CONTRACT,
        "endpoint": {"selected": socket_path, "resolved": None, "device": None, "inode": None},
        "request": {"method": "session.snapshot"},
        "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "authorization": "none", "dispatchable": False,
        "runtime_fact_freshness": "unknown", "server_snapshot_timestamp": None,
        "limitations": list(LIMITATIONS), "agents": [], "pane_count": None,
        "assessment": "unavailable", "diagnostics": [],
    }

    try:
        # Resolve parent aliases (not the leaf), notably macOS /var -> /private/var.
        parent, leaf = os.path.split(socket_path)
        resolved = os.path.join(os.path.realpath(parent), leaf)
        report["endpoint"]["resolved"] = resolved
        before = _endpoint_stat(resolved)
        report["endpoint"].update(device=before.st_dev, inode=before.st_ino)
        payload, request_id, _ = _read_request(
            report["endpoint"], "session.snapshot", {}, deadline=deadline,
            max_response_bytes=max_response_bytes)
        snapshot = _snapshot(payload, request_id)
        _remaining(deadline)
        agents = []
        for item in snapshot["agents"]:
            status = item["agent_status"]
            agent = {
                "provenance": "kitsune.session.snapshot",
                "locators": {"endpoint": dict(report["endpoint"]), **{k: item[k] for k in _LOCATORS}},
                "raw_status": status,
                "activity": "idle" if status in {"done", "idle"} else status,
                "attention": "unacknowledged" if status == "done" else "needs_attention" if status == "blocked" else "unknown",
                "native_lifecycle": "unestablished", "job_completion": "unestablished",
                "revision": item["revision"], "focused": item["focused"],
                "state_change_seq": item.get("state_change_seq", 0),
            }
            for key in ("launch_pending", "interactive_ready", "screen_detection_skipped"):
                agent[key] = item.get(key, False)
            for key, target in (("agent", "agent_kind"), ("name", "name")):
                if item.get(key) is not None:
                    agent[target] = item[key]
            if item.get("agent_session") is not None:
                native = item["agent_session"]
                agent["native_session"] = {k: native[k] for k in ("source", "agent", "kind", "value")}
            agents.append(agent)
        report.update(assessment="observed", agents=agents, pane_count=len(snapshot["panes"]))
    except _Failure as exc:
        report.update(assessment=exc.assessment, diagnostics=[exc.code])
    except (TimeoutError, socket.timeout):
        report["diagnostics"] = ["timeout"]
    except OSError:
        report["diagnostics"] = ["io_error"]
    report["observed_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    report["duration_ms"] = round((time.monotonic() - started) * 1000, 3)
    return report
