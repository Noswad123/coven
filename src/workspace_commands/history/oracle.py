"""Exercise the real Python presentation helpers without providers or apps."""
import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "runner"))
import coven

records = [{}]
values = [None, False, 0, -0.0, "", [], {}, True, 1, [None, False, "'\"\\\n\x00🦊"],
          {"key": [True, 0, -0.0]}, "literal\r\n\x1b\u2028🦊", 10**100, 1e-5]
for value in values:
    for field in ["ts", "type", "from", "to", "message", "request", "body", "agent", "checkpoint", "task_id", "task"]:
        records.append({field: value})
    records.append({field: value for field in ["ts", "type", "from", "to", "message", "request", "body", "agent", "checkpoint", "task_id"]})
    records.append({"message": value, "request": "request fallback", "body": "body fallback"})
    records.append({"message": None, "request": value, "body": "body fallback"})
    records.append({"task_id": value, "task": {"id": "nested fallback"}})
    records.append({"task_id": None, "task": {"id": value}})

with tempfile.TemporaryDirectory() as directory:
    path = Path(directory) / "log.jsonl"
    for record in records:
        path.write_text(json.dumps(record, ensure_ascii=True) + "\n", encoding="utf-8")
        result = {"record": record}
        for kind in ["events", "messages"]:
            stream = io.StringIO()
            with contextlib.redirect_stdout(stream):
                assert coven.print_pretty_jsonl(path, kind) == 0
            result[kind] = stream.getvalue()
        print(json.dumps(result, ensure_ascii=True))

    for text in [None, "", "\r", "\r\n", "a\rb\r\nc\n", "\n\n", "\x00\x1b🦊\u2028\u0085"]:
        if text is None:
            path.unlink()
        else:
            path.write_bytes(text.encode("utf-8"))
        stream = io.StringIO()
        with contextlib.redirect_stdout(stream):
            assert coven.print_file(path, "No events yet\n") == 0
        print(json.dumps({"raw": text, "text": stream.getvalue()}, ensure_ascii=True))
