"""Read-only inventory/declaration contracts using private sockets only."""

import contextlib
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import time
import unittest
from unittest import mock
import uuid

from runner import actions, coven, groups, storage, launch, workers, group_live
from runner import djinn_observation, djinn_live_observation, interactive_sessions
from runner import group_publisher as publisher, session_inspection as inspection
from runner import kitsune_observation as wire
from test_coven import CovenTestCase, snapshot
from test_group_publisher import FakeServer, inventory
from test_process_groups import process_handler, processes, send_result, process_result


def generic(count=1):
    value = inventory(count)
    value.update(agents={"SECRET": "ignored legacy metadata"}, workspaces=False, tabs=None,
                 layouts="SECRET", focused_pane_id=[])
    for pane in value["panes"]:
        pane.update(agent=["SECRET"], agent_session="SECRET", agent_status={}, revision=-1,
                    title="SECRET TITLE", cwd="/SECRET", tokens={"SECRET": "not output"})
    return value


class SessionInspectionTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace, "--blank", "--agents", "operator")
        for obj, name in ((subprocess, "run"), (subprocess, "Popen"), (launch, "execute"),
                          (workers, "inspect_readiness"), (interactive_sessions, "execute"),
                          (coven, "send_to_agent_pane"), (group_live, "status"),
                          (djinn_observation, "inspect"), (djinn_live_observation, "inspect")):
            patcher = mock.patch.object(obj, name, side_effect=AssertionError("not read-only"))
            guard = patcher.start()
            self.addCleanup(patcher.stop)
            self.addCleanup(guard.assert_not_called)

    def args(self, server, *, mode="list", pane="w1:p1", terminal=None):
        args = ["session", mode, str(self.workspace), "--socket", server.path, "--json"]
        if mode == "inspect":
            args.extend(["--pane", pane])
            if terminal is not None:
                args.extend(["--expected-terminal", terminal])
        return args

    def call(self, server, *, expected=0, **options):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(self.args(server, **options)), expected)
        return json.loads(output.getvalue())

    def unavailable(self, report, code=None):
        self.assertEqual(report["assessment"], "unavailable")
        self.assertEqual(report["targets"], [])
        self.assertIsNone(report["target_count"])
        self.assertEqual(report["declarations"]["assessment"], "unavailable")
        if code is not None:
            self.assertEqual(report["diagnostics"], [code])

    def add(self, server, *, member="one", pane="w1:p1", legacy=False, binding=None):
        if not groups.read_groups(self.workspace)["initialized"]:
            groups.create_group(self.workspace, "team", "Review team")
        if binding is None:
            if legacy:
                value = inventory(2, native={"source": "declared", "agent": "djinn", "kind": "id",
                                             "value": "explicit-declared-native-id"})
                binding = publisher.binding_from_snapshot({"endpoint": publisher._endpoint(server.path),
                    "observed_at": "2026-10-26T00:00:00+00:00", "agents": value["agents"]}, pane)
            else:
                binding = publisher.binding_from_process(server.path, pane, deadline=time.monotonic()+3)
        groups.add_member(self.workspace, "team", member, "Reviewer", binding)
        server.requests.clear()
        return binding

    def test_list_uses_n_plus_two_generic_reads_without_legacy_inventory(self):
        with FakeServer(generic(2), process_handler(processes()+processes(11, "djinn-ui"))) as server:
            before = snapshot(self.workspace)
            report = self.call(server)
            self.assertEqual([r["method"] for r in server.requests],
                ["session.snapshot", "pane.process_info", "pane.process_info", "session.snapshot"])
            self.assertEqual(report["target_count"], 2)
            self.assertEqual(report["recognized_target_count"], 2)
            self.assertTrue(all(len(t["matches"]) == 2 for t in report["targets"]))
            self.assertFalse(report["native_catalog_complete"])
            self.assertFalse(report["dispatchable"])
            self.assertEqual(report["authorization"], "none")
            self.assertNotIn("SECRET", json.dumps(report))
            self.assertEqual(snapshot(self.workspace), before)

    def test_inspect_reads_only_requested_target_and_copies_fresh_ids(self):
        with FakeServer(generic(3), process_handler()) as server:
            report = self.call(server, mode="inspect", pane="w1:p2", terminal="terminal-2")
            self.assertEqual([(r["method"], r["params"]) for r in server.requests], [
                ("session.snapshot", {}), ("pane.process_info", {"pane_id": "w1:p2"}),
                ("session.snapshot", {})])
            self.assertEqual(report["targets"][0]["terminal_id"], "terminal-2")
            self.assertEqual(report["target_count"], 1)

    def test_valid_nonmatch_and_missing_argv_remain_observed_unknown(self):
        for items in ([], [{"pid": 10, "name": "djinn", "argv": None}],
                      [{"pid": 10, "name": "djinn", "argv": ["node", "/private/djinn-ui"]}]):
            with self.subTest(items=items), FakeServer(generic(), process_handler(items)) as server:
                report = self.call(server)
                target, = report["targets"]
                self.assertEqual(target["recognition"], "unknown")
                self.assertEqual(target["activity"], "unknown")
                self.assertEqual(target["attention"], "unknown")
                self.assertEqual(target["native_lifecycle"], "unestablished")
                self.assertEqual(target["matches"], [])

    def test_empty_inventory_is_not_a_native_catalog_or_presence_claim(self):
        with FakeServer(generic(0), process_handler()) as server:
            report = self.call(server)
            self.assertEqual(report["assessment"], "observed")
            self.assertEqual(report["target_count"], 0)
            self.assertFalse(report["native_catalog_complete"])
            self.assertEqual(len(server.requests), 2)

    def test_uninitialized_groups_are_empty_without_migration_or_locks(self):
        with FakeServer(generic(), process_handler()) as server:
            before = snapshot(self.workspace)
            with mock.patch.object(storage, "workspace_lock", side_effect=AssertionError("lock")), \
                 mock.patch.object(storage, "transport_gate", side_effect=AssertionError("gate")):
                report = self.call(server)
            self.assertEqual(report["declarations"]["assessment"], "uninitialized")
            self.assertEqual(report["targets"][0]["associations"], [])
            self.assertEqual(snapshot(self.workspace), before)

    def test_inspection_is_allowed_in_approved_launch_mode_without_consulting_action_control(self):
        self.invoke("action", "enable-runtime-launch", self.workspace, "--json")
        with FakeServer(generic(), process_handler()) as server:
            before = snapshot(self.workspace)
            with mock.patch.object(actions.ActionStore, "read_control", side_effect=AssertionError("control")), \
                 mock.patch.object(storage, "workspace_lock", side_effect=AssertionError("lock")), \
                 mock.patch.object(storage, "transport_gate", side_effect=AssertionError("gate")):
                self.call(server)
            self.assertEqual(snapshot(self.workspace), before)

    def test_role_and_historical_native_reference_are_declarations_only(self):
        with FakeServer(generic(), process_handler()) as server:
            self.add(server, legacy=True)
            before = snapshot(self.workspace)
            report = self.call(server)
            association, = report["targets"][0]["associations"]
            self.assertEqual(association["role"], "Reviewer")
            self.assertEqual(association["association_basis"], "explicit_declaration")
            self.assertEqual(association["binding_check"], "legacy_evidence_not_revalidated")
            self.assertEqual(association["declared_native_reference"]["value"], "explicit-declared-native-id")
            self.assertEqual(association["native_identity"], "unestablished")
            self.assertEqual(snapshot(self.workspace), before)

    def test_process_binding_match_change_and_same_terminal_move(self):
        with FakeServer(generic(), process_handler()) as server:
            self.add(server)
            report = self.call(server)
            self.assertEqual(report["targets"][0]["associations"][0]["binding_check"], "matching_process_evidence")
            # An already completed same-terminal move preserves the declaration;
            # its old pane-at-binding must not become an action target or identity.
            server.value["panes"][0]["pane_id"] = "moved-pane"
            server.value["panes"][0]["tab_id"] = "moved-tab"
            report = self.call(server)
            association = report["targets"][0]["associations"][0]
            self.assertEqual(association["declared_pane_id"], "w1:p1")
            self.assertEqual(association["binding_check"], "matching_process_evidence")
            server.handler = process_handler(processes(99))
            report = self.call(server)
            self.assertEqual(report["targets"][0]["associations"][0]["binding_check"], "process_evidence_changed")

    def test_endpoint_and_terminal_mismatches_are_unmatched_not_followed(self):
        for mode in ("endpoint", "terminal", "other_endpoint"):
            with self.subTest(mode=mode), FakeServer(generic(), process_handler()) as server:
                binding = publisher.binding_from_process(server.path, "w1:p1", deadline=time.monotonic()+3)
                if mode == "endpoint":
                    binding["presentation"]["endpoint"]["inode"] += 1
                elif mode == "terminal":
                    binding["presentation"]["terminal_id"] = "old-terminal"
                else:
                    binding["presentation"]["endpoint"]["path"] = "/other/endpoint.sock"
                self.add(server, binding=binding, member=mode)
                report = self.call(server)
                self.assertEqual(report["targets"][0]["associations"], [])
                if mode == "other_endpoint":
                    self.assertEqual(report["declarations"]["other_endpoint_count"], 1)
                else:
                    self.assertTrue(any(a["binding_check"] == (
                        "endpoint_identity_changed" if mode == "endpoint" else "terminal_not_observed")
                        for a in report["declarations"]["unmatched"]))
                # Keep each subcase's declarations independent.
                groups.remove_member(self.workspace, "team", mode)

    def test_inspect_does_not_call_unselected_members_stale(self):
        with FakeServer(generic(2), process_handler()) as server:
            self.add(server, pane="w1:p2")
            report = self.call(server, mode="inspect")
            self.assertEqual(report["declarations"]["outside_inspection_count"], 1)
            self.assertEqual(report["declarations"]["unmatched"], [])

    def test_live_attachment_is_declared_but_never_opened_or_promoted_to_activity(self):
        with FakeServer(generic(), process_handler()) as server:
            self.add(server)
            view = groups.read_groups(self.workspace)
            member = view["state"]["groups"][0]["members"][0]
            runtime = str(uuid.uuid4())
            attachment = {"adapter": "djinn_live", "source_contract": "djinn.live-observation.v1",
                "artifact_path": "/never/open/"+runtime+".json", "runtime_id": runtime,
                "instance_id": str(uuid.uuid4()), "session_id": "native-selector",
                "attached_at": "2026-10-26T00:00:00+00:00"}
            groups.attach_live(self.workspace, "team", "one", attachment, expected_revision=view["revision"],
                               expected_participant_id=member["participant_id"])
            report = self.call(server)
            target, = report["targets"]
            self.assertTrue(target["associations"][0]["live_attachment_declared"])
            self.assertEqual(target["activity"], "unknown")
            self.assertNotIn("/never/open", json.dumps(report))

    def test_missing_or_mismatched_inspect_pin_stops_before_process_read(self):
        for pane, terminal, diagnostic in (("absent", None, "pane_not_found"),
                                            ("w1:p1", "wrong", "terminal_mismatch")):
            with self.subTest(pane=pane), FakeServer(generic(), process_handler()) as server:
                self.unavailable(self.call(server, mode="inspect", pane=pane, terminal=terminal, expected=1), diagnostic)
                self.assertEqual(len(server.requests), 1)

    def test_list_pane_limit_is_explicit_but_inspect_can_select_one(self):
        with FakeServer(generic(65), process_handler()) as server:
            self.unavailable(self.call(server, expected=1), "pane_limit")
            self.assertEqual(len(server.requests), 1)
            server.requests.clear()
            self.assertEqual(self.call(server, mode="inspect")["target_count"], 1)
            self.assertEqual(len(server.requests), 3)

    def test_list_global_mapping_drift_discards_all_positive_rows(self):
        for mode in ("terminal", "pane", "tab", "workspace", "add", "remove"):
            changed_once = []
            def changed(server, stream, request):
                if request["method"] == "pane.process_info":
                    if not changed_once:
                        changed_once.append(True)
                        if mode in ("terminal", "pane", "tab", "workspace"):
                            field = mode+"_id"
                            server.value["panes"][1][field] = "changed"
                        elif mode == "add":
                            server.value["panes"].append(inventory(3)["panes"][2])
                        else:
                            server.value["panes"].pop()
                    send_result(stream, request, process_result(request["params"]["pane_id"], processes()))
                    return True
                return False
            with self.subTest(mode=mode), FakeServer(generic(2), changed) as server:
                self.unavailable(self.call(server, expected=1), "pane_mapping_changed")

    def test_inspect_tolerates_unrelated_mapping_change_but_not_target_change(self):
        for index, expected in ((1, 0), (0, 1)):
            def changed(server, stream, request):
                if request["method"] == "pane.process_info":
                    server.value["panes"][index]["tab_id"] = "new-tab"
                return process_handler()(server, stream, request)
            with self.subTest(index=index), FakeServer(generic(2), changed) as server:
                report = self.call(server, mode="inspect", expected=expected)
                if expected:
                    self.unavailable(report, "pane_mapping_changed")
                else:
                    self.assertEqual(report["assessment"], "observed")

    def test_snapshot_order_focus_and_legacy_status_do_not_define_identity(self):
        def changed(server, stream, request):
            if request["method"] == "pane.process_info":
                server.value["panes"].reverse()
                server.value["agents"] = []
                for pane in server.value["panes"]:
                    pane["focused"] = True
                    pane["agent_status"] = "done"
                    pane["revision"] = 99
            return process_handler()(server, stream, request)
        with FakeServer(generic(2), changed) as server:
            report = self.call(server)
            self.assertEqual(report["target_count"], 2)
            self.assertTrue(all(t["activity"] == "unknown" for t in report["targets"]))

    def test_version_or_unsupported_protocol_is_not_silently_accepted(self):
        def changed(server, stream, request):
            if request["method"] == "pane.process_info":
                server.value["version"] = "replacement build"
            return process_handler()(server, stream, request)
        with FakeServer(generic(), changed) as server:
            self.unavailable(self.call(server, expected=1), "server_context_changed")
        value = generic()
        value["protocol"] = 19
        with FakeServer(value, process_handler()) as server:
            report = self.call(server, expected=2)
            self.assertEqual(report["assessment"], "incompatible")
            self.assertEqual(report["targets"], [])

    def test_group_content_or_schema_drift_discards_rows(self):
        for mode in ("content", "schema"):
            changed_once = []
            def changed(server, stream, request):
                if request["method"] == "pane.process_info" and not changed_once:
                    changed_once.append(True)
                    if mode == "content":
                        groups.create_group(self.workspace, "another", "Another group")
                    else:
                        path = self.workspace/"state/groups.json"
                        value = json.loads(path.read_text())
                        value["schema_version"] = 2
                        path.write_text(json.dumps(value))
                return process_handler()(server, stream, request)
            with self.subTest(mode=mode), FakeServer(generic(), process_handler()) as server:
                self.add(server, legacy=True)
                server.handler = changed
                self.unavailable(self.call(server, expected=1), "groups_changed")
                # Schema-only subcase must begin at schema 1 on an independent authority.
                (self.workspace/"state/groups.json").unlink()

    def test_counter_only_group_drift_is_not_a_declaration_change(self):
        fired = []
        def changed(server, stream, request):
            if request["method"] == "pane.process_info" and not fired:
                fired.append(True)
                view = groups.read_groups(self.workspace)
                groups.reserve_cycle(self.workspace, "team", view["revision"], publisher._endpoint(server.path),
                    "test", "epoch", ["terminal-1"], allow_new_epoch=True)
            return process_handler()(server, stream, request)
        with FakeServer(generic(), process_handler()) as server:
            self.add(server)
            revision = groups.read_groups(self.workspace)["revision"]
            server.handler = changed
            report = self.call(server)
            self.assertEqual(report["declarations"]["revision"], revision)
            self.assertTrue(fired)

    def test_corrupt_authority_stops_before_socket_and_preserves_records(self):
        (self.workspace/"state/groups.json").write_text("not JSON")
        with FakeServer(generic(), process_handler()) as server:
            before = snapshot(self.workspace)
            self.unavailable(self.call(server, expected=1), "groups_unavailable")
            self.assertEqual(server.requests, [])
            self.assertEqual(snapshot(self.workspace), before)

    def test_second_groups_read_failure_discards_observations(self):
        original = groups.read_groups
        calls = []
        def fail_second(*args, **kwargs):
            calls.append(True)
            if len(calls) == 2:
                raise storage.StorageError("PRIVATE error")
            return original(*args, **kwargs)
        with FakeServer(generic(), process_handler()) as server, mock.patch.object(groups, "read_groups", side_effect=fail_second):
            report = self.call(server, expected=1)
            self.unavailable(report, "groups_unavailable")
            self.assertNotIn("PRIVATE", json.dumps(report))

    def test_endpoint_replaced_after_last_api_read_discards_observations(self):
        original = groups.read_groups
        calls, replacements = [], []
        def replace_after_reads(*args, **kwargs):
            result = original(*args, **kwargs)
            calls.append(True)
            if len(calls) == 2:
                os.unlink(server.path)
                replacement = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                replacements.append(replacement)
                replacement.bind(server.path)
                os.chmod(server.path, 0o600)
            return result
        try:
            with FakeServer(generic(), process_handler()) as server:
                with mock.patch.object(groups, "read_groups", side_effect=replace_after_reads):
                    self.unavailable(self.call(server, expected=1), "endpoint_replaced")
        finally:
            for replacement in replacements:
                replacement.close()

    def test_malformed_process_response_is_not_a_partial_positive(self):
        def malformed(server, stream, request):
            if request["method"] != "pane.process_info":
                return False
            value = processes() if request["params"]["pane_id"] == "w1:p1" else [{"pid": True, "name": "bad"}]
            send_result(stream, request, process_result(request["params"]["pane_id"], value))
            return True
        with FakeServer(generic(2), malformed) as server:
            self.unavailable(self.call(server, expected=1), "schema_error")

    def test_shared_wire_budget_and_post_io_deadline_are_enforced(self):
        with FakeServer(generic(2), process_handler()) as server:
            args = self.args(server)+["--max-response-bytes", "1024"]
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(coven.main(args), 1)
            self.unavailable(json.loads(output.getvalue()), "response_limit")
        original = groups.read_groups
        calls = []
        def delayed(*args, **kwargs):
            result = original(*args, **kwargs)
            calls.append(True)
            if len(calls) == 2:
                time.sleep(0.06)
            return result
        with FakeServer(generic(), process_handler()) as server, mock.patch.object(groups, "read_groups", side_effect=delayed):
            report = inspection.inspect(self.workspace, server.path, timeout=0.05)
            self.unavailable(report, "timeout")

    def test_endpoint_alias_is_preserved_and_unsafe_socket_rejected(self):
        with FakeServer(generic(), process_handler()) as server:
            alias = self.root/"endpoint-alias"
            alias.symlink_to(Path(server.path).parent, target_is_directory=True)
            report = inspection.inspect(self.workspace, str(alias/"s"))
            self.assertEqual(report["assessment"], "observed")
            self.assertEqual(report["endpoint"]["resolved"], str(Path(server.path).resolve()))
            os.chmod(server.path, 0o644)
            self.unavailable(self.call(server, expected=1), "unsafe_endpoint")

    def test_cli_invalid_bounds_and_mutation_flags_have_no_effects(self):
        with FakeServer(generic(), process_handler()) as server:
            for extra in (["--execute"], ["--timeout", "nan"], ["--timeout", "0"],
                          ["--max-response-bytes", "1"], ["--runtime", "other"]):
                before = snapshot(self.workspace)
                with self.subTest(extra=extra), contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                    coven.main(self.args(server)+extra)
                self.assertEqual(snapshot(self.workspace), before)
                self.assertEqual(server.requests, [])

    def test_human_list_is_readable_without_raw_private_process_data(self):
        with FakeServer(generic(), process_handler()) as server:
            args = self.args(server)
            args.remove("--json")
            output = self.invoke(*args)
            self.assertIn("w1:p1", output)
            self.assertIn("terminal-1", output)
            self.assertIn("djinn_executable_family", output)
            self.assertNotIn("SECRET", output)


if __name__ == "__main__":
    unittest.main()
