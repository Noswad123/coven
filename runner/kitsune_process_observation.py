"""Opt-in Djinn executable-family evidence, independent of Kitsune classification.

No argv, native references, or process details are persisted. Matching mapping
samples bracket evidence; they do not exclude intervening changes or PID reuse.
"""

import datetime
import os
import time

if __package__:
    from . import kitsune_observation as observation
else:
    import kitsune_observation as observation

ADAPTER_CONTRACT = "coven.kitsune.djinn-process.v1"
LIMITATIONS = (
    "Exact argv[0] basename matching identifies a reported executable family, not "
    "an authenticated binary, interactive agent, native session or activity. All "
    "Djinn modes (including help and maintenance) share executable names.",
    "Only one selected pane's foreground process group is sampled. Missing argv, "
    "Bun/Node launchers, renamed binaries, background and headless work remain "
    "unrecognized. Unknown does not establish absence.",
    "Matching before/after pane mappings are not atomic attribution: intervening "
    "mapping changes, process exit/replacement and PID reuse remain possible. "
    "Process info supplies no terminal identity or runtime generation.",
)


def _pane(snapshot, pane_id):
    matches = [item for item in snapshot["panes"] if item["pane_id"] == pane_id]
    if len(matches) != 1:
        raise observation._Failure("pane_not_found")
    return {key: matches[0][key] for key in observation._LOCATORS}


def _processes(payload, request_id, pane_id):
    """Validate consumed fields; never fall back to name/argv0/cmdline."""
    result = observation._result(payload, request_id, "pane_process_info")
    info = result.get("process_info")
    observation._require(isinstance(info, dict) and info.get("pane_id") == pane_id)
    for key in ("shell_pid", "foreground_process_group_id"):
        if info.get(key) is not None:
            observation._require(type(info[key]) is int and 0 < info[key] <= 2**32 - 1)
    processes = info.get("foreground_processes", [])
    observation._require(isinstance(processes, list) and len(processes) <= 1024)
    matches, pids = [], set()
    for process in processes:
        observation._require(isinstance(process, dict))
        pid = process.get("pid")
        observation._require(type(pid) is int and 0 < pid <= 2**32 - 1 and pid not in pids)
        pids.add(pid)
        observation._string(process.get("name"))
        argv = process.get("argv")
        if argv is None:
            continue
        observation._require(isinstance(argv, list) and len(argv) <= 1024)
        for arg in argv:
            observation._string(arg)
            observation._require("\0" not in arg)
        # Literal POSIX basename only: no shell parsing, case-folding or resolution.
        executable = argv[0].rsplit("/", 1)[-1] if argv else ""
        if executable in {"djinn", "djinn-ui"}:
            matches.append({"pid": pid, "executable_family": executable,
                            "rule": "exact_posix_argv0_basename"})
    return matches


def _pane_id(pane_id):
    if (not isinstance(pane_id, str) or not pane_id or len(pane_id) > 4096
            or not pane_id.isprintable() or any(char.isspace() for char in pane_id)):
        raise ValueError("an exact current pane ID is required")


def _read_evidence(endpoint, pane_id, *, deadline, budget, expected_terminal_id=None):
    """Internal bracketed read with caller-owned endpoint, deadline and budget.

    Publication must pin its selected terminal before the first process query;
    equal bracket samples alone cannot establish that selected association.
    """
    def read(method, params):
        return budget.read(endpoint, method, params, deadline=deadline)

    before = _pane(observation._pane_snapshot(*read("session.snapshot", {})), pane_id)
    if expected_terminal_id is not None and before["terminal_id"] != expected_terminal_id:
        raise observation._Failure("pane_mapping_changed")
    matches = _processes(*read("pane.process_info", {"pane_id": pane_id}), pane_id)
    sampled_at = datetime.datetime.now(datetime.timezone.utc).isoformat()
    after = _pane(observation._pane_snapshot(*read("session.snapshot", {})), pane_id)
    if before != after:
        raise observation._Failure("pane_mapping_changed")
    observation._remaining(deadline)
    return {
        "provenance": "kitsune.pane.process_info",
        "interpreter": ADAPTER_CONTRACT,
        "locators": {"endpoint": dict(endpoint), **before},
        "sample_received_at": sampled_at,
        "mapping_evidence": "matching_bracketing_samples",
        "recognition": "djinn_executable_family" if matches else "unknown",
        "matches": matches,
        "activity": "unknown", "attention": "unknown",
        "native_lifecycle": "unestablished", "job_completion": "unestablished",
    }


def inspect(socket_path: str, pane_id: str, *, timeout: float = 3.0,
            max_response_bytes: int = 1048576) -> dict:
    """Three bounded reads; caller errors raise ValueError, failures are envelopes."""
    observation._arguments(socket_path, timeout, max_response_bytes)
    _pane_id(pane_id)
    started = time.monotonic()
    deadline = started + timeout
    report = {
        "schema_version": 1, "adapter": "kitsune", "adapter_contract": ADAPTER_CONTRACT,
        "endpoint": {"selected": socket_path, "resolved": None, "device": None, "inode": None},
        "request": {"mode": "djinn_process", "pane_id": pane_id},
        "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "authorization": "none", "dispatchable": False,
        "runtime_fact_freshness": "unknown", "server_snapshot_timestamp": None,
        "limitations": list(observation.LIMITATIONS) + list(LIMITATIONS),
        "assessment": "unavailable", "diagnostics": [], "observations": [],
    }
    try:
        parent, leaf = os.path.split(socket_path)
        resolved = os.path.join(os.path.realpath(parent), leaf)
        report["endpoint"]["resolved"] = resolved
        info = observation._endpoint_stat(resolved)
        report["endpoint"].update(device=info.st_dev, inode=info.st_ino)
        item = _read_evidence(report["endpoint"], pane_id, deadline=deadline,
                              budget=observation._ReadBudget(max_response_bytes))
        report.update(assessment="observed", observations=[item])
    except observation._Failure as exc:
        report.update(assessment=exc.assessment, diagnostics=[exc.code])
    except TimeoutError:
        report["diagnostics"] = ["timeout"]
    except OSError:
        report["diagnostics"] = ["io_error"]
    report["observed_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    report["duration_ms"] = round((time.monotonic() - started) * 1000, 3)
    return report
