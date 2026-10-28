"""Explicit Djinn-native interactive prompt admission; never terminal input."""

import hashlib
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

CONTRACT = "coven.interactive-prompt.v1"
NATIVE_CONTRACT = "djinn.interactive-prompt.v1"
MAX_PROMPT_BYTES = 65536
RESPONSE_BYTES = 65536
LIMITATIONS = [
    "Native acceptance acknowledges a message, not worker-policy admission, model execution or job completion.",
    "The selected runtime must explicitly enable its native prompt socket at startup; no discovery or fallback.",
    "Native configuration, plugins, credentials and permissions remain independent; no sandbox or grant transfer.",
    "No automatic retry or durable reconciliation; inspect native state after an uncertain outcome.",
    "Request suppression is process-local and bounded; a changed runtime is not a retry permission.",
    "Socket ownership/identity checks are not authentication or protection against hostile same-user races.",
    "Workspace is control context, not a native cwd/resource scope; filesystem calls are not interruptible by the budget.",
]


def _uuid(value):
    try:
        return isinstance(value, str) and str(uuid.UUID(value)) == value
    except (ValueError, AttributeError):
        return False


def _absolute(value, label):
    if (not isinstance(value, str) or not value.startswith("/") or ".." in value.split("/")
            or value.endswith("/") or any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in value)):
        raise ValueError(f"{label} requires an explicit absolute path without controls or '..'")
    try:
        if len(value.encode("utf-8")) > 4096:
            raise ValueError(f"{label} is too long")
    except UnicodeError:
        raise ValueError(f"{label} must be valid UTF-8") from None


def plan(workspace, socket_path, runtime_id, session_id, request_id, prompt_file):
    for value, label in ((workspace, "workspace"), (socket_path, "native socket"), (prompt_file, "prompt file")):
        _absolute(value, label)
    if not _uuid(runtime_id) or not _uuid(request_id):
        raise ValueError("expected runtime and request ID must be canonical UUIDs")
    if not isinstance(session_id, str) or re.fullmatch(r"ses_[A-Za-z0-9_-]{1,128}", session_id) is None:
        raise ValueError("session requires an exact native ses_ identifier, not a folder or pane")
    return {"schema_version": 1, "contract": CONTRACT, "workspace": workspace,
            "native_socket": socket_path, "expected_runtime_id": runtime_id,
            "session_id": session_id, "request_id": request_id, "prompt_file": prompt_file}


def preview(value):
    if not isinstance(value, dict) or type(value.get("schema_version")) is not int:
        raise ValueError("invalid native prompt plan")
    try:
        expected = plan(*(value[k] for k in ("workspace", "native_socket", "expected_runtime_id",
                                            "session_id", "request_id", "prompt_file")))
    except (KeyError, TypeError):
        raise ValueError("invalid native prompt plan") from None
    if value != expected:
        raise ValueError("native prompt plan differs from canonical arguments")
    return {**expected, "outcome": "preview", "delivery": "not_attempted", "authorization": "none",
            "native_prompt_acceptance": "unestablished", "native_worker_acceptance": "unestablished",
            "native_lifecycle": "unestablished", "job_completion": "unestablished",
            "prompt_sha256": None, "prompt_bytes": None, "native_message_id": None,
            "diagnostics": [], "limitations": list(LIMITATIONS)}


class PromptError(Exception):
    def __init__(self, code, delivery="unsent", *, interrupted=False):
        self.code, self.delivery, self.interrupted = code, delivery, interrupted
        super().__init__(code)


def _endpoint(address):
    parent, leaf = os.path.split(address)
    info = os.lstat(parent)
    if (not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid()
            or stat.S_IMODE(info.st_mode) != 0o700):
        raise PromptError("unsafe_native_parent")
    resolved = os.path.join(os.path.realpath(parent), leaf)
    socket_info = wire._endpoint_stat(resolved)
    if stat.S_IMODE(socket_info.st_mode) != 0o600:
        raise PromptError("unsafe_native_socket")
    return {"selected": address, "resolved": resolved, "device": socket_info.st_dev,
            "inode": socket_info.st_ino, "parent_device": info.st_dev, "parent_inode": info.st_ino}


def _exchange(endpoint, method, params, request_id, deadline, budget):
    if method not in {"capabilities", "submit"}:
        raise ValueError("unsupported native prompt method")
    started = False
    try:
        if _endpoint(endpoint["selected"]) != endpoint:
            raise PromptError("native_endpoint_changed")
        frame = json.dumps({"version": 1, "id": request_id, "method": method, "params": params},
                           ensure_ascii=True, allow_nan=False).encode()+b"\n"
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
            stream.settimeout(wire._remaining(deadline))
            stream.connect(min((endpoint["selected"], endpoint["resolved"]), key=lambda p: len(os.fsencode(p))))
            if _endpoint(endpoint["selected"]) != endpoint:
                raise PromptError("native_endpoint_changed")
            stream.settimeout(wire._remaining(deadline))
            started = True
            stream.sendall(frame)
            data = bytearray()
            while True:
                stream.settimeout(wire._remaining(deadline))
                chunk = stream.recv(min(65536, budget[0]-len(data)+1))
                if not chunk:
                    break
                data.extend(chunk)
                if len(data) > budget[0]:
                    raise wire._Failure("response_limit")
            budget[0] -= len(data)
            if not data.endswith(b"\n") or data.count(b"\n") != 1:
                raise wire._Failure("invalid_native_frame")
            if _endpoint(endpoint["selected"]) != endpoint:
                raise wire._Failure("native_endpoint_changed")
        wire._remaining(deadline)
        response = wire._decode(bytes(data[:-1]))
        wire._require(isinstance(response, dict) and type(response.get("version")) is int
                      and response["version"] == 1 and response.get("id") == request_id
                      and _uuid(response.get("runtime_id")) and (("result" in response) != ("error" in response)))
        return response
    except PromptError as exc:
        if started and method == "submit":
            raise PromptError(exc.code, "uncertain") from None
        raise
    except (wire._Failure, OSError, KeyboardInterrupt) as exc:
        code = (exc.code if isinstance(exc, wire._Failure) else "interrupted" if isinstance(exc, KeyboardInterrupt)
                else "timeout" if isinstance(exc, TimeoutError) else "native_io_error")
        raise PromptError(code, "uncertain" if started and method == "submit" else "unsent",
                          interrupted=isinstance(exc, KeyboardInterrupt)) from None


def _capability(response):
    result = response.get("result")
    if (not isinstance(result, dict) or result.get("type") != "capabilities"
            or result.get("contract") != NATIVE_CONTRACT or type(result.get("version")) is not int
            or result["version"] != 1 or result.get("runtime_id") != response["runtime_id"]
            or type(result.get("max_prompt_bytes")) is not int or result["max_prompt_bytes"] != MAX_PROMPT_BYTES):
        raise PromptError("native_prompt_unsupported")
    return result


def capabilities(socket_path, *, timeout=3.0):
    """One explicit native metadata query; not a permission or compatibility grant."""
    wire._arguments(socket_path, timeout, RESPONSE_BYTES)
    report = {"schema_version": 1, "contract": "coven.native-prompt-capabilities.v1",
              "assessment": "unavailable", "authorization": "none", "runtime_id": None,
              "diagnostics": []}
    try:
        endpoint = _endpoint(socket_path)
        result = _capability(_exchange(endpoint, "capabilities", {}, str(uuid.uuid4()),
                                      time.monotonic()+timeout, [RESPONSE_BYTES]))
        report.update(assessment="observed", runtime_id=result["runtime_id"], native_contract=NATIVE_CONTRACT)
    except (PromptError, wire._Failure, OSError) as exc:
        report["diagnostics"] = [exc.code if isinstance(exc, (PromptError, wire._Failure)) else "native_io_error"]
        if isinstance(exc, PromptError) and exc.interrupted:
            report["interrupted"] = True
    except KeyboardInterrupt:
        report.update(diagnostics=["interrupted"], interrupted=True)
    return report


def _prompt_file(address):
    descriptor = os.open(address, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as source:
        if not stat.S_ISREG(os.fstat(source.fileno()).st_mode):
            raise PromptError("invalid_prompt_file")
        raw = source.read(MAX_PROMPT_BYTES+1)
    try:
        text = raw.decode("utf-8", errors="strict")
    except UnicodeError:
        raise PromptError("invalid_prompt_text") from None
    if (len(raw) > MAX_PROMPT_BYTES or not text.strip()
            or any((ord(c) < 32 and c not in "\t\r\n") or 127 <= ord(c) <= 159 for c in text)):
        raise PromptError("invalid_prompt_text")
    return text, hashlib.sha256(raw).hexdigest(), len(raw)


def _receipt(response, frozen, digest, size):
    if response["runtime_id"] != frozen["expected_runtime_id"]:
        raise PromptError("invalid_native_runtime_receipt", "uncertain")
    if "error" in response:
        error = response["error"]
        allowed = {"runtime_mismatch", "request_capacity", "session_unavailable", "request_unavailable",
                   "request_already_seen", "admission_uncertain"}
        if isinstance(error, dict) and isinstance(error.get("code"), str) and error["code"] in allowed:
            expected = "uncertain" if error["code"] in {"request_already_seen", "admission_uncertain"} else "rejected"
            if error.get("delivery") == expected:
                raise PromptError("native_"+error["code"], expected)
        raise PromptError("invalid_native_error", "uncertain")
    result = response["result"]
    if (response["runtime_id"] != frozen["expected_runtime_id"] or not isinstance(result, dict)
            or result.get("type") != "prompt_accepted" or result.get("contract") != NATIVE_CONTRACT
            or result.get("session_id") != frozen["session_id"] or result.get("prompt_sha256") != digest
            or type(result.get("prompt_bytes")) is not int or result["prompt_bytes"] != size
            or not isinstance(result.get("message_id"), str)
            or re.fullmatch(r"msg_[A-Za-z0-9_-]{1,128}", result["message_id"]) is None):
        raise PromptError("invalid_native_receipt", "uncertain")
    return result["message_id"]


def execute(value, *, gate, timeout=3.0):
    report = preview(value)
    wire._arguments(value["native_socket"], timeout, RESPONSE_BYTES)
    if not isinstance(gate, storage.TransportGate):
        raise storage.StorageError("native prompt submission requires an admitted workspace transport gate")
    root = Path(value["workspace"])
    with storage.transport_gate(root, root/"state/.runtime-transport.lock", held=gate, timeout=0):
        if actions.ActionStore(root).read_control() is not None:
            raise actions.ActionError("Approved runtime-launch mode forbids unreviewed native prompt submission")
        report.update(outcome="unavailable", delivery="unsent", authorization="interactive_native_prompt_submission_only")
        phase = "unsent"
        try:
            deadline = time.monotonic()+timeout
            text, digest, size = _prompt_file(value["prompt_file"])
            report.update(prompt_sha256=digest, prompt_bytes=size)
            endpoint = _endpoint(value["native_socket"])
            budget = [RESPONSE_BYTES]
            capability = _capability(_exchange(endpoint, "capabilities", {}, str(uuid.uuid4()), deadline, budget))
            if capability["runtime_id"] != value["expected_runtime_id"]:
                raise PromptError("native_runtime_mismatch")
            params = {"expected_runtime_id": value["expected_runtime_id"], "session_id": value["session_id"], "text": text}
            phase = "uncertain"
            response = _exchange(endpoint, "submit", params, value["request_id"], deadline, budget)
            message = _receipt(response, value, digest, size)
            phase = "accepted"
            report.update(outcome="accepted", delivery="accepted", native_prompt_acceptance="accepted", native_message_id=message)
        except PromptError as exc:
            report.update(outcome="uncertain" if exc.delivery == "uncertain" else "rejected" if exc.delivery == "rejected"
                          else "unavailable", delivery=exc.delivery, diagnostics=[exc.code])
            if exc.interrupted:
                report["interrupted"] = True
        except (wire._Failure, OSError) as exc:
            report.update(outcome="unavailable", delivery="unsent",
                          diagnostics=[exc.code if isinstance(exc, wire._Failure) else "prompt_or_endpoint_unavailable"])
        except KeyboardInterrupt:
            report.update(outcome="accepted" if phase == "accepted" else "uncertain" if phase == "uncertain" else "unavailable",
                          delivery=phase, diagnostics=["interrupted"], interrupted=True)
    return report


def exit_code(report):
    return (130 if report.get("interrupted") else 0 if report["outcome"] in {"preview", "accepted"}
            else 2 if report["delivery"] == "uncertain" else 1)
