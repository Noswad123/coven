"""Test-only producer/storage oracle; never used by native production commands."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import time
from types import SimpleNamespace
from unittest.mock import patch
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from runner import group_publisher, groups

request = json.load(sys.stdin)
produced = request["produced"]
presentation = produced["presentation"]
evidence = produced["evidence"]
# Mirror the real endpoint selector and publication-snapshot projection, not the
# alphabetical object order of the JSON test transport.
endpoint = {key: presentation["endpoint"][key] for key in ("path", "device", "inode")}
pane = presentation["pane_id_at_binding"]
if request["process"]:
    item = {
        "recognition": "djinn_executable_family",
        "locators": {"terminal_id": presentation["terminal_id"]},
        "sample_received_at": evidence["observed_at"],
        "interpreter": evidence["process_evidence"]["interpreter"],
        "matches": [{key: match[key] for key in ("pid", "executable_family", "rule")}
                    for match in evidence["process_evidence"]["matches"]],
    }
    with patch.object(group_publisher, "_endpoint", return_value=endpoint), patch.object(
            group_publisher.process_observation, "_read_evidence", return_value=item):
        binding = group_publisher.binding_from_process(endpoint["path"], pane,
                                                       deadline=time.monotonic() + 3)
else:
    native = evidence["native_reference"]
    snapshot = {
        "endpoint": endpoint, "observed_at": evidence["observed_at"],
        "agents": [{"pane_id": pane, "terminal_id": presentation["terminal_id"],
                    "agent": evidence["agent_kind_hint"],
                    "agent_session": None if native is None else {
                        key: native[key] for key in ("source", "agent", "kind", "value")}}],
    }
    binding = group_publisher.binding_from_snapshot(snapshot, pane)

with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    (root / "coven.json").write_text("not team JSON")
    (root / "state").mkdir()
    path = root / "state" / "groups.json"
    path.write_bytes(request["seed"].encode())
    identities = SimpleNamespace(uuid4=lambda: uuid.UUID(request["participant"]),
                                 UUID=uuid.UUID, RFC_4122=uuid.RFC_4122)
    with patch.object(groups, "uuid", identities):
        result = groups.add_member(root, "g", "m", "Role", binding)
    raw = path.read_bytes()
    print(json.dumps({"raw": raw.decode(), "sha256": hashlib.sha256(raw).hexdigest(),
                      "revision": result["revision"],
                      "binding": json.dumps(binding, ensure_ascii=False, separators=(",", ":"))}))
