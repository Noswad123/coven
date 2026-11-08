"""Test-only frozen Python decoder oracle; no Djinn runtime or subprocess probes."""
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from runner import djinn_observation as adapter, kitsune_observation as wire

results = []
for case in json.loads(Path(sys.argv[1]).read_text(encoding="utf-8")):
    try:
        assessment, facts, producer = adapter._decode(case["payload"].encode("utf-8"), case["folder"])
        results.append({"assessment": assessment, "observation": facts, "producer": producer})
    except wire._Failure as error:
        results.append({"error": error.code})
    except (ValueError, UnicodeError):
        results.append({"error": "schema_error"})
print(json.dumps(results, ensure_ascii=True, allow_nan=False))
