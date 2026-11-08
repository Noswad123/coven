"""Test-only Python transport oracle. All commands and delays are injected."""
import json
import subprocess
import sys
from types import SimpleNamespace

sys.path.insert(0, sys.argv[1])
from runner import launch


def evaluate(case):
    try:
        plan = launch.build_plan(**case["inputs"])
    except ValueError:
        return {"build_error": True}
    if case.get("invalid_plan"):
        plan["steps"] = []
    calls, delays = [], []
    tabs = 0

    def runner(argv, **kwargs):
        nonlocal tabs
        index = len(calls)
        calls.append({"argv": argv, "cwd": kwargs["cwd"], "env": kwargs["env"],
                      "timeout": kwargs["timeout"]})
        replacement = case.get("overrides", {}).get(str(index))
        if replacement:
            status = replacement.get("status", 0)
            if status == "Timeout":
                raise subprocess.TimeoutExpired(argv, 60)
            if status == "SpawnError":
                raise OSError("injected")
            return SimpleNamespace(returncode=status, stdout=replacement.get("stdout", "{}"),
                                   stderr=replacement.get("stderr", ""))
        if argv[:2] == ["tmux", "has-session"]:
            return SimpleNamespace(returncode=1, stdout="", stderr="can't find session: " + plan["namespace"])
        payload = {}
        if argv[:3] == ["herdr", "workspace", "list"]:
            payload = {"result": {"workspaces": case.get("workspaces", [])}}
        if argv[:3] == ["herdr", "workspace", "create"]:
            payload = {"result": {"workspace": {"workspace_id": "ws_1"}, "root_pane": {"pane_id": "root-1"}}}
        if argv[:3] == ["herdr", "tab", "create"]:
            tabs += 1
            payload = {"result": {"root_pane": {"pane_id": f"pane_{tabs}"}}}
        return SimpleNamespace(returncode=0, stdout=json.dumps(payload), stderr="")

    def sleeper(seconds):
        delays.append(seconds)
        if case.get("sleep_error") or case.get("sleep_failure_at") == len(delays) - 1:
            raise RuntimeError("delay failed")

    report = launch.stop(plan, runner) if case.get("stopping") else launch.execute(plan, runner, sleeper)
    return {"plan": plan, "report": report, "calls": calls, "delays": delays}


print(json.dumps([evaluate(case) for case in json.load(sys.stdin)], ensure_ascii=False, allow_nan=False))
