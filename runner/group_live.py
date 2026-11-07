"""Declared participant/source attachments and ephemeral, revision-scoped status.

Attachment is not terminal/session equivalence. No live facts are persisted or
sent to the multiplexer; source lifetimes must not become six-second role labels.
"""

from copy import deepcopy
import datetime
import math
import time

if __package__:
    from . import djinn_live_observation as live
    from . import groups, storage
    from . import kitsune_observation as wire
else:
    import djinn_live_observation as live
    import groups
    import storage
    import kitsune_observation as wire

STATUS_CONTRACT = "coven.group.status.v1"
LIMITATIONS = [
    "Live attachments are explicit participant declarations, not verified native "
    "identity, current-pane association, or terminal/session equivalence.",
    "Busy evidence covers only freshly sampled local legacy runner ownership. "
    "Unknown is not idle, stopped, failure, completion, readiness or execution permission.",
    "Sequential source observations are not an atomic native snapshot. Group content "
    "is rechecked, but can change after that check. Shared source pins do not prove "
    "distinct workers and must not be counted as distinct native executions.",
    "Remaining lifetime is conservatively reduced after all reads and must continue "
    "decreasing during delivery/display. Same-host wall clock and local-filesystem "
    "assumptions, replay/rollback and same-UID pathname limitations still apply.",
    "Status takes no locks, writes no state, launches no work and publishes no labels. "
    "The overall timeout is an acceptance deadline, not a filesystem syscall interrupt.",
]


def _timeout(value):
    if type(value) not in (int, float) or not 0 < value <= 1 or not math.isfinite(value):
        raise ValueError("acceptance timeout must be finite and in (0, 1]")


def _member(group, member_key):
    member = next((m for m in group["members"] if m["key"] == member_key), None)
    groups._require(member is not None, "member does not exist")
    return member


def _preflight(workspace, group_key, member_key, expected_revision):
    groups._selector(group_key)
    groups._selector(member_key)
    view = groups.read_groups(workspace)
    groups._cas(view["state"], expected_revision)
    return view, _member(groups._group(view["state"], group_key), member_key)


def _accepted(result, attachment):
    """Keep the in-process adapter boundary explicit; project no extra fields."""
    wire._require(isinstance(result, dict) and type(result.get("schema_version")) is int
                  and result["schema_version"] == 1
                  and result.get("adapter_contract") == live.ADAPTER_CONTRACT
                  and result.get("assessment") == "observed"
                  and result.get("freshness") == "fresh_sample"
                  and result.get("activity") in ("busy", "unknown")
                  and result.get("activity_scope") == "legacy_runner_ownership"
                  and result.get("authorization") == "none" and result.get("dispatchable") is False
                  and result.get("native_lifecycle") == "unestablished"
                  and result.get("job_completion") == "unestablished")
    remaining = result.get("expires_in_ms")
    wire._require(type(remaining) is int and 0 < remaining <= 3000)
    pins = {key: attachment[key] for key in ("runtime_id", "instance_id", "session_id")}
    wire._require(result.get("selection") == pins)
    source = result.get("source")
    wire._require(isinstance(source, dict)
                  and source.get("contract") == attachment["source_contract"]
                  and all(source.get(k) == v for k, v in pins.items())
                  and source.get("source") == "legacy_session_run_state"
                  and source.get("activity_scope") == "legacy_runner_ownership")
    live._integer(source.get("sequence"), positive=True)
    live._integer(source.get("sampled_at_ms"))
    wire._require(type(source.get("valid_for_ms")) is int and source["valid_for_ms"] == 3000)
    capabilities = source.get("capabilities")
    wire._require(isinstance(capabilities, dict) and capabilities.keys() == live._CAPABILITIES.keys())
    wire._require(all(type(capabilities[k]) is bool and capabilities[k] is v
                      for k, v in live._CAPABILITIES.items()))
    projected = {key: source[key] for key in ("contract", "runtime_id", "instance_id", "session_id",
                 "sequence", "sampled_at_ms", "valid_for_ms", "source", "activity_scope")}
    projected["capabilities"] = dict(live._CAPABILITIES)
    return projected


def attach(workspace, group_key, member_key, file_path, runtime_id, instance_id, session_id, *,
           reattach=False, expected_revision=None, timeout=1.0):
    """Freshly validate a declaration; never store the accepted activity/lease."""
    groups._require(type(reattach) is bool, "reattach must be boolean")
    live._arguments(file_path, runtime_id, instance_id, session_id, timeout)
    view, member = _preflight(workspace, group_key, member_key, expected_revision)
    previous = member.get("live_attachment")
    # A spelling-only path change may be a parent alias. Compare the checked
    # canonical path only after reading; obvious changed pins need no artifact I/O.
    pins = {"runtime_id": runtime_id, "instance_id": instance_id, "session_id": session_id}
    if previous is not None and any(previous[k] != v for k, v in pins.items()) and not reattach:
        raise groups.GroupError("live attachment changed; explicit reattach required")
    report = live.inspect(file_path, runtime_id, instance_id, session_id, timeout=timeout)
    if not isinstance(report, dict) or report.get("assessment") != "observed":
        incompatible = isinstance(report, dict) and report.get("assessment") == "incompatible"
        raise groups.GroupError("cannot attach live source: " + ("incompatible" if incompatible else "unavailable"))
    try:
        _accepted(report, {"source_contract": live.NATIVE_CONTRACT, **pins})
        file_info = report.get("file")
        wire._require(isinstance(file_info, dict) and isinstance(file_info.get("resolved_path"), str))
    except wire._Failure:
        raise groups.GroupError("cannot attach live source: invalid_live_report") from None
    candidate = {"adapter": "djinn_live", "source_contract": live.NATIVE_CONTRACT,
                 "artifact_path": file_info["resolved_path"], **pins,
                 "attached_at": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    groups._attachment(candidate)
    if (previous is not None and groups._stable_attachment(previous) != groups._stable_attachment(candidate)
            and not reattach):
        raise groups.GroupError("live attachment changed; explicit reattach required")
    return groups.attach_live(workspace, group_key, member_key, candidate,
        expected_revision=view["revision"], expected_participant_id=member["participant_id"], reattach=reattach)


def detach(workspace, group_key, member_key, *, expected_revision=None):
    """Clear declaration only, even when the native artifact is gone or unsafe."""
    view, member = _preflight(workspace, group_key, member_key, expected_revision)
    return groups.detach_live(workspace, group_key, member_key, expected_revision=view["revision"],
                               expected_participant_id=member["participant_id"])


def _row(member):
    attachment = member.get("live_attachment")
    return {"participant_id": member["participant_id"], "key": member["key"], "role": member["role"],
            "live_attachment": deepcopy(attachment),
            "association_basis": "explicit_declaration" if attachment is not None else "unattached",
            "assessment": "unattached", "activity": "unknown", "activity_scope": "legacy_runner_ownership",
            "attention": "unknown", "freshness": "unknown", "expires_in_ms": 0, "source": None,
            "diagnostics": ["live_source_unattached"], "authorization": "none", "dispatchable": False,
            "native_lifecycle": "unestablished", "job_completion": "unestablished"}


def _unknown(row, diagnostic, assessment="unavailable"):
    row.update(assessment=assessment, diagnostics=[diagnostic], activity="unknown",
               freshness="unknown", expires_in_ms=0, source=None)


def status(workspace, group_key, *, member_key=None, timeout=1.0):
    """Read-only status; group edits/deadline expiry invalidate every positive row."""
    _timeout(timeout)
    groups._selector(group_key)
    if member_key is not None:
        groups._selector(member_key)
    started = time.monotonic()
    deadline = started + timeout
    started_at = datetime.datetime.now(datetime.timezone.utc).isoformat()
    view = groups.read_groups(workspace)
    group = groups._group(view["state"], group_key)
    members = group["members"] if member_key is None else [_member(group, member_key)]
    report = {"schema_version": 1, "contract": STATUS_CONTRACT, "assessment": "unavailable",
              "authority_schema_version": view["state"]["schema_version"], "revision": view["revision"],
              "group": {k: group[k] for k in ("group_id", "key", "name")}, "participants": [],
              "started_at": started_at, "diagnostics": [], "authorization": "none", "dispatchable": False,
              "native_lifecycle": "unestablished", "job_completion": "unestablished",
              "limitations": list(LIMITATIONS)}
    rows = []
    timed_out = False
    for member in members:
        row = _row(member)
        call_started = time.monotonic()
        remaining = deadline - call_started
        if remaining <= 0:
            timed_out = True
            break
        attachment = member.get("live_attachment")
        if attachment is not None:
            try:
                result = live.inspect(attachment["artifact_path"], attachment["runtime_id"],
                    attachment["instance_id"], attachment["session_id"], timeout=min(1.0, remaining))
                wire._require(isinstance(result, dict))
                assessment = result.get("assessment")
                if assessment == "observed":
                    source = _accepted(result, attachment)
                    row.update(assessment="observed", activity=result["activity"], freshness="fresh_sample",
                               expires_in_ms=result["expires_in_ms"], source=source, diagnostics=[])
                else:
                    _unknown(row, "live_source_unavailable",
                             "incompatible" if assessment == "incompatible" else "unavailable")
                    # Reader diagnostics are safe codes; do not project arbitrary
                    # exception messages or a native artifact body into group status.
                    codes = result.get("diagnostics")
                    allowed = {"schema_error", "unsupported_contract", "runtime_mismatch", "instance_not_observed",
                        "session_not_observed", "unsafe_directory", "directory_changed", "unsafe_artifact",
                        "response_limit", "artifact_changed", "timeout", "future_sample", "stale_sample",
                        "unsupported_platform", "io_error"}
                    if isinstance(codes, list) and len(codes) == 1 and isinstance(codes[0], str) and codes[0] in allowed:
                        row["diagnostics"] = list(codes)
            except wire._Failure:
                _unknown(row, "invalid_live_report")
            except (OSError, ValueError):
                _unknown(row, "live_source_unavailable")
        rows.append((row, call_started))
    try:
        current = groups.read_groups(workspace)
        if current["revision"] != view["revision"]:
            report["diagnostics"] = ["groups_changed"]
    except storage.StorageError:
        report["diagnostics"] = ["groups_unavailable"]
    finished = time.monotonic()
    wall_ms = time.time() * 1000
    if timed_out or finished >= deadline:
        report["diagnostics"] = ["status_timeout"]
    if not report["diagnostics"]:
        for row, call_started in rows:
            if row["source"] is None:
                continue
            age = wall_ms - row["source"]["sampled_at_ms"]
            expires = min(row["expires_in_ms"] - math.ceil((finished - call_started) * 1000),
                          math.floor(row["source"]["valid_for_ms"] - age))
            if age < 0:
                _unknown(row, "future_sample")
            elif expires <= 0:
                _unknown(row, "source_expired")
            else:
                row["expires_in_ms"] = expires
        if time.monotonic() >= deadline:
            report["diagnostics"] = ["status_timeout"]
        else:
            report.update(assessment="observed", participants=[row for row, _ in rows])
    report["observed_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    final_elapsed = time.monotonic() - started
    if final_elapsed >= timeout:
        report.update(assessment="unavailable", participants=[], diagnostics=["status_timeout"])
    report["duration_ms"] = round(final_elapsed * 1000, 3)
    return report
