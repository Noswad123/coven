"""Offline contracts for generic pane evidence, not legacy agent classification."""

import copy
import json
import unittest
from unittest import mock

from runner import group_publisher as publisher, groups
from runner import kitsune_observation as observation
from runner import kitsune_process_observation as process
from test_group_publisher import FakeServer, inventory
from test_process_groups import (
    BRACKET, ProcessGroupCase, pane_inventory, process_handler, process_result,
    processes, send_result,
)


LOCATORS = ("pane_id", "terminal_id", "workspace_id", "tab_id")


def sparse(count=1):
    value = inventory(count)
    return {"version": value["version"], "protocol": value["protocol"],
            "panes": [{key: pane[key] for key in LOCATORS} for pane in value["panes"]],
            "annotations": value["annotations"]}


def malformed_legacy(value):
    value.update(agents={"SECRET": "not an inventory"}, workspaces=False,
                 tabs=None, layouts="SECRET", focused_workspace_id=[],
                 focused_tab_id={}, focused_pane_id=42, revision=False)
    for pane in value["panes"]:
        pane.update(agent=["SECRET"], agent_status={}, agent_session="SECRET",
                    focused=None, revision=-1, name=False, launch_pending="SECRET",
                    interactive_ready=[], screen_detection_skipped={}, state_change_seq=-1)
    return value


def frame(value, request_id="request"):
    return json.dumps({"id": request_id, "result": {
        "type": "session_snapshot", "snapshot": value}}).encode()


class PaneSnapshotDecoderTests(unittest.TestCase):
    def decode(self, value):
        return observation._pane_snapshot(frame(value), "request")

    def test_minimal_allowlist_and_annotation_presence_preserved(self):
        for descriptor in ("absent", None, {}, {"epoch": False}, ["SECRET"]):
            with self.subTest(descriptor=descriptor):
                value = malformed_legacy(sparse())
                if descriptor == "absent":
                    value.pop("annotations")
                else:
                    value["annotations"] = descriptor
                expected = {key: value[key] for key in ("version", "protocol")}
                expected["panes"] = [{key: pane[key] for key in LOCATORS}
                                     for pane in value["panes"]]
                if descriptor != "absent":
                    expected["annotations"] = descriptor
                self.assertEqual(self.decode(value), expected)

    def test_opaque_unicode_ids_and_shared_workspace_tab_are_not_inferred(self):
        value = sparse(2)
        value["panes"] = [
            dict(zip(LOCATORS, ("Pane/\u00e9", "terminal:\u00df", "unrelated:\u03bb", "TAB/Z"))),
            dict(zip(LOCATORS, ("pane/\u00e9", "Terminal:\u00df", "unrelated:\u03bb", "TAB/Z"))),
        ]
        self.assertEqual(self.decode(value), value)

    def test_every_locator_required_bounded_printable_and_without_whitespace(self):
        for key in LOCATORS:
            for bad in (None, False, 1, [], {}, "", "x" * 4097, "a b", "a\tb",
                        "a\nb", "a\x00b", "a\x7fb", "a\u00a0b", "\ud800"):
                with self.subTest(key=key, bad=repr(bad)):
                    value = sparse(2)
                    value["panes"][1][key] = bad
                    with self.assertRaises(observation._Failure):
                        self.decode(value)
            value = sparse()
            del value["panes"][0][key]
            with self.subTest(key=key, missing=True), self.assertRaises(observation._Failure):
                self.decode(value)
        value = sparse()
        for key in LOCATORS:
            value["panes"][0][key] = "x" * 4096
        self.assertEqual(self.decode(value), value)

    def test_duplicate_pane_or_terminal_anywhere_is_rejected(self):
        for key in ("pane_id", "terminal_id"):
            with self.subTest(key=key):
                value = sparse(3)
                value["panes"][2][key] = value["panes"][1][key]
                with self.assertRaises(observation._Failure):
                    self.decode(value)

    def test_pane_collection_and_version_protocol_contract(self):
        cases = [("panes", None), ("panes", {}), ("panes", [None]),
                 ("panes", [{}]), ("version", None), ("version", 18),
                 ("version", "v" * 4097), ("protocol", True), ("protocol", "18"),
                 ("protocol", 18.0), ("protocol", -1), ("protocol", 2**32)]
        for key, bad in cases:
            with self.subTest(key=key, bad=repr(bad)):
                value = sparse()
                value[key] = bad
                with self.assertRaises(observation._Failure):
                    self.decode(value)
        for protocol in (0, 17, 19):
            with self.subTest(protocol=protocol):
                value = sparse()
                value["protocol"] = protocol
                with self.assertRaises(observation._Failure) as raised:
                    self.decode(value)
                self.assertEqual(raised.exception.code, "unsupported_protocol")
        value = sparse(1024)
        self.assertEqual(len(self.decode(value)["panes"]), 1024)
        value["panes"].append({**value["panes"][-1], "pane_id": "extra", "terminal_id": "extra"})
        with self.assertRaises(observation._Failure):
            self.decode(value)
        value["panes"] = []
        self.assertEqual(self.decode(value)["panes"], [])

    def test_envelope_correlation_result_type_and_server_error(self):
        valid = json.loads(frame(sparse()))
        cases = [[], {}, {**valid, "id": "stale"}, {"id": "request"},
                 {**valid, "error": {}}, {**valid, "result": []},
                 {**valid, "result": {"type": "pane_process_info", "snapshot": sparse()}},
                 {**valid, "result": {"type": "session_snapshot"}},
                 {"id": "request", "error": {"code": "gone", "message": "SECRET"}}]
        for root in cases:
            with self.subTest(root=root), self.assertRaises(observation._Failure):
                observation._pane_snapshot(json.dumps(root).encode(), "request")

    def test_global_json_limits_apply_even_to_ignored_fields(self):
        nested = 0
        for _ in range(33):
            nested = [nested]
        for bad in ("SECRET" * 700, [0] * 4097, {str(i): 0 for i in range(4097)},
                    nested, float("inf"), "\ud800"):
            with self.subTest(kind=type(bad).__name__):
                value = sparse()
                value["agents"] = bad
                with self.assertRaises(observation._Failure):
                    self.decode(value)
        valid = frame(sparse())
        for payload in (b"\xff", valid + b"{}", valid.replace(b'"protocol": 18',
                         b'"protocol": 18, "protocol": 18'),
                         valid[:-1] + b', "future": NaN}'):
            with self.subTest(payload=payload[:30]), self.assertRaises(observation._Failure):
                observation._pane_snapshot(payload, "request")

    def test_existing_strict_decoder_still_rejects_sparse_and_legacy_corruption(self):
        for value in (sparse(), malformed_legacy(inventory())):
            with self.subTest(sparse="agents" not in value):
                self.decode(value)
                with self.assertRaises(observation._Failure):
                    observation._snapshot(frame(value), "request")


class ProcessSnapshotWorkflowTests(ProcessGroupCase):
    def setUp(self):
        super().setUp()
        for module in (publisher, observation, process):
            patch = mock.patch.object(module, "time", self.clock)
            patch.start()
            self.addCleanup(patch.stop)

    def methods(self, server):
        return [request["method"] for request in server.requests]

    def clear_members(self):
        # A failed subtest must not leave bindings to its now-closed fake endpoint.
        group, = groups.read_groups(self.workspace)["state"]["groups"]
        for member in group["members"]:
            groups.remove_member(self.workspace, "team", member["key"])

    def assert_read_only(self, server, before, report=None):
        self.assertEqual(self.state_bytes(), before)
        self.assertEqual(groups.read_groups(self.workspace)["state"]["publication_counters"],
                         json.loads(before)["publication_counters"])
        self.assertNotIn("pane.annotate", self.methods(server))
        if report is not None:
            self.assertNotIn("SECRET", json.dumps(report))
            self.assertNotIn("/private/", json.dumps(report))

    def failed_publish(self, server, code):
        before = self.state_bytes()
        with mock.patch.object(groups, "reserve_cycle", wraps=groups.reserve_cycle) as reserve:
            report = self.publish(server)
        reserve.assert_not_called()
        self.assert_unreserved(server, report, before, code)
        self.assert_read_only(server, before, report)

    def test_sparse_process_inspect_and_cli_add_publish_without_legacy_inventories(self):
        for value in (sparse(), malformed_legacy(sparse())):
            with self.subTest(malformed="agents" in value), FakeServer(value, process_handler()) as server:
                self.clear_members()
                before = self.state_bytes()
                report = process.inspect(server.path, "w1:p1")
                self.assertEqual(report["assessment"], "observed")
                self.assertEqual(report["diagnostics"], [])
                self.assertEqual(report["authorization"], "none")
                self.assertFalse(report["dispatchable"])
                self.assertEqual(self.methods(server), BRACKET)
                self.assert_read_only(server, before, report)
                server.requests.clear()
                self.add(server)
                self.assertEqual(self.methods(server), BRACKET)
                server.requests.clear()
                result = self.call("group", "publish", self.workspace, "team", "--socket",
                                   server.path, "--source", "coven", "--duration", "1", "--json")
                self.assertEqual(result["acknowledged"], 1)
                self.assertEqual(self.methods(server), ["session.snapshot"] + BRACKET + ["pane.annotate"])
                self.assertNotIn("SECRET", self.state_bytes().decode())
                self.assertNotIn("/private/", self.state_bytes().decode())
                groups.remove_member(self.workspace, "team", "one")

    def test_default_observation_publication_reader_and_snapshot_add_remain_strict(self):
        for value in (sparse(), malformed_legacy(inventory())):
            with self.subTest(sparse="agents" not in value), FakeServer(value, process_handler()) as server:
                before = self.state_bytes()
                report = observation.inspect(server.path)
                self.assertEqual(report["diagnostics"], ["schema_error"])
                self.assertEqual(report["agents"], [])
                with self.assertRaises(publisher.PublisherError) as raised:
                    publisher.read_publication_snapshot(server.path, deadline=self.clock.now + 1)
                self.assertEqual(raised.exception.code, "schema_error")
                self.invoke(*self.add_args(server, process=False), error="schema_error")
                self.assertEqual(self.methods(server), ["session.snapshot"] * 3)
                self.assert_read_only(server, before, report)

    def test_process_only_publication_projection_never_reads_raw_agents(self):
        for raw in (None, False, "SECRET", [{"agent_session": "SECRET"}]):
            with self.subTest(raw=raw):
                value = malformed_legacy(sparse())
                value["agents"] = raw
                with FakeServer(value) as server:
                    before = self.state_bytes()
                    projected = publisher.read_publication_snapshot(
                        server.path, deadline=self.clock.now + 1, process_only=True)
                    self.assertEqual(projected["agents"], [])
                    self.assertEqual(projected["panes"], sparse()["panes"])
                    self.assertEqual(projected["epoch"], "epoch-1")
                    self.assertEqual(projected["sources"], ["coven"])
                    self.assertEqual(self.methods(server), ["session.snapshot"])
                    self.assert_read_only(server, before, projected)

    def test_only_all_process_members_select_process_only_initial_reader(self):
        for kinds in ((True, True), (True, False), (False, False)):
            with self.subTest(kinds=kinds), FakeServer(pane_inventory(2, agents=True), process_handler()) as server:
                self.clear_members()
                for index, kind in enumerate(kinds):
                    self.add(server, member=f"m{index}", pane=f"w1:p{index + 1}", process=kind)
                server.requests.clear()
                with mock.patch.object(publisher, "read_publication_snapshot",
                                       wraps=publisher.read_publication_snapshot) as reader:
                    report = self.publish(server)
                self.assertEqual(report["acknowledged"], 2)
                reader.assert_called_once()
                self.assertIs(reader.call_args.kwargs.get("process_only", False), all(kinds))
                self.assertEqual(self.methods(server), ["session.snapshot"] + BRACKET * sum(kinds)
                                 + ["pane.annotate"] * 2)
                for index in range(2):
                    groups.remove_member(self.workspace, "team", f"m{index}")

    def test_mixed_and_snapshot_only_initial_invalid_agent_metadata_never_falls_back(self):
        for mixed in (False, True):
            with self.subTest(mixed=mixed), FakeServer(pane_inventory(2, agents=True), process_handler()) as server:
                self.clear_members()
                self.add(server, process=False)
                if mixed:
                    self.add(server, member="two", pane="w1:p2")
                server.value["agents"][0]["agent_session"] = "SECRET"
                server.requests.clear()
                self.failed_publish(server, "schema_error")
                self.assertEqual(self.methods(server), ["session.snapshot"])
                groups.remove_member(self.workspace, "team", "one")
                if mixed:
                    groups.remove_member(self.workspace, "team", "two")

    def test_mixed_process_brackets_ignore_legacy_fields_after_strict_initial(self):
        with FakeServer(pane_inventory(2, agents=True), process_handler()) as server:
            self.add(server, process=False)
            self.add(server, member="two", pane="w1:p2")
            server.requests.clear()
            normal = process_handler()
            reads = 0

            def change(server, stream, request):
                nonlocal reads
                if request["method"] == "session.snapshot":
                    reads += 1
                    if reads >= 2:
                        malformed_legacy(server.value)
                        server.value["annotations"] = None
                return normal(server, stream, request)

            server.handler = change
            report = self.publish(server)
            self.assertEqual(report["acknowledged"], 2)
            self.assertEqual(self.methods(server), ["session.snapshot"] + BRACKET + ["pane.annotate"] * 2)

    def test_invalid_snapshot_initial_or_either_bracket_prevents_any_reservation(self):
        for stage in (1, 2, 3):
            for fault in ("duplicate_pane", "duplicate_terminal", "unselected", "protocol", "envelope"):
                with self.subTest(stage=stage, fault=fault), FakeServer(pane_inventory(3), process_handler()) as server:
                    self.clear_members()
                    self.add(server)
                    server.value = sparse(3)
                    server.requests.clear()
                    normal = process_handler()
                    reads = 0

                    def corrupt(server, stream, request):
                        nonlocal reads
                        if request["method"] == "session.snapshot":
                            reads += 1
                            if reads == stage:
                                if fault.startswith("duplicate"):
                                    key = "pane_id" if fault == "duplicate_pane" else "terminal_id"
                                    server.value["panes"][2][key] = server.value["panes"][1][key]
                                elif fault == "unselected":
                                    server.value["panes"][2]["tab_id"] = "bad tab"
                                elif fault == "protocol":
                                    server.value["protocol"] = 17
                                else:
                                    send_result(stream, {"id": "stale"}, {
                                        "type": "session_snapshot", "snapshot": server.value})
                                    return True
                        return normal(server, stream, request)

                    server.handler = corrupt
                    self.failed_publish(server, "unsupported_protocol" if fault == "protocol" else "schema_error")
                    expected = {1: ["session.snapshot"], 2: ["session.snapshot"] * 2,
                                3: ["session.snapshot"] + BRACKET}[stage]
                    self.assertEqual(self.methods(server), expected)
                    groups.remove_member(self.workspace, "team", "one")

    def test_ignored_payload_still_bounded_for_inspect_add_and_publication(self):
        for bad in ("SECRET" * 700, [0] * 4097, float("nan")):
            with self.subTest(kind=type(bad).__name__), FakeServer(pane_inventory(), process_handler()) as server:
                self.clear_members()
                self.add(server)
                server.value = sparse()
                server.value["agents"] = bad
                before = self.state_bytes()
                report = process.inspect(server.path, "w1:p1")
                code = "nonfinite_json" if isinstance(bad, float) else "schema_error"
                self.assertEqual(report["diagnostics"], [code])
                self.assertEqual(report["observations"], [])
                self.invoke(*self.add_args(server), "--rebind", error=code)
                self.assert_read_only(server, before, report)
                self.failed_publish(server, code)
                self.assertEqual(self.methods(server)[-3:], ["session.snapshot"] * 3)
                groups.remove_member(self.workspace, "team", "one")

    def test_annotation_semantics_ignored_by_standalone_but_strict_for_publication(self):
        descriptors = [("absent", "annotations_unsupported"), (None, "schema_error"),
                       ({}, "schema_error"), ({"epoch": "", "sources": []}, "schema_error"),
                       ({"epoch": "e", "sources": ["coven", "coven"]}, "schema_error"),
                       ({"epoch": "e", "sources": [False]}, "schema_error"),
                       ({"epoch": "e", "sources": ["a/b"]}, "schema_error"),
                       ({"epoch": "e", "sources": ["x"] * 9}, "schema_error"),
                       ({"epoch": "e", "sources": []}, "source_disabled")]
        for descriptor, code in descriptors:
            with self.subTest(descriptor=descriptor), FakeServer(sparse(), process_handler()) as server:
                self.clear_members()
                if descriptor == "absent":
                    del server.value["annotations"]
                else:
                    server.value["annotations"] = copy.deepcopy(descriptor)
                before = self.state_bytes()
                report = process.inspect(server.path, "w1:p1")
                self.assertEqual(report["assessment"], "observed")
                self.assert_read_only(server, before, report)
                self.add(server)
                server.requests.clear()
                self.failed_publish(server, code)
                self.assertEqual(self.methods(server), ["session.snapshot"])
                groups.remove_member(self.workspace, "team", "one")

    def test_process_only_projection_keeps_one_shared_wire_budget(self):
        with FakeServer(pane_inventory(), process_handler(padding=1100)) as server:
            self.add(server)
            server.value = malformed_legacy(sparse())
            server.value["future"] = "s" * 1100
            snapshot_bytes = len(frame(server.value, "x" * 32)) + 1
            process_bytes = len(json.dumps({"id": "x" * 32, "result": {
                **process_result("w1:p1", processes()), "future": "p" * 1100}}).encode()) + 1
            total = 3 * snapshot_bytes + process_bytes
            before = self.state_bytes()
            server.requests.clear()
            real_budget = observation._ReadBudget
            budgets = []

            def capped(remaining):
                budget = real_budget(min(remaining, total - 1))
                budgets.append(budget)
                return budget

            with mock.patch.object(observation, "_ReadBudget", side_effect=capped):
                self.failed_publish(server, "response_limit")
            self.assertEqual(len(budgets), 1)
            self.assertEqual(self.methods(server), ["session.snapshot"] + BRACKET)
            self.assert_read_only(server, before)


if __name__ == "__main__":
    unittest.main()
