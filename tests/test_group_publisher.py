"""Offline annotation publication against short, private AF_UNIX fixtures only."""

import copy
import inspect
import json
import os
from pathlib import Path
import socket
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

from runner import group_publisher as publisher
from runner import groups, storage

ROOT = Path(__file__).resolve().parents[1]


def inventory(count=1, *, native=None, annotations=True):
    value = dict(version="test", protocol=18, workspaces=[], tabs=[], layouts=[],
                 panes=[], agents=[])
    if annotations:
        value["annotations"] = {"epoch": "epoch-1", "sources": ["coven"]}
    for index in range(count):
        pane = dict(pane_id=f"w1:p{index + 1}", terminal_id=f"terminal-{index + 1}",
                    workspace_id="w1", tab_id="t1", focused=False,
                    agent_status="working", revision=1, agent="test-agent",
                    agent_session=copy.deepcopy(native), title="SECRET TITLE",
                    cwd="/SECRET", tokens={"private": "SECRET TOKEN"})
        value["panes"].append(pane)
        value["agents"].append(copy.deepcopy(pane))
    return value


class FakeServer:
    def __init__(self, value=None, handler=None):
        self.value = inventory() if value is None else value
        self.handler = handler
        self.requests = []
        self.errors = []
        self.stop = threading.Event()

    def __enter__(self):
        self.directory = tempfile.TemporaryDirectory(dir=ROOT, prefix=".gp-")
        self.path = str(Path(self.directory.name) / "s")
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.listener.bind(self.path)
        os.chmod(self.path, 0o600)
        self.listener.listen(8)
        self.listener.settimeout(0.05)
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()
        return self

    def serve(self):
        try:
            while not self.stop.is_set():
                try:
                    stream, _ = self.listener.accept()
                except socket.timeout:
                    continue
                with stream:
                    stream.settimeout(1)
                    data = bytearray()
                    while b"\n" not in data:
                        chunk = stream.recv(4096)
                        if not chunk:
                            break
                        data.extend(chunk)
                    if not data:
                        continue
                    request = json.loads(data)
                    self.requests.append(request)
                    if self.handler is None or not self.handler(self, stream, request):
                        result = ({"type": "session_snapshot", "snapshot": self.value}
                                  if request["method"] == "session.snapshot" else {"type": "ok"})
                        stream.sendall(json.dumps({"id": request["id"], "result": result}).encode() + b"\n")
        except (BrokenPipeError, ConnectionResetError):
            pass
        except Exception as exc:
            if not self.stop.is_set():
                self.errors.append(exc)

    def __exit__(self, *_args):
        self.stop.set()
        self.thread.join(2)
        self.listener.close()
        self.directory.cleanup()
        if self.thread.is_alive() or self.errors:
            raise AssertionError((self.thread.is_alive(), self.errors))


class Clock:
    def __init__(self):
        self.now = 100.0
        self.sleeps = []

    def monotonic(self):
        return self.now

    def sleep(self, seconds):
        self.sleeps.append(seconds)
        self.now += seconds


class PublisherTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(dir=ROOT, prefix=".gp-w-")
        self.addCleanup(self.directory.cleanup)
        self.workspace = Path(self.directory.name)
        (self.workspace / "state").mkdir()
        (self.workspace / "coven.json").write_text("{}\n")
        self.clock = Clock()

    def prepare(self, server, value=None):
        value = server.value if value is None else value
        projection = {"endpoint": publisher._endpoint(server.path), "agents": value["agents"],
                      "observed_at": "2026-10-23T00:00:00+00:00"}
        groups.create_group(self.workspace, "team", "Review Team")
        for index, agent in enumerate(value["agents"]):
            binding = publisher.binding_from_snapshot(projection, agent["pane_id"])
            groups.add_member(self.workspace, "team", f"member-{index}", f"Role {index}", binding)
        return groups.read_groups(self.workspace)

    def publish(self, server, duration=1):
        with mock.patch.object(publisher, "time", self.clock):
            result = publisher.publish_group(self.workspace, "team", server.path, "coven", duration=duration)
        self.assertEqual(sum(result[key] for key in ("acknowledged", "rejected", "unsent", "uncertain")),
                         result["reservation_attempted_targets"])
        self.assertEqual(result["reserved_targets"] + result["reservation_uncertain"],
                         result["reservation_attempted_targets"])
        self.assertLessEqual(result["completed_cycles"], result["cycles"])
        self.assertLessEqual(result["cycles"], result["reservation_attempts"])
        return result

    def interrupt_at(self, server, function, statement, *, when=lambda _frame: True, duration=1):
        lines, start = inspect.getsourcelines(function)
        matches = [start + index for index, text in enumerate(lines) if text.strip() == statement]
        self.assertEqual(len(matches), 1, statement)
        fired = False
        previous = sys.gettrace()

        def trace(frame, event, _arg):
            nonlocal fired
            if (not fired and event == "line" and frame.f_code is function.__code__
                    and frame.f_lineno == matches[0] and when(frame)):
                fired = True
                raise KeyboardInterrupt
            return trace

        try:
            sys.settrace(trace)
            result = self.publish(server, duration=duration)
        finally:
            sys.settrace(previous)
        self.assertTrue(fired, statement)
        return result

    def snapshot(self, server):
        return publisher.read_publication_snapshot(server.path, deadline=time.monotonic() + 1)

    def test_snapshot_projection_and_binding_native_optional(self):
        native = {"source": "persisted", "agent": "unknown-tool", "kind": "path", "value": "/old/native"}
        for reference in (None, native):
            with self.subTest(reference=reference), FakeServer(inventory(native=reference)) as server:
                result = self.snapshot(server)
                binding = publisher.binding_from_snapshot(result, "w1:p1")
                self.assertEqual(binding["evidence"]["native_reference"], reference)
                self.assertEqual(binding["evidence"]["native_binding"],
                                 "unestablished" if reference is None else "observed_reference")
                self.assertEqual(binding["presentation"], {
                    "adapter": "kitsune", "endpoint": publisher._endpoint(server.path),
                    "terminal_id": "terminal-1", "pane_id_at_binding": "w1:p1"})
                self.assertNotIn("SECRET", json.dumps(result))
                self.assertEqual(set(result["agents"][0]),
                                 {"pane_id", "terminal_id", "workspace_id", "tab_id", "agent", "agent_session"})
                self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"])
                if reference:
                    binding["evidence"]["native_reference"]["value"] = "changed"
                    self.assertEqual(result["agents"][0]["agent_session"], native)

    def test_binding_before_capability_opt_in_and_pane_only_rejected(self):
        with FakeServer(inventory(annotations=False)) as server:
            result = self.snapshot(server)
            self.assertIsNone(result["epoch"])
            self.assertIsNone(result["sources"])
            publisher.binding_from_snapshot(result, "w1:p1")
            with self.assertRaises(publisher.PublisherError):
                publisher.binding_from_snapshot(result, "p1")
            server.value["agents"] = []
            result = self.snapshot(server)
            with self.assertRaises(publisher.PublisherError):
                publisher.binding_from_snapshot(result, "w1:p1")

    def test_malformed_capability_rejected_but_bounded_unknown_fields_allowed(self):
        bad = [None, {}, {"epoch": "", "sources": []}, {"epoch": 2, "sources": []},
               {"epoch": "e", "sources": "coven"}, {"epoch": "e", "sources": ["a", "a"]},
               {"epoch": "e", "sources": ["a"] * 9}, {"epoch": "e", "sources": ["é"]},
               {"epoch": "e", "sources": ["a/b"]}, {"epoch": "e", "sources": [False]}]
        for descriptor in bad:
            with self.subTest(descriptor=descriptor):
                value = inventory()
                value["annotations"] = descriptor
                with FakeServer(value) as server, self.assertRaises(publisher.PublisherError):
                    self.snapshot(server)
        value = inventory()
        value["annotations"]["future"] = {"bounded": [1]}
        with FakeServer(value) as server:
            self.assertEqual(self.snapshot(server)["epoch"], "epoch-1")

    def test_strict_snapshot_wire_validation(self):
        def payloads(request):
            valid = json.dumps({"id": request["id"], "result": {
                "type": "session_snapshot", "snapshot": inventory()}}).encode()
            return [valid, valid + b"\n{}", valid + b"\n\n", b"\xff\n",
                    b'{"id":"x","id":"y"}\n', b'{"x":NaN}\n',
                    b"[" * 33 + b"0" + b"]" * 33 + b"\n",
                    valid.replace(request["id"].encode(), b"wrong") + b"\n",
                    valid.replace(b'"protocol": 18', b'"protocol": 17') + b"\n",
                    b"x" * 1025 + b"\n"]
        for index in range(10):
            def handler(_server, stream, request):
                stream.sendall(payloads(request)[index])
                return True
            with self.subTest(index=index), FakeServer(handler=handler) as server:
                with self.assertRaises(publisher.PublisherError):
                    publisher.read_publication_snapshot(server.path, deadline=time.monotonic() + 1,
                                                        max_response_bytes=1024)

    def test_snapshot_uses_existing_inventory_validation(self):
        for field, replacement in (("protocol", True), ("panes", []), ("agents", [{}])):
            value = inventory()
            value[field] = replacement
            with self.subTest(field=field), FakeServer(value) as server:
                with self.assertRaises(publisher.PublisherError):
                    self.snapshot(server)

    def test_invalid_arguments_precede_state_and_endpoint_io(self):
        invalid = [{"duration": n} for n in (0, True, 3601, 1.0, "1")]
        invalid += [{"source": s} for s in ("", "a b", "é", "x" * 65, None)]
        invalid += [{"socket_path": p} for p in ("relative", "/a/../s", "/a/", "/a\0s", "/\ud800")]
        invalid += [{"group_key": g} for g in ("", "Team", "a/b", "a b", None)]
        with mock.patch.object(groups, "read_groups", side_effect=AssertionError("state I/O")), \
                mock.patch.object(publisher, "_endpoint", side_effect=AssertionError("socket I/O")):
            for change in invalid:
                args = dict(workspace=self.workspace, group_key="team", socket_path="/s", source="coven", duration=1)
                args.update(change)
                with self.subTest(change=change), self.assertRaises(ValueError):
                    publisher.publish_group(**args)
            for change in ({"deadline": float("nan")}, {"deadline": True},
                           {"max_response_bytes": True}, {"max_response_bytes": 1023}):
                args = dict(socket_path="/s", deadline=1)
                args.update(change)
                with self.assertRaises(ValueError):
                    publisher.read_publication_snapshot(**args)

    def test_missing_group_does_not_create_authority_or_lease(self):
        result = publisher.publish_group(self.workspace, "team", "/missing", "coven", duration=1)
        self.assertEqual(result["diagnostics"], ["group_not_found"])
        self.assertEqual(list((self.workspace / "state").iterdir()), [])

    def test_full_group_durable_reservation_precedes_serial_pinned_writes(self):
        def handler(server, _stream, request):
            self.assertIn(request["method"], {"session.snapshot", "pane.annotate"})
            # Ordinary workspace lock is available during every network operation.
            with storage.workspace_lock(self.workspace, self.workspace / "state/.workspace.lock", timeout=0):
                pass
            if request["method"] == "pane.annotate":
                stored = groups.read_groups(self.workspace)
                counter, = stored["state"]["publication_counters"]
                self.assertEqual(counter["watermarks"], {"terminal-1": 1, "terminal-2": 1})
                self.assertEqual(stored["revision"], initial["revision"])
                params = request["params"]
                self.assertEqual(set(params), {"pane_id", "expected_terminal_id", "annotation_epoch",
                                              "source", "revision", "text", "ttl_ms"})
                self.assertEqual(params["expected_terminal_id"], "terminal-" + params["pane_id"][-1])
                self.assertEqual(params["ttl_ms"], 6000)
                self.assertEqual(params["annotation_epoch"], "epoch-1")
                self.assertEqual(params["revision"], 1)
                self.assertEqual(params["source"], "coven")
            return False
        with FakeServer(inventory(2), handler) as server:
            initial = self.prepare(server)
            result = self.publish(server)
            self.assertEqual(result["outcome"], "completed")
            self.assertEqual(result["cycles"], 1)
            self.assertEqual(result["acknowledged"], 2)
            self.assertEqual([r["method"] for r in server.requests],
                             ["session.snapshot", "pane.annotate", "pane.annotate"])
            self.assertEqual(result["authorization"], "annotation_publication_only")
            self.assertEqual(result["native_execution_authorization"], "none")
            self.assertFalse(result["dispatchable"])

    def test_crashed_reservation_is_burned_and_next_invocation_uses_new_revision(self):
        with FakeServer() as server:
            initial = self.prepare(server)
            endpoint = initial["state"]["groups"][0]["members"][0]["binding"]["presentation"]["endpoint"]
            groups.reserve_cycle(self.workspace, "team", initial["revision"], endpoint,
                                 "coven", "epoch-1", ["terminal-1"], allow_new_epoch=True)
            self.assertEqual(self.publish(server)["acknowledged"], 1)
            self.assertEqual(self.publish(server)["acknowledged"], 1)
            self.assertEqual([r["params"]["revision"] for r in server.requests
                              if r["method"] == "pane.annotate"], [2, 3])

    def test_reservation_durability_uncertain_sends_nothing(self):
        real_reserve = groups.reserve_cycle
        def uncertain(*args, **kwargs):
            result = real_reserve(*args, **kwargs)
            result["delivery"]["outcome"] = "durability_uncertain"
            return result
        with FakeServer(inventory(2)) as server:
            self.prepare(server)
            with mock.patch.object(groups, "reserve_cycle", side_effect=uncertain):
                result = self.publish(server)
            self.assertEqual(result["outcome"], "uncertain")
            self.assertEqual(result["unsent"], 2)
            self.assertEqual(result["acknowledged"], 0)
            self.assertEqual(result["reservation_uncertain"], 2)
            self.assertEqual(result["reserved_targets"], 0)
            self.assertEqual((result["cycles"], result["completed_cycles"]), (0, 0))
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"])
            self.assertEqual(self.publish(server)["acknowledged"], 2)
            self.assertEqual(server.requests[-1]["params"]["revision"], 2)

    def test_lost_ack_is_uncertain_no_retry_or_partial_cycle_continuation(self):
        def lose(_server, _stream, request):
            return request["method"] == "pane.annotate"
        with FakeServer(inventory(2), lose) as server:
            self.prepare(server)
            result = self.publish(server, duration=5)
            self.assertEqual((result["outcome"], result["uncertain"], result["unsent"]), ("uncertain", 1, 1))
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot", "pane.annotate"])
            server.handler = None
            result = self.publish(server)
            self.assertEqual(result["acknowledged"], 2)
            self.assertEqual([r["params"]["revision"] for r in server.requests[2:]
                              if r["method"] == "pane.annotate"], [2, 2])

    def test_valid_server_errors_are_rejected_and_messages_never_exposed(self):
        for code in ("annotation_stale_revision", "source_disabled", "unknown_method"):
            def reject(_server, stream, request):
                if request["method"] == "pane.annotate":
                    stream.sendall(json.dumps({"id": request["id"], "error": {
                        "code": code, "message": "SECRET SERVER MESSAGE"}}).encode() + b"\n")
                    return True
                return False
            with self.subTest(code=code), FakeServer(handler=reject) as server:
                self.prepare(server)
                result = self.publish(server, duration=5)
                self.assertEqual((result["outcome"], result["rejected"]), ("failed", 1))
                self.assertEqual(result["diagnostics"], ["server_error:" + code])
                self.assertNotIn("SECRET", json.dumps(result))
                self.assertEqual(len(server.requests), 2)
                groups.remove_member(self.workspace, "team", "member-0")

    def test_malformed_ack_is_uncertain_including_unbounded_error_code(self):
        invalid = [lambda r, result=result: {"id": r["id"], "result": result}
                   for result in ({"type": "pong"}, {"type": "Ok"}, None)]
        invalid += [lambda r: {"id": "wrong", "result": {"type": "ok"}},
                    lambda r: {"id": r["id"], "error": {"code": "x" * 65, "message": "SECRET"}},
                    lambda r: {"id": r["id"], "error": {"code": "bad code", "message": "SECRET"}},
                    lambda r: {"id": r["id"], "error": {"code": "invalid", "message": False}},
                    lambda r: {"id": r["id"], "result": {"type": "ok"}, "error": {}}]
        for index, build in enumerate(invalid):
            def malformed(_server, stream, request):
                if request["method"] == "pane.annotate":
                    stream.sendall(json.dumps(build(request)).encode() + b"\n")
                    return True
                return False
            with self.subTest(index=index), FakeServer(handler=malformed) as server:
                self.prepare(server)
                report = self.publish(server)
                self.assertEqual(report["uncertain"], 1)
                self.assertNotIn("SECRET", json.dumps(report))
                groups.remove_member(self.workspace, "team", "member-0")

    def test_newline_without_eof_is_not_an_acknowledgment(self):
        def no_eof(server, stream, request):
            result = {"type": "session_snapshot", "snapshot": server.value}
            stream.sendall(json.dumps({"id": request["id"], "result": result}).encode() + b"\n")
            self.assertEqual(stream.recv(1), b"")
            return True
        with FakeServer(handler=no_eof) as server:
            with self.assertRaises(publisher.PublisherError) as caught:
                publisher.read_publication_snapshot(server.path, deadline=time.monotonic() + 0.1)
            self.assertEqual(caught.exception.code, "timeout")

    def test_disabled_unsupported_and_epoch_change_stop_before_next_admission(self):
        for mode in ("disabled", "unsupported", "changed"):
            value = inventory()
            if mode == "disabled":
                value["annotations"]["sources"] = []
            elif mode == "unsupported":
                del value["annotations"]
            def change_epoch(server, _stream, request):
                if mode == "changed" and request["method"] == "pane.annotate":
                    server.value["annotations"]["epoch"] = "epoch-2"
                return False
            with self.subTest(mode=mode), FakeServer(value, change_epoch) as server:
                self.prepare(server)
                report = self.publish(server, duration=5)
                self.assertEqual(report["outcome"], "failed")
                self.assertEqual(report["acknowledged"], 1 if mode == "changed" else 0)
                self.assertEqual(report["diagnostics"], [{"disabled": "source_disabled",
                    "unsupported": "annotations_unsupported", "changed": "annotation_epoch_changed"}[mode]])
                groups.remove_member(self.workspace, "team", "member-0")

    def test_first_publication_may_use_epoch_newer_than_binding(self):
        with FakeServer(inventory(annotations=False)) as server:
            self.prepare(server)
            server.value["annotations"] = {"epoch": "newly-enabled", "sources": ["coven"]}
            self.assertEqual(self.publish(server)["outcome"], "completed")

    def test_native_and_kind_changes_require_rebind_even_none_to_reference(self):
        native = {"source": "persisted", "agent": "test-agent", "kind": "id", "value": "id1"}
        cases = [(None, native, "test-agent"), (native, None, "test-agent"),
                 (native, {**native, "value": "id2"}, "test-agent"), (None, None, "other")]
        for before, after, kind in cases:
            with self.subTest(before=before, after=after, kind=kind), FakeServer(inventory(native=before)) as server:
                self.prepare(server)
                for collection in ("panes", "agents"):
                    server.value[collection][0].update(agent_session=copy.deepcopy(after), agent=kind)
                report = self.publish(server)
                self.assertEqual(report["diagnostics"], ["rebind_required"])
                self.assertEqual(len(server.requests), 1)
                self.assertEqual(groups.read_groups(self.workspace)["state"]["publication_counters"], [])
                groups.remove_member(self.workspace, "team", "member-0")

    def test_terminal_move_allowed_but_replacement_or_agent_disappearance_stops(self):
        for mode in ("move", "replace", "missing"):
            with self.subTest(mode=mode), FakeServer() as server:
                self.prepare(server)
                for collection in ("panes", "agents"):
                    if mode == "move":
                        server.value[collection][0]["pane_id"] = "w2:p9"
                    elif mode == "replace":
                        server.value[collection][0]["terminal_id"] = "replacement"
                if mode == "missing":
                    server.value["agents"] = []
                report = self.publish(server)
                self.assertEqual(report["acknowledged"], 1 if mode == "move" else 0)
                if mode == "move":
                    self.assertEqual(server.requests[-1]["params"]["pane_id"], "w2:p9")
                else:
                    self.assertEqual(report["diagnostics"], ["bound_agent_missing"])
                groups.remove_member(self.workspace, "team", "member-0")

    def test_all_members_preflighted_before_reservation(self):
        with FakeServer(inventory(2)) as server:
            self.prepare(server)
            for collection in ("panes", "agents"):
                server.value[collection][1]["agent"] = "changed"
            report = self.publish(server)
            self.assertEqual(report["diagnostics"], ["rebind_required"])
            self.assertEqual(len(server.requests), 1)
            self.assertEqual(groups.read_groups(self.workspace)["state"]["publication_counters"], [])

    def test_content_edit_after_admission_does_not_claim_cancellation(self):
        def edit(_server, _stream, request):
            if request["method"] == "pane.annotate" and request["params"]["expected_terminal_id"] == "terminal-1":
                groups.remove_member(self.workspace, "team", "member-1")
            return False
        with FakeServer(inventory(2), edit) as server:
            self.prepare(server)
            report = self.publish(server, duration=5)
            self.assertEqual(report["acknowledged"], 2)
            self.assertEqual(report["diagnostics"], ["group_content_changed"])
            self.assertEqual(report["cycles"], 1)
            self.assertTrue(all(r["params"].get("text") is not None for r in server.requests[1:]))

    def test_edit_between_snapshot_and_reservation_is_rejected_by_cas(self):
        def edit(_server, _stream, request):
            if request["method"] == "session.snapshot":
                groups.remove_member(self.workspace, "team", "member-0")
            return False
        with FakeServer(handler=edit) as server:
            self.prepare(server)
            report = self.publish(server)
            self.assertEqual(report["diagnostics"], ["reservation_failed"])
            self.assertEqual(len(server.requests), 1)

    def test_duplicate_publisher_lease_busy_then_release(self):
        with FakeServer() as server:
            self.prepare(server)
            with storage.workspace_lock(self.workspace, self.workspace / "state/.group-publisher.lock", timeout=0) as lease:
                self.assertFalse(os.get_inheritable(lease.fd))
                report = self.publish(server)
                self.assertEqual(report["diagnostics"], ["publisher_busy"])
                self.assertEqual(server.requests, [])
            self.assertEqual(self.publish(server)["outcome"], "completed")

    def test_process_death_releases_persistent_lease_without_pid_stealing(self):
        read_fd, write_fd = os.pipe()
        pid = os.fork()
        if pid == 0:
            os.close(read_fd)
            try:
                with storage.workspace_lock(self.workspace, self.workspace / "state/.group-publisher.lock", timeout=0):
                    os.write(write_fd, b"held")
                    os._exit(0)
            except BaseException:
                os._exit(1)
        os.close(write_fd)
        try:
            self.assertEqual(os.read(read_fd, 4), b"held")
        finally:
            os.close(read_fd)
            _, status = os.waitpid(pid, 0)
        self.assertEqual(status, 0)
        path = self.workspace / "state/.group-publisher.lock"
        inode = path.stat().st_ino
        with storage.workspace_lock(self.workspace, path, timeout=0):
            self.assertEqual(path.stat().st_ino, inode)

    def test_empty_group_and_mixed_endpoints_fail_before_socket_io(self):
        groups.create_group(self.workspace, "team", "Review Team")
        report = publisher.publish_group(self.workspace, "team", "/unused", "coven", duration=1)
        self.assertEqual(report["diagnostics"], ["group_size"])
        with FakeServer(inventory(2)) as server:
            self.prepare(server)
            state = groups.read_groups(self.workspace)
            second = state["state"]["groups"][0]["members"][1]
            binding = copy.deepcopy(second["binding"])
            binding["presentation"]["endpoint"]["inode"] += 1
            groups.add_member(self.workspace, "team", second["key"], second["role"], binding, rebind=True)
            report = self.publish(server)
            self.assertEqual(report["diagnostics"], ["group_endpoint_mismatch"])
            self.assertEqual(server.requests, [])

    def test_changed_endpoint_during_run_stops_before_another_write(self):
        with FakeServer() as server:
            self.prepare(server)
            original = publisher._endpoint(server.path)
            real_snapshot = publisher.read_publication_snapshot
            snapshots = 0
            def changed(*args, **kwargs):
                nonlocal snapshots
                result = real_snapshot(*args, **kwargs)
                snapshots += 1
                if snapshots == 2:
                    result["endpoint"] = {**original, "inode": original["inode"] + 1}
                return result
            with mock.patch.object(publisher, "read_publication_snapshot", side_effect=changed):
                report = self.publish(server, duration=5)
            self.assertEqual(report["diagnostics"], ["endpoint_replaced"])
            self.assertEqual(report["acknowledged"], 1)
            self.assertEqual(len(server.requests), 3)

    def test_duration_fresh_cycles_allow_epoch_only_first_and_no_clear_on_stop(self):
        with FakeServer() as server:
            self.prepare(server)
            with mock.patch.object(groups, "reserve_cycle", wraps=groups.reserve_cycle) as reserve:
                report = self.publish(server, duration=5)
            self.assertEqual(report["cycles"], 3)
            self.assertEqual(self.clock.sleeps, [2, 2, 1])
            self.assertEqual([c.kwargs["allow_new_epoch"] for c in reserve.call_args_list], [True, False, False])
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot", "pane.annotate"] * 3)
            self.assertTrue(all(r["params"]["ttl_ms"] == 6000 and r["params"]["text"]
                                for r in server.requests if r["method"] == "pane.annotate"))

    def test_deadline_after_reservation_burns_without_send(self):
        real_reserve = groups.reserve_cycle
        def slow(*args, **kwargs):
            result = real_reserve(*args, **kwargs)
            self.clock.now += 4
            return result
        with FakeServer(inventory(2)) as server:
            self.prepare(server)
            with mock.patch.object(groups, "reserve_cycle", side_effect=slow):
                report = self.publish(server)
            self.assertEqual((report["outcome"], report["unsent"]), ("failed", 2))
            self.assertEqual(report["diagnostics"], ["timeout"])
            self.assertEqual(len(server.requests), 1)

    def test_slow_cycle_skips_missed_ticks_instead_of_burst(self):
        def slow(_server, _stream, request):
            if request["method"] == "pane.annotate":
                self.clock.now += 2.5
            return False
        with FakeServer(handler=slow) as server:
            self.prepare(server)
            report = self.publish(server, duration=5)
            self.assertEqual(report["cycles"], 2)
            self.assertEqual(self.clock.sleeps, [1.5])
            self.assertEqual(report["uncertain"], 1)

    def test_ctrl_c_during_sleep_releases_lease_without_clear(self):
        with FakeServer() as server:
            self.prepare(server)
            with mock.patch.object(self.clock, "sleep", side_effect=KeyboardInterrupt):
                report = self.publish(server)
            self.assertEqual(report["outcome"], "interrupted")
            self.assertEqual(report["acknowledged"], 1)
            with storage.workspace_lock(self.workspace, self.workspace / "state/.group-publisher.lock", timeout=0):
                pass
            self.assertEqual(len(server.requests), 2)

    def test_ctrl_c_inflight_summary_counts_uncertainty_and_unsent_remainder(self):
        real_request = publisher._request
        def interrupt(*args, **kwargs):
            if args[1] == "pane.annotate":
                raise publisher.PublisherError("interrupted", "uncertain", interrupted=True)
            return real_request(*args, **kwargs)
        with FakeServer(inventory(2)) as server:
            self.prepare(server)
            with mock.patch.object(publisher, "_request", side_effect=interrupt):
                report = self.publish(server)
            self.assertEqual((report["outcome"], report["uncertain"], report["unsent"]),
                             ("interrupted", 1, 1))
            with storage.workspace_lock(self.workspace, self.workspace / "state/.group-publisher.lock", timeout=0):
                pass

    def test_interrupt_after_real_reservation_burn_records_unknown_reservation(self):
        real_reserve = groups.reserve_cycle

        def burn_then_interrupt(*args, **kwargs):
            real_reserve(*args, **kwargs)
            raise KeyboardInterrupt

        with FakeServer(inventory(2)) as server:
            self.prepare(server)
            with mock.patch.object(groups, "reserve_cycle", side_effect=burn_then_interrupt):
                report = self.publish(server)
            self.assertEqual(report["outcome"], "interrupted")
            self.assertEqual((report["reservation_attempts"], report["reservation_uncertain"]), (1, 2))
            self.assertEqual((report["cycles"], report["reserved_targets"]), (0, 0))
            self.assertEqual((report["acknowledged"], report["uncertain"], report["unsent"]), (0, 0, 2))
            counter, = groups.read_groups(self.workspace)["state"]["publication_counters"]
            self.assertEqual(counter["watermarks"], {"terminal-1": 1, "terminal-2": 1})
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"])
            self.assertEqual(self.publish(server)["acknowledged"], 2)
            self.assertEqual([r["params"]["revision"] for r in server.requests
                              if r["method"] == "pane.annotate"], [2, 2])

    def test_storage_error_before_or_after_commit_never_claims_rollback(self):
        real_reserve = groups.reserve_cycle
        with FakeServer(inventory(2)) as server:
            self.prepare(server)
            for commit in (False, True):
                def fail(*args, **kwargs):
                    if commit:
                        real_reserve(*args, **kwargs)
                    raise storage.StorageError("SECRET storage error")

                with self.subTest(commit=commit), mock.patch.object(groups, "reserve_cycle", side_effect=fail):
                    report = self.publish(server)
                self.assertEqual(report["outcome"], "uncertain")
                self.assertEqual(report["diagnostics"], ["reservation_failed"])
                self.assertEqual((report["reservation_uncertain"], report["reserved_targets"]), (2, 0))
                self.assertEqual((report["unsent"], report["uncertain"]), (2, 0))
                self.assertEqual(report["cycles"], 0)
                counters = groups.read_groups(self.workspace)["state"]["publication_counters"]
                self.assertEqual(counters[0]["watermarks"] if commit else counters,
                                 {"terminal-1": 1, "terminal-2": 1} if commit else [])
                self.assertNotIn("SECRET", json.dumps(report))
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"] * 2)

    def test_interrupt_at_reservation_classification_is_one_cycle_transition(self):
        with FakeServer(inventory(2)) as server:
            self.prepare(server)
            for statement, expected_cycles, unknown in (
                    ('cycle["reservation"] = "durably_committed"', 0, 2),
                    ("first_cycle = False", 1, 0)):
                with self.subTest(statement=statement):
                    report = self.interrupt_at(server, publisher.publish_group, statement)
                    self.assertEqual(report["outcome"], "interrupted")
                    self.assertEqual(report["reservation_attempts"], 1)
                    self.assertEqual(report["cycles"], expected_cycles)
                    self.assertEqual(report["reservation_uncertain"], unknown)
                    self.assertEqual(report["reserved_targets"], 2 - unknown)
                    self.assertEqual((report["unsent"], report["uncertain"]), (2, 0))
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"] * 2)
            counter, = groups.read_groups(self.workspace)["state"]["publication_counters"]
            self.assertEqual(counter["watermarks"], {"terminal-1": 2, "terminal-2": 2})

    def test_interrupt_between_valid_ack_return_and_phase_assignment_is_uncertain(self):
        with FakeServer(inventory(2)) as server:
            self.prepare(server)
            report = self.interrupt_at(server, publisher.publish_group, 'target["phase"] = "acknowledged"')
            self.assertEqual(report["outcome"], "interrupted")
            self.assertEqual((report["acknowledged"], report["uncertain"], report["unsent"]), (0, 1, 1))
            self.assertEqual((report["cycles"], report["completed_cycles"]), (1, 0))
            self.assertEqual((report["reserved_targets"], report["reservation_uncertain"]), (2, 0))
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot", "pane.annotate"])
            self.assertEqual(server.requests[-1]["params"]["expected_terminal_id"], "terminal-1")
            counter, = groups.read_groups(self.workspace)["state"]["publication_counters"]
            self.assertEqual(counter["watermarks"], {"terminal-1": 1, "terminal-2": 1})

    def test_interrupt_after_ack_phase_cannot_drop_a_target_between_counters(self):
        with FakeServer(inventory(2)) as server:
            self.prepare(server)
            report = self.interrupt_at(
                server, publisher.publish_group, 'for target in cycle["targets"]:',
                when=lambda frame: frame.f_locals["cycle"]["targets"][0]["phase"] == "acknowledged")
            self.assertEqual(report["outcome"], "interrupted")
            self.assertEqual((report["acknowledged"], report["uncertain"], report["unsent"]), (1, 0, 1))
            self.assertEqual((report["cycles"], report["completed_cycles"]), (1, 0))
            self.assertEqual(len(server.requests), 2)
            counter, = groups.read_groups(self.workspace)["state"]["publication_counters"]
            self.assertEqual(counter["watermarks"], {"terminal-1": 1, "terminal-2": 1})

    def test_interrupt_during_final_count_derivation_recomputes_recorded_phases(self):
        with FakeServer(inventory(2)) as server:
            self.prepare(server)
            report = self.interrupt_at(
                server, publisher._publication_summary,
                'reserved_targets = sum(len(cycle["targets"]) for cycle in committed)')
            self.assertEqual(report["outcome"], "interrupted")
            self.assertEqual((report["acknowledged"], report["uncertain"], report["unsent"]), (2, 0, 0))
            self.assertEqual((report["cycles"], report["completed_cycles"]), (1, 1))
            self.assertEqual(report["reservation_uncertain"], 0)
            self.assertEqual(len(server.requests), 3)

    def test_interrupt_before_annotation_call_keeps_inflight_conservative(self):
        with FakeServer(inventory(2)) as server:
            self.prepare(server)
            report = self.interrupt_at(
                server, publisher.publish_group,
                '_request(socket_path, "pane.annotate", params, deadline=deadline,')
            self.assertEqual(report["outcome"], "interrupted")
            self.assertEqual((report["uncertain"], report["unsent"]), (1, 1))
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"])

    def test_interrupt_after_typed_failure_phase_preserves_delivery_classification(self):
        real_request = publisher._request

        def reject(_server, stream, request):
            if request["method"] == "pane.annotate":
                stream.sendall(json.dumps({"id": request["id"], "error": {
                    "code": "annotation_stale_revision", "message": "SECRET"}}).encode() + b"\n")
                return True
            return False

        with FakeServer(inventory(2)) as server:
            self.prepare(server)
            for delivery in ("rejected", "unsent", "uncertain"):
                def request(*args, **kwargs):
                    if delivery == "unsent" and args[1] == "pane.annotate":
                        raise publisher.PublisherError("io_error", "unsent")
                    return real_request(*args, **kwargs)

                server.handler = reject if delivery == "rejected" else (
                    lambda _server, _stream, r: r["method"] == "pane.annotate")
                before = len(server.requests)
                with self.subTest(delivery=delivery), mock.patch.object(publisher, "_request", side_effect=request):
                    report = self.interrupt_at(server, publisher.publish_group, "raise")
                    self.assertEqual(report["outcome"], "interrupted")
                    self.assertEqual(report[delivery], 2 if delivery == "unsent" else 1)
                    self.assertEqual(report["acknowledged"], 0)
                    self.assertEqual(report["unsent"], 2 if delivery == "unsent" else 1)
                    self.assertEqual(report["reservation_uncertain"], 0)
                    self.assertEqual(report["reserved_targets"], 2)
                    self.assertEqual(len(server.requests) - before, 1 if delivery == "unsent" else 2)
            counter, = groups.read_groups(self.workspace)["state"]["publication_counters"]
            self.assertEqual(counter["watermarks"], {"terminal-1": 3, "terminal-2": 3})

    def test_transport_partial_send_and_keyboard_interrupt_classification(self):
        with FakeServer() as server:
            endpoint = publisher._endpoint(server.path)
            for stage, error, delivery in (("connect", OSError(), "unsent"),
                                           ("sendall", OSError(), "uncertain"),
                                           ("sendall", KeyboardInterrupt(), "uncertain"),
                                           ("connect", KeyboardInterrupt(), "unsent")):
                stream = mock.MagicMock()
                stream.__enter__.return_value = stream
                getattr(stream, stage).side_effect = error
                network = mock.Mock(AF_UNIX=socket.AF_UNIX, SOCK_STREAM=socket.SOCK_STREAM)
                network.socket.return_value = stream
                with self.subTest(stage=stage, error=type(error)), mock.patch.object(publisher, "socket", network):
                    with self.assertRaises(publisher.PublisherError) as caught:
                        publisher._request(server.path, "pane.annotate", {},
                                           deadline=time.monotonic() + 1, frozen_endpoint=endpoint)
                    self.assertEqual(caught.exception.delivery, delivery)
                    self.assertEqual(caught.exception.interrupted, isinstance(error, KeyboardInterrupt))

    def test_endpoint_safety_aliases_and_frozen_inode(self):
        with FakeServer() as server:
            alias = Path(server.directory.name) / "alias"
            alias.symlink_to(server.directory.name, target_is_directory=True)
            result = publisher.read_publication_snapshot(str(alias / "s"), deadline=time.monotonic() + 1)
            self.assertEqual(result["endpoint"]["path"], os.path.realpath(server.path))
            leaf = Path(server.directory.name) / "leaf"
            leaf.symlink_to(server.path)
            with self.assertRaises(publisher.PublisherError):
                publisher.read_publication_snapshot(str(leaf), deadline=time.monotonic() + 1)
            os.chmod(server.path, 0o660)
            with self.assertRaises(publisher.PublisherError):
                self.snapshot(server)
            os.chmod(server.path, 0o600)
            endpoint = publisher._endpoint(server.path)
            endpoint["inode"] += 1
            with self.assertRaises(publisher.PublisherError) as caught:
                publisher._request(server.path, "pane.annotate", {}, deadline=time.monotonic() + 1,
                                   frozen_endpoint=endpoint)
            self.assertEqual(caught.exception.delivery, "unsent")
            self.assertEqual(caught.exception.code, "endpoint_replaced")
            self.assertEqual(len(server.requests), 1)

    def test_endpoint_fences_after_send_and_after_receipt(self):
        with FakeServer() as server:
            original = publisher._endpoint(server.path)
            changed = {**original, "inode": original["inode"] + 1}
            for values in ([original, original, changed], [original, original, original, changed]):
                stream = mock.MagicMock()
                stream.__enter__.return_value = stream
                stream.recv.side_effect = [b'{"id":"unused","result":{"type":"ok"}}\n', b""]
                network = mock.Mock(AF_UNIX=socket.AF_UNIX, SOCK_STREAM=socket.SOCK_STREAM)
                network.socket.return_value = stream
                with self.subTest(fence=len(values)), mock.patch.object(publisher, "socket", network), \
                        mock.patch.object(publisher, "_endpoint", side_effect=values):
                    with self.assertRaises(publisher.PublisherError) as caught:
                        publisher._request(server.path, "pane.annotate", {}, deadline=time.monotonic() + 1,
                                           frozen_endpoint=original)
                    self.assertEqual(caught.exception.code, "endpoint_replaced")
                    self.assertEqual(caught.exception.delivery, "uncertain")

    def test_transport_method_allowlist(self):
        with self.assertRaises(ValueError):
            publisher._request("/unused", "pane.send", {}, deadline=1)


if __name__ == "__main__":
    unittest.main()
