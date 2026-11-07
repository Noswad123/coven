"""Read-only pane-target inventory and declaration joins, never native readiness.

No legacy agent metadata, native CLI probes, live-artifact reads, locks or writes.
Inventory brackets preserve ordinary mapping changes, not atomic native identity.
"""

from copy import deepcopy
import datetime
import os
from pathlib import Path
import time

if __package__:
    from . import groups, storage
    from . import kitsune_observation as wire
    from . import kitsune_process_observation as process
else:
    import groups
    import storage
    import kitsune_observation as wire
    import kitsune_process_observation as process

CONTRACT = "coven.session-inspection.v1"
MAX_PANES = 64
LIMITATIONS = [
    "Scope is existing panes on one selected Kitsune endpoint, not a complete native session/worker catalog.",
    "Executable-family matches are process hints, not authenticated binaries, interactive mode or native session identity.",
    "Matching inventory brackets are not atomic attribution; PID reuse, ABA and post-read changes remain possible.",
    "Activity/attention are not assessed. Unknown is not idle, stopped, absent, ready, successful or complete.",
    "Group/role/native references are explicit historical declarations, not permissions or verified native identity.",
    "Live attachments are not opened; use the separate group status contract for scoped busy/unknown evidence.",
    "Reports are uncached observations, not authorization for subsequent input, launch, resume or stop.",
    "Deadline is an acceptance budget, not a filesystem syscall interrupt or hard native freshness bound.",
]


def _now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def _endpoint(path):
    parent, leaf = os.path.split(path)
    resolved = os.path.join(os.path.realpath(parent), leaf)
    info = wire._endpoint_stat(resolved)
    return {"selected": path, "resolved": resolved, "device": info.st_dev, "inode": info.st_ino}


def _mapping(snapshot):
    return {pane["pane_id"]: pane for pane in snapshot["panes"]}


def _schema(view):
    return None if view["state"] is None else view["state"]["schema_version"]


def _matches_key(matches):
    return sorted(matches, key=lambda p: (p["pid"], p["executable_family"], p["rule"]))


def _join_declarations(view, endpoint, all_panes, targets):
    """Build only report data; never mutate the authority or infer live native refs."""
    by_terminal = {pane["terminal_id"]: pane for pane in all_panes.values()}
    by_pane = {target["pane_id"]: target for target in targets}
    unmatched, elsewhere, outside_scope = [], 0, 0
    key = endpoint["resolved"], endpoint["device"], endpoint["inode"]
    for group in ([] if view["state"] is None else view["state"]["groups"]):
        for member in group["members"]:
            binding = member["binding"]
            presentation, evidence = binding["presentation"], binding["evidence"]
            declared_endpoint = presentation["endpoint"]
            if declared_endpoint["path"] != endpoint["resolved"]:
                elsewhere += 1
                continue
            association = {
                "group_id": group["group_id"], "group_key": group["key"], "group_name": group["name"],
                "participant_id": member["participant_id"], "member_key": member["key"], "role": member["role"],
                "association_basis": "explicit_declaration", "binding_provenance": evidence["provenance"],
                "declared_pane_id": presentation["pane_id_at_binding"],
                "declared_terminal_id": presentation["terminal_id"],
                "declared_native_reference": deepcopy(evidence["native_reference"]),
                "native_identity": "unestablished",
                "live_attachment_declared": member.get("live_attachment") is not None,
            }
            if groups._endpoint_key(declared_endpoint) != key:
                unmatched.append({**association, "binding_check": "endpoint_identity_changed"})
                continue
            current = by_terminal.get(presentation["terminal_id"])
            if current is None:
                unmatched.append({**association, "binding_check": "terminal_not_observed"})
                continue
            target = by_pane.get(current["pane_id"])
            if target is None:
                outside_scope += 1
                continue
            if evidence["provenance"] == "kitsune.pane.process_info":
                association["binding_check"] = ("matching_process_evidence"
                    if evidence["process_evidence"]["matches"] == target["matches"]
                    else "process_evidence_changed")
            else:
                association["binding_check"] = "legacy_evidence_not_revalidated"
            target["associations"].append(association)
    return {"assessment": "observed" if view["initialized"] else "uninitialized",
            "revision": view["revision"], "authority_schema_version": _schema(view),
            "unmatched": unmatched, "other_endpoint_count": elsewhere,
            "outside_inspection_count": outside_scope}


def inspect(workspace, socket_path, *, pane_id=None, expected_terminal_id=None,
            runtime="djinn", timeout=3.0, max_response_bytes=1048576):
    """N+2 read-only API calls, two declaration reads, no cached partial positives."""
    wire._arguments(socket_path, timeout, max_response_bytes)
    if runtime != "djinn":
        raise ValueError("only Djinn executable-family interpretation is supported")
    if pane_id is not None:
        process._pane_id(pane_id)
    if expected_terminal_id is not None:
        process._pane_id(expected_terminal_id)
        if pane_id is None:
            raise ValueError("expected terminal requires one explicit pane")
    started = time.monotonic()
    deadline = started+timeout
    report = {
        "schema_version": 1, "contract": CONTRACT,
        "scope": "existing_panes_on_selected_kitsune_endpoint", "runtime": runtime,
        "workspace": str(Path(workspace).absolute()),
        "request": {"mode": "list" if pane_id is None else "inspect", "pane_id": pane_id,
                    "expected_terminal_id": expected_terminal_id},
        "endpoint": {"selected": socket_path, "resolved": None, "device": None, "inode": None},
        "started_at": _now(), "assessment": "unavailable", "diagnostics": [], "targets": [],
        "target_count": None, "recognized_target_count": None,
        "declarations": {"assessment": "unavailable", "revision": None, "authority_schema_version": None,
                         "unmatched": [], "other_endpoint_count": None, "outside_inspection_count": None},
        "authorization": "none", "dispatchable": False, "native_catalog_complete": False,
        "native_identity": "unestablished", "native_lifecycle": "unestablished",
        "native_worker_acceptance": "unestablished", "job_completion": "unestablished",
        "runtime_fact_freshness": "unknown", "server_snapshot_timestamp": None,
        "limitations": list(wire.LIMITATIONS)+list(process.LIMITATIONS)+list(LIMITATIONS),
    }
    try:
        before_groups = groups.read_groups(workspace)
        wire._remaining(deadline)
        endpoint = _endpoint(socket_path)
        report["endpoint"] = endpoint
        budget = wire._ReadBudget(max_response_bytes)
        before_snapshot = wire._pane_snapshot(*budget.read(endpoint, "session.snapshot", {}, deadline=deadline))
        before = _mapping(before_snapshot)
        if pane_id is None:
            if len(before) > MAX_PANES:
                raise wire._Failure("pane_limit")
            selected = list(before.values())
        else:
            if pane_id not in before:
                raise wire._Failure("pane_not_found")
            selected = [before[pane_id]]
            if expected_terminal_id is not None and selected[0]["terminal_id"] != expected_terminal_id:
                raise wire._Failure("terminal_mismatch")
        targets = []
        for pane in selected:
            payload, request_id = budget.read(endpoint, "pane.process_info", {"pane_id": pane["pane_id"]},
                                             deadline=deadline)
            matches = _matches_key(process._processes(payload, request_id, pane["pane_id"]))
            targets.append({**pane, "provenance": "kitsune.pane.process_info",
                "interpreter": process.ADAPTER_CONTRACT, "sample_received_at": _now(),
                "mapping_evidence": "matching_inventory_brackets",
                "recognition": "djinn_executable_family" if matches else "unknown", "matches": matches,
                "activity": "unknown", "attention": "unknown", "native_identity": "unestablished",
                "native_lifecycle": "unestablished", "job_completion": "unestablished", "associations": []})
        after_snapshot = wire._pane_snapshot(*budget.read(endpoint, "session.snapshot", {}, deadline=deadline))
        after = _mapping(after_snapshot)
        if before_snapshot["version"] != after_snapshot["version"]:
            raise wire._Failure("server_context_changed")
        if ((pane_id is None and before != after)
                or (pane_id is not None and before[pane_id] != after.get(pane_id))):
            raise wire._Failure("pane_mapping_changed")
        after_groups = groups.read_groups(workspace)
        if (before_groups["revision"], _schema(before_groups)) != (after_groups["revision"], _schema(after_groups)):
            raise wire._Failure("groups_changed")
        if _endpoint(socket_path) != endpoint:
            raise wire._Failure("endpoint_replaced")
        declarations = _join_declarations(before_groups, endpoint, after, targets)
        wire._remaining(deadline)
        report.update(assessment="observed", targets=targets, target_count=len(targets),
                      recognized_target_count=sum(t["recognition"] == "djinn_executable_family" for t in targets),
                      declarations=declarations, server_version=before_snapshot["version"],
                      protocol=before_snapshot["protocol"])
    except storage.StorageError:
        report["diagnostics"] = ["groups_unavailable"]
    except wire._Failure as exc:
        report.update(assessment=exc.assessment, diagnostics=[exc.code])
    except TimeoutError:
        report["diagnostics"] = ["timeout"]
    except OSError:
        report["diagnostics"] = ["io_error"]
    report["observed_at"] = _now()
    report["duration_ms"] = round((time.monotonic()-started)*1000, 3)
    return report


def exit_code(report):
    return 0 if report["assessment"] == "observed" else 2 if report["assessment"] == "incompatible" else 1
