"""Path confinement tests exercise CLI, helper, log metadata, and runtime boundaries."""

import contextlib
import io
import json
from unittest import mock

from runner import coven
from test_coven import CovenTestCase, snapshot


class PathSafetyTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace, "--blank")
        self.runtime = mock.patch.object(coven.subprocess, "run", side_effect=AssertionError("Unexpected runtime call"))
        self.runtime_mock = self.runtime.start()
        self.addCleanup(self.runtime.stop)

    def test_checkpoint_cli_rejects_unsafe_ids_without_side_effects(self):
        outside = self.root / "private.md"
        outside.write_text("DO NOT READ OR OVERWRITE")
        ids = ("../goal", "../../private", str(outside.with_suffix("")), "nested/id",
               "back\\slash", "with.dot", "two words", "\n", "é", "")
        with mock.patch("builtins.input", side_effect=AssertionError("Unexpected interactive prompt")):
            for checkpoint in ids:
                for command in ("approve", "suggest", "review"):
                    with self.subTest(checkpoint=checkpoint, command=command):
                        before = snapshot(self.root)
                        extra = ["Please change this"] if command == "suggest" else []
                        output = self.invoke(command, self.workspace, checkpoint, *extra, error="Invalid checkpoint ID")
                        self.assertNotIn("DO NOT READ", output)
                        self.assertEqual(snapshot(self.root), before)

    def test_checkpoint_helpers_validate_types_before_logging(self):
        team = coven.load_team(self.workspace)
        for checkpoint in (None, 0, False, [], {}, "../goal", "/absolute"):
            for helper in (coven.approve_checkpoint, coven.suggest_checkpoint_changes):
                with self.subTest(checkpoint=checkpoint, helper=helper.__name__):
                    before = snapshot(self.root)
                    with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                        helper(self.workspace, team, checkpoint, "User decision")
                    self.assertEqual(snapshot(self.root), before)

    def test_console_cannot_bypass_checkpoint_validation(self):
        for command in ("approve ../goal", "suggest ../goal Do not escape"):
            with self.subTest(command=command), mock.patch("builtins.input", return_value=command):
                before = snapshot(self.root)
                self.invoke("console", self.workspace, error="Invalid checkpoint ID")
                self.assertEqual(snapshot(self.root), before)

    def test_log_checkpoint_paths_fail_closed_before_refresh_or_review(self):
        for filename in ("events", "messages"):
            path = self.workspace / f"logs/{filename}.jsonl"
            original = path.read_text()
            for checkpoint in ("../goal", str(self.root / "outside"), None, {}, 23, ""):
                with self.subTest(filename=filename, checkpoint=checkpoint):
                    # Validate the entire history before acting on any entry.
                    # Refresh must not rewrite its manifest on invalid metadata.
                    records = [
                        {"type": "checkpoint.approved", "checkpoint": "safe-custom"},
                        {"type": "checkpoint.requested", "checkpoint": checkpoint},
                    ]
                    path.write_text(original + "".join(json.dumps(record) + "\n" for record in records))
                    before = snapshot(self.root)
                    for command in ("refresh", "next", "review"):
                        self.invoke(command, self.workspace, error="Invalid checkpoint ID")
                        self.assertEqual(snapshot(self.root), before)
            path.write_text(original)

    def test_direct_log_discovery_prevalidates_entire_batch(self):
        path = self.workspace / "logs/events.jsonl"
        with path.open("a") as handle:
            for checkpoint in ("safe-custom", "../goal"):
                handle.write(json.dumps({"type": "checkpoint.approved", "checkpoint": checkpoint}) + "\n")
        before = snapshot(self.root)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            coven.checkpoint_log_records(self.workspace, "events")
        self.assertEqual(snapshot(self.root), before)

    @contextlib.contextmanager
    def managed_link(self, relative, kind):
        path = self.workspace / relative
        backup = self.root / "saved-managed-entry"
        existed = path.exists()
        if existed:
            path.rename(backup)
        is_directory = relative in coven.MANAGED_DIRECTORIES
        target = (self.workspace if kind == "inward" else self.root) / "link-target"
        if kind != "dangling":
            if is_directory:
                target.mkdir()
                (target / "sentinel").write_text("Private data")
            else:
                target.write_text("Private data")
        path.symlink_to(target, target_is_directory=is_directory)
        try:
            yield
        finally:
            path.unlink()
            if existed:
                backup.rename(path)
            if target.is_dir():
                (target / "sentinel").unlink()
                target.rmdir()
            elif target.exists():
                target.unlink()

    def test_managed_symlink_matrix_blocks_before_any_side_effect(self):
        files = coven.MANAGED_FILES + (
            "agents/jamal.md", "prompts/eric.md", "prompts/eric.start.txt",
            "checkpoints/plan-approval.md", "checkpoints/custom.md",
        )
        for relative in coven.MANAGED_DIRECTORIES + files:
            for kind in ("outward", "inward", "dangling"):
                with self.subTest(relative=relative, kind=kind), self.managed_link(relative, kind):
                    before = snapshot(self.root)
                    for command in ("refresh", "bootstrap", "status", "restart", "approve"):
                        self.invoke(command, self.workspace, error="symlinks are not supported")
                        self.assertEqual(snapshot(self.root), before)
        self.runtime_mock.assert_not_called()

    def test_wrong_managed_types_are_rejected(self):
        for relative in ("logs", "prompts", "dashboard.md", "state/tasks.json", "checkpoints/custom.md"):
            path = self.workspace / relative
            backup = self.root / "saved-managed-entry"
            existed = path.exists()
            if existed:
                path.rename(backup)
            directory = relative in coven.MANAGED_DIRECTORIES
            if directory:
                path.write_text("Not a directory")
            else:
                path.mkdir()
            try:
                before = snapshot(self.root)
                self.invoke("refresh", self.workspace, error="Managed path must be")
                self.assertEqual(snapshot(self.root), before)
            finally:
                path.unlink() if directory else path.rmdir()
                if existed:
                    backup.rename(path)

    def test_discovered_checkpoint_names_are_validated_before_read(self):
        path = self.workspace / "checkpoints/not.a.valid.id.md"
        path.write_text("Do not display these contents")
        before = snapshot(self.root)
        output = self.invoke("review", self.workspace, error="Invalid checkpoint ID")
        self.assertNotIn("Do not display", output)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            coven.checkpoint_review_candidates(self.workspace)
        self.assertEqual(snapshot(self.root), before)

    def test_invalid_persisted_rosters_block_refresh_bootstrap_and_launch(self):
        path = self.workspace / "coven.json"
        team = self.read_json("coven.json")
        rosters = (["jamal", "../escape"], ["jamal", "/absolute"], ["jamal", "a.b"],
                   ["jamal", "x'; touch bad"], ["jamal", {}], ["jamal", "jamal"], [], "jamal")
        for roster in rosters:
            with self.subTest(roster=roster):
                path.write_text(json.dumps({**team, "agents": roster}))
                before = snapshot(self.root)
                for command in ("refresh", "bootstrap", "restart"):
                    self.invoke(command, self.workspace, error="ID")
                    self.assertEqual(snapshot(self.root), before)
        self.runtime_mock.assert_not_called()

    def test_malformed_configuration_is_a_clean_error_before_writes(self):
        path = self.workspace / "coven.json"
        for content in ("invalid JSON", "[]", '{"name": 1}', '{"name": "ok", "agents": null}'):
            with self.subTest(content=content):
                path.write_text(content)
                before = snapshot(self.root)
                self.invoke("refresh", self.workspace, error="[ERROR]")
                self.assertEqual(snapshot(self.root), before)

    def test_legacy_safe_ids_refresh_without_renaming(self):
        team = self.read_json("coven.json")
        team["agents"] = ["Jamal", "_"]
        (self.workspace / "coven.json").write_text(json.dumps(team))
        self.invoke("refresh", self.workspace)
        for agent in ("Jamal", "_"):
            self.assertTrue((self.workspace / f"prompts/{agent}.start.txt").is_file())
        self.assertEqual(self.read_json("coven.json")["agents"], ["Jamal", "_"])

    def test_legacy_safe_ids_bootstrap_through_both_mocked_runtimes(self):
        team = self.read_json("coven.json")
        team["agents"] = ["Jamal", "_"]
        (self.workspace / "coven.json").write_text(json.dumps(team))
        self.invoke("refresh", self.workspace)
        self.runtime_mock.side_effect = None
        self.runtime_mock.return_value = mock.Mock(returncode=0, stdout="%1\n", stderr="")
        for multiplexer in ("herdr", "tmux"):
            with self.subTest(multiplexer=multiplexer), mock.patch.dict(coven.os.environ, {"COVEN_MULTIPLEXER": multiplexer}):
                self.runtime_mock.reset_mock()
                with mock.patch.object(coven, "running_agent_pane", return_value="pane-1"):
                    self.invoke("bootstrap", self.workspace)
                calls = str(self.runtime_mock.call_args_list)
                for agent in ("Jamal", "_"):
                    self.assertIn(f"prompts/{agent}.start.txt" if multiplexer == "tmux" else f"begin as `{agent}`", calls)
                    launch = coven.agent_launch_command(team["name"], self.workspace, agent, "opencode", multiplexer)
                    self.assertIn(f"prompts/{agent}.start.txt", launch)

    def test_review_and_agent_batches_validate_all_targets_first(self):
        before = snapshot(self.root)
        with mock.patch("builtins.input", side_effect=AssertionError("Unexpected interactive prompt")):
            self.invoke("review", self.workspace, "plan-approval", "../goal", error="Invalid checkpoint ID")
        for operation in ("start", "stop", "restart"):
            self.invoke("agent", operation, self.workspace, "jamal", "ghost", error="Unknown agent")
        self.invoke("approve", self.workspace, "custom", "--agent", "ghost", error="Unknown agent")
        self.assertEqual(snapshot(self.root), before)
        self.runtime_mock.assert_not_called()

    def test_bootstrap_late_bad_target_does_not_create_earlier_missing_prompt(self):
        earlier = self.workspace / "prompts/clippy.start.txt"
        earlier.unlink()
        with self.managed_link("prompts/eric.start.txt", "dangling"):
            before = snapshot(self.root)
            self.invoke("bootstrap", self.workspace, error="symlinks are not supported")
            self.assertEqual(snapshot(self.root), before)
            self.assertFalse(earlier.exists())

    def test_invalid_lead_id_blocks_mutations_and_runtime_actions(self):
        team = coven.load_team(self.workspace)
        (self.workspace / "state/lead.json").write_text('{"agent": "../escape"}')
        before = snapshot(self.root)
        commands = [
            ("refresh", self.workspace), ("approve", self.workspace),
            ("suggest", self.workspace, "custom", "Please change"),
            *(("agent", operation, self.workspace, "jamal") for operation in ("start", "stop", "restart")),
        ]
        for command in commands:
            self.invoke(*command, error="Invalid agent ID")
            self.assertEqual(snapshot(self.root), before)
        for helper in (coven.approve_checkpoint, coven.suggest_checkpoint_changes):
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                helper(self.workspace, team, "custom", "User decision")
            self.assertEqual(snapshot(self.root), before)
        self.runtime_mock.assert_not_called()

    def test_undecodable_checkpoint_blocks_approval_refresh_and_review_batches(self):
        (self.workspace / "checkpoints/zzz.md").write_bytes(b"\xff")
        with (self.workspace / "logs/events.jsonl").open("a") as handle:
            for checkpoint in ("aaa", "zzz"):
                handle.write(json.dumps({"type": "checkpoint.approved", "checkpoint": checkpoint}) + "\n")
        before = snapshot(self.root)
        with mock.patch("builtins.input", side_effect=AssertionError("Unexpected interactive prompt")):
            for command in (
                ("approve", self.workspace, "zzz"), ("refresh", self.workspace),
                ("review", self.workspace, "plan-approval", "zzz"),
            ):
                self.invoke(*command, error="Cannot read managed file")
                self.assertEqual(snapshot(self.root), before)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            coven.checkpoint_log_records(self.workspace, "events")
        self.assertEqual(snapshot(self.root), before)
        self.runtime_mock.assert_not_called()

    def test_late_undecodable_bootstrap_prompt_blocks_earlier_creation(self):
        (self.workspace / "prompts/clippy.start.txt").unlink()
        (self.workspace / "prompts/eric.start.txt").write_bytes(b"\xff")
        before = snapshot(self.root)
        self.invoke("bootstrap", self.workspace, error="Cannot read managed file")
        self.assertEqual(snapshot(self.root), before)
        self.runtime_mock.assert_not_called()

    def test_uppercase_checkpoint_extension_cannot_bypass_readability_checks(self):
        (self.workspace / "checkpoints/custom.MD").write_bytes(b"\xff")
        before = snapshot(self.root)
        for command in (("approve", self.workspace, "custom"), ("refresh", self.workspace)):
            self.invoke(*command, error="Cannot read managed file")
            self.assertEqual(snapshot(self.root), before)
        self.runtime_mock.assert_not_called()

    def test_direct_runtime_helpers_validate_before_restarting(self):
        team = coven.load_team(self.workspace)
        with self.managed_link("prompts/eric.start.txt", "outward"):
            before = snapshot(self.root)
            for helper in (coven.run_coven_tmux, coven.run_coven_herdr):
                with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                    helper(self.workspace, team, "restart")
                self.assertEqual(snapshot(self.root), before)
        self.runtime_mock.assert_not_called()

    def test_shared_checkpoint_helpers_also_guard_static_log_paths(self):
        team = coven.load_team(self.workspace)
        for relative in ("logs/events.jsonl", "logs/messages.jsonl", "dashboard.md"):
            with self.subTest(relative=relative), self.managed_link(relative, "outward"):
                before = snapshot(self.root)
                for helper in (coven.approve_checkpoint, coven.suggest_checkpoint_changes):
                    with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                        helper(self.workspace, team, "custom", "User decision")
                    self.assertEqual(snapshot(self.root), before)

    def test_safe_custom_checkpoint_can_be_reviewed_but_not_restored_from_history(self):
        with mock.patch.object(coven, "send_to_agent_pane", return_value=False):
            self.invoke("approve", self.workspace, "Custom_1")
            self.assertEqual(coven.checkpoint_status(self.workspace, "Custom_1"), "approved")
            self.invoke("suggest", self.workspace, "new-checkpoint", "Please add evidence")
            with mock.patch("builtins.input", return_value="a"):
                self.invoke("review", self.workspace, "new-checkpoint")
        (self.workspace / "checkpoints/Custom_1.md").unlink()
        events = (self.workspace / "logs/events.jsonl").read_bytes()
        self.invoke("refresh", self.workspace)
        self.assertEqual(coven.checkpoint_status(self.workspace, "Custom_1"), "missing")
        self.assertEqual((self.workspace / "logs/events.jsonl").read_bytes(), events)

    def test_workspace_root_alias_and_unrelated_artifact_links_are_allowed(self):
        outside = self.root / "artifact"
        outside.write_text("User artifact")
        (self.workspace / "work/user-artifact").symlink_to(outside)
        alias = self.root / "workspace-alias"
        alias.symlink_to(self.workspace, target_is_directory=True)
        self.invoke("refresh", alias)
        self.assertTrue((self.workspace / "work/user-artifact").is_symlink())
        self.assertEqual(outside.read_text(), "User artifact")
