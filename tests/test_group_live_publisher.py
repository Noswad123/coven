"""Frozen pilot-v1 publisher contracts: private artifacts/fake endpoints only."""

import contextlib
from copy import deepcopy
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import unittest
from unittest import mock

from runner import coven, djinn_live_observation as live, group_live, groups, launch, storage, workers
from runner import group_publisher as publisher
from test_djinn_live_observation import INSTANCE, OTHER, RUNTIME, SAMPLED, SESSION, native_fixture
from test_group_publisher import FakeServer, inventory
from test_process_groups import pane_inventory, process_handler


class Clock:
    def __init__(self):
        self.wall_ms = SAMPLED + 100
        self.mono_ms = 100000
        self.sleeps = []

    def monotonic(self):
        return self.mono_ms / 1000

    def time(self):
        return self.wall_ms / 1000

    def advance(self, milliseconds):
        self.wall_ms += milliseconds
        self.mono_ms += milliseconds

    def sleep(self, seconds):
        self.sleeps.append(seconds)
        self.advance(seconds * 1000)


def capable(count=1):
    value = inventory(count)
    value["annotations"]["valid_until_unix_ms"] = True
    return value


@unittest.skipUnless(os.name == "posix", "Private POSIX fixtures")
class LivePublisherTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="coven-live-publish-")
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name).resolve()
        self.root.chmod(0o700)
        self.workspace = self.root / "workspace"
        self.workspace.mkdir()
        (self.workspace / "state").mkdir()
        (self.workspace / "coven.json").write_bytes(b"{}\n")
        self.private = self.root / "private"
        self.private.mkdir(mode=0o700)
        self.path = self.private / (RUNTIME + ".json")
        self.clock = Clock()
        self.fixture = native_fixture()
        self.write_source()
        for module in (publisher, group_live, live, publisher.observation, publisher.process_observation):
            patcher = mock.patch.object(module, "time", self.clock)
            patcher.start()
            self.addCleanup(patcher.stop)
        for module, name in ((launch, "execute"), (workers, "inspect_readiness"),
                             (subprocess, "Popen"), (subprocess, "run")):
            guard = mock.patch.object(module, name, side_effect=AssertionError("native operation"))
            guard.start()
            self.addCleanup(guard.stop)

    def write_source(self):
        self.path.write_bytes(json.dumps(self.fixture).encode())
        self.path.chmod(0o600)

    def prepare(self, server, *, attached=True, process=False):
        # Each subcase has an independent authority/counter namespace. Do not let
        # endpoint churn in parameterized fixtures exhaust the eight-entry cap.
        authority = self.workspace / "state/groups.json"
        if authority.exists():
            authority.unlink()
        groups.create_group(self.workspace, "team", "Review Team")
        for index, pane in enumerate(server.value["panes"]):
            binding = (publisher.binding_from_process(server.path, pane["pane_id"], deadline=103)
                       if process else publisher.binding_from_snapshot({
                           "endpoint": publisher._endpoint(server.path), "agents": server.value["agents"],
                           "observed_at": "2026-10-25T00:00:00Z"}, pane["pane_id"]))
            groups.add_member(self.workspace, "team", f"m{index}", f"Role {index}", binding)
            if attached:
                group_live.attach(self.workspace, "team", f"m{index}", str(self.path),
                                  RUNTIME, INSTANCE, SESSION)
        server.requests.clear()
        return groups.read_groups(self.workspace)

    def publish(self, server, *, duration=1, live_mode=True):
        result = publisher.publish_group(self.workspace, "team", server.path, "coven",
                                         duration=duration, live=live_mode)
        self.assertEqual(sum(result[k] for k in ("acknowledged", "rejected", "unsent", "uncertain")),
                         result["reservation_attempted_targets"])
        self.assertEqual(result["reserved_targets"] + result["reservation_uncertain"],
                         result["reservation_attempted_targets"])
        return result

    def writes(self, server):
        return [r["params"] for r in server.requests if r["method"] == "pane.annotate"]

    def assert_role(self, params, index=0):
        self.assertEqual(params["text"], f"Review Team · Role {index}")
        self.assertEqual(params["ttl_ms"], 6000)
        self.assertNotIn("valid_until_unix_ms", params)

    def assert_unreserved(self, server, result):
        self.assertEqual(result["outcome"], "failed")
        self.assertEqual(result["reservation_attempts"], 0)
        self.assertEqual(self.writes(server), [])
        self.assertEqual(groups.read_groups(self.workspace)["state"]["publication_counters"], [])

    def test_busy_exact_wire_summary_and_unchanged_authority(self):
        with FakeServer(capable(2)) as server:
            before = self.prepare(server)
            with mock.patch.object(groups, "reserve_cycle", wraps=groups.reserve_cycle) as reserve:
                result = self.publish(server)
            self.assertEqual(result["outcome"], "completed")
            self.assertEqual(result["acknowledged"], 2)
            self.assertEqual({k: result[k] for k in ("publication_mode", "ttl_ms", "static_ttl_ms", "live_max_age_ms")},
                             dict(publication_mode="live", ttl_ms=None, static_ttl_ms=6000, live_max_age_ms=3000))
            reserve.assert_called_once()
            self.assertEqual(reserve.call_args.args[6], ["terminal-1", "terminal-2"])
            for index, params in enumerate(self.writes(server)):
                self.assertEqual(set(params), {"pane_id", "expected_terminal_id", "annotation_epoch", "source",
                                               "revision", "text", "ttl_ms", "valid_until_unix_ms"})
                self.assertEqual(params["text"], f"Review Team · Role {index} · legacy runner busy")
                self.assertEqual(params["ttl_ms"], 3000)
                self.assertLessEqual(params["valid_until_unix_ms"], SAMPLED + 3000)
                self.assertGreater(params["valid_until_unix_ms"], self.clock.wall_ms - 1000)
                self.assertEqual(params["revision"], 1)
            after = groups.read_groups(self.workspace)
            self.assertEqual(after["state"]["groups"], before["state"]["groups"])
            self.assertEqual(after["revision"], before["revision"])

    def test_static_exact_payload_summary_and_no_attachment_reads_on_old_server(self):
        with FakeServer(inventory()) as server:
            self.prepare(server)
            self.path.unlink()
            with mock.patch.object(group_live, "status", side_effect=AssertionError("status read")), \
                    mock.patch.object(live, "inspect", side_effect=AssertionError("source read")):
                result = self.publish(server, live_mode=False)
            self.assert_role(self.writes(server)[0])
            self.assertEqual(set(self.writes(server)[0]), {"pane_id", "expected_terminal_id", "annotation_epoch",
                                                         "source", "revision", "text", "ttl_ms"})
            self.assertEqual(result["ttl_ms"], 6000)
            self.assertNotIn("publication_mode", result)
            self.assertNotIn("static_ttl_ms", result)
            self.assertEqual(result["limitations"], [
                "Labels expire by TTL; stopping does not cancel admitted writes.",
                "Filesystem fsync is not wall-clock bounded."])

    def test_only_literal_true_capability_authorizes_even_unattached(self):
        for value in ("absent", False, None, 1, "true", {}, []):
            with self.subTest(value=value), FakeServer(capable()) as server:
                if value == "absent":
                    server.value["annotations"].pop("valid_until_unix_ms")
                else:
                    server.value["annotations"]["valid_until_unix_ms"] = value
                self.prepare(server, attached=False)
                with mock.patch.object(group_live, "status", side_effect=AssertionError("premature source I/O")):
                    result = self.publish(server)
                self.assert_unreserved(server, result)
                self.assertEqual(result["diagnostics"], ["annotation_deadline_unsupported"])
                groups.remove_member(self.workspace, "team", "m0")

    def test_capability_loss_epoch_drift_and_source_disable_stop_next_cycle(self):
        for mode in ("capability", "epoch", "source"):
            def change(server, _stream, request):
                if request["method"] == "pane.annotate":
                    if mode == "capability":
                        server.value["annotations"]["valid_until_unix_ms"] = False
                    elif mode == "epoch":
                        server.value["annotations"]["epoch"] = "epoch-2"
                    else:
                        server.value["annotations"]["sources"] = []
                return False
            with self.subTest(mode=mode), FakeServer(capable(), change) as server:
                self.prepare(server, attached=False)
                result = self.publish(server, duration=5)
                self.assertEqual(result["outcome"], "failed")
                self.assertEqual(result["acknowledged"], 1)
                self.assertEqual(result["cycles"], 1)
                groups.remove_member(self.workspace, "team", "m0")

    def test_unknown_loss_generation_and_bad_source_are_role_only(self):
        original = deepcopy(self.fixture)
        for mode in ("unknown", "missing", "runtime", "instance", "session", "unsafe", "stale", "future", "bad", "incompatible"):
            self.fixture = deepcopy(original)
            self.clock.wall_ms = SAMPLED + 100
            self.write_source()
            with self.subTest(mode=mode), FakeServer(capable()) as server:
                self.prepare(server)
                if mode == "unknown":
                    self.fixture["instances"][0]["sessions"][0]["activity"] = "unknown"
                elif mode == "runtime":
                    self.fixture["runtime_id"] = OTHER
                elif mode == "instance":
                    self.fixture["instances"][0]["instance_id"] = OTHER
                elif mode == "session":
                    self.fixture["instances"][0]["sessions"][0]["session_id"] = "replacement"
                elif mode == "incompatible":
                    self.fixture["schema_version"] = 2
                self.write_source()
                if mode == "missing":
                    self.path.unlink()
                elif mode == "unsafe":
                    self.path.chmod(0o644)
                elif mode == "stale":
                    self.clock.advance(3000)
                elif mode == "future":
                    self.clock.wall_ms = SAMPLED - 1
                elif mode == "bad":
                    self.path.write_bytes(b"PRIVATE malformed artifact")
                before = groups.read_groups(self.workspace)["state"]["groups"]
                result = self.publish(server)
                self.assertEqual(result["outcome"], "completed")
                self.assert_role(self.writes(server)[0])
                self.assertIn("live_source_unknown", result["diagnostics"])
                self.assertNotIn("PRIVATE", json.dumps(result))
                self.assertEqual(groups.read_groups(self.workspace)["state"]["groups"], before)
                groups.remove_member(self.workspace, "team", "m0")

    def test_unattached_and_old_schema_do_not_migrate(self):
        with FakeServer(capable()) as server:
            before = self.prepare(server, attached=False)
            with mock.patch.object(live, "inspect", side_effect=AssertionError("unattached source")):
                result = self.publish(server)
            self.assertEqual(result["acknowledged"], 1)
            self.assert_role(self.writes(server)[0])
            self.assertEqual(groups.read_groups(self.workspace)["state"]["schema_version"], before["state"]["schema_version"])

    def test_status_identity_corruption_blocks_whole_cycle(self):
        with FakeServer(capable()) as server:
            self.prepare(server)
            status = group_live.status
            for mode in ("revision", "group", "uuid", "duplicate", "missing", "role", "pins", "scope"):
                def changed(*args, **kwargs):
                    report = status(*args, **kwargs)
                    if mode == "revision":
                        report["revision"] = "other"
                    elif mode == "group":
                        report["group"]["group_id"] = OTHER
                    elif mode == "uuid":
                        report["participants"][0]["participant_id"] = OTHER
                    elif mode == "duplicate":
                        report["participants"] *= 2
                    elif mode == "missing":
                        report["participants"] = []
                    elif mode == "role":
                        report["participants"][0]["role"] = "changed"
                    elif mode == "pins":
                        report["participants"][0]["live_attachment"]["instance_id"] = OTHER
                    else:
                        report["dispatchable"] = True
                    return report
                with self.subTest(mode=mode), mock.patch.object(group_live, "status", side_effect=changed):
                    result = self.publish(server)
                self.assert_unreserved(server, result)

    def test_invalid_positive_source_cannot_supply_busy(self):
        with FakeServer(capable()) as server:
            self.prepare(server)
            status = group_live.status
            def wrong(*args, **kwargs):
                report = status(*args, **kwargs)
                report["participants"][0]["source"]["instance_id"] = OTHER
                return report
            with mock.patch.object(group_live, "status", side_effect=wrong):
                result = self.publish(server)
            self.assertEqual(result["acknowledged"], 1)
            self.assert_role(self.writes(server)[0])

    def test_group_edit_remove_readd_and_counter_only_during_status(self):
        with FakeServer(capable()) as server:
            before = self.prepare(server)
            inspect = live.inspect
            for mode in ("role", "attachment", "aba", "unrelated", "counter"):
                # Restore the declared fixture between races, never reuse a report.
                authority = self.workspace / "state/groups.json"
                authority.write_bytes(storage.encode_json(before["state"]))
                def edit(*args, **kwargs):
                    report = inspect(*args, **kwargs)
                    member = before["state"]["groups"][0]["members"][0]
                    if mode == "role":
                        groups.add_member(self.workspace, "team", "m0", "changed", member["binding"])
                    elif mode == "attachment":
                        group_live.detach(self.workspace, "team", "m0")
                    elif mode == "aba":
                        groups.remove_member(self.workspace, "team", "m0")
                        groups.add_member(self.workspace, "team", "m0", "Role 0", member["binding"])
                    elif mode == "unrelated":
                        groups.create_group(self.workspace, "other", "Other")
                    else:
                        groups.reserve_cycle(self.workspace, "team", before["revision"],
                            member["binding"]["presentation"]["endpoint"], "coven", "epoch-1", ["terminal-1"],
                            allow_new_epoch=True)
                    return report
                server.requests.clear()
                with self.subTest(mode=mode), mock.patch.object(live, "inspect", side_effect=edit):
                    result = self.publish(server)
                if mode == "counter":
                    self.assertEqual(result["acknowledged"], 1)
                    self.assertEqual(self.writes(server)[0]["revision"], 2)
                else:
                    self.assertEqual(result["reservation_attempts"], 0)
                    self.assertEqual(self.writes(server), [])

    def test_final_binding_failure_precedes_status_and_reservation(self):
        with FakeServer(capable(2)) as server:
            self.prepare(server)
            for collection in ("panes", "agents"):
                server.value[collection][1]["agent"] = "replacement"
            with mock.patch.object(group_live, "status", side_effect=AssertionError("status before bindings")):
                result = self.publish(server)
            self.assert_unreserved(server, result)

    def test_process_only_generic_snapshot_and_complete_brackets(self):
        value = pane_inventory(2)
        value["annotations"]["valid_until_unix_ms"] = True
        with FakeServer(value, process_handler()) as server:
            self.prepare(server, process=True)
            reserve = groups.reserve_cycle
            def checked(*args, **kwargs):
                self.assertEqual([r["method"] for r in server.requests],
                    ["session.snapshot"] + ["session.snapshot", "pane.process_info", "session.snapshot"] * 2)
                with storage.workspace_lock(self.workspace, self.workspace / "state/.workspace.lock", timeout=0):
                    pass
                return reserve(*args, **kwargs)
            with mock.patch.object(groups, "reserve_cycle", side_effect=checked):
                result = self.publish(server)
            self.assertEqual(result["acknowledged"], 2)

    def test_source_status_and_transport_are_outside_workspace_lock(self):
        def unlocked(_server, _stream, _request):
            with storage.workspace_lock(self.workspace, self.workspace / "state/.workspace.lock", timeout=0):
                pass
            return False
        with FakeServer(capable(), unlocked) as server:
            self.prepare(server)
            inspect = live.inspect
            def source(*args, **kwargs):
                with storage.workspace_lock(self.workspace, self.workspace / "state/.workspace.lock", timeout=0):
                    return inspect(*args, **kwargs)
            with mock.patch.object(live, "inspect", side_effect=source):
                self.assertEqual(self.publish(server)["acknowledged"], 1)

    def test_age_reduced_after_later_source_and_delayed_status_return(self):
        with FakeServer(capable(2)) as server:
            self.prepare(server)
            inspect, status = live.inspect, group_live.status
            calls = []
            def delayed_read(*args, **kwargs):
                report = inspect(*args, **kwargs)
                calls.append(report)
                self.clock.advance(100)
                return report
            def delayed_status(*args, **kwargs):
                self.assertLessEqual(kwargs["timeout"], 1)
                report = status(*args, **kwargs)
                self.clock.advance(100)
                return report
            with mock.patch.object(live, "inspect", side_effect=delayed_read), \
                    mock.patch.object(group_live, "status", side_effect=delayed_status):
                result = self.publish(server, duration=3)
            self.assertEqual(len(calls), 4)  # two scheduled cycles, freshly read
            self.assertGreaterEqual(result["acknowledged"], 2)
            for params in self.writes(server)[:2]:
                self.assertLess(params["valid_until_unix_ms"], SAMPLED + 3000)

    def test_later_member_delay_and_reservation_expiry_recompose_never_sent(self):
        with FakeServer(capable(2)) as server:
            self.prepare(server)
            self.clock.wall_ms = SAMPLED + 2800
            reserve = groups.reserve_cycle
            def slow(*args, **kwargs):
                result = reserve(*args, **kwargs)
                self.clock.advance(250)
                return result
            with mock.patch.object(groups, "reserve_cycle", side_effect=slow):
                result = self.publish(server)
            self.assertEqual(result["acknowledged"], 2)
            self.assertEqual(result["reserved_targets"], 2)
            for index, params in enumerate(self.writes(server)):
                self.assert_role(params, index)
                self.assertEqual(params["revision"], 1)
            self.assertIn("live_source_expired", result["diagnostics"])

    def test_prior_send_delay_expires_next_target_not_first(self):
        def slow(_server, _stream, request):
            if request["method"] == "pane.annotate" and request["params"]["expected_terminal_id"] == "terminal-1":
                self.clock.advance(250)
            return False
        with FakeServer(capable(2), slow) as server:
            self.prepare(server)
            self.clock.wall_ms = SAMPLED + 2800
            result = self.publish(server)
            self.assertEqual(result["acknowledged"], 2)
            self.assertIn("valid_until_unix_ms", self.writes(server)[0])
            self.assert_role(self.writes(server)[1], 1)

    def test_connect_delay_recomposes_at_transport_send_boundary(self):
        with FakeServer(capable()) as server:
            self.prepare(server)
            self.clock.wall_ms = SAMPLED + 2800
            real_socket = socket.socket
            class DelayedSocket:
                def __init__(inner, *args, **kwargs):
                    inner.stream = real_socket(*args, **kwargs)
                def __enter__(inner):
                    return inner
                def __exit__(inner, *args):
                    inner.stream.close()
                def __getattr__(inner, name):
                    return getattr(inner.stream, name)
                def connect(inner, address):
                    inner.stream.connect(address)
                    # Snapshot connects too; arrange delay only after reservation.
                    if groups.read_groups(self.workspace)["state"]["publication_counters"]:
                        self.clock.advance(250)
            network = mock.Mock(AF_UNIX=socket.AF_UNIX, SOCK_STREAM=socket.SOCK_STREAM)
            network.socket = DelayedSocket
            with mock.patch.object(publisher, "socket", network):
                result = self.publish(server)
            self.assertEqual(result["acknowledged"], 1)
            self.assert_role(self.writes(server)[0])

    def test_budget_exhaustion_after_burn_sends_nothing(self):
        with FakeServer(capable(2)) as server:
            self.prepare(server)
            reserve = groups.reserve_cycle
            def slow(*args, **kwargs):
                result = reserve(*args, **kwargs)
                self.clock.advance(3100)
                return result
            with mock.patch.object(groups, "reserve_cycle", side_effect=slow):
                result = self.publish(server, duration=5)
            self.assertEqual((result["reserved_targets"], result["unsent"]), (2, 2))
            self.assertEqual(self.writes(server), [])
            self.assertIn("timeout", result["diagnostics"])

    def test_queued_expiry_rejected_without_retry_or_repair(self):
        def queued(_server, stream, request):
            if request["method"] != "pane.annotate":
                return False
            self.clock.advance(900)
            params = request["params"]
            self.assertLessEqual(params["valid_until_unix_ms"], self.clock.wall_ms)
            stream.sendall(json.dumps({"id": request["id"], "error": {
                "code": "annotation_expired", "message": "private server detail"}}).encode() + b"\n")
            return True
        with FakeServer(capable(2), queued) as server:
            self.prepare(server)
            self.clock.wall_ms = SAMPLED + 2200
            result = self.publish(server, duration=5)
            self.assertEqual((result["rejected"], result["unsent"], result["uncertain"]), (1, 1, 0))
            self.assertEqual(len(self.writes(server)), 1)
            self.assertEqual(groups.read_groups(self.workspace)["state"]["publication_counters"][0]["watermarks"],
                             {"terminal-1": 1, "terminal-2": 1})
            self.assertNotIn("private", json.dumps(result))

    def test_residual_acceptance_whole_frame_expiry_and_later_role_restoration(self):
        state = {}
        def apply(_server, _stream, request):
            if request["method"] != "pane.annotate":
                return False
            params = request["params"]
            self.clock.advance(100)
            residual = min(params["ttl_ms"], params.get("valid_until_unix_ms", self.clock.wall_ms + 6000)
                           - self.clock.wall_ms)
            self.assertGreater(residual, 0)
            if "valid_until_unix_ms" in params:
                self.assertLessEqual(residual, 700)
                self.path.unlink()
            state.update(text=params["text"], expiry=self.clock.monotonic() + residual / 1000)
            return False
        with FakeServer(capable(), apply) as server:
            self.prepare(server)
            self.clock.wall_ms = SAMPLED + 2200
            sleep = self.clock.sleep
            def expired_before_refresh(seconds):
                sleep(seconds)
                if state["expiry"] <= self.clock.monotonic():
                    state.pop("text", None)
                    self.assertNotIn("text", state)  # whole frame, no static layer
            with mock.patch.object(self.clock, "sleep", side_effect=expired_before_refresh):
                result = self.publish(server, duration=3)
            self.assertEqual(result["acknowledged"], 2)
            self.assert_role(self.writes(server)[1])
            self.assertEqual(self.writes(server)[1]["revision"], 2)
            self.assertEqual(state["text"], "Review Team · Role 0")

    def test_malformed_lost_and_rejected_stop_with_no_repair(self):
        for mode in ("malformed", "wrong_id", "lost", "rejected"):
            def reply(_server, stream, request):
                if request["method"] != "pane.annotate":
                    return False
                if mode == "lost":
                    return True
                body = {"id": request["id"], "result": {"type": "wrong"}}
                if mode == "wrong_id":
                    body = {"id": "wrong", "result": {"type": "ok"}}
                elif mode == "rejected":
                    body = {"id": request["id"], "error": {"code": "annotation_stale_revision", "message": "secret"}}
                stream.sendall(json.dumps(body).encode() + b"\n")
                return True
            with self.subTest(mode=mode), FakeServer(capable(2), reply) as server:
                self.prepare(server)
                result = self.publish(server, duration=5)
                self.assertEqual(result["rejected"] if mode == "rejected" else result["uncertain"], 1)
                self.assertEqual(result["unsent"], 1)
                self.assertEqual(len(self.writes(server)), 1)
                for index in range(2):
                    groups.remove_member(self.workspace, "team", f"m{index}")

    def test_unknown_reservation_durability_and_interrupt_after_burn_send_nothing(self):
        with FakeServer(capable(2)) as server:
            self.prepare(server)
            reserve = groups.reserve_cycle
            for mode in ("uncertain", "error", "interrupt"):
                def burn(*args, **kwargs):
                    result = reserve(*args, **kwargs)
                    if mode == "interrupt":
                        raise KeyboardInterrupt
                    if mode == "error":
                        raise storage.StorageError("secret")
                    result["delivery"]["outcome"] = "durability_uncertain"
                    return result
                with self.subTest(mode=mode), mock.patch.object(groups, "reserve_cycle", side_effect=burn):
                    result = self.publish(server)
                self.assertEqual((result["reservation_uncertain"], result["unsent"], result["uncertain"]), (2, 2, 0))
                self.assertEqual(self.writes(server), [])
            self.assertEqual(groups.read_groups(self.workspace)["state"]["publication_counters"][0]["watermarks"],
                             {"terminal-1": 3, "terminal-2": 3})

    def test_observed_wall_or_monotonic_regression_stops_after_burn(self):
        with FakeServer(capable(2)) as server:
            self.prepare(server)
            reserve = groups.reserve_cycle
            for field in ("wall_ms", "mono_ms"):
                def backwards(*args, **kwargs):
                    result = reserve(*args, **kwargs)
                    setattr(self.clock, field, getattr(self.clock, field) - 1)
                    return result
                with self.subTest(field=field), mock.patch.object(groups, "reserve_cycle", side_effect=backwards):
                    result = self.publish(server)
                self.assertIn("live_clock_regressed", result["diagnostics"])
                self.assertEqual(result["unsent"], 2)
                self.assertEqual(self.writes(server), [])

    def test_regression_after_ack_keeps_acknowledged_phase_and_stops(self):
        def backwards(_server, _stream, request):
            if request["method"] == "pane.annotate":
                self.clock.wall_ms -= 1
            return False
        with FakeServer(capable(2), backwards) as server:
            self.prepare(server)
            result = self.publish(server)
            self.assertEqual((result["acknowledged"], result["unsent"]), (1, 1))
            self.assertIn("live_clock_regressed", result["diagnostics"])

    def test_clock_regression_during_status_blocks_reservation(self):
        with FakeServer(capable()) as server:
            self.prepare(server)
            status = group_live.status
            def backwards(*args, **kwargs):
                report = status(*args, **kwargs)
                self.clock.wall_ms -= 1
                return report
            with mock.patch.object(group_live, "status", side_effect=backwards):
                result = self.publish(server)
            self.assert_unreserved(server, result)
            self.assertIn("live_clock_regressed", result["diagnostics"])

    def test_forward_jump_and_fractional_boundary_fail_closed(self):
        for remaining in (0, 0.25, -1):
            acceptance_wall = []
            def observe(_server, _stream, request):
                if request["method"] == "pane.annotate":
                    acceptance_wall.append(self.clock.wall_ms)
                return False
            with self.subTest(remaining=remaining), FakeServer(capable(), observe) as server:
                self.prepare(server)
                reserve = groups.reserve_cycle
                def jump(*args, **kwargs):
                    result = reserve(*args, **kwargs)
                    self.clock.wall_ms = SAMPLED + 3000 - remaining
                    return result
                with mock.patch.object(groups, "reserve_cycle", side_effect=jump):
                    result = self.publish(server)
                # A genuinely positive sub-ms bound may be sent, never rounded up.
                params = self.writes(server)[0]
                if "valid_until_unix_ms" in params:
                    self.assertGreater(params["valid_until_unix_ms"] - acceptance_wall[0], 0)
                    self.assertLessEqual(params["valid_until_unix_ms"] - acceptance_wall[0], remaining)
                else:
                    self.assert_role(params)
                self.assertEqual(result["acknowledged"], 1)
                groups.remove_member(self.workspace, "team", "m0")
                self.clock.wall_ms = SAMPLED + 100

    def test_live_partial_send_and_inflight_interrupt_are_uncertain_without_retry(self):
        with FakeServer(capable(2)) as server:
            self.prepare(server)
            request = publisher._request
            for error in (OSError("partial send"), KeyboardInterrupt()):
                stream = mock.MagicMock()
                stream.__enter__.return_value = stream
                stream.sendall.side_effect = error
                network = mock.Mock(AF_UNIX=socket.AF_UNIX, SOCK_STREAM=socket.SOCK_STREAM)
                network.socket.return_value = stream
                def partial(*args, **kwargs):
                    method = kwargs.get("method", args[1] if len(args) > 1 else None)
                    if method == "pane.annotate":
                        with mock.patch.object(publisher, "socket", network):
                            return request(*args, **kwargs)
                    return request(*args, **kwargs)
                with self.subTest(error=type(error)), mock.patch.object(publisher, "_request", side_effect=partial):
                    result = self.publish(server, duration=5)
                self.assertEqual((result["uncertain"], result["unsent"], result["acknowledged"]), (1, 1, 0))
                stream.sendall.assert_called_once()
                self.assertEqual(self.writes(server), [])

    def test_later_member_io_expires_first_busy_row_before_reservation(self):
        with FakeServer(capable(2)) as server:
            self.prepare(server)
            self.clock.wall_ms = SAMPLED + 2800
            inspect = live.inspect
            calls = []
            def delayed(*args, **kwargs):
                report = inspect(*args, **kwargs)
                calls.append(report)
                if len(calls) == 2:
                    self.clock.advance(250)
                return report
            with mock.patch.object(live, "inspect", side_effect=delayed):
                result = self.publish(server)
            self.assertEqual(result["acknowledged"], 2)
            self.assertEqual(len(calls), 2)
            self.assertEqual(calls[0]["activity"], "busy")
            for index, params in enumerate(self.writes(server)):
                self.assert_role(params, index)

    def test_status_uses_remaining_cycle_budget_and_whole_timeout_never_reserves(self):
        with FakeServer(capable()) as server:
            self.prepare(server)
            snapshot, inspect = publisher.read_publication_snapshot, live.inspect
            budgets = []
            def slow_snapshot(*args, **kwargs):
                result = snapshot(*args, **kwargs)
                self.clock.advance(2700)
                return result
            def slow_source(*args, **kwargs):
                budgets.append(kwargs["timeout"])
                report = inspect(*args, **kwargs)
                self.clock.advance(400)
                return report
            with mock.patch.object(publisher, "read_publication_snapshot", side_effect=slow_snapshot), \
                    mock.patch.object(live, "inspect", side_effect=slow_source):
                result = self.publish(server, duration=5)
            self.assertAlmostEqual(budgets[0], 0.3)
            self.assert_unreserved(server, result)

    def test_terminal_move_allowed_but_replacement_stops_live_preflight(self):
        for replacement in (False, True):
            with self.subTest(replacement=replacement), FakeServer(capable()) as server:
                self.prepare(server)
                for collection in ("panes", "agents"):
                    server.value[collection][0]["pane_id"] = "moved:p9"
                    if replacement:
                        server.value[collection][0]["terminal_id"] = "replacement"
                result = self.publish(server)
                if replacement:
                    self.assert_unreserved(server, result)
                else:
                    self.assertEqual(self.writes(server)[0]["pane_id"], "moved:p9")
                    self.assertEqual(self.writes(server)[0]["expected_terminal_id"], "terminal-1")

    def test_interruption_after_valid_ack_return_remains_uncertain(self):
        with FakeServer(capable(2)) as server:
            self.prepare(server)
            request = publisher._request
            def interrupt(*args, **kwargs):
                result = request(*args, **kwargs)
                if kwargs.get("method") == "pane.annotate":
                    raise KeyboardInterrupt
                return result
            with mock.patch.object(publisher, "_request", side_effect=interrupt):
                result = self.publish(server)
            self.assertEqual((result["uncertain"], result["unsent"], result["acknowledged"]), (1, 1, 0))
            self.assertEqual(len(self.writes(server)), 1)

    def test_invalid_live_argument_precedes_state_or_socket_io(self):
        with mock.patch.object(groups, "read_groups", side_effect=AssertionError("state read")):
            for value in (None, 0, 1, "true"):
                with self.subTest(value=value), self.assertRaises(ValueError):
                    publisher.publish_group(self.workspace, "team", "/unused", "coven", live=value)

    def test_outbound_horizon_never_increases_when_wall_outpaces_elapsed(self):
        clock = publisher._LiveClock()
        lease = {"wall": SAMPLED + 3000, "mono": 102.9, "sample": SAMPLED}
        params = {"text": "role · legacy runner busy", "ttl_ms": 3000}
        summary = {"diagnostics": []}
        bounds = []
        for delta in (0, 10, 20, 30):
            self.clock.wall_ms += delta
            self.clock.mono_ms += 40
            publisher._prepare_live(params, "role", lease, clock, 110, summary)
            bounds.append(params["valid_until_unix_ms"])
        self.assertEqual(bounds, sorted(bounds, reverse=True))

    def test_cli_opt_in_exact_summary_exit_and_help(self):
        with FakeServer(capable()) as server:
            self.prepare(server)
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = coven.main(["group", "publish", str(self.workspace), "team", "--socket", server.path,
                                   "--source", "coven", "--live", "--duration", "1", "--json"])
            self.assertEqual(code, 0)
            self.assertEqual(json.loads(output.getvalue())["publication_mode"], "live")
            output = io.StringIO()
            with contextlib.redirect_stdout(output), self.assertRaises(SystemExit) as raised:
                coven.main(["group", "publish", "--help"])
            self.assertEqual(raised.exception.code, 0)
            self.assertIn("--live", output.getvalue())
            self.assertIn("legacy runner", output.getvalue())


if __name__ == "__main__":
    unittest.main()
