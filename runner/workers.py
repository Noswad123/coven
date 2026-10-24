"""Help-only readiness and bounded transport for explicit native observations.

Readiness follows only advertised help routes and never declares compatibility.
The separate fixed session-observation wrapper shares transport, not readiness
semantics: persisted evidence is not a native worker admission contract.
"""

from dataclasses import dataclass
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import time


ADAPTER_CONTRACT = "djinn-help-v1"
SCHEMA_VERSION = 1
MAX_EXECUTABLE_BYTES = 1024 ** 3
ENV_KEYS = frozenset({"PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TERM"})
HELP_PROBE_ALLOWLIST = frozenset({
    ("--help",), ("--version",), ("session", "--help"),
    ("session", "run", "--help"), ("session", "status", "--help"),
    ("session", "watch", "--help"), ("ask", "--help"), ("agents", "--help"),
})
_ANSI = re.compile(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\))")

# A live, isolated supervisor retains the process-group identity until cleanup.
# It closes its own output copies so native EOF remains observable, reports the
# native exit on a private non-inherited descriptor, then waits to be killed.
# This avoids PID reuse and zombie-only killpg behavior across POSIX platforms.
_SUPERVISOR = r'''
import json, os, selectors, signal, subprocess, sys, time
channel = int(sys.argv[1])
parent = int(sys.argv[2])
budget = float(sys.argv[3]) + 1.0
deadline = time.monotonic() + budget
def stop_group(*unused):
    try:
        os.killpg(os.getpgrp(), signal.SIGKILL)
    finally:
        os._exit(125)
signal.signal(signal.SIGALRM, stop_group)
signal.setitimer(signal.ITIMER_REAL, budget)
os.set_inheritable(channel, False)
os.set_inheritable(parent, False)
safe = {key: value for key, value in os.environ.items()
        if key in {'PATH','HOME','SHELL','LANG','LC_ALL','TERM'}}
try:
    child = subprocess.Popen(sys.argv[4:], stdin=subprocess.DEVNULL,
                             stdout=1, stderr=2, env=safe, close_fds=True)
except OSError:
    child = None
os.close(1)
os.close(2)
watcher = selectors.DefaultSelector()
watcher.register(parent, selectors.EVENT_READ)
sent = False
while True:
    remaining = deadline - time.monotonic()
    lost_parent = remaining <= 0
    if not lost_parent:
        events = watcher.select(min(0.05, remaining))
        lost_parent = bool(events) and os.read(parent, 1) == b''
    if lost_parent:
        stop_group()
    code = child.poll() if child is not None else None
    if not sent and (child is None or code is not None):
        payload = {'spawned': child is not None, 'returncode': code}
        try:
            os.write(channel, json.dumps(payload).encode('ascii'))
        except OSError:
            pass
        os.close(channel)
        sent = True
'''


@dataclass(frozen=True)
class ProbeResult:
    status: str
    returncode: int | None
    stdout: bytes
    stderr: bytes
    duration_ms: float


def _validate_limits(timeout, max_output_bytes):
    if (isinstance(timeout, bool) or not isinstance(timeout, (int, float))
            or not 0 < timeout <= 30 or not math.isfinite(timeout)):
        raise ValueError("timeout must be finite and in (0, 30]")
    if (type(max_output_bytes) is not int
            or not 1024 <= max_output_bytes <= 1048576):
        raise ValueError("max_output_bytes must be an integer in [1024, 1048576]")


def run_help_probe(executable: Path, arguments: tuple[str, ...], *, timeout,
                    max_output_bytes, cwd, env) -> ProbeResult:
    """Run only an allowlisted observation with bounded combined pipe output.

    This low-level runner permits --version; the adapter additionally requires
    root help to advertise it. A live supervisor keeps process-group cleanup safe
    after native exit, including descendants that closed their output pipes.
    """
    _validate_limits(timeout, max_output_bytes)
    if (not isinstance(arguments, tuple) or not all(isinstance(a, str) for a in arguments)
            or arguments not in HELP_PROBE_ALLOWLIST):
        raise ValueError("arguments are not an allowlisted help probe")
    return _run_bounded_probe(executable, arguments, timeout=timeout,
                              max_output_bytes=max_output_bytes, cwd=cwd, env=env)


def _observation_folder(folder):
    if (not isinstance(folder, str) or not folder.startswith("/")
            or folder.endswith("/") or ".." in folder.split("/")
            or any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in folder)):
        raise ValueError("an explicit absolute folder without parent components or controls is required")
    try:
        if len(folder.encode("utf-8")) > 4096:
            raise ValueError("folder exceeds 4096 UTF-8 bytes")
    except UnicodeError:
        raise ValueError("folder must be valid UTF-8") from None


def run_session_observation_probe(executable: Path, folder: str, *, timeout,
                                  max_output_bytes, cwd, env) -> ProbeResult:
    """Only the documented persisted-evidence route, never status/help fallback."""
    _validate_limits(timeout, max_output_bytes)
    _observation_folder(folder)
    return _run_bounded_probe(executable, ("session", "observe", folder, "--json"),
                              timeout=timeout, max_output_bytes=max_output_bytes, cwd=cwd, env=env)


def _run_bounded_probe(executable, arguments, *, timeout, max_output_bytes, cwd, env):
    """Private transport shared by independently allowlisted observation wrappers."""
    if not isinstance(executable, Path) or not executable.is_absolute():
        raise ValueError("executable must be an absolute Path")
    if not isinstance(env, dict) or not set(env) <= ENV_KEYS:
        raise ValueError("environment must contain only safe whitelist keys")
    started = time.monotonic()
    if signal.getsignal(signal.SIGCHLD) != signal.SIG_DFL:
        return ProbeResult("spawn_error", None, b"", b"", 0.0)
    output = {"stdout": bytearray(), "stderr": bytearray()}
    process = None
    native_code = None
    channel_read = channel_write = parent_read = parent_write = None
    channel_data = bytearray()
    native_done = False
    selector = selectors.DefaultSelector()
    status = "spawn_error"
    try:
        channel_read, channel_write = os.pipe()
        parent_read, parent_write = os.pipe()
        process = subprocess.Popen(
            [sys.executable, "-I", "-S", "-c", _SUPERVISOR, str(channel_write), str(parent_read), str(timeout), str(executable), *arguments], stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, cwd=cwd, env=env,
            close_fds=True, start_new_session=True, pass_fds=(channel_write, parent_read),
        )
        os.close(channel_write)
        channel_write = None
        os.close(parent_read)
        parent_read = None
        for name, pipe in (("stdout", process.stdout), ("stderr", process.stderr)):
            os.set_blocking(pipe.fileno(), False)
            selector.register(pipe, selectors.EVENT_READ, name)
        os.set_blocking(channel_read, False)
        selector.register(channel_read, selectors.EVENT_READ, "control")
        size = 0
        deadline = started + timeout
        while True:
            if not selector.get_map():
                status = "native_exited" if native_done else "spawn_error"
                break
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                status = "timeout"
                break
            events = selector.select(min(remaining, 0.05))
            for key, _ in events:
                try:
                    chunk = os.read(key.fd, 1024 if key.data == "control" else min(65536, max_output_bytes - size + 1))
                except BlockingIOError:
                    continue
                if not chunk:
                    selector.unregister(key.fileobj)
                    if key.data == "control":
                        os.close(channel_read)
                        channel_read = None
                        message = json.loads(channel_data)
                        if (not isinstance(message, dict) or set(message) != {"spawned", "returncode"}
                                or message["spawned"] is not True or type(message["returncode"]) is not int):
                            raise ValueError("invalid supervisor completion")
                        native_code = message["returncode"]
                        native_done = True
                    else:
                        key.fileobj.close()
                    continue
                if key.data == "control":
                    channel_data.extend(chunk)
                    if len(channel_data) > 1024:
                        raise ValueError("invalid supervisor completion size")
                    continue
                available = max_output_bytes - size
                output[key.data].extend(chunk[:available])
                size += min(len(chunk), available)
                if len(chunk) > available:
                    status = "output_limit"
                    break
            if status == "output_limit":
                break
    except (OSError, ValueError):
        # Exception strings and native output may include secrets; never echo them.
        status = "spawn_error"
    finally:
        if process is not None:
            # Also terminate descendants after successful/nonzero leader exits,
            # even when they closed both pipes. The supervisor remains live.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            except OSError:
                status = "spawn_error"  # Cleanup could not be confirmed.
            for pipe in (process.stdout, process.stderr):
                if pipe is not None:
                    try:
                        pipe.close()
                    except OSError:
                        status = "spawn_error"
            try:
                process.wait(timeout=1.0)
            except subprocess.TimeoutExpired:
                status = "spawn_error"
                try:
                    process.kill()
                    process.wait(timeout=1.0)
                except (OSError, subprocess.TimeoutExpired):
                    pass
            except OSError:
                status = "spawn_error"
            if status == "native_exited":
                status = "ok" if native_code == 0 else "nonzero"
        for descriptor in (channel_read, channel_write, parent_read, parent_write):
            if descriptor is not None:
                try:
                    os.close(descriptor)
                except OSError:
                    pass
        selector.close()
    return ProbeResult(status, native_code,
                       bytes(output["stdout"]), bytes(output["stderr"]),
                       round((time.monotonic() - started) * 1000, 3))


def _metadata(path):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or not os.access(path, os.X_OK):
        raise OSError("not a regular executable")
    return {"device": info.st_dev, "inode": info.st_ino, "size": info.st_size,
            "mtime_ns": info.st_mtime_ns}


def _identity(path):
    before = _metadata(path)
    if before["size"] > MAX_EXECUTABLE_BYTES:
        raise OSError("executable exceeds identity limit")
    digest = hashlib.sha256()
    # O_NOFOLLOW prevents a replaced final path from redirecting the read.
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        if (not stat.S_ISREG(info.st_mode)
                or (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns)
                != tuple(before[key] for key in ("device", "inode", "size", "mtime_ns"))):
            raise OSError("executable changed")
        size = 0
        while True:
            chunk = stream.read(min(1024 * 1024, MAX_EXECUTABLE_BYTES - size + 1))
            if not chunk:
                break
            size += len(chunk)
            if size > MAX_EXECUTABLE_BYTES:
                raise OSError("executable exceeds identity limit")
            digest.update(chunk)
        after = os.fstat(stream.fileno())
        if (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns) != (
                info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns):
            raise OSError("executable changed")
    if size != before["size"] or _metadata(path) != before:
        raise OSError("executable changed")
    return {**before, "sha256": digest.hexdigest()}


def _parse_help(data, route):
    """Extract only entries inside complete Clap-style help sections."""
    text = _ANSI.sub("", data.decode("utf-8", errors="strict"))
    if (not text.endswith("\n") or "\x1b" in text
            or any(ord(c) < 32 and c not in "\n\r\t" for c in text)):
        raise ValueError("incomplete or invalid help")
    lines = text.splitlines()
    usage = []
    sections = {}
    current = None
    for line in lines:
        match = re.fullmatch(r"Usage:\s+(.+)", line)
        if match:
            usage.append(match.group(1))
            current = None
        elif re.fullmatch(r"[A-Za-z][A-Za-z /-]*:", line):
            current = line[:-1]
            if current in sections:
                raise ValueError("duplicate help section")
            sections[current] = []
        elif current is not None:
            sections[current].append(line)
    if len(usage) != 1 or not ({"Commands", "Options"} & sections.keys()):
        raise ValueError("missing help structure")
    tokens = usage[0].split()
    if (not tokens or not re.fullmatch(r"[\w./-]+", tokens[0])
            or tokens[1:1 + len(route)] != list(route)
            or any(not token.startswith(("[", "<"))
                   for token in tokens[1 + len(route):] if token not in {"...", "|"})):
        raise ValueError("unexpected help usage")
    commands = []
    flags = []
    for section, entries in sections.items():
        if section not in {"Commands", "Options"}:
            continue
        for line in entries:
            if not line.strip():
                continue
            if not line.startswith("  "):
                raise ValueError("malformed help section")
            if section == "Commands":
                match = re.fullmatch(r"  ([a-z][a-z0-9-]*)(?: {2,}.*)?", line)
                if match:
                    commands.append(match.group(1))
                elif not line.startswith("    "):
                    raise ValueError("malformed command entry")
            else:
                # Only the option declaration, never prose, advertises flags.
                match = re.match(
                    r"^ {2}(?: {4})?(?:-[A-Za-z0-9?], )?(--[A-Za-z][A-Za-z0-9-]*)(?=[ =\[]|$)",
                    line,
                )
                if match:
                    flags.append(match.group(1))
                elif not line.startswith("    "):
                    raise ValueError("malformed option entry")
    if (("Commands" in sections and not commands)
            or ("Options" in sections and not flags)):
        raise ValueError("empty help section")
    return {"state": "advertised", "evidence_source": "help_only",
            "commands": sorted(set(commands)), "flags": sorted(set(flags)),
            "help_sha256": hashlib.sha256(data).hexdigest()}


def inspect_readiness(workspace: Path, *, adapter="djinn", profile="research-readonly",
                      executable: str | None = None, timeout: float = 3.0,
                      max_output_bytes: int = 131072, probe_runner=None) -> dict:
    """Observe advertised help only. Never grant worker or policy authorization."""
    _validate_limits(timeout, max_output_bytes)
    if adapter != "djinn" or profile != "research-readonly":
        raise ValueError("unsupported adapter or profile")
    if not isinstance(workspace, Path) or not workspace.is_dir():
        raise ValueError("workspace must be an existing directory Path")
    if executable is not None and (not isinstance(executable, str)
                                   or not executable or "\0" in executable):
        raise ValueError("executable must be a nonempty NUL-free path")
    if probe_runner is not None and not callable(probe_runner):
        raise ValueError("probe_runner must be callable")
    cwd = workspace.resolve()
    capability_codes = {
        "worker_contract": "worker_contract_unverified",
        "native_execution_identity": "native_identity_unverified",
        "durable_attempt_correlation": "attempt_correlation_unverified",
        "submission_acknowledgement": "submission_ack_unverified",
        "lost_ack_reconciliation": "reconcile_unverified",
        "lifecycle_including_permission_pause": "lifecycle_unverified",
        "side_effect_free_inspection": "inspection_purity_unverified",
        "result_transcript_references": "result_references_unverified",
        "cancellation": "cancellation_unverified",
        "immutable_policy_admission": "readonly_policy_unverified",
        "resource_scoped_readonly_enforcement": "resource_scope_unverified",
    }
    report = {
        "schema_version": SCHEMA_VERSION, "adapter": adapter,
        "adapter_contract": ADAPTER_CONTRACT, "profile": profile,
        "assessment": "unknown", "authorization": "none", "native_version": None,
        "context": {"workspace": str(cwd), "configuration_inspected": False,
                    "policy_inspected": False, "sessions_inspected": False},
        "notes": ["No supported versioned native worker contract interrogated; "
                  "help does not attest policy.",
                  "Version output, when observed, is not capability attestation."],
        "capabilities": {key: {"state": "unknown", "evidence_source": "unverified"}
                         for key in capability_codes},
        "cli_surfaces": {key: {"state": "unknown", "evidence_source": "unverified"}
                         for key in ("root", "session", "session run", "session status",
                                     "session watch", "ask", "agents", "version")},
        "executable": {"selected_path": None, "resolved_path": None, "identity": None},
        "probes": [], "blockers": [],
    }

    def block(code, next_step):
        if not any(item["code"] == code for item in report["blockers"]):
            report["blockers"].append({"code": code, "next_step": next_step})

    guidance = {
        "worker_contract": "Require a documented versioned native worker contract and a matching Coven adapter; do not invent commands from help.",
        "native_execution_identity": "Require stable native execution/session references in submission or reconciliation, not pane IDs or PIDs.",
        "durable_attempt_correlation": "Require persisted caller-supplied attempt identity and a supported lookup for that identity.",
        "submission_acknowledgement": "Require attempt-bound acceptance/rejection; a spawned process is not accepted work.",
        "lost_ack_reconciliation": "Require recoverable lookup or idempotent admission before any automatic resubmission.",
        "lifecycle_including_permission_pause": "Require typed native lifecycle facts that distinguish permission pauses, completion, failure, and unknown state.",
        "side_effect_free_inspection": "Require a documented non-mutating inspection route; command names such as status do not prove purity.",
        "result_transcript_references": "Require result/transcript pointers bound to the admitted attempt, not exit-code or stdout success alone.",
        "cancellation": "Require cancellation acknowledgement and separate termination observation; deletion is not cancellation.",
        "immutable_policy_admission": "Require accepted immutable per-attempt tool policy and digest; role names or read-only prompts are not evidence.",
        "resource_scoped_readonly_enforcement": "Require native enforcement of the attempt's exact resource/path limits, not unrestricted default reads or shell access.",
    }
    for capability, code in capability_codes.items():
        block(code, guidance[capability])
    selected = executable if executable is not None else shutil.which("djinn")
    if selected is None:
        block("executable_missing", "Install Djinn or select an existing executable path.")
        return report
    try:
        selected_path = Path(selected).expanduser().absolute()
    except (OSError, RuntimeError, ValueError):
        block("executable_unverified", "Select an explicit trusted executable path with a resolvable home directory.")
        return report
    report["executable"]["selected_path"] = str(selected_path)
    try:
        resolved = selected_path.resolve(strict=True)
        report["executable"]["resolved_path"] = str(resolved)
        identity = _identity(resolved)
        report["executable"]["identity"] = identity
    except (OSError, RuntimeError, ValueError):
        block("executable_missing" if not selected_path.exists() else "executable_unverified",
              "Select a stable, regular executable readable within the 1 GiB identity limit.")
        return report
    runner = probe_runner or run_help_probe
    env = {key: value for key, value in os.environ.items() if key in ENV_KEYS}
    failed = False
    changed = False

    def probe(arguments, route):
        nonlocal failed, changed
        try:
            if (selected_path.resolve(strict=True) != resolved
                    or _metadata(resolved) != {k: v for k, v in identity.items() if k != "sha256"}):
                raise OSError("executable changed")
        except (OSError, RuntimeError):
            changed = True
            return None
        observed = datetime.now(timezone.utc).isoformat()
        try:
            result = runner(executable=resolved, arguments=arguments, timeout=timeout,
                            max_output_bytes=max_output_bytes, cwd=cwd, env=env)
            if (not isinstance(result, ProbeResult)
                    or result.status not in {"ok", "nonzero", "timeout", "output_limit", "spawn_error"}
                    or not isinstance(result.stdout, bytes) or not isinstance(result.stderr, bytes)
                    or (result.returncode is not None and type(result.returncode) is not int)
                    or isinstance(result.duration_ms, bool) or not isinstance(result.duration_ms, (float, int))
                    or not math.isfinite(result.duration_ms) or result.duration_ms < 0):
                raise ValueError("invalid probe result")
        except Exception:
            result = ProbeResult("spawn_error", None, b"", b"", 0.0)
        stdout, stderr = result.stdout, result.stderr
        status = result.status
        if len(stdout) + len(stderr) > max_output_bytes:
            stdout = stdout[:max_output_bytes]
            stderr = stderr[:max_output_bytes - len(stdout)]
            status = "output_limit"
        provenance = {
            "id": f"probe-{len(report['probes']) + 1}", "argv": [str(resolved), *arguments],
            "observed_at": observed, "status": status, "returncode": result.returncode,
            "output_size": len(stdout) + len(stderr),
            "stdout_size": len(stdout), "stderr_size": len(stderr),
            "stdout_sha256": hashlib.sha256(stdout).hexdigest(),
            "stderr_sha256": hashlib.sha256(stderr).hexdigest(),
            "output_sha256": hashlib.sha256(stdout + stderr).hexdigest(),
            "duration_ms": result.duration_ms,
        }
        report["probes"].append(provenance)
        if status != "ok" or result.returncode != 0:
            failed = True
            provenance["diagnostic_excerpt"] = "Probe failed: " + status + ". Native output withheld."
            block("probe_failure", "Check the selected executable and probe bounds; retry help only.")
            return None
        try:
            # Non-UTF8 stderr is not credible even if stdout resembles help.
            stderr.decode("utf-8", errors="strict")
            if arguments == ("--version",):
                version = _ANSI.sub("", stdout.decode("utf-8", errors="strict")).strip()
                if len(version) > 256 or not re.fullmatch(
                        r"(?:djinn\s+)?v?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", version):
                    raise ValueError("unrecognized version")
                report["native_version"] = version
                surface = {"state": "observed", "evidence_source": "help_only",
                           "attestation": False}
            else:
                surface = _parse_help(stdout, route)
            surface["probe_id"] = provenance["id"]
            report["cli_surfaces"][" ".join(route) if route else (
                "version" if arguments == ("--version",) else "root")] = surface
            return surface
        except (ValueError, UnicodeError):
            failed = True
            provenance["parse_status"] = "error"
            provenance["diagnostic_excerpt"] = "Unrecognized or incomplete help/version text; output withheld."
            block("probe_parse_error", "Verify complete UTF-8 Clap-style help for this adapter route.")
            return None

    root = probe(("--help",), ())
    missing_route = False
    if root is not None:
        for command in ("session", "ask", "agents"):
            if command not in root["commands"]:
                report["cli_surfaces"][command] = {
                    "state": "not_advertised", "evidence_source": "help_only",
                    "probe_id": root["probe_id"],
                }
                if command == "session":
                    missing_route = True
                continue
            surface = probe((command, "--help"), (command,))
            if command == "session" and surface is not None:
                for child in ("run", "status", "watch"):
                    if child in surface["commands"]:
                        probe(("session", child, "--help"), ("session", child))
                    else:
                        report["cli_surfaces"]["session " + child] = {
                            "state": "not_advertised", "evidence_source": "help_only",
                            "probe_id": surface["probe_id"],
                        }
                        if child == "run":
                            missing_route = True
        if "--version" in root["flags"]:
            probe(("--version",), ())
        else:
            report["cli_surfaces"]["version"] = {
                "state": "not_advertised", "evidence_source": "help_only",
                "probe_id": root["probe_id"],
            }
    try:
        final_identity = _identity(resolved)
        report["executable"]["identity_after"] = final_identity
        if selected_path.resolve(strict=True) != resolved or final_identity != identity:
            changed = True
    except (OSError, RuntimeError, ValueError):
        changed = True
    if changed:
        block("executable_changed", "Keep the selected executable and alias stable, then retry.")
    if missing_route:
        block("unsupported_advertised_session_route",
              "Select an adapter/executable advertising session run; do not try bare fallback commands.")
    if missing_route and not failed and not changed:
        report["assessment"] = "blocked"
    return report


def report_exit_code(report) -> int:
    """Compatible is reserved for future contract adapters, never emitted here."""
    return {"compatible": 0, "blocked": 1, "unknown": 2}.get(report.get("assessment"), 2)
