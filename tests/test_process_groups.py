"""Offline process-bound groups: association evidence, never native identity.

PID/family samples do not exclude PID reuse or establish an authenticated binary,
native generation, interactive agent, or activity. No real runtime is contacted.
"""

import contextlib
import copy
import io
import json
import os
import subprocess
import unittest
from unittest import mock

from runner import coven, group_publisher as publisher, groups, launch, storage, workers
from runner import kitsune_observation as observation
from runner import kitsune_process_observation as process_observation
from test_coven import CovenTestCase, ROOT, snapshot
from test_group_publisher import Clock, FakeServer, inventory


BRACKET = ["session.snapshot", "pane.process_info", "session.snapshot"]
REAL_READ_BUDGET = observation._ReadBudget


def pane_inventory(count=1, *, agents=False, padding=0):
    value = inventory(count)
    for collection in ("panes", "agents"):
        for pane in value[collection]:
            pane.update(agent="djinn" if agents else None, agent_session=None)
    if not agents:
        value["agents"] = []
    if padding:
        value["future"] = "s" * padding
    return value


def processes(pid=10, family="djinn"):
    return [{"pid": pid, "name": "not-authority", "argv": ["/private/" + family,
             "--help", "SECRET ARGUMENT"]}]


def process_result(pane, items):
    return {"type": "pane_process_info", "process_info": {
        "pane_id": pane, "foreground_processes": items}}


def send_result(stream, request, result):
    stream.sendall(json.dumps({"id": request["id"], "result": result}).encode() + b"\n")


def process_handler(items=None, *, padding=0):
    """Handle only process reads; FakeServer retains snapshot/annotation behavior."""
    items = processes() if items is None else items

    def handle(_server, stream, request):
        if request["method"] != "pane.process_info":
            return False
        result = process_result(request["params"]["pane_id"], copy.deepcopy(items))
        if padding:
            result["future"] = "p" * padding
        send_result(stream, request, result)
        return True

    return handle


class ProcessGroupCase(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace)
        self.clock = Clock()
        for target, name in ((launch, "execute"), (workers, "inspect_readiness"),
                             (coven, "send_to_agent_pane")):
            patch = mock.patch.object(target, name, side_effect=AssertionError("native control"))
            guard = patch.start()
            self.addCleanup(patch.stop)
            self.addCleanup(guard.assert_not_called)
        self.call("group", "create", self.workspace, "team", "--name", "Review", "--json")

    def call(self, *args, expected=0):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(list(map(str, args))), expected)
        return json.loads(output.getvalue())

    def add_args(self, server, *, member="one", pane="w1:p1", process=True):
        return ["group", "add", self.workspace, "team", "--member", member,
                "--role", "Reviewer", "--socket", server.path, "--pane", pane,
                "--json", *(["--process-evidence"] if process else [])]

    def add(self, server, **kwargs):
        return self.call(*self.add_args(server, **kwargs))

    def state_bytes(self):
        return (self.workspace / "state/groups.json").read_bytes()

    def publish(self, server, *, duration=1):
        with contextlib.ExitStack() as stack:
            for module in (publisher, observation, process_observation):
                stack.enter_context(mock.patch.object(module, "time", self.clock))
            return publisher.publish_group(self.workspace, "team", server.path, "coven", duration=duration)

    def assert_unreserved(self, server, report, before, code):
        self.assertEqual(report["outcome"], "failed")
        self.assertEqual(report["diagnostics"], [code])
        for field in ("reservation_attempts", "reservation_attempted_targets", "reserved_targets",
                      "reservation_uncertain", "cycles", "completed_cycles", "acknowledged",
                      "rejected", "unsent", "uncertain"):
            self.assertEqual(report[field], 0, field)
        self.assertEqual(self.state_bytes(), before)
        self.assertEqual(groups.read_groups(self.workspace)["state"]["publication_counters"],
                         json.loads(before)["publication_counters"])
        self.assertNotIn("pane.annotate", [r["method"] for r in server.requests])
        self.assertNotIn("SECRET", json.dumps(report))


class ProcessGroupWorkflowTests(ProcessGroupCase):
    def test_empty_agents_opt_in_schema_and_observation_only_add(self):
        value = pane_inventory()
        del value["annotations"]
        with FakeServer(value, process_handler()) as server:
            before = self.state_bytes()
            self.invoke(*self.add_args(server, process=False), error="agent_not_found")
            self.assertEqual(self.state_bytes(), before)
            server.requests.clear()
            added = self.add(server)
            self.assertEqual([r["method"] for r in server.requests], BRACKET)
            self.assertEqual(server.requests[1]["params"], {"pane_id": "w1:p1"})
            self.assertEqual(len({r["id"] for r in server.requests}), 3)
            evidence = added["member"]["binding"]["evidence"]
            self.assertEqual(evidence["provenance"], "kitsune.pane.process_info")
            self.assertEqual(evidence["agent_kind_hint"], "djinn")
            self.assertIsNone(evidence["native_reference"])
            self.assertEqual(evidence["native_binding"], "unestablished")
            self.assertEqual(evidence["process_evidence"], {
                "interpreter": process_observation.ADAPTER_CONTRACT,
                "matches": [{"pid": 10, "executable_family": "djinn",
                             "rule": "exact_posix_argv0_basename"}]})
            state = groups.read_groups(self.workspace)["state"]
            self.assertEqual(state["schema_version"], 2)
            self.assertEqual(state["publication_counters"], [])
            self.assertNotIn("SECRET", self.state_bytes().decode())
            self.assertNotIn("/private/", self.state_bytes().decode())
            listed = self.call("group", "list", self.workspace, "--json")
            self.assertEqual(listed["authorization"], "none")
            self.assertFalse(listed["dispatchable"])
            self.assertEqual(listed["native_lifecycle"], "unestablished")

    def test_reversed_process_set_is_canonical_unchanged_without_rebind(self):
        items = processes(20, "djinn-ui") + processes(10)
        with FakeServer(pane_inventory(), process_handler(items)) as server:
            first = self.add(server)
            before = self.state_bytes()
            server.handler = process_handler(list(reversed(items)))
            again = self.add(server)
            self.assertEqual(again["outcome"], "unchanged")
            self.assertEqual(again["member"], first["member"])
            self.assertEqual(self.state_bytes(), before)
            self.assertEqual([m["pid"] for m in first["member"]["binding"]["evidence"]
                              ["process_evidence"]["matches"]], [10, 20])

    def test_pid_family_match_set_and_evidence_source_require_explicit_rebind(self):
        cases = [("pid", processes(11)), ("family", processes(10, "djinn-ui")),
                 ("set", processes() + processes(20)), ("source", processes())]
        for name, changed in cases:
            with self.subTest(name=name), FakeServer(pane_inventory(agents=True), process_handler()) as server:
                first = self.add(server, process=name != "source")
                before = self.state_bytes()
                server.handler = process_handler(changed)
                self.invoke(*self.add_args(server), error="explicit rebind required")
                self.assertEqual(self.state_bytes(), before)
                rebound = self.call(*self.add_args(server), "--rebind")
                self.assertEqual(rebound["member"]["participant_id"], first["member"]["participant_id"])
                self.assertNotEqual(rebound["revision"], first["revision"])
                self.assertEqual(groups.read_groups(self.workspace)["state"]["schema_version"], 2)
                groups.remove_member(self.workspace, "team", "one")

    def test_failed_fresh_observation_preserves_schema_one_and_existing_binding(self):
        with FakeServer(pane_inventory(agents=True), process_handler()) as server:
            self.add(server, process=False)
            self.assertEqual(groups.read_groups(self.workspace)["state"]["schema_version"], 1)
            for items, code in (([], "process_not_recognized"), ([{"pid": True}], "schema_error")):
                with self.subTest(code=code):
                    before = self.state_bytes()
                    server.handler = process_handler(items)
                    self.invoke(*self.add_args(server), "--rebind", error=code)
                    self.assertEqual(self.state_bytes(), before)
            server.handler = process_handler()
            self.call(*self.add_args(server), "--rebind")
            before = self.state_bytes()
            server.handler = process_handler([])
            self.invoke(*self.add_args(server), "--rebind", error="process_not_recognized")
            self.assertEqual(self.state_bytes(), before)

    def test_cas_group_and_input_validation_precede_any_socket_access(self):
        before = snapshot(self.workspace)
        with FakeServer(pane_inventory(), process_handler()) as server, \
                mock.patch.object(publisher, "binding_from_process", side_effect=AssertionError("observation")), \
                mock.patch.object(publisher, "read_publication_snapshot", side_effect=AssertionError("snapshot")):
            args = self.add_args(server)
            self.invoke(*args, "--expect-revision", groups.UNINITIALIZED, error="revision conflict")
            for option, value, error in (("--member", "Bad", "invalid human selector"),
                                         ("--role", "", "malformed group authority"),
                                         ("--pane", "bad pane", "exact current pane")):
                invalid = list(args)
                invalid[invalid.index(option) + 1] = value
                self.invoke(*invalid, error=error)
            invalid = list(args)
            invalid[3] = "missing"
            self.invoke(*invalid, error="group does not exist")
            self.assertEqual(server.requests, [])
        self.assertEqual(snapshot(self.workspace), before)

    def test_uncertain_promotion_remains_schema_two_and_confirms_without_rewriting(self):
        real_replace = storage.atomic_replace_bytes

        def committed_but_uncertain(path, data):
            real_replace(path, data)
            raise storage.CommitUncertain(path)

        with FakeServer(pane_inventory(agents=True), process_handler()) as server:
            first = self.add(server, process=False)
            with mock.patch.object(storage, "atomic_replace_bytes", side_effect=committed_but_uncertain):
                result = self.call(*self.add_args(server), "--rebind", expected=2)
            self.assertEqual(result["delivery"]["outcome"], "durability_uncertain")
            state = groups.read_groups(self.workspace)["state"]
            self.assertEqual(state["schema_version"], 2)
            self.assertEqual(state["publication_counters"], [])
            self.assertEqual(result["member"]["participant_id"], first["member"]["participant_id"])
            before = self.state_bytes()
            self.invoke(*self.add_args(server), "--expect-revision", first["revision"], error="revision conflict")
            with mock.patch.object(storage, "atomic_replace_bytes", side_effect=AssertionError("rewrite")):
                confirmed = self.call(*self.add_args(server), "--expect-revision", result["revision"])
            self.assertEqual(confirmed["outcome"], "unchanged")
            self.assertEqual(confirmed["delivery"]["outcome"], "durably_committed")
            self.assertEqual(self.state_bytes(), before)
            self.assertNotIn("pane.annotate", [r["method"] for r in server.requests])

    def test_task_and_execution_files_are_not_dependencies(self):
        for relative in ("coven.json", "state/tasks.json", "state/agents.json", "state/execution.json"):
            (self.workspace / relative).write_bytes(b"broken\xff")
        before = snapshot(self.workspace)
        with FakeServer(pane_inventory(), process_handler()) as server, \
                mock.patch.object(coven, "load_team", side_effect=AssertionError("task dependency")):
            self.add(server)
            self.assertEqual(self.publish(server)["acknowledged"], 1)
        after = snapshot(self.workspace)
        self.assertEqual({key: after[key] for key in before if key != "state/groups.json"},
                         {key: value for key, value in before.items() if key != "state/groups.json"})

    def test_actual_wrapper_uses_explicit_fake_socket_for_add_publish_and_list(self):
        env = {key: value for key, value in os.environ.items() if key not in {"COVEN_HOME", "COVEN_COMMAND"}}
        env.update(PYTHONDONTWRITEBYTECODE="1", KITSUNE_SOCKET_PATH="/not-selected",
                   KITSUNE_CLIENT_SOCKET_PATH="/not-selected")

        def run(*args):
            result = subprocess.run([str(ROOT / "bin/coven"), *map(str, args)], cwd=ROOT,
                                    env=env, text=True, capture_output=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout)

        with FakeServer(pane_inventory(), process_handler()) as server:
            added = run(*self.add_args(server))
            report = run("group", "publish", self.workspace, "team", "--socket", server.path,
                         "--source", "coven", "--duration", "1", "--json")
            self.assertEqual(report["acknowledged"], 1)
            self.assertEqual([r["method"] for r in server.requests],
                             BRACKET + ["session.snapshot"] + BRACKET + ["pane.annotate"])
        listed = run("group", "list", self.workspace, "--json")
        self.assertEqual(listed["groups"][0]["members"][0], added["member"])


class ProcessGroupPublisherTests(ProcessGroupCase):
    def test_mixed_success_reserves_whole_plan_only_after_all_process_brackets(self):
        with FakeServer(pane_inventory(3, agents=True), process_handler()) as server:
            self.add(server, process=False)
            self.add(server, member="two", pane="w1:p2")
            self.add(server, member="three", pane="w1:p3")
            server.requests.clear()
            real_reserve = groups.reserve_cycle

            def reserve(*args, **kwargs):
                self.assertEqual([r["method"] for r in server.requests],
                                 ["session.snapshot"] + BRACKET * 2)
                self.assertEqual(args[6], ["terminal-1", "terminal-2", "terminal-3"])
                return real_reserve(*args, **kwargs)

            with mock.patch.object(groups, "reserve_cycle", side_effect=reserve) as reserved:
                report = self.publish(server)
            reserved.assert_called_once()
            self.assertEqual(report["acknowledged"], 3)
            self.assertEqual(report["completed_cycles"], 1)
            self.assertEqual([r["method"] for r in server.requests],
                             ["session.snapshot"] + BRACKET * 2 + ["pane.annotate"] * 3)

    def test_success_empty_agents_moves_terminal_and_ignores_legacy_metadata(self):
        with FakeServer(pane_inventory(), process_handler()) as server:
            self.add(server)
            server.requests.clear()
            pane = server.value["panes"][0]
            pane.update(pane_id="w9:p7", workspace_id="w9", tab_id="t7", agent="other",
                        agent_session={"source": "persisted", "agent": "other", "kind": "id", "value": "old"})
            server.value["agents"] = [copy.deepcopy(pane)]
            report = self.publish(server)
            self.assertEqual(report["outcome"], "completed")
            self.assertEqual(report["acknowledged"], 1)
            self.assertEqual(report["authorization"], "annotation_publication_only")
            self.assertEqual(report["native_execution_authorization"], "none")
            self.assertFalse(report["dispatchable"])
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"] + BRACKET + ["pane.annotate"])
            self.assertEqual(server.requests[2]["params"], {"pane_id": "w9:p7"})
            self.assertEqual(server.requests[-1]["params"]["pane_id"], "w9:p7")
            self.assertEqual(server.requests[-1]["params"]["expected_terminal_id"], "terminal-1")
            counter, = groups.read_groups(self.workspace)["state"]["publication_counters"]
            self.assertEqual(counter["watermarks"], {"terminal-1": 1})

    def test_process_failures_never_fall_back_to_still_recognized_djinn_metadata(self):
        cases = [("absent", [], "process_not_recognized"),
                 ("unknown", processes(family="node"), "process_not_recognized"),
                 ("malformed", processes() + [{"pid": True}], "schema_error"),
                 ("changed_pid", processes(11), "rebind_required"),
                 ("changed_family", processes(family="djinn-ui"), "rebind_required"),
                 ("changed_set", processes() + processes(20), "rebind_required")]
        for name, items, code in cases:
            with self.subTest(name=name), FakeServer(pane_inventory(agents=True), process_handler()) as server:
                self.add(server)
                server.requests.clear()
                server.handler = process_handler(items)
                before = self.state_bytes()
                with mock.patch.object(groups, "reserve_cycle", wraps=groups.reserve_cycle) as reserve:
                    report = self.publish(server)
                reserve.assert_not_called()
                self.assert_unreserved(server, report, before, code)
                self.assertEqual(server.value["agents"][0]["agent"], "djinn")
                groups.remove_member(self.workspace, "team", "one")

    def test_mixed_members_all_verified_before_reserving_last_process_failure(self):
        with FakeServer(pane_inventory(3, agents=True), process_handler()) as server:
            self.add(server, process=False)
            self.add(server, member="two", pane="w1:p2")
            self.add(server, member="three", pane="w1:p3")
            server.requests.clear()
            normal = process_handler()

            def fail_last(server, stream, request):
                if request["method"] == "pane.process_info" and request["params"]["pane_id"] == "w1:p3":
                    return process_handler([])(server, stream, request)
                return normal(server, stream, request)

            server.handler = fail_last
            before = self.state_bytes()
            with mock.patch.object(groups, "reserve_cycle", wraps=groups.reserve_cycle) as reserve:
                report = self.publish(server)
            reserve.assert_not_called()
            self.assert_unreserved(server, report, before, "process_not_recognized")
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"] + BRACKET * 2)

    def test_missing_or_stale_process_response_never_uses_agent_metadata(self):
        for mode, code in (("eof", "incomplete_response"), ("error", "server_error"),
                           ("correlation", "schema_error"), ("type", "schema_error"),
                           ("pane", "schema_error")):
            with self.subTest(mode=mode), FakeServer(pane_inventory(agents=True), process_handler()) as server:
                self.add(server)
                server.requests.clear()

                def invalid(_server, stream, request):
                    if request["method"] != "pane.process_info":
                        return False
                    if mode == "eof":
                        return True
                    if mode == "error":
                        stream.sendall(json.dumps({"id": request["id"], "error": {
                            "code": "gone", "message": "SECRET SERVER MESSAGE"}}).encode() + b"\n")
                        return True
                    response_request = {"id": "stale-request"} if mode == "correlation" else request
                    result = process_result("old-pane" if mode == "pane" else "w1:p1", processes())
                    if mode == "type":
                        result["type"] = "session_snapshot"
                    send_result(stream, response_request, result)
                    return True

                server.handler = invalid
                before = self.state_bytes()
                self.assert_unreserved(server, self.publish(server), before, code)
                self.assertEqual([r["method"] for r in server.requests],
                                 ["session.snapshot", *BRACKET[:2]])
                groups.remove_member(self.workspace, "team", "one")

    def test_terminal_replaced_after_cycle_snapshot_rejected_before_process_query(self):
        with FakeServer(pane_inventory(), process_handler()) as server:
            self.add(server)
            server.requests.clear()
            reads = 0

            def replace(server, _stream, request):
                nonlocal reads
                if request["method"] == "session.snapshot":
                    reads += 1
                    if reads == 2:
                        server.value["panes"][0]["terminal_id"] = "replacement"
                return False

            server.handler = replace
            before = self.state_bytes()
            self.assert_unreserved(server, self.publish(server), before, "pane_mapping_changed")
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"] * 2)

    def test_final_bracket_schema_or_mapping_failure_discards_process_match(self):
        for mode in ("schema", "mapping", "missing"):
            with self.subTest(mode=mode), FakeServer(pane_inventory(), process_handler()) as server:
                self.add(server)
                server.requests.clear()
                normal = process_handler()
                reads = 0

                def change(server, stream, request):
                    nonlocal reads
                    if request["method"] == "session.snapshot":
                        reads += 1
                        if reads == 3:
                            if mode == "schema":
                                server.value["protocol"] = True
                            elif mode == "mapping":
                                server.value["panes"][0]["tab_id"] = "changed"
                            else:
                                server.value["panes"] = []
                    return normal(server, stream, request)

                server.handler = change
                before = self.state_bytes()
                code = {"schema": "schema_error", "mapping": "pane_mapping_changed", "missing": "pane_not_found"}[mode]
                self.assert_unreserved(server, self.publish(server), before, code)
                self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"] + BRACKET)
                groups.remove_member(self.workspace, "team", "one")

    def test_endpoint_frozen_before_process_and_between_bracket_reads(self):
        with FakeServer(pane_inventory(), process_handler()) as server:
            self.add(server)
            before = self.state_bytes()
            original = publisher._endpoint(server.path)
            changed = {**original, "inode": original["inode"] + 1}
            server.requests.clear()
            real_binding = publisher.binding_from_process

            def replaced(*args, **kwargs):
                self.assertEqual(kwargs["frozen_endpoint"], original)
                self.assertEqual(kwargs["expected_terminal_id"], "terminal-1")
                with mock.patch.object(publisher, "_endpoint", return_value=changed):
                    return real_binding(*args, **kwargs)

            with mock.patch.object(publisher, "binding_from_process", side_effect=replaced):
                self.assert_unreserved(server, self.publish(server), before, "endpoint_replaced")
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"])
            server.requests.clear()
            normal = process_handler()
            real_stat = observation._endpoint_stat
            replaced_endpoint = False

            def mark(server, stream, request):
                nonlocal replaced_endpoint
                if request["method"] == "pane.process_info":
                    replaced_endpoint = True
                return normal(server, stream, request)

            def stat(path):
                info = real_stat(path)
                if replaced_endpoint:
                    return mock.Mock(st_dev=info.st_dev, st_ino=info.st_ino + 1)
                return info

            server.handler = mark
            with mock.patch.object(observation, "_endpoint_stat", side_effect=stat):
                self.assert_unreserved(server, self.publish(server), before, "endpoint_replaced")
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot", *BRACKET[:2]])

    def test_global_wire_budget_counts_initial_snapshot_and_every_member_frame(self):
        for count in (1, 2):
            with self.subTest(count=count), FakeServer(pane_inventory(count, padding=1100),
                                                       process_handler(padding=1100)) as server:
                for index in range(count):
                    self.add(server, member=f"member-{index}", pane=f"w1:p{index + 1}")
                request = {"id": "x" * 32}
                snapshot_frame = json.dumps({"id": request["id"], "result": {
                    "type": "session_snapshot", "snapshot": server.value}}).encode() + b"\n"
                process_frame = json.dumps({"id": request["id"], "result": {
                    **process_result("w1:p1", processes()), "future": "p" * 1100}}).encode() + b"\n"
                self.assertGreater(len(snapshot_frame), 1024)
                self.assertGreater(len(process_frame), 1024)
                total = len(snapshot_frame) * (1 + 2 * count) + len(process_frame) * count
                before = self.state_bytes()
                budgets = []

                def capped(remaining):
                    budget = REAL_READ_BUDGET(min(remaining, total - 1))
                    budgets.append(budget)
                    return budget

                server.requests.clear()
                with mock.patch.object(observation, "_ReadBudget", side_effect=capped):
                    report = self.publish(server)
                self.assert_unreserved(server, report, before, "response_limit")
                self.assertEqual(len(budgets), 1)
                self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"] + BRACKET * count)
                server.requests.clear()
                with mock.patch.object(observation, "_ReadBudget",
                                       side_effect=lambda remaining: REAL_READ_BUDGET(min(remaining, total))):
                    report = self.publish(server)
                self.assertEqual(report["acknowledged"], count)
                for index in range(count):
                    groups.remove_member(self.workspace, "team", f"member-{index}")

    def test_one_cycle_deadline_includes_initial_snapshot_and_all_process_members(self):
        for count, step, duration in ((1, 0.26, 1), (2, 0.44, 5)):
            with self.subTest(count=count), FakeServer(pane_inventory(count), process_handler()) as server:
                for index in range(count):
                    self.add(server, member=f"member-{index}", pane=f"w1:p{index + 1}")
                normal = process_handler()
                deadlines = []
                real_read = observation._read_request

                def slow(server, stream, request):
                    self.clock.now += step
                    return normal(server, stream, request)

                def read(*args, **kwargs):
                    deadlines.append(kwargs["deadline"])
                    return real_read(*args, **kwargs)

                server.handler = slow
                server.requests.clear()
                before = self.state_bytes()
                started = self.clock.now
                with mock.patch.object(observation, "_read_request", side_effect=read):
                    report = self.publish(server, duration=duration)
                self.assert_unreserved(server, report, before, "timeout")
                self.assertEqual(deadlines, [started + min(duration, 3)] * (3 * count))
                self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"] + BRACKET * count)
                for index in range(count):
                    groups.remove_member(self.workspace, "team", f"member-{index}")


if __name__ == "__main__":
    unittest.main()
