"""Explicit trusted Djinn producer -> bounded persisted evidence, never admission."""

import calendar
import datetime
import hashlib
import os
from pathlib import Path
import re
import time

if __package__:
    from . import kitsune_observation as wire, workers
else:
    import kitsune_observation as wire
    import workers

ADAPTER_CONTRACT = "coven.djinn.observation.v1"
NATIVE_CONTRACT = "djinn.session.observation.v1"
CAPABILITIES = {"persisted_folder_events": True, "live_status": False,
                "ui_sessions": False, "native_store": False}
_STATES = {"created", "running", "paused", "completed", "failed", "cancelled"}
_HASH = re.compile(r"[0-9a-f]{64}\Z")
_NATIVE_ID = re.compile(r"agt_[A-Za-z0-9_-]+\Z", re.ASCII)
_TIME = re.compile(r"(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})"
                   r"(?:\.\d+)?([Zz]|[+-]\d{2}:\d{2})\Z", re.ASCII)
LIMITATIONS = [
    "Persisted evidence only: observation receipt is not event freshness, live "
    "activity, native runtime verification, job acceptance or success.",
    "The explicit executable is trusted, not sandboxed or authenticated. "
    "Fingerprints and alias checks detect ordinary changes, not every execution race. "
    "A selected binary can ignore arguments and access the inherited HOME/PATH.",
    "Executable fingerprint I/O (up to 1 GiB per hash) is outside the subprocess "
    "deadline. Provider keys and Djinn/Kitsune configuration overrides are withheld, "
    "but this is not native filesystem/network isolation.",
    "Folder/native IDs and file digests describe sourced persisted records, not "
    "a live process binding. Per-file stability is not a transactional snapshot. "
    "No group membership, worker readiness, policy or execution authority follows.",
]


def _text(value, limit):
    wire._string(value, limit)
    wire._require(0 < len(value.encode("utf-8")) <= limit)


def _native_id(value):
    if value is not None:
        _text(value, 128)
        wire._require(bool(_NATIVE_ID.fullmatch(value)))


def _timestamp(value, *, utc=False):
    _text(value, 64)
    match = _TIME.fullmatch(value)
    wire._require(match is not None)
    year, month, day, hour, minute, second = map(int, match.groups()[:6])
    wire._require(1 <= month <= 12 and 1 <= day <= calendar.monthrange(year, month)[1]
                  and hour <= 23 and minute <= 59 and second <= 60)
    zone = match[7]
    wire._require(zone.lower() == "z" or (int(zone[1:3]) <= 23 and int(zone[4:]) <= 59))
    if utc:
        wire._require(zone.lower() == "z" or zone == "+00:00")


def _event(value, *, lifecycle=False):
    wire._require(isinstance(value, dict))
    keys = ("state", "mode", "event_id", "session_id", "recorded_at") if lifecycle else (
        "kind", "event_id", "session_id", "recorded_at")
    wire._require(set(keys) <= value.keys())
    if lifecycle:
        wire._require(isinstance(value["state"], str) and value["state"] in _STATES)
        wire._require(value["mode"] is None or value["mode"] in ("foreground", "background"))
    else:
        _text(value["kind"], 64)
        wire._require(bool(re.fullmatch(r"[a-z][a-z0-9_]{0,63}", value["kind"], re.ASCII)))
    if value["event_id"] is not None:
        _text(value["event_id"], 256)
        wire._require(not any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in value["event_id"]))
    _native_id(value["session_id"])
    if value["recorded_at"] is not None:
        _timestamp(value["recorded_at"])
    return {key: value[key] for key in keys}


def _count(value, cap):
    wire._require(type(value) is int and 0 <= value <= cap)


def _source(value, path, cap):
    wire._require(isinstance(value, dict) and value.get("path") == path)
    _count(value.get("bytes"), cap)
    digest = value.get("sha256")
    wire._require(isinstance(digest, str) and bool(_HASH.fullmatch(digest)))
    if value["bytes"] == 0:
        wire._require(digest == hashlib.sha256(b"").hexdigest())
    return {key: value[key] for key in ("path", "bytes", "sha256")}


def _facts(value):
    wire._require(isinstance(value, dict))
    keys = {"recorded_native_session_id", "recorded_created_at", "sources", "event_count",
            "last_event", "last_lifecycle", "consistency"}
    wire._require(keys <= value.keys() and value["consistency"] == "stable_per_file_not_transactional")
    _native_id(value["recorded_native_session_id"])
    if value["recorded_created_at"] is not None:
        _timestamp(value["recorded_created_at"])
    sources = value["sources"]
    wire._require(isinstance(sources, dict))
    manifest = _source(sources.get("manifest"), "djinn.toml", 65536)
    events = sources.get("folder_events")
    wire._require(isinstance(events, dict) and events.get("path") == "events.jsonl")
    wire._require({"status", "bytes", "sha256"} <= events.keys())
    if events["status"] == "missing":
        wire._require(events["bytes"] is None and events["sha256"] is None
                      and all(value[key] is None for key in ("event_count", "last_event", "last_lifecycle")))
        projected_events = {key: events[key] for key in ("path", "status", "bytes", "sha256")}
    else:
        wire._require(events["status"] == "observed")
        projected_events = {**_source(events, "events.jsonl", 4194304), "status": "observed"}
        _count(value["event_count"], 16384)
        if value["event_count"] == 0:
            wire._require(value["last_event"] is None and value["last_lifecycle"] is None)
        else:
            wire._require(events["bytes"] > 0 and value["last_event"] is not None)
    last = None if value["last_event"] is None else _event(value["last_event"])
    lifecycle = None if value["last_lifecycle"] is None else _event(value["last_lifecycle"], lifecycle=True)
    if last is not None and last["kind"] == "session_lifecycle_updated":
        wire._require(lifecycle is not None and all(last[k] == lifecycle[k]
                      for k in ("event_id", "session_id", "recorded_at")))
    elif last is not None and lifecycle is not None:
        # A non-lifecycle last record cannot reuse a preceding lifecycle ID:
        # the producer rejects duplicate non-null IDs across the entire ledger.
        wire._require(last["event_id"] is None or lifecycle["event_id"] is None
                      or last["event_id"] != lifecycle["event_id"])
    if lifecycle is not None and value["event_count"] == 1:
        wire._require(last is not None and last["kind"] == "session_lifecycle_updated")
    identities = [v for v in (value["recorded_native_session_id"],
                  None if last is None else last["session_id"],
                  None if lifecycle is None else lifecycle["session_id"]) if v is not None]
    wire._require(len(set(identities)) <= 1)
    return {"recorded_native_session_id": value["recorded_native_session_id"],
            "recorded_created_at": value["recorded_created_at"],
            "sources": {"manifest": manifest, "folder_events": projected_events},
            "event_count": value["event_count"], "last_event": last,
            "last_lifecycle": lifecycle, "consistency": value["consistency"]}


def _decode(payload, folder):
    value = wire._decode(payload)
    wire._require(isinstance(value, dict))
    wire._require(type(value.get("schema_version")) is int and isinstance(value.get("contract"), str))
    if value["schema_version"] != 1 or value["contract"] != NATIVE_CONTRACT:
        raise wire._Failure("unsupported_contract", "incompatible")
    wire._require(value.get("selected_folder") == folder)
    for key, expected in (("capabilities", CAPABILITIES), ("authorization", "none"),
                          ("runtime_fact_freshness", "unknown"), ("native_lifecycle", "unestablished"),
                          ("job_completion", "unestablished"), ("activity", "unknown")):
        wire._require(value.get(key) == expected)
    wire._require(value.get("dispatchable") is False)
    # Dict equality alone would accept integer 0/1 in place of booleans.
    wire._require(all(type(v) is bool for v in value["capabilities"].values()))
    _timestamp(value.get("started_at"), utc=True)
    _timestamp(value.get("observed_at"), utc=True)
    wire._require("observation" in value and "resolved_folder" in value)
    assessment = value.get("assessment")
    diagnostics = value.get("diagnostics")
    wire._require(isinstance(diagnostics, list) and len(diagnostics) <= 16)
    wire._require(all(isinstance(d, str) and re.fullmatch(r"[a-z][a-z0-9_]{0,63}", d, re.ASCII)
                      for d in diagnostics))
    resolved = value["resolved_folder"]
    if resolved is not None:
        workers._observation_folder(resolved)
        wire._require(os.path.normpath(resolved) == resolved)
    if assessment == "observed":
        wire._require(resolved is not None and not diagnostics)
        facts = _facts(value["observation"])
    else:
        wire._require(assessment == "unavailable" and bool(diagnostics) and value["observation"] is None)
        facts = None
    return assessment, facts, {"contract": NATIVE_CONTRACT,
        "started_at": value["started_at"], "observed_at": value["observed_at"],
        "resolved_folder": resolved, "capabilities": dict(CAPABILITIES)}


def inspect(executable: str, folder: str, *, timeout=3.0, max_output_bytes=131072) -> dict:
    """One fixed producer call; no discovery, workspace, retries or cached facts."""
    workers._validate_limits(timeout, max_output_bytes)
    workers._observation_folder(folder)
    workers._observation_folder(executable)
    started = time.monotonic()
    report = {"schema_version": 1, "adapter": "djinn", "adapter_contract": ADAPTER_CONTRACT,
              "executable": {"selected_path": executable, "resolved_path": None, "identity": None},
              "folder": folder, "request": {"method": "session.observe"},
              "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "assessment": "unavailable", "diagnostics": [], "authorization": "none",
              "dispatchable": False, "runtime_fact_freshness": "unknown", "activity": "unknown",
              "native_lifecycle": "unestablished", "job_completion": "unestablished",
              "producer": None, "observation": None, "limitations": list(LIMITATIONS)}
    try:
        selected = Path(executable)
        resolved = selected.resolve(strict=True)
        identity = workers._identity(resolved)
        report["executable"].update(resolved_path=str(resolved), identity=identity)
        if (selected.resolve(strict=True) != resolved
                or workers._metadata(resolved) != {k: v for k, v in identity.items() if k != "sha256"}):
            raise wire._Failure("executable_changed")
        result = workers.run_session_observation_probe(
            executable=resolved, folder=folder, timeout=timeout, max_output_bytes=max_output_bytes,
            cwd="/", env={k: v for k, v in os.environ.items() if k in workers.ENV_KEYS})
        final = workers._identity(resolved)
        if selected.resolve(strict=True) != resolved or identity != final:
            raise wire._Failure("executable_changed")
        wire._require(isinstance(result, workers.ProbeResult)
                      and isinstance(result.stdout, bytes) and isinstance(result.stderr, bytes)
                      and isinstance(result.status, str))
        if len(result.stdout) + len(result.stderr) > max_output_bytes:
            raise wire._Failure("output_limit")
        if result.status != "ok" or result.returncode != 0 or type(result.returncode) is not int:
            code = result.status if result.status in {"timeout", "output_limit", "spawn_error"} else "producer_error"
            raise wire._Failure(code)
        assessment, facts, producer = _decode(result.stdout, folder)
        report.update(assessment=assessment, observation=facts, producer=producer,
                      diagnostics=[] if assessment == "observed" else ["native_unavailable"])
    except wire._Failure as exc:
        report.update(assessment=exc.assessment, diagnostics=[exc.code])
    except (OSError, RuntimeError):
        report["diagnostics"] = ["executable_unavailable"]
    except (ValueError, UnicodeError):
        report["diagnostics"] = ["schema_error"]
    report["observed_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    report["duration_ms"] = round((time.monotonic() - started) * 1000, 3)
    return report
