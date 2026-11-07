"""Mixed native/Python persistent flock behavior, including process death."""
import argparse
import os
from pathlib import Path
import select
import subprocess
import sys
import tempfile

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from runner import storage


def main(probe):
    with tempfile.TemporaryDirectory(prefix="native-lock-") as temporary:
        parent=Path(temporary)
        root=parent/"workspace"
        root.mkdir()
        alias=parent/"alias"
        alias.symlink_to(root,target_is_directory=True)
        (root/"coven.json").write_text("marker")
        lock=root/"state/.workspace.lock"
        def child(selected=root):
            return subprocess.Popen([probe,"--exact","unix::native_lock_probe","--nocapture"],cwd=ROOT,
                env={**os.environ,"COVEN_NATIVE_LOCK_PROBE":str(selected)},stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,stderr=subprocess.PIPE,bufsize=0)
        def ready(process, wanted):
            buffered=b""
            while True:
                assert select.select([process.stdout],[],[],5)[0],"native lock probe not ready"
                data=os.read(process.stdout.fileno(),4096)
                assert data,"native lock probe exited without readiness"
                buffered+=data
                if wanted.encode() in [line.strip() for line in buffered.splitlines()]: return
        # Python ownership excludes native immediately, no PID-based recovery.
        with storage.workspace_lock(root,lock):
            inode=lock.stat().st_ino
            process=child()
            try:
                ready(process,"BUSY")
                assert process.wait(timeout=5)==0
            finally:
                if process.poll() is None: process.kill()
                process.communicate(timeout=5)
        # Native ownership excludes Python and is shared across a root alias.
        process=child(alias)
        try:
            ready(process,"LOCKED")
            try:
                with storage.workspace_lock(root,lock,timeout=0):
                    raise AssertionError("Python acquired a native-owned lock")
            except storage.LockBusy:
                pass
            assert lock.stat().st_ino==inode
            process.stdin.write(b"release\n")
            process.stdin.flush()
            assert process.wait(timeout=5)==0
        finally:
            if process.poll() is None: process.kill()
            process.communicate(timeout=5)
        with storage.workspace_lock(root,lock,timeout=0):
            assert lock.stat().st_ino==inode
        # A killed native owner releases the kernel lock, not the persistent inode.
        process=child()
        try:
            ready(process,"LOCKED")
            process.kill()
            process.wait(timeout=5)
        finally:
            if process.poll() is None: process.kill()
            process.communicate(timeout=5)
        with storage.workspace_lock(root,lock,timeout=0):
            assert lock.stat().st_ino==inode
    print("mixed Python/Rust flock exclusion and death release passed")


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--probe",required=True)
    main(parser.parse_args().probe)
