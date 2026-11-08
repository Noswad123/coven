"""Frozen Python publisher oracle. Fake effects only; no external endpoints."""
import contextlib
import copy
import json
from pathlib import Path
import sys
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from runner import group_publisher as publisher


class Clock:
    def __init__(self):
        self.elapsed = 0.0

    def monotonic(self):
        return 100.0 + self.elapsed

    def time(self):
        return 100.0 + self.elapsed

    def sleep(self, seconds):
        self.elapsed += seconds


data = json.load(sys.stdin)
clock = Clock()
writes = []
group = data["group"]


def reserve(*args, **kwargs):
    if data["mode"] == "reservation_error":
        raise publisher.storage.StorageError("private")
    if data["mode"] == "reservation_uncertain":
        return {"delivery": {"outcome": "durability_uncertain"}}
    if data["mode"] == "invalid_reservation":
        return {"revision": "revision", "delivery": {"outcome": "durably_committed"},
                "reservations": {}}
    return {"revision": "revision", "delivery": {"outcome": "durably_committed"},
            "reservations": {m["binding"]["presentation"]["terminal_id"]: 1
                             for m in group["members"]}}


def send(*args, **kwargs):
    if data["mode"] == "unsent":
        raise publisher.PublisherError("io_error")
    prepare = kwargs.get("prepare_send")
    if prepare is not None:
        prepare()
        prepare()
    params = kwargs.get("params", args[2] if len(args) > 2 else None)
    writes.append(copy.deepcopy(params))
    if data["mode"] == "reject":
        raise publisher.PublisherError("server_error:annotation_expired", "rejected")
    if data["mode"] == "lost_ack":
        raise publisher.PublisherError("incomplete_response", "uncertain")


with contextlib.ExitStack() as stack:
    for target, value in [
        ("time", clock),
        ("groups.read_groups", lambda *a: {"revision": "revision", "initialized": True,
                                          "state": {"groups": [group]}}),
        ("storage.workspace_lock", lambda *a, **kw: contextlib.nullcontext()),
        ("read_publication_snapshot", lambda *a, **kw: copy.deepcopy(data["fresh"])),
        ("groups.reserve_cycle", reserve),
        ("group_live.status", lambda *a, **kw: copy.deepcopy(data["status"])),
        ("_request", send),
    ]:
        stack.enter_context(mock.patch("runner.group_publisher." + target, value))
    report = publisher.publish_group("/unused", "team", "/unused/socket", "coven",
                                     duration=1, live=data["live"])
sys.stdout.write(json.dumps({"report": report, "writes": writes}, ensure_ascii=False,
                           sort_keys=True, separators=(",", ":")))
