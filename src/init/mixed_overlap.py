"""Test-only barriers around real, unlocked Python initialization."""
import sys
import time
from pathlib import Path

repository, root, ipc = map(Path, sys.argv[1:])
sys.path.insert(0, str(repository))
from runner import coven


def signal(name):
    (ipc / name).write_text("ready", encoding="utf-8")


def wait(name):
    deadline = time.monotonic() + 15
    while not (ipc / name).exists():
        if time.monotonic() >= deadline:
            raise TimeoutError(name)
        time.sleep(0.01)


original_preflight = coven.preflight_init


def preflight(workspace):
    original_preflight(workspace)
    assert not (workspace / "state").exists()
    signal("preflight-passed")
    wait("continue-preflight")


original_open = Path.open


class MarkerWriter:
    def __init__(self, handle):
        self.handle = handle

    def __enter__(self):
        return self.handle.__enter__()

    def __exit__(self, *args):
        result = self.handle.__exit__(*args)
        signal("marker-published")
        wait("continue-after-marker")
        return result


def open_path(path, *args, **kwargs):
    handle = original_open(path, *args, **kwargs)
    mode = args[0] if args else kwargs.get("mode", "r")
    if path == root / "coven.json" and mode == "x":
        return MarkerWriter(handle)
    return handle


coven.preflight_init = preflight
Path.open = open_path
raise SystemExit(coven.main([
    "init", str(root), "--blank", "--name", "python-owner", "--agents", "Python"
]))
