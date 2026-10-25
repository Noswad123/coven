"""Pinned, private source heartbeat -> short-lived legacy busy evidence only."""

import datetime
import math
import os
import re
import stat
import time
import uuid

if __package__:
    from . import kitsune_observation as wire
else:
    import kitsune_observation as wire

ADAPTER_CONTRACT = "coven.djinn.live-observation.v1"
NATIVE_CONTRACT = "djinn.live-observation.v1"
_MAX_BYTES = 65536
_MAX_SAFE_INT = 2**53 - 1
_SESSION = re.compile(r"[!-~]{1,128}\Z", re.ASCII)
_CAPABILITIES = {"activity": True, "idle": False, "catalog_complete": False,
                 "decision_state": False, "retry_state": False,
                 "durable_lifecycle": False, "job_completion": False}
LIMITATIONS = [
    "Freshly sampled legacy runner ownership only, not provider progress, health, "
    "global activity or idle. Current/V2 execution, decisions and retry phases are unsupported.",
    "Freshness assumes a suitable local filesystem and a shared same-host wall clock. "
    "Rollback can make old samples appear younger; a stateless reader cannot detect "
    "all replay or sequence regression. Sequence is ordering metadata, not authentication.",
    "Private ownership/modes and path/inode checks are not peer authentication, "
    "a sandbox or protection against malicious same-UID writers or every pathname race.",
    "The timeout is an acceptance deadline, not a way to interrupt blocked filesystem "
    "syscalls. Remaining lifetime is receipt-relative and must decrease with elapsed time; "
    "reading or importing saved reports never renews source freshness.",
    "No discovery, runtime startup, subprocess, socket, database, config or state write. "
    "No group enrollment, readiness upgrade, lifecycle, job completion or execution grant.",
]


def _fields(value, names):
    wire._require(isinstance(value, dict) and set(value) == set(names.split()))


def _uuid(value):
    wire._string(value, 36)
    try:
        parsed = uuid.UUID(value)
    except ValueError:
        raise wire._Failure("schema_error") from None
    wire._require(str(parsed) == value and parsed.version == 4 and parsed.variant == uuid.RFC_4122)


def _arguments(file_path, runtime_id, instance_id, session_id, timeout):
    if (not isinstance(file_path, str) or not file_path.startswith("/")
            or file_path.endswith("/") or ".." in file_path.split("/")
            or any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in file_path)):
        raise ValueError("an explicit absolute artifact path without parent components or controls is required")
    try:
        if len(file_path.encode("utf-8")) > 4096:
            raise ValueError("artifact path exceeds 4096 UTF-8 bytes")
        _uuid(runtime_id)
        _uuid(instance_id)
    except UnicodeError:
        raise ValueError("artifact path must be valid UTF-8") from None
    except wire._Failure:
        raise ValueError("explicit canonical version-4 runtime and instance UUIDs are required") from None
    if not isinstance(session_id, str) or not _SESSION.fullmatch(session_id):
        raise ValueError("an explicit opaque ASCII session ID of 1..128 printable non-space characters is required")
    if os.path.basename(file_path) != runtime_id + ".json":
        raise ValueError("artifact filename must match the explicitly pinned runtime UUID")
    if (type(timeout) not in (int, float) or not 0 < timeout <= 1
            or not math.isfinite(timeout)):
        raise ValueError("acceptance timeout must be finite and in (0, 1]")


def _integer(value, *, positive=False):
    wire._require(type(value) is int and int(positive) <= value <= _MAX_SAFE_INT)


def _decode(payload, runtime_id, instance_id, session_id):
    value = wire._decode(payload)
    wire._require(isinstance(value, dict) and type(value.get("schema_version")) is int
                  and isinstance(value.get("contract"), str))
    if value["schema_version"] != 1 or value["contract"] != NATIVE_CONTRACT:
        raise wire._Failure("unsupported_contract", "incompatible")
    _fields(value, "schema_version contract runtime_id sequence sampled_at_ms valid_for_ms "
                   "source activity_scope capabilities authorization dispatchable native_lifecycle "
                   "job_completion instances")
    _uuid(value["runtime_id"])
    _integer(value["sequence"], positive=True)
    _integer(value["sampled_at_ms"])
    wire._require(type(value["valid_for_ms"]) is int and value["valid_for_ms"] == 3000)
    wire._require(value["source"] == "legacy_session_run_state"
                  and value["activity_scope"] == "legacy_runner_ownership"
                  and value["authorization"] == "none" and value["dispatchable"] is False
                  and value["native_lifecycle"] == "unestablished"
                  and value["job_completion"] == "unestablished")
    capabilities = value["capabilities"]
    wire._require(isinstance(capabilities, dict) and capabilities.keys() == _CAPABILITIES.keys())
    wire._require(all(type(capabilities[k]) is bool and capabilities[k] is v
                      for k, v in _CAPABILITIES.items()))
    instances = value["instances"]
    wire._require(isinstance(instances, list) and len(instances) <= 16)
    seen_instances, count, target, matched_instance = set(), 0, None, False
    for instance in instances:
        _fields(instance, "instance_id sessions")
        _uuid(instance["instance_id"])
        wire._require(instance["instance_id"] not in seen_instances)
        seen_instances.add(instance["instance_id"])
        sessions = instance["sessions"]
        wire._require(isinstance(sessions, list))
        count += len(sessions)
        wire._require(count <= 128)
        seen_sessions = set()
        if instance["instance_id"] == instance_id:
            matched_instance = True
        for session in sessions:
            _fields(session, "session_id activity")
            selected = session["session_id"]
            wire._require(isinstance(selected, str) and bool(_SESSION.fullmatch(selected))
                          and selected not in seen_sessions)
            seen_sessions.add(selected)
            wire._require(session["activity"] in ("busy", "unknown"))
            if instance["instance_id"] == instance_id and selected == session_id:
                target = session["activity"]
    # Validate the entire document before selecting the requested target.
    if value["runtime_id"] != runtime_id:
        raise wire._Failure("runtime_mismatch")
    if not matched_instance:
        raise wire._Failure("instance_not_observed")
    if target is None:
        raise wire._Failure("session_not_observed")
    source = {key: value[key] for key in ("contract", "runtime_id", "sequence", "sampled_at_ms",
                                         "valid_for_ms", "source", "activity_scope", "capabilities")}
    return target, {**source, "instance_id": instance_id, "session_id": session_id}


def _directory(selected):
    info = os.lstat(selected)
    if (not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid()
            or stat.S_IMODE(info.st_mode) != 0o700):
        raise wire._Failure("unsafe_directory")
    resolved = os.path.realpath(selected)
    actual = os.lstat(resolved)
    if (not stat.S_ISDIR(actual.st_mode) or actual.st_uid != os.getuid()
            or stat.S_IMODE(actual.st_mode) != 0o700
            or (info.st_dev, info.st_ino) != (actual.st_dev, actual.st_ino)):
        raise wire._Failure("directory_changed")
    return resolved, info.st_dev, info.st_ino


def _stamp(info):
    if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
            or stat.S_IMODE(info.st_mode) != 0o600 or info.st_nlink != 1):
        raise wire._Failure("unsafe_artifact")
    if info.st_size > _MAX_BYTES:
        raise wire._Failure("response_limit")
    return info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns


def inspect(file_path: str, runtime_id: str, instance_id: str, session_id: str, *, timeout=1.0) -> dict:
    """Read one pinned source artifact; unknown never becomes idle or stopped."""
    _arguments(file_path, runtime_id, instance_id, session_id, timeout)
    started = time.monotonic()
    report = {
        "schema_version": 1, "adapter": "djinn_live", "adapter_contract": ADAPTER_CONTRACT,
        "file": {"selected_path": file_path, "resolved_path": None, "device": None, "inode": None},
        "selection": {"runtime_id": runtime_id, "instance_id": instance_id, "session_id": session_id},
        "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "assessment": "unavailable", "diagnostics": [], "activity": "unknown",
        "activity_scope": "legacy_runner_ownership", "attention": "unknown",
        "freshness": "unknown", "expires_in_ms": 0, "source": None,
        "authorization": "none", "dispatchable": False,
        "native_lifecycle": "unestablished", "job_completion": "unestablished",
        "limitations": list(LIMITATIONS),
    }
    try:
        if os.name != "posix" or not all(hasattr(os, k) for k in ("getuid", "O_NOFOLLOW", "O_NONBLOCK")):
            raise wire._Failure("unsupported_platform")
        parent, leaf = os.path.split(file_path)
        directory = _directory(parent)
        resolved = os.path.join(directory[0], leaf)
        report["file"]["resolved_path"] = resolved
        before = _stamp(os.lstat(resolved))
        report["file"].update(device=before[0], inode=before[1])
        fd = os.open(resolved, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd, "rb") as stream:
            if _stamp(os.fstat(stream.fileno())) != before or _directory(parent) != directory:
                raise wire._Failure("artifact_changed")
            payload = stream.read(_MAX_BYTES + 1)
            if len(payload) > _MAX_BYTES:
                raise wire._Failure("response_limit")
            activity, source = _decode(payload, runtime_id, instance_id, session_id)
            if (_stamp(os.fstat(stream.fileno())) != before or _stamp(os.lstat(resolved)) != before
                    or _directory(parent) != directory):
                raise wire._Failure("artifact_changed")
        elapsed_ms = (time.monotonic() - started) * 1000
        if elapsed_ms >= timeout * 1000:
            raise wire._Failure("timeout")
        age_ms = time.time() * 1000 - source["sampled_at_ms"]
        if age_ms < 0:
            raise wire._Failure("future_sample")
        # Deliberately conservative: age at acceptance plus elapsed read time can
        # double-count latency. Never grant the full TTL again on receipt.
        remaining_ms = math.floor(source["valid_for_ms"] - age_ms - elapsed_ms)
        if remaining_ms <= 0:
            raise wire._Failure("stale_sample")
        report.update(assessment="observed", activity=activity, source=source,
                      freshness="fresh_sample", expires_in_ms=remaining_ms)
    except wire._Failure as exc:
        report.update(assessment=exc.assessment, diagnostics=[exc.code])
    except OSError:
        report["diagnostics"] = ["io_error"]
    report["observed_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    report["duration_ms"] = round((time.monotonic() - started) * 1000, 3)
    return report
