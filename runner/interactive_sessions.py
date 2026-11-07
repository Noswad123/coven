"""Explicit interactive UI submission, not bounded-worker execution or readiness.

Preview is pure. Execution requires the workspace's admitted transport gate and
uses only a discovered generic command API. Never retry an ambiguous submission.
"""

import json
import os
from pathlib import Path
import re
import socket
import stat
import time
import uuid

if __package__:
    from . import actions, storage
    from . import kitsune_observation as wire
else:
    import actions
    import storage
    import kitsune_observation as wire

CONTRACT = "coven.interactive-session.v1"
LIMITATIONS = [
    "Input queued is not native startup, session identity, readiness, worker acceptance or completion.",
    "Select a quiescent empty shell prompt; other writers and post-check foreground changes remain possible.",
    "The native UI uses its own configuration, credentials and permissions; this is not a resource/tool sandbox.",
    "Executable/socket metadata checks are not authentication or protection against hostile same-user races.",
    "No durable submission receipt or automatic retry: inspect native state after an uncertain outcome.",
    "Workspace is control context, not a cwd change. Read requests may trigger existing server housekeeping.",
]


def _text(value, label, *, nonblank=True):
    if (not isinstance(value, str) or (nonblank and not value.strip())
            or any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in value)):
        raise ValueError(f"{label} must be nonblank text without control characters")
    try:
        if len(value.encode("utf-8")) > 4096:
            raise ValueError(f"{label} exceeds 4096 UTF-8 bytes")
    except UnicodeError:
        raise ValueError(f"{label} must be valid UTF-8") from None
    return value


def _absolute(value, label):
    _text(value, label)
    if not value.startswith("/") or ".." in value.split("/") or value.endswith("/"):
        raise ValueError(f"{label} must be an explicit absolute path without '..' or a trailing slash")
    return value


def plan(workspace, operation, runtime, executable, socket_path, pane_id, terminal_id, session=None):
    """No filesystem, network, subprocess, environment inspection or mutation."""
    _absolute(workspace, "workspace")
    if runtime != "djinn" or operation not in {"start", "resume"}:
        raise ValueError("only explicit Djinn interactive start/resume is supported")
    _absolute(executable, "executable")
    _absolute(socket_path, "socket")
    for value, label in ((pane_id, "pane"), (terminal_id, "expected terminal")):
        _text(value, label)
        if any(c.isspace() for c in value):
            raise ValueError(f"{label} must be an exact opaque identifier")
    if operation == "resume":
        _text(session, "session selector")
        if session.startswith("-"):
            raise ValueError("session selector must not start with '-'")
        argv = [executable, "--session", session]
    else:
        if session is not None:
            raise ValueError("start does not accept a session selector")
        argv = [executable, "--ui"]
    return {"schema_version": 1, "contract": CONTRACT, "workspace": workspace,
            "operation": operation, "runtime": runtime, "executable": executable,
            "socket": socket_path, "pane_id": pane_id, "expected_terminal_id": terminal_id,
            "session_selector": session, "argv": argv}


def _frozen(value):
    if not isinstance(value, dict) or type(value.get("schema_version")) is not int:
        raise ValueError("invalid interactive plan")
    try:
        expected = plan(*(value[k] for k in ("workspace", "operation", "runtime", "executable",
                        "socket", "pane_id", "expected_terminal_id")), value["session_selector"])
    except KeyError:
        raise ValueError("invalid interactive plan") from None
    if value != expected:
        raise ValueError("interactive plan differs from canonical arguments")
    return expected


def preview(value):
    frozen = _frozen(value)
    return {**frozen, "outcome": "preview", "delivery": "not_attempted",
            "authorization": "none", "native_lifecycle": "unestablished",
            "native_worker_acceptance": "unestablished", "job_completion": "unestablished",
            "diagnostics": [], "limitations": list(LIMITATIONS)}


class SubmissionError(Exception):
    def __init__(self, code, delivery="unsent", *, interrupted=False):
        self.code, self.delivery, self.interrupted = code, delivery, interrupted
        super().__init__(code)


def _endpoint(path):
    parent, leaf = os.path.split(path)
    resolved = os.path.join(os.path.realpath(parent), leaf)
    info = wire._endpoint_stat(resolved)
    return {"selected": path, "resolved": resolved, "device": info.st_dev, "inode": info.st_ino}


def _fence(endpoint):
    if _endpoint(endpoint["selected"]) != endpoint:
        raise wire._Failure("endpoint_replaced")


def _executable(path):
    try:
        info = os.stat(path)
    except OSError:
        raise SubmissionError("executable_unavailable") from None
    if not stat.S_ISREG(info.st_mode) or not os.access(path, os.X_OK):
        raise SubmissionError("executable_unavailable")
    return info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns


def _snapshot(endpoint, frozen, budget, deadline):
    payload, request_id = budget.read(endpoint, "session.snapshot", {}, deadline=deadline)
    raw = wire._result(payload, request_id, "session_snapshot").get("snapshot")
    wire._require(isinstance(raw, dict))
    capability = raw.get("command_submission")
    if (not isinstance(capability, dict) or type(capability.get("version")) is not int
            or capability["version"] != 1):
        raise SubmissionError("command_submission_unsupported")
    generic = wire._pane_snapshot(payload, request_id)
    panes = [p for p in generic["panes"] if p["pane_id"] == frozen["pane_id"]]
    if len(panes) != 1:
        raise SubmissionError("pane_not_found")
    if panes[0]["terminal_id"] != frozen["expected_terminal_id"]:
        raise SubmissionError("terminal_mismatch")
    return panes[0]


def _submit(endpoint, frozen, *, deadline, max_response_bytes):
    """One mutating request; delivery classification survives partial send/errors."""
    started_write = False
    try:
        wire._remaining(deadline)
        _fence(endpoint)
        request_id = uuid.uuid4().hex
        params = {k: frozen[k] for k in ("pane_id", "expected_terminal_id", "argv")}
        payload = json.dumps({"id": request_id, "method": "pane.submit_command", "params": params},
                             ensure_ascii=True, allow_nan=False).encode()+b"\n"
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
            stream.settimeout(wire._remaining(deadline))
            address = min((endpoint["resolved"], endpoint["selected"]), key=lambda p: len(os.fsencode(p)))
            stream.connect(address)
            _fence(endpoint)
            stream.settimeout(wire._remaining(deadline))
            started_write = True
            stream.sendall(payload)
            data = bytearray()
            while True:
                stream.settimeout(wire._remaining(deadline))
                chunk = stream.recv(min(65536, max_response_bytes-len(data)+1))
                if not chunk:
                    break
                data.extend(chunk)
                if len(data) > max_response_bytes:
                    raise wire._Failure("response_limit")
                if b"\n" in data and data.find(b"\n") != len(data)-1:
                    raise wire._Failure("trailing_data")
            if not data.endswith(b"\n"):
                raise wire._Failure("incomplete_response")
            _fence(endpoint)
        wire._remaining(deadline)
        root = wire._decode(bytes(data[:-1]))
        wire._require(isinstance(root, dict) and root.get("id") == request_id
                      and (("result" in root) != ("error" in root)))
        if "error" in root:
            error = root["error"]
            wire._require(isinstance(error, dict))
            code = error.get("code")
            wire._require(isinstance(code, str) and re.fullmatch(r"[A-Za-z0-9_.-]{1,64}", code))
            wire._string(error.get("message"))
            raise SubmissionError("server_error:"+code, "rejected")
        result = root["result"]
        wire._require(isinstance(result, dict) and result.get("type") == "pane_command_submitted"
                      and result.get("pane_id") == frozen["pane_id"]
                      and result.get("terminal_id") == frozen["expected_terminal_id"]
                      and result.get("argv") == frozen["argv"])
    except SubmissionError:
        raise
    except (wire._Failure, OSError, KeyboardInterrupt) as exc:
        code = (exc.code if isinstance(exc, wire._Failure) else "interrupted"
                if isinstance(exc, KeyboardInterrupt) else "timeout" if isinstance(exc, TimeoutError) else "io_error")
        raise SubmissionError(code, "uncertain" if started_write else "unsent",
                              interrupted=isinstance(exc, KeyboardInterrupt)) from None


def execute(value, *, gate, timeout=3.0):
    """Require a current admitted workspace gate; never accepts a readiness report."""
    frozen = _frozen(value)
    wire._arguments(frozen["socket"], timeout, 1048576)
    if not isinstance(gate, storage.TransportGate):
        raise storage.StorageError("interactive submission requires an admitted workspace transport gate")
    root = Path(frozen["workspace"])
    report = preview(frozen)
    report.update(authorization="interactive_terminal_submission_only", outcome="unavailable", delivery="unsent")
    phase = "unsent"
    # Validate token location, owner and liveness through the existing gate contract.
    with storage.transport_gate(root, root/"state/.runtime-transport.lock", held=gate, timeout=0):
        if actions.ActionStore(root).read_control() is not None:
            raise actions.ActionError("Approved runtime-launch mode forbids unreviewed interactive submission")
        deadline = time.monotonic()+timeout
        try:
            executable = _executable(frozen["executable"])
            endpoint = _endpoint(frozen["socket"])
            budget = wire._ReadBudget(1048576)
            before = _snapshot(endpoint, frozen, budget, deadline)
            after = _snapshot(endpoint, frozen, budget, deadline)
            if before != after:
                raise SubmissionError("pane_mapping_changed")
            if _executable(frozen["executable"]) != executable:
                raise SubmissionError("executable_changed")
            wire._remaining(deadline)
            if budget.remaining <= 0:
                raise SubmissionError("response_limit")
            phase = "uncertain"  # Covers interruption after return but before recording ack.
            _submit(endpoint, frozen, deadline=deadline, max_response_bytes=budget.remaining)
            phase = "queued"
            report.update(outcome="input_queued", delivery="queued")
        except SubmissionError as exc:
            report.update(outcome="uncertain" if exc.delivery == "uncertain" else
                          "rejected" if exc.delivery == "rejected" else "unavailable",
                          delivery=exc.delivery, diagnostics=[exc.code])
            if exc.interrupted:
                report["interrupted"] = True
        except (wire._Failure, OSError) as exc:
            report.update(outcome="unavailable", delivery=phase,
                          diagnostics=[exc.code if isinstance(exc, wire._Failure) else "io_error"])
        except KeyboardInterrupt:
            report.update(outcome="input_queued" if phase == "queued" else "uncertain" if phase == "uncertain"
                          else "unavailable", delivery=phase, diagnostics=["interrupted"], interrupted=True)
    return report


def exit_code(report):
    if report.get("interrupted"):
        return 130
    if report["outcome"] in {"preview", "input_queued"}:
        return 0
    return 2 if report["delivery"] == "uncertain" else 1
