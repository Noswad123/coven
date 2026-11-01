"""Disposable local mixed writers; only children created here are terminated."""
import argparse
import os
from pathlib import Path
import queue
import select
import stat
import subprocess
import sys
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from runner import storage


def payload(kind, index):
    return f"{kind}{index:06d}:".encode() + kind.encode() * 262144


def main(probe):
    counts = {"python": 0, "rust": 0, "observations": 0,
              "python_observations": 0, "rust_observations": 0, "excluded": 0}
    with tempfile.TemporaryDirectory(prefix="replacement-writers-") as temporary:
        root = Path(temporary)
        (root / "coven.json").write_text("marker")
        target = root / "state/snapshot"
        lock = root / "state/.workspace.lock"
        with storage.workspace_lock(root, lock):
            storage.atomic_replace_bytes(target, payload("P", 0))
            counts["python"] += 1
            target.chmod(0o6750)
        identity = (lock.stat().st_dev, lock.stat().st_ino)
        stop = threading.Event()
        errors = queue.Queue()

        def observe():
            try:
                while not stop.is_set():
                    # Inspect mode and bytes on one opened inode across renames.
                    with target.open("rb") as stream:
                        data = stream.read()
                        mode = stat.S_IMODE(os.fstat(stream.fileno()).st_mode)
                    assert mode == 0o6750, oct(mode)
                    kind = chr(data[0])
                    assert kind in ("P", "R")
                    index = int(data[1:7])
                    assert 0 <= index <= 64
                    assert data == payload(kind, index), "torn snapshot"
                    counts["observations"] += 1
                    counts["python_observations" if kind == "P" else "rust_observations"] += 1
            except BaseException as error:
                errors.put(error)
                stop.set()

        observer = threading.Thread(target=observe, daemon=True)
        observer.start()

        def child():
            return subprocess.Popen(
                [probe, "--exact", "unix::native_replacement_probe", "--nocapture"],
                cwd=ROOT, env={**os.environ, "COVEN_REPLACEMENT_PROBE": str(root)},
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                bufsize=0,
            )

        def ready(process, wanted):
            buffered = b""
            while True:
                assert select.select([process.stdout], [], [], 10)[0], "probe timeout"
                data = os.read(process.stdout.fileno(), 4096)
                assert data, "probe exited before readiness"
                buffered += data
                if wanted.encode() in [line.strip() for line in buffered.splitlines()]:
                    return

        def finish(process):
            if process.poll() is None:
                # This handle is always our own disposable probe, never a shell.
                process.kill()
            process.communicate(timeout=10)

        def python_excluded():
            try:
                with storage.workspace_lock(root, lock, timeout=0):
                    raise AssertionError("Python acquired native-owned lock")
            except storage.LockBusy:
                counts["excluded"] += 1

        try:
            for round_index in range(4):
                process = child()
                try:
                    ready(process, "LOCKED")
                    python_excluded()
                    for index in range(round_index * 16 + 1, round_index * 16 + 17):
                        process.stdin.write(f"{index}\n".encode())
                        process.stdin.flush()
                        ready(process, f"REPLACED {index}")
                        counts["rust"] += 1
                        assert target.read_bytes() == payload("R", index)
                    # A separate native process also remains excluded after rename.
                    competitor = child()
                    try:
                        ready(competitor, "BUSY")
                        assert competitor.wait(timeout=10) == 0
                        counts["excluded"] += 1
                    finally:
                        finish(competitor)
                    python_excluded()
                    assert (lock.stat().st_dev, lock.stat().st_ino) == identity
                    process.stdin.write(b"release\n")
                    process.stdin.flush()
                    assert process.wait(timeout=10) == 0
                finally:
                    finish(process)
                with storage.workspace_lock(root, lock, timeout=0):
                    competitor = child()
                    try:
                        ready(competitor, "BUSY")
                        assert competitor.wait(timeout=10) == 0
                        counts["excluded"] += 1
                    finally:
                        finish(competitor)
                    for index in range(round_index * 16 + 1, round_index * 16 + 17):
                        storage.atomic_replace_bytes(target, payload("P", index))
                        counts["python"] += 1
                        assert target.read_bytes() == payload("P", index)
                    assert (lock.stat().st_dev, lock.stat().st_ino) == identity
        finally:
            stop.set()
            observer.join(timeout=10)
        assert not observer.is_alive()
        if not errors.empty():
            raise errors.get()
        assert counts["observations"] > 0
        assert counts["python_observations"] > 0
        assert counts["rust_observations"] > 0
        assert stat.S_IMODE(target.stat().st_mode) == 0o6750
        assert (lock.stat().st_dev, lock.stat().st_ino) == identity
        assert not list(target.parent.glob(".snapshot.*.tmp"))
    print("mixed replacement interop passed: " + ", ".join(f"{key}={value}" for key, value in counts.items()))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--probe", required=True)
    main(parser.parse_args().probe)
