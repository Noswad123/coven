"""Actual Python reservation writer oracle, against an isolated authority copy."""
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from runner import groups, group_publisher

workspace = Path(sys.argv[1])
source = sys.argv[2]
revision, _, original = group_publisher._read_group(groups, workspace, "team")
if sys.argv[3] == "publisher":
    # Actual publisher policy: capture at line 483, retain through rereads, pass
    # unchanged at line 564. Change only declaration order under the same revision.
    endpoint = original
    state = groups.read_groups(workspace)["state"]
    state["groups"][0]["members"][0]["binding"]["presentation"]["endpoint"] = dict(reversed(list(original.items())))
    (workspace / "state/groups.json").write_text(json.dumps(state, indent=2) + "\n")
    group_publisher._read_group(groups, workspace, "team", revision)
else:
    # Primitive policy: groups.py:515 deep-copies the exact caller's object.
    endpoint = {key: original[key] for key in ("path", "device", "inode")}
groups.reserve_cycle(workspace, "team", revision, endpoint, source,
                     "epoch", ["terminal"], allow_new_epoch=True)
sys.stdout.buffer.write((workspace / "state/groups.json").read_bytes())
