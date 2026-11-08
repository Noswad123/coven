"""Test-only CPython group authority oracle; never imported by production Rust."""
import json
from pathlib import Path
import sys
import tempfile
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from runner import groups

request = json.load(sys.stdin)
mode = request["mode"]
if mode == "pure":
    result = []
    validator = getattr(groups, "_" + request["validator"])
    for value in request["values"]:
        try:
            validator(value)
            result.append(True)
        except Exception:
            result.append(False)
    print(json.dumps(result))
elif mode == "timestamp":
    from datetime import datetime
    result = []
    for stamp in request["values"]:
        try:
            d = datetime.fromisoformat(stamp)
            result.append("T" in stamp and d.tzinfo is not None
                          and d.utcoffset().total_seconds() == 0)
        except ValueError:
            result.append(False)
    print(json.dumps(result))
elif mode == "validate":
    result = []
    for raw in request["values"]:
        try:
            state = groups._validate(groups._decode(raw.encode()))
            result.append({"accepted": True, "revision": groups._revision(state)})
        except Exception:
            result.append({"accepted": False})
    print(json.dumps(result, ensure_ascii=False))
else:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        (root / "state").mkdir()
        (root / "coven.json").write_text("not JSON")
        path = root / "state" / "groups.json"
        if request["raw"] is not None:
            path.write_text(request["raw"], encoding="utf-8")
        ids = iter(request["ids"])
        groups.uuid.uuid4 = lambda: uuid.UUID(next(ids))
        action = request["action"]
        args = action["args"]
        try:
            result = getattr(groups, action["operation"])(root, **args)
            output = {"accepted": True, "result": result}
        except Exception:
            output = {"accepted": False}
        output["raw"] = path.read_text(encoding="utf-8") if path.exists() else None
        print(json.dumps(output, ensure_ascii=False))
