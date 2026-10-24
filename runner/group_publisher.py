"""Foreground, annotation-only group heartbeats to one explicitly selected socket.

Labels expire naturally; stopping is not cancellation of an admitted write. Socket
stat fences are not peer authentication. Filesystem fsync is not wall-clock bounded.
No native execution, lifecycle inference, source enabling, or payload replay occurs.
"""

import datetime
import json
import math
import os
from pathlib import Path
import re
import socket
import time
import uuid

if __package__:
    from . import groups, storage
    from . import kitsune_observation as observation
    from . import kitsune_process_observation as process_observation
else:
    import groups
    import storage
    import kitsune_observation as observation
    import kitsune_process_observation as process_observation

_INTERVAL = 2.0
_CYCLE_BUDGET = 3.0
_TTL_MS = 6000
_SOURCE = re.compile(r"[A-Za-z0-9_.-]{1,64}\Z", re.ASCII)


class PublisherError(Exception):
    """Safe diagnostic and delivery classification, without raw peer messages."""

    def __init__(self, code, delivery="unsent", *, interrupted=False):
        self.code = code
        self.delivery = delivery
        self.interrupted = interrupted
        super().__init__(code)


def _arguments(socket_path, deadline, max_response_bytes):
    if (not isinstance(socket_path, str) or not socket_path.startswith("/")
            or "\0" in socket_path or ".." in socket_path.split("/")
            or socket_path.endswith("/")
            or any(0xD800 <= ord(c) <= 0xDFFF for c in socket_path)):
        raise ValueError("an explicit absolute socket path without '..' is required")
    if len(os.fsencode(socket_path)) > 4096:
        raise ValueError("socket path is too long")
    if (type(deadline) not in (int, float) or abs(deadline) > 1e100
            or not math.isfinite(deadline)):
        raise ValueError("deadline must be a finite absolute monotonic time")
    if type(max_response_bytes) is not int or not 1024 <= max_response_bytes <= 8388608:
        raise ValueError("response budget must be between 1024 and 8388608")


def _remaining(deadline):
    left = deadline - time.monotonic()
    if left <= 0:
        raise PublisherError("timeout")
    return left


def _endpoint(path):
    parent, leaf = os.path.split(path)
    resolved = os.path.join(os.path.realpath(parent), leaf)
    info = observation._endpoint_stat(resolved)
    return {"path": resolved, "device": info.st_dev, "inode": info.st_ino}


def _request(socket_path, method, params, *, deadline, max_response_bytes=1048576,
             frozen_endpoint=None):
    """Narrow internal transport; sendall failure may include a partial write."""
    if method not in {"session.snapshot", "pane.annotate"}:
        raise ValueError("unsupported publication method")
    writing = method == "pane.annotate"
    if writing and frozen_endpoint is None:
        raise ValueError("annotation requires a frozen endpoint")
    started_write = False
    try:
        _remaining(deadline)
        endpoint = _endpoint(socket_path)
        if frozen_endpoint is not None and endpoint != frozen_endpoint:
            raise PublisherError("endpoint_replaced")

        def fence():
            if _endpoint(socket_path) != endpoint:
                raise PublisherError("endpoint_replaced")

        request_id = uuid.uuid4().hex
        payload = json.dumps({"id": request_id, "method": method, "params": params},
                             ensure_ascii=True, allow_nan=False).encode() + b"\n"
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
            stream.settimeout(_remaining(deadline))
            # Transport spelling need not be the canonical identity: a short
            # parent alias may be the only address that fits sockaddr_un (or
            # vice versa). fence() still re-resolves the original selection.
            address = min((endpoint["path"], socket_path), key=lambda p: len(os.fsencode(p)))
            stream.connect(address)
            fence()
            stream.settimeout(_remaining(deadline))
            started_write = writing
            stream.sendall(payload)
            fence()
            data = bytearray()
            while True:
                stream.settimeout(_remaining(deadline))
                chunk = stream.recv(min(65536, max_response_bytes - len(data) + 1))
                if not chunk:
                    break
                data.extend(chunk)
                if len(data) > max_response_bytes:
                    raise PublisherError("response_limit")
                newline = data.find(b"\n")
                if newline >= 0 and newline != len(data) - 1:
                    raise PublisherError("trailing_data")
            if not data.endswith(b"\n"):
                raise PublisherError("incomplete_response")
            fence()
        _remaining(deadline)
        if not writing:
            return bytes(data[:-1]), request_id, endpoint
        root = observation._decode(bytes(data[:-1]))
        observation._require(isinstance(root, dict) and root.get("id") == request_id)
        observation._require(("result" in root) != ("error" in root))
        if "error" in root:
            error = root["error"]
            observation._require(isinstance(error, dict))
            code = error.get("code")
            observation._string(code, 64)
            observation._require(bool(_SOURCE.fullmatch(code)))
            observation._string(error.get("message"))
            raise PublisherError("server_error:" + code, "rejected")
        result = root["result"]
        observation._require(isinstance(result, dict) and result.get("type") == "ok")
        _remaining(deadline)
        return None
    except KeyboardInterrupt:
        if not writing:
            raise
        raise PublisherError("interrupted", "uncertain" if started_write else "unsent",
                             interrupted=True) from None
    except (PublisherError, observation._Failure, OSError) as exc:
        if isinstance(exc, PublisherError) and exc.delivery == "rejected":
            raise
        code = exc.code if isinstance(exc, (PublisherError, observation._Failure)) else (
            "timeout" if isinstance(exc, TimeoutError) else "io_error")
        raise PublisherError(code, "uncertain" if started_write else "unsent") from None


def read_publication_snapshot(socket_path, *, deadline, max_response_bytes=1048576,
                              read_budget=None):
    """Read a strict, minimal inventory, even before annotation capability opt-in."""
    _arguments(socket_path, deadline, max_response_bytes)
    payload, request_id, endpoint = _request(
        socket_path, "session.snapshot", {}, deadline=deadline,
        max_response_bytes=max_response_bytes if read_budget is None else read_budget.remaining)
    if read_budget is not None:
        read_budget.remaining -= len(payload) + 1
    try:
        snapshot = observation._snapshot(payload, request_id)
        epoch, sources = None, None
        if "annotations" in snapshot:
            descriptor = snapshot["annotations"]
            observation._require(isinstance(descriptor, dict))
            epoch, sources = descriptor.get("epoch"), descriptor.get("sources")
            observation._string(epoch)
            observation._require(bool(epoch))
            observation._require(isinstance(sources, list) and len(sources) <= 8)
            observation._require(all(isinstance(s, str) and _SOURCE.fullmatch(s) for s in sources))
            observation._require(len(set(sources)) == len(sources))
        agents = []
        for item in snapshot["agents"]:
            agent = {key: item[key] for key in observation._LOCATORS}
            agent["agent"] = item.get("agent")
            native = item.get("agent_session")
            agent["agent_session"] = None if native is None else {
                key: native[key] for key in ("source", "agent", "kind", "value")}
            agents.append(agent)
        _remaining(deadline)
        return {"endpoint": endpoint, "epoch": epoch,
                "sources": None if sources is None else list(sources),
                "observed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                "agents": agents,
                "panes": [{key: pane[key] for key in observation._LOCATORS}
                          for pane in snapshot["panes"]]}
    except observation._Failure as exc:
        raise PublisherError(exc.code) from None


def binding_from_snapshot(snapshot, pane_id):
    """Bind an exact current agent entry; a pane alone is not agent evidence."""
    if (not isinstance(pane_id, str) or not pane_id or not pane_id.isprintable()
            or any(c.isspace() for c in pane_id)):
        raise ValueError("an exact current pane ID is required")
    matches = [agent for agent in snapshot["agents"] if agent["pane_id"] == pane_id]
    if len(matches) != 1:
        raise PublisherError("agent_not_found")
    agent = matches[0]
    native = agent["agent_session"]
    return {
        "presentation": {"adapter": "kitsune", "endpoint": dict(snapshot["endpoint"]),
                         "terminal_id": agent["terminal_id"], "pane_id_at_binding": pane_id},
        "evidence": {"provenance": "kitsune.session.snapshot",
                     "observed_at": snapshot["observed_at"], "agent_kind_hint": agent["agent"],
                     "native_reference": None if native is None else dict(native),
                     "native_binding": "unestablished" if native is None else "observed_reference"},
    }


def binding_from_process(socket_path, pane_id, *, deadline, read_budget=None,
                         frozen_endpoint=None, expected_terminal_id=None):
    """Fresh process-family evidence only; no saved report or native fallback."""
    _arguments(socket_path, deadline, 1048576)
    process_observation._pane_id(pane_id)
    try:
        endpoint = _endpoint(socket_path)
        if frozen_endpoint is not None and endpoint != frozen_endpoint:
            raise PublisherError("endpoint_replaced")
        selected = {"selected": socket_path, "resolved": endpoint["path"],
                    "device": endpoint["device"], "inode": endpoint["inode"]}
        item = process_observation._read_evidence(
            selected, pane_id, deadline=deadline,
            budget=observation._ReadBudget(1048576) if read_budget is None else read_budget,
            expected_terminal_id=expected_terminal_id)
        if item["recognition"] != "djinn_executable_family":
            raise PublisherError("process_not_recognized")
        matches = sorted(item["matches"], key=lambda p: (p["pid"], p["executable_family"], p["rule"]))
        binding = {
            "presentation": {"adapter": "kitsune", "endpoint": endpoint,
                             "terminal_id": item["locators"]["terminal_id"],
                             "pane_id_at_binding": pane_id},
            "evidence": {"provenance": "kitsune.pane.process_info",
                         "observed_at": item["sample_received_at"], "agent_kind_hint": "djinn",
                         "native_reference": None, "native_binding": "unestablished",
                         "process_evidence": {"interpreter": item["interpreter"], "matches": matches}},
        }
        groups._binding(binding)
        _remaining(deadline)
        return binding
    except observation._Failure as exc:
        raise PublisherError(exc.code) from None
    except OSError as exc:
        raise PublisherError("timeout" if isinstance(exc, TimeoutError) else "io_error") from None
    except groups.GroupError:
        raise PublisherError("process_binding_invalid") from None


def _read_group(groups, workspace, group_key, revision=None):
    try:
        result = groups.read_groups(workspace)
    except storage.StorageError:
        raise PublisherError("group_state_error") from None
    if revision is not None and result["revision"] != revision:
        raise PublisherError("group_content_changed")
    if not result["initialized"] or result["state"] is None:
        raise PublisherError("group_not_found")
    matches = [group for group in result["state"]["groups"] if group["key"] == group_key]
    if len(matches) != 1:
        raise PublisherError("group_not_found")
    group = matches[0]
    members = group["members"]
    if not 1 <= len(members) <= 16:
        raise PublisherError("group_size")
    endpoint = members[0]["binding"]["presentation"]["endpoint"]
    if any(member["binding"]["presentation"]["endpoint"] != endpoint for member in members):
        raise PublisherError("group_endpoint_mismatch")
    terminals = [member["binding"]["presentation"]["terminal_id"] for member in members]
    if len(set(terminals)) != len(terminals):
        raise PublisherError("duplicate_terminal")
    return result["revision"], group, endpoint


def _publication_summary(summary, cycles):
    """Derive counts without transferring targets between incremental counters."""
    phases = [target["phase"] for cycle in cycles for target in cycle["targets"]]
    committed = [cycle for cycle in cycles if cycle["reservation"] == "durably_committed"]
    reserved_targets = sum(len(cycle["targets"]) for cycle in committed)
    return {
        **summary,
        "cycles": len(committed),
        "completed_cycles": sum(all(target["phase"] == "acknowledged" for target in cycle["targets"])
                                for cycle in committed),
        "reservation_attempts": len(cycles),
        "reservation_attempted_targets": len(phases),
        "reserved_targets": reserved_targets,
        "reservation_uncertain": len(phases) - reserved_targets,
        "acknowledged": phases.count("acknowledged"),
        "rejected": phases.count("rejected"),
        "unsent": phases.count("unsent"),
        "uncertain": phases.count("uncertain") + phases.count("in_flight"),
    }


def publish_group(workspace, group_key, socket_path, source, *, duration=60):
    """Publish fresh serial cycles; summaries never include raw peer/state errors.

    Delivery counts partition reservation_attempted_targets. reserved_targets and
    cycles count confirmed durable reservations; completed_cycles requires every
    target acknowledged. reservation_uncertain counts targets whose reservation
    may have burned numbers but was not confirmed durable, NOT remote deliveries.
    Those targets remain unsent. A raw interrupt in flight is conservatively
    uncertain, including the gap between transport return and recording the ack.
    No lock except the publisher lease is held during network I/O.
    """
    _arguments(socket_path, 0.0, 1048576)
    if not isinstance(source, str) or not _SOURCE.fullmatch(source):
        raise ValueError("source must be 1..64 ASCII letters, digits, '-', '_' or '.'")
    if not isinstance(group_key, str) or not re.fullmatch(r"[a-z0-9][a-z0-9_-]{0,63}", group_key):
        raise ValueError("group key must be a lowercase human selector of 1..64 characters")
    if type(duration) is not int or not 1 <= duration <= 3600:
        raise ValueError("duration must be an integer in 1..3600 seconds")
    summary = {"outcome": "completed", "diagnostics": [],
               "authorization": "annotation_publication_only",
               "native_execution_authorization": "none", "dispatchable": False,
               "ttl_ms": _TTL_MS,
               "limitations": ["Labels expire by TTL; stopping does not cancel admitted writes.",
                               "Filesystem fsync is not wall-clock bounded."]}
    cycles = []
    try:
        revision, _, endpoint = _read_group(groups, workspace, group_key)
        root = Path(workspace)
        with storage.workspace_lock(root, root / "state/.group-publisher.lock", timeout=0):
            end = time.monotonic() + duration
            epoch = None
            first_cycle = True
            while time.monotonic() < end:
                cycle_started = time.monotonic()
                deadline = min(end, cycle_started + _CYCLE_BUDGET)
                _, group, current_endpoint = _read_group(groups, workspace, group_key, revision)
                if current_endpoint != endpoint:
                    raise PublisherError("group_endpoint_mismatch")
                process_bound = any(m["binding"]["evidence"]["provenance"] == "kitsune.pane.process_info"
                                    for m in group["members"])
                read_budget = observation._ReadBudget(1048576) if process_bound else None
                if process_bound:
                    fresh = read_publication_snapshot(socket_path, deadline=deadline, read_budget=read_budget)
                else:
                    fresh = read_publication_snapshot(socket_path, deadline=deadline)
                if fresh["endpoint"] != endpoint:
                    raise PublisherError("endpoint_replaced")
                if fresh["epoch"] is None:
                    raise PublisherError("annotations_unsupported")
                if source not in fresh["sources"]:
                    raise PublisherError("source_disabled")
                if epoch is not None and epoch != fresh["epoch"]:
                    raise PublisherError("annotation_epoch_changed")
                epoch = fresh["epoch"]
                by_terminal = {agent["terminal_id"]: agent for agent in fresh["agents"]}
                panes_by_terminal = {pane["terminal_id"]: pane for pane in fresh["panes"]}
                plan = []
                for member in group["members"]:
                    binding = member["binding"]
                    terminal = binding["presentation"]["terminal_id"]
                    evidence = binding["evidence"]
                    if evidence["provenance"] == "kitsune.pane.process_info":
                        pane = panes_by_terminal.get(terminal)
                        if pane is None:
                            raise PublisherError("bound_terminal_missing")
                        observed = binding_from_process(
                            socket_path, pane["pane_id"], deadline=deadline, read_budget=read_budget,
                            frozen_endpoint=endpoint, expected_terminal_id=terminal)
                        if groups._stable(observed) != groups._stable(binding):
                            raise PublisherError("rebind_required")
                    else:
                        pane = by_terminal.get(terminal)
                        if pane is None:
                            raise PublisherError("bound_agent_missing")
                        if (pane["agent"] != evidence["agent_kind_hint"]
                                or pane["agent_session"] != evidence["native_reference"]):
                            raise PublisherError("rebind_required")
                    plan.append({"pane_id": pane["pane_id"], "expected_terminal_id": terminal,
                                 "annotation_epoch": epoch, "source": source,
                                 "text": group["name"] + " · " + member["role"], "ttl_ms": _TTL_MS})
                _remaining(deadline)
                # Record possible burns BEFORE calling an interruptible storage helper.
                cycle = {"reservation": "outcome_unknown",
                         "targets": [{"params": params, "phase": "unsent"} for params in plan]}
                cycles.append(cycle)
                try:
                    reserved = groups.reserve_cycle(
                        workspace, group_key, revision, endpoint, source, epoch,
                        [item["expected_terminal_id"] for item in plan],
                        allow_new_epoch=first_cycle)
                except storage.StorageError:
                    # The helper may have committed before raising; never claim rollback.
                    raise PublisherError("reservation_failed", "uncertain") from None
                if reserved["delivery"]["outcome"] != "durably_committed":
                    cycle["reservation"] = "durability_uncertain"
                    raise PublisherError("reservation_durability_uncertain", "uncertain")
                if (reserved["revision"] != revision
                        or set(reserved["reservations"]) != {p["expected_terminal_id"] for p in plan}
                        or any(type(n) is not int or not 1 <= n <= 2**64 - 1
                               for n in reserved["reservations"].values())):
                    raise PublisherError("invalid_reservation", "uncertain")
                cycle["reservation"] = "durably_committed"
                first_cycle = False
                for target in cycle["targets"]:
                    params = target["params"]
                    params["revision"] = reserved["reservations"][params["expected_terminal_id"]]
                    try:
                        target["phase"] = "in_flight"
                        _request(socket_path, "pane.annotate", params, deadline=deadline,
                                 frozen_endpoint=endpoint)
                        target["phase"] = "acknowledged"
                    except PublisherError as exc:
                        target["phase"] = exc.delivery
                        raise
                # Skip missed ticks: no overlapping cycles and no catch-up bursts.
                now = time.monotonic()
                next_tick = cycle_started + (math.floor((now - cycle_started) / _INTERVAL) + 1) * _INTERVAL
                wait = min(end - now, max(0.0, next_tick - now))
                if wait > 0:
                    time.sleep(wait)
    except PublisherError as exc:
        summary["outcome"] = "interrupted" if exc.interrupted else (
            "uncertain" if exc.delivery == "uncertain" else "failed")
        summary["diagnostics"].append(exc.code)
    except KeyboardInterrupt:
        summary["outcome"] = "interrupted"
        summary["diagnostics"].append("interrupted")
    except storage.StorageError as exc:
        summary["outcome"] = "failed"
        summary["diagnostics"].append("publisher_busy" if isinstance(exc, storage.LockBusy)
                                      else "publisher_lease_error")
    try:
        return _publication_summary(summary, cycles)
    except KeyboardInterrupt:
        # A single interrupt while deriving counts must not lose recorded phases.
        # Recompute once; a second interrupt is not suppressed indefinitely.
        summary["outcome"] = "interrupted"
        if "interrupted" not in summary["diagnostics"]:
            summary["diagnostics"].append("interrupted")
        return _publication_summary(summary, cycles)
