"""Temporary readiness parity oracle; no installed adapter or real subprocess."""
import contextlib
import io
import json
from pathlib import Path
import sys
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from runner import coven, workers

if sys.argv[1:] == ["--parse"]:
    results = []
    for item in json.load(sys.stdin):
        try:
            results.append(workers._parse_help(bytes(item["data"]), tuple(item["route"])))
        except (ValueError, UnicodeError):
            results.append(None)
    print(json.dumps(results, ensure_ascii=False))
    raise SystemExit(0)

workspace, executable, case = sys.argv[1:]


def help_text(route=(), commands=(), flags=("--help",)):
    lines = ["Usage: djinn " + " ".join((*route, "[OPTIONS]"))]
    if commands:
        lines += ["", "Commands:", *("  " + name + "  Description" for name in commands)]
    lines += ["", "Options:", *("  " + name + "  Description" for name in flags), ""]
    return "\n".join(lines).encode()


def probe(**kwargs):
    args = kwargs["arguments"]
    route = args[:-1]
    commands = ("session", "ask", "agents") if not route else (
        ("run", "status", "watch") if route == ("session",) else ())
    flags = ("--help", "--version") if case == "version" and not route else ("--help",)
    if args == ("--version",):
        data = b"djinn 1.2.3-rc.1+build\n"
    elif case in {"missing", "missing-failure", "missing-parse"} and not route:
        data = help_text(commands=("ask",))
    elif case == "missing-parse" and route == ("ask",):
        data = b"incomplete\n"
    else:
        data = help_text(route, commands, flags)
    status, code, stderr = "ok", 0, b"warning\n"
    if case == "missing-failure" and route == ("ask",):
        status, code, data = "nonzero", 7, b"TOKEN=private"
    return workers.ProbeResult(status, code, data, stderr, 1.0)


with mock.patch.object(workers, "run_help_probe", side_effect=probe):
    report = workers.inspect_readiness(Path(workspace), executable=executable)
    human = io.StringIO()
    with contextlib.redirect_stdout(human):
        code = coven.main(["worker", "readiness", workspace, "--executable", executable])
print(json.dumps({"report": report, "human": human.getvalue().removesuffix("\n"), "exit_code": code}, ensure_ascii=False))
