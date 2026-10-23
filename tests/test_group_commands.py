"""Explicit group workflow, actual wrapper, and annotation-only CLI boundaries."""

import contextlib
import copy
import io
import json
import os
import subprocess
from unittest import mock

from runner import coven, group_publisher as publisher, groups, launch, storage, task_attempts, workers
from test_coven import CovenTestCase, ROOT, snapshot
from test_group_publisher import Clock, FakeServer, inventory


class GroupCommandTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace)
        for target, name in ((launch, "execute"), (workers, "inspect_readiness"), (coven, "send_to_agent_pane")):
            guard = mock.patch.object(target, name, side_effect=AssertionError("Groups do not control native workers or send terminal input"))
            observed = guard.start()
            self.addCleanup(guard.stop)
            self.addCleanup(observed.assert_not_called)

    def call(self, *args, expected=0):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(list(map(str, args))), expected)
        return json.loads(output.getvalue())

    def create(self, key="review", name="Auth review"):
        return self.call("group", "create", self.workspace, key, "--name", name, "--json")

    def add(self, server, member="reviewer", pane="w1:p1", role="Reviewer", *extra):
        return self.call("group", "add", self.workspace, "review", "--member", member, "--role", role,
            "--socket", server.path, "--pane", pane, *extra, "--json")

    def publish(self, server, *, expected=0, duration=1, source="coven", clock=None):
        with mock.patch.object(publisher, "time", clock or Clock()):
            return self.call("group", "publish", self.workspace, "review", "--socket", server.path,
                "--source", source, "--duration", duration, "--json", expected=expected)

    def test_complete_workflow_groups_unresolved_agents_and_publishes_roles(self):
        before = snapshot(self.workspace)
        empty = self.call("group", "list", self.workspace, "--json")
        self.assertFalse(empty["initialized"])
        self.assertEqual(snapshot(self.workspace), before)
        first = self.create()
        again = self.create()
        self.assertEqual(first["group"]["group_id"], again["group"]["group_id"])
        self.assertEqual(again["outcome"], "unchanged")
        value = inventory(2)
        native = dict(source="kitsune:djinn", agent="djinn", kind="id", value="native-1")
        for field in ("panes", "agents"):
            value[field][0]["agent_session"] = copy.deepcopy(native)
        with FakeServer(value) as server:
            one = self.add(server)
            two = self.add(server, "investigator", "w1:p2", "Investigator")
            self.assertNotEqual(one["member"]["participant_id"], two["member"]["participant_id"])
            self.assertEqual(one["member"]["binding"]["evidence"]["native_binding"], "observed_reference")
            self.assertEqual(two["member"]["binding"]["evidence"]["native_binding"], "unestablished")
            token = two["revision"]
            clock = Clock()
            result = self.publish(server, duration=3, clock=clock)
            self.assertEqual(result["cycles"], 2)
            self.assertEqual(result["acknowledged"], 4)
            self.assertEqual(result["uncertain"], 0)
            self.assertEqual(result["authorization"], "annotation_publication_only")
            self.assertEqual(result["native_execution_authorization"], "none")
            self.assertFalse(result["dispatchable"])
            self.assertEqual(groups.read_groups(self.workspace)["revision"], token)
            self.publish(server, clock=clock)
            writes = [r["params"] for r in server.requests if r["method"] == "pane.annotate"]
            for terminal in ("terminal-1", "terminal-2"):
                selected = [p for p in writes if p["expected_terminal_id"] == terminal]
                self.assertEqual([p["revision"] for p in selected], [1, 2, 3])
                self.assertTrue(all(p["annotation_epoch"] == "epoch-1" and p["ttl_ms"] == 6000 for p in selected))
            self.assertEqual(writes[0]["text"], "Auth review · Reviewer")
            self.assertEqual(writes[1]["text"], "Auth review · Investigator")
            self.assertEqual({r["method"] for r in server.requests}, {"session.snapshot", "pane.annotate"})
        listed = self.call("group", "list", self.workspace, "--json")
        self.assertEqual(len(listed["groups"][0]["members"]), 2)
        self.assertNotIn("publication_counters", listed)
        self.assertEqual(listed["native_lifecycle"], "unestablished")
        after = snapshot(self.workspace)
        self.assertEqual({key: after[key] for key in before}, before)
        self.assertEqual(set(after) - set(before), {"state/groups.json", "state/.group-publisher.lock"})

    def test_add_is_observation_only_and_can_precede_source_enablement(self):
        self.create()
        with FakeServer(inventory(annotations=False)) as server:
            added = self.add(server)
            again = self.add(server)
            self.assertEqual(again["outcome"], "unchanged")
            self.assertEqual(again["member"], added["member"])
            server.value["annotations"] = {"epoch": "enabled-later", "sources": ["coven"]}
            self.publish(server)
            self.assertEqual(server.requests[-1]["params"]["annotation_epoch"], "enabled-later")
        self.assertEqual([r["method"] for r in server.requests[:2]], ["session.snapshot"] * 2)

    def test_native_evidence_change_requires_explicit_rebind(self):
        self.create()
        with FakeServer() as server:
            original = self.add(server)
            for field in ("panes", "agents"):
                server.value[field][0]["agent_session"] = dict(source="kitsune:djinn", agent="djinn", kind="id", value="new-native")
            self.assertEqual(self.publish(server, expected=1)["diagnostics"], ["rebind_required"])
            args = ["group", "add", self.workspace, "review", "--member", "reviewer", "--role", "Reviewer",
                    "--socket", server.path, "--pane", "w1:p1", "--json"]
            self.invoke(*args, error="explicit rebind required")
            rebound = self.call(*args, "--rebind")
            self.assertEqual(rebound["member"]["participant_id"], original["member"]["participant_id"])
            self.publish(server)

    def test_source_must_already_be_enabled_and_does_not_reserve_on_rejection(self):
        self.create()
        with FakeServer() as server:
            self.add(server)
            before = (self.workspace / "state/groups.json").read_bytes()
            result = self.publish(server, source="not-enabled", expected=1)
            self.assertEqual(result["diagnostics"], ["source_disabled"])
            self.assertEqual(result["cycles"], 0)
            self.assertEqual((self.workspace / "state/groups.json").read_bytes(), before)
            self.assertNotIn("pane.annotate", [r["method"] for r in server.requests])

    def test_rejected_or_lost_ack_stops_without_retry(self):
        for lost in (False, True):
            workspace = self.root / ("lost" if lost else "rejected")
            self.invoke("init", workspace)
            self.call("group", "create", workspace, "review", "--json")
            def fail(_server, stream, request):
                if request["method"] != "pane.annotate":
                    return False
                if not lost:
                    stream.sendall(json.dumps({"id": request["id"], "error": {
                        "code": "annotation_stale_revision", "message": "SECRET ERROR"}}).encode() + b"\n")
                return True
            with FakeServer(handler=fail) as server:
                self.call("group", "add", workspace, "review", "--member", "reviewer", "--role", "Reviewer",
                    "--socket", server.path, "--pane", "w1:p1", "--json")
                with mock.patch.object(publisher, "time", Clock()):
                    result = self.call("group", "publish", workspace, "review", "--socket", server.path,
                        "--source", "coven", "--duration", 1, "--json", expected=2 if lost else 1)
                self.assertEqual(len([r for r in server.requests if r["method"] == "pane.annotate"]), 1)
                self.assertEqual(result["uncertain" if lost else "rejected"], 1)
                self.assertNotIn("SECRET", json.dumps(result))
                counter, = groups.read_groups(workspace)["state"]["publication_counters"]
                self.assertEqual(counter["watermarks"], {"terminal-1": 1})

    def test_remove_membership_does_not_clear_labels_or_watermarks(self):
        self.create()
        with FakeServer() as server:
            self.add(server)
            self.publish(server)
            count = len(server.requests)
            before = groups.read_groups(self.workspace)["state"]["publication_counters"]
            self.call("group", "remove", self.workspace, "review", "--member", "reviewer", "--json")
            self.assertEqual(len(server.requests), count)
            self.assertEqual(groups.read_groups(self.workspace)["state"]["publication_counters"], before)
            self.assertEqual(self.publish(server, expected=1)["diagnostics"], ["group_size"])
            self.assertEqual(len(server.requests), count)

    def test_deterministic_bad_input_has_no_remote_or_workspace_effect(self):
        self.create()
        before = snapshot(self.workspace)
        with mock.patch.object(publisher, "read_publication_snapshot", side_effect=AssertionError("Unexpected server observation")):
            for extra in (("--member", "Bad"), ("--role", ""), ("--pane", "")):
                options = {"--member": "reviewer", "--role": "Reviewer", "--pane": "w1:p1"}
                options[extra[0]] = extra[1]
                args = ["group", "add", self.workspace, "review", "--socket", "/unused"]
                for key, value in options.items():
                    args += [key, value]
                self.invoke(*args, error="invalid human selector" if extra[0] == "--member" else "malformed group authority")
            self.invoke("group", "add", self.workspace, "review", "--member", "reviewer", "--role", "Reviewer",
                "--pane", "w1:p1", "--socket", "/unused", "--expect-revision", groups.UNINITIALIZED, error="revision conflict")
        self.invoke("group", "create", self.workspace, "empty-name", "--name", "", error="malformed group authority")
        self.assertEqual(snapshot(self.workspace), before)

    def test_explicit_workspace_socket_and_source_are_required(self):
        for args in (("group", "create"), ("group", "list"),
                     ("group", "add", str(self.workspace), "review", "--member", "reviewer", "--role", "Reviewer", "--pane", "w1:p1"),
                     ("group", "publish", str(self.workspace), "review", "--socket", "/unused")):
            with self.subTest(args=args), contextlib.redirect_stderr(io.StringIO()), \
                    mock.patch.object(publisher, "read_publication_snapshot", side_effect=AssertionError("Unexpected observation")), \
                    self.assertRaises(SystemExit) as raised:
                coven.main(list(args))
            self.assertEqual(raised.exception.code, 2)

    def test_membership_and_publication_do_not_depend_on_task_or_execution_health(self):
        token = json.loads(self.invoke("task", "list", self.workspace, "--json"))["revision"]
        self.invoke("state", "import-tasks", self.workspace, "--expect-revision", token)
        self.invoke("execution", "enable", self.workspace, "--expect-execution-revision", task_attempts.DISABLED, "--request-id", "enable")
        self.invoke("execution", "upgrade", self.workspace, "--expect-execution-revision",
            task_attempts.Store(self.workspace).read().revision, "--request-id", "upgrade")
        for relative in ("coven.json", "state/tasks.json", "state/agents.json"):
            (self.workspace / relative).write_bytes(b"broken\xff")
        before = snapshot(self.workspace)
        self.create()
        with FakeServer() as server, mock.patch.object(coven, "load_team", side_effect=AssertionError("Unrelated task/roster dependency")):
            self.add(server)
            self.publish(server)
            self.call("group", "list", self.workspace, "--json")
        after = snapshot(self.workspace)
        self.assertEqual({key: after[key] for key in before}, before)

    def test_interrupt_releases_lease_without_sending_clear(self):
        self.create()
        clock = Clock()
        clock.sleep = mock.Mock(side_effect=KeyboardInterrupt)
        with FakeServer() as server:
            self.add(server)
            result = self.publish(server, duration=60, clock=clock, expected=130)
            self.assertEqual(result["outcome"], "interrupted")
            self.assertEqual(result["acknowledged"], 1)
            self.assertTrue(all(r["params"].get("text") is not None for r in server.requests if r["method"] == "pane.annotate"))
            self.publish(server)

    def test_mutation_uncertainty_preserves_known_group_and_replay(self):
        original = storage.atomic_replace_bytes
        def uncertain(path, data):
            original(path, data)
            raise storage.CommitUncertain(path)
        with mock.patch.object(storage, "atomic_replace_bytes", side_effect=uncertain):
            result = self.call("group", "create", self.workspace, "review", "--json", expected=2)
        replay = self.call("group", "create", self.workspace, "review", "--json")
        self.assertEqual(replay["group"], result["group"])
        self.assertEqual(replay["outcome"], "unchanged")

    def test_real_wrapper_create_observe_add_publish_and_list(self):
        env = {key: value for key, value in os.environ.items() if key not in {"COVEN_HOME", "COVEN_COMMAND"}}
        env.update(KITSUNE_SOCKET_PATH="/not-selected", KITSUNE_CLIENT_SOCKET_PATH="/not-selected")
        def run(*args):
            response = subprocess.run([str(ROOT / "bin/coven"), *map(str, args)], cwd=ROOT,
                env=env, text=True, capture_output=True, timeout=15)
            self.assertEqual(response.returncode, 0, response.stderr)
            return json.loads(response.stdout)
        run("group", "create", self.workspace, "review", "--name", "Auth review", "--json")
        with FakeServer() as server:
            added = run("group", "add", self.workspace, "review", "--member", "reviewer", "--role", "Reviewer",
                "--socket", server.path, "--pane", "w1:p1", "--json")
            result = run("group", "publish", self.workspace, "review", "--socket", server.path,
                "--source", "coven", "--duration", 1, "--json")
            self.assertEqual(result["acknowledged"], 1)
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot", "session.snapshot", "pane.annotate"])
        listed = run("group", "list", self.workspace, "--json")
        self.assertEqual(listed["groups"][0]["members"][0]["participant_id"], added["member"]["participant_id"])
