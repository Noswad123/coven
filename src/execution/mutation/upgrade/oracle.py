"""Test-only upgrade oracle; invoked with -I -B, never by production Rust."""
import json
import os
from pathlib import Path
import stat
import sys
import tempfile
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[4]))
from runner import execution_v2, storage, task_attempts

request = json.load(sys.stdin)
with tempfile.TemporaryDirectory() as temporary:
    root = Path(temporary)
    path = root / task_attempts.CONTROL
    path.parent.mkdir(parents=True)
    path.write_bytes(request["raw"].encode("utf-8"))
    os.chmod(path, request["mode"])
    store = task_attempts.Store(root)
    revision = store.read().revision

    def captured(store, control, view, binding, effect, key, lock):
        backup = root / effect["backup_path"]
        return {"control": control, "effect": effect,
                "backup_hex": backup.read_bytes().hex(),
                "backup_mode": stat.S_IMODE(backup.stat().st_mode)}

    with storage.workspace_lock(root, root / "state/.workspace.lock") as guard, \
         patch.object(execution_v2.uuid, "uuid4", return_value=SimpleNamespace(hex="d" * 32)), \
         patch.object(execution_v2, "_publish", side_effect=captured):
        result = execution_v2.upgrade(store, revision, "upgrade", guard)
    print(json.dumps(result, ensure_ascii=True))
