"""Frozen COLD, unrestricted runtime transport, not task dispatch or readiness.

Plans contain shell program strings intentionally. Successful transport is not a
worker readiness, native session identity, or read-only sandbox guarantee.
"""

import json
import math
import posixpath
import re
import subprocess
import time


MAX_PLAN_BYTES = 8 * 1024 * 1024
ENV_KEYS = frozenset({"PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TERM"})


def _text(value, field, *, nonblank=True):
    if not isinstance(value, str) or "\0" in value or (nonblank and not value.strip()):
        raise ValueError(f"{field} must be NUL-free {'nonblank ' if nonblank else ''}text")
    try:
        value.encode("utf-8")
    except UnicodeError as exc:
        raise ValueError(f"{field} must be valid UTF-8 text") from exc
    return value


def _serialized(value):
    try:
        encoded = json.dumps(value, sort_keys=True, ensure_ascii=False, allow_nan=False)
        if len(encoded.encode("utf-8")) > MAX_PLAN_BYTES:
            raise ValueError("plan exceeds 8 MiB")
        return encoded
    except (TypeError, UnicodeError, RecursionError) as exc:
        raise ValueError("plan must be bounded JSON data") from exc


def build_plan(root: str, namespace: str, multiplexer: str,
               agents: list[dict], environment: dict[str, str]) -> dict:
    """Build a deterministic JSON plan without filesystem or ambient env access."""
    _text(root, "root")
    if not posixpath.isabs(root):
        raise ValueError("root must be absolute")
    if not isinstance(namespace, str) or not re.fullmatch(r"coven-[0-9a-fA-F]+-[0-9a-fA-F]+", namespace):
        raise ValueError("namespace must be coven-<hex>-<hex>")
    if multiplexer not in ("tmux", "herdr"):
        raise ValueError("multiplexer must be tmux or herdr")
    if not isinstance(agents, list) or not 1 <= len(agents) <= 32:
        raise ValueError("agents must contain 1 through 32 entries")
    roster = []
    seen = set()
    for agent in agents:
        if not isinstance(agent, dict) or set(agent) != {"id", "command", "starter"}:
            raise ValueError("agent must contain exactly id, command, starter")
        agent_id = agent["id"]
        if not isinstance(agent_id, str) or not re.fullmatch(r"[A-Za-z0-9_-]{1,128}", agent_id):
            raise ValueError("unsafe agent ID")
        if agent_id in seen:
            raise ValueError("duplicate agent ID")
        seen.add(agent_id)
        roster.append({"id": agent_id, "command": _text(agent["command"], "command"),
                       "starter": _text(agent["starter"], "starter")})
    if not isinstance(environment, dict) or not set(environment) <= ENV_KEYS:
        raise ValueError("environment contains unsupported keys")
    env = {}
    for key, value in environment.items():
        _text(value, f"environment.{key}", nonblank=False)
        if len(value.encode("utf-8")) > 32768:
            raise ValueError("environment value exceeds 32 KiB")
        env[key] = value

    steps = []

    def run(argv, outputs=None):
        step = {"operation": "run", "argv": argv}
        if outputs is not None:
            step["outputs"] = outputs
        steps.append(step)

    if multiplexer == "tmux":
        target = f"={namespace}"
        steps.append({"operation": "ensure_absent", "argv": ["tmux", "has-session", "-t", target]})
        run(["tmux", "new-session", "-d", "-s", namespace, "-n", "orchestrator", "-c", root, "/bin/zsh"])
        for agent in roster:
            pane = f"={namespace}:=agent-{agent['id']}"
            run(["tmux", "new-window", "-d", "-t", target, "-n", "agent-" + agent["id"], "-c", root, agent["command"]])
            steps.append({"operation": "sleep", "seconds": 5})
            run(["tmux", "send-keys", "-t", pane, "-l", "--", agent["starter"]])
            run(["tmux", "send-keys", "-t", pane, "Enter"])
    else:
        workspace = {"handle": "workspace"}
        steps.append({"operation": "ensure_absent", "argv": ["herdr", "workspace", "list"]})
        run(["herdr", "workspace", "create", "--cwd", root, "--label", namespace, "--no-focus"],
            {"workspace": ["result", "workspace", "workspace_id"],
             "orchestrator": ["result", "root_pane", "pane_id"]})
        run(["herdr", "pane", "run", {"handle": "orchestrator"}, "/bin/zsh"])
        for index, agent in enumerate(roster):
            name = f"agent_{index}"
            pane = {"handle": name}
            run(["herdr", "tab", "create", "--workspace", workspace, "--cwd", root,
                  "--label", "agent-" + agent["id"], "--no-focus"], {name: ["result", "root_pane", "pane_id"]})
            run(["herdr", "pane", "run", pane, agent["command"]])
            steps.append({"operation": "sleep", "seconds": 5})
            run(["herdr", "pane", "send-text", pane, agent["starter"]])
            run(["herdr", "pane", "send-keys", pane, "enter"])
    plan = {"version": 1, "operation": "runtime_launch", "root": root, "cwd": root,
            "namespace": namespace, "multiplexer": multiplexer, "agents": roster,
            "env": env, "steps": steps}
    _serialized(plan)
    return plan


def validate_plan(plan) -> dict:
    """Reject any noncanonical field/step; return an independent frozen snapshot.

    This checks structure, not approval authority. Changing input fields creates
    a different valid plan that the caller must separately authorize.
    """
    encoded = _serialized(plan)
    if not isinstance(plan, dict) or set(plan) != {
        "version", "operation", "root", "cwd", "namespace", "multiplexer", "agents", "env", "steps"
    }:
        raise ValueError("invalid plan fields")
    expected = build_plan(plan["root"], plan["namespace"], plan["multiplexer"], plan["agents"], plan["env"])
    if encoded != _serialized(expected):
        raise ValueError("plan differs from deterministic runtime_launch v1 plan")
    return expected


def _json_output(text):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("duplicate JSON key")
            result[key] = value
        return result

    def constant(value):
        raise ValueError("nonfinite JSON number")

    def finite_float(value):
        number = float(value)
        if not math.isfinite(number):
            raise ValueError("nonfinite JSON number")
        return number

    return json.loads(text, object_pairs_hook=pairs, parse_constant=constant, parse_float=finite_float)


def _handle(value):
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9_-]{1,128}", value):
        raise ValueError("missing or unsafe native handle")
    return value


def _tmux_present(result, namespace):
    if result.returncode == 0:
        return True
    detail = str(result.stderr or "").strip()
    lower = detail.lower()
    absent = (detail in {f"can't find session: {namespace}", f"can't find session: ={namespace}"}
              or lower.startswith("no server running on ")
              or (lower.startswith(("error connecting to ", "failed to connect to "))
                  and "no such file or directory" in lower))
    if result.returncode == 1 and absent:
        return False
    raise ValueError(f"tmux presence query failed ({result.returncode}): {detail[:2048] or 'no established absence response'}")


def _transport(plan, runner, sleeper, *, stopping):
    report = {"operation": "runtime_stop" if stopping else "runtime_launch",
               "status": "transport_failed_or_partial", "completed_steps": 0,
               "handles": {}, "diagnostics": [], "outcome_unknown": False}
    if stopping:
        report.update(stop_command_attempted=False, stop_command_succeeded=False)
    current = None
    try:
        frozen = validate_plan(plan)
        runner = subprocess.run if runner is None else runner
        sleeper = time.sleep if sleeper is None else sleeper
        backend, namespace = frozen["multiplexer"], frozen["namespace"]
        steps = frozen["steps"]
        if stopping:
            steps = [{**steps[0], "operation": "ensure_present"},
                     {"operation": "run", "argv": (
                         ["tmux", "kill-session", "-t", f"={namespace}"] if backend == "tmux" else
                         ["herdr", "workspace", "close", {"handle": "workspace"}])}]
        for index, step in enumerate(steps):
            current = index
            operation = step["operation"]
            if operation == "sleep":
                sleeper(step["seconds"])
            else:
                argv = [report["handles"][arg["handle"]] if isinstance(arg, dict) else arg
                        for arg in step["argv"]]
                if stopping and operation == "run":
                    report["stop_command_attempted"] = True
                result = runner(argv, env=dict(frozen["env"]), cwd=frozen["cwd"],
                                capture_output=True, text=True, check=False, timeout=60)
                if operation in ("ensure_absent", "ensure_present"):
                    if backend == "tmux":
                        present = _tmux_present(result, namespace)
                    else:
                        if result.returncode != 0:
                            raise ValueError(f"workspace list returned {result.returncode}")
                        workspaces = _json_output(result.stdout)["result"]["workspaces"]
                        if not isinstance(workspaces, list) or any(not isinstance(w, dict) for w in workspaces):
                            raise ValueError("workspaces must be a list of objects")
                        matches = [w for w in workspaces if w.get("label") == namespace]
                        if len(matches) > 1:
                            raise ValueError("ambiguous scoped workspace label")
                        present = bool(matches)
                        if stopping and present:
                            report["handles"]["workspace"] = _handle(matches[0].get("workspace_id"))
                    if not stopping and present:
                        raise ValueError("scoped namespace already exists; no adoption or replacement")
                    if stopping and not present:
                        report["completed_steps"] += 1
                        break
                else:
                    if result.returncode != 0:
                        raise ValueError(f"command returned {result.returncode}: {str(result.stderr or '')[:2048]}")
                    if stopping:
                        report["stop_command_succeeded"] = True
                    if "outputs" in step:
                        payload = _json_output(result.stdout)
                        for name, path in step["outputs"].items():
                            value = payload
                            for key in path:
                                value = value[key]
                            report["handles"][name] = _handle(value)
            report["completed_steps"] += 1
        report["status"] = "transport_completed"
    except (subprocess.TimeoutExpired, OSError) as exc:
        report["outcome_unknown"] = True
        report["diagnostics"].append({"step": current, "error": type(exc).__name__,
                                      "message": "transport failed; native effects unknown"})
    except Exception as exc:
        report["diagnostics"].append({"step": current, "error": type(exc).__name__, "message": str(exc)[:2048]})
    if stopping and report.get("stop_command_attempted") and not report.get("stop_command_succeeded"):
        report["outcome_unknown"] = True
    return report


def execute(plan, runner=None, sleeper=None) -> dict:
    """Execute only the validated plan, with no retry, attach, focus, or cleanup."""
    return _transport(plan, runner, sleeper, stopping=False)


def stop(plan, runner=None) -> dict:
    """Explicit operator control; transport completion does not prove worker death."""
    return _transport(plan, runner, None, stopping=True)
