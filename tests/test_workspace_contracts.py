"""Schema validation runs before CLI/helper effects; labels never become identities."""

import contextlib
import io
import json
import tomllib
from unittest import mock

from runner import coven
from test_coven import CovenTestCase, snapshot


class WorkspaceContractTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace)
        guard = mock.patch.object(coven.subprocess, "run", side_effect=AssertionError("Unexpected runtime effect"))
        self.runtime = guard.start()
        self.addCleanup(guard.stop)
        sender = mock.patch.object(coven, "send_to_agent_pane", return_value=False)
        self.sender = sender.start()
        self.addCleanup(sender.stop)

    def mutation_commands(self):
        w = self.workspace
        return (
            ("goal", w, "New goal"), ("task", "add", w, "Another task"),
            ("task", "assign", w, "plan-checkpoint", "jamal"), ("task", "list", w),
            ("message", w, "user", "jamal", "Please continue"), ("send", w, "jamal", "Continue"),
            ("lead", w, "jamal"), ("lead", w, "clear"), ("approve", w),
            ("suggest", w, "plan-approval", "More QA"), ("review", w), ("console", w),
            ("refresh", w), ("status", w), ("bootstrap", w), ("standup", w),
            *(("agent", operation, w, "jamal") for operation in ("start", "stop", "restart", "status")),
            *((operation, w) for operation in ("up", "down", "restart", "plan")),
        )

    def assert_commands_fail_without_effects(self):
        before = snapshot(self.workspace)
        self.sender.reset_mock()
        with mock.patch("builtins.input", side_effect=AssertionError("Unexpected prompt")):
            for command in self.mutation_commands():
                with self.subTest(command=command):
                    self.invoke(*command, error="[ERROR]")
                    self.assertEqual(snapshot(self.workspace), before)
        self.sender.assert_not_called()
        self.runtime.assert_not_called()

    def test_late_invalid_state_records_block_all_command_effects(self):
        cases = {
            "state/tasks.json": self.read_json("state/tasks.json") + [{"id": "bad", "title": "Late", "status": []}],
            "state/agents.json": self.read_json("state/agents.json") + [{"id": "late", "status": "idle", "role": None}],
            "state/lead.json": {"agent": "jamal", "set_at": []},
        }
        for relative, content in cases.items():
            path = self.workspace / relative
            original = path.read_bytes() if path.exists() else None
            with self.subTest(relative=relative):
                path.write_text(json.dumps(content))
                self.assert_commands_fail_without_effects()
            path.write_bytes(original) if original is not None else path.unlink()

    def test_corrupt_json_wrong_root_and_encoding_block_commands_cleanly(self):
        for relative in ("state/tasks.json", "state/agents.json", "state/lead.json", "coven.json"):
            path = self.workspace / relative
            original = path.read_bytes() if path.exists() else None
            for content in (b"{truncated", b"\xff", b"false"):
                with self.subTest(relative=relative, content=content):
                    path.write_bytes(content)
                    self.assert_commands_fail_without_effects()
            path.write_bytes(original) if original is not None else path.unlink()

    def test_shared_helpers_revalidate_persisted_state_with_cached_team(self):
        team = coven.load_team(self.workspace)
        w = self.workspace
        actions = (
            lambda: coven.approve_checkpoint(w, team, "plan-approval", "Approved"),
            lambda: coven.suggest_checkpoint_changes(w, team, "plan-approval", "More QA"),
            lambda: coven.notify_agents(w, team, "all", "Hello", event_type="test.sent"),
            lambda: coven.start_agent_window(w, team, "jamal"),
            lambda: coven.stop_agent_window(w, team, "jamal"),
            lambda: coven.create_coven_tmux_session(w, team),
            lambda: coven.create_coven_herdr_workspace(w, team),
            lambda: coven.run_coven_tmux(w, team, "restart"),
            lambda: coven.run_coven_herdr(w, team, "restart"),
            lambda: coven.run_coven_runtime(w, team, "restart"),
            lambda: coven.append_event(w, "test.event"),
            lambda: coven.append_message(w, "user", "all", "Hello"),
            lambda: coven.write_lead(w, None),
            lambda: coven.refresh_dashboard(w),
        )
        for relative in ("coven.json", "state/tasks.json", "state/agents.json", "state/lead.json"):
            path = self.workspace / relative
            original = path.read_bytes() if path.exists() else None
            path.write_text("false")
            before = snapshot(w)
            for action in actions:
                with self.subTest(relative=relative, action=action), contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                    action()
                self.assertEqual(snapshot(w), before)
            path.write_bytes(original) if original is not None else path.unlink()
        self.sender.assert_not_called()
        self.runtime.assert_not_called()

    def test_invalid_runtime_configuration_cannot_hide_behind_overrides(self):
        metadata = self.workspace / "coven.json"
        config = self.workspace / "coven.toml"
        team = self.read_json("coven.json")
        for field, value in (("version", True), ("version", 2), ("name", "!!!"),
                             ("opencode_command", []), ("multiplexer", 13), ("agents", ["Jamal", "jamal"])):
            with self.subTest(field=field):
                metadata.write_text(json.dumps({**team, field: value}))
                self.assert_commands_fail_without_effects()
        metadata.write_text(json.dumps(team))
        for text in ('runtime = "bad"', 'runtime = []', '[runtime]\nopencode_command = 42',
                     '[runtime]\nmultiplexer = []', '[runtime]\nmultiplexer = "unknown"',
                     '[runtime]\nopencode_command = " "'):
            with self.subTest(config=text):
                config.write_text(text)
                before = snapshot(self.workspace)
                self.invoke("refresh", self.workspace, error="[ERROR]")
                self.assertEqual(snapshot(self.workspace), before)

    def test_unknown_fields_survive_task_and_lead_mutations(self):
        tasks = self.read_json("state/tasks.json")
        tasks[0]["extension"] = {"nested": [1, {"custom": True}]}
        tasks[0]["status"] = "custom waiting state"
        tasks[0]["created_at"] = "old"
        tasks[0]["owner"] = "Retired"
        (self.workspace / "state/tasks.json").write_text(json.dumps(tasks))
        self.invoke("task", "assign", self.workspace, tasks[0]["id"], "JAMAL")
        self.invoke("task", "add", self.workspace, "Another task")
        saved = self.read_json("state/tasks.json")[0]
        self.assertEqual(saved["extension"], tasks[0]["extension"])
        self.assertEqual(saved["created_at"], "old")
        self.assertEqual(saved["owner"], "jamal")
        (self.workspace / "state/lead.json").write_text('{"agent":"srikanth","extension":{"custom":[1]}}')
        self.invoke("lead", self.workspace, "jamal")
        self.assertEqual(self.read_json("state/lead.json")["extension"], {"custom": [1]})

    def test_legacy_absent_optional_state_and_metadata_remain_unmigrated(self):
        team = self.read_json("coven.json")
        team.pop("version")
        team.pop("display_names")
        team["agents"] = ["Jamal", "_"]
        path = self.workspace / "coven.json"
        path.write_text(json.dumps(team))
        metadata = path.read_bytes()
        for relative in ("state/tasks.json", "state/agents.json"):
            (self.workspace / relative).unlink()
        (self.workspace / "state/lead.json").write_text("null")
        self.invoke("refresh", self.workspace)
        loaded = coven.load_workspace_state(self.workspace)
        self.assertEqual((loaded.tasks, loaded.agents, loaded.lead), ([], [], None))
        self.assertEqual(path.read_bytes(), metadata)
        self.assertEqual(coven.agent_display_name(loaded.team, "Jamal"), "Jamal")
        self.assertFalse((self.workspace / "state/tasks.json").exists())

    def test_invalid_new_task_titles_fail_without_mutation(self):
        before = snapshot(self.workspace)
        for title in (" ", "!!!", "_"):
            self.invoke("task", "add", self.workspace, title, error="Invalid task title")
            self.assertEqual(snapshot(self.workspace), before)

    def test_display_names_are_optional_presentation_not_aliases_or_paths(self):
        team = self.read_json("coven.json")
        self.assertEqual(team["display_names"]["jamal"], "Jamal")
        team["display_names"] = {"jamal": "Dr Oom / UI | <b>`name`\n\x1b[31m", "eric": "Same label", "clippy": "Same label"}
        (self.workspace / "coven.json").write_text(json.dumps(team))
        before = set(snapshot(self.workspace))
        self.invoke("refresh", self.workspace)
        dashboard = (self.workspace / "dashboard.md").read_text()
        self.assertIn("| jamal | Dr Oom / UI \\| &lt;b&gt;\\`name\\`", dashboard)
        self.assertNotIn("\x1b", dashboard)
        self.assertIn("\\\\u000a", dashboard)
        self.assertEqual(set(snapshot(self.workspace)), before)
        loaded = coven.load_team(self.workspace)
        self.assertEqual(coven.resolve_agent(loaded, "Jamal"), "jamal")
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            coven.resolve_agent(loaded, "Same label")
        self.invoke("task", "assign", self.workspace, "core-chatbot", "Jamal")
        self.assertEqual(next(t for t in self.read_json("state/tasks.json") if t["id"] == "core-chatbot")["owner"], "jamal")
        self.invoke("send", self.workspace, "jamal", "Continue")
        self.assertEqual(self.sender.call_args[0][1], "jamal")
        self.assertEqual(coven.agent_window_target(loaded, "jamal").split(":")[-1], "agent-jamal")
        launch = coven.agent_launch_command(loaded["name"], self.workspace, "jamal", "opencode")
        self.assertNotIn("Dr Oom", launch)
        self.assertIn("prompts/jamal.start.txt", launch)

    def test_bad_labels_and_proposed_lead_records_fail_before_writes(self):
        path = self.workspace / "coven.json"
        team = self.read_json("coven.json")
        for labels in ([], {"jamal": None}, {"jamal": " "}, {"../escape": "Name"}):
            path.write_text(json.dumps({**team, "display_names": labels}))
            before = snapshot(self.workspace)
            self.invoke("refresh", self.workspace, error="display_names")
            self.assertEqual(snapshot(self.workspace), before)
        path.write_text(json.dumps(team))
        before = snapshot(self.workspace)
        for lead in ({}, False, {"agent": "jamal", "until": []}):
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                coven.write_lead(self.workspace, lead)
            self.assertEqual(snapshot(self.workspace), before)

    def test_unicode_display_names_round_trip_through_initialization(self):
        target = self.root / "unicode-labels"
        self.invoke("init", target, "--blank", "--agents", "Alice 🚀,Dr Oom", "--name", "Team 🚀")
        loaded = coven.load_workspace_state(target)
        self.assertEqual(loaded.team["agents"], ["alice", "dr-oom"])
        self.assertEqual(loaded.team["display_names"], {"alice": "Alice 🚀", "dr-oom": "Dr Oom"})
        config = tomllib.loads((target / "coven.toml").read_text())
        self.assertEqual(config["agents"][0]["display_name"], "Alice 🚀")
        manifest = tomllib.loads((target / "tmux/coven.toml").read_text())
        self.assertEqual(manifest["name"], "Coven: Team 🚀")
        self.invoke("refresh", target)
        self.assertEqual(coven.load_team(target)["display_names"], loaded.team["display_names"])
        for value in ("Rocket 🚀", "DEL\x7f", "line\nquote\"slash\\"):
            self.assertEqual(tomllib.loads("value = " + coven.toml_string(value))["value"], value)

    def test_invalid_generated_toml_is_rejected_before_creating_workspace(self):
        for generator in ("team_toml", "tmux_manifest"):
            target = self.root / f"invalid-{generator}"
            before = snapshot(self.root)
            with self.subTest(generator=generator), mock.patch.object(coven, generator, return_value="invalid = ["):
                self.invoke("init", target, "--blank", error="Invalid initialization inputs")
            self.assertFalse(target.exists())
            self.assertEqual(snapshot(self.root), before)

    def test_label_rendering_in_standup_agent_and_lead_status_is_safe(self):
        team = self.read_json("coven.json")
        team["display_names"]["jamal"] = "Dr | <b>`UI`\n\x1b[31m"
        (self.workspace / "coven.json").write_text(json.dumps(team))
        (self.workspace / "state/lead.json").write_text('{"agent":"jamal"}')
        with mock.patch.object(coven, "agent_runtime_status", return_value=("stopped", None)):
            standup = self.invoke("standup", self.workspace)
            agent = self.invoke("agent", "status", self.workspace, "jamal")
            lead = self.invoke("lead", self.workspace)
        for output in (standup, agent, lead):
            self.assertNotIn("\x1b", output)
            self.assertIn("jamal", output)
            self.assertIn("u001b", output)
        self.assertIn("Dr \\| &lt;b&gt;\\`UI\\`", standup)
        self.assertIn("Dr | <b>`UI`\\u000a\\u001b[31m (jamal)", agent)
        self.assertIn("Dr | <b>`UI`\\u000a\\u001b[31m (jamal)", lead)
