"""Test-only subprocess execvpe oracle; executes only compiled inert fixtures."""
import json
import subprocess
import sys

sys.path.insert(0, sys.argv[1])
from runner import launch

inputs = json.load(sys.stdin)
calls = []


def runner(argv, **kwargs):
    result = subprocess.run(argv, **kwargs)
    calls.append({"args": argv[1:], "cwd": kwargs["cwd"], "env": kwargs["env"],
                  "code": result.returncode, "stdout": result.stdout, "stderr": result.stderr})
    return result


report = (launch.stop(inputs["plan"], runner) if inputs["stopping"] else
          launch.execute(inputs["plan"], runner, lambda _: None))
print(json.dumps({"report": report, "calls": calls}, allow_nan=False))
